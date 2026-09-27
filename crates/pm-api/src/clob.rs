//! CLOB read endpoints: POST /books (≤ 500 tokens per request), GET /ok (spec §3), and GET /time
//! (clock-offset sampling, scanner-clock-offset brief).
use crate::ApiError;
use crate::routes::{CLOB_BASE, CLOB_BOOKS, CLOB_OK, CLOB_TIME};
use crate::wire::string_or_number;
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MAX_TOKENS_PER_REQUEST: usize = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct LevelWire {
    #[serde(deserialize_with = "string_or_number")]
    pub price: String,
    #[serde(deserialize_with = "string_or_number")]
    pub size: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BookWire {
    pub asset_id: String,
    pub market: Option<String>,
    pub timestamp: Option<Value>,
    pub hash: Option<String>,
    #[serde(default)]
    pub bids: Vec<LevelWire>,
    #[serde(default)]
    pub asks: Vec<LevelWire>,
    pub min_order_size: Option<Value>,
    pub tick_size: Option<Value>,
    pub neg_risk: Option<bool>,
}

pub fn levels(ws: &[LevelWire]) -> Result<Vec<(Price, Shares)>, ApiError> {
    ws.iter()
        .map(|l| {
            let p = Price::parse(&l.price).map_err(|e| ApiError::field("price", e.to_string()))?;
            let s = Shares::parse(&l.size).map_err(|e| ApiError::field("size", e.to_string()))?;
            Ok((p, s))
        })
        .collect()
}

/// Millisecond timestamp sent as a string or a number.
pub fn ts_ms(v: &Option<Value>) -> Result<i64, ApiError> {
    let text = match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(ApiError::field("timestamp", "missing")),
    };
    text.trim()
        .parse::<i64>()
        .map_err(|e| ApiError::field("timestamp", e.to_string()))
}

impl BookWire {
    pub fn server_ts_ms(&self) -> Result<i64, ApiError> {
        ts_ms(&self.timestamp)
    }

    pub fn to_book(&self) -> Result<Book, ApiError> {
        let mut b = Book::default();
        b.apply_snapshot(
            &levels(&self.bids)?,
            &levels(&self.asks)?,
            self.server_ts_ms()?,
        );
        Ok(b)
    }
}

pub fn chunk_tokens(tokens: &[String]) -> Vec<Vec<String>> {
    tokens
        .chunks(MAX_TOKENS_PER_REQUEST)
        .map(<[String]>::to_vec)
        .collect()
}

pub struct ClobClient {
    http: reqwest::Client,
    base: String,
    rtt: Option<crate::RttSink>,
}

impl ClobClient {
    pub fn new() -> Result<Self, ApiError> {
        Ok(Self {
            http: crate::http::http_client()?,
            base: CLOB_BASE.to_string(),
            rtt: None,
        })
    }

    /// Report every request's round trip to `sink` (the scanner's health row).
    pub fn with_rtt(mut self, sink: crate::RttSink) -> Self {
        self.rtt = Some(sink);
        self
    }

    pub async fn books(&self, tokens: &[String]) -> Result<Vec<BookWire>, ApiError> {
        let mut out = Vec::new();
        for chunk in chunk_tokens(tokens) {
            let body: Vec<Value> = chunk.iter().map(|t| json!({ "token_id": t })).collect();
            let req = self
                .http
                .post(format!("{}{}", self.base, CLOB_BOOKS))
                .json(&body);
            let text = crate::http::send_timed(req, self.rtt.as_ref())
                .await?
                .error_for_status()?
                .text()
                .await?;
            let mut books: Vec<BookWire> = serde_json::from_str(&text)?;
            out.append(&mut books);
        }
        Ok(out)
    }

    pub async fn ok(&self) -> Result<bool, ApiError> {
        let req = self.http.get(format!("{}{}", self.base, CLOB_OK));
        let resp = crate::http::send_timed(req, self.rtt.as_ref()).await?;
        Ok(resp.status().is_success())
    }

    /// One clock-offset sample: `sent_ms` just before the request, `recv_ms` just after the body
    /// is read, `server_s` the unix-second body CLOB replied with.
    pub async fn time(&self) -> Result<TimeSample, ApiError> {
        let req = self.http.get(format!("{}{}", self.base, CLOB_TIME));
        let sent_ms = pm_api_now_ms();
        let text = crate::http::send_timed(req, self.rtt.as_ref())
            .await?
            .error_for_status()?
            .text()
            .await?;
        let recv_ms = pm_api_now_ms();
        let server_s = parse_time_body(&text)?;
        Ok(TimeSample {
            sent_ms,
            recv_ms,
            server_s,
        })
    }
}

/// Wall-clock milliseconds on the same clock the scanner uses for lag (`pm_api::ws::now_us`).
fn pm_api_now_ms() -> i64 {
    crate::ws::now_us() / 1000
}

/// A local `(sent, recv)` bracket around one CLOB `/time` unix-second reading (spec: the estimator
/// in `bins/scanner/src/clock.rs` uses this to bound the clock offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSample {
    pub sent_ms: i64,
    pub recv_ms: i64,
    pub server_s: i64,
}

/// The `/time` body is a bare integer of unix seconds, possibly with surrounding whitespace.
/// Anything else — not an integer, or non-positive — is rejected: a bad reading must never enter
/// the offset estimator silently.
fn parse_time_body(text: &str) -> Result<i64, ApiError> {
    let s: i64 = text
        .trim()
        .parse()
        .map_err(|e: std::num::ParseIntError| ApiError::field("clob_time", e.to_string()))?;
    if s <= 0 {
        return Err(ApiError::field("clob_time", "non-positive"));
    }
    Ok(s)
}

#[cfg(test)]
mod time_tests {
    #![allow(clippy::unwrap_used)]
    use super::parse_time_body;

    #[test]
    fn a_good_body_parses() {
        assert_eq!(parse_time_body("1790523400").unwrap(), 1_790_523_400);
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(parse_time_body("  1790523400\n").unwrap(), 1_790_523_400);
    }

    #[test]
    fn a_non_integer_body_is_rejected() {
        assert!(parse_time_body("not-a-number").is_err());
        assert!(parse_time_body("").is_err());
        assert!(parse_time_body("1790523400.5").is_err());
    }

    #[test]
    fn negative_or_zero_is_rejected() {
        assert!(parse_time_body("0").is_err());
        assert!(parse_time_body("-1").is_err());
    }
}
