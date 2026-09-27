//! GET /api/cards?win= and GET /api/cards/meta (Phase 5a spec §8.1). Both read what analyze wrote:
//! one small query each on card_results / card_meta. Bodies pass through as stored (decimal
//! strings, null distinct from "0"); the api computes nothing.
use crate::AppState;
use crate::error::ApiError;
use crate::params::{self, Params};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub const WINS: [&str; 5] = ["24h", "7d", "30d", "all", "today"];

#[derive(Debug, Serialize)]
pub struct CardsDto {
    pub win: String,
    pub cards: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub struct MetaDto {
    pub tick: Value,
    pub last_error: Value,
    pub catch_up: BTreeMap<String, Value>,
    pub papers: Value,
    pub not_replicated: Value,
}

fn parse(what: &str, text: &str) -> Result<Value, ApiError> {
    serde_json::from_str(text).map_err(|e| {
        eprintln!("api: unreadable {what}: {e}");
        ApiError::Internal
    })
}

pub async fn list(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<CardsDto>, ApiError> {
    let q = params::query(q)?;
    let win = params::win(&q)?;
    let mut cards = Vec::new();
    for (card, body) in st.store.card_results_for(win).await? {
        // spec §8.1: win=today returns Edge capture only
        if win == "today" && card != "EDGE" {
            continue;
        }
        cards.push(parse(&format!("card {card}"), &body)?);
    }
    Ok(Json(CardsDto {
        win: win.to_string(),
        cards,
    }))
}

pub async fn meta(State(st): State<AppState>) -> Result<Json<MetaDto>, ApiError> {
    let mut dto = MetaDto {
        tick: Value::Null,
        last_error: Value::Null,
        catch_up: BTreeMap::new(),
        papers: Value::Array(Vec::new()),
        not_replicated: Value::Array(Vec::new()),
    };
    for (key, value) in st.store.card_meta_all().await? {
        let v = parse(&format!("meta {key}"), &value)?;
        match key.as_str() {
            "tick" => dto.tick = v,
            "last_error" => dto.last_error = v,
            "papers" => dto.papers = v,
            "not_replicated" => dto.not_replicated = v,
            k => {
                if let Some(fam) = k.strip_prefix("fam:") {
                    dto.catch_up.insert(fam.to_string(), v);
                }
            }
        }
    }
    Ok(Json(dto))
}
