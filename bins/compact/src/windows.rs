//! l2 windows around net-positive arbitrage episodes (spec 5b §3). Task 13 adds the math.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub episode_id: String,
    pub event_id: String,
    /// The episode's open (server ms): windows are ordered by it, then by id.
    pub opened_ms: i64,
    pub start_ms: i64,
    /// Exclusive.
    pub end_ms: i64,
    /// Capped at l2_max_window_secs (or still open when read).
    pub truncated: bool,
}
