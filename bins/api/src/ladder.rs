//! GET /api/paper/ladder (Phase 7a spec §7.1): "at which latency would this pay?" For the current
//! epoch, each LADDER account (P0-converter, fee 0) sorted by latency, then the two extras, with
//! its gains (converts), leg-risk cost (unwinds) and settlements since the epoch start, all cut at
//! one common mark so gains + leg_risk + settle + unrealized = net exactly. Live runs only;
//! read-only (analyze_ro). `markets` carries SWEET's 30d NO rows so the panel makes one fetch.
use crate::dto::Dec;
use crate::error::ApiError;
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::State;
use cards::edge::{EXTRAS, LADDER, host_from_key};
use cards::ladder::{BreakEven, Rung, break_even, measured};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;

const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Serialize)]
pub struct RungDto {
    pub account_id: String,
    pub scenario: String,
    pub latency_ms: Option<i64>,
    pub extra: bool,
    pub n_exec: i64,
    pub n_leg_risk: i64,
    pub n_missed: i64,
    pub gains: Dec,
    pub leg_risk: Dec,
    pub settle: Dec,
    pub unrealized: Dec,
    pub net: Dec,
}

#[derive(Debug, Serialize)]
pub struct MeasuredDto {
    pub latency_ms: i64,
    pub host: String,
}

#[derive(Debug, Serialize)]
pub struct LadderDto {
    pub epoch: i32,
    pub start_ms: Option<i64>,
    pub as_of_ms: Option<i64>,
    pub rungs: Vec<RungDto>,
    pub markets: Vec<Value>,
    pub measured: Option<MeasuredDto>,
    pub markets_status: &'static str,
    pub break_even: BreakEven,
}

/// SWEET's card status as the hero's fold needs it: a `thin` card still carries its rows.
fn markets_status(body: &Value) -> &'static str {
    match body["status"].as_str() {
        Some("live") | Some("thin") => "live",
        Some("warming") => "warming",
        Some("under_review") => "under_review",
        _ => "missing",
    }
}

/// NO-side rows at fee 0 for the LADDER scenarios, dimensions `all` and `category`.
fn ladder_rows(body: &Value) -> Vec<Value> {
    body["series"]["sweet"]["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|r| {
                    r["side"].as_str() == Some("no")
                        && r["fee_bips"].as_i64() == Some(0)
                        && r["scenario"].as_str().is_some_and(|s| LADDER.contains(&s))
                        && r["dimension"]
                            .as_str()
                            .is_some_and(|d| d == "all" || d == "category")
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// `break_even` over ALL six LADDER rungs: it drops a rung of unknown latency and never sees an
/// omitted one, so a rung with no account yet, or no known latency, makes the answer `thin` here
/// (interpolating across a gap would place the break-even from five rungs as if they were six).
fn ladder_break_even(rungs: &[RungDto]) -> BreakEven {
    let mut ladder = Vec::with_capacity(LADDER.len());
    for scenario in LADDER {
        match rungs.iter().find(|r| !r.extra && r.scenario == scenario) {
            Some(r) if r.latency_ms.is_some() => ladder.push(Rung {
                latency_ms: r.latency_ms,
                extra: false,
                n_exec: r.n_exec,
                net: r.net.0,
            }),
            _ => return BreakEven::Thin,
        }
    }
    break_even(&ladder)
}

pub async fn ladder(State(st): State<AppState>) -> Result<Json<LadderDto>, ApiError> {
    let now = now_ms();
    // before migrations 0004/0009 there are no paper tables: epoch 0, never a 500
    let epochs = if st.store.paper_ready().await? {
        st.store.paper_epochs().await?
    } else {
        Vec::new()
    };
    let (epoch, start_ms) = epochs
        .last()
        .map_or((0, None), |e| (e.epoch, Some(e.start_ms)));
    let latency = |scenario: &str| {
        st.scenarios
            .as_ref()
            .and_then(|s| s.iter().find(|x| x.name == scenario))
            .map(|x| x.latency_ms)
    };
    let (mut rungs, as_of_ms) = match start_ms {
        // epoch 0 (no epoch row yet): nothing to sum from a common start
        None => (Vec::new(), None),
        Some(start) => {
            let accounts: Vec<_> = st
                .store
                .paper_accounts_now()
                .await?
                .into_iter()
                .filter(|a| {
                    a.policy == "P0-converter"
                        && a.fee_bips.is_zero()
                        && (LADDER.contains(&a.scenario.as_str())
                            || EXTRAS.contains(&a.scenario.as_str()))
                })
                .collect();
            let ids: Vec<String> = accounts.iter().map(|a| a.account_id.clone()).collect();
            let as_of = st.store.ladder_as_of(&ids, start).await?;
            let sums = match as_of {
                Some(cut) => st.store.ladder_sums(&ids, start, cut).await?,
                None => Vec::new(),
            };
            let z = Decimal::ZERO;
            let rungs = accounts
                .iter()
                .map(|a| {
                    let s = sums.iter().find(|x| x.account_id == a.account_id);
                    RungDto {
                        account_id: a.account_id.clone(),
                        scenario: a.scenario.clone(),
                        latency_ms: latency(&a.scenario),
                        extra: EXTRAS.contains(&a.scenario.as_str()),
                        n_exec: s.map_or(0, |s| s.n_exec),
                        n_leg_risk: s.map_or(0, |s| s.n_leg_risk),
                        n_missed: s.map_or(0, |s| s.n_missed),
                        gains: Dec(s.map_or(z, |s| s.gains)),
                        leg_risk: Dec(s.map_or(z, |s| s.leg_risk)),
                        settle: Dec(s.map_or(z, |s| s.settle)),
                        unrealized: Dec(s.and_then(|s| s.unrealized).unwrap_or(z)),
                        net: Dec(s.and_then(|s| s.equity).map_or(z, |e| e - a.bankroll)),
                    }
                })
                .collect::<Vec<_>>();
            (rungs, as_of)
        }
    };
    rungs.sort_by(|a, b| {
        (a.extra, a.latency_ms.unwrap_or(i64::MAX), &a.account_id).cmp(&(
            b.extra,
            b.latency_ms.unwrap_or(i64::MAX),
            &b.account_id,
        ))
    });
    let be = ladder_break_even(&rungs);
    let from = now - DAY_MS;
    let runs = st.store.live_run_ids_between(from, now).await?;
    let minutes = st.store.health_latency(&runs, from, now).await?;
    let measured = match measured(minutes.iter().map(|m| (m.lag_p50_ms, m.rest_rtt_p50_ms))) {
        None => None,
        Some(latency_ms) => {
            let host = st.store.newest_live_host().await?;
            Some(MeasuredDto {
                latency_ms,
                host: host
                    .as_deref()
                    .and_then(|v| host_from_key(&format!("host={v}")).map(str::to_string))
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        }
    };
    let sweet = st
        .store
        .card_results_for("30d")
        .await?
        .into_iter()
        .find(|(c, _)| c == "SWEET");
    let (markets_status, markets) = match sweet {
        None => ("missing", Vec::new()),
        Some((_, body)) => {
            let v: Value = serde_json::from_str(&body).map_err(|e| {
                eprintln!("api: unreadable card SWEET: {e}");
                ApiError::Internal
            })?;
            match markets_status(&v) {
                s @ ("live" | "warming") => (s, ladder_rows(&v)),
                s => (s, Vec::new()),
            }
        }
    };
    Ok(Json(LadderDto {
        epoch,
        start_ms,
        as_of_ms,
        rungs,
        markets,
        measured,
        markets_status,
        break_even: be,
    }))
}
