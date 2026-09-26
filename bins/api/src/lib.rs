//! api: the dashboard's read-only JSON + SSE server (Phase 4 spec §4). Reads Postgres only through
//! `store`, as analyze_ro, on read-only sessions; exposes no write endpoint; paper only (spec §16).
#![forbid(unsafe_code)]

pub mod config;
pub mod dto;
pub mod episodes;
pub mod error;
pub mod guard;
pub mod health;
pub mod integrity;
pub mod paper;
pub mod params;

use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::any::Any;
use std::sync::Arc;
use store::Store;
use tokio::sync::{Semaphore, broadcast};
use tower_http::catch_panic::CatchPanicLayer;

/// Most concurrent SSE clients (one owner, a few tabs); more get a 503.
pub const MAX_STREAMS: usize = 16;

/// What the shared LISTEN bridge fans out to stream clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// The scanner's `NOTIFY episode` payload (an episode id).
    Episode(String),
    /// A new health minute appeared (its minute_ms).
    Health(i64),
}

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub events: broadcast::Sender<StreamEvent>,
    pub stream_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            store,
            events,
            stream_slots: Arc::new(Semaphore::new(MAX_STREAMS)),
        }
    }
}

/// Wall-clock milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Spec §3 "The API can't write": refuse to serve with a role that can INSERT.
pub async fn ensure_read_only(store: &Store) -> anyhow::Result<()> {
    anyhow::ensure!(
        !store.role_can_write().await?,
        "refusing to start: the database role can write; connect as analyze_ro"
    );
    Ok(())
}

/// A panicking handler must not drop the connection: turn it into the same JSON 500 every other
/// internal error produces, with no panic payload leaked to the client.
fn handle_panic(_err: Box<dyn Any + Send + 'static>) -> Response {
    error::ApiError::Internal.into_response()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health/now", get(health::now))
        .route("/api/health/series", get(health::series))
        .route("/api/episodes/open", get(episodes::open))
        .route("/api/episodes", get(episodes::list))
        .route("/api/episodes/{id}", get(episodes::detail))
        .route("/api/today", get(episodes::today))
        .route("/api/integrity/summary", get(integrity::summary))
        .route("/api/paper/accounts", get(paper::accounts))
        .route("/api/paper/equity", get(paper::equity))
        .route("/api/paper/positions", get(paper::positions))
        .route("/api/paper/ledger", get(paper::ledger))
        .method_not_allowed_fallback(error::method_not_allowed)
        .fallback(error::not_found)
        .with_state(state)
        .layer(CatchPanicLayer::custom(handle_panic))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn boom() -> &'static str {
        panic!("kaboom, do not leak me")
    }

    #[tokio::test]
    async fn a_panicking_handler_returns_the_internal_json_error_not_a_dropped_connection() {
        let app = Router::new()
            .route("/boom", get(boom))
            .layer(CatchPanicLayer::custom(handle_panic));
        let resp = app
            .oneshot(Request::get("/boom").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "internal");
        assert_eq!(
            v["detail"],
            "Something broke on our side. It has been written down."
        );
        assert!(!v.to_string().contains("kaboom"));
    }
}
