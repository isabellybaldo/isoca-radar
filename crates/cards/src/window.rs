//! Incremental windows (spec §7 step 3, kept fast as "all" grows: Review Focus 5). Instead of
//! re-reading and re-merging up to every cell of a key on each tick, each window keeps the sum of
//! the cells it covers: a rebuilt hour swaps its old cell for the new one (subtract, add), and
//! advancing the clock subtracts the hours that leave each span and adds stored cells of the
//! hours that enter. The invariant analyze keeps: the applied version of any hour ≤ now_hour is
//! exactly the one stored in card_cells.
//!
//! Carried Task 5 finding: `km` in release mode trusts its inputs, so a subtraction that drives a
//! count negative (an hour removed twice, removed before it was ever added, or removed with the
//! wrong value) would silently produce a negative Ŝ and a wrong median. Both places that subtract
//! a caller-supplied cell — `apply`'s `old` and `advance`'s expiry loop (which reads `stored`, also
//! caller-supplied) — are the realistic failure points, so both are checked against a per-hour
//! fingerprint kept alongside `maxes` before anything is touched:
//! - `old`'s fingerprint (in `apply`) or the expiring cell's fingerprint (in `advance`) must match
//!   the one on record for that hour. `None` (absent) and an empty/default cell both count as "no
//!   fingerprint" — a default cell is the same as an absent one, so a caller that never bothers to
//!   store a row for an all-zero hour is never a mismatch (`fp_of`, below).
//! - A mismatch trips a `debug_assert!` (this project's convention: every build on the dev machine
//!   is a debug build, so this fails loudly and immediately there; a release build — Task 14's
//!   Docker builds — carries on) and sets `needs_rebuild()`, and leaves every sum untouched for
//!   that hour: no add, no sub. Recovery is a full rebuild of this key's `Windows` from
//!   `card_cells` (Task 9), not incremental patching.
//!
//! Once both call sites are fingerprint-checked, neither needs a separate presence set to avoid
//! subtracting a cell that wasn't really there: the fingerprint gate already proves it. See
//! `tests/window_property.rs` for the cases this guards.
use crate::HOUR_MS;
use crate::cells::CellData;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io;

/// Rolling spans in hours, ending on (and including) the current, partial hour.
pub const SPANS: [(&str, i64); 3] = [("24h", 24), ("7d", 168), ("30d", 720)];
/// Cells kept in memory: today's window and the 24h window's expiring hour.
pub const RECENT_HOURS: i64 = 48;

/// Forwards every byte written to it straight into a `Hasher`, so `serde_json::to_writer` can feed
/// a cell's JSON to the fingerprint hash without ever materializing the JSON as a `String` (fix
/// round 1, spec §9: SWEET's ~25 KB cells were re-serialized to a fresh `String` on every one of
/// the ~8 fingerprint checks a tick makes).
struct HashWriter<'a, H: Hasher>(&'a mut H);

impl<H: Hasher> io::Write for HashWriter<'_, H> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A cheap, process-local fingerprint of a cell's canonical JSON (not stable across versions or
/// processes — only used to detect a mismatch within one running `Windows`). Streams the exact
/// same bytes `c.to_json().hash(...)` would have hashed straight into the hasher (`str`'s `Hash`
/// impl writes the bytes then a `0xff` terminator; matched here so the fingerprint values this
/// produces are identical to before, and to each other across a restart — `window.rs` tests this).
fn fingerprint<C: CellData>(c: &C) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match serde_json::to_writer(HashWriter(&mut hasher), c) {
        Ok(()) => hasher.write_u8(0xff),
        Err(_) => "cards::window: a cell that failed to serialize".hash(&mut hasher),
    }
    hasher.finish()
}

/// The fingerprint to check `c` against, or `None` if there's nothing to check: absent (`None`)
/// and an empty/default cell (`Some(default)`) are the same thing (controller ruling, fix round
/// 2), so a caller that stores no row for an all-zero hour is never flagged as a mismatch.
fn fp_of<C: CellData>(c: Option<&C>) -> Option<u64> {
    c.filter(|c| **c != C::default()).map(fingerprint)
}

#[derive(Debug, Clone)]
pub struct Windows<C: CellData> {
    /// The current hour (the last hour of every window).
    pub now_hour: i64,
    /// One sum per SPANS entry.
    pub spans: [C; 3],
    /// Every applied hour ≤ now_hour.
    pub all: C,
    /// Per-hour maxima of every applied hour (tiny: Edge capture's two numbers). Doubles as the
    /// record of every hour ever applied.
    pub maxes: BTreeMap<i64, C::Max>,
    pub recent: BTreeMap<i64, C>,
    /// `fingerprint()` of the cell currently applied for each hour on record — but only when that
    /// cell isn't empty/default (see `fp_of`): an hour whose applied cell is empty has no entry
    /// here, the same as an hour that was never applied. `apply`'s `old` and `advance`'s expiring
    /// cells are checked against this before anything is touched: see the module doc.
    fingerprints: BTreeMap<i64, u64>,
    /// Set once a fingerprint mismatch is seen. Sticky: only a full rebuild of this `Windows`
    /// (Task 9, from `card_cells`) is expected to clear it.
    dirty: bool,
}

impl<C: CellData> Windows<C> {
    pub fn new(now_hour: i64) -> Self {
        Self {
            now_hour,
            spans: Default::default(),
            all: C::default(),
            maxes: BTreeMap::new(),
            recent: BTreeMap::new(),
            fingerprints: BTreeMap::new(),
            dirty: false,
        }
    }

    pub fn span_start(&self, span_hours: i64) -> i64 {
        self.now_hour - (span_hours - 1) * HOUR_MS
    }

    fn recent_floor(&self) -> i64 {
        self.now_hour - (RECENT_HOURS - 1) * HOUR_MS
    }

    /// True once a fingerprint mismatch was seen in `apply` or `advance` (a never-applied hour
    /// claimed as applied, a wrong-value cell, or a missing/absent cell for an hour that has a
    /// real one on record). The caller must rebuild this key's `Windows` from `card_cells`
    /// (Task 9): the sums are left exactly as they were at the point of mismatch, not patched
    /// incrementally.
    pub fn needs_rebuild(&self) -> bool {
        self.dirty
    }

    /// Record (or, for an empty/default cell, forget — see `fp_of`) the fingerprint on file for
    /// `hour`.
    fn record_fingerprint(&mut self, hour: i64, cell: &C) {
        if *cell == C::default() {
            self.fingerprints.remove(&hour);
        } else {
            self.fingerprints.insert(hour, fingerprint(cell));
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
    ///
    /// The expiry loop checks each expiring hour's fingerprint before subtracting it (see the
    /// module doc): a missing or wrong-value cell for an hour that has a real fingerprint on
    /// record sets `needs_rebuild()` and is skipped (left in the span, an intentional overcount
    /// safer than an unverified subtraction) rather than trusted.
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
                let found = self.recent.get(&h).or_else(|| stored.get(&h)).cloned();
                let found_fp = fp_of(found.as_ref());
                let stored_fp = self.fingerprints.get(&h).copied();
                if found_fp == stored_fp {
                    if let Some(c) = found {
                        self.spans[i].sub(&c);
                    }
                } else {
                    self.dirty = true;
                    debug_assert!(
                        false,
                        "cards::window: advance() expiring hour {h} found fingerprint {found_fp:?}, \
                         but {stored_fp:?} is on record; skipping the subtraction and flagging for a rebuild"
                    );
                }
                h += HOUR_MS;
            }
        }
        for (h, c) in stored.range(old_now + HOUR_MS..=to_hour) {
            for (i, (_, span)) in SPANS.iter().enumerate() {
                if *h >= to_hour - (span - 1) * HOUR_MS {
                    self.spans[i].add(c);
                }
            }
            // Every hour reaching this loop is strictly newer than the previous now_hour, so it
            // can never already be on record: `apply` only ever touches hours ≤ now_hour.
            debug_assert!(
                !self.maxes.contains_key(h),
                "cards::window: advance() is entering hour {h} that's already on record"
            );
            self.all.add(c);
            self.record_fingerprint(*h, c);
            self.maxes.insert(*h, c.max_part());
            self.recent.insert(*h, c.clone());
        }
        self.now_hour = to_hour;
        let floor = self.recent_floor();
        self.recent = self.recent.split_off(&floor);
    }

    /// Replace hour `hour`'s applied cell `old` (None: nothing applied yet) with `new` in every
    /// window holding it. Hours after now_hour are left to `advance` (it adds their stored cell).
    ///
    /// `old` is checked against the fingerprint this struct has on record for `hour` *before*
    /// anything is touched: a mismatch (a never-applied hour claimed as applied, a wrong-value
    /// `old`, or an `old` that doesn't match a real fingerprint on record — including `None` sent
    /// for an hour that has one, which would otherwise double-count) trips a `debug_assert!`, sets
    /// `needs_rebuild()`, and the call is a no-op — neither `old` nor `new` change any sum. See the
    /// module doc, including `fp_of`'s empty-cell-is-absent rule.
    pub fn apply(&mut self, hour: i64, old: Option<&C>, new: &C) {
        if hour > self.now_hour {
            return;
        }
        let stored_fp = self.fingerprints.get(&hour).copied();
        let old_fp = fp_of(old);
        if old_fp != stored_fp {
            // Set the flag *before* the assert: a debug build panics right here (this always
            // signals a caller bug), but a release build carries on with `needs_rebuild()` set.
            self.dirty = true;
            debug_assert!(
                false,
                "cards::window: apply({hour}) called with old fingerprint {old_fp:?}, but {stored_fp:?} \
                 is on record; flagging for a full rebuild instead of touching any sum"
            );
            return;
        }
        for (i, (_, span)) in SPANS.iter().enumerate() {
            let start = self.span_start(*span);
            if hour >= start {
                if let Some(o) = old {
                    self.spans[i].sub(o);
                }
                self.spans[i].add(new);
            }
        }
        if let Some(o) = old {
            self.all.sub(o);
        }
        self.all.add(new);
        self.record_fingerprint(hour, new);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::{G1Cell, Num, PerSide, Side};
    use crate::sweet::{Acc, SweetCell, row_key};
    use rust_decimal_macros::dec;

    /// The fingerprint computation before fix round 1: hash the full JSON `String`. `fingerprint`
    /// must keep producing exactly this value (stored/compared fingerprints must survive the
    /// change), even though it no longer builds that `String` to get there.
    fn old_fingerprint<C: CellData>(c: &C) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match c.to_json() {
            Ok(json) => json.hash(&mut hasher),
            Err(_) => "cards::window: a cell that failed to serialize".hash(&mut hasher),
        }
        hasher.finish()
    }

    #[test]
    fn the_streamed_fingerprint_equals_the_old_to_json_hash_for_a_5a_and_a_sweet_cell() {
        assert_eq!(
            fingerprint(&G1Cell::default()),
            old_fingerprint(&G1Cell::default())
        );
        let g1 = G1Cell {
            their: PerSide { yes: 3, no: 5 },
            ..G1Cell::default()
        };
        assert_eq!(fingerprint(&g1), old_fingerprint(&g1));

        assert_eq!(
            fingerprint(&SweetCell::default()),
            old_fingerprint(&SweetCell::default())
        );
        let mut sweet = SweetCell::default();
        sweet.rows.insert(
            row_key(Side::No, "sp-idle", "all", "all"),
            Acc {
                eps: 2,
                on_table: Num(dec!(1.5)),
                ..Acc::default()
            },
        );
        assert_eq!(fingerprint(&sweet), old_fingerprint(&sweet));
    }
}
