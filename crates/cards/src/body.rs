//! The card payload (spec §8.1): analyze stores it in card_results.body and the api serves it as
//! stored. Numbers that can be money, rates or durations are decimal strings or null; counts are
//! integers.
use crate::cells::CellData;
use crate::hour_of;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub const WINS: [&str; 5] = ["24h", "7d", "30d", "all", "today"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Live,
    Warming,
    Thin,
    UnderReview,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Live => "live",
            Status::Warming => "warming",
            Status::Thin => "thin",
            Status::UnderReview => "under_review",
        }
    }
}

/// One line of the three-column table: PAPER | THEIR METHOD | OUR METHOD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub label: String,
    pub unit: String,
    pub paper: Option<String>,
    pub their: Option<String>,
    pub ours: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub t_ms: i64,
    pub s: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Curve {
    pub label: String,
    pub points: Vec<CurvePoint>,
    pub median_ms: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bin {
    pub side: String,
    pub lo_bps: i64,
    /// None: the open top bin.
    pub hi_bps: Option<i64>,
    pub net: i64,
    pub gross: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecayPoint {
    pub scenario: String,
    /// proc + net + match of the scenario (policies.toml).
    pub latency_ms: i64,
    pub net_ms: i64,
    pub captured: Option<String>,
    pub rate_pct: Option<String>,
    pub headline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measured {
    pub latency_ms: i64,
    pub host: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bars: Vec<Bar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<Curve>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<Bin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decay: Vec<DecayPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Measured>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardBody {
    pub card: String,
    pub paper: String,
    pub title: String,
    pub win: String,
    pub status: Status,
    pub premise_key: String,
    pub hash: String,
    pub data_from_ms: Option<i64>,
    pub data_to_ms: Option<i64>,
    /// When the current premise key's data starts ("since ⟨date⟩ · ⟨x⟩ days of data").
    pub since_ms: Option<i64>,
    pub n: i64,
    pub columns: Vec<Row>,
    pub series: Series,
    pub notes: Vec<String>,
    pub thin_reasons: Vec<String>,
    pub episode_link: Option<String>,
    pub computed_ms: i64,
}

/// A latency scenario of policies.toml, as the Edge capture decay chart plots it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioLat {
    pub name: String,
    pub net_ms: i64,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct BodyCtx<'a> {
    pub win: &'a str,
    /// under_review / warming / live, before the card's own thin check.
    pub base: Status,
    pub premise_key: &'a str,
    pub analyze_version: &'a str,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub since_ms: Option<i64>,
    pub computed_ms: i64,
    pub scenarios: &'a [ScenarioLat],
}

/// A cell family that renders cards from a window's merged cell.
pub trait Card: CellData {
    /// The cards' window-independent parts. Must read from `ctx` only `premise_key` and
    /// `scenarios` (the same for every window of a key).
    fn parts(agg: &Self, max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<Parts>;

    fn bodies(agg: &Self, max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        Self::parts(agg, max, ctx)
            .into_iter()
            .map(|p| make(p, ctx))
            .collect()
    }
}

/// Spec §4: under_review while catch-up runs; warming when the key's data starts inside the
/// window (or no data yet); otherwise live. `window_start_ms` None is `all` (never warming).
pub fn base_status(
    catching_up: bool,
    since_ms: Option<i64>,
    window_start_ms: Option<i64>,
) -> Status {
    if catching_up {
        return Status::UnderReview;
    }
    match (since_ms, window_start_ms) {
        (None, _) => Status::Warming,
        (Some(since), Some(start)) if since > start => Status::Warming,
        _ => Status::Live,
    }
}

/// Precedence: under_review, warming, thin, live. Thin reasons show whatever the status.
pub fn final_status(base: Status, thin: &[String]) -> Status {
    if base == Status::Live && !thin.is_empty() {
        Status::Thin
    } else {
        base
    }
}

/// sha256 of the premise key, the analyze version, the window's data range (first and last hour),
/// the card, the window and the card's parameter set (spec §4).
pub fn repro_hash(
    premise_key: &str,
    analyze_version: &str,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    card: &str,
    win: &str,
    params: &str,
) -> String {
    let hour = |v: Option<i64>| v.map(|x| hour_of(x).to_string()).unwrap_or_default();
    let text = format!(
        "{premise_key}\n{analyze_version}\n{}\n{}\n{card}\n{win}\n{params}",
        hour(from_ms),
        hour(to_ms)
    );
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(text.as_bytes()) {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub fn row(
    label: &str,
    unit: &str,
    paper: Option<&str>,
    their: Option<String>,
    ours: Option<String>,
) -> Row {
    Row {
        label: label.to_string(),
        unit: unit.to_string(),
        paper: paper.map(str::to_string),
        their,
        ours,
    }
}

/// The parts of a card that differ per card. They depend on the window's merged cell (and the
/// window-independent premise key and scenarios) only, never on the window itself, so two windows
/// with the same merged cell can share them (`all` reuses `30d` while the key is younger).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts {
    pub card: &'static str,
    pub paper: &'static str,
    pub title: &'static str,
    pub params: String,
    pub n: i64,
    pub columns: Vec<Row>,
    pub series: Series,
    pub notes: Vec<String>,
    pub thin: Vec<String>,
    pub episode_link: Option<String>,
}

pub fn make(p: Parts, ctx: &BodyCtx<'_>) -> CardBody {
    CardBody {
        card: p.card.to_string(),
        paper: p.paper.to_string(),
        title: p.title.to_string(),
        win: ctx.win.to_string(),
        status: final_status(ctx.base, &p.thin),
        premise_key: ctx.premise_key.to_string(),
        hash: repro_hash(
            ctx.premise_key,
            ctx.analyze_version,
            ctx.from_ms,
            ctx.to_ms,
            p.card,
            ctx.win,
            &p.params,
        ),
        data_from_ms: ctx.from_ms,
        data_to_ms: ctx.to_ms,
        since_ms: ctx.since_ms,
        n: p.n,
        columns: p.columns,
        series: p.series,
        notes: p.notes,
        thin_reasons: p.thin,
        episode_link: p.episode_link,
        computed_ms: ctx.computed_ms,
    }
}
