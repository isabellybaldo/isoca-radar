//! Paper trading (spec §4) over S3c's tables. Empty lists, never errors, until they exist.
use crate::dto::{Dec, Page, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use store::{PaperAccountNow, PaperLedgerEntry, PaperPosition};

#[derive(Debug, Serialize)]
pub struct AccountDto {
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Dec,
    pub bankroll: Dec,
    pub run_id: i64,
    pub marked_ms: Option<i64>,
    pub cash: Option<Dec>,
    pub equity: Option<Dec>,
    pub realized: Option<Dec>,
    pub unrealized: Option<Dec>,
    pub fees_paid: Option<Dec>,
    pub locked: Option<Dec>,
    pub max_drawdown: Option<Dec>,
    pub n_positions: Option<i64>,
}

impl From<PaperAccountNow> for AccountDto {
    fn from(a: PaperAccountNow) -> Self {
        Self {
            account_id: a.account_id,
            policy: a.policy,
            scenario: a.scenario,
            fee_bips: Dec(a.fee_bips),
            bankroll: Dec(a.bankroll),
            run_id: a.run_id,
            marked_ms: a.t_ms,
            cash: opt(a.cash),
            equity: opt(a.equity),
            realized: opt(a.realized),
            unrealized: opt(a.unrealized),
            fees_paid: opt(a.fees_paid),
            locked: opt(a.locked),
            max_drawdown: opt(a.max_drawdown),
            n_positions: a.n_positions,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ItemsDto<T> {
    pub items: Vec<T>,
}

#[derive(Debug, Serialize)]
pub struct EquityPointDto {
    pub t_ms: i64,
    pub equity: Dec,
}

#[derive(Debug, Serialize)]
pub struct EquityDto {
    pub account_id: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub points: Vec<EquityPointDto>,
}

#[derive(Debug, Serialize)]
pub struct PositionDto {
    pub account_id: String,
    pub market_id: String,
    pub outcome: String,
    pub qty: Dec,
    pub basis: Dec,
    pub returned: bool,
    pub event_title: Option<String>,
}

impl From<PaperPosition> for PositionDto {
    fn from(p: PaperPosition) -> Self {
        Self {
            account_id: p.account_id,
            market_id: p.market_id,
            outcome: p.outcome,
            qty: Dec(p.qty),
            basis: Dec(p.basis),
            returned: p.returned,
            event_title: p.event_title,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LedgerDto {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub outcome: Option<String>,
    pub qty: Dec,
    pub price: Option<Dec>,
    pub cash_delta: Dec,
    pub fees: Dec,
    pub realized_delta: Dec,
    pub cash_after: Dec,
    pub equity: Option<Dec>,
}

impl From<PaperLedgerEntry> for LedgerDto {
    fn from(l: PaperLedgerEntry) -> Self {
        Self {
            run_id: l.run_id,
            account_id: l.account_id,
            seq: l.seq,
            t_ms: l.t_ms,
            kind: l.kind,
            episode_id: l.episode_id,
            market_id: l.market_id,
            outcome: l.outcome,
            qty: Dec(l.qty),
            price: opt(l.price),
            cash_delta: Dec(l.cash_delta),
            fees: Dec(l.fees),
            realized_delta: Dec(l.realized_delta),
            cash_after: Dec(l.cash_after),
            equity: opt(l.equity),
        }
    }
}

fn account(q: &Params) -> Result<String, ApiError> {
    params::text(q, "account", 128, "bad_account")?
        .map(str::to_string)
        .ok_or_else(|| ApiError::BadRequest("bad_account", "account is required.".to_string()))
}

fn decode_ledger_cursor(s: &str) -> Option<(i64, i64)> {
    let (run, seq) = s.split_once('_')?;
    Some((run.parse().ok()?, seq.parse().ok()?))
}

pub async fn accounts(State(st): State<AppState>) -> Result<Json<ItemsDto<AccountDto>>, ApiError> {
    let items = st
        .store
        .paper_accounts_now()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ItemsDto { items }))
}

pub async fn equity(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<EquityDto>, ApiError> {
    let q = params::query(q)?;
    let account_id = account(&q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * 3_600_000;
    let points = st
        .store
        .paper_equity(&account_id, from_ms, to_ms)
        .await?
        .into_iter()
        .map(|p| EquityPointDto {
            t_ms: p.t_ms,
            equity: Dec(p.equity),
        })
        .collect();
    Ok(Json(EquityDto {
        account_id,
        from_ms,
        to_ms,
        points,
    }))
}

pub async fn positions(
    State(st): State<AppState>,
) -> Result<Json<ItemsDto<PositionDto>>, ApiError> {
    let items = st
        .store
        .paper_positions()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ItemsDto { items }))
}

pub async fn ledger(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<Page<LedgerDto>>, ApiError> {
    let q = params::query(q)?;
    let account_id = account(&q)?;
    let before = match params::text(&q, "cursor", 64, "bad_cursor")? {
        None => None,
        Some(c) => Some(decode_ledger_cursor(c).ok_or_else(|| {
            ApiError::BadRequest("bad_cursor", "That cursor is not one of ours.".to_string())
        })?),
    };
    let limit = params::limit(&q)?;
    let page = st
        .store
        .paper_ledger_page(&account_id, before, limit)
        .await?;
    Ok(Json(Page {
        items: page.rows.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|(run, seq)| format!("{run}_{seq}")),
    }))
}
