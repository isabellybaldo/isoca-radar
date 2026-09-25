//! Shared exact-parsing helpers. Numbers keep their source text (serde_json arbitrary_precision).
use crate::ApiError;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Accept a JSON string or number; numbers are rendered from their exact source text.
pub fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}

/// Exact Decimal from a JSON string or number.
pub fn decimal(v: &Value, field: &'static str) -> Result<Decimal, ApiError> {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => return Err(ApiError::field(field, format!("not a number: {other}"))),
    };
    Decimal::from_str_exact(text.trim()).map_err(|e| ApiError::field(field, e.to_string()))
}

/// A list of strings sent either as a real JSON array or as a JSON-encoded string ("[\"a\",\"b\"]").
/// Absent or null → empty.
pub fn string_array(v: &Option<Value>, field: &'static str) -> Result<Vec<String>, ApiError> {
    let raw = match v {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(v) => v,
    };
    let arr = match raw {
        Value::String(s) => {
            serde_json::from_str::<Value>(s).map_err(|e| ApiError::field(field, e.to_string()))?
        }
        other => other.clone(),
    };
    match arr {
        Value::Array(items) => items
            .into_iter()
            .map(|x| match x {
                Value::String(s) => Ok(s),
                Value::Number(n) => Ok(n.to_string()),
                other => Err(ApiError::field(
                    field,
                    format!("unexpected element {other}"),
                )),
            })
            .collect(),
        other => Err(ApiError::field(field, format!("not an array: {other}"))),
    }
}
