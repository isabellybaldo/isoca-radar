//! Market websocket (public, no auth): frame parsing and one connection (spec §3, §7).
//! Reconnect policy lives in the scanner (S2c) using backoff.rs; this is one session.
use crate::ApiError;
use crate::routes::WS_MARKET;
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketEvent {
    Snapshot {
        token: String,
        server_ts_ms: i64,
        bids: Vec<(Price, Shares)>,
        asks: Vec<(Price, Shares)>,
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
        market: String,
        server_ts_ms: i64,
    },
    NewMarket {
        server_ts_ms: i64,
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
            market: str_field(v, "market").unwrap_or_default(),
            server_ts_ms: ts(v).unwrap_or(0),
        }),
        "new_market" => out.push(MarketEvent::NewMarket {
            server_ts_ms: ts(v).unwrap_or(0),
        }),
        other => out.push(MarketEvent::Other {
            event_type: other.to_string(),
        }),
    }
    Ok(())
}

/// Parse one text frame. The initial dump is a JSON array; later frames are single objects.
/// `PONG` (heartbeat reply) and blank frames yield no events.
pub fn parse_frame(text: &str) -> Result<Vec<MarketEvent>, ApiError> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("PONG") {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(t)?;
    let items = match v {
        Value::Array(a) => a,
        other => vec![other],
    };
    let mut out = Vec::new();
    for item in &items {
        parse_event(item, &mut out)?;
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

/// One websocket session: connect, subscribe, send `PING` every 10 s, forward every text frame.
/// Returns Ok(()) when the server closes or the receiver is dropped; Err on transport errors.
pub async fn run_connection(
    conn: usize,
    tokens: Vec<String>,
    tx: mpsc::Sender<Frame>,
) -> Result<(), ApiError> {
    let (ws, _) = tokio_tungstenite::connect_async(WS_MARKET).await?;
    let (mut write, mut read) = ws.split();
    write
        .send(Message::text(subscribe_message(&tokens)))
        .await?;
    let mut ping = tokio::time::interval(Duration::from_secs(10));
    ping.tick().await;
    loop {
        tokio::select! {
            _ = ping.tick() => {
                write.send(Message::text("PING")).await?;
            }
            msg = read.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    let frame = Frame { conn, recv_us: now_us(), text: t.as_str().to_owned() };
                    if tx.send(frame).await.is_err() {
                        return Ok(());
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
            }
        }
    }
}
