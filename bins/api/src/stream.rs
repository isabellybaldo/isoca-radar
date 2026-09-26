//! GET /api/stream (spec §4): Server-Sent Events for the live Radar.
//! ONE Postgres LISTEN serves every client (spec §3): a bridge task fans the scanner's
//! `NOTIFY episode` out through a broadcast channel. `health` fires when a new health minute
//! appears (one cheap indexed poll per interval; no trigger, no migration). A `: hb` comment
//! every 15 s keeps proxies from closing idle streams.
//! The LISTEN loop and the health poll are separate tasks: `Listener::recv_payload` is never
//! raced in a `select!`, so a cancelled read can't lose part of a notification.
use crate::error::ApiError;
use crate::{AppState, StreamEvent};
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use std::convert::Infallible;
use std::time::Duration;
use store::Store;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub const HEARTBEAT: Duration = Duration::from_secs(15);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Start the shared LISTEN (with reconnect backoff) and the health-minute poll.
pub fn spawn_bridge(
    store: Store,
    tx: broadcast::Sender<StreamEvent>,
    health_every: Duration,
) -> [JoinHandle<()>; 2] {
    let episodes = {
        let store = store.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut backoff = BACKOFF_START;
            loop {
                match store.listen("episode").await {
                    Ok(mut listener) => {
                        backoff = BACKOFF_START;
                        loop {
                            match listener.recv_payload().await {
                                Ok(id) => {
                                    let id: String =
                                        id.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                                    let _ = tx.send(StreamEvent::Episode(id));
                                }
                                Err(e) => {
                                    eprintln!("api: stream LISTEN lost: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("api: stream LISTEN failed: {e}"),
                }
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(BACKOFF_MAX);
            }
        })
    };
    let health = tokio::spawn(async move {
        let mut last: Option<i64> = None;
        let mut tick = tokio::time::interval(health_every);
        loop {
            tick.tick().await;
            match store.latest_health_minute().await {
                Ok(Some(m)) if last != Some(m) => {
                    last = Some(m);
                    let _ = tx.send(StreamEvent::Health(m));
                }
                Ok(_) => {}
                Err(e) => eprintln!("api: health poll failed: {e}"),
            }
        }
    });
    [episodes, health]
}

pub fn to_event(ev: &StreamEvent) -> Event {
    match ev {
        StreamEvent::Episode(id) => Event::default().event("episode").data(id),
        StreamEvent::Health(minute_ms) => {
            Event::default().event("health").data(minute_ms.to_string())
        }
    }
}

pub async fn sse(
    State(st): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = st.stream_slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::Unavailable(
            "too_many_streams",
            "Too many open streams. Close a tab or two.",
        )
    })?;
    let rx = st.events.subscribe();
    // The permit lives inside the stream state, so it is released when the client disconnects.
    let events = stream::unfold((rx, permit), |(mut rx, permit)| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => return Some((Ok(to_event(&ev)), (rx, permit))),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT).text("hb")))
}
