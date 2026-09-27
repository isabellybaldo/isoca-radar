//! Sweet spots (Phase 5d spec `2026-09-27-phase5d-sweet-spots-design.md`): the edge each paper
//! attempt actually captured, after fees and lockup, sliced one dimension at a time. Pure: analyze
//! reads the rows (store) and calls in. No IO, no clock, no floats.
//!
//! Parameters are versioned card-side, not as a premise area (spec §6, review B1): every constant
//! that changes which rows exist or how a row is computed enters `params_text()`, whose sha8 is
//! the premise-key suffix, so an edit starts a new key and 5a's catch-up rebuilds the card from
//! stored history. (Plain labels — `OTHER`, `UNTAGGED`, `NOT_NET_POSITIVE`, `DROPPED`, `DIMS` —
//! don't change a computation, so they are left out.)
use crate::cells::{CellData, HourInput, Num, Side};
use crate::{DAY_MS, HOUR_MS, hour_of};
use arb_core::premises::sha8;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
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

/// One episode as SWEET sees it (analyze maps `store::SweetEpisodeRow` into this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweetEp {
    pub id: String,
    pub event_id: String,
    pub side: Side,
    pub opened_ms: i64,
    pub pnl_net: Decimal,
    pub budget100: Option<Decimal>,
    /// engine_suspect or stale_transport: excluded and counted (spec §4).
    pub flagged: bool,
    pub days: Option<Decimal>,
    pub annualized: Option<Decimal>,
    pub peak_cost: Decimal,
    pub s_legs: i64,
    pub tags: Option<Vec<String>>,
}

impl SweetEp {
    pub fn positive(&self) -> bool {
        self.pnl_net > Decimal::ZERO
    }

    /// (dimension, bucket) rows this episode lands in; a fee row has only "all".
    pub fn buckets(&self, fee_row: bool) -> Vec<(&'static str, &'static str)> {
        let mut out = vec![("all", "all")];
        if fee_row {
            return out;
        }
        if CATEGORY_AVAILABLE {
            out.push(("category", category_bucket(self.tags.as_deref())));
        }
        out.push(("days", days_bucket(self.days)));
        out.push(("s", s_bucket(self.s_legs)));
        out.push(("hour", hour_bucket(self.opened_ms)));
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SweetInput<'a> {
    pub now_ms: i64,
    /// Episodes of the build range; the cell keeps those opened in its hour.
    pub eps: &'a [SweetEp],
    pub attempts: &'a [Attempt],
    /// Every (side, row scenario) with an account: an episode is on the table in each, attempt or not.
    pub scenarios: &'a [(Side, String)],
}

/// One row's additive sums (spec §6). Every field merges by addition, so windows are sums of cells
/// and an expiring hour is subtracted exactly. Counts are i64; money is Num (a JSON string).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Acc {
    /// Net-positive, unflagged episodes with a $100-capped P&L, and their Σ (edge on the table).
    pub eps: i64,
    pub on_table: Num,
    /// On the table minus the episodes this row's P1 account filter-skipped (NO rows: the same).
    pub on_table_elig: Num,
    /// Net-positive but flagged episodes: excluded, counted.
    pub flagged: i64,
    /// YES upper bound (spec §7.5): Σ annualized × peak cost and Σ peak cost, over those episodes.
    pub up_w: Num,
    pub up_c: Num,
    pub attempts: i64,
    pub skipped: BTreeMap<String, i64>,
    pub filter_skips: i64,
    pub missed: i64,
    pub filled: i64,
    pub unpriced: i64,
    pub pooled: i64,
    /// Executed YES attempts whose episode has no days_to_end (annualization is then withheld).
    pub nodays: i64,
    pub sx: Num,
    pub sx2: Num,
    pub sabs: Num,
    pub sx0: Num,
    pub cost: Num,
    pub cost_days: Num,
    /// Clusters: (event, hour) groups with at least one executed attempt, and Σc², Σc·n, Σn².
    pub clusters: i64,
    pub sc2: Num,
    pub scn: Num,
    pub sn2: i64,
}

impl Acc {
    fn apply(&mut self, o: &Acc, sign: i64) {
        let s = Decimal::from(sign);
        self.eps += sign * o.eps;
        self.on_table.0 += s * o.on_table.0;
        self.on_table_elig.0 += s * o.on_table_elig.0;
        self.flagged += sign * o.flagged;
        self.up_w.0 += s * o.up_w.0;
        self.up_c.0 += s * o.up_c.0;
        self.attempts += sign * o.attempts;
        for (k, v) in &o.skipped {
            let e = self.skipped.entry(k.clone()).or_insert(0);
            *e += sign * v;
            if *e == 0 {
                self.skipped.remove(k);
            }
        }
        self.filter_skips += sign * o.filter_skips;
        self.missed += sign * o.missed;
        self.filled += sign * o.filled;
        self.unpriced += sign * o.unpriced;
        self.pooled += sign * o.pooled;
        self.nodays += sign * o.nodays;
        self.sx.0 += s * o.sx.0;
        self.sx2.0 += s * o.sx2.0;
        self.sabs.0 += s * o.sabs.0;
        self.sx0.0 += s * o.sx0.0;
        self.cost.0 += s * o.cost.0;
        self.cost_days.0 += s * o.cost_days.0;
        self.clusters += sign * o.clusters;
        self.sc2.0 += s * o.sc2.0;
        self.scn.0 += s * o.scn.0;
        self.sn2 += sign * o.sn2;
    }

    /// Executed attempts (the statistic's unit, spec §4): missed + filled.
    pub fn n_exec(&self) -> i64 {
        self.missed + self.filled
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweetCell {
    /// row_key → sums; empty rows are never stored.
    #[serde(default)]
    pub rows: BTreeMap<String, Acc>,
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Yes => "yes",
        Side::No => "no",
    }
}

pub fn row_key(side: Side, scenario: &str, dim: &str, bucket: &str) -> String {
    format!("{}|{scenario}|{dim}|{bucket}", side_str(side))
}

pub fn parse_key(k: &str) -> Option<(Side, String, String, String)> {
    let mut it = k.split('|');
    let side = match it.next()? {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => return None,
    };
    let (scen, dim, bucket) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some((side, scen.to_string(), dim.to_string(), bucket.to_string()))
}

impl SweetCell {
    fn apply(&mut self, o: &SweetCell, sign: i64) {
        for (k, a) in &o.rows {
            let e = self.rows.entry(k.clone()).or_default();
            e.apply(a, sign);
            if *e == Acc::default() {
                self.rows.remove(k);
            }
        }
    }
}

impl CellData for SweetCell {
    const FAMILY: &'static str = "sweet";
    /// Edge capture's areas (spec §6): no premise area of its own (review B1).
    const AREAS: &'static [&'static str] = &[
        "depth",
        "detector_no",
        "detector_yes",
        "episode_def",
        "fees",
        "host",
        "sim",
    ];
    const SWEET_INPUTS: bool = true;
    const LATE_MS: i64 = LATE_MS;
    type Max = ();

    fn key_suffix() -> String {
        key_suffix()
    }

    fn build(inp: &HourInput<'_>) -> Self {
        let s = &inp.sweet;
        let h = inp.hour_ms;
        let eps: BTreeMap<&str, &SweetEp> = s
            .eps
            .iter()
            .filter(|e| hour_of(e.opened_ms) == h)
            .map(|e| (e.id.as_str(), e))
            .collect();
        let mut by: BTreeMap<(&str, &str), &Attempt> = BTreeMap::new();
        for a in s.attempts {
            if eps.contains_key(a.episode_id.as_str()) {
                by.insert((a.scenario.as_str(), a.episode_id.as_str()), a);
            }
        }
        let mut c = SweetCell::default();
        // row → event → (executed n, Σx)
        let mut clus: BTreeMap<String, BTreeMap<&str, (i64, Decimal)>> = BTreeMap::new();
        for e in eps.values() {
            if !e.positive() {
                continue;
            }
            for (_, scen) in s.scenarios.iter().filter(|(sd, _)| *sd == e.side) {
                let fee_row = scen.contains('@');
                let a = by
                    .get(&(scen.as_str(), e.id.as_str()))
                    .copied()
                    .filter(|a| a.side == e.side);
                let cap = a.filter(|a| a.exec.is_some()).map(captured);
                for (dim, bucket) in e.buckets(fee_row) {
                    let key = row_key(e.side, scen, dim, bucket);
                    let acc = c.rows.entry(key.clone()).or_default();
                    if e.flagged {
                        acc.flagged += 1;
                        continue;
                    }
                    if let Some(b) = e.budget100 {
                        acc.eps += 1;
                        acc.on_table.0 += b;
                        let filtered = a
                            .and_then(|a| a.skip.as_deref())
                            .is_some_and(is_filter_skip);
                        if !filtered {
                            acc.on_table_elig.0 += b;
                        }
                    }
                    if e.side == Side::Yes
                        && let Some(ann) = e.annualized
                    {
                        acc.up_w.0 += ann * e.peak_cost;
                        acc.up_c.0 += e.peak_cost;
                    }
                    let Some(a) = a else { continue };
                    let in_flight = a.exec.is_some_and(|(t, _)| t > s.now_ms - IN_FLIGHT_MS);
                    if a.pending || in_flight || a.skip.as_deref() == Some(NOT_NET_POSITIVE) {
                        continue;
                    }
                    acc.attempts += 1;
                    if let Some(r) = &a.skip {
                        *acc.skipped.entry(r.clone()).or_insert(0) += 1;
                        if is_filter_skip(r) {
                            acc.filter_skips += 1;
                        }
                        continue;
                    }
                    let (Some((_, q)), Some(cp)) = (a.exec, cap) else {
                        continue;
                    };
                    if q > Decimal::ZERO {
                        acc.filled += 1;
                    } else {
                        acc.missed += 1;
                    }
                    acc.sx.0 += cp.x;
                    acc.sx2.0 += cp.x * cp.x;
                    acc.sabs.0 += cp.x.abs();
                    acc.sx0.0 += cp.x0;
                    acc.cost.0 += cp.cost;
                    acc.unpriced += cp.unpriced;
                    acc.pooled += cp.pooled;
                    if e.side == Side::Yes {
                        match e.days {
                            Some(d) => acc.cost_days.0 += cp.cost * d,
                            None => acc.nodays += 1,
                        }
                    }
                    let cl = clus
                        .entry(key)
                        .or_default()
                        .entry(e.event_id.as_str())
                        .or_insert((0, Decimal::ZERO));
                    cl.0 += 1;
                    cl.1 += cp.x;
                }
            }
        }
        for (key, events) in clus {
            if let Some(acc) = c.rows.get_mut(&key) {
                for (n, cx) in events.values() {
                    acc.clusters += 1;
                    acc.sc2.0 += *cx * *cx;
                    acc.scn.0 += *cx * Decimal::from(*n);
                    acc.sn2 += n * n;
                }
            }
        }
        c.rows.retain(|_, a| *a != Acc::default());
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) {}

    fn merge_max(_: &mut (), _: &()) {}
}
