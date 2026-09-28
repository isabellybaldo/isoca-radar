//! Event-affine shard planning (spec §7, Phase 0 Q7e). A unit (NegRisk group or canary pair) is
//! never split. Hot units (top `hot_events` by message rate) share shards of ≤ hot_shard_tokens,
//! the rest ≤ cold_shard_tokens. At most max_connections shards: once the budget is used, the
//! lowest-priority units are dropped and reported (health.dropped_groups). A unit above
//! MAX_UNIT_TOKENS can't be subscribed safely at all and is never planned (integrity
//! `unit_oversize`). Between retiers, `replan` keeps every unit on its connection (S3a).
use crate::config::ShardsCfg;
use crate::universe::Universe;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub key: String,
    pub tokens: Vec<String>,
    pub rate: u64,
    pub canary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shard {
    pub hot: bool,
    pub units: Vec<String>,
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub shards: Vec<Shard>,
    pub dropped: Vec<String>,
    pub dropped_tokens: usize,
    /// Units above MAX_UNIT_TOKENS as (key, tokens), sorted by key: never subscribed.
    pub oversize: Vec<(String, usize)>,
}

impl Plan {
    pub fn conn_of(&self) -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for (i, s) in self.shards.iter().enumerate() {
            for u in &s.units {
                m.insert(u.clone(), i);
            }
        }
        m
    }

    pub fn tokens(&self) -> usize {
        self.shards.iter().map(|s| s.tokens.len()).sum()
    }

    /// Shards that carry a connection (an idle slot has no tokens).
    pub fn live_shards(&self) -> usize {
        self.shards.iter().filter(|s| !s.tokens.is_empty()).count()
    }

    /// Oversize units not already oversize in `prev`: each is reported once, not every refresh.
    pub fn new_oversize(&self, prev: &Plan) -> Vec<(String, usize)> {
        self.oversize
            .iter()
            .filter(|(k, _)| !prev.oversize.iter().any(|(pk, _)| pk == k))
            .cloned()
            .collect()
    }
}

pub use tape::universe_view::canary_key;

pub fn units(u: &Universe, rates: &HashMap<String, u64>) -> Vec<Unit> {
    let mut out = Vec::new();
    for g in &u.groups {
        let tokens = g.live_yes_tokens();
        if tokens.is_empty() {
            continue;
        }
        out.push(Unit {
            key: g.event_id.clone(),
            tokens,
            rate: rates.get(&g.event_id).copied().unwrap_or(0),
            canary: false,
        });
    }
    for c in &u.canary {
        let key = canary_key(&c.market_id);
        out.push(Unit {
            rate: rates.get(&key).copied().unwrap_or(0),
            key,
            tokens: vec![c.yes_token.clone(), c.no_token.clone()],
            canary: true,
        });
    }
    out
}

/// Phase 0: 5,000 tokens on one connection failed (p99 lag 15.7 s); S2c runs ≤ 1,000. A unit is
/// never split, so a unit above this has no safe connection and is left out of the plan.
pub const MAX_UNIT_TOKENS: usize = 1000;

/// Priority: canaries (integrity) → message rate → size (bigger groups carry more baskets) → key.
fn sort_units(units: &mut [Unit]) {
    units.sort_by(|a, b| {
        b.canary
            .cmp(&a.canary)
            .then(b.rate.cmp(&a.rate))
            .then(b.tokens.len().cmp(&a.tokens.len()))
            .then(a.key.cmp(&b.key))
    });
}

/// Move units above MAX_UNIT_TOKENS into `p.oversize`; return the rest.
fn split_oversize(units: Vec<Unit>, p: &mut Plan) -> Vec<Unit> {
    let (ok, big): (Vec<Unit>, Vec<Unit>) = units
        .into_iter()
        .partition(|u| u.tokens.len() <= MAX_UNIT_TOKENS);
    p.oversize = big.into_iter().map(|u| (u.key, u.tokens.len())).collect();
    p.oversize.sort();
    ok
}

/// First fit into a non-empty shard of the same tier with room, else an idle slot, else a new
/// shard while the connection budget allows; otherwise the unit is dropped and counted.
fn place(p: &mut Plan, u: Unit, is_hot: bool, cfg: &ShardsCfg) {
    let cap = if is_hot {
        cfg.hot_shard_tokens
    } else {
        cfg.cold_shard_tokens
    };
    let slot = p
        .shards
        .iter()
        .position(|s| {
            !s.tokens.is_empty() && s.hot == is_hot && s.tokens.len() + u.tokens.len() <= cap
        })
        .or_else(|| p.shards.iter().position(|s| s.tokens.is_empty()));
    let idx = match slot {
        Some(i) => i,
        None if p.shards.len() < cfg.max_connections => {
            p.shards.push(Shard {
                hot: is_hot,
                units: Vec::new(),
                tokens: Vec::new(),
            });
            p.shards.len() - 1
        }
        None => {
            p.dropped_tokens += u.tokens.len();
            p.dropped.push(u.key);
            return;
        }
    };
    let s = &mut p.shards[idx];
    if s.tokens.is_empty() {
        s.hot = is_hot;
    }
    s.units.push(u.key);
    s.tokens.extend(u.tokens);
}

pub fn plan(units: Vec<Unit>, cfg: &ShardsCfg) -> Plan {
    let mut p = Plan::default();
    let mut units = split_oversize(units, &mut p);
    sort_units(&mut units);
    let hot: HashSet<String> = units
        .iter()
        .filter(|u| !u.canary && u.rate > 0)
        .take(cfg.hot_events)
        .map(|u| u.key.clone())
        .collect();
    for u in units {
        let is_hot = hot.contains(&u.key);
        place(&mut p, u, is_hot, cfg);
    }
    p
}

/// Sticky re-plan for a universe refresh (S3a): without `retier`, every unit stays on its
/// previous connection while it still fits there, so only shards whose own units changed get a
/// new token list (and a reconnect). A shard emptied by departures stays as an idle cold slot
/// so later shards keep their index. New, grown and previously dropped units are placed cold
/// (tiers only change on a retier). `retier` (or no previous plan) is a fresh `plan`.
pub fn replan(prev: &Plan, units: Vec<Unit>, cfg: &ShardsCfg, retier: bool) -> Plan {
    if retier || prev.shards.is_empty() {
        return plan(units, cfg);
    }
    let mut p = Plan::default();
    let mut rest: HashMap<String, Unit> = split_oversize(units, &mut p)
        .into_iter()
        .map(|u| (u.key.clone(), u))
        .collect();
    for s in &prev.shards {
        let cap = if s.hot {
            cfg.hot_shard_tokens
        } else {
            cfg.cold_shard_tokens
        };
        let mut ns = Shard {
            hot: s.hot,
            units: Vec::new(),
            tokens: Vec::new(),
        };
        for key in &s.units {
            let fits = rest
                .get(key)
                .is_some_and(|u| ns.units.is_empty() || ns.tokens.len() + u.tokens.len() <= cap);
            if fits && let Some(u) = rest.remove(key) {
                ns.units.push(u.key);
                ns.tokens.extend(u.tokens);
            }
        }
        if ns.tokens.is_empty() {
            ns.hot = false; // an idle slot takes any unit
        }
        p.shards.push(ns);
    }
    let mut left: Vec<Unit> = rest.into_values().collect();
    sort_units(&mut left);
    for u in left {
        place(&mut p, u, false, cfg);
    }
    while p.shards.last().is_some_and(|s| s.tokens.is_empty()) {
        p.shards.pop();
    }
    p
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn cfg(max_connections: usize) -> ShardsCfg {
        ShardsCfg {
            max_connections,
            hot_events: 2,
            hot_shard_tokens: 5,
            cold_shard_tokens: 10,
            retier_every_refreshes: 6,
            per_shard_capacity: 16,
        }
    }

    fn unit(key: &str, n: usize, rate: u64) -> Unit {
        Unit {
            key: key.into(),
            tokens: (0..n).map(|i| format!("{key}-{i}")).collect(),
            rate,
            canary: false,
        }
    }

    fn cold(key: &str, n: usize) -> Unit {
        unit(key, n, 0)
    }

    #[test]
    fn never_splits_a_unit_and_respects_tier_caps() {
        let us = vec![
            unit("a", 4, 9),
            unit("b", 3, 8),
            unit("c", 6, 0),
            unit("d", 6, 0),
            unit("big", 14, 0),
        ];
        let p = plan(us, &cfg(40));
        for s in &p.shards {
            // every unit's tokens are contiguous in exactly one shard
            let cap = if s.hot { 5 } else { 10 };
            assert!(s.tokens.len() <= cap || s.units.len() == 1, "{s:?}");
        }
        let conn = p.conn_of();
        assert_eq!(conn.len(), 5);
        // a and b are hot (top-2 rate > 0): 4 + 3 > 5, so they sit in two hot shards
        assert!(p.shards[conn["a"]].hot && p.shards[conn["b"]].hot);
        assert_ne!(conn["a"], conn["b"]);
        // the oversize unit gets a shard of its own
        assert_eq!(p.shards[conn["big"]].units, vec!["big".to_string()]);
        assert!(p.dropped.is_empty());
        assert_eq!(p.tokens(), 4 + 3 + 6 + 6 + 14);
    }

    #[test]
    fn budget_drops_the_lowest_priority_units_and_keeps_canaries() {
        let mut us = vec![
            unit("hot", 5, 100),
            unit("warm", 10, 1),
            unit("cold1", 10, 0),
            unit("cold2", 10, 0),
        ];
        us.push(Unit {
            key: canary_key("m9"),
            tokens: vec!["y".into(), "n".into()],
            rate: 0,
            canary: true,
        });
        let p = plan(us, &cfg(3));
        assert_eq!(p.shards.len(), 3);
        let conn = p.conn_of();
        assert!(conn.contains_key("canary:m9"));
        assert!(conn.contains_key("hot"));
        // canary -> shard 0 (cold, 2 tokens); hot and warm are the top-2 rated units, so both are hot:
        // hot -> shard 1, warm (10 > hot cap 5) -> shard 2 on its own. The budget of 3 is used, so
        // both cold units are dropped; canaries and hot units always survive.
        assert_eq!(p.dropped, vec!["cold1".to_string(), "cold2".to_string()]);
        assert_eq!(p.dropped_tokens, 20);
    }

    #[test]
    fn planning_is_deterministic() {
        let us = || vec![unit("x", 3, 0), unit("y", 3, 0), unit("z", 7, 2)];
        assert_eq!(plan(us(), &cfg(40)), plan(us(), &cfg(40)));
    }

    #[test]
    fn an_unchanged_universe_replans_to_the_identical_plan() {
        let us = || vec![unit("a", 4, 9), cold("c", 6), cold("d", 6), cold("e", 3)];
        let p = plan(us(), &cfg(40));
        // a hot → 0; c → 1; d (6 + 6 > 10) → 2; e → 1
        assert_eq!(p.conn_of()["e"], 1);
        assert_eq!(replan(&p, us(), &cfg(40), false), p);
    }

    #[test]
    fn a_refresh_only_touches_the_shards_whose_units_changed() {
        let p = plan(
            vec![unit("a", 4, 9), cold("c", 6), cold("d", 6), cold("e", 3)],
            &cfg(40),
        );
        // c leaves, f (2 tokens) arrives
        let q = replan(
            &p,
            vec![unit("a", 4, 9), cold("d", 6), cold("e", 3), cold("f", 2)],
            &cfg(40),
            false,
        );
        assert_eq!(q.shards.len(), p.shards.len());
        assert_eq!(q.shards[0], p.shards[0], "a's shard is untouched");
        assert_eq!(q.shards[2], p.shards[2], "d's shard is untouched");
        assert_eq!(q.shards[1].units, vec!["e".to_string(), "f".to_string()]);
    }

    #[test]
    fn a_shard_whose_units_all_left_stays_an_idle_slot_so_later_shards_keep_their_index() {
        let p = plan(vec![cold("c", 6), cold("d", 6), cold("g", 6)], &cfg(40)); // c 0, d 1, g 2
        let q = replan(&p, vec![cold("c", 6), cold("g", 6)], &cfg(40), false);
        assert_eq!(q.shards.len(), 3);
        assert!(q.shards[1].tokens.is_empty() && q.shards[1].units.is_empty());
        assert_eq!(q.conn_of()["g"], 2, "g did not move");
        assert_eq!(q.live_shards(), 2);
        // the idle slot is where the next new unit goes
        let r = replan(
            &q,
            vec![cold("c", 6), cold("g", 6), cold("h", 8)],
            &cfg(40),
            false,
        );
        assert_eq!(r.conn_of()["h"], 1);
        // trailing idle slots are released
        let s = replan(&q, vec![cold("c", 6)], &cfg(40), false);
        assert_eq!(s.shards.len(), 1);
    }

    #[test]
    fn a_unit_that_outgrows_its_shard_moves_and_the_rest_stay() {
        let p = plan(vec![cold("c", 6), cold("e", 3)], &cfg(40)); // one shard [c, e] = 9
        let q = replan(&p, vec![cold("c", 6), cold("e", 5)], &cfg(40), false);
        assert_eq!(q.conn_of()["c"], 0);
        assert_eq!(q.conn_of()["e"], 1, "6 + 5 > 10: e moves to a new shard");
    }

    #[test]
    fn a_retier_is_a_fresh_plan() {
        let p = plan(vec![cold("c", 6), cold("d", 6)], &cfg(40));
        let us = || vec![unit("c", 6, 5), cold("d", 6), cold("x", 1)];
        assert_eq!(replan(&p, us(), &cfg(40), true), plan(us(), &cfg(40)));
    }

    #[test]
    fn a_unit_over_the_phase0_connection_limit_is_never_planned_and_is_reported_once() {
        let big = |n: usize| unit("big", n, 50);
        let p = plan(vec![big(MAX_UNIT_TOKENS + 1), cold("c", 6)], &cfg(40));
        assert!(!p.conn_of().contains_key("big"));
        assert!(p.shards.iter().all(|s| s.tokens.len() <= MAX_UNIT_TOKENS));
        assert_eq!(p.oversize, vec![("big".to_string(), MAX_UNIT_TOKENS + 1)]);
        assert_eq!(
            p.new_oversize(&Plan::default()),
            p.oversize,
            "first sighting is reported"
        );
        let q = replan(
            &p,
            vec![big(MAX_UNIT_TOKENS + 5), cold("c", 6)],
            &cfg(40),
            false,
        );
        assert!(
            q.new_oversize(&p).is_empty(),
            "still oversize: not reported again"
        );
        // shrinks back to the limit: subscribed again at the next refresh
        let r = replan(
            &q,
            vec![big(MAX_UNIT_TOKENS), cold("c", 6)],
            &cfg(40),
            false,
        );
        assert!(r.conn_of().contains_key("big") && r.oversize.is_empty());
    }
}
