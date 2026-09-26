//! api: the dashboard's read-only JSON + SSE server (Phase 4 spec §4). Reads Postgres only through
//! `store`, as analyze_ro, on read-only sessions; exposes no write endpoint; paper only (spec §16).
#![forbid(unsafe_code)]

pub mod config;
pub mod dto;
pub mod error;
pub mod guard;
pub mod health;
pub mod params;

use axum::Router;
use axum::routing::get;
use std::sync::Arc;
use store::Store;
use tokio::sync::{Semaphore, broadcast};

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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health/now", get(health::now))
        .route("/api/health/series", get(health::series))
        .fallback(error::not_found)
        .with_state(state)
}
