//! Shared counters for the health task (spec §10 `health`). Counters are drained per minute; gauges are read.
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};

#[derive(Debug, Default)]
pub struct Metrics {
    pub msgs: AtomicU64,
    pub bytes: AtomicU64,
    pub drops: AtomicU64,
    pub reconnects: AtomicU64,
    pub capture_drops: AtomicU64,
    pub parked: AtomicU64,
    pub tokens: AtomicU64,
    pub shards: AtomicU64,
    pub dropped_groups: AtomicU64,
    pub capture_paused: AtomicBool,
    pub lags: Mutex<Vec<i64>>,
    pub rest_ms: Mutex<Vec<i64>>,
    pub ok: Mutex<Option<bool>>,
}
