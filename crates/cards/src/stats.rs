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

/// Two sorted maps as one sorted stream, equal keys combined with `plus`. Collected into one
/// Vec so the stream is `Clone` (Kaplan–Meier walks it twice).
pub fn merge_sorted<'a, V: Copy + 'a>(
    a: &'a BTreeMap<i64, V>,
    b: &'a BTreeMap<i64, V>,
    plus: fn(V, V) -> V,
) -> impl Iterator<Item = (i64, V)> + Clone + 'a {
    let mut ia = a.iter().peekable();
    let mut ib = b.iter().peekable();
    let mut out: Vec<(i64, V)> = Vec::new();
    loop {
        let ka = ia.peek().map(|(k, _)| **k);
        let kb = ib.peek().map(|(k, _)| **k);
        match (ka, kb) {
            (None, None) => break,
            (Some(x), Some(y)) if x == y => {
                if let (Some((k, va)), Some((_, vb))) = (ia.next(), ib.next()) {
                    out.push((*k, plus(*va, *vb)));
                }
            }
            (Some(x), Some(y)) if x < y => {
                if let Some((k, v)) = ia.next() {
                    out.push((*k, *v));
                }
            }
            (Some(_), None) => {
                if let Some((k, v)) = ia.next() {
                    out.push((*k, *v));
                }
            }
            _ => {
                if let Some((k, v)) = ib.next() {
                    out.push((*k, *v));
                }
            }
        }
    }
    out.into_iter()
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
    debug_assert!(
        grid.windows(2).all(|w| w[0] <= w[1]),
        "km: grid must be ascending, got {grid:?}"
    );
    let (events, censored) = entries
        .clone()
        .fold((0i64, 0i64), |a, (_, v)| (a.0 + v.0, a.1 + v.1));
    let mut at_risk = events + censored;
    let mut s = FP_ONE;
    let mut at = Vec::with_capacity(grid.len());
    let mut gi = 0usize;
    let mut median = None;
    for (t, (e, c)) in entries {
        debug_assert!(
            e >= 0 && c >= 0,
            "km: negative counts (e={e}, c={c}) at t={t}"
        );
        debug_assert!(
            e <= at_risk,
            "km: events ({e}) exceed at-risk ({at_risk}) at t={t}"
        );
        while gi < grid.len() && grid.get(gi).is_some_and(|g| *g < t) {
            at.push(s);
            gi += 1;
        }
        if e > 0 && at_risk > 0 {
            let factor = i128::from(at_risk - e);
            let product = s.checked_mul(factor);
            debug_assert!(
                product.is_some(),
                "km: i128 overflow computing the survival product at t={t} (s={s}, factor={factor})"
            );
            // Release fallback only: saturate toward the largest representable value rather
            // than wrapping to 0. Unreachable for realistic episode counts (s ≤ FP_ONE ≈ 1e28,
            // factor bounded by the at-risk count), so no test exercises this arm.
            s = product.unwrap_or(i128::MAX) / i128::from(at_risk);
            s = s.max(0); // defence in depth: a corrupted window sum must never yield a negative Ŝ
            if median.is_none() && s.saturating_mul(2) <= FP_ONE {
                median = Some(t);
            }
        }
        at_risk -= e + c;
    }
    while gi < grid.len() {
        at.push(s);
        gi += 1;
    }
    KmSummary {
        at,
        median,
        events,
        censored,
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
    grid.iter()
        .map(|tau| {
            let above: i64 = m.range(tau.saturating_add(1)..).map(|(_, c)| c).sum();
            Some(Decimal::from(above) / Decimal::from(n))
        })
        .collect()
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
