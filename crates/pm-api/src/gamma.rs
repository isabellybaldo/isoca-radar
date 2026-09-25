//! Gamma (market discovery): wire types and normalization (spec §3, §7).
use crate::ApiError;
use crate::routes::{GAMMA_BASE, GAMMA_EVENTS_KEYSET};
use crate::wire::{decimal, string_array, string_or_number};
use arb_core::detect::LegStatus;
use arb_core::fee::FeeSchedule;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeScheduleWire {
    pub rate: Option<Value>,
    pub exponent: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaMarket {
    #[serde(deserialize_with = "string_or_number")]
    pub id: String,
    pub condition_id: Option<String>,
    pub question: Option<String>,
    pub group_item_title: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub accepting_orders: Option<bool>,
    pub enable_order_book: Option<bool>,
    pub neg_risk_other: Option<bool>,
    pub clob_token_ids: Option<Value>,
    pub outcome_prices: Option<Value>,
    pub fees_enabled: Option<bool>,
    pub fee_schedule: Option<FeeScheduleWire>,
    pub fee_type: Option<String>,
    pub order_min_size: Option<Value>,
    pub order_price_min_tick_size: Option<Value>,
    pub seconds_delay: Option<Value>,
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaEvent {
    #[serde(deserialize_with = "string_or_number")]
    pub id: String,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub neg_risk: Option<bool>,
    pub enable_neg_risk: Option<bool>,
    pub neg_risk_augmented: Option<bool>,
    #[serde(rename = "negRiskMarketID")]
    pub neg_risk_market_id: Option<String>,
    pub markets: Option<Vec<GammaMarket>>,
}

impl GammaEvent {
    pub fn is_neg_risk(&self) -> bool {
        self.neg_risk.unwrap_or(false) || self.enable_neg_risk.unwrap_or(false)
    }
    pub fn is_augmented(&self) -> bool {
        self.neg_risk_augmented.unwrap_or(false)
    }
    pub fn markets(&self) -> &[GammaMarket] {
        self.markets.as_deref().unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeysetPage {
    #[serde(default)]
    pub events: Vec<GammaEvent>,
    pub next_cursor: Option<String>,
}

pub fn parse_page(text: &str) -> Result<KeysetPage, ApiError> {
    Ok(serde_json::from_str(text)?)
}

/// (YES token, NO token), or None when Gamma has no ids yet.
pub fn token_ids(m: &GammaMarket) -> Result<Option<(String, String)>, ApiError> {
    let ids = string_array(&m.clob_token_ids, "clobTokenIds")?;
    match ids.as_slice() {
        [] => Ok(None),
        [yes, no] => Ok(Some((yes.clone(), no.clone()))),
        _ => Err(ApiError::field(
            "clobTokenIds",
            format!("expected 2 ids, got {}", ids.len()),
        )),
    }
}

/// The market's taker fee schedule; disabled or absent → zero fees.
pub fn fee_schedule(m: &GammaMarket) -> Result<FeeSchedule, ApiError> {
    if m.fees_enabled == Some(false) {
        return Ok(FeeSchedule::ZERO);
    }
    let Some(fs) = &m.fee_schedule else {
        return Ok(FeeSchedule::ZERO);
    };
    let rate = match &fs.rate {
        Some(v) => decimal(v, "feeSchedule.rate")?,
        None => Decimal::ZERO,
    };
    let exponent = match &fs.exponent {
        Some(v) => {
            let d = decimal(v, "feeSchedule.exponent")?;
            if d.fract() != Decimal::ZERO || d < Decimal::ONE || d > Decimal::from(4u8) {
                return Err(ApiError::field("feeSchedule.exponent", d.to_string()));
            }
            let mut e: u8 = 1;
            while Decimal::from(e) < d {
                e += 1;
            }
            e
        }
        None => 1,
    };
    FeeSchedule::new(rate, exponent).map_err(|e| ApiError::field("feeSchedule", e.to_string()))
}

/// Resolved outcome from outcomePrices: Some(true) = YES won, Some(false) = NO won.
fn resolved_outcome(m: &GammaMarket) -> Option<bool> {
    let p = string_array(&m.outcome_prices, "outcomePrices").ok()?;
    if p.len() != 2 {
        return None;
    }
    let y = Decimal::from_str_exact(p[0].trim()).ok()?;
    let n = Decimal::from_str_exact(p[1].trim()).ok()?;
    if y == Decimal::ONE && n == Decimal::ZERO {
        Some(true)
    } else if y == Decimal::ZERO && n == Decimal::ONE {
        Some(false)
    } else {
        None
    }
}

/// Classify a leg (spec §7 universe): Other > Closed* > Live > Placeholder (augmented) / Halted.
pub fn leg_status(m: &GammaMarket, augmented: bool) -> LegStatus {
    if m.neg_risk_other.unwrap_or(false) {
        return LegStatus::Other;
    }
    if m.closed.unwrap_or(false) {
        return match resolved_outcome(m) {
            Some(true) => LegStatus::ClosedYes,
            Some(false) => LegStatus::ClosedNo,
            None => LegStatus::Halted,
        };
    }
    let live = m.active.unwrap_or(false)
        && m.accepting_orders.unwrap_or(false)
        && m.enable_order_book.unwrap_or(false);
    if live {
        LegStatus::Live
    } else if augmented {
        LegStatus::Placeholder
    } else {
        LegStatus::Halted
    }
}

/// Page through Gamma's keyset endpoint. Stops when no cursor is returned.
/// A page that brings no new ids while a cursor is still offered, or hitting `max_pages`,
/// is an error: a silently truncated universe is worse than a failed refresh (spec §7).
/// A page with no cursor that is nonetheless full (`events.len() >= page_size`) is also an
/// error: that shape means the "no more pages" signal is more likely a renamed/dropped field
/// than an actually-final short page (spec F7).
pub async fn fetch_all<F, Fut>(
    mut fetch: F,
    max_pages: usize,
    page_size: usize,
) -> Result<Vec<GammaEvent>, ApiError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<KeysetPage, ApiError>>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for pages in 1..=max_pages {
        let page = fetch(cursor.clone()).await?;
        let page_len = page.events.len();
        let mut new_ids = 0usize;
        for e in page.events {
            if seen.insert(e.id.clone()) {
                new_ids += 1;
                out.push(e);
            }
        }
        match page.next_cursor {
            Some(c) if !c.is_empty() => {
                if new_ids == 0 {
                    return Err(ApiError::Stuck(pages));
                }
                cursor = Some(c);
            }
            _ => {
                if page_len >= page_size {
                    return Err(ApiError::field(
                        "next_cursor",
                        "missing on a full page (field renamed?)",
                    ));
                }
                return Ok(out);
            }
        }
    }
    Err(ApiError::Stuck(max_pages))
}

pub struct GammaClient {
    http: reqwest::Client,
    base: String,
}

impl GammaClient {
    pub fn new() -> Result<Self, ApiError> {
        Ok(Self {
            http: crate::http::http_client()?,
            base: GAMMA_BASE.to_string(),
        })
    }

    /// One page of active, open events (≤ 100). The cursor parameter is exactly `after_cursor`.
    pub async fn page(&self, cursor: Option<String>) -> Result<KeysetPage, ApiError> {
        let mut query: Vec<(&str, String)> = vec![
            ("closed", "false".to_string()),
            ("active", "true".to_string()),
            ("limit", "100".to_string()),
        ];
        if let Some(c) = cursor {
            query.push(("after_cursor", c));
        }
        let text = self
            .http
            .get(format!("{}{}", self.base, GAMMA_EVENTS_KEYSET))
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        parse_page(&text)
    }

    pub async fn all_active_events(&self) -> Result<Vec<GammaEvent>, ApiError> {
        fetch_all(|c| self.page(c), 1_000, 100).await
    }
}
