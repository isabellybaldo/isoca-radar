//! Incremental windows (spec §7 step 3, kept fast as "all" grows: Review Focus 5). Instead of
//! re-reading and re-merging up to every cell of a key on each tick, each window keeps the sum of
//! the cells it covers: a rebuilt hour swaps its old cell for the new one (subtract, add), and
//! advancing the clock subtracts the hours that leave each span and adds stored cells of the
//! hours that enter. The invariant analyze keeps: the applied version of any hour ≤ now_hour is
//! exactly the one stored in card_cells.
//!
//! Carried Task 5 finding: `km` in release mode trusts its inputs, so a subtraction that drives a
//! count negative (a hour removed twice, or removed before it was ever added) would silently
//! produce a negative Ŝ and a wrong median. The sliding windows below never call `C::sub` on an
//! hour's cell unless this struct's own bookkeeping (`span_present`, and `maxes` for `all`) shows
//! that hour is currently included in that sum — a caller-supplied `old` for an hour that was
//! never added is ignored instead of subtracted, so `spans`/`all` can never be pushed negative by
//! this arithmetic. See `apply_ignores_a_cell_that_was_never_added` in `tests/window_property.rs`.
use crate::HOUR_MS;
use crate::cells::CellData;
use std::collections::{BTreeMap, BTreeSet};

/// Rolling spans in hours, ending on (and including) the current, partial hour.
pub const SPANS: [(&str, i64); 3] = [("24h", 24), ("7d", 168), ("30d", 720)];
/// Cells kept in memory: today's window and the 24h window's expiring hour.
pub const RECENT_HOURS: i64 = 48;

#[derive(Debug, Clone)]
pub struct Windows<C: CellData> {
    /// The current hour (the last hour of every window).
    pub now_hour: i64,
    /// One sum per SPANS entry.
    pub spans: [C; 3],
    /// Every applied hour ≤ now_hour.
    pub all: C,
    /// Per-hour maxima of every applied hour (tiny: Edge capture's two numbers). Doubles as the
    /// record of every hour ever applied, so `all`'s subtraction can be checked against it.
    pub maxes: BTreeMap<i64, C::Max>,
    pub recent: BTreeMap<i64, C>,
    /// Hours currently summed into each `spans` entry. Bookkeeping only (not part of the public
    /// contract): guards `spans[i].sub(..)` against an hour that isn't actually part of that sum.
    span_present: [BTreeSet<i64>; 3],
}

impl<C: CellData> Windows<C> {
    pub fn new(now_hour: i64) -> Self {
        Self {
            now_hour,
            spans: Default::default(),
            all: C::default(),
            maxes: BTreeMap::new(),
            recent: BTreeMap::new(),
            span_present: Default::default(),
        }
    }

    pub fn span_start(&self, span_hours: i64) -> i64 {
        self.now_hour - (span_hours - 1) * HOUR_MS
    }

    fn recent_floor(&self) -> i64 {
        self.now_hour - (RECENT_HOURS - 1) * HOUR_MS
    }

    /// Subtract `cell` from `spans[i]` only if `hour` is currently tracked as part of that span's
    /// sum (and stop tracking it). A caller-supplied cell for an hour that was never added to this
    /// span is ignored rather than subtracted, so the span can never go negative from this call.
    fn checked_span_sub(&mut self, i: usize, hour: i64, cell: &C) {
        if self.span_present[i].remove(&hour) {
            self.spans[i].sub(cell);
        }
    }

    /// Add `cell` to `spans[i]` and mark `hour` as part of that span's sum.
    fn span_add(&mut self, i: usize, hour: i64, cell: &C) {
        self.span_present[i].insert(hour);
        self.spans[i].add(cell);
    }

    /// Subtract `cell` from `all` only if `hour` was ever applied (tracked via `maxes`).
    fn checked_all_sub(&mut self, hour: i64, cell: &C) {
        if self.maxes.contains_key(&hour) {
            self.all.sub(cell);
        }
    }

    /// The stored cells `advance(to_hour)` needs, as [from, to) ranges: hours leaving each span
    /// (some are in `recent` already) and hours entering the windows.
    pub fn needed_ranges(&self, to_hour: i64) -> Vec<(i64, i64)> {
        if to_hour <= self.now_hour {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (_, span) in SPANS {
            let from = self.span_start(span);
            let end = (to_hour - (span - 1) * HOUR_MS).min(self.now_hour + HOUR_MS);
            if from < end {
                out.push((from, end));
            }
        }
        out.push((self.now_hour + HOUR_MS, to_hour + HOUR_MS));
        out
    }

    /// Move the windows' end to `to_hour`. `stored` holds the stored cells of
    /// `needed_ranges(to_hour)` (a missing hour is an empty cell). A `to_hour` that isn't later
    /// is ignored: a clock that went back changes nothing.
    pub fn advance(&mut self, to_hour: i64, stored: &BTreeMap<i64, C>) {
        if to_hour <= self.now_hour {
            return;
        }
        let old_now = self.now_hour;
        for (i, (_, span)) in SPANS.iter().enumerate() {
            let from = old_now - (span - 1) * HOUR_MS;
            let end = (to_hour - (span - 1) * HOUR_MS).min(old_now + HOUR_MS);
            let mut h = from;
            while h < end {
                match self.recent.get(&h).or_else(|| stored.get(&h)).cloned() {
                    Some(c) => self.checked_span_sub(i, h, &c),
                    None => {
                        self.span_present[i].remove(&h);
                    }
                }
                h += HOUR_MS;
            }
        }
        for (h, c) in stored.range(old_now + HOUR_MS..=to_hour) {
            for (i, (_, span)) in SPANS.iter().enumerate() {
                if *h >= to_hour - (span - 1) * HOUR_MS {
                    self.span_add(i, *h, c);
                }
            }
            if !self.maxes.contains_key(h) {
                self.all.add(c);
            }
            self.maxes.insert(*h, c.max_part());
            self.recent.insert(*h, c.clone());
        }
        self.now_hour = to_hour;
        let floor = self.recent_floor();
        self.recent = self.recent.split_off(&floor);
    }

    /// Replace hour `hour`'s applied cell `old` (None: nothing applied yet) with `new` in every
    /// window holding it. Hours after now_hour are left to `advance` (it adds their stored cell).
    /// `old` is trusted only to decide *whether* a hand-off happened (and even then, is only ever
    /// subtracted from a sum this struct itself tracks `hour` as part of, see `checked_span_sub` /
    /// `checked_all_sub`): a stale or mistaken `old` for an hour this struct never added can only
    /// be ignored, never subtracted, so it can never drive a count negative.
    pub fn apply(&mut self, hour: i64, old: Option<&C>, new: &C) {
        if hour > self.now_hour {
            return;
        }
        for (i, (_, span)) in SPANS.iter().enumerate() {
            let start = self.span_start(*span);
            if hour >= start {
                if let Some(o) = old {
                    self.checked_span_sub(i, hour, o);
                }
                self.span_add(i, hour, new);
            }
        }
        if let Some(o) = old {
            self.checked_all_sub(hour, o);
        }
        self.all.add(new);
        self.maxes.insert(hour, new.max_part());
        if hour >= self.recent_floor() {
            self.recent.insert(hour, new.clone());
        }
    }

    /// The merged maxima of the applied hours in [from_hour, now_hour].
    pub fn max_from(&self, from_hour: i64) -> C::Max {
        let mut m = C::Max::default();
        for (_, v) in self.maxes.range(from_hour..=self.now_hour) {
            C::merge_max(&mut m, v);
        }
        m
    }

    /// The sum of the applied hours in [day_start, now_hour] (Edge capture's `today`).
    pub fn today(&self, day_start: i64) -> C {
        let mut c = C::default();
        for (_, v) in self.recent.range(day_start..=self.now_hour) {
            c.add(v);
        }
        c
    }
}
