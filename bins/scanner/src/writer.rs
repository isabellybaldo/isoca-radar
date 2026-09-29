//! Batching store writer (spec §8.2/§10): one transaction per flush interval. A transiently failed
//! write keeps the batch and retries with backoff; receiving never stops, so the engine never blocks
//! on Postgres. A batch Postgres rejects for its DATA (`StoreError::is_data_error`, SQLSTATE 22/23)
//! is quarantined row by row so one poison row can't stall every later flush. Above `max_rows`, the
//! oldest health, then integrity, then observation rows are dropped and counted (episodes and
//! paper rows never are, except a single row Postgres itself rejects).
//! A paper reset (Phase 7a §4.2) is a group of its own: written alone after everything queued before it, retried whole, and on a data error dropped whole (never quarantined row by row: the epoch row and `paper_state` must never commit apart).
use crate::metrics::Metrics;
use std::collections::{HashMap, VecDeque};
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
    /// Paper-trader rows (S3c). Only the batch's `paper_*` fields are used.
    Paper(Box<Batch>),
    /// Phase 7a §4.2: one epoch's rows (close rows, the paper_epochs row, marks, paper_state),
    /// written alone in one transaction after everything queued before it.
    PaperReset(Box<Batch>),
    Flush(oneshot::Sender<()>),
}

/// What a flush writes next, in order (spec 7a §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// The open rows (merged, deduplicated and capped as always).
    Open,
    /// Rows that were open when a reset arrived: written before it, quarantined if rejected.
    Sealed,
    /// A reset: written alone; a data error drops it whole.
    Reset,
}

#[derive(Debug)]
pub struct Group {
    pub kind: GroupKind,
    pub batch: Batch,
}

/// Rows waiting for the next flush. Episodes are deduplicated by id (latest version wins). The
/// other rows are FIFO queues, so the cap evicts the oldest in O(1). Paper rows are kept in
/// arrival order and, like episodes, are never evicted: a gap in `paper_ledger` would break the
/// equity identity.
#[derive(Debug, Default)]
pub struct Pending {
    episodes: Vec<EpisodeWrite>,
    observations: VecDeque<ObservationRow>,
    integrity: VecDeque<IntegrityRow>,
    health: VecDeque<HealthRow>,
    /// Only the `paper_*` fields are ever filled.
    paper: Batch,
    /// Groups ahead of the open rows, oldest first: rows sealed by a reset, then the reset.
    sealed: VecDeque<Group>,
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

    /// Every row waiting, sealed groups included (what the flush tick checks).
    pub fn rows(&self) -> usize {
        self.open_rows() + self.sealed.iter().map(|g| g.batch.rows()).sum::<usize>()
    }

    /// The open rows only (what the cap counts: sealed groups are never evicted).
    pub fn open_rows(&self) -> usize {
        self.episodes.len()
            + self.observations.len()
            + self.integrity.len()
            + self.health.len()
            + self.paper.paper_rows()
    }

    fn upsert_episode(&mut self, w: EpisodeWrite, overwrite: bool) {
        match self.ep_index.get(&w.row.episode_id) {
            Some(&i) => {
                if overwrite {
                    self.episodes[i] = w;
                }
            }
            None => {
                self.ep_index
                    .insert(w.row.episode_id.clone(), self.episodes.len());
                self.episodes.push(w);
            }
        }
    }

    pub fn push(&mut self, m: WriterMsg) {
        match m {
            WriterMsg::Episode(w) => self.upsert_episode(w, true),
            WriterMsg::Observation(o) => self.observations.push_back(o),
            WriterMsg::Integrity(i) => self.integrity.push_back(i),
            WriterMsg::Health(h) => self.health.push_back(h),
            WriterMsg::Paper(b) => self.paper.extend_paper(*b),
            WriterMsg::PaperReset(b) => {
                let open = self.take();
                if open.rows() > 0 {
                    self.sealed.push_back(Group {
                        kind: GroupKind::Sealed,
                        batch: open,
                    });
                }
                self.sealed.push_back(Group {
                    kind: GroupKind::Reset,
                    batch: *b,
                });
            }
            WriterMsg::Flush(_) => {}
        }
        self.enforce_cap();
    }

    /// Over the cap, drop the oldest health, then integrity, then observation rows (episodes and
    /// paper rows never), counting each.
    fn enforce_cap(&mut self) {
        while self.open_rows() > self.max_rows {
            let evicted = self.health.pop_front().is_some()
                || self.integrity.pop_front().is_some()
                || self.observations.pop_front().is_some();
            if !evicted {
                break; // only episodes and paper rows left: never dropped
            }
            self.dropped += 1;
        }
    }

    pub fn take(&mut self) -> Batch {
        self.ep_index.clear();
        let mut b = std::mem::take(&mut self.paper);
        b.episodes = std::mem::take(&mut self.episodes);
        b.observations = std::mem::take(&mut self.observations).into();
        b.integrity = std::mem::take(&mut self.integrity).into();
        b.health = std::mem::take(&mut self.health).into();
        b
    }

    /// Put back a batch whose write failed. It is OLDER than anything pushed since, so newer
    /// episode versions already here win, and its other rows go first.
    pub fn restore(&mut self, mut older: Batch) {
        let mut newer = self.take();
        let mut paper = older.take_paper();
        paper.extend_paper(newer.take_paper());
        for w in older.episodes {
            self.upsert_episode(w, false);
        }
        for w in newer.episodes {
            self.upsert_episode(w, true);
        }
        self.observations = older
            .observations
            .into_iter()
            .chain(newer.observations)
            .collect();
        self.integrity = older.integrity.into_iter().chain(newer.integrity).collect();
        self.health = older.health.into_iter().chain(newer.health).collect();
        self.paper = paper;
        self.enforce_cap();
    }

    /// The next group to write alone: a sealed group first, else the open rows.
    pub fn next_group(&mut self) -> Option<Group> {
        if let Some(g) = self.sealed.pop_front() {
            return Some(g);
        }
        if self.open_rows() == 0 {
            return None;
        }
        Some(Group {
            kind: GroupKind::Open,
            batch: self.take(),
        })
    }

    /// A group's write failed. Transient: it goes back, still ahead of everything newer (open
    /// rows through `restore`, sealed groups and resets to the front). Data error: a reset is
    /// dropped whole and counted (spec 7a §4.2, never split); other rows come back for the
    /// row-by-row quarantine.
    pub fn failed(&mut self, g: Group, transient: bool) -> Option<Batch> {
        match (g.kind, transient) {
            (GroupKind::Open, true) => {
                self.restore(g.batch);
                None
            }
            (_, true) => {
                self.sealed.push_front(g);
                None
            }
            (GroupKind::Reset, false) => {
                self.dropped += u64::try_from(g.batch.rows()).unwrap_or(u64::MAX);
                None
            }
            (_, false) => Some(g.batch),
        }
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

/// Write each row of a data-rejected batch on its own; returns how many rows still failed
/// (dropped). `Batch::into_singles` puts episodes before their observations and paper accounts
/// before the paper rows that reference them.
async fn quarantine(store: &Store, b: Batch) -> u64 {
    let mut dropped = 0;
    for single in b.into_singles() {
        // Same 10 s bound as the main flush: a stalled row must not freeze the writer (it's dropped).
        let r =
            match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&single)).await {
                Ok(r) => r.map_err(|e| e.to_string()),
                Err(_) => Err("timed out".to_string()),
            };
        if let Err(e) = r {
            if let Some(w) = single.episodes.first() {
                eprintln!("writer: dropped episode {}: {e}", w.row.episode_id);
            } else if let Some(l) = single.paper_ledger.first() {
                eprintln!(
                    "writer: dropped paper ledger row {}#{}: {e}",
                    l.account_id, l.seq
                );
            }
            dropped += 1;
        }
    }
    dropped
}

/// A reset group's epoch and request for its error line, from its `paper_epochs` row: the
/// operator then knows which request a restart re-applies (it stays pending: its id is above
/// every committed `paper_epochs.request_id`).
fn reset_label(b: &Batch) -> String {
    if b.paper_epochs.is_empty() {
        return "no epoch row".to_string();
    }
    b.paper_epochs
        .iter()
        .map(|e| match e.request_id {
            Some(r) => format!("epoch {} (request {r})", e.epoch),
            None => format!("epoch {} ({})", e.epoch, e.cause),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Write every waiting group, each in its own transaction, in order. Transient failure → the
/// group goes back for a retry (returns false); data error → quarantine row by row, except a
/// reset, which is dropped whole and logged at error level. Returns true when nothing is left.
async fn flush(store: &Store, pending: &mut Pending, fails: &mut u32) -> bool {
    while let Some(g) = pending.next_group() {
        let rows = g.batch.rows();
        let label = (g.kind == GroupKind::Reset).then(|| reset_label(&g.batch));
        match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&g.batch)).await {
            Ok(Ok(())) => *fails = 0,
            Ok(Err(e)) if e.is_data_error() => {
                // Postgres rejected the DATA (SQLSTATE 22/23): retrying the same rows can never succeed.
                *fails = 0;
                match pending.failed(g, false) {
                    Some(b) => {
                        let dropped = quarantine(store, b).await;
                        pending.dropped += dropped;
                        eprintln!(
                            "writer: data error ({e}); quarantined batch, dropped {dropped} rows"
                        );
                    }
                    None => eprintln!(
                        "writer: ERROR: paper reset {} rejected ({e}); dropped whole ({rows} rows), \
                         never split; its request stays pending and the next scanner restart \
                         re-applies it",
                        label.as_deref().unwrap_or("?")
                    ),
                }
            }
            Ok(Err(e)) => {
                eprintln!("writer: batch failed ({rows} rows): {e}");
                let _ = pending.failed(g, true);
                *fails += 1;
                return false;
            }
            Err(_) => {
                eprintln!("writer: batch timed out");
                let _ = pending.failed(g, true);
                *fails += 1;
                return false;
            }
        }
    }
    true
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

    fn paper_ledger(seq: i64) -> WriterMsg {
        WriterMsg::Paper(Box::new(Batch {
            paper_ledger: vec![store::PaperLedgerRow {
                run_id: 1,
                account_id: "a".into(),
                seq,
                t_ms: seq,
                kind: "mark".into(),
                episode_id: None,
                market_id: None,
                yes_token: None,
                outcome: None,
                qty: 0.into(),
                price: None,
                cash_delta: 0.into(),
                fees: 0.into(),
                realized_delta: 0.into(),
                cash_after: 1000.into(),
                equity: Some(1000.into()),
            }],
            ..Batch::default()
        }))
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

    #[test]
    fn evicting_a_large_backlog_is_fast_and_keeps_the_newest_rows() {
        // 60,000 evictions from a 30,000-row queue: Vec::remove(0) shifted ~1.8e9 rows here.
        let started = std::time::Instant::now();
        let mut p = Pending::new(30_000);
        for t in 0..90_000 {
            p.push(integ(t));
        }
        assert_eq!((p.rows(), p.dropped), (30_000, 60_000));
        let b = p.take();
        assert_eq!(b.integrity.first().map(|i| i.ts_ms), Some(60_000));
        assert_eq!(b.integrity.last().map(|i| i.ts_ms), Some(89_999));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "eviction must be O(1) per row: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn paper_rows_are_never_evicted_by_the_cap() {
        let mut p = Pending::new(3);
        for s in 1..=5 {
            p.push(paper_ledger(s));
        }
        for t in 0..3 {
            p.push(integ(t));
        }
        assert_eq!(p.rows(), 5, "only the integrity rows could go");
        assert_eq!(p.dropped, 3);
        let b = p.take();
        assert_eq!(
            b.paper_ledger.iter().map(|l| l.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(b.integrity.is_empty());
        assert_eq!(p.rows(), 0);
    }

    #[test]
    fn restore_keeps_older_paper_rows_first() {
        let mut p = Pending::new(100);
        p.push(paper_ledger(1));
        let failed = p.take();
        p.push(paper_ledger(2)); // arrived while the write was failing
        p.restore(failed);
        assert_eq!(
            p.take()
                .paper_ledger
                .iter()
                .map(|l| l.seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    fn reset_msg(seq: i64) -> (WriterMsg, Batch) {
        let mut b = match paper_ledger(seq) {
            WriterMsg::Paper(b) => *b,
            _ => unreachable!(),
        };
        b.paper_epochs.push(store::PaperEpochRow {
            epoch: 2,
            start_ms: seq,
            cause: "reset".into(),
            request_id: Some(7),
            run_id: 1,
            policies_sha256: "s".into(),
        });
        (WriterMsg::PaperReset(Box::new(b.clone())), b)
    }

    fn seqs(b: &Batch) -> Vec<i64> {
        b.paper_ledger.iter().map(|l| l.seq).collect()
    }

    #[test]
    fn a_reset_is_written_alone_after_the_rows_queued_before_it() {
        let mut p = Pending::new(100);
        p.push(WriterMsg::Episode(ep("poison", None)));
        p.push(paper_ledger(1));
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        p.push(paper_ledger(3));
        let first = p.next_group().unwrap();
        assert_eq!(first.kind, GroupKind::Sealed);
        assert_eq!(
            (first.batch.episodes.len(), seqs(&first.batch)),
            (1, vec![1])
        );
        // the pending batch is rejected for its poison episode: it goes to the quarantine
        assert!(p.failed(first, false).is_some());
        let second = p.next_group().unwrap();
        assert_eq!(second.kind, GroupKind::Reset);
        assert_eq!(second.batch, reset, "the reset is written whole, alone");
        let third = p.next_group().unwrap();
        assert_eq!((third.kind, seqs(&third.batch)), (GroupKind::Open, vec![3]));
        assert!(p.next_group().is_none());
    }

    #[test]
    fn a_dropped_reset_names_its_epoch_and_request() {
        let (_, reset) = reset_msg(2);
        assert_eq!(reset_label(&reset), "epoch 2 (request 7)");
        let mut policies = reset.clone();
        policies.paper_epochs[0].request_id = None;
        policies.paper_epochs[0].cause = "policies".into();
        assert_eq!(reset_label(&policies), "epoch 2 (policies)");
        assert_eq!(reset_label(&Batch::default()), "no epoch row");
    }

    #[test]
    fn a_rejected_reset_is_dropped_whole_never_split() {
        let mut p = Pending::new(100);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        let g = p.next_group().unwrap();
        assert!(
            p.failed(g, false).is_none(),
            "never handed to the row-by-row quarantine"
        );
        assert_eq!(p.dropped, u64::try_from(reset.rows()).unwrap());
        assert!(p.next_group().is_none());
    }

    #[test]
    fn a_transient_failure_keeps_the_reset_ahead_of_newer_rows() {
        let mut p = Pending::new(100);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        let g = p.next_group().unwrap();
        p.push(paper_ledger(9)); // a new-epoch trade arrives while the write is failing
        assert!(p.failed(g, true).is_none());
        let again = p.next_group().unwrap();
        assert_eq!((again.kind, again.batch), (GroupKind::Reset, reset));
        let next = p.next_group().unwrap();
        assert_eq!((next.kind, seqs(&next.batch)), (GroupKind::Open, vec![9]));
    }

    #[test]
    fn a_failed_open_batch_retried_during_a_reset_still_goes_first() {
        let mut p = Pending::new(100);
        p.push(paper_ledger(1));
        let g = p.next_group().unwrap();
        assert_eq!(g.kind, GroupKind::Open);
        assert!(p.failed(g, true).is_none()); // back into the open rows
        let (msg, _) = reset_msg(2);
        p.push(msg);
        let first = p.next_group().unwrap();
        assert_eq!(
            (first.kind, seqs(&first.batch)),
            (GroupKind::Sealed, vec![1])
        );
        assert_eq!(p.next_group().unwrap().kind, GroupKind::Reset);
    }

    #[test]
    fn the_cap_never_evicts_a_reset() {
        let mut p = Pending::new(2);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        for t in 0..4 {
            p.push(integ(t));
        }
        assert_eq!(p.open_rows(), 2, "only the open integrity rows are capped");
        assert_eq!(p.next_group().unwrap().batch, reset);
    }

    #[test]
    fn restore_keeps_paper_epochs() {
        let mut p = Pending::new(100);
        let (_, epoch) = reset_msg(1);
        p.push(WriterMsg::Paper(Box::new(epoch.clone())));
        let failed = p.take();
        p.push(paper_ledger(2)); // arrived while the write was failing
        p.restore(failed);
        let b = p.take();
        assert_eq!(b.paper_epochs, epoch.paper_epochs);
        assert_eq!(seqs(&b), vec![1, 2]);
    }
}
