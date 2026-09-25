//! Event-affine shard planning (spec §7, Phase 0 Q7e). A unit (NegRisk group or canary pair) is
//! never split. Hot units (top `hot_events` by message rate) share shards of ≤ hot_shard_tokens,
//! the rest ≤ cold_shard_tokens. At most max_connections shards: once the budget is used, the
//! lowest-priority units are dropped and reported (health.dropped_groups).
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
}

pub fn canary_key(market_id: &str) -> String {
    format!("canary:{market_id}")
}

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

pub fn plan(mut units: Vec<Unit>, cfg: &ShardsCfg) -> Plan {
    // Priority: canaries (integrity) → message rate → size (bigger groups carry more baskets) → key.
    units.sort_by(|a, b| {
        b.canary
            .cmp(&a.canary)
            .then(b.rate.cmp(&a.rate))
            .then(b.tokens.len().cmp(&a.tokens.len()))
            .then(a.key.cmp(&b.key))
    });
    let hot: HashSet<String> = units
        .iter()
        .filter(|u| !u.canary && u.rate > 0)
        .take(cfg.hot_events)
        .map(|u| u.key.clone())
        .collect();
    let mut p = Plan::default();
    for u in units {
        let is_hot = hot.contains(&u.key);
        let cap = if is_hot {
            cfg.hot_shard_tokens
        } else {
            cfg.cold_shard_tokens
        };
        let slot = p
            .shards
            .iter()
            .position(|s| s.hot == is_hot && s.tokens.len() + u.tokens.len() <= cap);
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
                continue;
            }
        };
        p.shards[idx].units.push(u.key);
        p.shards[idx].tokens.extend(u.tokens);
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
}
