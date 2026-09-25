//! Execution latency (spec §5.6): everything is anchored on OUR receive time of the opening frame.
use crate::config::Scenario;
use std::collections::VecDeque;

/// Frames received in the trailing window, from input times only (so replay reproduces it).
#[derive(Debug, Clone, Default)]
pub struct FrameRate {
    window_ms: i64,
    times: VecDeque<i64>,
}

impl FrameRate {
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms: window_ms.max(1),
            times: VecDeque::new(),
        }
    }

    pub fn push(&mut self, t_ms: i64) {
        self.times.push_back(t_ms);
        while let Some(&front) = self.times.front() {
            if front <= t_ms - self.window_ms {
                self.times.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn per_second(&self) -> u64 {
        let n = u64::try_from(self.times.len()).unwrap_or(u64::MAX);
        let w = u64::try_from(self.window_ms).unwrap_or(1).max(1);
        n.saturating_mul(1000) / w
    }
}

/// Matching-time multiplier: the highest step whose threshold the current rate exceeds.
pub fn burst_multiplier(sc: &Scenario, frames_per_s: u64) -> i64 {
    sc.burst
        .iter()
        .filter(|b| frames_per_s > b.frames_per_s_above)
        .map(|b| b.multiplier)
        .max()
        .unwrap_or(1)
}

/// Receive time → matchable: proc + net + match × burst + secondsDelay (if the scenario honors it).
pub fn exec_delay_ms(sc: &Scenario, seconds_delay: Option<i32>, frames_per_s: u64) -> i64 {
    let delay = if sc.honor_seconds_delay {
        i64::from(seconds_delay.unwrap_or(0).max(0)) * 1000
    } else {
        0
    };
    sc.proc_ms + sc.net_ms + sc.match_ms * burst_multiplier(sc, frames_per_s) + delay
}
