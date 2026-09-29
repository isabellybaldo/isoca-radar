//! Phase 7a (spec 2026-09-29 §2, §7.1): the latency ladder's pure parts. `break_even` answers
//! "at which latency would this pay?" from each rung's net since the epoch start; `measured` is
//! Edge capture's latency estimate (the same per-minute figure and nearest-rank median).
use crate::stats::{self, CountMap};
use crate::sweet::MIN_EXEC;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;

/// The break-even latency is rounded to this many ms.
pub const ROUND_MS: i64 = 5;

/// One paper account's figures since the epoch start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rung {
    /// proc + net + match from policies.toml; None: unknown (never placed).
    pub latency_ms: Option<i64>,
    /// sp-burst / sp-idle-nodelay: shown, never computed with.
    pub extra: bool,
    pub n_exec: i64,
    /// Latest mark equity − bankroll.
    pub net: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// ≥ 0 then < 0: the expected shape.
    LosesBeyond,
    /// < 0 then ≥ 0: the faster rungs lose.
    PaysBeyond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BreakEven {
    /// Some ladder rung has fewer than `MIN_EXEC` executed attempts (SWEET's minimum).
    Thin,
    #[serde(rename = "all")]
    PaysEverywhere,
    #[serde(rename = "none")]
    PaysNowhere,
    At {
        latency_ms: i64,
        direction: Direction,
        #[serde(skip_serializing_if = "is_false")]
        not_monotone: bool,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Spec 7a §7.1, defined for every sign pattern: `thin` if any ladder rung has n_exec < 30;
/// otherwise `all` (every net ≥ 0), `none` (every net < 0), or `at` the FIRST sign change between
/// adjacent rungs by latency, linearly interpolated, rounded to 5 ms and clamped between those two
/// rungs, with `not_monotone` when the sign changes more than once. Extras and rungs with an
/// unknown latency never enter. One epoch is one path: there is no interval.
pub fn break_even(rungs: &[Rung]) -> BreakEven {
    let mut ladder: Vec<(i64, i64, Decimal)> = rungs
        .iter()
        .filter(|r| !r.extra)
        .filter_map(|r| r.latency_ms.map(|l| (l, r.n_exec, r.net)))
        .collect();
    if ladder.is_empty() || ladder.iter().any(|&(_, n, _)| n < MIN_EXEC) {
        return BreakEven::Thin;
    }
    ladder.sort_by_key(|&(l, _, _)| l);
    let pays = |x: Decimal| x >= Decimal::ZERO;
    let changes: Vec<usize> = ladder
        .windows(2)
        .enumerate()
        .filter(|(_, w)| pays(w[0].2) != pays(w[1].2))
        .map(|(i, _)| i)
        .collect();
    // No sign change means one sign throughout, so the first rung says which (the ladder is
    // non-empty here); every arm either yields the change or names its real answer.
    let i = match (changes.first(), ladder.first()) {
        (Some(&i), _) => i,
        (None, Some(&(_, _, x))) if pays(x) => return BreakEven::PaysEverywhere,
        (None, _) => return BreakEven::PaysNowhere,
    };
    let ((a, _, x), (b, _, y)) = (ladder[i], ladder[i + 1]);
    // x and y have opposite signs, so x − y ≠ 0 and x / (x − y) is in [0, 1]
    let frac = x.checked_div(x - y).unwrap_or(Decimal::ZERO);
    let v = Decimal::from(a) + Decimal::from(b - a) * frac;
    let step = Decimal::from(ROUND_MS);
    let rounded =
        (v / step).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero) * step;
    let latency_ms = rounded.to_i64().unwrap_or(a).clamp(a, b);
    BreakEven::At {
        latency_ms,
        direction: if pays(x) {
            Direction::LosesBeyond
        } else {
            Direction::PaysBeyond
        },
        not_monotone: changes.len() > 1,
    }
}

/// Edge capture's measured latency of one health minute: feed lag p50 + REST round trip p50 / 2,
/// when both were measured (Phase 5a spec §6.2). Shared by analyze and the api.
pub fn minute_latency(lag_p50_ms: Option<i32>, rest_rtt_p50_ms: Option<i32>) -> Option<i64> {
    Some(i64::from(lag_p50_ms?) + i64::from(rest_rtt_p50_ms?) / 2)
}

/// The nearest-rank median of `minute_latency` over the minutes that have one.
pub fn measured(minutes: impl Iterator<Item = (Option<i32>, Option<i32>)>) -> Option<i64> {
    let mut m = CountMap::new();
    for (lag, rtt) in minutes {
        if let Some(v) = minute_latency(lag, rtt) {
            *m.entry(v).or_insert(0) += 1;
        }
    }
    let n = stats::total(&m);
    stats::nearest_rank(m.iter().map(|(v, k)| (*v, *k)), n, 50)
}
