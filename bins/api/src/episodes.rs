//! Episodes (spec §4): open (Radar), keyset-paged list (Episodes), detail, today's counts.
use crate::dto::{Dec, Page, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use serde::Serialize;
use store::{EpisodeFilter, EpisodeLegRow, EpisodeRow, EpisodeSummary, ObservationRow, Today};

pub const KINDS: [&str; 4] = [
    "yes_basket",
    "yes_basket_incomplete",
    "no_basket",
    "implication",
];

/// Episode ids are scanner-made (`live-7-00001234`): short, ASCII, no spaces.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn encode_cursor(opened_ms: i64, id: &str) -> String {
    format!("{opened_ms}_{id}")
}

pub fn decode_cursor(s: &str) -> Option<(i64, String)> {
    let (ms, id) = s.split_once('_')?;
    let ms = ms.parse::<i64>().ok()?;
    valid_id(id).then(|| (ms, id.to_string()))
}

#[derive(Debug, Serialize)]
pub struct EpisodeSummaryDto {
    pub id: String,
    pub kind: String,
    pub realization: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub n_legs: i32,
    pub s_legs: i64,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub edge: Dec,
    pub q_star: Dec,
    pub net: Dec,
    pub gross: Dec,
    /// Net profit at the peak with at most $100 spent (null: not sized).
    pub net_budget100: Option<Dec>,
    pub suspect: bool,
    pub stale: bool,
    pub censored: bool,
}

impl From<EpisodeSummary> for EpisodeSummaryDto {
    fn from(e: EpisodeSummary) -> Self {
        Self {
            id: e.episode_id,
            kind: e.kind,
            realization: e.realization,
            event_title: e.event_title,
            event_slug: e.event_slug,
            n_legs: e.n_legs,
            s_legs: e.s_legs,
            opened_ms: e.opened_server_ms,
            closed_ms: e.closed_server_ms,
            duration_ms: e.duration_ms,
            edge: Dec(e.peak_edge_top),
            q_star: Dec(e.peak_max_size),
            net: Dec(e.peak_pnl_net_usd),
            gross: Dec(e.peak_pnl_gross_usd),
            net_budget100: opt(e.pnl_net_budget100_usd),
            suspect: e.engine_suspect,
            stale: e.stale_transport,
            censored: e.censored,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OpenDto {
    pub now_ms: i64,
    pub items: Vec<EpisodeSummaryDto>,
}

#[derive(Debug, Serialize)]
pub struct EpisodeDto {
    pub id: String,
    pub run_id: i64,
    pub source: String,
    pub kind: String,
    pub realization: String,
    pub event_id: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub category: Option<String>,
    pub n_outcomes: i32,
    pub n_legs: i32,
    pub legs_dropped: i32,
    pub augmented: bool,
    pub incomplete_set: bool,
    pub uncovered_legs: i32,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    pub detect_lag_ms: Option<i64>,
    pub n_updates: i32,
    pub peak_ms: i64,
    pub edge: Dec,
    pub edge_gross: Dec,
    pub q_star: Dec,
    pub cost: Dec,
    pub payout: Dec,
    pub fees: Dec,
    pub net: Dec,
    pub gross: Dec,
    pub profit_per_dollar: Option<Dec>,
    pub net_budget100: Option<Dec>,
    pub bottleneck_leg_idx: Option<i32>,
    pub below_min_size: Option<bool>,
    pub max_seconds_delay: Option<i32>,
    pub days_to_end: Option<Dec>,
    pub annualized_return: Option<Dec>,
    pub post_end: Option<bool>,
    pub suspect: bool,
    pub stale: bool,
    pub open_lag_ms: Option<i64>,
    pub oracle_downgraded: Option<bool>,
    pub scanner_version: String,
}

impl From<EpisodeRow> for EpisodeDto {
    fn from(e: EpisodeRow) -> Self {
        Self {
            id: e.episode_id,
            run_id: e.run_id,
            source: e.source,
            kind: e.kind,
            realization: e.realization,
            event_id: e.event_id,
            event_title: e.event_title,
            event_slug: e.event_slug,
            category: e.category,
            n_outcomes: e.n_outcomes,
            n_legs: e.n_legs,
            legs_dropped: e.legs_dropped,
            augmented: e.augmented,
            incomplete_set: e.incomplete_set,
            uncovered_legs: e.uncovered_legs,
            opened_ms: e.opened_server_ms,
            closed_ms: e.closed_server_ms,
            duration_ms: e.duration_ms,
            censor_left: e.censor_left,
            censor_right: e.censor_right,
            censor_gap: e.censor_gap,
            detect_lag_ms: e.detect_lag_ms,
            n_updates: e.n_updates,
            peak_ms: e.peak_server_ms,
            edge: Dec(e.peak_edge_top),
            edge_gross: Dec(e.peak_edge_top_gross),
            q_star: Dec(e.peak_max_size),
            cost: Dec(e.peak_cost_usd),
            payout: Dec(e.peak_payout_usd),
            fees: Dec(e.peak_fees_usd),
            net: Dec(e.peak_pnl_net_usd),
            gross: Dec(e.peak_pnl_gross_usd),
            profit_per_dollar: opt(e.peak_profit_per_dollar),
            net_budget100: opt(e.pnl_net_budget100_usd),
            bottleneck_leg_idx: e.bottleneck_leg_idx,
            below_min_size: e.below_min_size,
            max_seconds_delay: e.max_seconds_delay,
            days_to_end: opt(e.days_to_end),
            annualized_return: opt(e.annualized_return),
            post_end: e.post_end,
            suspect: e.engine_suspect,
            stale: e.stale_transport,
            open_lag_ms: e.open_lag_ms,
            oracle_downgraded: e.oracle_downgraded,
            scanner_version: e.scanner_version,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LegDto {
    pub leg_idx: i32,
    pub market_id: String,
    /// buy_yes | buy_no
    pub action: String,
    pub best_price: Option<Dec>,
    pub vwap: Option<Dec>,
    pub levels: i32,
    pub size_at_best: Option<Dec>,
    pub filled: Dec,
    pub notional: Dec,
    pub fees: Dec,
    pub in_s: bool,
}

impl From<EpisodeLegRow> for LegDto {
    fn from(l: EpisodeLegRow) -> Self {
        Self {
            leg_idx: l.leg_idx,
            market_id: l.market_id,
            action: l.action,
            best_price: opt(l.best_price),
            vwap: opt(l.vwap),
            levels: l.levels,
            size_at_best: opt(l.size_at_best),
            filled: Dec(l.filled),
            notional: Dec(l.notional),
            fees: Dec(l.fees),
            in_s: l.in_s,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ObservationDto {
    pub offset_ms: i32,
    /// ws | rest
    pub source: String,
    pub observed_ms: Option<i64>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub edge: Option<Dec>,
    pub q_star: Option<Dec>,
    pub pnl: Option<Dec>,
    pub still_profitable: Option<bool>,
    pub fill_at_original_size: Option<bool>,
}

impl From<ObservationRow> for ObservationDto {
    fn from(o: ObservationRow) -> Self {
        Self {
            offset_ms: o.offset_ms,
            source: o.source,
            observed_ms: o.observed_ms,
            skipped: o.skipped,
            skip_reason: o.skip_reason,
            edge: opt(o.edge),
            q_star: opt(o.q_max),
            pnl: opt(o.pnl),
            still_profitable: o.still_profitable,
            fill_at_original_size: o.fill_at_original_size,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DetailDto {
    pub episode: EpisodeDto,
    pub legs: Vec<LegDto>,
    pub observations: Vec<ObservationDto>,
}

#[derive(Debug, Serialize)]
pub struct HourDto {
    pub hour: i64,
    pub arb: i64,
}

#[derive(Debug, Serialize)]
pub struct TodayDto {
    pub day_start_ms: i64,
    pub arb: i64,
    pub no: i64,
    pub yes: i64,
    /// Sum of the $100-capped net edge over today's arbitrage episodes (null: none measured).
    pub edge_budget100: Option<Dec>,
    pub hourly: Vec<HourDto>,
}

impl From<Today> for TodayDto {
    fn from(t: Today) -> Self {
        Self {
            day_start_ms: t.day_start_ms,
            arb: t.arb,
            no: t.no,
            yes: t.yes,
            edge_budget100: opt(t.edge_budget100),
            hourly: (0i64..)
                .zip(t.hourly)
                .map(|(hour, arb)| HourDto { hour, arb })
                .collect(),
        }
    }
}

fn bad(code: &'static str, detail: &str) -> ApiError {
    ApiError::BadRequest(code, detail.to_string())
}

pub async fn open(State(st): State<AppState>) -> Result<Json<OpenDto>, ApiError> {
    let items = st
        .store
        .open_episodes()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(OpenDto {
        now_ms: now_ms(),
        items,
    }))
}

pub async fn list(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<Page<EpisodeSummaryDto>>, ApiError> {
    let q = params::query(q)?;
    let kind = match params::text(&q, "kind", 32, "bad_kind")? {
        None => None,
        Some(k) if KINDS.contains(&k) => Some(k.to_string()),
        Some(_) => {
            return Err(bad(
                "bad_kind",
                "kind must be one of yes_basket, yes_basket_incomplete, no_basket, implication.",
            ));
        }
    };
    let (from_ms, to_ms) = params::range(&q, now_ms())?;
    let (mut suspect, mut stale, mut censored) = (false, false, false);
    if let Some(flags) = params::text(&q, "flags", 64, "bad_flags")? {
        for f in flags.split(',') {
            match f {
                "suspect" => suspect = true,
                "stale" => stale = true,
                "censored" => censored = true,
                _ => {
                    return Err(bad(
                        "bad_flags",
                        "flags is a comma list of suspect, stale, censored.",
                    ));
                }
            }
        }
    }
    let before = match params::text(&q, "cursor", 100, "bad_cursor")? {
        None => None,
        Some(c) => Some(
            decode_cursor(c).ok_or_else(|| bad("bad_cursor", "That cursor is not one of ours."))?,
        ),
    };
    let filter = EpisodeFilter {
        kind,
        from_ms,
        to_ms,
        net_positive: params::flag(&q, "net")?,
        suspect,
        stale,
        censored,
        before,
        limit: params::limit(&q)?,
    };
    let page = st.store.episodes_page(&filter).await?;
    Ok(Json(Page {
        items: page.rows.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|(ms, id)| encode_cursor(ms, &id)),
    }))
}

pub async fn detail(
    State(st): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Json<DetailDto>, ApiError> {
    let Ok(Path(id)) = id else {
        return Err(ApiError::NotFound);
    };
    if !valid_id(&id) {
        return Err(ApiError::NotFound);
    }
    let episode = st.store.episode(&id).await?.ok_or(ApiError::NotFound)?;
    let legs = st.store.episode_legs(&id).await?;
    let mut observations = st.store.observations_for(&id).await?;
    observations.sort_by_key(|o| o.offset_ms);
    Ok(Json(DetailDto {
        episode: episode.into(),
        legs: legs.into_iter().map(Into::into).collect(),
        observations: observations.into_iter().map(Into::into).collect(),
    }))
}

pub async fn today(State(st): State<AppState>) -> Result<Json<TodayDto>, ApiError> {
    Ok(Json(load_today(&st).await?))
}

/// Today's counts as the handler serves them; the public snapshot builds from it too (7b §4).
pub(crate) async fn load_today(st: &AppState) -> Result<TodayDto, store::StoreError> {
    Ok(st.store.today(now_ms()).await?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_junk() {
        let c = encode_cursor(1_790_000_000_000, "live-7-00001234");
        assert_eq!(
            decode_cursor(&c),
            Some((1_790_000_000_000, "live-7-00001234".to_string()))
        );
        assert_eq!(decode_cursor("garbage"), None);
        assert_eq!(decode_cursor("x_live-1"), None);
        assert_eq!(decode_cursor("12_bad id"), None);
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("'; DROP"));
    }
}
