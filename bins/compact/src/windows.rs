//! l2 windows around net-positive arbitrage episodes (spec 5b §3, §4.2).
use crate::config::Config;
use serde::{Deserialize, Serialize};
use store::CompactEpisode;
use tape::time::HOUR_MS;

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

/// [open − before, close + after), capped at `max_ms` from its start. A still-open episode is
/// capped the same way (spec §4.2: an hour never waits for an episode to close).
pub fn window_of(ep: &CompactEpisode, before_ms: i64, after_ms: i64, max_ms: i64) -> Window {
    let start_ms = ep.opened_ms.saturating_sub(before_ms);
    let cap = start_ms.saturating_add(max_ms);
    let natural = ep.closed_ms.map(|c| c.saturating_add(after_ms));
    Window {
        episode_id: ep.episode_id.clone(),
        event_id: ep.event_id.clone(),
        opened_ms: ep.opened_ms,
        start_ms,
        end_ms: natural.map_or(cap, |e| e.min(cap)),
        truncated: natural.is_none_or(|e| e > cap),
    }
}

/// The opens [from, to) whose window can touch [hour, hour + 1 h).
pub fn query_range(hour_ms: i64, cfg: &Config) -> (i64, i64) {
    let before = cfg.l2_before_secs.saturating_mul(1_000);
    let max = cfg.l2_max_window_secs.saturating_mul(1_000);
    (hour_ms - max + before, hour_ms + HOUR_MS + before)
}

/// The windows that intersect the hour, ordered by open then id (the merge order).
pub fn windows_for_hour(eps: &[CompactEpisode], hour_ms: i64, cfg: &Config) -> Vec<Window> {
    let mut out: Vec<Window> = eps
        .iter()
        .map(|e| {
            window_of(
                e,
                cfg.l2_before_secs.saturating_mul(1_000),
                cfg.l2_after_secs.saturating_mul(1_000),
                cfg.l2_max_window_secs.saturating_mul(1_000),
            )
        })
        .filter(|w| w.start_ms < hour_ms + HOUR_MS && w.end_ms > hour_ms)
        .collect();
    out.sort_by(|a, b| {
        a.opened_ms
            .cmp(&b.opened_ms)
            .then_with(|| a.episode_id.cmp(&b.episode_id))
    });
    out
}
