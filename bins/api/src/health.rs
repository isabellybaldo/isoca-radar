//! GET /api/health/now and GET /api/health/series?hours= (spec §4).
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use store::{HealthNow, HealthRow};

const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Serialize)]
pub struct HealthNowDto {
    pub now_ms: i64,
    /// now − minute_ms (server clock).
    pub age_ms: i64,
    pub run_id: i64,
    pub minute_ms: i64,
    pub tokens: i32,
    pub shards: i32,
    pub parked_shards: i32,
    /// Messages in that minute.
    pub msgs: i64,
    pub drops: i32,
    pub drops_5m: i64,
    pub reconnects: i32,
    pub lag_p50_ms: Option<i32>,
    pub lag_p99_ms: Option<i32>,
    pub rest_p50_ms: Option<i32>,
    pub rest_p99_ms: Option<i32>,
    pub clock_offset_ms: Option<i32>,
    pub ok: Option<bool>,
    pub capture_paused: bool,
    pub dropped_groups: i32,
}

impl HealthNowDto {
    fn new(h: HealthNow, now: i64) -> Self {
        let r = h.row;
        Self {
            now_ms: now,
            age_ms: now.saturating_sub(r.minute_ms).max(0),
            run_id: r.run_id,
            minute_ms: r.minute_ms,
            tokens: r.tokens,
            shards: r.shards,
            parked_shards: r.parked_shards,
            msgs: r.msgs,
            drops: r.drops,
            drops_5m: h.drops_5m,
            reconnects: r.reconnects,
            lag_p50_ms: r.lag_p50_ms,
            lag_p99_ms: r.lag_p99_ms,
            rest_p50_ms: r.rest_p50_ms,
            rest_p99_ms: r.rest_p99_ms,
            clock_offset_ms: r.clock_offset_ms,
            ok: r.ok,
            capture_paused: r.capture_paused,
            dropped_groups: r.dropped_groups,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HealthEnvelope {
    pub health: Option<HealthNowDto>,
}

#[derive(Debug, Serialize)]
pub struct HealthPointDto {
    pub minute_ms: i64,
    pub run_id: i64,
    pub msgs: i64,
    pub drops: i32,
    pub reconnects: i32,
    pub lag_p50_ms: Option<i32>,
    pub lag_p99_ms: Option<i32>,
    pub shards: i32,
    pub parked_shards: i32,
    pub capture_paused: bool,
}

impl From<HealthRow> for HealthPointDto {
    fn from(r: HealthRow) -> Self {
        Self {
            minute_ms: r.minute_ms,
            run_id: r.run_id,
            msgs: r.msgs,
            drops: r.drops,
            reconnects: r.reconnects,
            lag_p50_ms: r.lag_p50_ms,
            lag_p99_ms: r.lag_p99_ms,
            shards: r.shards,
            parked_shards: r.parked_shards,
            capture_paused: r.capture_paused,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SeriesDto {
    pub from_ms: i64,
    pub to_ms: i64,
    pub points: Vec<HealthPointDto>,
}

pub async fn now(State(st): State<AppState>) -> Result<Json<HealthEnvelope>, ApiError> {
    let now = now_ms();
    let h = st.store.health_now(now).await?;
    Ok(Json(HealthEnvelope {
        health: h.map(|h| HealthNowDto::new(h, now)),
    }))
}

pub async fn series(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<SeriesDto>, ApiError> {
    let q = params::query(q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * HOUR_MS;
    let points = st
        .store
        .health_series(from_ms, to_ms)
        .await?
        .into_iter()
        .map(HealthPointDto::from)
        .collect();
    Ok(Json(SeriesDto {
        from_ms,
        to_ms,
        points,
    }))
}
