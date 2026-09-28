//! The scanner engine's book rules (bins/scanner/src/engine.rs) without detection, integrity rows
//! or the paper trader: which tokens hold a book (live group legs and canary tokens), snapshots,
//! deltas, the deferred server-top trim (engine `pending_tops` / `resolve_pending` /
//! `check_book`), REST snapshots, disconnects, shutdown, and what a universe change carries (books
//! by token) or resets (a group with no connection). bins/scanner/tests/tape_agreement.rs pins
//! these books to the engine's after every line (spec 5b §2, §8).
//!
//! The layout mirrors the engine's own: one book per leg SLOT of each group (engine `groups`), an
//! index of live legs by YES token (engine `token_ix`, last live listing wins), and a separate
//! canary map, so even a token listed twice resolves to the same book the engine uses. Line
//! numbers below refer to engine.rs (and replay.rs for how each capture line reaches the engine).
use crate::frame::{MarketEvent, parse_frame};
use crate::line::{Ctl, Line, ctl_book};
use crate::universe_view::{RecordView, canary_key};
use anyhow::Context;
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Polymarket reports an EMPTY bid side as best_bid "0" and an EMPTY ask side as best_ask "1"
/// (live smoke test 2026-09-25): no real order rests at either, so neither is a level. The
/// engine calls this too, so the two can never disagree about it.
pub fn server_top(
    best_bid: Option<Price>,
    best_ask: Option<Price>,
) -> (Option<Price>, Option<Price>) {
    (
        best_bid.filter(|x| x.get() != Decimal::ZERO),
        best_ask.filter(|x| x.get() != Decimal::ONE),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMeta {
    pub market_id: String,
    pub event_id: String,
    /// "yes" or "no".
    pub outcome: String,
    pub neg_risk: bool,
}

/// A book's levels, best first on each side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    pub bids: Vec<(Price, Shares)>,
    pub asks: Vec<(Price, Shares)>,
}

impl Levels {
    pub fn of(b: &Book) -> Levels {
        Levels {
            bids: b.bids_best_first(),
            asks: b.asks_best_first(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UniverseChange {
    /// Tokens that hold a visible book now and did not before (sorted).
    pub appeared: Vec<String>,
    /// Visible tokens whose book was emptied because their group has no connection (sorted).
    pub reset: Vec<String>,
    /// Tokens that held a visible book before and no longer do (sorted).
    pub gone: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LineEvent {
    #[default]
    None,
    Frame,
    /// A frame that does not parse, or a control line with bad levels.
    BadLine,
    Down(usize),
    Universe(UniverseChange),
    /// The universe record the line names could not be read (the error, for the log).
    UniverseMissing(String),
    Shutdown,
}

/// What one line did to the books.
#[derive(Debug, Default)]
pub struct Applied {
    /// Tokens whose book this line changed (or may have: every delta counts), first touch first.
    pub touched: Vec<String>,
    /// Tokens that received a complete book (a ws snapshot or an accepted REST snapshot).
    pub snapshotted: Vec<String>,
    /// For tokens the caller watches: their levels just before this line's first change.
    pub before: Vec<(String, Levels)>,
    pub event: LineEvent,
}

impl Applied {
    fn touch(&mut self, t: &str) {
        if !self.touched.iter().any(|x| x == t) {
            self.touched.push(t.to_string());
        }
    }

    fn snapshot(&mut self, t: &str) {
        self.touch(t);
        if !self.snapshotted.iter().any(|x| x == t) {
            self.snapshotted.push(t.to_string());
        }
    }

    fn has_before(&self, t: &str) -> bool {
        self.before.iter().any(|(x, _)| x == t)
    }

    pub fn before_of(&self, t: &str) -> Option<&Levels> {
        self.before.iter().find(|(x, _)| x == t).map(|(_, l)| l)
    }
}

/// One leg of a group (engine `GroupLeg`): every leg holds a book, live or not; only live legs
/// with a YES token are indexed, so frames reach only those (engine.rs 645-647).
#[derive(Debug, Clone)]
struct LegSlot {
    yes_token: String,
    live: bool,
    book: Book,
    meta: TokenMeta,
}

#[derive(Debug, Clone)]
struct GroupSlot {
    event_id: String,
    /// engine `group_conn` (engine.rs 690-694): None = no connection this refresh (F4).
    conn: Option<usize>,
    legs: Vec<LegSlot>,
}

#[derive(Debug, Clone)]
struct CanaryTok {
    book: Book,
    /// Not an engine field (the engine keeps canary books only): the canary pair's shard.
    conn: Option<usize>,
    meta: TokenMeta,
}

/// engine `PendingTop` (engine.rs 145-151).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    conn: usize,
    ts: i64,
    bid: Option<Price>,
    ask: Option<Price>,
}

#[derive(Debug, Clone, Default)]
pub struct FeedBooks {
    groups: Vec<GroupSlot>,
    /// Live YES token → (group, leg), the engine's `token_ix`.
    token_ix: HashMap<String, (usize, usize)>,
    canary: HashMap<String, CanaryTok>,
    /// Derived from `groups`: an event's live YES tokens in leg order.
    event_tokens: HashMap<String, Vec<String>>,
    /// engine `pending_tops` and its conn index `pending_by_conn` (engine.rs 176-181).
    pending: BTreeMap<String, Pending>,
    by_conn: HashMap<usize, BTreeSet<(i64, String)>>,
}

fn bump(f: &mut Option<i64>, t: i64) {
    *f = Some(f.map_or(t, |x| x.max(t)));
}

impl FeedBooks {
    pub fn new() -> Self {
        Self::default()
    }

    /// engine `book_of` (engine.rs 1504-1509): a live group leg first, else a canary token.
    pub fn book(&self, t: &str) -> Option<&Book> {
        match self.token_ix.get(t) {
            Some(&(g, l)) => self.leg(g, l).map(|x| &x.book),
            None => self.canary.get(t).map(|x| &x.book),
        }
    }

    /// engine `book_mut` (engine.rs 897-906).
    fn book_mut(&mut self, t: &str) -> Option<&mut Book> {
        match self.token_ix.get(t).copied() {
            Some((g, l)) => self
                .groups
                .get_mut(g)
                .and_then(|x| x.legs.get_mut(l))
                .map(|x| &mut x.book),
            None => self.canary.get_mut(t).map(|x| &mut x.book),
        }
    }

    fn leg(&self, g: usize, l: usize) -> Option<&LegSlot> {
        self.groups.get(g).and_then(|x| x.legs.get(l))
    }

    pub fn meta(&self, t: &str) -> Option<&TokenMeta> {
        match self.token_ix.get(t) {
            Some(&(g, l)) => self.leg(g, l).map(|x| &x.meta),
            None => self.canary.get(t).map(|x| &x.meta),
        }
    }

    pub fn conn(&self, t: &str) -> Option<usize> {
        match self.token_ix.get(t) {
            Some(&(g, _)) => self.groups.get(g).and_then(|x| x.conn),
            None => self.canary.get(t).and_then(|x| x.conn),
        }
    }

    /// Every token that holds a visible book, sorted.
    pub fn visible_tokens(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .token_ix
            .keys()
            .chain(self.canary.keys())
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Visible tokens on connection `conn`, sorted.
    pub fn tokens_on(&self, conn: usize) -> Vec<String> {
        self.visible_tokens()
            .into_iter()
            .filter(|t| self.conn(t) == Some(conn))
            .collect()
    }

    /// The live YES tokens of an event, in leg order.
    pub fn event_tokens(&self, event_id: &str) -> Vec<String> {
        self.event_tokens.get(event_id).cloned().unwrap_or_default()
    }

    /// engine `pending_insert` (engine.rs 453-457): a replace drops the old index key first.
    fn pending_insert(&mut self, t: &str, p: Pending) {
        self.pending_remove(t);
        self.by_conn
            .entry(p.conn)
            .or_default()
            .insert((p.ts, t.to_string()));
        self.pending.insert(t.to_string(), p);
    }

    /// engine `pending_remove` (engine.rs 459-463, 433-449).
    fn pending_remove(&mut self, t: &str) -> Option<Pending> {
        let p = self.pending.remove(t)?;
        if let Some(s) = self.by_conn.get_mut(&p.conn) {
            s.remove(&(p.ts, t.to_string()));
            if s.is_empty() {
                self.by_conn.remove(&p.conn);
            }
        }
        Some(p)
    }

    fn rebuild_by_conn(&mut self) {
        self.by_conn.clear();
        for (t, p) in &self.pending {
            self.by_conn
                .entry(p.conn)
                .or_default()
                .insert((p.ts, t.clone()));
        }
    }

    /// `token_ix` and `event_tokens` from `groups`: the engine indexes a leg when it is live and
    /// has a YES token, in group then leg order, so a later live listing wins (engine.rs 645-647).
    fn reindex(&mut self) {
        self.token_ix.clear();
        self.event_tokens.clear();
        for (g, gs) in self.groups.iter().enumerate() {
            for (l, leg) in gs.legs.iter().enumerate() {
                if leg.live && !leg.yes_token.is_empty() {
                    self.token_ix.insert(leg.yes_token.clone(), (g, l));
                    self.event_tokens
                        .entry(gs.event_id.clone())
                        .or_default()
                        .push(leg.yes_token.clone());
                }
            }
        }
    }

    fn before(&self, t: &str, ap: &mut Applied, watch: &dyn Fn(&str) -> bool) {
        if watch(t)
            && !ap.has_before(t)
            && let Some(b) = self.book(t)
        {
            ap.before.push((t.to_string(), Levels::of(b)));
        }
    }

    /// Apply one capture line, as replay.rs `apply_line` feeds it to the engine. `load` reads the
    /// universe record a `Ctl::Universe` names; `watch` says which tokens need their levels from
    /// before this line (`Applied::before`).
    pub fn apply(
        &mut self,
        line: &Line,
        load: &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>,
        watch: &dyn Fn(&str) -> bool,
    ) -> Applied {
        let mut ap = Applied::default();
        if let Some(raw) = &line.raw {
            // replay.rs: a frame that does not parse is counted and otherwise ignored
            match parse_frame(raw) {
                Ok(evs) => {
                    ap.event = LineEvent::Frame;
                    self.on_frame(line.shard, &evs, &mut ap, watch);
                }
                Err(_) => ap.event = LineEvent::BadLine,
            }
        }
        match &line.ctl {
            // engine `mark_suspect` and `on_resolution` never touch a book
            None | Some(Ctl::Suspect { .. }) | Some(Ctl::Resolved { .. }) => {}
            Some(Ctl::Down) => {
                // engine `disconnect` (engine.rs 1416-1425): this connection's deferred checks
                // are dropped unresolved, no check
                let gone: Vec<String> = self
                    .by_conn
                    .get(&line.shard)
                    .map(|s| s.iter().map(|(_, t)| t.clone()).collect())
                    .unwrap_or_default();
                for t in gone {
                    self.pending_remove(&t);
                }
                ap.event = LineEvent::Down(line.shard);
            }
            // replay stops on an unreadable record; the compactor logs it and carries on
            Some(Ctl::Universe { taken_ms, file }) => match load(*taken_ms, file) {
                Ok(rec) => ap.event = LineEvent::Universe(self.apply_universe(&rec)),
                Err(e) => ap.event = LineEvent::UniverseMissing(format!("{e:#}")),
            },
            Some(Ctl::RestSnapshot {
                token,
                server_ts_ms,
                bids,
                asks,
            }) => match ctl_book(bids, asks, *server_ts_ms) {
                Ok(b) => self.rest_snapshot(token, b, &mut ap, watch),
                Err(_) => ap.event = LineEvent::BadLine,
            },
            Some(Ctl::Shutdown) => {
                // engine `close_all` (engine.rs 1450-1454) clears every deferred check
                self.pending.clear();
                self.by_conn.clear();
                ap.event = LineEvent::Shutdown;
            }
        }
        ap
    }

    /// engine `on_frame` (engine.rs 727-859), books only.
    fn on_frame(
        &mut self,
        conn: usize,
        evs: &[MarketEvent],
        ap: &mut Applied,
        watch: &dyn Fn(&str) -> bool,
    ) {
        let mut frame_ts: Option<i64> = None;
        for ev in evs {
            match ev {
                MarketEvent::Snapshot {
                    token,
                    server_ts_ms,
                    bids,
                    asks,
                    ..
                } => {
                    // engine.rs 750-770
                    bump(&mut frame_ts, *server_ts_ms);
                    if self.book(token).is_some() {
                        self.before(token, ap, watch);
                        if let Some(b) = self.book_mut(token) {
                            b.apply_snapshot(bids, asks, *server_ts_ms);
                        }
                        self.pending_remove(token); // a snapshot supersedes earlier deltas' tops
                        ap.snapshot(token);
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
                    // engine.rs 771-818
                    bump(&mut frame_ts, *server_ts_ms);
                    let (bb, ba) = server_top(*best_bid, *best_ask);
                    let ts = *server_ts_ms;
                    // a pending check from an EARLIER server ts belongs to a finished update:
                    // resolve it against the book as it stood before this delta
                    if self.pending.get(token.as_str()).is_some_and(|p| p.ts < ts) {
                        self.resolve(token, ap, watch);
                    }
                    if self.book(token).is_some() {
                        self.before(token, ap, watch);
                        if let Some(b) = self.book_mut(token) {
                            let _ = b.apply_delta(*side, *price, *size, ts);
                        }
                        ap.touch(token);
                        // same-ts deltas replace the pending entry: last wins
                        self.pending_insert(
                            token,
                            Pending {
                                conn,
                                ts,
                                bid: bb,
                                ask: ba,
                            },
                        );
                    }
                }
                MarketEvent::LastTrade { server_ts_ms, .. }
                | MarketEvent::TickSize { server_ts_ms, .. } => bump(&mut frame_ts, *server_ts_ms),
                MarketEvent::Resolved { .. }
                | MarketEvent::NewMarket { .. }
                | MarketEvent::Other { .. } => {}
            }
        }
        let Some(ts) = frame_ts else {
            return;
        };
        // engine.rs 834-850: checks on this connection more than 1 s older than this frame
        // settle now, in token order. (The engine's end-of-frame `check_book` for snapshotted
        // tokens, engine.rs 851-853, passes no server top: crossed check only, no book change.)
        let mut due: Vec<String> = self
            .by_conn
            .get(&conn)
            .map(|s| {
                s.range(..(ts.saturating_sub(1_000), String::new()))
                    .map(|(_, t)| t.clone())
                    .collect()
            })
            .unwrap_or_default();
        due.sort_unstable();
        for t in due {
            self.resolve(&t, ap, watch);
        }
    }

    /// engine `resolve_pending` + `check_book` (engine.rs 890-895, 935-1031), books only: the
    /// server's final top trims the levels it disproves.
    fn resolve(&mut self, t: &str, ap: &mut Applied, watch: &dyn Fn(&str) -> bool) {
        let Some(p) = self.pending_remove(t) else {
            return;
        };
        // engine.rs 944-949: a group with no connection this refresh is never checked (F4)
        if let Some(&(g, _)) = self.token_ix.get(t)
            && self.groups.get(g).and_then(|x| x.conn).is_none()
        {
            return;
        }
        let Some(book) = self.book(t) else {
            return;
        };
        let before = (watch(t) && !ap.has_before(t)).then(|| Levels::of(book));
        // The engine trims only on a top mismatch (engine.rs 956-981); without one, trim_to_top
        // removes nothing (a matching or absent server price disproves no level), so trimming
        // unconditionally gives the same book.
        let changed = self
            .book_mut(t)
            .is_some_and(|b| b.trim_to_top(p.bid, p.ask));
        if changed {
            if let Some(l) = before {
                ap.before.push((t.to_string(), l));
            }
            ap.touch(t);
        }
    }

    /// engine `rest_snapshot` (engine.rs 1511-1535): replaces the book unless it is older than
    /// what we hold, superseding any deferred check.
    fn rest_snapshot(
        &mut self,
        t: &str,
        book: Book,
        ap: &mut Applied,
        watch: &dyn Fn(&str) -> bool,
    ) {
        let Some(cur) = self.book(t) else {
            return;
        };
        if book.server_ts_ms() < cur.server_ts_ms() {
            return;
        }
        self.before(t, ap, watch);
        if let Some(b) = self.book_mut(t) {
            *b = book;
        }
        self.pending_remove(t);
        ap.snapshot(t);
    }

    /// engine `apply_universe` (engine.rs 527-725), books only.
    fn apply_universe(&mut self, rec: &RecordView) -> UniverseChange {
        let was: BTreeSet<String> = self.visible_tokens().into_iter().collect();
        // engine.rs 603-613: every old leg with a YES token offers its book, live or not; a
        // later listing of the same token overwrites an earlier one
        let mut old_books: HashMap<String, Book> = HashMap::new();
        for g in std::mem::take(&mut self.groups) {
            for leg in g.legs {
                if !leg.yes_token.is_empty() {
                    old_books.insert(leg.yes_token, leg.book);
                }
            }
        }
        // engine.rs 637-667: the first listing takes the carried book; a group with no
        // connection this refresh gets an empty one (F4)
        for gv in &rec.universe.groups {
            let conn = rec.conn_of.get(&gv.event_id).copied();
            let legs = gv
                .legs
                .iter()
                .map(|leg| {
                    let carried = old_books.remove(&leg.yes_token);
                    LegSlot {
                        yes_token: leg.yes_token.clone(),
                        live: leg.status == "live",
                        book: match conn {
                            Some(_) => carried.unwrap_or_default(),
                            None => Book::default(),
                        },
                        meta: TokenMeta {
                            market_id: leg.market_id.clone(),
                            event_id: gv.event_id.clone(),
                            outcome: "yes".into(),
                            neg_risk: gv.neg_risk_market_id.is_some(),
                        },
                    }
                })
                .collect();
            self.groups.push(GroupSlot {
                event_id: gv.event_id.clone(),
                conn,
                legs,
            });
        }
        self.reindex();
        // engine.rs 668-675: canary books carry by token, a repeated token gets an empty one
        let mut old_canary = std::mem::take(&mut self.canary);
        for c in &rec.universe.canary {
            let conn = rec.conn_of.get(&canary_key(&c.market_id)).copied();
            for (t, outcome) in [(&c.yes_token, "yes"), (&c.no_token, "no")] {
                let book = old_canary.remove(t).map(|x| x.book).unwrap_or_default();
                self.canary.insert(
                    t.clone(),
                    CanaryTok {
                        book,
                        conn,
                        meta: TokenMeta {
                            market_id: c.market_id.clone(),
                            event_id: c.event_id.clone(),
                            outcome: outcome.into(),
                            neg_risk: false,
                        },
                    },
                );
            }
        }
        // engine.rs 680-688: deferred checks survive only for tokens still subscribed
        let (ix, canary) = (&self.token_ix, &self.canary);
        self.pending
            .retain(|t, _| ix.contains_key(t) || canary.contains_key(t));
        self.rebuild_by_conn();
        let now: BTreeSet<String> = self.visible_tokens().into_iter().collect();
        let mut reset: Vec<String> = self
            .token_ix
            .iter()
            .filter(|(_, (g, _))| self.groups.get(*g).is_some_and(|x| x.conn.is_none()))
            .map(|(t, _)| t.clone())
            .collect();
        reset.sort();
        UniverseChange {
            appeared: now.difference(&was).cloned().collect(),
            reset,
            gone: was.difference(&now).cloned().collect(),
        }
    }

    pub fn to_state(&self) -> FeedState {
        let groups = self
            .groups
            .iter()
            .map(|g| GroupState {
                event_id: g.event_id.clone(),
                conn: g.conn,
                legs: g
                    .legs
                    .iter()
                    .map(|l| LegState {
                        yes_token: l.yes_token.clone(),
                        live: l.live,
                        book: BookState::of(&l.book),
                        meta: l.meta.clone(),
                    })
                    .collect(),
            })
            .collect();
        let mut canary: Vec<CanaryState> = self
            .canary
            .iter()
            .map(|(t, x)| CanaryState {
                token: t.clone(),
                book: BookState::of(&x.book),
                conn: x.conn,
                meta: x.meta.clone(),
            })
            .collect();
        canary.sort_by(|a, b| a.token.cmp(&b.token));
        let pending = self
            .pending
            .iter()
            .map(|(t, p)| PendingState {
                token: t.clone(),
                conn: p.conn,
                ts: p.ts,
                bid: p.bid.map(|x| x.get().to_string()),
                ask: p.ask.map(|x| x.get().to_string()),
            })
            .collect();
        FeedState {
            groups,
            canary,
            pending,
        }
    }

    pub fn from_state(s: &FeedState) -> anyhow::Result<FeedBooks> {
        let price = |v: &Option<String>| -> anyhow::Result<Option<Price>> {
            Ok(v.as_deref().map(Price::parse).transpose()?)
        };
        let mut f = FeedBooks::new();
        for g in &s.groups {
            let mut legs = Vec::with_capacity(g.legs.len());
            for l in &g.legs {
                legs.push(LegSlot {
                    yes_token: l.yes_token.clone(),
                    live: l.live,
                    book: l
                        .book
                        .to_book()
                        .with_context(|| format!("book of {}", l.yes_token))?,
                    meta: l.meta.clone(),
                });
            }
            f.groups.push(GroupSlot {
                event_id: g.event_id.clone(),
                conn: g.conn,
                legs,
            });
        }
        f.reindex();
        for c in &s.canary {
            f.canary.insert(
                c.token.clone(),
                CanaryTok {
                    book: c
                        .book
                        .to_book()
                        .with_context(|| format!("book of {}", c.token))?,
                    conn: c.conn,
                    meta: c.meta.clone(),
                },
            );
        }
        for p in &s.pending {
            f.pending.insert(
                p.token.clone(),
                Pending {
                    conn: p.conn,
                    ts: p.ts,
                    bid: price(&p.bid).with_context(|| format!("pending bid of {}", p.token))?,
                    ask: price(&p.ask).with_context(|| format!("pending ask of {}", p.token))?,
                },
            );
        }
        f.rebuild_by_conn();
        Ok(f)
    }
}

/// `FeedBooks` as plain data for the compactor's checkpoints (exact decimal strings; groups and
/// legs in universe order, canary and pending sorted by token).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedState {
    pub groups: Vec<GroupState>,
    pub canary: Vec<CanaryState>,
    pub pending: Vec<PendingState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupState {
    pub event_id: String,
    pub conn: Option<usize>,
    pub legs: Vec<LegState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegState {
    pub yes_token: String,
    pub live: bool,
    pub book: BookState,
    pub meta: TokenMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryState {
    pub token: String,
    pub book: BookState,
    pub conn: Option<usize>,
    pub meta: TokenMeta,
}

/// A book's levels best first, as exact decimal strings (the `Ctl::RestSnapshot` form).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookState {
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
    pub server_ts_ms: i64,
}

impl BookState {
    fn of(b: &Book) -> BookState {
        let lv = |v: Vec<(Price, Shares)>| -> Vec<(String, String)> {
            v.into_iter()
                .map(|(p, s)| (p.get().to_string(), s.get().to_string()))
                .collect()
        };
        BookState {
            bids: lv(b.bids_best_first()),
            asks: lv(b.asks_best_first()),
            server_ts_ms: b.server_ts_ms(),
        }
    }

    fn to_book(&self) -> anyhow::Result<Book> {
        ctl_book(&self.bids, &self.asks, self.server_ts_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingState {
    pub token: String,
    pub conn: usize,
    pub ts: i64,
    pub bid: Option<String>,
    pub ask: Option<String>,
}
