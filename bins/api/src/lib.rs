//! api: the dashboard's JSON + SSE server (Phase 4 spec §4). Reads Postgres only through `store`,
//! as analyze_ro, on read-only sessions. Its one write (Phase 7a §5) is a row in
//! paper_reset_requests, through a second one-connection pool as paper_reset_rw; paper only
//! (spec §16).
#![forbid(unsafe_code)]

pub mod cards;
pub mod config;
pub mod dto;
pub mod episodes;
pub mod error;
pub mod guard;
pub mod health;
pub mod integrity;
pub mod ladder;
pub mod paper;
pub mod params;
pub mod public;
pub mod reset;
pub mod stream;

use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use std::any::Any;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use store::Store;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast, watch};
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
    /// Phase 7a: the reset pool (paper_reset_rw, INSERT on paper_reset_requests only). None:
    /// POST /api/paper/reset answers 503 reset_disabled.
    pub reset: Option<Store>,
    /// Phase 7a: the policies file's scenario latencies (POLICIES_PATH). None: unknown (every
    /// rung's latency_ms is null).
    pub scenarios: Option<Arc<Vec<::cards::body::ScenarioLat>>>,
    pub events: broadcast::Sender<StreamEvent>,
    pub stream_slots: Arc<Semaphore>,
    /// Flips to `true` on graceful shutdown; every open SSE stream watches this and ends itself
    /// so `axum::serve`'s graceful shutdown (which waits for in-flight connections) can return.
    pub shutdown_tx: watch::Sender<bool>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let (events, _) = broadcast::channel(256);
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            store,
            reset: None,
            scenarios: None,
            events,
            stream_slots: Arc::new(Semaphore::new(MAX_STREAMS)),
            shutdown_tx,
        }
    }

    pub fn with_reset(mut self, reset: Option<Store>) -> Self {
        self.reset = reset;
        self
    }

    pub fn with_scenarios(mut self, s: Option<Vec<::cards::body::ScenarioLat>>) -> Self {
        self.scenarios = s.map(Arc::new);
        self
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

/// Spec 7a §5.1: the reset pool's role may INSERT into paper_reset_requests and nothing else
/// (mirrors `ensure_read_only`).
pub async fn ensure_reset_role(reset: &Store) -> anyhow::Result<()> {
    let extra = reset.role_writes_outside_reset().await?;
    anyhow::ensure!(
        extra.is_empty(),
        "refusing to start: the reset role can write {}; connect as paper_reset_rw",
        extra.join(", ")
    );
    Ok(())
}

/// Spec 7a §5.1: the reset pool from its connect attempt. A pool that can't connect (a NOLOGIN
/// role, a bad password, a restored cluster) or can't read its grants must not take the
/// read-only dashboard down: one log line and None (the POST answers 503 reset_disabled). Only an
/// over-granted role refuses to start.
pub async fn reset_pool(connected: anyhow::Result<Store>) -> anyhow::Result<Option<Store>> {
    const DISABLED: &str = "POST /api/paper/reset answers 503 reset_disabled";
    let reset = match connected {
        Ok(r) => r,
        Err(e) => {
            eprintln!("api: reset pool: {e:#}; {DISABLED}");
            return Ok(None);
        }
    };
    match reset.role_writes_outside_reset().await {
        Ok(extra) if extra.is_empty() => Ok(Some(reset)),
        Ok(extra) => {
            reset.close().await;
            anyhow::bail!(
                "refusing to start: the reset role can write {}; connect as paper_reset_rw",
                extra.join(", ")
            )
        }
        Err(e) => {
            eprintln!("api: reset pool: read its grants: {e}; {DISABLED}");
            reset.close().await;
            Ok(None)
        }
    }
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
        .route("/api/paper/reset", post(reset::reset))
        .route("/api/paper/epochs", get(reset::epochs))
        .route("/api/paper/ladder", get(ladder::ladder))
        .route("/api/cards", get(cards::list))
        .route("/api/cards/meta", get(cards::meta))
        .route("/api/stream", get(stream::sse))
        .method_not_allowed_fallback(error::method_not_allowed)
        .fallback(error::not_found)
        .with_state(state)
        .layer(CatchPanicLayer::custom(handle_panic))
}

/// Bind `bind` and serve `state` (see `run_on_state`); also logs the startup line ops tooling
/// greps for.
pub async fn run(
    state: AppState,
    bind: &str,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    eprintln!("api: listening on {bind} (read-only except paper reset requests, paper only)");
    run_on_state(listener, state, health_every, shutdown).await
}

/// `run_on_state` with no reset pool.
pub async fn run_on(
    listener: TcpListener,
    store: Store,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    run_on_state(listener, AppState::new(store), health_every, shutdown).await
}

/// Serve `router(state)` on `listener` until `shutdown` resolves, then shut down cleanly: every
/// open SSE stream watches `AppState::shutdown_tx` and ends itself so `axum::serve`'s graceful
/// shutdown (which otherwise waits forever for an infinite stream) can return; only then are the
/// bridge's two tasks aborted and joined (dropping a `JoinHandle` does NOT stop its task — it
/// must be `abort()`ed) and the pools closed. The bridge is stopped, and its pooled LISTEN
/// connection freed, strictly before `store.close()`, which waits for every checked-out
/// connection and would otherwise hang forever on that one.
pub async fn run_on_state(
    listener: TcpListener,
    state: AppState,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let store = state.store.clone();
    let reset = state.reset.clone();
    let bridge = stream::spawn_bridge(store.clone(), state.events.clone(), health_every);
    let shutdown_tx = state.shutdown_tx.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            shutdown.await;
            // `send` is a no-op (and returns `Err`) when there are no receivers yet — e.g. a
            // stream that is mid-handler, between `AppState::new`'s `subscribe()` call site in
            // `stream::sse` and actually polling, would then read the old `false` and never end.
            // `send_replace` always sets the value, so any receiver that subscribes even after
            // this line still observes `true`.
            shutdown_tx.send_replace(true);
        })
        .await?;
    for h in bridge {
        h.abort();
        let _ = h.await;
    }
    if let Some(r) = reset {
        r.close().await;
    }
    store.close().await;
    Ok(())
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

    #[test]
    fn shutdown_flag_reaches_a_subscriber_that_arrives_after_shutdown_with_no_streams_open() {
        // Mirrors `AppState::new` (which discards its initial receiver) and `run_on`'s shutdown
        // closure: with zero receivers, a plain `send` would return `Err` and leave the value at
        // `false`. `send_replace` must set it regardless, so a stream that subscribes even after
        // shutdown has already fired still sees `true`, not the stale default.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        drop(shutdown_rx);
        shutdown_tx.send_replace(true);
        let late = shutdown_tx.subscribe();
        assert!(
            *late.borrow(),
            "a late subscriber must see the post-shutdown value"
        );
    }
}
