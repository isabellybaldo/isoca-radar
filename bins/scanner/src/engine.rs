//! The engine (spec §8.2): single owner of every book and the episode tracker. No IO, no clock:
//! time arrives on the inputs, so live and replay produce identical outputs (spec §9.3).
use crate::universe::{GroupMeta, Status, Universe};
use arb_core::book::Book;
use arb_core::detect::{
    Group, GroupLeg, Kind, Realization, Signal, detect_no_basket, detect_yes_basket,
    no_basket_may_signal, yes_basket_may_signal,
};
use arb_core::episode::{Censor, EpisodeEvent, EpisodeTracker};
use arb_core::fill::{BasketQuote, BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use arb_core::stats::{annualized, days_to_end, parse_rfc3339_ms};
use pm_api::ws::MarketEvent;
use rust_decimal::Decimal;
use sim::policy::{LegInfo, OpenCtx};
use sim::rows::SimOut;
use sim::trader::{Books, PaperTrader, TraderState};
use sim::types::BookSide;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    pub issue_throttle_ms: i64,
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
    /// A paper-trader row (S3c), boxed like `Episode` to keep `Out` small.
    Paper(Box<SimOut>),
    /// A ws `market_resolved` arrived: the live resolver should ask Gamma soon.
    ResolvedHint,
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
    /// Latest endDate over the group's live legs at open (spec §5.5 lock-up estimate).
    end_ms: Option<i64>,
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

/// A deferred top-check expectation for one token: Polymarket splits ONE logical update across
/// SEVERAL websocket messages sharing ONE server ts, and each item carries the FINAL top, so a
/// check right after the message that set it is too early (live smoke 2026-09-25: ~550
/// false top_mismatch/min at that cadence). Resolved once a later message proves the update is
/// over — see `Engine::resolve_pending`.
#[derive(Debug, Clone)]
struct PendingTop {
    conn: usize,
    ts: i64,
    bid: Option<Price>,
    ask: Option<Price>,
}

/// An episode that CLOSED while a deferred top-check on its group (ts ≤ its close) was still
/// unresolved: if that check later resolves as a mismatch, the closed row is re-flagged too.
#[derive(Debug, Clone)]
struct HeldClosed {
    ep: OpenEp,
    closed: (i64, i64, Censor),
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
    /// Keyed by token; iterated in key order so live and replay resolve pending checks
    /// identically (spec §9.3: no clock, deterministic from inputs alone).
    pending_tops: BTreeMap<String, PendingTop>,
    /// Secondary index of `pending_tops` (S3a-2 Task 4): conn → {(ts, token)}. The per-frame stale
    /// sweep is a range query on it, O(log n + due) instead of a scan of every pending check.
    /// Kept equal to `pending_tops` by `pending_insert`/`pending_remove`, plus the bulk resets
    /// in `apply_universe` (rebuild) and `close_all` (clear).
    pending_by_conn: HashMap<usize, BTreeSet<(i64, String)>>,
    /// Secondary index of `pending_tops`: group → multiset of pending ts (ts → count), so
    /// `pending_in_group` reads the group's minimum ts. Canary tokens are in no group. Indexed by
    /// the CURRENT `token_ix`, so `apply_universe` rebuilds it after remapping groups.
    pending_by_group: Vec<BTreeMap<i64, u32>>,
    /// Highest `ts` ever passed to `evaluate` for each group, carried across refreshes by
    /// event id. Lets `rest_snapshot` evaluate at a ts that never goes backwards even when the
    /// REST snapshot is older than a ws frame already evaluated for this group.
    group_last_ts: Vec<i64>,
    /// See `HeldClosed`. Emptied by `prune_held` once no unresolved check can flag them.
    held_closed: Vec<HeldClosed>,
    /// Per-(kind, token) integrity-row rate limit (S3a Task 8), in event time. Nested by kind
    /// (only ever a handful of `&'static str` constants) so a lookup by token borrows `&str`
    /// (`String: Borrow<str>`) instead of allocating one on every call; only a miss allocates,
    /// to insert the token's own key.
    issue_throttle: HashMap<&'static str, HashMap<String, IssueThrottle>>,
    /// The paper trader (S3c), fed by the S3b call contract. None = scanner only.
    paper: Option<PaperTrader>,
    /// The paper clock: the latest input instant the trader has been driven to (frame receive
    /// times, `on_resolution`, `paper_close`, a reset's start). Spec 7a §4.2: EVERY trader entry
    /// point is driven at max(its own time, this), so no row of a new epoch is ever stamped
    /// before its start; outside a reset this changes nothing, because everything due at or
    /// before this clock already ran at the frame that set it.
    last_recv_ms: i64,
    /// Spec §5.4/§8.2: skip a detector's depth walk when its top-of-book gate is false and no
    /// episode is open for its key (S3a-2). Always on in live and replay. Tests and the bench turn
    /// it off to prove the outputs are identical.
    threshold_gate: bool,
    /// Detector calls made so far (D2a + D2b). A counter of inputs processed, not a clock: pure.
    detector_calls: u64,
}

/// Per-(kind, token) integrity-row rate limit (S3a Task 8), in event time.
#[derive(Debug, Clone, Copy)]
struct IssueThrottle {
    last_ms: i64,
    suppressed: u32,
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

/// The engine's YES books seen through `sim::trader::Books`. The fields are borrowed one by one,
/// so the trader (another field of the engine) can read them with no clone.
struct EngineBooks<'a> {
    token_ix: &'a HashMap<String, (usize, usize)>,
    groups: &'a [Group],
}

impl Books for EngineBooks<'_> {
    fn yes_book(&self, yes_token: &str) -> Option<&Book> {
        let &(g, l) = self.token_ix.get(yes_token)?;
        self.groups.get(g)?.legs.get(l).map(|leg| &leg.book)
    }
}

fn paper_outs(v: Vec<SimOut>, out: &mut Vec<Out>) {
    out.extend(v.into_iter().map(|o| Out::Paper(Box::new(o))));
}

/// Spec §5.5's secondary figure for a quote: (1 + Π/cost)^(365/max(days, 7)) − 1 (no cost → None).
fn lockup_annualized(q: &BasketQuote, days: Decimal) -> Option<Decimal> {
    if q.cost <= Decimal::ZERO {
        return None;
    }
    annualized(q.pnl.checked_div(q.cost)?, days)
}

/// The levels present before a `Book::trim_to_top` and absent from `now` (a trim only removes),
/// as (side, price): each is a removal the paper trader's consumed-liquidity ledger must see.
fn gone_levels(
    bids_before: &[(Price, Shares)],
    asks_before: &[(Price, Shares)],
    now: &Book,
) -> Vec<(BookSide, Decimal)> {
    let (bids_now, asks_now) = (now.bids_best_first(), now.asks_best_first());
    let mut gone = Vec::new();
    for (side, before, after) in [
        (BookSide::Bid, bids_before, &bids_now),
        (BookSide::Ask, asks_before, &asks_now),
    ] {
        for (p, _) in before {
            if !after.iter().any(|(q, _)| q == p) {
                gone.push((side, p.get()));
            }
        }
    }
    gone
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
            pending_tops: BTreeMap::new(),
            pending_by_conn: HashMap::new(),
            pending_by_group: Vec::new(),
            group_last_ts: Vec::new(),
            held_closed: Vec::new(),
            issue_throttle: HashMap::new(),
            paper: None,
            last_recv_ms: i64::MIN,
            threshold_gate: true,
            detector_calls: 0,
        }
    }

    pub fn open_episodes(&self) -> usize {
        self.open.len()
    }

    /// Turn the §8.2 threshold gate off or on. Tests and the bench only: live and replay never call
    /// this. With the gate off, every evaluation runs both detectors, as before S3a-2.
    pub fn set_threshold_gate(&mut self, on: bool) {
        self.threshold_gate = on;
    }

    /// How many detector calls (D2a + D2b) this engine has made.
    pub fn detector_calls(&self) -> u64 {
        self.detector_calls
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

    /// Mark group `g` suspect until `until_ms`, AND flag right now every episode the bad book
    /// may already have touched: each open episode of `g`, and each held closed one that
    /// closed at or after `since_ms`. Each flag that flips re-emits that episode's row (the
    /// writer and `write_batch` keep the latest version). Sorted ids keep the output order
    /// deterministic (spec §9.3).
    pub(crate) fn suspect(&mut self, g: usize, until_ms: i64, since_ms: i64, out: &mut Vec<Out>) {
        if let Some(t) = self.suspect_until.get_mut(g) {
            *t = (*t).max(until_ms);
        }
        let mut open_ids: Vec<u64> = self
            .open
            .iter()
            .filter(|(_, ep)| ep.group == g && !ep.suspect)
            .map(|(id, _)| *id)
            .collect();
        open_ids.sort_unstable();
        for id in open_ids {
            if let Some(ep) = self.open.get_mut(&id) {
                ep.suspect = true;
            }
            if let Some(ep) = self.open.get(&id) {
                out.push(Out::Episode(Box::new(self.row(ep, None))));
            }
        }
        let held: Vec<usize> = self
            .held_closed
            .iter()
            .enumerate()
            .filter(|(_, h)| h.ep.group == g && !h.ep.suspect && h.closed.0 >= since_ms)
            .map(|(i, _)| i)
            .collect();
        for i in held {
            if let Some(h) = self.held_closed.get_mut(i) {
                h.ep.suspect = true;
            }
            if let Some(h) = self.held_closed.get(i) {
                out.push(Out::Episode(Box::new(self.row(&h.ep, Some(h.closed)))));
            }
        }
    }

    /// Is a deferred top-check on a leg of group `g` still unresolved with ts ≤ `ts_ms`?
    fn pending_in_group(&self, g: usize, ts_ms: i64) -> bool {
        self.pending_by_group
            .get(g)
            .and_then(|m| m.keys().next())
            .is_some_and(|&min| min <= ts_ms)
    }

    // ---- `pending_tops` and its two indexes, mutated together (bulk resets aside) ----

    fn index_add(&mut self, token: &str, p: &PendingTop) {
        self.pending_by_conn
            .entry(p.conn)
            .or_default()
            .insert((p.ts, token.to_string()));
        if let Some(&(g, _)) = self.token_ix.get(token)
            && let Some(m) = self.pending_by_group.get_mut(g)
        {
            *m.entry(p.ts).or_insert(0) += 1;
        }
    }

    fn index_drop(&mut self, token: &str, p: &PendingTop) {
        if let Some(set) = self.pending_by_conn.get_mut(&p.conn) {
            set.remove(&(p.ts, token.to_string()));
            if set.is_empty() {
                self.pending_by_conn.remove(&p.conn);
            }
        }
        if let Some(&(g, _)) = self.token_ix.get(token)
            && let Some(m) = self.pending_by_group.get_mut(g)
            && let Some(n) = m.get_mut(&p.ts)
        {
            *n -= 1;
            if *n == 0 {
                m.remove(&p.ts);
            }
        }
    }

    /// Insert or replace `token`'s pending check; a replace drops the old entry's index keys.
    /// The old keys go first: a same-(conn, ts) replace has identical (ts, token) keys.
    fn pending_insert(&mut self, token: &str, p: PendingTop) {
        self.pending_remove(token);
        self.index_add(token, &p);
        self.pending_tops.insert(token.to_string(), p);
    }

    fn pending_remove(&mut self, token: &str) -> Option<PendingTop> {
        let p = self.pending_tops.remove(token)?;
        self.index_drop(token, &p);
        Some(p)
    }

    /// Rebuild `pending_by_group` from `pending_tops` and the current `token_ix` (group indices
    /// change on a universe refresh).
    fn rebuild_pending_by_group(&mut self) {
        let mut by_group: Vec<BTreeMap<i64, u32>> = vec![BTreeMap::new(); self.groups.len()];
        for (t, p) in &self.pending_tops {
            if let Some(&(g, _)) = self.token_ix.get(t)
                && let Some(m) = by_group.get_mut(g)
            {
                *m.entry(p.ts).or_insert(0) += 1;
            }
        }
        self.pending_by_group = by_group;
    }

    /// Both indexes must equal what a full scan of `pending_tops` gives: Ok(number of pending
    /// checks), or a description of the difference.
    fn check_pending_indexes(&self) -> Result<usize, String> {
        let mut by_conn: HashMap<usize, BTreeSet<(i64, String)>> = HashMap::new();
        let mut by_group: Vec<BTreeMap<i64, u32>> = vec![BTreeMap::new(); self.groups.len()];
        for (t, p) in &self.pending_tops {
            by_conn.entry(p.conn).or_default().insert((p.ts, t.clone()));
            if let Some(&(g, _)) = self.token_ix.get(t) {
                match by_group.get_mut(g) {
                    Some(m) => *m.entry(p.ts).or_insert(0) += 1,
                    None => return Err(format!("{t}: group {g} out of range")),
                }
            }
        }
        if by_conn != self.pending_by_conn {
            return Err(format!(
                "pending_by_conn {:?} != scan {by_conn:?}",
                self.pending_by_conn
            ));
        }
        if by_group != self.pending_by_group {
            return Err(format!(
                "pending_by_group {:?} != scan {by_group:?}",
                self.pending_by_group
            ));
        }
        Ok(self.pending_tops.len())
    }

    /// For tests only: checks the pending top-check indexes against a full scan of
    /// `pending_tops`. O(pending); the engine never calls it.
    #[doc(hidden)]
    pub fn debug_check_pending_indexes(&self) -> Result<usize, String> {
        self.check_pending_indexes()
    }

    /// Forget held closed episodes that no unresolved check can flag any more.
    fn prune_held(&mut self) {
        if self.held_closed.is_empty() {
            return;
        }
        let held = std::mem::take(&mut self.held_closed);
        self.held_closed = held
            .into_iter()
            .filter(|h| self.pending_in_group(h.ep.group, h.closed.0))
            .collect();
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
        // 1b. groups that stay in the universe but get no connection this refresh (dropped by
        // the connection budget or unassigned): right-censor any open episode and reset the
        // tracker's first-sight state for BOTH keys (rows from the OLD universe, before any
        // mutation — same treatment as `disconnect`).
        let no_conn: HashSet<&str> = u
            .groups
            .iter()
            .filter(|m| !conn_of.contains_key(&m.event_id))
            .map(|m| m.event_id.as_str())
            .collect();
        let mut no_conn_groups: Vec<usize> = self
            .universe
            .groups
            .iter()
            .enumerate()
            .filter(|(_, m)| no_conn.contains(m.event_id.as_str()))
            .map(|(g, _)| g)
            .collect();
        no_conn_groups.sort_unstable();
        for g in &no_conn_groups {
            for det in [Det::Yes, Det::No] {
                let key = Self::key(&self.universe.groups[*g], det);
                for e in self.tracker.on_disconnect(&key, ts_ms) {
                    self.close_episode(e, &mut out);
                }
            }
        }
        // 1c. an open episode's `legs_used` indices refer to the leg list AT OPEN TIME: if the
        // event's (market_id, yes_token) leg sequence changed, those indices may no longer be
        // valid. Right-censor before the universe (and any leg list) is swapped.
        let mut leg_changed_groups: Vec<usize> = Vec::new();
        for (g, meta) in self.universe.groups.iter().enumerate() {
            if let Some(new_meta) = u.group_by_event(&meta.event_id) {
                let old_legs = meta
                    .legs
                    .iter()
                    .map(|l| (l.market_id.as_str(), l.yes_token.as_str()));
                let new_legs = new_meta
                    .legs
                    .iter()
                    .map(|l| (l.market_id.as_str(), l.yes_token.as_str()));
                if !old_legs.eq(new_legs) {
                    leg_changed_groups.push(g);
                }
            }
        }
        for g in &leg_changed_groups {
            for det in [Det::Yes, Det::No] {
                let key = Self::key(&self.universe.groups[*g], det);
                for e in self.tracker.on_disconnect(&key, ts_ms) {
                    self.close_episode(e, &mut out);
                }
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
        let old_last_ts: HashMap<String, i64> = self
            .universe
            .groups
            .iter()
            .zip(&self.group_last_ts)
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
            let has_conn = conn_of.contains_key(&meta.event_id);
            let legs = meta
                .legs
                .iter()
                .enumerate()
                .map(|(l, leg)| {
                    if leg.status == Status::Live && !leg.yes_token.is_empty() {
                        self.token_ix.insert(leg.yes_token.clone(), (g, l));
                    }
                    let carried = old_books.remove(&leg.yes_token);
                    // A group with no connection this refresh gets no book: nothing stale
                    // survives to look tradeable while unsubscribed (F4).
                    let book = if has_conn {
                        carried.unwrap_or_default()
                    } else {
                        Book::default()
                    };
                    GroupLeg {
                        status: leg.status.to_core(),
                        book,
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
        // Drop deferred top-checks for tokens no longer subscribed at all: there's no book left
        // to resolve them against.
        // `token_ix` is already the new universe's, so drop from `pending_tops` directly and
        // rebuild both indexes from it (once per refresh).
        self.pending_tops
            .retain(|t, _| self.token_ix.contains_key(t) || self.canary.contains_key(t));
        self.pending_by_conn.clear();
        for (t, p) in &self.pending_tops {
            self.pending_by_conn
                .entry(p.conn)
                .or_default()
                .insert((p.ts, t.clone()));
        }
        self.rebuild_pending_by_group();
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
        self.group_last_ts = u
            .groups
            .iter()
            .map(|m| old_last_ts.get(&m.event_id).copied().unwrap_or(i64::MIN))
            .collect();
        for (id, ev) in open_events {
            if let (Some(&g), Some(ep)) = (index.get(&ev), self.open.get_mut(&id)) {
                ep.group = g;
            }
        }
        // Held closed episodes follow their event to its new index; an event that left the
        // universe drops them (its deferred checks were dropped with its tokens above).
        let held = std::mem::take(&mut self.held_closed);
        self.held_closed = held
            .into_iter()
            .filter_map(|mut h| {
                let ev = &self.universe.groups[h.ep.group].event_id;
                index.get(ev).map(|&g| {
                    h.ep.group = g;
                    h
                })
            })
            .collect();
        self.universe = u;
        out
    }

    pub fn on_frame(&mut self, conn: usize, recv_ms: i64, events: &[MarketEvent]) -> Vec<Out> {
        let mut out = Vec::new();
        let mut touched: BTreeSet<usize> = BTreeSet::new();
        let mut frame_ts: Option<i64> = None;
        // A snapshot is complete state, so its crossed check runs immediately, once per token,
        // after the whole frame (like evaluation, spec 5.5). A delta's top/crossed check is
        // deferred instead (`pending_tops`): Polymarket splits ONE logical update across SEVERAL
        // websocket messages sharing ONE server ts, and each item's best_bid/best_ask already
        // describes the FINAL top, so checking right after one item flags in-between states that
        // never existed (live smoke 2026-09-25). See `resolve_pending`.
        let mut frame_tokens: Vec<String> = Vec::new();
        self.last_recv_ms = self.last_recv_ms.max(recv_ms);
        let clock = self.last_recv_ms;
        // S3b call contract: work due at or before this frame runs on the books as they were
        // just BEFORE it (the state at its due time). Spec 7a §4.2: at the paper clock.
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(clock, &books), &mut out);
        }
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
                    if self.on_book(token, &mut touched, |b| b.apply_snapshot(bids, asks, ts)) {
                        self.pending_remove(token); // a snapshot supersedes earlier deltas' tops
                        if !frame_tokens.iter().any(|t| t == token) {
                            frame_tokens.push(token.clone());
                        }
                        if let (Some(t), Some(&(g, l))) =
                            (self.paper.as_mut(), self.token_ix.get(token.as_str()))
                        {
                            t.on_snapshot(token, &self.groups[g].legs[l].book);
                        }
                    }
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
                    // Likewise an EMPTY ask side arrives as "1": no real order rests at 1 either.
                    let (bb, ba) = tape::feed::server_top(*best_bid, *best_ask);
                    let ts = *server_ts_ms;
                    let (side, price, size) = (*side, *price, *size);
                    // A pending entry from an EARLIER server ts is for a DIFFERENT logical update
                    // (messages of the SAME update all share one ts): it's now definitely
                    // complete, so resolve it against our book as it stood before this delta.
                    if self
                        .pending_tops
                        .get(token.as_str())
                        .is_some_and(|p| p.ts < ts)
                    {
                        self.resolve_pending(token, recv_ms, &mut out);
                    }
                    let found = self.on_book(token, &mut touched, |b| {
                        let _ = b.apply_delta(side, price, size, ts);
                    });
                    if found {
                        // A same-ts delta for this token just replaces the pending entry: last wins.
                        self.pending_insert(
                            token,
                            PendingTop {
                                conn,
                                ts,
                                bid: bb,
                                ask: ba,
                            },
                        );
                    }
                    if found
                        && let Some(t) = self.paper.as_mut()
                        && self.token_ix.contains_key(token.as_str())
                    {
                        t.on_level(token, BookSide::from_core(side), price.get(), size.get());
                    }
                }
                MarketEvent::LastTrade { server_ts_ms, .. }
                | MarketEvent::TickSize { server_ts_ms, .. } => {
                    bump(&mut frame_ts, *server_ts_ms);
                }
                MarketEvent::Resolved { .. } => {
                    out.push(Out::UniverseChanged);
                    out.push(Out::ResolvedHint);
                }
                MarketEvent::NewMarket { .. } => out.push(Out::UniverseChanged),
                MarketEvent::Other { .. } => {}
            }
        }
        let Some(ts) = frame_ts else {
            return out;
        };
        // Pending entries old enough that this frame can't be a further split of the same
        // logical update settle now (same shard, ts stale by more than 1 s). A range query on the
        // conn index finds them (p.ts + 1 000 < ts ⇔ p.ts < ts − 1 000), and they resolve in
        // TOKEN order, the order of the old full scan of `pending_tops`: outputs are unchanged.
        let mut due: Vec<String> = self
            .pending_by_conn
            .get(&conn)
            .map(|set| {
                set.range(..(ts.saturating_sub(1_000), String::new()))
                    .map(|(_, t)| t.clone())
                    .collect()
            })
            .unwrap_or_default();
        due.sort_unstable();
        for token in due {
            self.resolve_pending(&token, recv_ms, &mut out);
        }
        for token in &frame_tokens {
            self.check_book(conn, token, ts, recv_ms, None, &mut out);
        }
        out.push(Out::Lag(recv_ms - ts));
        for g in touched {
            self.evaluate(g, ts, recv_ms, &mut out);
        }
        out
    }

    /// Apply one book change. Returns whether `token` was actually known (a group leg or a
    /// canary), so callers can decide whether to track it for a deferred/immediate check.
    fn on_book<F>(&mut self, token: &str, touched: &mut BTreeSet<usize>, f: F) -> bool
    where
        F: FnOnce(&mut Book),
    {
        if let Some(&(g, l)) = self.token_ix.get(token) {
            f(&mut self.groups[g].legs[l].book);
            touched.insert(g);
            *self
                .msgs
                .entry(self.universe.groups[g].event_id.clone())
                .or_insert(0) += 1;
            true
        } else if let Some(b) = self.canary.get_mut(token) {
            f(b);
            true
        } else {
            false
        }
    }

    /// Settle one deferred top-check: compare the pending expectation (and the crossed check)
    /// against our book now, stamped with the pending entry's own connection and ts — not the
    /// caller's — since it may be resolved from a later frame. `recv_ms` is the RESOLVING frame's
    /// own receipt time (an engine input, captured in replay): if this resolution repairs a
    /// phantom and re-evaluates its group, the episode that opens must get this real recv_ms,
    /// never the event ts (S3a final review: a fabricated recv_ms == ts gave a false
    /// open_lag_ms of 0 / stale = false).
    fn resolve_pending(&mut self, token: &str, recv_ms: i64, out: &mut Vec<Out>) {
        if let Some(p) = self.pending_remove(token) {
            self.check_book(p.conn, token, p.ts, recv_ms, Some((p.bid, p.ask)), out);
            self.prune_held();
        }
    }

    fn book_mut(&mut self, token: &str) -> Option<&mut Book> {
        if let Some((g, l)) = self.token_ix.get(token).copied() {
            return self
                .groups
                .get_mut(g)
                .and_then(|gr| gr.legs.get_mut(l))
                .map(|leg| &mut leg.book);
        }
        self.canary.get_mut(token)
    }

    /// Some(suppressed since the last emitted row) when a row may be emitted at `ts`; None while
    /// still inside `issue_throttle_ms` of the last one. Pure: time is the event's ts.
    fn throttle(&mut self, kind: &'static str, token: &str, ts: i64) -> Option<u32> {
        let window = self.ctx.issue_throttle_ms;
        let by_token = self.issue_throttle.entry(kind).or_default();
        // `get_mut` borrows `token` as-is (no allocation): only a miss allocates, to insert.
        let e = if let Some(e) = by_token.get_mut(token) {
            e
        } else {
            by_token.entry(token.to_string()).or_insert(IssueThrottle {
                last_ms: i64::MIN,
                suppressed: 0,
            })
        };
        if e.last_ms != i64::MIN && ts.saturating_sub(e.last_ms) < window {
            e.suppressed = e.suppressed.saturating_add(1);
            return None;
        }
        let n = e.suppressed;
        e.last_ms = ts;
        e.suppressed = 0;
        Some(n)
    }

    /// End-of-frame integrity for one token: the server's final top vs ours (repaired from the
    /// stream when possible), then the crossed-book check on the (possibly trimmed) book.
    /// `recv_ms` is the resolving frame's own receipt time; see `resolve_pending`.
    fn check_book(
        &mut self,
        conn: usize,
        token: &str,
        ts: i64,
        recv_ms: i64,
        server: Option<(Option<Price>, Option<Price>)>,
        out: &mut Vec<Out>,
    ) {
        let group = self.token_ix.get(token).map(|&(g, _)| g);
        // A group with no connection this refresh has no live feed to check against (F4): a
        // deferred top-check on it can only be a false positive.
        if group.is_some_and(|g| self.group_conn.get(g).copied().flatten().is_none()) {
            return;
        }
        let Some(book) = self.book_of(token) else {
            return;
        };
        let top = book.top();
        let ours_bid = top.best_bid.map(|(p, _)| p);
        let ours_ask = top.best_ask.map(|(p, _)| p);
        let mismatch = server.and_then(|(bb, ba)| {
            let bad = (bb.is_some() && bb != ours_bid) || (ba.is_some() && ba != ours_ask);
            bad.then(|| {
                format!("server bid {bb:?} ask {ba:?}; ours bid {ours_bid:?} ask {ours_ask:?}")
            })
        });
        let event_id = group.map(|g| self.universe.groups[g].event_id.clone());
        if let Some(mut detail) = mismatch {
            // Rule 1 (S3a Task 8): the stream's top is authoritative. Drop every level of ours
            // the server's top disproves, then see if that alone repairs the disagreement. The
            // trim is unconditional: a phantom level must never be priced, whether or not it's
            // the ONLY thing wrong with the book.
            let (sb, sa) = server.unwrap_or((None, None));
            // S3c: a trimmed level is a removal the paper trader's consumed-liquidity ledger must
            // see, exactly like a size-0 delta. Remember the levels before the trim: group legs
            // only, only when a trader is attached, and only on a top mismatch (one book's levels,
            // never a scan of the universe).
            let before = match (group, self.paper.is_some()) {
                (Some(_), true) => self
                    .book_of(token)
                    .map(|b| (b.bids_best_first(), b.asks_best_first())),
                _ => None,
            };
            let mut changed = false;
            let repaired = self.book_mut(token).is_some_and(|b| {
                changed = b.trim_to_top(sb, sa);
                let t = b.top();
                (sb.is_none() || sb == t.best_bid.map(|(x, _)| x))
                    && (sa.is_none() || sa == t.best_ask.map(|(x, _)| x))
            });
            if changed && let Some((bids, asks)) = before {
                let gone = self
                    .book_of(token)
                    .map(|b| gone_levels(&bids, &asks, b))
                    .unwrap_or_default();
                if let Some(t) = self.paper.as_mut() {
                    for (side, price) in gone {
                        t.on_level(token, side, price, Decimal::ZERO);
                    }
                }
            }
            if repaired {
                detail.push_str(" → repaired from stream");
            }
            if let Some(n) = self.throttle("top_mismatch", token, ts) {
                if n > 0 {
                    detail.push_str(&format!(" (+{n} suppressed)"));
                }
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
                if !repaired {
                    out.push(Out::Resnapshot {
                        token: token.to_string(),
                    });
                }
            }
            if let Some(g) = group {
                self.suspect(g, ts + self.ctx.suspect_window_ms, ts, out);
            }
            // Re-evaluate whenever the trim changed the book, repaired or not (review round 1,
            // 2026-09-25): a phantom that only partly disproves our top still puts a wrong book
            // in front of `evaluate`, and the close time must depend on the mismatch's own `ts`,
            // never on whether some OTHER traffic happens to share this frame.
            if changed && let Some(g) = group {
                let eval_ts = ts.max(self.group_last_ts.get(g).copied().unwrap_or(i64::MIN));
                self.evaluate(g, eval_ts, recv_ms, out);
            }
        }
        // Rule 3: evaluated after the trim, so a phantom that crossed the book and was repaired
        // is not also reported as crossed.
        let crossed = self.book_of(token).is_some_and(|b| b.is_crossed());
        if crossed {
            if let Some(n) = self.throttle("crossed_book", token, ts) {
                out.push(self.integrity(
                    ts,
                    Issue {
                        kind: "crossed_book",
                        shard: Some(conn),
                        token: Some(token.to_string()),
                        event_id,
                        detail: (n > 0).then(|| format!("+{n} suppressed")),
                    },
                ));
            }
            if let Some(g) = group {
                self.suspect(g, ts + self.ctx.suspect_window_ms, ts, out);
            }
        }
    }

    pub(crate) fn evaluate(&mut self, g: usize, ts: i64, recv_ms: i64, out: &mut Vec<Out>) {
        // A group with no connection this refresh (dropped by the connection budget or
        // unassigned) has no live book to evaluate (F4): its episode was already closed
        // right-censored in `apply_universe`.
        if self.group_conn.get(g).copied().flatten().is_none() {
            return;
        }
        if let Some(last) = self.group_last_ts.get_mut(g) {
            *last = (*last).max(ts);
        }
        for det in [Det::Yes, Det::No] {
            let key = Self::key(&self.universe.groups[g], det);
            let group = &self.groups[g];
            // Spec §5.4/§8.2: walk the book only when the top-of-book sums cross the detector's
            // threshold. A false gate proves the detector returns None (arb-core `*_may_signal`),
            // so skipping it changes nothing. The tracker below still gets `None`, which records
            // first sight exactly as before. A key with an open episode always runs its detector,
            // so a close never depends on the gate.
            let run = !self.threshold_gate
                || self.open_by_key.contains_key(&key)
                || match det {
                    Det::Yes => yes_basket_may_signal(group),
                    Det::No => no_basket_may_signal(group),
                };
            let sig = if run {
                self.detector_calls += 1;
                match det {
                    Det::Yes => detect_yes_basket(group),
                    Det::No => detect_no_basket(group),
                }
            } else {
                None
            };
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

    /// The bought outcome's ask levels per used leg: YES asks, or NO asks (= 1 − YES bids, L1).
    fn buy_legs(&self, at: At, s: &Signal) -> Vec<BuyLeg> {
        s.legs_used
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
            .collect()
    }

    fn snap(&self, at: At, s: &Signal) -> Snap {
        let legs = self.buy_legs(at, s);
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

    /// The latest endDate over the group's live legs (spec §5.5: an estimate of lock-up).
    fn group_end_ms(&self, g: usize) -> Option<i64> {
        self.universe
            .groups
            .get(g)?
            .legs
            .iter()
            .filter(|l| l.status == Status::Live)
            .filter_map(|l| l.end_date.as_deref().and_then(parse_rfc3339_ms))
            .max()
    }

    /// Everything a policy may look at when this episode opens (S3b `OpenCtx`).
    fn open_ctx(&self, ep: &OpenEp, at: At, s: &Signal) -> OpenCtx {
        let meta = &self.universe.groups[at.g];
        let days = ep.end_ms.map(|e| days_to_end(e, at.ts));
        OpenCtx {
            episode_id: ep.episode_id.clone(),
            event_id: meta.event_id.clone(),
            // A frame (or a deferred check it settled) passes its own recv; `rest_snapshot`
            // passes the REST book's SERVER ts, which can be far behind. Never start the paper
            // clock before the latest instant the trader was driven to (rulings 15, 24).
            r_open_ms: at.recv_ms.max(self.last_recv_ms),
            signal: s.clone(),
            legs: meta
                .legs
                .iter()
                .map(|l| LegInfo {
                    market_id: l.market_id.clone(),
                    yes_token: l.yes_token.clone(),
                    fee: l.fee(),
                    seconds_delay: l.seconds_delay,
                })
                .collect(),
            open_levels: self.buy_legs(at, s).into_iter().map(|l| l.levels).collect(),
            days_to_end: days,
            annualized: days.and_then(|d| lockup_annualized(&s.quote, d)),
            pair_approved: false,
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
            end_ms: self.group_end_ms(at.g),
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
        if self.paper.is_some() {
            let ctx = self.open_ctx(&ep, at, s);
            if let Some(t) = self.paper.as_mut() {
                paper_outs(t.on_open(&ctx), out);
            }
        }
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
            let closed = (ts_ms, duration_ms, censor);
            out.push(Out::Episode(Box::new(self.row(&ep, Some(closed)))));
            if !ep.suspect && self.pending_in_group(ep.group, ts_ms) {
                self.held_closed.push(HeldClosed { ep, closed });
            }
        }
    }

    fn row(&self, ep: &OpenEp, closed: Option<(i64, i64, Censor)>) -> EpisodeWrite {
        let meta = &self.universe.groups[ep.group];
        let s = &ep.peak.signal;
        let q = &s.quote;
        let days = ep.end_ms.map(|e| days_to_end(e, ep.opened_ms));
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
        let min_size = used
            .iter()
            .filter_map(|&i| meta.legs.get(i).and_then(|l| l.min_size))
            .max();
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
                .filter_map(|&i| meta.legs.get(i).and_then(|l| l.seconds_delay))
                .max(),
            days_to_end: days,
            annualized_return: days.and_then(|d| lockup_annualized(q, d)),
            post_end: days.map(|d| d < Decimal::ZERO),
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
            .filter_map(|(k, &i)| {
                // Defensive: `i` indexes the leg list AT OPEN TIME. If the universe refreshed
                // with a shorter leg list before this row was written, skip the row rather than
                // index out of bounds (F5) — `apply_universe` already closes such episodes, so
                // this is a belt-and-suspenders guard, not the primary defense.
                let lm = meta.legs.get(i)?;
                let fill = q.legs.get(k).cloned().unwrap_or_default();
                let (best_price, size_at_best) =
                    ep.peak.best.get(k).copied().unwrap_or((None, None));
                Some(EpisodeLegRow {
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
                })
            })
            .collect();
        EpisodeWrite { row, legs }
    }

    pub fn disconnect(&mut self, conn: usize, ts_ms: i64) -> Vec<Out> {
        // The feed died: drop this conn's deferred top-checks unresolved, no check.
        let gone: Vec<String> = self
            .pending_by_conn
            .get(&conn)
            .map(|set| set.iter().map(|(_, t)| t.clone()).collect())
            .unwrap_or_default();
        for t in gone {
            self.pending_remove(&t);
        }
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
        self.prune_held();
        out
    }

    pub fn close_all(&mut self, ts_ms: i64) -> Vec<Out> {
        self.pending_tops.clear();
        self.pending_by_conn.clear();
        for m in &mut self.pending_by_group {
            m.clear();
        }
        self.held_closed.clear();
        let mut out = Vec::new();
        for e in self.tracker.close_all(ts_ms) {
            self.close_episode(e, &mut out);
        }
        // A suppressed throttle count is a measurement, never silently dropped (review round 1,
        // 2026-09-25): flush every (kind, token) still holding one as its own integrity row, in
        // a deterministic order, then reset it.
        let mut suppressed: Vec<(&'static str, String, u32)> = Vec::new();
        for (kind, by_token) in self.issue_throttle.iter_mut() {
            for (token, t) in by_token.iter_mut() {
                if t.suppressed > 0 {
                    suppressed.push((*kind, token.clone(), t.suppressed));
                    t.suppressed = 0;
                }
            }
        }
        suppressed.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(&b.1)));
        for (kind, token, n) in suppressed {
            let event_id = self
                .token_ix
                .get(&token)
                .map(|&(g, _)| self.universe.groups[g].event_id.clone());
            out.push(self.integrity(
                ts_ms,
                Issue {
                    kind,
                    shard: None,
                    token: Some(token),
                    event_id,
                    detail: Some(format!("+{n} suppressed (at close)")),
                },
            ));
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
            if let Some(t) = self.paper.as_mut() {
                t.on_snapshot(token, &self.groups[g].legs[l].book);
            }
            // A REST snapshot is complete state, superseding any deferred delta top-check.
            self.pending_remove(token);
            // Never move the tracker's clock backwards: a REST snapshot can be older than a ws
            // frame this group already evaluated.
            let eval_ts = ts.max(self.group_last_ts.get(g).copied().unwrap_or(i64::MIN));
            self.evaluate(g, eval_ts, eval_ts, &mut out);
        } else if let Some(b) = self.canary.get_mut(token)
            && ts >= b.server_ts_ms()
        {
            *b = book;
            self.pending_remove(token);
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
            if self
                .token_ix
                .get(&p.token)
                .is_some_and(|&(g, _)| self.group_conn.get(g).copied().flatten().is_none())
            {
                continue; // no connection this refresh (F4): our book can't be trusted as "current"
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

    /// A REST check (drift, L1 mirror) found this event's book wrong at `now_ms`: suspect until
    /// `until_ms`, and flag its open episodes now. Returns their re-emitted rows.
    pub fn mark_suspect(&mut self, event_id: &str, until_ms: i64, now_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(g) = self
            .universe
            .groups
            .iter()
            .position(|m| m.event_id == event_id)
        {
            self.suspect(g, until_ms, now_ms, &mut out);
        }
        out
    }

    pub fn rates(&mut self) -> HashMap<String, u64> {
        std::mem::take(&mut self.msgs)
    }

    pub fn set_paper(&mut self, trader: PaperTrader) {
        self.paper = Some(trader);
    }

    pub fn paper(&self) -> Option<&PaperTrader> {
        self.paper.as_ref()
    }

    /// Sorted market ids the trader holds (the live resolver's question list).
    pub fn paper_held_markets(&self) -> Vec<String> {
        self.paper
            .as_ref()
            .map(|t| t.held_markets().into_iter().collect())
            .unwrap_or_default()
    }

    pub fn paper_state(&self) -> Option<TraderState> {
        self.paper.as_ref().map(PaperTrader::state)
    }

    /// The paper clock (spec 7a §4.2): the instant a live reset closes at.
    pub fn paper_clock(&self) -> i64 {
        self.last_recv_ms
    }

    /// Spec 7a §4.2: a start-time epoch starts the paper clock at `start_ms` (never i64::MIN,
    /// review C3). Never moves the clock back.
    pub fn seed_paper_clock(&mut self, start_ms: i64) {
        self.last_recv_ms = self.last_recv_ms.max(start_ms);
    }

    /// Spec 7a §4.2: a paper reset at `t_ms` (live: the paper clock, captured as
    /// `Ctl::PaperReset { t }`; replay: that payload). Closes the trader at t = max(t_ms, paper
    /// clock) through `paper_close` (conversions complete, pending intents release, unwinds and
    /// markouts drop, every account marks), resets every account in place, then marks every
    /// account at `t + 1`, the epoch's start, which becomes the paper clock. Returns
    /// (start_ms, every row) for ONE `WriterMsg::PaperReset`; None without a trader.
    pub fn paper_reset(&mut self, t_ms: i64) -> Option<(i64, Vec<SimOut>)> {
        self.paper.as_ref()?;
        let mut rows: Vec<SimOut> = self
            .paper_close(t_ms)
            .into_iter()
            .filter_map(|o| match o {
                Out::Paper(s) => Some(*s),
                _ => None,
            })
            .collect();
        let start_ms = self.last_recv_ms + 1;
        let tr = self.paper.as_mut()?;
        let books = EngineBooks {
            token_ix: &self.token_ix,
            groups: &self.groups,
        };
        tr.reset();
        rows.extend(tr.mark_now(start_ms, &books));
        self.last_recv_ms = start_ms;
        Some((start_ms, rows))
    }

    /// Shutdown at `t_ms` (live: the Ctl::Shutdown instant; replay: the same line, or the last
    /// input). S3b contract: `close` must follow `on_frame(t_ms)`, so work due at or before the
    /// close instant first runs at its own due time; then pending conversions complete at
    /// `t_ms`, other pending work is dropped, and every account marks.
    pub fn paper_close(&mut self, t_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        // spec 7a §4.2: at the paper clock, never before it
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        let t_ms = self.last_recv_ms;
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(t_ms, &books), &mut out);
            paper_outs(t.close(t_ms, &books), &mut out);
        }
        out
    }

    /// A market resolved with this payout vector (live: from Gamma via `Ctl::Resolved`; replay:
    /// the same line). A vector other than {1,0}/{0,1} is also a Q19 integrity row. The trader is
    /// first driven to `t_ms` (`on_frame`), so an execution already due settles with the shares
    /// it bought; then every account holding the market settles, and its `AccountRow` values the
    /// account's other open positions on the current books.
    pub fn on_resolution(
        &mut self,
        t_ms: i64,
        market_id: &str,
        payout_yes: Decimal,
        payout_no: Decimal,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        let binary = (payout_yes == Decimal::ONE && payout_no == Decimal::ZERO)
            || (payout_yes == Decimal::ZERO && payout_no == Decimal::ONE);
        if !binary {
            let event_id = self
                .universe
                .groups
                .iter()
                .find(|m| m.legs.iter().any(|l| l.market_id == market_id))
                .map(|m| m.event_id.clone());
            out.push(self.integrity(
                t_ms,
                Issue {
                    kind: "non_binary_payout",
                    event_id,
                    detail: Some(format!(
                        "market {market_id}: payout yes {payout_yes} no {payout_no}"
                    )),
                    ..Issue::default()
                },
            ));
        }
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        let clock = self.last_recv_ms; // spec 7a §4.2: at the paper clock, never before it
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(clock, &books), &mut out);
            paper_outs(
                t.on_resolution(clock, market_id, payout_yes, payout_no, &books),
                &mut out,
            );
        }
        out
    }
}
