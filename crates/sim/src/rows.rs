//! What the paper trader emits. Plain rows: S3c maps them onto the `paper_*` tables.
use crate::account::LedgerEntry;
use crate::types::LegKey;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimOut {
    Intent(IntentRow),
    Exec(ExecRow),
    Fill(FillRow),
    Ledger(LedgerEntry),
    Markout(MarkoutRow),
    Account(AccountRow),
}

/// A policy's decision on an opened episode (taken or skipped with a reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentRow {
    pub account: String,
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
pub struct ExecRow {
    pub account: String,
    pub episode_id: String,
    pub exec_ms: i64,
    pub q_eff: Decimal,
    pub leg_risk_qty: Decimal,
    /// Π of the same basket on the books at t_x minus consumed liquidity (None: a book was missing).
    pub pnl_at_tx: Option<Decimal>,
    pub hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillRow {
    pub account: String,
    pub episode_id: String,
    pub leg: LegKey,
    pub t_ms: i64,
    pub side: &'static str,
    pub limit: Option<Decimal>,
    pub requested: Decimal,
    pub filled: Decimal,
    pub vwap: Option<Decimal>,
    pub fees: Decimal,
    pub levels_used: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkoutRow {
    pub account: String,
    pub episode_id: String,
    pub leg: LegKey,
    pub fill_vwap: Decimal,
    pub offset_ms: i64,
    pub mid: Option<Decimal>,
    /// mid − fill VWAP, per share, from the buyer's side (negative = the market moved against us).
    pub markout: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub account: String,
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
