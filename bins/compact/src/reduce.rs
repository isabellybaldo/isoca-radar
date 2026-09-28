//! One raw hour → archive rows (spec 5b §3, §4.1). Pure: no files, no database. Lines go through
//! tape::feed::FeedBooks, the scanner's own book rules (spec §2). On top of it this keeps which
//! books are trusted (untrusted from a disconnect, a restart, a missing or lost stretch, or a
//! token's first appearance, until the feed sends it a full snapshot: no row is ever made from a
//! stale book), the day's token dictionary, top-of-book changes, periodic depth, and the gaps.
//! Every stretch in which a visible token is untrusted lies inside a gap row. Inside an episode
//! window (spec §3) each token of the episode's event gets its exact level changes (`l2`) under
//! the same trust rules: it starts with a full snapshot and restarts with one whenever it becomes
//! trusted again or its owning episode changes.
//!
//! Reading l2 per token: the `snapshot = true` rows sharing one `recv_ms` replace the token's
//! whole book; every other row sets one level (`new_size` 0 = the level is gone). A token's
//! `ts_ms` never goes back.
use crate::rows::{
    DepthRow, GapRow, L2Row, SIDE_ASK, SIDE_BID, Sink, TobRow, TokenRow, micros, reason,
};
use crate::windows::Window;
use anyhow::Context;
use arb_core::book::Top;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tape::feed::{Applied, FeedBooks, FeedState, Levels, LineEvent, TokenMeta};
use tape::line::{Ctl, Line};
use tape::time::{HOUR_MS, day_of};
use tape::universe_view::RecordView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceCfg {
    pub depth_every_ms: i64,
    pub depth_levels: usize,
}

/// A top of book in micro-units: ((bid, bid size), (ask, ask size)).
pub type TopMicros = (Option<(i64, i64)>, Option<(i64, i64)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastTop {
    pub top: TopMicros,
    pub ts_ms: i64,
}

/// A shard's tokens waiting for a snapshot since `since_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenGap {
    pub reason: String,
    pub since_ms: i64,
    pub waiting: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourStats {
    pub lines: u64,
    pub bad_lines: u64,
    pub ts_clamped: u64,
    pub universe_missing: u64,
}

/// What a checkpoint carries from one hour to the next (spec §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReducerState {
    pub feed: FeedState,
    pub started: bool,
    pub trusted: BTreeSet<String>,
    pub dict_day_ms: i64,
    pub dict: Vec<TokenRow>,
    pub last_top: BTreeMap<String, LastTop>,
    pub changed: BTreeSet<String>,
    pub next_depth_ms: Option<i64>,
    pub clock_ms: i64,
    pub shard_gaps: BTreeMap<usize, OpenGap>,
    pub global_gap: Option<(String, i64)>,
    /// The last shard each visible token was on: a token whose group lost its connection waits
    /// (and is covered by a `down` gap) there.
    pub last_conn: BTreeMap<String, usize>,
    /// Since when the universe is unknown (a record could not be read): a whole-feed
    /// `raw_hour_lost` gap until the next record is applied.
    pub universe_lost: Option<i64>,
    /// Each token's latest l2 ts_ms: its l2 rows never go back, across hours too.
    #[serde(default)]
    pub l2_ts: BTreeMap<String, i64>,
}

pub struct Reducer {
    cfg: ReduceCfg,
    feed: FeedBooks,
    started: bool,
    trusted: BTreeSet<String>,
    dict_day_ms: i64,
    dict: Vec<TokenRow>,
    idx: HashMap<String, u32>,
    last_top: BTreeMap<String, LastTop>,
    changed: BTreeSet<String>,
    next_depth_ms: Option<i64>,
    clock_ms: i64,
    shard_gaps: BTreeMap<usize, OpenGap>,
    global_gap: Option<(String, i64)>,
    last_conn: BTreeMap<String, usize>,
    universe_lost: Option<i64>,
    l2_ts: BTreeMap<String, i64>,
    // per hour, never checkpointed
    hour_ms: i64,
    windows: Vec<Window>,
    /// token → index in `windows` of the episode that owns its l2 rows now.
    owner: HashMap<String, usize>,
    /// Owned tokens whose owner has not had a non-empty snapshot from a trusted book yet.
    needs_snap: BTreeSet<String>,
    /// The next window start or end: owners are recomputed when the clock reaches it.
    next_owner_ms: i64,
    /// The levels last written to l2 for each token this hour (a reader's view of its book).
    shown: HashMap<String, Levels>,
    stats: HourStats,
    /// This line carried a value that does not fit the archive (counted once as a bad line).
    unfit: bool,
}

/// What must stay equal across a universe line for a trusted book to stay trusted.
type Mark = (Levels, i64, TokenMeta);

fn top_micros(t: Top) -> anyhow::Result<TopMicros> {
    let pair = |x: Option<(Price, Shares)>| -> anyhow::Result<Option<(i64, i64)>> {
        x.map(|(p, s)| Ok((micros(p.get())?, micros(s.get())?)))
            .transpose()
    };
    Ok((pair(t.best_bid)?, pair(t.best_ask)?))
}

/// Emit a gap clipped to the hour being built: every hour's parts stand on their own.
fn gap(
    hour_ms: i64,
    start: i64,
    end: i64,
    shard: Option<usize>,
    why: &str,
    sink: &mut dyn Sink,
) -> anyhow::Result<()> {
    let (s, e) = (start.max(hour_ms), end.min(hour_ms + HOUR_MS));
    if e > s {
        sink.gap(GapRow {
            start_ms: s,
            end_ms: e,
            shard: shard.map(|c| i32::try_from(c).unwrap_or(i32::MAX)),
            reason: why.to_string(),
        })?;
    }
    Ok(())
}

/// Levels whose size differs between two states of one book, bids then asks, prices ascending.
/// `None` = the level is gone (written as new_size 0).
fn diff(before: &Levels, now: &Levels) -> Vec<(u8, Price, Option<Shares>)> {
    let mut out = Vec::new();
    for (side, b, n) in [
        (SIDE_BID, &before.bids, &now.bids),
        (SIDE_ASK, &before.asks, &now.asks),
    ] {
        let bm: BTreeMap<Price, Shares> = b.iter().copied().collect();
        let nm: BTreeMap<Price, Shares> = n.iter().copied().collect();
        let prices: BTreeSet<Price> = bm.keys().chain(nm.keys()).copied().collect();
        for p in prices {
            let (x, y) = (bm.get(&p), nm.get(&p));
            if x != y {
                out.push((side, p, y.copied()));
            }
        }
    }
    out
}

/// `diff` in micro-units: (side, price, new_size), 0 for a level that is gone.
fn diff_rows(before: &Levels, now: &Levels) -> anyhow::Result<Vec<(u8, i64, i64)>> {
    diff(before, now)
        .into_iter()
        .map(|(side, p, s)| {
            Ok((
                side,
                micros(p.get())?,
                s.map_or(Ok(0), |x| micros(x.get()))?,
            ))
        })
        .collect()
}

impl Reducer {
    pub fn new(cfg: ReduceCfg) -> Reducer {
        Reducer {
            cfg,
            feed: FeedBooks::new(),
            started: false,
            trusted: BTreeSet::new(),
            dict_day_ms: i64::MIN,
            dict: Vec::new(),
            idx: HashMap::new(),
            last_top: BTreeMap::new(),
            changed: BTreeSet::new(),
            next_depth_ms: None,
            clock_ms: i64::MIN,
            shard_gaps: BTreeMap::new(),
            global_gap: None,
            last_conn: BTreeMap::new(),
            universe_lost: None,
            l2_ts: BTreeMap::new(),
            hour_ms: 0,
            windows: Vec::new(),
            owner: HashMap::new(),
            needs_snap: BTreeSet::new(),
            next_owner_ms: i64::MIN,
            shown: HashMap::new(),
            stats: HourStats::default(),
            unfit: false,
        }
    }

    pub fn from_state(cfg: ReduceCfg, s: ReducerState) -> anyhow::Result<Reducer> {
        let mut r = Reducer::new(cfg);
        r.feed = FeedBooks::from_state(&s.feed).context("checkpoint books")?;
        r.started = s.started;
        r.trusted = s.trusted;
        r.dict_day_ms = s.dict_day_ms;
        r.idx = s
            .dict
            .iter()
            .map(|t| (t.token_id.clone(), t.token_idx))
            .collect();
        r.dict = s.dict;
        r.last_top = s.last_top;
        r.changed = s.changed;
        r.next_depth_ms = s.next_depth_ms;
        r.clock_ms = s.clock_ms;
        r.shard_gaps = s.shard_gaps;
        r.global_gap = s.global_gap;
        r.last_conn = s.last_conn;
        r.universe_lost = s.universe_lost;
        r.l2_ts = s.l2_ts;
        Ok(r)
    }

    pub fn state(&self) -> ReducerState {
        ReducerState {
            feed: self.feed.to_state(),
            started: self.started,
            trusted: self.trusted.clone(),
            dict_day_ms: self.dict_day_ms,
            dict: self.dict.clone(),
            last_top: self.last_top.clone(),
            changed: self.changed.clone(),
            next_depth_ms: self.next_depth_ms,
            clock_ms: self.clock_ms,
            shard_gaps: self.shard_gaps.clone(),
            global_gap: self.global_gap.clone(),
            last_conn: self.last_conn.clone(),
            universe_lost: self.universe_lost,
            l2_ts: self.l2_ts.clone(),
        }
    }

    /// The day's token dictionary so far (index = token_idx).
    pub fn tokens(&self) -> &[TokenRow] {
        &self.dict
    }

    /// The latest receive time seen (ms).
    pub fn clock_ms(&self) -> i64 {
        self.clock_ms
    }

    pub fn count_bad_line(&mut self) {
        self.stats.bad_lines += 1;
    }

    pub fn begin_hour(&mut self, hour_ms: i64, mut windows: Vec<Window>) {
        self.hour_ms = hour_ms;
        let day = day_of(hour_ms);
        if day != self.dict_day_ms {
            self.dict.clear();
            self.idx.clear();
            self.dict_day_ms = day;
        }
        windows.sort_by(|a, b| {
            a.opened_ms
                .cmp(&b.opened_ms)
                .then_with(|| a.episode_id.cmp(&b.episode_id))
        });
        self.windows = windows;
        self.owner.clear();
        self.needs_snap.clear();
        self.shown.clear();
        self.next_owner_ms = i64::MIN; // every hour restarts its windows with snapshots (ruling R5)
        self.stats = HourStats::default();
        if !self.started {
            // cold start: nothing is known until the first line (and books until snapshots)
            self.started = true;
            self.global_gap = Some((reason::NO_BOOK_YET.to_string(), hour_ms));
        }
    }

    /// One capture line of the hour, in file order.
    pub fn line(
        &mut self,
        line: &Line,
        load: &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>,
        sink: &mut dyn Sink,
    ) -> anyhow::Result<()> {
        let r = line.recv_ts_us.div_euclid(1000);
        self.stats.lines += 1;
        self.clock_ms = self.clock_ms.max(r);
        self.unfit = false;
        if let Some((why, since)) = self.global_gap.take() {
            gap(self.hour_ms, since, r, None, &why, sink)?;
            self.wait_for_untrusted(r);
        }
        self.depth_due(r, sink)?;
        // A universe line can hand a still-visible token a different (empty) book: a token live
        // in two groups, or one moving between canary and a group leg. FeedBooks reports neither
        // as a reset (it mirrors the engine), so compare every trusted book across the line.
        let marks: Vec<(String, Mark)> = if matches!(line.ctl, Some(Ctl::Universe { .. })) {
            self.trusted
                .iter()
                .filter_map(|t| self.mark(t).map(|m| (t.clone(), m)))
                .collect()
        } else {
            Vec::new()
        };
        self.owners_due(r, sink)?;
        let ap = self.feed.apply(line, load, &|_| false);
        for (t, m) in marks {
            if self.mark(&t).as_ref() != Some(&m) {
                self.trusted.remove(&t);
            }
        }
        self.on_event(&ap, r, sink)?;
        self.rows(&ap, r, sink)?;
        if self.unfit && ap.event != LineEvent::BadLine {
            self.stats.bad_lines += 1;
        }
        Ok(())
    }

    fn mark(&self, t: &str) -> Option<Mark> {
        let b = self.feed.book(t)?;
        Some((Levels::of(b), b.server_ts_ms(), self.feed.meta(t)?.clone()))
    }

    fn rows(&mut self, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if self.universe_lost.is_none() {
            for t in &ap.snapshotted {
                self.retrust(t, r, sink)?;
            }
        }
        for t in &ap.touched {
            if self.trusted.contains(t) {
                self.changed.insert(t.clone());
                self.tob(t, r, sink)?;
            }
            // after tob, which may have found an unfit value and untrusted the token
            if self.trusted.contains(t) {
                self.l2(t, r, sink)?;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        match &ap.event {
            LineEvent::Down(conn) => {
                for t in self.feed.tokens_on(*conn) {
                    self.trusted.remove(&t);
                    self.wait(*conn, &t, reason::DOWN, r);
                }
            }
            LineEvent::Universe(ch) => {
                self.close_universe_gap(r, sink)?;
                self.next_owner_ms = i64::MIN; // an event's tokens may have changed
                for t in &ch.gone {
                    self.forget(t, r, sink)?;
                }
                for t in ch.appeared.iter().chain(ch.reset.iter()) {
                    self.trusted.remove(t);
                }
                // before the shards are updated: a token that lost its connection waits on its
                // last one
                self.wait_for_untrusted(r);
                for t in self.feed.visible_tokens() {
                    if let Some(c) = self.feed.conn(&t) {
                        self.last_conn.insert(t, c);
                    }
                }
            }
            LineEvent::UniverseMissing(_) => {
                // the universe is unknown until the next record: the whole feed is lost
                self.stats.universe_missing += 1;
                self.close_shard_gaps(r, sink)?;
                self.trusted.clear();
                if self.universe_lost.is_none() {
                    self.universe_lost = Some(r);
                }
            }
            LineEvent::Shutdown => {
                // the shard gaps and the whole-feed gap meet at r: no instant is left uncovered
                self.close_shard_gaps(r, sink)?;
                self.close_universe_gap(r, sink)?;
                self.trusted.clear();
                if self.global_gap.is_none() {
                    self.global_gap = Some((reason::SCANNER_STOPPED.to_string(), r));
                }
            }
            LineEvent::BadLine => self.stats.bad_lines += 1,
            LineEvent::None | LineEvent::Frame => {}
        }
        Ok(())
    }

    fn wait(&mut self, conn: usize, t: &str, why: &str, r: i64) {
        if self.universe_lost.is_some() {
            return; // inside a whole-feed gap
        }
        self.shard_gaps
            .entry(conn)
            .or_insert_with(|| OpenGap {
                reason: why.to_string(),
                since_ms: r,
                waiting: BTreeSet::new(),
            })
            .waiting
            .insert(t.to_string());
    }

    /// An untrusted token waits for its snapshot on its shard; with no connection now, on the
    /// last shard it was on, as `down`.
    fn wait_one(&mut self, t: &str, r: i64) {
        if let Some(c) = self.feed.conn(t) {
            self.wait(c, t, reason::NO_BOOK_YET, r);
        } else if let Some(&c) = self.last_conn.get(t) {
            self.wait(c, t, reason::DOWN, r);
        }
    }

    /// Every visible, untrusted token waits for its snapshot.
    fn wait_for_untrusted(&mut self, r: i64) {
        for t in self.feed.visible_tokens() {
            if !self.trusted.contains(&t) {
                self.wait_one(&t, r);
            }
        }
    }

    /// A token whose book holds a value the archive cannot store: no row, a bad line, and the
    /// token waits for its next snapshot.
    fn mark_unfit(&mut self, t: &str, r: i64) {
        self.unfit = true;
        self.trusted.remove(t);
        self.wait_one(t, r);
    }

    fn stop_waiting(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let mut closed = Vec::new();
        for (c, g) in self.shard_gaps.iter_mut() {
            if g.waiting.remove(t) && g.waiting.is_empty() {
                closed.push(*c);
            }
        }
        for c in closed {
            if let Some(g) = self.shard_gaps.remove(&c) {
                gap(self.hour_ms, g.since_ms, r, Some(c), &g.reason, sink)?;
            }
        }
        Ok(())
    }

    fn retrust(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if self.trusted.insert(t.to_string()) {
            self.changed.insert(t.to_string());
            if self.owner.contains_key(t) {
                self.needs_snap.insert(t.to_string()); // its l2 stream restarts
            }
            self.stop_waiting(t, r, sink)?;
        }
        Ok(())
    }

    fn forget(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        self.trusted.remove(t);
        self.changed.remove(t);
        self.last_conn.remove(t);
        self.owner.remove(t);
        self.needs_snap.remove(t);
        self.shown.remove(t);
        self.l2_ts.remove(t);
        self.stop_waiting(t, r, sink)
    }

    fn close_shard_gaps(&mut self, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        for (c, g) in std::mem::take(&mut self.shard_gaps) {
            gap(self.hour_ms, g.since_ms, at, Some(c), &g.reason, sink)?;
        }
        Ok(())
    }

    fn close_universe_gap(&mut self, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if let Some(since) = self.universe_lost.take() {
            gap(self.hour_ms, since, at, None, reason::RAW_HOUR_LOST, sink)?;
        }
        Ok(())
    }

    fn index(&mut self, t: &str) -> anyhow::Result<u32> {
        if let Some(&i) = self.idx.get(t) {
            return Ok(i);
        }
        let meta = self
            .feed
            .meta(t)
            .cloned()
            .with_context(|| format!("token {t} has no metadata"))?;
        let i = u32::try_from(self.dict.len()).context("token dictionary overflow")?;
        self.dict.push(TokenRow {
            token_idx: i,
            token_id: t.to_string(),
            market_id: meta.market_id,
            event_id: meta.event_id,
            outcome: meta.outcome,
            neg_risk: meta.neg_risk,
        });
        self.idx.insert(t.to_string(), i);
        Ok(i)
    }

    fn tob(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let ts = book.server_ts_ms();
        let Ok(top) = top_micros(book.top()) else {
            self.mark_unfit(t, r);
            return Ok(());
        };
        let prev = self.last_top.get(t).copied();
        if prev.is_some_and(|p| p.top == top) {
            return Ok(());
        }
        let ts_ms = match prev {
            Some(p) if ts < p.ts_ms => {
                self.stats.ts_clamped += 1;
                p.ts_ms
            }
            _ => ts,
        };
        let token_idx = self.index(t)?;
        sink.tob(TobRow {
            ts_ms,
            recv_ms: r,
            token_idx,
            bid: top.0.map(|x| x.0),
            bid_sz: top.0.map(|x| x.1),
            ask: top.1.map(|x| x.0),
            ask_sz: top.1.map(|x| x.1),
        })?;
        self.last_top.insert(t.to_string(), LastTop { top, ts_ms });
        Ok(())
    }

    /// When the receive clock (now at line time `r`) has crossed a depth boundary: the books as
    /// of the first boundary crossed, which is the first after every pending change.
    fn depth_due(&mut self, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let every = self.cfg.depth_every_ms;
        let after_clock = (self.clock_ms.div_euclid(every) + 1) * every;
        let Some(at) = self.next_depth_ms else {
            self.next_depth_ms = Some(after_clock);
            return Ok(());
        };
        if self.clock_ms < at {
            return Ok(());
        }
        let toks: Vec<String> = self
            .changed
            .iter()
            .filter(|t| self.trusted.contains(*t))
            .cloned()
            .collect();
        for t in toks {
            self.depth_rows(&t, at, r, sink)?;
        }
        self.changed.clear();
        self.next_depth_ms = Some(after_clock);
        Ok(())
    }

    fn depth_rows(&mut self, t: &str, at: i64, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let n = self.cfg.depth_levels;
        let sides = [
            (SIDE_BID, book.bids_best_first()),
            (SIDE_ASK, book.asks_best_first()),
        ];
        let mut levels = Vec::new();
        for (side, lv) in sides {
            for (level, (p, s)) in lv.into_iter().take(n).enumerate() {
                match (micros(p.get()), micros(s.get())) {
                    (Ok(price), Ok(size)) => levels.push((side, level, price, size)),
                    _ => {
                        self.mark_unfit(t, r);
                        return Ok(());
                    }
                }
            }
        }
        let token_idx = self.index(t)?;
        for (side, level, price, size) in levels {
            sink.depth(DepthRow {
                ts_ms: at,
                token_idx,
                side,
                level: u16::try_from(level)?,
                price,
                size,
            })?;
        }
        Ok(())
    }

    /// Recompute which episode owns each token when a window starts or ends: the first window
    /// (by open, then id) covering the clock whose event lists the token. A token that gets a new
    /// owner starts with a snapshot under it, from a trusted book only: an untrusted one waits
    /// for its next feed snapshot. Windows are in server ms (episode open/close); ownership
    /// switches on the receive clock, at the first line at or past a boundary.
    fn owners_due(&mut self, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if self.clock_ms < self.next_owner_ms {
            return Ok(());
        }
        let now = self.clock_ms;
        let mut owner: HashMap<String, usize> = HashMap::new();
        let mut next = i64::MAX;
        for (i, w) in self.windows.iter().enumerate() {
            if w.start_ms > now {
                next = next.min(w.start_ms);
                continue;
            }
            if w.end_ms <= now {
                continue;
            }
            next = next.min(w.end_ms);
            for t in self.feed.event_tokens(&w.event_id) {
                owner.entry(t).or_insert(i);
            }
        }
        self.next_owner_ms = next;
        let mut handover: Vec<(String, usize)> = owner
            .iter()
            .filter(|(t, w)| self.owner.get(*t) != Some(*w))
            .map(|(t, w)| (t.clone(), *w))
            .collect();
        handover.sort();
        self.needs_snap.retain(|t| owner.contains_key(t));
        self.owner = owner;
        for (t, w) in handover {
            if self.trusted.contains(&t) {
                self.l2_restart(&t, w, r, sink)?;
            } else {
                self.needs_snap.insert(t);
            }
        }
        Ok(())
    }

    /// A trusted, touched token's l2 rows. A token already streaming under its owner gets the
    /// exact difference from the levels last written for it (snapshot = false), however the feed
    /// sent the change (deltas or a full book); a token (re)starting gets snapshot rows.
    fn l2(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(&w) = self.owner.get(t) else {
            return Ok(());
        };
        if self.needs_snap.contains(t) {
            return self.l2_restart(t, w, r, sink);
        }
        let Some(shown) = self.shown.get(t) else {
            return self.l2_restart(t, w, r, sink);
        };
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let (now, ts) = (Levels::of(book), book.server_ts_ms());
        let Ok(rows) = diff_rows(shown, &now) else {
            self.mark_unfit(t, r);
            return Ok(());
        };
        self.shown.insert(t.to_string(), now);
        self.emit_l2(t, w, (ts, r, false), rows, sink)
    }

    /// (Re)start a token's stream under window `w`: one `snapshot = true` row per level, stamped
    /// no earlier than the window's start. An empty book waits for its first level (ruling R5);
    /// levels written for the token earlier this hour are first zeroed, so none outlives it.
    fn l2_restart(&mut self, t: &str, w: usize, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let (lv, ts) = (Levels::of(book), book.server_ts_ms());
        if lv.bids.is_empty() && lv.asks.is_empty() {
            self.needs_snap.insert(t.to_string());
            if let Some(prev) = self.shown.insert(t.to_string(), lv.clone()) {
                // these levels were written, so they fit
                let rows = diff_rows(&prev, &lv)?;
                self.emit_l2(t, w, (ts, r, false), rows, sink)?;
            }
            return Ok(());
        }
        let mut rows = Vec::new();
        for (side, levels) in [(SIDE_BID, &lv.bids), (SIDE_ASK, &lv.asks)] {
            for (p, s) in levels {
                match (micros(p.get()), micros(s.get())) {
                    (Ok(price), Ok(size)) => rows.push((side, price, size)),
                    _ => {
                        // no row: the token waits for its next feed snapshot, which restarts it
                        self.mark_unfit(t, r);
                        return Ok(());
                    }
                }
            }
        }
        self.needs_snap.remove(t);
        self.shown.insert(t.to_string(), lv);
        let start = self.windows.get(w).map_or(ts, |x| x.start_ms);
        self.emit_l2(t, w, (ts.max(start), r, true), rows, sink)
    }

    /// `rows` = (side, price, new_size) under window `w`, stamped (server ts, recv, snapshot).
    /// A token's l2 ts never goes back: a row older than its previous one is raised to it.
    fn emit_l2(
        &mut self,
        t: &str,
        w: usize,
        (ts, recv_ms, snapshot): (i64, i64, bool),
        rows: Vec<(u8, i64, i64)>,
        sink: &mut dyn Sink,
    ) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let token_idx = self.index(t)?;
        let episode_id = self
            .windows
            .get(w)
            .map(|x| x.episode_id.clone())
            .context("l2 owner is not a window of this hour")?;
        let floor = self.l2_ts.get(t).copied().unwrap_or(i64::MIN);
        let ts_ms = ts.max(floor);
        for (side, price, new_size) in rows {
            if ts < floor {
                self.stats.ts_clamped += 1;
            }
            sink.l2(L2Row {
                ts_ms,
                recv_ms,
                token_idx,
                side,
                price,
                new_size,
                snapshot,
                episode_id: episode_id.clone(),
            })?;
        }
        self.l2_ts.insert(t.to_string(), ts_ms);
        Ok(())
    }

    /// The whole hour has no raw file: `raw_hour_lost` (recorded lost) or `scanner_stopped`.
    pub fn missing_hour(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let h = self.hour_ms;
        self.close_shard_gaps(h, sink)?;
        self.close_universe_gap(h, sink)?;
        self.trusted.clear();
        match &self.global_gap {
            Some((w, _)) if w == why => {}
            _ => self.global_gap = Some((why.to_string(), h)),
        }
        self.clock_ms = self.clock_ms.max(h + HOUR_MS - 1);
        self.next_depth_ms = None;
        Ok(())
    }

    /// The raw file can't be read past the last line: the rest of the hour is `why`, whatever
    /// whole-feed gap was open before (unreadable data is lost even after a shutdown).
    pub fn cut(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let at = self.clock_ms.saturating_add(1).max(self.hour_ms);
        self.close_shard_gaps(at, sink)?;
        self.close_universe_gap(at, sink)?;
        self.trusted.clear();
        self.global_gap = match self.global_gap.take() {
            Some((w, since)) if w == why => Some((w, since)),
            Some((w, since)) => {
                gap(self.hour_ms, since, at, None, &w, sink)?;
                Some((why.to_string(), at))
            }
            None => Some((why.to_string(), at)),
        };
        self.next_depth_ms = None;
        Ok(())
    }

    /// Flush every open gap up to the hour's end (they stay open) and return the hour's counts.
    pub fn end_hour(&mut self, sink: &mut dyn Sink) -> anyhow::Result<HourStats> {
        let end = self.hour_ms + HOUR_MS;
        if let Some((why, since)) = self.global_gap.clone() {
            gap(self.hour_ms, since, end, None, &why, sink)?;
        }
        if let Some(since) = self.universe_lost {
            gap(self.hour_ms, since, end, None, reason::RAW_HOUR_LOST, sink)?;
        }
        let open: Vec<(usize, OpenGap)> = self
            .shard_gaps
            .iter()
            .map(|(c, g)| (*c, g.clone()))
            .collect();
        for (c, g) in open {
            gap(self.hour_ms, g.since_ms, end, Some(c), &g.reason, sink)?;
        }
        Ok(std::mem::take(&mut self.stats))
    }
}
