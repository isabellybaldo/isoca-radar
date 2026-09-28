//! One raw hour → archive rows (spec 5b §3, §4.1). Pure: no files, no database. Lines go through
//! tape::feed::FeedBooks, the scanner's own book rules (spec §2). On top of it this keeps which
//! books are trusted (untrusted from a disconnect, a restart, a missing or lost stretch, or a
//! token's first appearance, until the feed sends it a full snapshot: no row is ever made from a
//! stale book), the day's token dictionary, top-of-book changes, periodic depth, and the gaps.
use crate::rows::{DepthRow, GapRow, SIDE_ASK, SIDE_BID, Sink, TobRow, TokenRow, micros, reason};
use crate::windows::Window;
use anyhow::Context;
use arb_core::book::Top;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tape::feed::{Applied, FeedBooks, FeedState, LineEvent};
use tape::line::Line;
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
    // per hour, never checkpointed
    hour_ms: i64,
    windows: Vec<Window>,
    stats: HourStats,
}

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
            hour_ms: 0,
            windows: Vec::new(),
            stats: HourStats::default(),
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
        if let Some((why, since)) = self.global_gap.take() {
            gap(self.hour_ms, since, r, None, &why, sink)?;
            self.wait_for_untrusted(r);
        }
        self.depth_due(sink)?;
        let ap = self.feed.apply(line, load, &|_| false);
        self.on_event(&ap, r, sink)?;
        self.rows(&ap, r, sink)
    }

    fn rows(&mut self, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        for t in &ap.snapshotted {
            self.retrust(t, r, sink)?;
        }
        for t in &ap.touched {
            if self.trusted.contains(t) {
                self.changed.insert(t.clone());
                self.tob(t, r, sink)?;
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
                for t in &ch.gone {
                    self.forget(t, r, sink)?;
                }
                for t in ch.appeared.iter().chain(ch.reset.iter()) {
                    self.trusted.remove(t);
                }
                self.wait_for_untrusted(r);
            }
            LineEvent::UniverseMissing(_) => {
                self.stats.universe_missing += 1;
                self.trusted.clear();
                self.wait_for_untrusted(r);
            }
            LineEvent::Shutdown => {
                self.close_shard_gaps(r, sink)?;
                self.trusted.clear();
                if self.global_gap.is_none() {
                    self.global_gap =
                        Some((reason::SCANNER_STOPPED.to_string(), r.saturating_add(1)));
                }
            }
            LineEvent::BadLine => self.stats.bad_lines += 1,
            LineEvent::None | LineEvent::Frame => {}
        }
        Ok(())
    }

    fn wait(&mut self, conn: usize, t: &str, why: &str, r: i64) {
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

    /// Every visible, untrusted token with a connection waits for its snapshot.
    fn wait_for_untrusted(&mut self, r: i64) {
        for t in self.feed.visible_tokens() {
            if self.trusted.contains(&t) {
                continue;
            }
            if let Some(c) = self.feed.conn(&t) {
                self.wait(c, &t, reason::NO_BOOK_YET, r);
            }
        }
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
            self.stop_waiting(t, r, sink)?;
        }
        Ok(())
    }

    fn forget(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        self.trusted.remove(t);
        self.changed.remove(t);
        self.stop_waiting(t, r, sink)
    }

    fn close_shard_gaps(&mut self, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        for (c, g) in std::mem::take(&mut self.shard_gaps) {
            gap(self.hour_ms, g.since_ms, at, Some(c), &g.reason, sink)?;
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
        let top = top_micros(book.top())?;
        let ts = book.server_ts_ms();
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

    /// At each depth boundary the receive clock crosses: the books as of that boundary.
    fn depth_due(&mut self, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let every = self.cfg.depth_every_ms;
        let Some(next) = self.next_depth_ms else {
            self.next_depth_ms = Some((self.clock_ms.div_euclid(every) + 1) * every);
            return Ok(());
        };
        if self.clock_ms < next {
            return Ok(());
        }
        let at = self.clock_ms.div_euclid(every) * every;
        let toks: Vec<String> = self
            .changed
            .iter()
            .filter(|t| self.trusted.contains(*t))
            .cloned()
            .collect();
        for t in toks {
            self.depth_rows(&t, at, sink)?;
        }
        self.changed.clear();
        self.next_depth_ms = Some(at + every);
        Ok(())
    }

    fn depth_rows(&mut self, t: &str, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let n = self.cfg.depth_levels;
        let sides = [
            (
                SIDE_BID,
                book.bids_best_first()
                    .into_iter()
                    .take(n)
                    .collect::<Vec<_>>(),
            ),
            (
                SIDE_ASK,
                book.asks_best_first()
                    .into_iter()
                    .take(n)
                    .collect::<Vec<_>>(),
            ),
        ];
        let token_idx = self.index(t)?;
        for (side, levels) in sides {
            for (level, (p, s)) in levels.into_iter().enumerate() {
                sink.depth(DepthRow {
                    ts_ms: at,
                    token_idx,
                    side,
                    level: u16::try_from(level)?,
                    price: micros(p.get())?,
                    size: micros(s.get())?,
                })?;
            }
        }
        Ok(())
    }

    /// The whole hour has no raw file: `raw_hour_lost` (recorded lost) or `scanner_stopped`.
    pub fn missing_hour(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let h = self.hour_ms;
        self.close_shard_gaps(h, sink)?;
        self.trusted.clear();
        match &self.global_gap {
            Some((w, _)) if w == why => {}
            _ => self.global_gap = Some((why.to_string(), h)),
        }
        self.clock_ms = self.clock_ms.max(h + HOUR_MS - 1);
        self.next_depth_ms = None;
        Ok(())
    }

    /// The raw file can't be read past the last line: the rest of the hour is `why`.
    pub fn cut(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let at = self.clock_ms.saturating_add(1).max(self.hour_ms);
        self.close_shard_gaps(at, sink)?;
        self.trusted.clear();
        if self.global_gap.is_none() {
            self.global_gap = Some((why.to_string(), at));
        }
        self.next_depth_ms = None;
        Ok(())
    }

    /// Flush every open gap up to the hour's end (they stay open) and return the hour's counts.
    pub fn end_hour(&mut self, sink: &mut dyn Sink) -> anyhow::Result<HourStats> {
        let end = self.hour_ms + HOUR_MS;
        if let Some((why, since)) = self.global_gap.clone() {
            gap(self.hour_ms, since, end, None, &why, sink)?;
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
