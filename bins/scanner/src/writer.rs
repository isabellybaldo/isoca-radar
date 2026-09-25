//! Batching store writer (spec §8.2/§10): one transaction per flush interval. A transiently failed
//! write keeps the batch and retries with backoff; receiving never stops, so the engine never blocks
//! on Postgres. A batch Postgres rejects for its DATA (`StoreError::is_data_error`, SQLSTATE 22/23)
//! is quarantined row by row so one poison row can't stall every later flush. Above `max_rows`, the
//! oldest health, then integrity, then observation rows are dropped and counted (episodes never are,
//! except a single episode row Postgres itself rejects).
use crate::metrics::Metrics;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use store::{Batch, EpisodeWrite, HealthRow, IntegrityRow, ObservationRow, Store};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum WriterMsg {
    Episode(EpisodeWrite),
    Observation(ObservationRow),
    Integrity(IntegrityRow),
    Health(HealthRow),
    Flush(oneshot::Sender<()>),
}

#[derive(Debug, Default)]
pub struct Pending {
    batch: Batch,
    ep_index: HashMap<String, usize>,
    max_rows: usize,
    pub dropped: u64,
}

impl Pending {
    pub fn new(max_rows: usize) -> Pending {
        Pending {
            max_rows,
            ..Pending::default()
        }
    }

    pub fn rows(&self) -> usize {
        self.batch.episodes.len()
            + self.batch.observations.len()
            + self.batch.integrity.len()
            + self.batch.health.len()
    }

    fn upsert_episode(&mut self, w: EpisodeWrite, overwrite: bool) {
        match self.ep_index.get(&w.row.episode_id) {
            Some(&i) => {
                if overwrite {
                    self.batch.episodes[i] = w;
                }
            }
            None => {
                self.ep_index
                    .insert(w.row.episode_id.clone(), self.batch.episodes.len());
                self.batch.episodes.push(w);
            }
        }
    }

    pub fn push(&mut self, m: WriterMsg) {
        match m {
            WriterMsg::Episode(w) => self.upsert_episode(w, true),
            WriterMsg::Observation(o) => self.batch.observations.push(o),
            WriterMsg::Integrity(i) => self.batch.integrity.push(i),
            WriterMsg::Health(h) => self.batch.health.push(h),
            WriterMsg::Flush(_) => {}
        }
        self.enforce_cap();
    }

    fn enforce_cap(&mut self) {
        while self.rows() > self.max_rows {
            if !self.batch.health.is_empty() {
                self.batch.health.remove(0);
            } else if !self.batch.integrity.is_empty() {
                self.batch.integrity.remove(0);
            } else if !self.batch.observations.is_empty() {
                self.batch.observations.remove(0);
            } else {
                break; // only episodes left: never dropped
            }
            self.dropped += 1;
        }
    }

    pub fn take(&mut self) -> Batch {
        self.ep_index.clear();
        std::mem::take(&mut self.batch)
    }

    /// Put back a batch whose write failed. It is OLDER than anything pushed since, so newer
    /// episode versions already here win, and its other rows go first.
    pub fn restore(&mut self, older: Batch) {
        let newer = self.take();
        for w in older.episodes {
            self.upsert_episode(w, false);
        }
        for w in newer.episodes {
            self.upsert_episode(w, true);
        }
        // fix ordering: older episode versions were inserted first, newer overwrote them in place
        self.batch.observations = older
            .observations
            .into_iter()
            .chain(newer.observations)
            .collect();
        self.batch.integrity = older.integrity.into_iter().chain(newer.integrity).collect();
        self.batch.health = older.health.into_iter().chain(newer.health).collect();
        self.enforce_cap();
    }
}

pub async fn run_writer(
    store: Store,
    mut rx: mpsc::Receiver<WriterMsg>,
    flush_ms: u64,
    max_rows: usize,
    metrics: Arc<Metrics>,
) {
    let mut pending = Pending::new(max_rows);
    let mut last_dropped: u64 = 0;
    let mut tick = tokio::time::interval(Duration::from_millis(flush_ms.max(10)));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut fails: u32 = 0;
    let mut retry_at = tokio::time::Instant::now();
    loop {
        tokio::select! {
            m = rx.recv() => match m {
                Some(WriterMsg::Flush(done)) => {
                    flush(&store, &mut pending, &mut fails).await;
                    let _ = done.send(());
                }
                Some(other) => pending.push(other),
                None => {
                    for _ in 0..3 {
                        if flush(&store, &mut pending, &mut fails).await {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    return;
                }
            },
            _ = tick.tick() => {
                if pending.rows() > 0 && tokio::time::Instant::now() >= retry_at && !flush(&store, &mut pending, &mut fails).await {
                    let backoff = Duration::from_millis((100_u64 << fails.min(8)).min(30_000));
                    retry_at = tokio::time::Instant::now() + backoff;
                }
            }
        }
        // `pending.dropped` is a running total (cap evictions + quarantine failures): forward
        // only the delta since the last time round, so a restarted count is never double-counted.
        if pending.dropped > last_dropped {
            metrics
                .writer_drops
                .fetch_add(pending.dropped - last_dropped, Ordering::Relaxed);
            last_dropped = pending.dropped;
        }
    }
}

/// Write each row of a data-rejected batch on its own; returns how many rows still failed (dropped).
/// Episodes go first so observations referencing them can still land.
async fn quarantine(store: &Store, b: Batch) -> u64 {
    let mut dropped = 0;
    // Same 10 s bound as the main flush: a stalled row must not freeze the writer (it's dropped).
    let one = |batch: Batch| async move {
        match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&batch)).await {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err("timed out".to_string()),
        }
    };
    for w in b.episodes {
        let id = w.row.episode_id.clone();
        if let Err(e) = one(Batch {
            episodes: vec![w],
            ..Batch::default()
        })
        .await
        {
            eprintln!("writer: dropped episode {id}: {e}");
            dropped += 1;
        }
    }
    for o in b.observations {
        if one(Batch {
            observations: vec![o],
            ..Batch::default()
        })
        .await
        .is_err()
        {
            dropped += 1;
        }
    }
    for i in b.integrity {
        if one(Batch {
            integrity: vec![i],
            ..Batch::default()
        })
        .await
        .is_err()
        {
            dropped += 1;
        }
    }
    for h in b.health {
        if one(Batch {
            health: vec![h],
            ..Batch::default()
        })
        .await
        .is_err()
        {
            dropped += 1;
        }
    }
    dropped
}

/// Write the pending batch once. Transient failure → restore it for a retry; data error →
/// quarantine it row by row. Returns true when nothing is left to retry.
async fn flush(store: &Store, pending: &mut Pending, fails: &mut u32) -> bool {
    if pending.rows() == 0 {
        return true;
    }
    let b = pending.take();
    match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&b)).await {
        Ok(Ok(())) => {
            *fails = 0;
            true
        }
        Ok(Err(e)) if e.is_data_error() => {
            // Postgres rejected the DATA (SQLSTATE 22/23): retrying the same batch can never succeed.
            // Quarantine: write row by row, drop (and log) only the rows that still fail.
            let dropped = quarantine(store, b).await;
            pending.dropped += dropped;
            eprintln!("writer: data error ({e}); quarantined batch, dropped {dropped} rows");
            *fails = 0;
            true
        }
        Ok(Err(e)) => {
            eprintln!(
                "writer: batch failed ({} rows): {e}",
                b.episodes.len() + b.observations.len() + b.integrity.len() + b.health.len()
            );
            pending.restore(b);
            *fails += 1;
            false
        }
        Err(_) => {
            eprintln!("writer: batch timed out");
            pending.restore(b);
            *fails += 1;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use store::{EpisodeRow, IntegrityRow};

    fn ep(id: &str, closed: Option<i64>) -> EpisodeWrite {
        let row = EpisodeRow {
            episode_id: id.into(),
            run_id: 1,
            source: "live".into(),
            kind: "no_basket".into(),
            realization: "converter".into(),
            event_id: "e".into(),
            event_slug: None,
            event_title: None,
            neg_risk_market_id: None,
            pair_id: None,
            category: None,
            n_outcomes: 2,
            n_legs: 2,
            legs_dropped: 0,
            augmented: false,
            incomplete_set: false,
            uncovered_legs: 0,
            opened_server_ms: 1,
            closed_server_ms: closed,
            duration_ms: None,
            censor_left: false,
            censor_right: false,
            censor_gap: false,
            detect_lag_ms: None,
            n_updates: 1,
            peak_server_ms: 1,
            peak_edge_top: 0.into(),
            peak_edge_top_gross: 0.into(),
            peak_max_size: 0.into(),
            peak_cost_usd: 0.into(),
            peak_payout_usd: 0.into(),
            peak_fees_usd: 0.into(),
            peak_pnl_net_usd: 0.into(),
            peak_pnl_gross_usd: 0.into(),
            peak_profit_per_dollar: None,
            pnl_net_budget100_usd: None,
            bottleneck_leg_idx: None,
            bottleneck_notional_usd: None,
            below_min_size: None,
            max_seconds_delay: None,
            days_to_end: None,
            annualized_return: None,
            post_end: None,
            engine_suspect: false,
            stale_transport: false,
            open_lag_ms: None,
            oracle_downgraded: None,
            scanner_version: "v".into(),
            config_hash: "c".into(),
        };
        EpisodeWrite { row, legs: vec![] }
    }

    fn integ(ts: i64) -> WriterMsg {
        WriterMsg::Integrity(IntegrityRow {
            run_id: 1,
            ts_ms: ts,
            kind: "drop".into(),
            shard: None,
            token: None,
            event_id: None,
            detail: None,
        })
    }

    #[test]
    fn keeps_only_the_latest_version_of_an_episode() {
        let mut p = Pending::new(100);
        p.push(WriterMsg::Episode(ep("a", None)));
        p.push(WriterMsg::Episode(ep("b", None)));
        p.push(WriterMsg::Episode(ep("a", Some(9))));
        let b = p.take();
        assert_eq!(b.episodes.len(), 2);
        assert_eq!(b.episodes[0].row.closed_server_ms, Some(9));
        assert_eq!(p.rows(), 0);
    }

    #[test]
    fn restore_after_a_failed_write_lets_newer_versions_win() {
        let mut p = Pending::new(100);
        p.push(WriterMsg::Episode(ep("a", None)));
        let failed = p.take();
        p.push(WriterMsg::Episode(ep("a", Some(5)))); // arrived while the write was failing
        p.restore(failed);
        let b = p.take();
        assert_eq!(b.episodes.len(), 1);
        assert_eq!(b.episodes[0].row.closed_server_ms, Some(5));
    }

    #[test]
    fn over_the_cap_drops_non_episode_rows_first() {
        let mut p = Pending::new(3);
        p.push(WriterMsg::Episode(ep("a", None)));
        for t in 0..5 {
            p.push(integ(t));
        }
        assert_eq!(p.rows(), 3);
        assert_eq!(p.dropped, 3);
        let b = p.take();
        assert_eq!(b.episodes.len(), 1);
        assert_eq!(
            b.integrity.iter().map(|i| i.ts_ms).collect::<Vec<_>>(),
            vec![3, 4],
            "oldest dropped"
        );
    }
}
