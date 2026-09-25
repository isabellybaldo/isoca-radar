//! Websocket shards (spec §7, §8.1.2). Each shard reconnects on its own with backoff + jitter
//! (1 s → 60 s, reset only after a session > 30 s). ONE global budget gates every connection
//! attempt across shards (≤ 2/s, ≤ 20/min). K failures in W minutes park the shard until the next
//! universe refresh.
use crate::config::ConnectCfg;
use crate::metrics::Metrics;
use crate::now_ms;
use crate::shards::Plan;
use pm_api::backoff::{Backoff, ConnectBudget};
use pm_api::ws::{Frame, run_connection};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub enum FeedMsg {
    Frame(Frame),
    Up { conn: usize },
    Down { conn: usize, reason: String },
    Parked { conn: usize },
}

#[derive(Debug, Clone)]
pub struct Parking {
    max: usize,
    window_ms: i64,
    fails: VecDeque<i64>,
}

impl Parking {
    pub fn new(max_failures: usize, window_ms: i64) -> Parking {
        Parking {
            max: max_failures.max(1),
            window_ms,
            fails: VecDeque::new(),
        }
    }

    pub fn record(&mut self, now_ms: i64) -> bool {
        self.fails.push_back(now_ms);
        while self
            .fails
            .front()
            .is_some_and(|&t| now_ms - t > self.window_ms)
        {
            self.fails.pop_front();
        }
        self.fails.len() >= self.max
    }
}

/// 80–120 % of `delay_ms`, from a cheap deterministic hash of (conn, attempt).
pub fn jittered(delay_ms: u64, conn: usize, attempt: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in (conn as u64)
        .to_le_bytes()
        .into_iter()
        .chain(attempt.to_le_bytes())
    {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let pct = 80 + h % 41; // 80..=120
    delay_ms.saturating_mul(pct) / 100
}

pub fn needs_restart(old: Option<&[String]>, finished: bool, new: &[String]) -> bool {
    match old {
        None => true,
        Some(t) => finished || t != new,
    }
}

fn take_budget(budget: &Mutex<ConnectBudget>) -> bool {
    budget
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .try_take(now_ms())
}

pub fn spawn_shard(
    conn: usize,
    tokens: Vec<String>,
    shared: mpsc::Sender<FeedMsg>,
    budget: Arc<Mutex<ConnectBudget>>,
    cfg: ConnectCfg,
    capacity: usize,
    metrics: Arc<Metrics>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Backoff::new(
            cfg.backoff_base_ms,
            cfg.backoff_cap_ms,
            cfg.healthy_session_ms,
        );
        let mut parking = Parking::new(cfg.park_after_failures, cfg.park_window_secs * 1000);
        let mut attempt: u32 = 0;
        loop {
            while !take_budget(&budget) {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let started = now_ms();
            backoff.on_connected(started);
            if shared.send(FeedMsg::Up { conn }).await.is_err() {
                return;
            }
            let (ftx, mut frx) = mpsc::channel::<Frame>(capacity.max(1));
            let fwd_shared = shared.clone();
            let fwd = tokio::spawn(async move {
                while let Some(f) = frx.recv().await {
                    if fwd_shared.send(FeedMsg::Frame(f)).await.is_err() {
                        break;
                    }
                }
            });
            let result = run_connection(conn, tokens.clone(), ftx).await;
            let _ = fwd.await; // all of this session's frames are in `shared` before Down
            let ended = now_ms();
            if matches!(result, Err(pm_api::ApiError::Dropped { .. })) {
                metrics.drops.fetch_add(1, Ordering::Relaxed);
            }
            metrics.reconnects.fetch_add(1, Ordering::Relaxed);
            let reason = match &result {
                Ok(()) => "closed".to_string(),
                Err(e) => e.to_string(),
            };
            if shared.send(FeedMsg::Down { conn, reason }).await.is_err() {
                return;
            }
            let failed = result.is_err() || ended - started < cfg.healthy_session_ms;
            if failed && parking.record(ended) {
                metrics.parked.fetch_add(1, Ordering::Relaxed);
                let _ = shared.send(FeedMsg::Parked { conn }).await;
                return;
            }
            let delay = backoff.on_disconnected(ended);
            attempt = attempt.saturating_add(1);
            tokio::time::sleep(Duration::from_millis(jittered(delay, conn, attempt))).await;
        }
    })
}

pub struct Supervisor {
    handles: Vec<(Vec<String>, JoinHandle<()>)>,
    shared: mpsc::Sender<FeedMsg>,
    budget: Arc<Mutex<ConnectBudget>>,
    cfg: ConnectCfg,
    capacity: usize,
    metrics: Arc<Metrics>,
}

impl Supervisor {
    pub fn new(
        shared: mpsc::Sender<FeedMsg>,
        budget: Arc<Mutex<ConnectBudget>>,
        cfg: ConnectCfg,
        capacity: usize,
        metrics: Arc<Metrics>,
    ) -> Supervisor {
        Supervisor {
            handles: Vec::new(),
            shared,
            budget,
            cfg,
            capacity,
            metrics,
        }
    }

    pub async fn apply(&mut self, plan: &Plan) {
        while self.handles.len() > plan.shards.len() {
            if let Some((_, h)) = self.handles.pop() {
                let running = !h.is_finished();
                h.abort();
                if running {
                    let conn = self.handles.len();
                    let _ = self
                        .shared
                        .send(FeedMsg::Down {
                            conn,
                            reason: "shard removed".into(),
                        })
                        .await;
                }
            }
        }
        let parked_before = self.handles.iter().filter(|(_, h)| h.is_finished()).count() as u64;
        for (i, s) in plan.shards.iter().enumerate() {
            let (old, finished) = match self.handles.get(i) {
                Some((t, h)) => (Some(t.as_slice()), h.is_finished()),
                None => (None, false),
            };
            if !needs_restart(old, finished, &s.tokens) {
                continue;
            }
            if let Some((_, h)) = self.handles.get(i)
                && !h.is_finished()
            {
                h.abort();
                let _ = self
                    .shared
                    .send(FeedMsg::Down {
                        conn: i,
                        reason: "resharded".into(),
                    })
                    .await;
            }
            let h = spawn_shard(
                i,
                s.tokens.clone(),
                self.shared.clone(),
                self.budget.clone(),
                self.cfg.clone(),
                self.capacity,
                self.metrics.clone(),
            );
            if i < self.handles.len() {
                self.handles[i] = (s.tokens.clone(), h);
            } else {
                self.handles.push((s.tokens.clone(), h));
            }
        }
        let _ = self
            .metrics
            .parked
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
                Some(p.saturating_sub(parked_before))
            });
        self.metrics
            .shards
            .store(plan.shards.len() as u64, Ordering::Relaxed);
        self.metrics
            .tokens
            .store(plan.tokens() as u64, Ordering::Relaxed);
        self.metrics
            .dropped_groups
            .store(plan.dropped.len() as u64, Ordering::Relaxed);
    }

    pub fn abort_all(&mut self) {
        for (_, h) in self.handles.drain(..) {
            h.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parks_after_max_failures_within_the_window_only() {
        let mut p = Parking::new(3, 600_000);
        assert!(!p.record(0));
        assert!(!p.record(1_000));
        assert!(p.record(2_000));
        let mut q = Parking::new(3, 600_000);
        assert!(!q.record(0));
        assert!(!q.record(1_000));
        assert!(!q.record(700_000), "the first two fell out of the window");
    }

    #[test]
    fn jitter_stays_within_twenty_percent_and_is_deterministic() {
        for conn in 0..40 {
            for attempt in 0..20 {
                let j = jittered(10_000, conn, attempt);
                assert!((8_000..=12_000).contains(&j), "{j}");
                assert_eq!(j, jittered(10_000, conn, attempt));
            }
        }
    }

    #[test]
    fn restart_rules() {
        let a = vec!["t1".to_string()];
        let b = vec!["t2".to_string()];
        assert!(needs_restart(None, false, &a), "new shard");
        assert!(!needs_restart(Some(&a), false, &a), "unchanged and running");
        assert!(needs_restart(Some(&a), false, &b), "tokens changed");
        assert!(
            needs_restart(Some(&a), true, &a),
            "finished (parked) shards are retried"
        );
    }
}
