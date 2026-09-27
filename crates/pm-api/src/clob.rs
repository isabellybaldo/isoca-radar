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
    /// is read, `server_s` the unix-second body CLOB replied with. Deliberately never reports to
    /// the RTT sink (`None`, not `self.rtt`): `/time` is not REST-check work, and feeding it into
    /// `rest_rtt_*` would change the host-band's latency input for every normal run.
    pub async fn time(&self) -> Result<TimeSample, ApiError> {
        let req = self.http.get(format!("{}{}", self.base, CLOB_TIME));
        let sent_ms = pm_api_now_ms();
        let text = crate::http::send_timed(req, None)
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

/// Unix-second readings we accept: from 2001-09-09 to 5138-11-16. Wide enough for any real clock,
/// narrow enough that a millisecond body (13 digits) or a bad reply can never sneak through — and
/// small enough that `offset_window`'s `* 1000` can never overflow even before its own saturating
/// arithmetic (defense in depth, see `bins/scanner/src/clock.rs`).
const MIN_UNIX_S: i64 = 1_000_000_000;
const MAX_UNIX_S: i64 = 100_000_000_000;

/// The `/time` body is a bare integer of unix seconds, possibly with surrounding whitespace.
/// Anything else — a leading `+`/`-`, embedded whitespace, a non-digit, or a value outside
/// `MIN_UNIX_S..=MAX_UNIX_S` (catches a millisecond-precision body, for instance) — is rejected: a
/// bad reading must never enter the offset estimator silently.
fn parse_time_body(text: &str) -> Result<i64, ApiError> {
    let t = text.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::field(
            "clob_time",
            "not a bare unix-second integer",
        ));
    }
    let s: i64 = t
        .parse()
        .map_err(|e: std::num::ParseIntError| ApiError::field("clob_time", e.to_string()))?;
    if !(MIN_UNIX_S..=MAX_UNIX_S).contains(&s) {
        return Err(ApiError::field("clob_time", "unix seconds out of range"));
    }
    Ok(s)
}

#[cfg(test)]
mod time_tests {
    #![allow(clippy::unwrap_used)]
    use super::{ClobClient, parse_time_body};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    #[test]
    fn a_value_too_large_to_be_unix_seconds_is_rejected() {
        assert!(parse_time_body("200000000000").is_err());
    }

    #[test]
    fn a_millisecond_looking_value_is_rejected() {
        // 1790523400000 is 1790523400 in milliseconds, not seconds: 13 digits, far past the
        // accepted unix-second range, so it must never silently pass as seconds.
        assert!(parse_time_body("1790523400000").is_err());
    }

    #[test]
    fn a_leading_plus_is_rejected() {
        assert!(parse_time_body("+1790523400").is_err());
    }

    fn recorder() -> (crate::RttSink, Arc<Mutex<Vec<i64>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sink: crate::RttSink = Arc::new(move |ms| s2.lock().unwrap().push(ms));
        (sink, seen)
    }

    /// `/time` must never feed `rest_rtt_*` (the host-band input): a normal run's band inputs
    /// must stay byte-for-byte what they were before this brief. `ok()` on the same client, over
    /// the same mock server, DOES report — proving the recorder isn't just inert.
    #[tokio::test]
    async fn time_never_reports_to_the_rtt_sink_but_ok_still_does() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2u8 {
                let Ok((mut s, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf).await;
                let body = "1790523400";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        let (sink, seen) = recorder();
        let client = reqwest::Client::builder().build().unwrap();
        let clob = ClobClient {
            http: client,
            base: format!("{}{addr}", crate::routes::LOCAL_TEST_SCHEME),
            rtt: Some(sink),
        };
        let sample = clob.time().await.unwrap();
        assert_eq!(sample.server_s, 1_790_523_400);
        assert!(
            seen.lock().unwrap().is_empty(),
            "time() must not feed the rtt sink"
        );
        assert!(clob.ok().await.unwrap());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "ok() should still report its round trip"
        );
    }
}
