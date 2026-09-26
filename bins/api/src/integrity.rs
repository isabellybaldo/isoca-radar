//! GET /api/integrity/summary?hours= (spec §4): counts by kind and events per million messages.
use crate::dto::{Dec, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct KindCountDto {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct IntegrityDto {
    pub from_ms: i64,
    pub to_ms: i64,
    pub by_kind: Vec<KindCountDto>,
    pub total: i64,
    pub msgs: i64,
    pub per_million: Option<Dec>,
}

pub async fn summary(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<IntegrityDto>, ApiError> {
    let q = params::query(q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * 3_600_000;
    let s = st.store.integrity_summary(from_ms, to_ms).await?;
    Ok(Json(IntegrityDto {
        from_ms,
        to_ms,
        by_kind: s
            .by_kind
            .into_iter()
            .map(|(kind, count)| KindCountDto { kind, count })
            .collect(),
        total: s.total,
        msgs: s.msgs,
        per_million: opt(s.per_million),
    }))
}
