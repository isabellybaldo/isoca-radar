//! Sweet spots (Phase 5d spec `2026-09-27-phase5d-sweet-spots-design.md`): the edge each paper
//! attempt actually captured, after fees and lockup, sliced one dimension at a time. Pure: analyze
//! reads the rows (store) and calls in. No IO, no clock, no floats.
//!
//! Parameters are versioned card-side, not as a premise area (spec §6, review B1): every constant
//! that changes which rows exist or how a row is computed enters `params_text()`, whose sha8 is
//! the premise-key suffix, so an edit starts a new key and 5a's catch-up rebuilds the card from
//! stored history. (Plain labels — `OTHER`, `UNTAGGED`, `NOT_NET_POSITIVE`, `DROPPED`, `DIMS` —
//! don't change a computation, so they are left out.)
use crate::cells::Side;
use crate::{DAY_MS, HOUR_MS};
use arb_core::premises::sha8;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const SWEET_VERSION: &str = "1";
/// Spec §7.1: a verdict needs this many executed attempts and this many event-hour clusters.
pub const MIN_EXEC: i64 = 30;
pub const MIN_CLUSTERS: i64 = 30;
/// Spec §4: executed less than this before the tick: conversion, unwinds and the 30 s markout may
/// not be written yet, so the attempt waits for a later tick.
pub const IN_FLIGHT_MS: i64 = 60_000;
/// Spec §6: every tick rebuilds every hour touched by [last tick − LATE_MS, now].
pub const LATE_MS: i64 = 600_000;
/// Spec §4: leftovers are valued at this markout's mid.
pub const MARKOUT_MS: i64 = 30_000;
/// Spec §7.2: below this effective n, "few trades carry this row".
pub const FEW_TRADES_EFF_N: i64 = 5;
/// P1's `max_days_to_end` in config/policies.toml (the days bucket closes here; analyze checks it).
pub const P1_MAX_DAYS: i64 = 30;
/// The headline scenario (the same as Edge capture's).
pub const HEADLINE: &str = "sp-idle";
/// The fee-sensitivity P0 accounts at the headline scenario.
pub const FEE_BIPS: [i64; 2] = [10, 50];
/// P1's three filter skips (policy.rs:255-263): a YES row whose every intent is one of these was
/// never tested (spec §7.4).
pub const P1_FILTER_SKIPS: [&str; 3] = ["annualized_too_low", "no_lockup_figures", "too_long"];
/// Not an attempt: the episode was not net-positive for the policy (spec §4, review S8).
pub const NOT_NET_POSITIVE: &str = "not_net_positive";
/// An intent with no skip whose planned execution passed without an exec row (a restart drops
/// pending intents, trader.rs:94-95): counted as a skipped attempt with this reason.
pub const DROPPED: &str = "dropped";
/// Task 1's go/no-go. false: the Category dimension is left out everywhere.
pub const CATEGORY_AVAILABLE: bool = true;
/// Spec §3: ordered buckets; the first bucket holding one of an event's tag slugs wins. From the
/// Task 1 sample (see the plan's Results), with `putin` and `turkey` added to `world` for parity
/// with zelensky/india/poland (controller ruling, task-1-results.md).
pub const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "politics",
        &[
            "politics",
            "us-politics",
            "elections",
            "us-election",
            "trump",
            "congress",
            "senate",
            "us-presidential-election",
            "midterms",
            "governor-midterms",
            "trump-presidency",
            "governor-tossups",
            "main-election",
            "primaries",
            "senate-elections",
            "us-government",
            "house",
            "supreme-court",
            "courts",
        ],
    ),
    (
        "sports",
        &[
            "sports",
            "nba",
            "nfl",
            "mlb",
            "nhl",
            "soccer",
            "football",
            "tennis",
            "golf",
            "mma",
            "ufc",
            "f1",
            "boxing",
            "esports",
            "cricket",
            "basketball",
        ],
    ),
    (
        "crypto",
        &[
            "crypto",
            "bitcoin",
            "ethereum",
            "solana",
            "crypto-prices",
            "xrp",
            "memecoins",
            "token-launch",
            "airdrops",
            "stablecoins",
            "crypto-listings",
            "crypto-legal",
            "pumpptfun",
            "megaeth",
        ],
    ),
    (
        "economy",
        &[
            "economy",
            "finance",
            "business",
            "fed",
            "fed-rates",
            "inflation",
            "stocks",
            "earnings",
            "commodities",
            "ipos",
            "economic-policy",
            "taxes",
            "fiscal",
        ],
    ),
    (
        "world",
        &[
            "world",
            "geopolitics",
            "international-elections",
            "middle-east",
            "ukraine",
            "china",
            "europe",
            "global-elections",
            "foreign-policy",
            "israel",
            "world-elections",
            "russia",
            "nato",
            "ukraine-map",
            "world-affairs",
            "gaza",
            "international-election-props",
            "brazil",
            "france",
            "russia-capture",
            "macron",
            "uk",
            "starmer",
            "uk-labour-leadership",
            "india",
            "eu",
            "zelensky",
            "us-iran",
            "poland",
            "syria",
            "macro-geopolitics",
            "israel-election",
            "putin",
            "turkey",
        ],
    ),
    (
        "culture",
        &[
            "pop-culture",
            "culture",
            "entertainment",
            "awards",
            "music",
            "movies",
            "tv",
            "celebrities",
            "oscars",
            "grammys",
            "taylor-swift",
        ],
    ),
    (
        "tech",
        &[
            "tech",
            "technology",
            "ai",
            "science",
            "space",
            "big-tech",
            "openai",
            "sam-altman",
            "gpt-5",
        ],
    ),
];
pub const OTHER: &str = "other";
pub const UNTAGGED: &str = "untagged";
pub const DIMS: [&str; 5] = ["all", "category", "days", "s", "hour"];
pub const DAYS_BUCKETS: [&str; 7] = ["lt1", "1-7", "7-30", "30-90", "90-365", "ge365", "unknown"];
pub const S_BUCKETS: [&str; 5] = ["2", "3", "4-5", "6-10", "gt10"];
pub const HOUR_BUCKETS: [&str; 6] = ["00-04", "04-08", "08-12", "12-16", "16-20", "20-24"];
/// Spec §7.2: two-sided 5% family error over m rows, Bonferroni; the next larger m is used. z is
/// stored × 1000 (Decimal::new(z, 3)); the rows never exceed 500 (28 buckets × 9 row scenarios).
pub const Z_TABLE: [(i64, i64); 8] = [
    (1, 1960),
    (5, 2576),
    (10, 2807),
    (20, 3023),
    (50, 3291),
    (100, 3481),
    (200, 3662),
    (500, 3891),
];

/// (market_id, outcome).
pub type Leg = (String, String);

/// Every parameter, in one canonical text (the premise-key suffix hashes it; the card hash
/// includes it through `Parts.params`).
pub fn params_text() -> String {
    let mut s = format!(
        "v={SWEET_VERSION};min_exec={MIN_EXEC};min_clusters={MIN_CLUSTERS};in_flight_ms={IN_FLIGHT_MS};\
         late_ms={LATE_MS};markout_ms={MARKOUT_MS};few_eff_n={FEW_TRADES_EFF_N};p1_max_days={P1_MAX_DAYS};\
         headline={HEADLINE};category={CATEGORY_AVAILABLE};days={};s={};hour={};z=",
        DAYS_BUCKETS.join(","),
        S_BUCKETS.join(","),
        HOUR_BUCKETS.join(","),
    );
    for (m, z) in Z_TABLE {
        let _ = write!(s, "{m}:{z},");
    }
    let _ = write!(
        s,
        ";fee_bips={};filter_skips={}",
        FEE_BIPS.map(|f| f.to_string()).join(","),
        P1_FILTER_SKIPS.join(","),
    );
    s.push_str(";cats=");
    for (b, slugs) in CATEGORIES {
        let _ = write!(s, "{b}:{}|", slugs.join(","));
    }
    s
}

/// Appended to the run-derived premise key: a `;`-separated segment like an area's, so 5a's key
/// parsing (`edge::host_from_key`) is unaffected.
pub fn key_suffix() -> String {
    format!(";sweet={SWEET_VERSION}+{}", sha8(&params_text()))
}

pub fn days_bucket(days: Option<Decimal>) -> &'static str {
    let Some(d) = days else { return "unknown" };
    if d < Decimal::ZERO {
        "unknown"
    } else if d < Decimal::ONE {
        "lt1"
    } else if d < Decimal::from(7) {
        "1-7"
    } else if d <= Decimal::from(P1_MAX_DAYS) {
        "7-30"
    } else if d < Decimal::from(90) {
        "30-90"
    } else if d < Decimal::from(365) {
        "90-365"
    } else {
        "ge365"
    }
}

pub fn s_bucket(s_legs: i64) -> &'static str {
    match s_legs {
        i64::MIN..=2 => "2",
        3 => "3",
        4..=5 => "4-5",
        6..=10 => "6-10",
        _ => "gt10",
    }
}

pub fn hour_bucket(opened_ms: i64) -> &'static str {
    let block = opened_ms.rem_euclid(DAY_MS) / (4 * HOUR_MS);
    usize::try_from(block)
        .ok()
        .and_then(|i| HOUR_BUCKETS.get(i))
        .copied()
        .unwrap_or("20-24")
}

pub fn category_bucket(tags: Option<&[String]>) -> &'static str {
    let tags = match tags {
        Some(t) if !t.is_empty() => t,
        _ => return UNTAGGED,
    };
    for (bucket, slugs) in CATEGORIES {
        if tags.iter().any(|t| slugs.contains(&t.as_str())) {
            return bucket;
        }
    }
    OTHER
}

pub fn is_filter_skip(reason: &str) -> bool {
    P1_FILTER_SKIPS.contains(&reason)
}

/// The row scenario of a paper account (spec §4): P0 at fee 0 → NO rows under its scenario; P0 at
/// fee 10 or 50 → NO rows under "sp-idle@10" / "sp-idle@50", headline scenario only; P1 at fee 0 →
/// YES rows; anything else is not read.
pub fn row_scenario(policy: &str, scenario: &str, fee_bips: Decimal) -> Option<(Side, String)> {
    let fee = fee_bips.normalize();
    match policy {
        "P0-converter" if fee.is_zero() => Some((Side::No, scenario.to_string())),
        "P0-converter" if scenario == HEADLINE => FEE_BIPS
            .iter()
            .find(|f| Decimal::from(**f) == fee)
            .map(|f| (Side::No, format!("{HEADLINE}@{f}"))),
        "P1-settlement" if fee.is_zero() => Some((Side::Yes, scenario.to_string())),
        _ => None,
    }
}

/// "sp-idle@10" → ("sp-idle", 10); a plain scenario → (it, 0).
pub fn fee_of(scenario: &str) -> (String, i64) {
    match scenario.split_once('@') {
        Some((s, f)) => (s.to_string(), f.parse().unwrap_or(0)),
        None => (scenario.to_string(), 0),
    }
}

/// An intent's state for SWEET: (skip reason, pending). An intent with no skip and no exec row is
/// pending while its planned execution is at most IN_FLIGHT_MS old (exactly IN_FLIGHT_MS still
/// counts), and dropped only once it is older than that (a restart drops pending intents).
pub fn classify_intent(
    skip: Option<String>,
    planned_exec_ms: Option<i64>,
    executed: bool,
    now_ms: i64,
) -> (Option<String>, bool) {
    if skip.is_some() || executed {
        return (skip, false);
    }
    match planned_exec_ms {
        Some(t) if t >= now_ms - IN_FLIGHT_MS => (None, true),
        _ => (Some(DROPPED.to_string()), false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Buy,
    Sell,
    Convert,
}

/// One buy / sell / convert ledger row carrying the attempt's episode id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// None for a conversion (it takes `qty` from every leg the attempt bought).
    pub leg: Option<Leg>,
    pub qty: Decimal,
    pub cash_delta: Decimal,
}

/// One (paper account, episode) pair with an intent (spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub episode_id: String,
    pub side: Side,
    /// The row scenario (`row_scenario`).
    pub scenario: String,
    /// The episode's complete-set size S (its number of legs). A YES leg that filled 0 writes no
    /// ledger row (`Account::buy`, sim/account.rs:172-174), so `captured` compares this against
    /// the legs actually present in `lines` rather than trusting `net.len()` alone.
    pub s_legs: i64,
    /// The skip reason (`classify_intent`), None when executed or pending.
    pub skip: Option<String>,
    /// (exec_ms, q_eff) once executed.
    pub exec: Option<(i64, Decimal)>,
    pub pending: bool,
    pub lines: Vec<Line>,
    /// The MARKOUT_MS markout mid per leg (absent: not priced).
    pub mid30: BTreeMap<Leg, Decimal>,
}

/// The captured value of one executed attempt (spec §4). `x` values leftovers at the 30 s mid,
/// `x0` at zero; `cost` is what the buys cost, fees included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Captured {
    pub x: Decimal,
    pub x0: Decimal,
    pub cost: Decimal,
    /// Leftover legs with no 30 s mid (valued at 0 in both).
    pub unpriced: i64,
    /// Legs whose net went negative (clamped at 0, never priced).
    pub pooled: i64,
}

pub fn captured(a: &Attempt) -> Captured {
    let mut out = Captured::default();
    let mut cash = Decimal::ZERO;
    let mut converted = Decimal::ZERO;
    let mut net: BTreeMap<&Leg, Decimal> = BTreeMap::new();
    for l in &a.lines {
        cash += l.cash_delta;
        match (l.kind, &l.leg) {
            (LineKind::Buy, Some(leg)) => {
                out.cost -= l.cash_delta;
                *net.entry(leg).or_default() += l.qty;
            }
            (LineKind::Buy, None) => out.cost -= l.cash_delta,
            (LineKind::Sell, Some(leg)) => *net.entry(leg).or_default() -= l.qty,
            (LineKind::Sell, None) => {}
            (LineKind::Convert, _) => converted += l.qty,
        }
    }
    // a conversion takes its quantity from every S leg: the legs this attempt bought
    if !converted.is_zero() {
        for q in net.values_mut() {
            *q -= converted;
        }
    }
    for q in net.values_mut() {
        if *q < Decimal::ZERO {
            *q = Decimal::ZERO;
            out.pooled += 1;
        }
    }
    // Spec §4: sets = min over the S legs. A leg that filled 0 writes no row (`net` lacks it), so
    // a YES episode with fewer legs present than its S never pays out a set at all — not just a
    // smaller one.
    let sets = match a.side {
        Side::Yes if i64::try_from(net.len()).unwrap_or(i64::MAX) >= a.s_legs => {
            net.values().min().copied().unwrap_or(Decimal::ZERO)
        }
        _ => Decimal::ZERO,
    };
    let mut leftover = Decimal::ZERO;
    for (leg, q) in &net {
        let left = *q - sets;
        if left <= Decimal::ZERO {
            continue;
        }
        match a.mid30.get(*leg) {
            Some(m) => leftover += left * *m,
            None => out.unpriced += 1,
        }
    }
    out.x0 = cash + sets;
    out.x = out.x0 + leftover;
    out
}
