//! The engine (spec §8.2): single owner of every book and the episode tracker. No IO, no clock:
//! time arrives on the inputs, so live and replay produce identical outputs (spec §9.3).
use crate::universe::{GroupMeta, Status, Universe};
use arb_core::book::Book;
use arb_core::detect::{
    Group, GroupLeg, Kind, Realization, Signal, detect_no_basket, detect_yes_basket,
};
use arb_core::episode::{Censor, EpisodeEvent, EpisodeTracker};
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use pm_api::ws::MarketEvent;
use rust_decimal::Decimal;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use store::{EpisodeLegRow, EpisodeRow, EpisodeWrite, IntegrityRow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCtx {
    pub run_id: i64,
    pub source: String,
    pub scanner_version: String,
    pub config_hash: String,
    pub gap_ms: i64,
    pub stale_lag_ms: i64,
    pub suspect_window_ms: i64,
    pub l1_suspect_window_ms: i64,
    pub drift_settle_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Det {
    Yes,
    No,
}

impl Det {
    pub fn of_kind(k: Kind) -> Det {
        match k {
            Kind::NoBasket => Det::No,
            _ => Det::Yes,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Det::Yes => "yes",
            Det::No => "no",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecheckReq {
    pub episode_id: String,
    pub event_id: String,
    pub det: Det,
    pub opened_ms: i64,
    pub q_max: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Episode(Box<EpisodeWrite>),
    Integrity(IntegrityRow),
    Resnapshot {
        token: String,
    },
    Recheck(RecheckReq),
    UniverseChanged,
    Lag(i64),
    Drift {
        token: String,
        event_id: Option<String>,
        detail: String,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Issue {
    pub kind: &'static str,
    pub shard: Option<usize>,
    pub token: Option<String>,
    pub event_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
struct Snap {
    signal: Signal,
    budget100: Decimal,
    best: Vec<(Option<Price>, Option<Shares>)>,
    leg_fees: Vec<Decimal>,
    ts_ms: i64,
}

#[derive(Debug, Clone)]
struct OpenEp {
    episode_id: String,
    key: String,
    group: usize,
    det: Det,
    opened_ms: i64,
    open_lag_ms: i64,
    stale: bool,
    suspect: bool,
    n_updates: i32,
    /// From `EpisodeTracker::open_censor` at open time (spec censor_left).
    censor_left: bool,
    peak: Snap,
}

#[derive(Debug, Clone, Copy)]
struct At {
    g: usize,
    det: Det,
    ts: i64,
    recv_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingDrift {
    pub token: String,
    pub rest: Book,
    pub due_ms: i64,
}

pub struct Engine {
    ctx: EngineCtx,
    universe: Arc<Universe>,
    groups: Vec<Group>,
    token_ix: HashMap<String, (usize, usize)>,
    canary: HashMap<String, Book>,
    group_conn: Vec<Option<usize>>,
    tracker: EpisodeTracker,
    open: HashMap<u64, OpenEp>,
    open_by_key: HashMap<String, u64>,
    suspect_until: Vec<i64>,
    msgs: HashMap<String, u64>,
    pending_drift: Vec<PendingDrift>,
}

fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::YesBasket => "yes_basket",
        Kind::YesBasketIncomplete => "yes_basket_incomplete",
        Kind::NoBasket => "no_basket",
        Kind::Implication => "implication",
    }
}

fn realization_str(r: Realization) -> &'static str {
    match r {
        Realization::Settlement => "settlement",
        Realization::Converter => "converter",
        Realization::NotArbitrage => "not_arbitrage",
    }
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

fn bump(ts: &mut Option<i64>, t: i64) {
    *ts = Some(ts.map_or(t, |x| x.max(t)));
}

/// Conservative fee of buying `filled` shares on `leg`, rounded up per level (Q3).
fn leg_fee(leg: &BuyLeg, filled: Decimal) -> Decimal {
    let mut left = filled;
    let mut fee = Decimal::ZERO;
    for &(p, q) in &leg.levels {
        if left <= Decimal::ZERO {
            break;
        }
        let take = left.min(q.get());
        fee += leg.fee.fee_conservative(p, take);
        left -= take;
    }
    fee
}

impl Engine {
    pub fn new(ctx: EngineCtx) -> Engine {
        let gap_ms = ctx.gap_ms;
        Engine {
            ctx,
            universe: Arc::new(Universe {
                taken_ms: 0,
                groups: Vec::new(),
                canary: Vec::new(),
            }),
            groups: Vec::new(),
            token_ix: HashMap::new(),
            canary: HashMap::new(),
            group_conn: Vec::new(),
            tracker: EpisodeTracker::new(gap_ms),
            open: HashMap::new(),
            open_by_key: HashMap::new(),
            suspect_until: Vec::new(),
            msgs: HashMap::new(),
            pending_drift: Vec::new(),
        }
    }

    pub fn open_episodes(&self) -> usize {
        self.open.len()
    }

    fn key(meta: &GroupMeta, det: Det) -> String {
        format!("{}:{}", meta.event_id, det.tag())
    }

    pub(crate) fn integrity(&self, ts_ms: i64, i: Issue) -> Out {
        Out::Integrity(IntegrityRow {
            run_id: self.ctx.run_id,
            ts_ms,
            kind: i.kind.to_string(),
            shard: i.shard.map(to_i32),
            token: i.token,
            event_id: i.event_id,
            detail: i.detail,
        })
    }

    pub(crate) fn suspect(&mut self, g: usize, until_ms: i64) {
        if let Some(t) = self.suspect_until.get_mut(g) {
            *t = (*t).max(until_ms);
        }
    }

    pub fn apply_universe(
        &mut self,
        u: Arc<Universe>,
        conn_of: &HashMap<String, usize>,
        ts_ms: i64,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        // 1. episodes of events that left the universe: right-censor them (rows built from the OLD universe)
        let keep: HashSet<&str> = u.groups.iter().map(|g| g.event_id.as_str()).collect();
        let mut gone: Vec<(u64, String)> = self
            .open
            .iter()
            .filter(|(_, ep)| !keep.contains(self.universe.groups[ep.group].event_id.as_str()))
            .map(|(id, ep)| (*id, ep.key.clone()))
            .collect();
        gone.sort();
        for (_, key) in gone {
            for e in self.tracker.on_disconnect(&key, ts_ms) {
                self.close_episode(e, &mut out);
            }
        }
        // 2. carry books (by token), suspect windows and open episodes (by event id)
        let mut old_books: HashMap<String, Book> = HashMap::new();
        for (g, meta) in self.universe.groups.iter().enumerate() {
            for (l, leg) in meta.legs.iter().enumerate() {
                if !leg.yes_token.is_empty() {
                    old_books.insert(
                        leg.yes_token.clone(),
                        std::mem::take(&mut self.groups[g].legs[l].book),
                    );
                }
            }
        }
        let old_suspect: HashMap<String, i64> = self
            .universe
            .groups
            .iter()
            .zip(&self.suspect_until)
            .map(|(m, &t)| (m.event_id.clone(), t))
            .collect();
        let open_events: Vec<(u64, String)> = self
            .open
            .iter()
            .map(|(id, ep)| (*id, self.universe.groups[ep.group].event_id.clone()))
            .collect();

        let mut index: HashMap<String, usize> = HashMap::new();
        self.groups = Vec::with_capacity(u.groups.len());
        self.token_ix.clear();
        for (g, meta) in u.groups.iter().enumerate() {
            index.insert(meta.event_id.clone(), g);
            let legs = meta
                .legs
                .iter()
                .enumerate()
                .map(|(l, leg)| {
                    if leg.status == Status::Live && !leg.yes_token.is_empty() {
                        self.token_ix.insert(leg.yes_token.clone(), (g, l));
                    }
                    GroupLeg {
                        status: leg.status.to_core(),
                        book: old_books.remove(&leg.yes_token).unwrap_or_default(),
                        fee: leg.fee(),
                    }
                })
                .collect();
            self.groups.push(Group {
                legs,
                augmented: meta.augmented,
            });
        }
        let mut canary = HashMap::new();
        for c in &u.canary {
            for t in [&c.yes_token, &c.no_token] {
                let b = self.canary.remove(t).unwrap_or_default();
                canary.insert(t.clone(), b);
            }
        }
        self.canary = canary;
        self.group_conn = u
            .groups
            .iter()
            .map(|m| conn_of.get(&m.event_id).copied())
            .collect();
        self.suspect_until = u
            .groups
            .iter()
            .map(|m| old_suspect.get(&m.event_id).copied().unwrap_or(i64::MIN))
            .collect();
        for (id, ev) in open_events {
            if let (Some(&g), Some(ep)) = (index.get(&ev), self.open.get_mut(&id)) {
                ep.group = g;
            }
        }
        self.universe = u;
        out
    }

    pub fn on_frame(&mut self, conn: usize, recv_ms: i64, events: &[MarketEvent]) -> Vec<Out> {
        let mut out = Vec::new();
        let mut touched: BTreeSet<usize> = BTreeSet::new();
        let mut frame_ts: Option<i64> = None;
        for ev in events {
            match ev {
                MarketEvent::Snapshot {
                    token,
                    server_ts_ms,
                    bids,
                    asks,
                    ..
                } => {
                    bump(&mut frame_ts, *server_ts_ms);
                    let ts = *server_ts_ms;
                    self.on_book(conn, token, ts, &mut touched, &mut out, |b| {
                        b.apply_snapshot(bids, asks, ts);
                        None
                    });
                }
                MarketEvent::Delta {
                    token,
                    server_ts_ms,
                    side,
                    price,
                    size,
                    best_bid,
                    best_ask,
                } => {
                    bump(&mut frame_ts, *server_ts_ms);
                    // Polymarket reports an EMPTY side as "0" (live smoke test 2026-09-25), not as a
                    // missing field; no real order rests at price 0, so 0 means "no level".
                    let empty_is_none = |p: Option<Price>| p.filter(|x| x.get() != Decimal::ZERO);
                    let (ts, bb, ba) = (
                        *server_ts_ms,
                        empty_is_none(*best_bid),
                        empty_is_none(*best_ask),
                    );
                    let (side, price, size) = (*side, *price, *size);
                    self.on_book(conn, token, ts, &mut touched, &mut out, |b| {
                        let _ = b.apply_delta(side, price, size, ts);
                        let top = b.top();
                        let ours_bid = top.best_bid.map(|(p, _)| p);
                        let ours_ask = top.best_ask.map(|(p, _)| p);
                        let bad = (bb.is_some() && bb != ours_bid) || (ba.is_some() && ba != ours_ask);
                        bad.then(|| format!("server bid {bb:?} ask {ba:?}; ours bid {ours_bid:?} ask {ours_ask:?}"))
                    });
                }
                MarketEvent::LastTrade { server_ts_ms, .. }
                | MarketEvent::TickSize { server_ts_ms, .. } => {
                    bump(&mut frame_ts, *server_ts_ms);
                }
                MarketEvent::Resolved { .. } | MarketEvent::NewMarket { .. } => {
                    out.push(Out::UniverseChanged)
                }
                MarketEvent::Other { .. } => {}
            }
        }
        let Some(ts) = frame_ts else {
            return out;
        };
        out.push(Out::Lag(recv_ms - ts));
        for g in touched {
            self.evaluate(g, ts, recv_ms, &mut out);
        }
        out
    }

    fn on_book<F>(
        &mut self,
        conn: usize,
        token: &str,
        ts: i64,
        touched: &mut BTreeSet<usize>,
        out: &mut Vec<Out>,
        f: F,
    ) where
        F: FnOnce(&mut Book) -> Option<String>,
    {
        let (mismatch, crossed, group) = if let Some(&(g, l)) = self.token_ix.get(token) {
            let b = &mut self.groups[g].legs[l].book;
            let m = f(b);
            let crossed = b.is_crossed();
            touched.insert(g);
            *self
                .msgs
                .entry(self.universe.groups[g].event_id.clone())
                .or_insert(0) += 1;
            (m, crossed, Some(g))
        } else if let Some(b) = self.canary.get_mut(token) {
            let m = f(b);
            (m, b.is_crossed(), None)
        } else {
            return;
        };
        let event_id = group.map(|g| self.universe.groups[g].event_id.clone());
        if let Some(detail) = mismatch {
            out.push(self.integrity(
                ts,
                Issue {
                    kind: "top_mismatch",
                    shard: Some(conn),
                    token: Some(token.to_string()),
                    event_id: event_id.clone(),
                    detail: Some(detail),
                },
            ));
            out.push(Out::Resnapshot {
                token: token.to_string(),
            });
            if let Some(g) = group {
                self.suspect(g, ts + self.ctx.suspect_window_ms);
            }
        }
        if crossed {
            out.push(self.integrity(
                ts,
                Issue {
                    kind: "crossed_book",
                    shard: Some(conn),
                    token: Some(token.to_string()),
                    event_id,
                    detail: None,
                },
            ));
            if let Some(g) = group {
                self.suspect(g, ts + self.ctx.suspect_window_ms);
            }
        }
    }

    pub(crate) fn evaluate(&mut self, g: usize, ts: i64, recv_ms: i64, out: &mut Vec<Out>) {
        for det in [Det::Yes, Det::No] {
            let sig = match det {
                Det::Yes => detect_yes_basket(&self.groups[g]),
                Det::No => detect_no_basket(&self.groups[g]),
            };
            let key = Self::key(&self.universe.groups[g], det);
            let pnl = sig
                .as_ref()
                .filter(|s| s.net_positive())
                .map(|s| s.quote.pnl);
            let at = At {
                g,
                det,
                ts,
                recv_ms,
            };
            let mut opened = false;
            for e in self.tracker.on_state(&key, ts, pnl) {
                match e {
                    EpisodeEvent::Open { id, .. } => {
                        if let Some(s) = &sig {
                            self.open_episode(id, &key, at, s, out);
                            opened = true;
                        }
                    }
                    EpisodeEvent::Peak { id, .. } => {
                        if let Some(s) = &sig {
                            self.peak_episode(id, at, s, out);
                        }
                    }
                    close @ EpisodeEvent::Close { .. } => self.close_episode(close, out),
                }
            }
            if pnl.is_some()
                && !opened
                && let Some(ep) = self
                    .open_by_key
                    .get(&key)
                    .and_then(|id| self.open.get_mut(id))
            {
                ep.n_updates += 1;
            }
        }
    }

    fn snap(&self, at: At, s: &Signal) -> Snap {
        let legs: Vec<BuyLeg> = s
            .legs_used
            .iter()
            .map(|&i| {
                let leg = &self.groups[at.g].legs[i];
                BuyLeg {
                    levels: match at.det {
                        Det::Yes => leg.book.asks_best_first(),
                        Det::No => leg.book.no_asks_best_first(),
                    },
                    fee: leg.fee,
                }
            })
            .collect();
        let rule = match at.det {
            Det::Yes => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
            Det::No => BasketRule::NestedNo,
        };
        let budget100 = walk(&legs, rule, Some(Decimal::from(100))).pnl;
        let best = legs
            .iter()
            .map(|l| {
                l.levels
                    .first()
                    .map_or((None, None), |&(p, q)| (Some(p), Some(q)))
            })
            .collect();
        let leg_fees = legs
            .iter()
            .zip(&s.quote.legs)
            .map(|(l, f)| leg_fee(l, f.filled))
            .collect();
        Snap {
            signal: s.clone(),
            budget100,
            best,
            leg_fees,
            ts_ms: at.ts,
        }
    }

    fn open_episode(&mut self, id: u64, key: &str, at: At, s: &Signal, out: &mut Vec<Out>) {
        let lag = at.recv_ms - at.ts;
        // The tracker has already inserted the open episode by the time it returns the Open
        // event, so `open_censor` reads back the `left` flag it just decided (spec censor_left).
        let censor_left = self.tracker.open_censor(key).is_some_and(|c| c.left);
        let ep = OpenEp {
            episode_id: format!("{}-{}-{:08}", self.ctx.source, self.ctx.run_id, id),
            key: key.to_string(),
            group: at.g,
            det: at.det,
            opened_ms: at.ts,
            open_lag_ms: lag,
            stale: lag > self.ctx.stale_lag_ms,
            suspect: self.suspect_until[at.g] > at.ts,
            n_updates: 1,
            censor_left,
            peak: self.snap(at, s),
        };
        out.push(Out::Episode(Box::new(self.row(&ep, None))));
        out.push(Out::Recheck(RecheckReq {
            episode_id: ep.episode_id.clone(),
            event_id: self.universe.groups[at.g].event_id.clone(),
            det: at.det,
            opened_ms: at.ts,
            q_max: s.quote.q_max,
        }));
        self.open_by_key.insert(key.to_string(), id);
        self.open.insert(id, ep);
    }

    fn peak_episode(&mut self, id: u64, at: At, s: &Signal, out: &mut Vec<Out>) {
        let snap = self.snap(at, s);
        let suspect_now = self.suspect_until[at.g] > at.ts;
        if let Some(ep) = self.open.get_mut(&id) {
            ep.peak = snap;
            ep.suspect |= suspect_now;
        }
        if let Some(ep) = self.open.get(&id) {
            out.push(Out::Episode(Box::new(self.row(ep, None))));
        }
    }

    pub(crate) fn close_episode(&mut self, e: EpisodeEvent, out: &mut Vec<Out>) {
        if let EpisodeEvent::Close {
            id,
            ts_ms,
            duration_ms,
            censor,
            ..
        } = e
            && let Some(ep) = self.open.remove(&id)
        {
            self.open_by_key.remove(&ep.key);
            out.push(Out::Episode(Box::new(
                self.row(&ep, Some((ts_ms, duration_ms, censor))),
            )));
        }
    }

    fn row(&self, ep: &OpenEp, closed: Option<(i64, i64, Censor)>) -> EpisodeWrite {
        let meta = &self.universe.groups[ep.group];
        let s = &ep.peak.signal;
        let q = &s.quote;
        let used = &s.legs_used;
        let live = meta
            .legs
            .iter()
            .filter(|l| l.status == Status::Live)
            .count();
        let uncovered = meta
            .legs
            .iter()
            .filter(|l| !matches!(l.status, Status::Live | Status::ClosedNo))
            .count();
        let bottleneck = q
            .legs
            .iter()
            .enumerate()
            .min_by_key(|(_, f)| f.notional)
            .map(|(k, f)| (used.get(k).copied().unwrap_or(k), f.notional));
        let min_size = used.iter().filter_map(|&i| meta.legs[i].min_size).max();
        let censor = closed.map(|c| c.2).unwrap_or(Censor {
            left: ep.censor_left,
            right: false,
            gap: false,
        });
        let row = EpisodeRow {
            episode_id: ep.episode_id.clone(),
            run_id: self.ctx.run_id,
            source: self.ctx.source.clone(),
            kind: kind_str(s.kind).to_string(),
            realization: realization_str(s.realization).to_string(),
            event_id: meta.event_id.clone(),
            event_slug: meta.slug.clone(),
            event_title: meta.title.clone(),
            neg_risk_market_id: meta.neg_risk_market_id.clone(),
            pair_id: None,
            category: None,
            n_outcomes: to_i32(meta.legs.len()),
            n_legs: to_i32(used.len()),
            legs_dropped: to_i32(live.saturating_sub(used.len())),
            augmented: meta.augmented,
            incomplete_set: s.kind == Kind::YesBasketIncomplete,
            uncovered_legs: to_i32(uncovered),
            opened_server_ms: ep.opened_ms,
            closed_server_ms: closed.map(|c| c.0),
            duration_ms: closed.map(|c| c.1),
            censor_left: censor.left,
            censor_right: censor.right,
            censor_gap: censor.gap,
            detect_lag_ms: Some(ep.open_lag_ms),
            n_updates: ep.n_updates,
            peak_server_ms: ep.peak.ts_ms,
            peak_edge_top: q.top_edge,
            peak_edge_top_gross: q.top_edge_gross,
            peak_max_size: q.q_max,
            peak_cost_usd: q.cost,
            peak_payout_usd: q.payout,
            peak_fees_usd: q.fees,
            peak_pnl_net_usd: q.pnl,
            peak_pnl_gross_usd: q.pnl_gross,
            peak_profit_per_dollar: if q.cost > Decimal::ZERO {
                q.pnl.checked_div(q.cost)
            } else {
                None
            },
            pnl_net_budget100_usd: Some(ep.peak.budget100),
            bottleneck_leg_idx: bottleneck.map(|b| to_i32(b.0)),
            bottleneck_notional_usd: bottleneck.map(|b| b.1),
            below_min_size: min_size.map(|m| q.q_max < m),
            max_seconds_delay: used
                .iter()
                .filter_map(|&i| meta.legs[i].seconds_delay)
                .max(),
            days_to_end: None,
            annualized_return: None,
            post_end: None,
            engine_suspect: ep.suspect,
            stale_transport: ep.stale,
            open_lag_ms: Some(ep.open_lag_ms),
            oracle_downgraded: None,
            scanner_version: self.ctx.scanner_version.clone(),
            config_hash: self.ctx.config_hash.clone(),
        };
        let legs = used
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let lm = &meta.legs[i];
                let fill = q.legs.get(k).cloned().unwrap_or_default();
                let (best_price, size_at_best) =
                    ep.peak.best.get(k).copied().unwrap_or((None, None));
                EpisodeLegRow {
                    episode_id: ep.episode_id.clone(),
                    leg_idx: to_i32(i),
                    market_id: lm.market_id.clone(),
                    token: match ep.det {
                        Det::Yes => lm.yes_token.clone(),
                        Det::No => lm.no_token.clone(),
                    },
                    action: match ep.det {
                        Det::Yes => "buy_yes".to_string(),
                        Det::No => "buy_no".to_string(),
                    },
                    best_price: best_price.map(|p| p.get()),
                    vwap: if fill.filled > Decimal::ZERO {
                        fill.notional.checked_div(fill.filled)
                    } else {
                        None
                    },
                    levels: to_i32(fill.levels_used),
                    size_at_best: size_at_best.map(|s| s.get()),
                    filled: fill.filled,
                    notional: fill.notional,
                    fees: ep.peak.leg_fees.get(k).copied().unwrap_or(Decimal::ZERO),
                    in_s: true,
                    fee_rate: lm.fee_rate,
                    fee_exponent: i16::from(lm.fee_exponent),
                }
            })
            .collect();
        EpisodeWrite { row, legs }
    }

    pub fn disconnect(&mut self, conn: usize, ts_ms: i64) -> Vec<Out> {
        let mut out = vec![self.integrity(
            ts_ms,
            Issue {
                kind: "reconnect_gap",
                shard: Some(conn),
                ..Issue::default()
            },
        )];
        for g in 0..self.groups.len() {
            if self.group_conn[g] != Some(conn) {
                continue;
            }
            for det in [Det::Yes, Det::No] {
                let key = Self::key(&self.universe.groups[g], det);
                for e in self.tracker.on_disconnect(&key, ts_ms) {
                    self.close_episode(e, &mut out);
                }
            }
        }
        out
    }

    pub fn close_all(&mut self, ts_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        for e in self.tracker.close_all(ts_ms) {
            self.close_episode(e, &mut out);
        }
        out
    }

    pub fn alive(&mut self, conn: usize, ts_ms: i64) {
        let keys: Vec<String> = self
            .open
            .values()
            .filter(|ep| self.group_conn.get(ep.group).copied().flatten() == Some(conn))
            .map(|ep| ep.key.clone())
            .collect();
        for k in keys {
            self.tracker.on_alive(&k, ts_ms);
        }
    }

    pub fn book_of(&self, token: &str) -> Option<&Book> {
        match self.token_ix.get(token) {
            Some(&(g, l)) => Some(&self.groups[g].legs[l].book),
            None => self.canary.get(token),
        }
    }

    pub fn rest_snapshot(&mut self, token: &str, book: Book) -> Vec<Out> {
        let mut out = Vec::new();
        let ts = book.server_ts_ms();
        if let Some(&(g, l)) = self.token_ix.get(token) {
            if ts < self.groups[g].legs[l].book.server_ts_ms() {
                return out;
            }
            self.groups[g].legs[l].book = book;
            self.evaluate(g, ts, ts, &mut out);
        } else if let Some(b) = self.canary.get_mut(token)
            && ts >= b.server_ts_ms()
        {
            *b = book;
        }
        out
    }

    pub fn drift_check(&mut self, token: &str, rest: Book, now_ms: i64) {
        self.pending_drift.push(PendingDrift {
            token: token.to_string(),
            rest,
            due_ms: now_ms + self.ctx.drift_settle_ms,
        });
    }

    pub fn tick(&mut self, now_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let (due, later): (Vec<PendingDrift>, Vec<PendingDrift>) =
            std::mem::take(&mut self.pending_drift)
                .into_iter()
                .partition(|p| p.due_ms <= now_ms);
        self.pending_drift = later;
        for p in due {
            let Some(ours) = self.book_of(&p.token) else {
                continue;
            };
            if ours.server_ts_ms() == 0 {
                continue; // never received a snapshot (unsubscribed / dropped group): nothing to compare
            }
            if ours.server_ts_ms() > p.rest.server_ts_ms() {
                continue; // we moved on after the REST snapshot: inconclusive
            }
            if ours.asks_best_first() == p.rest.asks_best_first()
                && ours.bids_best_first() == p.rest.bids_best_first()
            {
                continue;
            }
            let event_id = self
                .token_ix
                .get(&p.token)
                .map(|&(g, _)| self.universe.groups[g].event_id.clone());
            out.push(Out::Drift {
                token: p.token.clone(),
                event_id,
                detail: format!(
                    "ws asks {} bids {} vs rest asks {} bids {} (ws ts {} rest ts {})",
                    ours.asks_best_first().len(),
                    ours.bids_best_first().len(),
                    p.rest.asks_best_first().len(),
                    p.rest.bids_best_first().len(),
                    ours.server_ts_ms(),
                    p.rest.server_ts_ms()
                ),
            });
        }
        out
    }

    pub fn mark_suspect(&mut self, event_id: &str, until_ms: i64) {
        if let Some(g) = self
            .universe
            .groups
            .iter()
            .position(|m| m.event_id == event_id)
        {
            self.suspect(g, until_ms);
        }
    }

    pub fn rates(&mut self) -> HashMap<String, u64> {
        std::mem::take(&mut self.msgs)
    }
}
