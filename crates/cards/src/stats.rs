//! Exact statistics over integer count maps (spec §7: durations as `duration_ms → (events,
//! censored)` count maps, so Kaplan–Meier is exact and hourly cells merge by addition). Survival
//! probabilities use an i128 fixed point (1.0 = 10²⁸, Decimal's largest scale): one multiply and
//! one divide per event time, fast even in a debug build, truncation ≤ 10⁻²⁸ per step.
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use std::collections::BTreeMap;

/// duration_ms → (events, censored). Counts are i64 so windows can subtract.
pub type DurMap = BTreeMap<i64, (i64, i64)>;
/// value → count.
pub type CountMap = BTreeMap<i64, i64>;

pub const FP_SCALE: u32 = 28;
pub const FP_ONE: i128 = 10_000_000_000_000_000_000_000_000_000;

pub fn add_counts(into: &mut CountMap, other: &CountMap, sign: i64) {
    for (k, v) in other {
        let e = into.entry(*k).or_insert(0);
        *e += sign * v;
        if *e == 0 {
            into.remove(k);
        }
    }
}

pub fn add_durs(into: &mut DurMap, other: &DurMap, sign: i64) {
    for (k, (ev, ce)) in other {
        let e = into.entry(*k).or_insert((0, 0));
        e.0 += sign * ev;
        e.1 += sign * ce;
        if *e == (0, 0) {
            into.remove(k);
        }
    }
}

pub fn total(m: &CountMap) -> i64 {
    m.values().sum()
}

/// (events, censored)
pub fn dur_total(m: &DurMap) -> (i64, i64) {
    m.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1))
}

/// Two sorted maps as one sorted stream, equal keys combined with `plus`. Lazy and `Clone` (Kaplan–Meier
/// walks it twice; a median stops halfway), so nothing is collected (Review Focus 5: one tick < 200 ms).
pub fn merge_sorted<'a, V: Copy + 'a>(
    a: &'a BTreeMap<i64, V>,
    b: &'a BTreeMap<i64, V>,
    plus: fn(V, V) -> V,
) -> impl Iterator<Item = (i64, V)> + Clone + 'a {
    Merge {
        a: a.iter().peekable(),
        b: b.iter().peekable(),
        plus,
    }
}

#[derive(Clone)]
struct Merge<'a, V> {
    a: std::iter::Peekable<std::collections::btree_map::Iter<'a, i64, V>>,
    b: std::iter::Peekable<std::collections::btree_map::Iter<'a, i64, V>>,
    plus: fn(V, V) -> V,
}

impl<V: Copy> Iterator for Merge<'_, V> {
    type Item = (i64, V);

    fn next(&mut self) -> Option<(i64, V)> {
        let ka = self.a.peek().map(|(k, _)| **k);
        let kb = self.b.peek().map(|(k, _)| **k);
        match (ka, kb) {
            (None, None) => None,
            (Some(x), Some(y)) if x == y => {
                let (k, va) = self.a.next()?;
                let (_, vb) = self.b.next()?;
                Some((*k, (self.plus)(*va, *vb)))
            }
            (Some(x), Some(y)) if x < y => self.a.next().map(|(k, v)| (*k, *v)),
            (Some(_), None) => self.a.next().map(|(k, v)| (*k, *v)),
            _ => self.b.next().map(|(k, v)| (*k, *v)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KmSummary {
    /// Ŝ(τ) for each grid point, fixed point (FP_ONE = 1).
    pub at: Vec<i128>,
    /// OUR convention (Kleinbaum & Klein): the smallest *event* time at which the Kaplan–Meier
    /// estimate first reaches Ŝ(t) ≤ 1/2 (None: never reached). This is not an interpolated
    /// midpoint — it is a specific observed time, chosen because it stays exact under integer/
    /// fixed-point maths and matches the textbook's own worked examples. It intentionally
    /// disagrees with [`median_of`]'s sample-median convention even on the same uncensored data:
    /// e.g. `[1, 2, 3, 4]` gives a sample median of 2.5 (the mean of the middle pair) but a KM
    /// median of 2 (the first event time where Ŝ ≤ 1/2 — see
    /// `median_of_and_km_median_disagree_by_design_on_even_n` in `tests/stats.rs`).
    pub median: Option<i64>,
    pub events: i64,
    pub censored: i64,
}

/// Kaplan–Meier, Ŝ(τ) = Π_{t_j ≤ τ} (1 − d_j / n_j), over `(t, (events, censored))` in ascending
/// t (a censored time tied with events stays at risk at that time, the textbook convention).
/// `grid` must be ascending. Debug builds assert the caller's invariants (non-negative counts,
/// events not exceeding the current at-risk count, an ascending grid, and no i128 overflow in
/// the survival product); a release build never panics on bad input but the fixed point then
/// saturates toward its maximum representable value instead of silently wrapping to 0 — a
/// documented, conservative fallback that no valid input exercises (see `debug_assert!`
/// call sites below and the 6-MP oracle / proptest that pin normal behaviour).
pub fn km<I>(entries: I, grid: &[i64]) -> KmSummary
where
    I: Iterator<Item = (i64, (i64, i64))> + Clone,
{
    let (events, censored) = entries
        .clone()
        .fold((0i64, 0i64), |a, (_, v)| (a.0 + v.0, a.1 + v.1));
    km_totals(entries, events, censored, grid)
}

/// [`km`] when the caller already knows the entries' totals (events, censored), e.g. the sums of
/// two sides it merges: one walk instead of two. `events`/`censored` must be the entries' sums.
pub fn km_totals<I>(entries: I, events: i64, censored: i64, grid: &[i64]) -> KmSummary
where
    I: Iterator<Item = (i64, (i64, i64))>,
{
    let mut acc = KmAcc::new(grid, events, censored);
    for (t, (e, c)) in entries {
        acc.step(t, e, c);
        if acc.settled() {
            break;
        }
    }
    acc.finish()
}

/// Kaplan–Meier of two sides and of both merged, in one walk over the merged stream (G2 needs each
/// side at `side_grid`, G3 both sides together at `merged_grid`): the same three summaries as
/// `km` on each side and `km` on `merge_sorted` of both (Review Focus 5: one pass per window).
pub fn km_sides_and_merged(
    yes: &DurMap,
    no: &DurMap,
    side_grid: &[i64],
    merged_grid: &[i64],
) -> [KmSummary; 3] {
    let (ey, cy) = dur_total(yes);
    let (en, cn) = dur_total(no);
    let mut ay = KmAcc::new(side_grid, ey, cy);
    let mut an = KmAcc::new(side_grid, en, cn);
    let mut am = KmAcc::new(merged_grid, ey + en, cy + cn);
    let mut iy = yes.iter().peekable();
    let mut in_ = no.iter().peekable();
    loop {
        let ky = iy.peek().map(|(k, _)| **k);
        let kn = in_.peek().map(|(k, _)| **k);
        let t = match (ky, kn) {
            (None, None) => break,
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
        };
        let vy = if ky == Some(t) {
            iy.next().map(|(_, v)| *v)
        } else {
            None
        };
        let vn = if kn == Some(t) {
            in_.next().map(|(_, v)| *v)
        } else {
            None
        };
        if let Some((e, c)) = vy
            && !ay.settled()
        {
            ay.step(t, e, c);
        }
        if let Some((e, c)) = vn
            && !an.settled()
        {
            an.step(t, e, c);
        }
        if !am.settled() {
            let (y, n) = (vy.unwrap_or((0, 0)), vn.unwrap_or((0, 0)));
            am.step(t, y.0 + n.0, y.1 + n.1);
        }
        if ay.settled() && an.settled() && am.settled() {
            break;
        }
    }
    [ay.finish(), an.finish(), am.finish()]
}

/// One Kaplan–Meier walk in progress (the body of `km`), so several can share one stream.
struct KmAcc<'g> {
    grid: &'g [i64],
    at_risk: i64,
    s: i128,
    at: Vec<i128>,
    gi: usize,
    median: Option<i64>,
    events: i64,
    censored: i64,
}

impl<'g> KmAcc<'g> {
    fn new(grid: &'g [i64], events: i64, censored: i64) -> Self {
        debug_assert!(
            grid.windows(2).all(|w| w[0] <= w[1]),
            "km: grid must be ascending, got {grid:?}"
        );
        Self {
            grid,
            at_risk: events + censored,
            s: FP_ONE,
            at: Vec::with_capacity(grid.len()),
            gi: 0,
            median: None,
            events,
            censored,
        }
    }

    /// Every grid point is filled and the median is found: the rest of the walk cannot change
    /// the summary (the totals came from the first pass), so a caller may stop.
    fn settled(&self) -> bool {
        self.gi >= self.grid.len() && self.median.is_some()
    }

    fn step(&mut self, t: i64, e: i64, c: i64) {
        debug_assert!(
            e >= 0 && c >= 0,
            "km: negative counts (e={e}, c={c}) at t={t}"
        );
        debug_assert!(
            e <= self.at_risk,
            "km: events ({e}) exceed at-risk ({}) at t={t}",
            self.at_risk
        );
        while self.grid.get(self.gi).is_some_and(|g| *g < t) {
            self.at.push(self.s);
            self.gi += 1;
        }
        if e > 0 && self.at_risk > 0 {
            let factor = i128::from(self.at_risk - e);
            let product = self.s.checked_mul(factor);
            debug_assert!(
                product.is_some(),
                "km: i128 overflow computing the survival product at t={t} (s={}, factor={factor})",
                self.s
            );
            // Release fallback only: saturate toward the largest representable value rather
            // than wrapping to 0. Unreachable for realistic episode counts (s ≤ FP_ONE ≈ 1e28,
            // factor bounded by the at-risk count), so no test exercises this arm.
            self.s = product.unwrap_or(i128::MAX) / i128::from(self.at_risk);
            self.s = self.s.max(0); // defence in depth: a corrupted window sum must never yield a negative Ŝ
            if self.median.is_none() && self.s.saturating_mul(2) <= FP_ONE {
                self.median = Some(t);
            }
        }
        self.at_risk -= e + c;
    }

    fn finish(mut self) -> KmSummary {
        while self.gi < self.grid.len() {
            self.at.push(self.s);
            self.gi += 1;
        }
        KmSummary {
            at: self.at,
            median: self.median,
            events: self.events,
            censored: self.censored,
        }
    }
}

pub fn fp_to_dec(v: i128) -> Decimal {
    Decimal::try_from_i128_with_scale(v, FP_SCALE).unwrap_or(Decimal::ZERO)
}

/// Gebele's empirical Ŝ(τ) = K⁻¹ Σ 𝟙{D_k > τ} over uncensored durations; None when K = 0.
pub fn emp_at(m: &CountMap, grid: &[i64]) -> Vec<Option<Decimal>> {
    let n = total(m);
    if n <= 0 {
        return vec![None; grid.len()];
    }
    if !grid.windows(2).all(|w| w[0] <= w[1]) {
        // not ascending: one range sum per point (never the case for GRID_MS)
        return grid
            .iter()
            .map(|tau| {
                let above: i64 = m.range(tau.saturating_add(1)..).map(|(_, c)| c).sum();
                Some(Decimal::from(above) / Decimal::from(n))
            })
            .collect();
    }
    // ascending grid: one walk, counting the durations ≤ τ as τ grows (same integers as above)
    let mut out = Vec::with_capacity(grid.len());
    let mut it = m.iter().peekable();
    let mut at_or_below = 0i64;
    for tau in grid {
        while let Some((v, c)) = it.peek() {
            if **v > *tau {
                break;
            }
            at_or_below += **c;
            it.next();
        }
        out.push(Some(Decimal::from(n - at_or_below) / Decimal::from(n)));
    }
    out
}

/// [`emp_at`] and [`median_of`] of one count map (`n` = its total) in a single walk; the same
/// integers and the same decimals as calling the two (Review Focus 5: one walk per map).
pub fn emp_and_median(
    m: &CountMap,
    n: i64,
    grid: &[i64],
) -> (Vec<Option<Decimal>>, Option<Decimal>) {
    if n <= 0 || !grid.windows(2).all(|w| w[0] <= w[1]) {
        return (
            emp_at(m, grid),
            median_of(m.iter().map(|(v, c)| (*v, *c)), n),
        );
    }
    let (klo, khi) = ((n - 1) / 2, n / 2);
    let (mut lo, mut med) = (None, None);
    let mut out = Vec::with_capacity(grid.len());
    let mut seen = 0i64;
    let mut gi = 0usize;
    for (v, c) in m {
        // every grid point below v is settled: `seen` counts exactly the durations <= it
        while grid.get(gi).is_some_and(|t| *t < *v) {
            out.push(Some(Decimal::from(n - seen) / Decimal::from(n)));
            gi += 1;
        }
        seen += c;
        if lo.is_none() && seen > klo {
            lo = Some(*v);
        }
        if med.is_none() && seen > khi {
            med = lo.map(|l| (Decimal::from(l) + Decimal::from(*v)) / Decimal::from(2));
        }
        if gi >= grid.len() && med.is_some() {
            break;
        }
    }
    while gi < grid.len() {
        out.push(Some(Decimal::from(n - seen) / Decimal::from(n)));
        gi += 1;
    }
    (out, med)
}

/// THEIR convention (an empirical/Gebele-style sample median): the median of `n` values given as
/// ascending (value, count), the mean of the middle pair when n is even. This differs from
/// [`KmSummary::median`]'s Kaplan–Meier convention by design — the sample median is a plain
/// order statistic (needs uncensored data and interpolates between two observations), while the
/// KM median is the first *observed* event time at which the survival curve crosses 1/2 (works
/// under censoring, never interpolates). The two do not have to agree, even on the same
/// uncensored data: see `median_of_and_km_median_disagree_by_design_on_even_n` in
/// `tests/stats.rs`.
pub fn median_of<I: Iterator<Item = (i64, i64)>>(entries: I, n: i64) -> Option<Decimal> {
    if n <= 0 {
        return None;
    }
    let (klo, khi) = ((n - 1) / 2, n / 2);
    let mut lo = None;
    let mut seen = 0i64;
    for (v, c) in entries {
        seen += c;
        if lo.is_none() && seen > klo {
            lo = Some(v);
        }
        if seen > khi {
            return lo.map(|l| (Decimal::from(l) + Decimal::from(v)) / Decimal::from(2));
        }
    }
    None
}

/// Nearest-rank percentile: the smallest value whose cumulative count reaches ⌈pct·n/100⌉.
pub fn nearest_rank<I: Iterator<Item = (i64, i64)>>(entries: I, n: i64, pct: i64) -> Option<i64> {
    if n <= 0 {
        return None;
    }
    let rank = ((pct * n + 99) / 100).max(1);
    let mut seen = 0i64;
    for (v, c) in entries {
        seen += c;
        if seen >= rank {
            return Some(v);
        }
    }
    None
}

/// [`nearest_rank`] at two percentiles and [`bins`], in one walk over `m` (`n` = its total).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RanksAndBins {
    /// `nearest_rank(m, n, pcts[i])`.
    pub ranks: [Option<i64>; 2],
    /// `bins(m, width, top)` (None when `width <= 0`).
    pub bins: Option<(i64, Vec<(i64, i64)>)>,
}

/// The same integers as `nearest_rank` (twice) and `bins`, in a single walk (Review Focus 5).
pub fn ranks_and_bins(m: &CountMap, n: i64, pcts: [i64; 2], width: i64, top: i64) -> RanksAndBins {
    let rank = |pct: i64| ((pct * n + 99) / 100).max(1);
    let (t0, t1) = (rank(pcts[0]), rank(pcts[1]));
    let (mut r0, mut r1) = (None, None);
    let mut bin_part = bins(&CountMap::new(), width, top);
    let mut seen = 0i64;
    // n <= 0 means no ranks (nearest_rank's rule): start with both "found"
    let mut pending = n > 0;
    match bin_part.as_mut() {
        Some((below, out)) => {
            let last = out.len().saturating_sub(1);
            for (v, c) in m {
                if pending {
                    seen += c;
                    if r0.is_none() && seen >= t0 {
                        r0 = Some(*v);
                    }
                    if r1.is_none() && seen >= t1 {
                        r1 = Some(*v);
                    }
                    pending = r0.is_none() || r1.is_none();
                }
                if *v < 0 {
                    *below += c;
                } else {
                    let i = usize::try_from((*v).min(top) / width)
                        .unwrap_or(0)
                        .min(last);
                    if let Some(slot) = out.get_mut(i) {
                        slot.1 += c;
                    }
                }
            }
        }
        None => {
            for (v, c) in m {
                if !pending {
                    break;
                }
                seen += c;
                if r0.is_none() && seen >= t0 {
                    r0 = Some(*v);
                }
                if r1.is_none() && seen >= t1 {
                    r1 = Some(*v);
                }
                pending = r0.is_none() || r1.is_none();
            }
        }
    }
    RanksAndBins {
        ranks: [r0, r1],
        bins: bin_part,
    }
}

/// Fixed-width bins: (count below 0, [(lo, count)] for lo = 0, width, …, top − width, then the
/// open bin from `top`). `None` when `width <= 0`: a non-positive width has no meaning as a bin
/// size and would otherwise divide by zero.
pub fn bins(m: &CountMap, width: i64, top: i64) -> Option<(i64, Vec<(i64, i64)>)> {
    if width <= 0 {
        return None;
    }
    let n_bins = usize::try_from(top / width).unwrap_or(0);
    let mut out: Vec<(i64, i64)> = (0..=n_bins)
        .map(|i| (i64::try_from(i).unwrap_or(0) * width, 0))
        .collect();
    let mut below = 0;
    for (v, c) in m {
        if *v < 0 {
            below += c;
            continue;
        }
        let i = usize::try_from((*v).min(top) / width)
            .unwrap_or(0)
            .min(n_bins);
        if let Some(slot) = out.get_mut(i) {
            slot.1 += c;
        }
    }
    Some((below, out))
}

/// An edge given as a fraction of $1 (0.0049) in 0.01-bp units (4900), rounded half-even.
pub fn centi_bps(edge: Decimal) -> i64 {
    (edge * Decimal::from(1_000_000))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        .to_i64()
        .unwrap_or(i64::MAX)
}

/// ms → seconds with at most 2 decimals, as a decimal string ("16.15").
pub fn secs(ms: Decimal) -> String {
    dec((ms / Decimal::from(1_000)).round_dp(2))
}

/// num / den as a percentage with 1 decimal; None when den is 0.
pub fn pct(num: i64, den: i64) -> Option<Decimal> {
    if den == 0 {
        return None;
    }
    Some((Decimal::from(num) * Decimal::from(100) / Decimal::from(den)).round_dp(1))
}

/// The api's decimal string: normalized (no trailing zeros), `-0` → `"0"`.
pub fn dec(d: Decimal) -> String {
    let n = d.normalize();
    if n.is_zero() {
        "0".to_string()
    } else {
        n.to_string()
    }
}
