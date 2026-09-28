//! A read-only projection of the scanner's universe record (bins/scanner/src/capture.rs
//! `UniverseRecord`): only the fields the compactor needs. serde ignores the rest, so the scanner
//! can add fields freely; bins/scanner/tests/tape_agreement.rs parses a real record with it.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegView {
    pub market_id: String,
    pub yes_token: String,
    pub no_token: String,
    /// The scanner's `Status` in snake_case: "live", "halted", "closed_no", …
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupView {
    pub event_id: String,
    #[serde(default)]
    pub neg_risk_market_id: Option<String>,
    pub legs: Vec<LegView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryView {
    pub market_id: String,
    pub event_id: String,
    pub yes_token: String,
    pub no_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseView {
    pub taken_ms: i64,
    pub groups: Vec<GroupView>,
    pub canary: Vec<CanaryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordView {
    pub universe: UniverseView,
    /// Unit key → connection: an event id for a group, `canary_key(market_id)` for a canary pair.
    pub conn_of: BTreeMap<String, usize>,
    pub file: String,
}

/// The shard-plan unit key of a canary pair (bins/scanner/src/shards.rs re-exports this one).
pub fn canary_key(market_id: &str) -> String {
    format!("canary:{market_id}")
}

pub fn load_record(dir: &Path, taken_ms: i64, file: &str) -> anyhow::Result<RecordView> {
    crate::files::read_zst_json(&crate::files::universe_path(dir, taken_ms, file))
}
