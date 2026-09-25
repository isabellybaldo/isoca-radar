//! CLOB read endpoints: POST /books (≤ 500 tokens per request) and GET /ok (spec §3).
use crate::ApiError;
use crate::routes::{CLOB_BASE, CLOB_BOOKS, CLOB_OK};
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
}

impl ClobClient {
    pub fn new() -> Result<Self, ApiError> {
        Ok(Self {
            http: crate::http::http_client()?,
            base: CLOB_BASE.to_string(),
        })
    }

    pub async fn books(&self, tokens: &[String]) -> Result<Vec<BookWire>, ApiError> {
        let mut out = Vec::new();
        for chunk in chunk_tokens(tokens) {
            let body: Vec<Value> = chunk.iter().map(|t| json!({ "token_id": t })).collect();
            let text = self
                .http
                .post(format!("{}{}", self.base, CLOB_BOOKS))
                .json(&body)
                .send()
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
        let resp = self
            .http
            .get(format!("{}{}", self.base, CLOB_OK))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }
}
