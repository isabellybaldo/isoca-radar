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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
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

/// The `Down` reason for one finished session: "stopped" wins over whatever `run_connection`
/// returned, since a deliberate stop is not a transport failure (Review Focus, fix round 1).
fn session_reason(stopping: bool, result: &Result<(), pm_api::ApiError>) -> String {
    if stopping {
        "stopped".to_string()
    } else {
        match result {
            Ok(()) => "closed".to_string(),
            Err(e) => e.to_string(),
        }
    }
}

/// One websocket shard. `stop` is watched everywhere the shard could otherwise block (budget
/// wait, the connection itself, the backoff sleep) so a caller can ask it to end its current
/// session and return WITHOUT ever aborting the task: dropping the `run_connection` future drops
/// its `ftx`, the forwarder then drains and exits, and only after that does the shard send its
/// own `Down` — so `Down` always follows the shard's last frame (fix round 1, ordering flaw).
/// A closed `stop` channel (sender dropped, `changed()` returning `Err`) is treated exactly like
/// an explicit `true`: `tokio::select!`'s `_ = stop.changed()` arm fires on either outcome, so
/// dropping the `Supervisor` stops every shard even without calling `abort_all`.
#[allow(clippy::too_many_arguments)] // only `Supervisor` calls this; each parameter is load-bearing
pub fn spawn_shard(
    conn: usize,
    tokens: Vec<String>,
    shared: mpsc::Sender<FeedMsg>,
    budget: Arc<Mutex<ConnectBudget>>,
    cfg: ConnectCfg,
    capacity: usize,
    metrics: Arc<Metrics>,
    mut stop: watch::Receiver<bool>,
    parked_flag: Arc<AtomicBool>,
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
            loop {
                if take_budget(&budget) {
                    break;
                }
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(250)) => {}
                    // Not connected yet: nothing to drain, nothing to report.
                    _ = stop.changed() => return,
                }
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
            let mut stopping = false;
            let result = tokio::select! {
                r = run_connection(conn, tokens.clone(), ftx) => r,
                // Dropping this branch's `ftx` (via the future above being cancelled) is what
                // makes the forwarder see a closed channel and drain below.
                _ = stop.changed() => {
                    stopping = true;
                    Ok(())
                }
            };
            let _ = fwd.await; // all of this session's frames are in `shared` before Down
            let ended = now_ms();
            if matches!(result, Err(pm_api::ApiError::Dropped { .. })) {
                metrics.drops.fetch_add(1, Ordering::Relaxed);
            }
            metrics.reconnects.fetch_add(1, Ordering::Relaxed);
            let reason = session_reason(stopping, &result);
            if shared.send(FeedMsg::Down { conn, reason }).await.is_err() {
                return;
            }
            if stopping {
                return;
            }
            let failed = result.is_err() || ended - started < cfg.healthy_session_ms;
            if failed && parking.record(ended) {
                metrics.parked.fetch_add(1, Ordering::Relaxed);
                parked_flag.store(true, Ordering::Relaxed);
                let _ = shared.send(FeedMsg::Parked { conn }).await;
                return;
            }
            let delay = backoff.on_disconnected(ended);
            attempt = attempt.saturating_add(1);
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(jittered(delay, conn, attempt))) => {}
                _ = stop.changed() => return,
            }
        }
    })
}

type Slot = (
    Vec<String>,
    JoinHandle<()>,
    watch::Sender<bool>,
    Arc<AtomicBool>,
);

pub struct Supervisor {
    handles: Vec<Slot>,
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

    /// Ask a shard to stop, wait (bounded) for it to drain and send its own `Down`, and only
    /// then return — so the caller never spawns a replacement for `conn` before the old task is
    /// gone. Falls back to `abort()` only if the shard doesn't end within the timeout (logged:
    /// that shard's `Down`, if any, may be missing or arrive late).
    async fn stop_slot(&self, conn: usize, slot: Slot) {
        let (_, mut handle, tx, parked) = slot;
        let _ = tx.send(true);
        if tokio::time::timeout(Duration::from_secs(5), &mut handle)
            .await
            .is_err()
        {
            eprintln!("feed: shard {conn} did not stop within 5s; aborting");
            handle.abort();
        }
        if parked.load(Ordering::Relaxed) {
            let _ = self
                .metrics
                .parked
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
                    Some(p.saturating_sub(1))
                });
        }
    }

    pub async fn apply(&mut self, plan: &Plan) {
        while self.handles.len() > plan.shards.len() {
            if let Some(slot) = self.handles.pop() {
                let conn = self.handles.len();
                self.stop_slot(conn, slot).await;
            }
        }
        for (i, s) in plan.shards.iter().enumerate() {
            let (old, finished) = match self.handles.get(i) {
                Some((t, h, _, _)) => (Some(t.as_slice()), h.is_finished()),
                None => (None, false),
            };
            if !needs_restart(old, finished, &s.tokens) {
                continue;
            }
            if i < self.handles.len() {
                let slot = self.handles.remove(i);
                self.stop_slot(i, slot).await;
            }
            let (tx, stop) = watch::channel(false);
            let parked = Arc::new(AtomicBool::new(false));
            let h = spawn_shard(
                i,
                s.tokens.clone(),
                self.shared.clone(),
                self.budget.clone(),
                self.cfg.clone(),
                self.capacity,
                self.metrics.clone(),
                stop,
                parked.clone(),
            );
            let slot = (s.tokens.clone(), h, tx, parked);
            if i < self.handles.len() {
                self.handles.insert(i, slot);
            } else {
                self.handles.push(slot);
            }
        }
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

    /// Signal every shard to stop and detach: the tasks drain and send their own `Down` (or exit
    /// silently if never connected) without this call waiting for them. Non-async by design —
    /// dropping the `Supervisor` itself (e.g. on process shutdown) has the same effect, since
    /// dropping `self.handles` drops every `watch::Sender`, closing each shard's stop channel.
    pub fn abort_all(&mut self) {
        for (_, _, tx, _) in self.handles.drain(..) {
            let _ = tx.send(true);
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

    #[test]
    fn session_reason_prefers_stopped_over_the_transport_result() {
        assert_eq!(session_reason(true, &Ok(())), "stopped");
        assert_eq!(session_reason(false, &Ok(())), "closed");
        assert_eq!(
            session_reason(true, &Err(pm_api::ApiError::Timeout("connect"))),
            "stopped",
            "a deliberate stop is not a transport failure"
        );
        assert_eq!(
            session_reason(false, &Err(pm_api::ApiError::Timeout("connect"))),
            "timed out: connect"
        );
    }
}
