//! Reconnect pacing (spec §8.1.2). Pure: time is passed in. Jitter is added by the caller.
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    cap_ms: u64,
    healthy_ms: i64,
    attempt: u32,
    connected_at: Option<i64>,
}

impl Backoff {
    pub fn new(base_ms: u64, cap_ms: u64, healthy_ms: i64) -> Self {
        Self {
            base_ms,
            cap_ms,
            healthy_ms,
            attempt: 0,
            connected_at: None,
        }
    }

    pub fn on_connected(&mut self, now_ms: i64) {
        self.connected_at = Some(now_ms);
    }

    /// Delay before the next attempt. Resets to `base_ms` only if the session that just ended
    /// lived longer than `healthy_ms` (a flapping link must not pull the delay down).
    pub fn on_disconnected(&mut self, now_ms: i64) -> u64 {
        if let Some(t) = self.connected_at.take()
            && now_ms - t > self.healthy_ms
        {
            self.attempt = 0;
        }
        let shift = self.attempt.min(20);
        let delay = self.base_ms.saturating_mul(1u64 << shift).min(self.cap_ms);
        self.attempt = self.attempt.saturating_add(1);
        delay
    }
}

/// Global limit on connection attempts across all shards: ≤ per_sec in any 1 s, ≤ per_min in any 60 s.
#[derive(Debug, Clone)]
pub struct ConnectBudget {
    per_sec: usize,
    per_min: usize,
    recent: VecDeque<i64>,
}

impl ConnectBudget {
    pub fn new(per_sec: usize, per_min: usize) -> Self {
        Self {
            per_sec,
            per_min,
            recent: VecDeque::new(),
        }
    }

    pub fn try_take(&mut self, now_ms: i64) -> bool {
        while self.recent.front().is_some_and(|&t| now_ms - t >= 60_000) {
            self.recent.pop_front();
        }
        let last_sec = self.recent.iter().filter(|&&t| now_ms - t < 1_000).count();
        if last_sec >= self.per_sec || self.recent.len() >= self.per_min {
            return false;
        }
        self.recent.push_back(now_ms);
        true
    }
}
