//! Market websocket frame parsing (parent spec §3, §7), moved from pm-api (spec 5b §2) so the
//! compactor parses frames exactly as the scanner does without depending on the network crate.
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bad field {field}: {detail}")]
    Field { field: &'static str, detail: String },
}

impl FrameError {
    pub fn field(field: &'static str, detail: impl Into<String>) -> Self {
        FrameError::Field {
            field,
            detail: detail.into(),
        }
    }
}

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

fn text_of(v: &Value, field: &'static str) -> Result<String, FrameError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(FrameError::field(field, format!("unexpected {other}"))),
    }
}

fn str_field(v: &Value, field: &'static str) -> Result<String, FrameError> {
    v.get(field)
        .ok_or_else(|| FrameError::field(field, "missing"))
        .and_then(|x| text_of(x, field))
}

fn ts(v: &Value) -> Result<i64, FrameError> {
    str_field(v, "timestamp")?
        .trim()
        .parse::<i64>()
        .map_err(|e| FrameError::field("timestamp", e.to_string()))
}

/// Missing or invalid → `None`, never a fabricated 0 (spec M1).
fn opt_ts(v: &Value) -> Option<i64> {
    ts(v).ok()
}

/// Missing or the wrong JSON type → `None`.
fn opt_str_field(v: &Value, field: &'static str) -> Option<String> {
    v.get(field).and_then(|x| text_of(x, field).ok())
}

fn price(v: &Value, field: &'static str) -> Result<Price, FrameError> {
    Price::parse(&str_field(v, field)?).map_err(|e| FrameError::field(field, e.to_string()))
}

fn opt_price(v: Option<&Value>, field: &'static str) -> Result<Option<Price>, FrameError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(x) => Price::parse(&text_of(x, field)?)
            .map(Some)
            .map_err(|e| FrameError::field(field, e.to_string())),
    }
}

fn levels_of(v: Option<&Value>) -> Result<Vec<(Price, Shares)>, FrameError> {
    let Some(Value::Array(items)) = v else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|l| {
            let p = price(l, "price")?;
            let s = Shares::parse(&str_field(l, "size")?)
                .map_err(|e| FrameError::field("size", e.to_string()))?;
            Ok((p, s))
        })
        .collect()
}

fn parse_event(v: &Value, out: &mut Vec<MarketEvent>) -> Result<(), FrameError> {
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
                .ok_or_else(|| FrameError::field("price_changes", "missing"))?;
            for c in changes {
                let side = match c.get("side").and_then(Value::as_str) {
                    Some("BUY") => Side::Bid,
                    Some("SELL") => Side::Ask,
                    other => return Err(FrameError::field("side", format!("{other:?}"))),
                };
                out.push(MarketEvent::Delta {
                    token: str_field(c, "asset_id")?,
                    server_ts_ms: t,
                    side,
                    price: price(c, "price")?,
                    size: Shares::parse(&str_field(c, "size")?)
                        .map_err(|e| FrameError::field("size", e.to_string()))?,
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
fn with_asset_id(e: FrameError, id: &str) -> FrameError {
    match e {
        FrameError::Field { field, detail } => FrameError::Field {
            field,
            detail: format!("asset_id={id}: {detail}"),
        },
        other => FrameError::field("asset_id", format!("asset_id={id}: {other}")),
    }
}

/// Parse one text frame. The initial dump is a JSON array; later frames are single objects.
/// `PONG` (heartbeat reply) and blank frames yield no events.
pub fn parse_frame(text: &str) -> Result<Vec<MarketEvent>, FrameError> {
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
