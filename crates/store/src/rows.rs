//! Row types shared with the scanner. `Option` = may be unmeasured (NULL); a measured zero is `Some(0)`.
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub service: String,
    pub started_ms: i64,
    pub version: String,
    pub config_hash: String,
    /// live | safe | replay | census
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct UniverseSnapshot {
    pub taken_ms: i64,
    pub n_events: i32,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub event_id: String,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub neg_risk_market_id: Option<String>,
    pub augmented: bool,
    pub n_legs: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegRow {
    pub market_id: String,
    pub event_id: String,
    pub yes_token: String,
    pub no_token: String,
    /// live | placeholder | other | closed_no | closed_yes | halted
    pub status: String,
    pub fee_rate: Decimal,
    pub fee_exponent: i16,
    pub tick: Option<Decimal>,
    pub min_size: Option<Decimal>,
    pub seconds_delay: Option<i32>,
    pub end_date: Option<String>,
}

/// One episode (spec §10 column list). Every column is rewritten on each upsert (open → peak → close).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EpisodeRow {
    pub episode_id: String,
    pub run_id: i64,
    /// live | replay
    pub source: String,
    pub kind: String,
    pub realization: String,
    pub event_id: String,
    pub event_slug: Option<String>,
    pub event_title: Option<String>,
    pub neg_risk_market_id: Option<String>,
    pub pair_id: Option<String>,
    pub category: Option<String>,
    pub n_outcomes: i32,
    pub n_legs: i32,
    pub legs_dropped: i32,
    pub augmented: bool,
    pub incomplete_set: bool,
    pub uncovered_legs: i32,
    pub opened_server_ms: i64,
    pub closed_server_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    pub detect_lag_ms: Option<i64>,
    pub n_updates: i32,
    pub peak_server_ms: i64,
    pub peak_edge_top: Decimal,
    pub peak_edge_top_gross: Decimal,
    pub peak_max_size: Decimal,
    pub peak_cost_usd: Decimal,
    pub peak_payout_usd: Decimal,
    pub peak_fees_usd: Decimal,
    pub peak_pnl_net_usd: Decimal,
    pub peak_pnl_gross_usd: Decimal,
    pub peak_profit_per_dollar: Option<Decimal>,
    pub pnl_net_budget100_usd: Option<Decimal>,
    pub bottleneck_leg_idx: Option<i32>,
    pub bottleneck_notional_usd: Option<Decimal>,
    pub below_min_size: Option<bool>,
    pub max_seconds_delay: Option<i32>,
    pub days_to_end: Option<Decimal>,
    pub annualized_return: Option<Decimal>,
    pub post_end: Option<bool>,
    pub engine_suspect: bool,
    pub stale_transport: bool,
    pub open_lag_ms: Option<i64>,
    pub oracle_downgraded: Option<bool>,
    pub scanner_version: String,
    pub config_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EpisodeLegRow {
    pub episode_id: String,
    pub leg_idx: i32,
    pub market_id: String,
    pub token: String,
    /// buy_yes | buy_no
    pub action: String,
    pub best_price: Option<Decimal>,
    pub vwap: Option<Decimal>,
    pub levels: i32,
    pub size_at_best: Option<Decimal>,
    pub filled: Decimal,
    pub notional: Decimal,
    pub fees: Decimal,
    pub in_s: bool,
    pub fee_rate: Decimal,
    pub fee_exponent: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeWrite {
    pub row: EpisodeRow,
    pub legs: Vec<EpisodeLegRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRow {
    pub episode_id: String,
    pub offset_ms: i32,
    /// ws | rest
    pub source: String,
    pub observed_ms: Option<i64>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub edge: Option<Decimal>,
    pub q_max: Option<Decimal>,
    pub pnl: Option<Decimal>,
    pub still_profitable: Option<bool>,
    pub fill_at_original_size: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityRow {
    pub run_id: i64,
    pub ts_ms: i64,
    /// crossed_book | mirror_mismatch | top_mismatch | drift | drop | reconnect_gap | parse_error | shard_parked
    pub kind: String,
    pub shard: Option<i32>,
    pub token: Option<String>,
    pub event_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthRow {
    pub run_id: i64,
    pub minute_ms: i64,
    pub tokens: i32,
    pub shards: i32,
    pub msgs: i64,
    pub bytes: i64,
    pub drops: i32,
    pub reconnects: i32,
    pub rest_p50_ms: Option<i32>,
    pub rest_p99_ms: Option<i32>,
    pub lag_p50_ms: Option<i32>,
    pub lag_p99_ms: Option<i32>,
    pub clock_offset_ms: Option<i32>,
    pub ok: Option<bool>,
    pub capture_paused: bool,
    pub parked_shards: i32,
    pub dropped_groups: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    pub episodes: Vec<EpisodeWrite>,
    pub observations: Vec<ObservationRow>,
    pub integrity: Vec<IntegrityRow>,
    pub health: Vec<HealthRow>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
            && self.observations.is_empty()
            && self.integrity.is_empty()
            && self.health.is_empty()
    }
}
