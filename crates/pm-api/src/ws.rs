//! Market websocket (public, no auth): frame parsing and one connection (spec §3, §7).
//! Reconnect policy lives in the scanner (S2c) using backoff.rs; this is one session.
use crate::ApiError;
use crate::routes::WS_MARKET;
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::{self, Message};

/// How often we send a `PING` heartbeat on an otherwise-idle connection.
pub const PING_EVERY: Duration = Duration::from_secs(10);
/// No frame of any kind (text, ping, pong, binary) for this long means the connection is
/// stale even though TCP hasn't noticed yet; the caller should reconnect (spec F3).
pub const STALE_AFTER: Duration = Duration::from_secs(30);
/// Upper bound on connect + subscribe before giving up on this attempt.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketEvent {
    Snapshot {
        token: String,
        server_ts_ms: i64,
        bids: Vec<(Price, Shares)>,
        asks: Vec<(Price, Shares)>,
        hash: Option<String>,
    },
    Delta {
        token: String,
        server_ts_ms: i64,
        side: Side,
        price: Price,
        size: Shares,
        best_bid: Option<Price>,
        best_ask: Option<Price>,
    },
    LastTrade {
        token: String,
        server_ts_ms: i64,
    },
    TickSize {
        token: String,
        server_ts_ms: i64,
    },
    Resolved {
        market: Option<String>,
        server_ts_ms: Option<i64>,
    },
    NewMarket {
        server_ts_ms: Option<i64>,
    },
    Other {
        event_type: String,
    },
}

fn text_of(v: &Value, field: &'static str) -> Result<String, ApiError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(ApiError::field(field, format!("unexpected {other}"))),
    }
}

fn str_field(v: &Value, field: &'static str) -> Result<String, ApiError> {
    v.get(field)
        .ok_or_else(|| ApiError::field(field, "missing"))
        .and_then(|x| text_of(x, field))
}

fn ts(v: &Value) -> Result<i64, ApiError> {
    str_field(v, "timestamp")?
        .trim()
        .parse::<i64>()
        .map_err(|e| ApiError::field("timestamp", e.to_string()))
}

/// Missing or invalid → `None`, never a fabricated 0 (spec M1).
fn opt_ts(v: &Value) -> Option<i64> {
    ts(v).ok()
}

/// Missing or the wrong JSON type → `None`.
fn opt_str_field(v: &Value, field: &'static str) -> Option<String> {
    v.get(field).and_then(|x| text_of(x, field).ok())
}

fn price(v: &Value, field: &'static str) -> Result<Price, ApiError> {
    Price::parse(&str_field(v, field)?).map_err(|e| ApiError::field(field, e.to_string()))
}

fn opt_price(v: Option<&Value>, field: &'static str) -> Result<Option<Price>, ApiError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(x) => Price::parse(&text_of(x, field)?)
            .map(Some)
            .map_err(|e| ApiError::field(field, e.to_string())),
    }
}

fn levels_of(v: Option<&Value>) -> Result<Vec<(Price, Shares)>, ApiError> {
    let Some(Value::Array(items)) = v else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|l| {
            let p = price(l, "price")?;
            let s = Shares::parse(&str_field(l, "size")?)
                .map_err(|e| ApiError::field("size", e.to_string()))?;
            Ok((p, s))
        })
        .collect()
}

fn parse_event(v: &Value, out: &mut Vec<MarketEvent>) -> Result<(), ApiError> {
    let et = v.get("event_type").and_then(Value::as_str).unwrap_or("");
    match et {
        "book" => out.push(MarketEvent::Snapshot {
            token: str_field(v, "asset_id")?,
            server_ts_ms: ts(v)?,
            bids: levels_of(v.get("bids"))?,
            asks: levels_of(v.get("asks"))?,
            hash: opt_str_field(v, "hash"),
        }),
        "price_change" => {
            let t = ts(v)?;
            let changes = v
                .get("price_changes")
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::field("price_changes", "missing"))?;
            for c in changes {
                let side = match c.get("side").and_then(Value::as_str) {
                    Some("BUY") => Side::Bid,
                    Some("SELL") => Side::Ask,
                    other => return Err(ApiError::field("side", format!("{other:?}"))),
                };
                out.push(MarketEvent::Delta {
                    token: str_field(c, "asset_id")?,
                    server_ts_ms: t,
                    side,
                    price: price(c, "price")?,
                    size: Shares::parse(&str_field(c, "size")?)
                        .map_err(|e| ApiError::field("size", e.to_string()))?,
                    best_bid: opt_price(c.get("best_bid"), "best_bid")?,
                    best_ask: opt_price(c.get("best_ask"), "best_ask")?,
                });
            }
        }
        "last_trade_price" => out.push(MarketEvent::LastTrade {
            token: str_field(v, "asset_id")?,
            server_ts_ms: ts(v)?,
        }),
        "tick_size_change" => out.push(MarketEvent::TickSize {
            token: str_field(v, "asset_id")?,
            server_ts_ms: ts(v)?,
        }),
        "market_resolved" => out.push(MarketEvent::Resolved {
            market: opt_str_field(v, "market"),
            server_ts_ms: opt_ts(v),
        }),
        "new_market" => out.push(MarketEvent::NewMarket {
            server_ts_ms: opt_ts(v),
        }),
        other => out.push(MarketEvent::Other {
            event_type: other.to_string(),
        }),
    }
    Ok(())
}

/// The `asset_id` of a frame item, for error messages only — `"?"` when missing or not a
/// string/number (spec F5).
fn item_asset_id(item: &Value) -> String {
    match item.get("asset_id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "?".to_string(),
    }
}

/// Prefix an item-parse error with the asset id it came from, so a bad item in a big initial
/// dump doesn't read as an anonymous parse failure (spec F5). Full per-item recovery (skip the
/// bad item, keep the rest) is deferred to S2c.
fn with_asset_id(e: ApiError, id: &str) -> ApiError {
    match e {
        ApiError::Field { field, detail } => ApiError::Field {
            field,
            detail: format!("asset_id={id}: {detail}"),
        },
        other => ApiError::field("asset_id", format!("asset_id={id}: {other}")),
    }
}

/// Parse one text frame. The initial dump is a JSON array; later frames are single objects.
/// `PONG` (heartbeat reply) and blank frames yield no events.
pub fn parse_frame(text: &str) -> Result<Vec<MarketEvent>, ApiError> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("PONG") {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(t)?;
    let is_array = matches!(v, Value::Array(_));
    let items = match v {
        Value::Array(a) => a,
        other => vec![other],
    };
    let mut out = Vec::new();
    for item in &items {
        parse_event(item, &mut out).map_err(|e| {
            if is_array {
                with_asset_id(e, &item_asset_id(item))
            } else {
                e
            }
        })?;
    }
    Ok(out)
}

pub fn subscribe_message(tokens: &[String]) -> String {
    json!({ "assets_ids": tokens, "type": "market", "initial_dump": true }).to_string()
}

/// Wall-clock microseconds (our receive clock; compared against server ms for lag).
pub fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_micros()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub conn: usize,
    pub recv_us: i64,
    pub text: String,
}

type WsStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect and send the subscribe message. Split out so the whole thing can be bounded by
/// `CONNECT_TIMEOUT` in `run_connection` (spec F3).
async fn connect_and_subscribe(
    tokens: &[String],
) -> Result<
    (
        futures_util::stream::SplitSink<WsStream, Message>,
        futures_util::stream::SplitStream<WsStream>,
    ),
    ApiError,
> {
    let (ws, _) = tokio_tungstenite::connect_async(WS_MARKET).await?;
    let (mut write, read) = ws.split();
    write.send(Message::text(subscribe_message(tokens))).await?;
    Ok((write, read))
}

/// One websocket session: connect, subscribe, send `PING` every `PING_EVERY`, forward every
/// text frame, and give up on the connection if nothing at all arrives for `STALE_AFTER`.
/// Returns Ok(()) when the server closes or the receiver is dropped; Err on transport errors,
/// a connect timeout, staleness, or a full receiver.
pub async fn run_connection(
    conn: usize,
    tokens: Vec<String>,
    tx: mpsc::Sender<Frame>,
) -> Result<(), ApiError> {
    crate::init_tls();
    let (write, read) = tokio::time::timeout(CONNECT_TIMEOUT, connect_and_subscribe(&tokens))
        .await
        .map_err(|_| ApiError::Timeout("connect"))??;
    session_loop(conn, write, read, &tx).await
}

/// The read/ping/stale-detection loop, generic over the sink/stream so it can be driven by an
/// in-memory transport in tests without a real network connection (spec F3/M4).
pub(crate) async fn session_loop<W, R>(
    conn: usize,
    mut write: W,
    mut read: R,
    tx: &mpsc::Sender<Frame>,
) -> Result<(), ApiError>
where
    W: Sink<Message, Error = tungstenite::Error> + Unpin,
    R: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ping.tick().await;
    let mut last_rx = tokio::time::Instant::now();
    // Non-text data frames still count as life on the wire (spec F4); kept only for that,
    // no API surfaces it yet.
    #[allow(unused_assignments, unused_variables)]
    let mut binary_frames: u64 = 0;
    loop {
        let stale_at = last_rx + STALE_AFTER;
        tokio::select! {
            _ = ping.tick() => {
                write.send(Message::text("PING")).await?;
            }
            () = tokio::time::sleep_until(stale_at) => {
                let silent_ms = u64::try_from(STALE_AFTER.as_millis()).unwrap_or(u64::MAX);
                return Err(ApiError::Stale { conn, silent_ms });
            }
            msg = read.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    last_rx = tokio::time::Instant::now();
                    let frame = Frame { conn, recv_us: now_us(), text: t.as_str().to_owned() };
                    match tx.try_send(frame) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => return Err(ApiError::Dropped { conn }),
                        Err(TrySendError::Closed(_)) => return Ok(()),
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    last_rx = tokio::time::Instant::now();
                    #[allow(unused_assignments)]
                    {
                        binary_frames += 1;
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {
                    // Ping/Pong/Frame variants: still a live connection (spec F3).
                    last_rx = tokio::time::Instant::now();
                }
                Some(Err(e)) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use futures_util::stream;

    /// A `Sink<Message>` that swallows everything, with the right error type for `session_loop`.
    fn drain_sink() -> impl Sink<Message, Error = tungstenite::Error> + Unpin {
        futures_util::sink::drain::<Message>()
            .sink_map_err(|e: std::convert::Infallible| match e {})
    }

    #[tokio::test(start_paused = true)]
    async fn stale_after_30s_of_silence() {
        let (tx, _rx) = mpsc::channel(8);
        let read = stream::pending::<Result<Message, tungstenite::Error>>();
        let err = session_loop(1, drain_sink(), read, &tx).await.unwrap_err();
        match err {
            ApiError::Stale { conn, silent_ms } => {
                assert_eq!(conn, 1);
                assert!(silent_ms >= 30_000);
            }
            other => panic!("expected Stale, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn drop_on_full_receiver() {
        let (tx, _rx) = mpsc::channel(1); // capacity 1; receiver never drains
        let read = stream::iter(vec![
            Ok(Message::text("one")),
            Ok(Message::text("two")),
            Ok(Message::text("three")),
        ]);
        let err = session_loop(2, drain_sink(), read, &tx).await.unwrap_err();
        assert!(matches!(err, ApiError::Dropped { conn: 2 }));
    }

    #[tokio::test]
    async fn ok_when_receiver_is_dropped() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let read = stream::iter(vec![Ok(Message::text("one"))]);
        session_loop(3, drain_sink(), read, &tx).await.unwrap();
    }
}
