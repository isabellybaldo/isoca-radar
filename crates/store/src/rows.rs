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

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
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

/// One paper account of a run: its pre-registered spec (spec §11) and the policies file hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperAccountRow {
    pub run_id: i64,
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Decimal,
    pub bankroll: Decimal,
    pub per_trade: Decimal,
    pub policies_sha256: String,
}

/// A policy's decision on an opened episode (taken, or skipped with a reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperIntentRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub decided_ms: i64,
    pub exec_ms: Option<i64>,
    pub skip_reason: Option<String>,
    pub q_target: Option<Decimal>,
    pub detected_q: Option<Decimal>,
    pub detected_pnl: Option<Decimal>,
    pub reserve: Option<Decimal>,
}

/// What happened at t_x.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperExecRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub exec_ms: i64,
    pub q_eff: Decimal,
    pub leg_risk_qty: Decimal,
    pub pnl_at_tx: Option<Decimal>,
    pub hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperFillRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub market_id: String,
    pub yes_token: String,
    /// yes | no
    pub outcome: String,
    /// buy | sell
    pub side: String,
    pub t_ms: i64,
    pub limit_price: Option<Decimal>,
    pub requested: Decimal,
    pub filled: Decimal,
    pub vwap: Option<Decimal>,
    pub fees: Decimal,
    pub levels_used: i32,
}

/// One `sim::account::LedgerEntry`. Keyed (run_id, account_id, seq).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperLedgerRow {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    /// buy | sell | convert | settle | mark
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub yes_token: Option<String>,
    pub outcome: Option<String>,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub cash_delta: Decimal,
    pub fees: Decimal,
    pub realized_delta: Decimal,
    pub cash_after: Decimal,
    pub equity: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperMarkoutRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub market_id: String,
    pub yes_token: String,
    pub outcome: String,
    pub fill_vwap: Decimal,
    pub offset_ms: i64,
    pub mid: Option<Decimal>,
    pub markout: Option<Decimal>,
}

/// One `sim::rows::AccountRow` (a mark). Identity: equity = bankroll + realized + unrealized.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperMarkRow {
    pub run_id: i64,
    pub account_id: String,
    pub t_ms: i64,
    pub cash: Decimal,
    pub equity: Decimal,
    pub realized: Decimal,
    pub unrealized: Decimal,
    pub fees_paid: Decimal,
    pub locked: Decimal,
    pub max_drawdown: Decimal,
    pub n_positions: i64,
}

/// Live only: one serialized `sim::account::Account` per account id (restored at start).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperStateRow {
    pub account_id: String,
    pub state: String,
    pub updated_ms: i64,
    /// The policies sha256 this account was saved under (migration 0005, ruling 26). `NULL` for a
    /// row saved before this column existed: a restore treats that as a mismatch, never a match.
    pub policies_sha256: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    pub episodes: Vec<EpisodeWrite>,
    pub observations: Vec<ObservationRow>,
    pub integrity: Vec<IntegrityRow>,
    pub health: Vec<HealthRow>,
    /// Paper trading (S3c). Accounts are written first: every other paper row references one.
    pub paper_accounts: Vec<PaperAccountRow>,
    pub paper_intents: Vec<PaperIntentRow>,
    pub paper_execs: Vec<PaperExecRow>,
    pub paper_fills: Vec<PaperFillRow>,
    pub paper_ledger: Vec<PaperLedgerRow>,
    pub paper_markouts: Vec<PaperMarkoutRow>,
    pub paper_marks: Vec<PaperMarkRow>,
    /// Live only: upserted by account id (the newest `updated_ms` wins).
    pub paper_state: Vec<PaperStateRow>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.rows() == 0
    }

    pub fn paper_rows(&self) -> usize {
        self.paper_accounts.len()
            + self.paper_intents.len()
            + self.paper_execs.len()
            + self.paper_fills.len()
            + self.paper_ledger.len()
            + self.paper_markouts.len()
            + self.paper_marks.len()
            + self.paper_state.len()
    }

    pub fn rows(&self) -> usize {
        self.episodes.len()
            + self.observations.len()
            + self.integrity.len()
            + self.health.len()
            + self.paper_rows()
    }

    /// Move the paper rows out into a batch of their own (the rest stays here).
    pub fn take_paper(&mut self) -> Batch {
        Batch {
            paper_accounts: std::mem::take(&mut self.paper_accounts),
            paper_intents: std::mem::take(&mut self.paper_intents),
            paper_execs: std::mem::take(&mut self.paper_execs),
            paper_fills: std::mem::take(&mut self.paper_fills),
            paper_ledger: std::mem::take(&mut self.paper_ledger),
            paper_markouts: std::mem::take(&mut self.paper_markouts),
            paper_marks: std::mem::take(&mut self.paper_marks),
            paper_state: std::mem::take(&mut self.paper_state),
            ..Batch::default()
        }
    }

    /// Append `other`'s paper rows after ours, in order. `other`'s non-paper rows are ignored.
    pub fn extend_paper(&mut self, other: Batch) {
        self.paper_accounts.extend(other.paper_accounts);
        self.paper_intents.extend(other.paper_intents);
        self.paper_execs.extend(other.paper_execs);
        self.paper_fills.extend(other.paper_fills);
        self.paper_ledger.extend(other.paper_ledger);
        self.paper_markouts.extend(other.paper_markouts);
        self.paper_marks.extend(other.paper_marks);
        self.paper_state.extend(other.paper_state);
    }

    /// Every row as its own batch (the writer's quarantine path). Episodes come before the
    /// observations that reference them, and paper accounts before the paper rows.
    pub fn into_singles(self) -> Vec<Batch> {
        let mut out = Vec::with_capacity(self.rows());
        macro_rules! each {
            ($($field:ident),*) => {$(
                for x in self.$field {
                    out.push(Batch { $field: vec![x], ..Batch::default() });
                }
            )*};
        }
        each!(
            episodes,
            observations,
            integrity,
            health,
            paper_accounts,
            paper_intents,
            paper_execs,
            paper_fills,
            paper_ledger,
            paper_markouts,
            paper_marks,
            paper_state
        );
        out
    }
}
