//! Query-string parsing with the spec §3 bounds. Out-of-range input is a 400, never clamped.
use crate::error::ApiError;
use axum::extract::Query;
use axum::extract::rejection::QueryRejection;
use std::collections::HashMap;

pub type Params = HashMap<String, String>;

pub const MAX_HOURS: i64 = 168;
pub const MAX_RANGE_MS: i64 = 7 * 86_400_000;
pub const MAX_LIMIT: i64 = 200;
pub const DEFAULT_LIMIT: i64 = 50;

fn bad(code: &'static str, detail: &str) -> ApiError {
    ApiError::BadRequest(code, detail.to_string())
}

pub fn query(q: Result<Query<Params>, QueryRejection>) -> Result<Params, ApiError> {
    q.map(|Query(p)| p)
        .map_err(|_| bad("bad_query", "The query string could not be read."))
}

pub fn opt_i64(q: &Params, name: &str, code: &'static str) -> Result<Option<i64>, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|_| bad(code, &format!("{name} must be an integer."))),
    }
}

pub fn hours(q: &Params, default: i64) -> Result<i64, ApiError> {
    let h = opt_i64(q, "hours", "bad_hours")?.unwrap_or(default);
    if (1..=MAX_HOURS).contains(&h) {
        Ok(h)
    } else {
        Err(bad(
            "bad_hours",
            "hours must be between 1 and 168. A week is plenty.",
        ))
    }
}

pub fn limit(q: &Params) -> Result<i64, ApiError> {
    let l = opt_i64(q, "limit", "bad_limit")?.unwrap_or(DEFAULT_LIMIT);
    if (1..=MAX_LIMIT).contains(&l) {
        Ok(l)
    } else {
        Err(bad("bad_limit", "limit must be between 1 and 200."))
    }
}

pub fn flag(q: &Params, name: &str) -> Result<bool, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") | Some("0") | Some("false") => Ok(false),
        Some("1") | Some("true") => Ok(true),
        Some(_) => Err(bad("bad_flag", &format!("{name} must be 1 or 0."))),
    }
}

/// A short text parameter without control characters; empty counts as absent.
pub fn text<'a>(
    q: &'a Params,
    name: &str,
    max_len: usize,
    code: &'static str,
) -> Result<Option<&'a str>, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") => Ok(None),
        Some(s) if s.len() <= max_len && !s.chars().any(char::is_control) => Ok(Some(s)),
        Some(_) => Err(bad(
            code,
            &format!("{name} is too long or has control characters."),
        )),
    }
}

/// `from`/`to` in ms since epoch. Defaults: to = now, from = to − 7 days. Needs 0 < to − from ≤ 7 days.
pub fn range(q: &Params, now_ms: i64) -> Result<(i64, i64), ApiError> {
    let to = opt_i64(q, "to", "bad_range")?.unwrap_or(now_ms);
    let from = opt_i64(q, "from", "bad_range")?.unwrap_or(to.saturating_sub(MAX_RANGE_MS));
    match to.checked_sub(from) {
        Some(span) if span > 0 && span <= MAX_RANGE_MS => Ok((from, to)),
        _ => Err(bad(
            "bad_range",
            "from must be before to, and seven days apart at most.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn bounds_are_errors_not_clamps() {
        assert_eq!(hours(&p(&[]), 24), Ok(24));
        assert_eq!(hours(&p(&[("hours", "168")]), 24), Ok(168));
        assert!(hours(&p(&[("hours", "169")]), 24).is_err());
        assert_eq!(limit(&p(&[])), Ok(50));
        assert!(limit(&p(&[("limit", "201")])).is_err());
        assert!(limit(&p(&[("limit", "0")])).is_err());
        assert_eq!(flag(&p(&[("net", "1")]), "net"), Ok(true));
        assert!(flag(&p(&[("net", "yes")]), "net").is_err());
        assert!(text(&p(&[("kind", "a\nb")]), "kind", 32, "bad_kind").is_err());
    }

    #[test]
    fn ranges_default_to_the_last_seven_days_and_reject_longer_ones() {
        assert_eq!(
            range(&p(&[]), 1_000_000_000),
            Ok((1_000_000_000 - MAX_RANGE_MS, 1_000_000_000))
        );
        assert_eq!(range(&p(&[("from", "10"), ("to", "20")]), 0), Ok((10, 20)));
        assert!(range(&p(&[("from", "20"), ("to", "10")]), 0).is_err());
        assert!(
            range(
                &p(&[("from", "0"), ("to", &(MAX_RANGE_MS + 1).to_string())]),
                0
            )
            .is_err()
        );
        assert!(
            range(
                &p(&[
                    ("from", "-9223372036854775808"),
                    ("to", "9223372036854775807")
                ]),
                0
            )
            .is_err()
        );
    }
}
