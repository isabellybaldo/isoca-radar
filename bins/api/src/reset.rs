//! POST /api/paper/reset and GET /api/paper/epochs (Phase 7a spec §5). The api's only write: one
//! row in paper_reset_requests, through its own one-connection pool as paper_reset_rw; the
//! scanner applies it within ~30 s. Traefik's forward-auth (owner only) guards every /api route;
//! the custom header forces a CORS preflight no cross-site page can pass, and Sec-Fetch-Site,
//! when a browser sends it, must be same-origin.
use crate::error::ApiError;
use crate::{AppState, now_ms};
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use store::ResetInsert;

pub const INTENT_HEADER: &str = "x-ct-intent";
pub const INTENT: &str = "reset-paper";
const DAY_MS: i64 = 86_400_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    confirm: String,
}

#[derive(Debug, Serialize)]
pub struct QueuedDto {
    pub request_id: i64,
    pub requested_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct EpochDto {
    pub epoch: i32,
    pub start_ms: i64,
    pub cause: String,
}

#[derive(Debug, Serialize)]
pub struct RequestDto {
    pub id: i64,
    pub requested_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct EpochsDto {
    pub epochs: Vec<EpochDto>,
    /// 0 before the first epoch row (epoch 0 is implicit).
    pub current: i32,
    pub pending_request: Option<RequestDto>,
    pub next_allowed_ms: i64,
}

/// The next UTC midnight after `ms`.
pub fn next_midnight(ms: i64) -> i64 {
    (ms.div_euclid(DAY_MS) + 1) * DAY_MS
}

/// After today's request: the next UTC midnight; otherwise now.
pub fn next_allowed(latest_requested_ms: Option<i64>, now: i64) -> i64 {
    match latest_requested_ms {
        Some(r) if r.div_euclid(DAY_MS) == now.div_euclid(DAY_MS) => next_midnight(r),
        _ => now,
    }
}

/// `YYYY-MM-DD` of a UTC instant (civil-from-days, H. Hinnant; integers only).
pub fn utc_date(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn check_headers(h: &HeaderMap) -> Result<(), ApiError> {
    if h.get(INTENT_HEADER).and_then(|v| v.to_str().ok()) != Some(INTENT) {
        return Err(ApiError::Forbidden(
            "forbidden",
            "A reset needs the X-CT-Intent: reset-paper header.".to_string(),
        ));
    }
    if let Some(site) = h.get("sec-fetch-site")
        && site.to_str().ok() != Some("same-origin")
    {
        return Err(ApiError::Forbidden(
            "forbidden",
            "Only the dashboard itself can ask for a reset.".to_string(),
        ));
    }
    Ok(())
}

/// Checks in order: headers (403), body (400), the pool (503), a pending request (409), then the
/// insert (202, or 429 on the unique utc_day: one per UTC day, races included).
pub async fn reset(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<QueuedDto>), ApiError> {
    check_headers(&headers)?;
    match serde_json::from_slice::<Confirm>(&body) {
        Ok(c) if c.confirm == "reset" => {}
        _ => {
            return Err(ApiError::BadRequest(
                "bad_request",
                "The body must be {\"confirm\": \"reset\"}.".to_string(),
            ));
        }
    }
    let Some(reset) = st.reset.as_ref() else {
        return Err(ApiError::Unavailable(
            "reset_disabled",
            "Paper reset is not configured on this server.",
        ));
    };
    if let Some(p) = reset.pending_reset_request().await? {
        return Err(ApiError::Conflict(
            "reset_pending",
            format!(
                "Request {} is queued; the scanner applies it within about 30 s.",
                p.id
            ),
        ));
    }
    let now = now_ms();
    match reset.insert_reset_request(now).await? {
        ResetInsert::Queued(r) => Ok((
            StatusCode::ACCEPTED,
            Json(QueuedDto {
                request_id: r.id,
                requested_ms: r.requested_ms,
            }),
        )),
        ResetInsert::DailyLimit => Err(ApiError::TooMany(
            "reset_daily_limit",
            format!(
                "One reset per UTC day. The next one is allowed from {} 00:00 UTC.",
                utc_date(next_midnight(now))
            ),
        )),
    }
}

pub async fn epochs(State(st): State<AppState>) -> Result<Json<EpochsDto>, ApiError> {
    let list = st.store.paper_epochs().await?;
    let pending = st.store.pending_reset_request().await?;
    let latest = st.store.latest_reset_request().await?;
    Ok(Json(EpochsDto {
        current: list.last().map_or(0, |e| e.epoch),
        epochs: list
            .into_iter()
            .map(|e| EpochDto {
                epoch: e.epoch,
                start_ms: e.start_ms,
                cause: e.cause,
            })
            .collect(),
        pending_request: pending.map(|r| RequestDto {
            id: r.id,
            requested_ms: r.requested_ms,
        }),
        next_allowed_ms: next_allowed(latest.map(|r| r.requested_ms), now_ms()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_midnights_are_utc() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_790_121_600_000), "2026-09-23");
        assert_eq!(utc_date(951_782_400_000), "2000-02-29");
        assert_eq!(next_midnight(1_790_121_600_000), 1_790_208_000_000);
        assert_eq!(next_midnight(1_790_208_000_000 - 1), 1_790_208_000_000);
        let now = 1_790_121_600_000 + 5_000;
        assert_eq!(next_allowed(None, now), now);
        assert_eq!(next_allowed(Some(now - DAY_MS), now), now, "yesterday's");
        assert_eq!(next_allowed(Some(now - 1_000), now), 1_790_208_000_000);
    }
}
