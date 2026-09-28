//! Raw-hour retention with the compaction guard (spec 5b §4.3, §9.5). `plan` is pure, `housekeep`
//! applies it to the capture folder, and `guard_task` keeps the capture thread's view of
//! compact_hours fresh and records the hours deleted before they were compacted. Capture never
//! stalls on the database or on a stopped compactor: with no fresh view, retention is today's rule
//! (age plus cap), and every deletion made blind is marked lost (a no-op for hours already done:
//! ON CONFLICT DO NOTHING). Before `guard_task`'s first read (scanner start) only the cap applies:
//! an alive compactor may still need hours past `retention_hours` (final review M2).
use crate::config::CaptureCfg;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::Store;
use tape::files::HourFile;
use tape::time::HOUR_MS;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// The guard's view is trusted for this long after a successful read.
pub const VIEW_FRESH_MS: i64 = 120_000;
/// How often `guard_task` re-reads compact_hours (and retries lost marks that failed).
pub const REFRESH_MS: u64 = 30_000;
/// How far back `guard_task` reads (24 h retention and a 30 GB cap keep far less on disk).
pub const LOOKBACK_MS: i64 = 14 * 24 * HOUR_MS;
/// A compactor silent this long is treated as stopped: retention returns to today's rule (§9.5).
pub const COMPACTOR_STALE_MS: i64 = 6 * HOUR_MS;

/// What retention knows about compaction when it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// `guard_task` hasn't read compact_hours yet (scanner start): only the cap deletes, so a
    /// warm start never age-deletes hours an alive compactor still needs.
    Unread,
    /// Read, but no fresh view: the compactor is silent or absent, or the database is down (or
    /// was at the first read). Today's rule: age plus cap.
    Blind,
    /// The hours compact_hours records done or lost, read within `VIEW_FRESH_MS` from an alive
    /// compactor: age deletes only these.
    Compacted(BTreeSet<i64>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub path: PathBuf,
    pub hour_ms: i64,
    /// Deleted to get under max_capture_gb (else: past retention_hours).
    pub by_cap: bool,
    /// Not known to be compacted (done or lost): gets a lost mark.
    pub uncompacted: bool,
}

/// Which hour files to delete, oldest first. Age: past `retention_hours` and compacted (any age
/// when the view is `Blind`, the fallback; none while `Unread`). Cap: while the capture folder
/// (hours + universe records) is over `max_bytes`, the oldest remaining hour, whatever its status.
/// `keep` (the file being written) is never deleted.
pub fn plan(
    hours: &[HourFile],
    universe_bytes: u64,
    now_ms: i64,
    retention_hours: i64,
    max_bytes: u64,
    keep: Option<&Path>,
    view: &View,
) -> Vec<Deletion> {
    let cutoff = now_ms.saturating_sub(retention_hours.saturating_mul(HOUR_MS));
    let is_done = |h: i64| matches!(view, View::Compacted(s) if s.contains(&h));
    let age_ok = |h: i64| match view {
        View::Unread => false,
        View::Blind => true,
        View::Compacted(s) => s.contains(&h),
    };
    let mut sorted: Vec<&HourFile> = hours.iter().collect();
    sorted.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut total = sorted
        .iter()
        .map(|h| h.bytes)
        .sum::<u64>()
        .saturating_add(universe_bytes);
    let mut out: Vec<Deletion> = Vec::new();
    for h in &sorted {
        if keep == Some(h.path.as_path()) {
            continue;
        }
        let old = h.start_ms.saturating_add(HOUR_MS) <= cutoff;
        if old && age_ok(h.start_ms) {
            total = total.saturating_sub(h.bytes);
            out.push(Deletion {
                path: h.path.clone(),
                hour_ms: h.start_ms,
                by_cap: false,
                uncompacted: !is_done(h.start_ms),
            });
        }
    }
    for h in &sorted {
        if total <= max_bytes {
            break;
        }
        if keep == Some(h.path.as_path()) || out.iter().any(|d| d.path == h.path) {
            continue;
        }
        total = total.saturating_sub(h.bytes);
        out.push(Deletion {
            path: h.path.clone(),
            hour_ms: h.start_ms,
            by_cap: true,
            uncompacted: !is_done(h.start_ms),
        });
    }
    out
}

/// The rollback rule (spec §9.5, owner ruling): wait for compaction only while the compactor has
/// reported within `COMPACTOR_STALE_MS`. No status row (compact not deployed yet) or a stale one
/// means today's rule.
pub fn compactor_alive(updated_ms: Option<i64>, now_ms: i64) -> bool {
    updated_ms.is_some_and(|u| now_ms.saturating_sub(u) <= COMPACTOR_STALE_MS)
}

/// One housekeeping pass over the capture folder: apply `plan`, send a lost mark for every hour
/// deleted before it was known compacted (logging the ones the cap forced), then delete universe
/// records no remaining hour can name. Returns the number of files deleted.
pub fn housekeep(
    dir: &Path,
    now_ms: i64,
    cfg: &CaptureCfg,
    view: &View,
    lost: &UnboundedSender<i64>,
    keep: Option<&Path>,
) -> u64 {
    let listing = tape::files::listing(dir);
    let universe_bytes: u64 = listing.universes.iter().map(|u| u.bytes).sum();
    let budget = cfg.max_capture_gb.saturating_mul(1_000_000_000);
    let mut n = 0u64;
    for d in plan(
        &listing.hours,
        universe_bytes,
        now_ms,
        cfg.retention_hours,
        budget,
        keep,
        view,
    ) {
        if std::fs::remove_file(&d.path).is_ok() {
            n += 1;
            if d.uncompacted {
                if let Err(e) = lost.send(d.hour_ms) {
                    eprintln!(
                        "capture: hour {} deleted, its lost mark not sent ({e}); the compactor labels it itself",
                        tape::time::hour_label(d.hour_ms)
                    );
                }
                if d.by_cap {
                    eprintln!(
                        "capture: hour {} deleted before compaction (disk cap)",
                        tape::time::hour_label(d.hour_ms)
                    );
                }
            }
        }
    }
    for p in crate::capture::stale_universe_files(dir) {
        if std::fs::remove_file(p).is_ok() {
            n += 1;
        }
    }
    n
}

#[derive(Debug, Default)]
enum Seen {
    #[default]
    Unread,
    Blind,
    Read(i64, BTreeSet<i64>),
}

/// The capture thread's view of compact_hours (done or lost hours) and when it was read.
#[derive(Debug, Default)]
pub struct GuardView {
    inner: Mutex<Seen>,
}

impl GuardView {
    pub fn set(&self, fetched_ms: i64, hours: BTreeSet<i64>) {
        if let Ok(mut g) = self.inner.lock() {
            *g = Seen::Read(fetched_ms, hours);
        }
    }

    /// Forget the view: retention falls back to today's rule until the next `set`.
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            *g = Seen::Blind;
        }
    }

    /// A read failed (the database is down): a view never read becomes `Blind` (today's rule);
    /// a view already read stays until it goes stale.
    pub fn read_failed(&self) {
        if let Ok(mut g) = self.inner.lock()
            && matches!(*g, Seen::Unread)
        {
            *g = Seen::Blind;
        }
    }

    /// `Compacted` while read within `VIEW_FRESH_MS`; `Unread` before the first read; `Blind`
    /// (fall back) when cleared, stale, or after a failed first read. A poisoned lock is `Blind`.
    pub fn get(&self, now_ms: i64) -> View {
        let Ok(g) = self.inner.lock() else {
            return View::Blind;
        };
        match &*g {
            Seen::Unread => View::Unread,
            Seen::Blind => View::Blind,
            Seen::Read(t, s) if now_ms.abs_diff(*t) <= VIEW_FRESH_MS.unsigned_abs() => {
                View::Compacted(s.clone())
            }
            Seen::Read(..) => View::Blind,
        }
    }
}

async fn flush_lost(store: &Store, pending: &mut BTreeSet<i64>) {
    if pending.is_empty() {
        return;
    }
    let hours: Vec<i64> = pending.iter().copied().collect();
    if store.mark_hours_lost(&hours, crate::now_ms()).await.is_ok() {
        pending.clear();
    }
}

/// Records lost hours the moment the capture thread reports them (so the compactor can't pass an
/// hour before its mark, spec §4.3 / D5), and every 30 s retries failed marks and refreshes the
/// view: set while the compactor is alive, cleared (today's rule) while it is silent. Ends when the
/// capture thread drops its sender, after one last try at the marks still pending (the shutdown
/// awaits it after joining the capture thread).
pub async fn guard_task(store: Store, view: Arc<GuardView>, mut lost: UnboundedReceiver<i64>) {
    let mut every = tokio::time::interval(Duration::from_millis(REFRESH_MS));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pending: BTreeSet<i64> = BTreeSet::new();
    let (mut failing, mut silent) = (false, false);
    loop {
        tokio::select! {
            _ = every.tick() => {}
            h = lost.recv() => match h {
                Some(h) => {
                    pending.insert(h);
                    while let Ok(more) = lost.try_recv() {
                        pending.insert(more);
                    }
                    flush_lost(&store, &mut pending).await;
                    continue;
                }
                None => {
                    flush_lost(&store, &mut pending).await;
                    if !pending.is_empty() {
                        eprintln!(
                            "capture: {} lost mark(s) not recorded at shutdown (the database write failed); the compactor labels those hours itself",
                            pending.len()
                        );
                    }
                    return;
                }
            },
        }
        flush_lost(&store, &mut pending).await;
        let now = crate::now_ms();
        let status = store.compact_status().await;
        let hours = store
            .compact_hour_statuses(now.saturating_sub(LOOKBACK_MS), now.saturating_add(HOUR_MS))
            .await;
        match (status, hours) {
            (Ok(status), Ok(rows)) => {
                if failing {
                    eprintln!("capture: compact_hours readable again");
                    failing = false;
                }
                if compactor_alive(status.map(|s| s.updated_ms), now) {
                    view.set(now, rows.into_iter().map(|(h, _)| h).collect());
                    if silent {
                        eprintln!("capture: compactor reporting; raw hours wait for compaction");
                        silent = false;
                    }
                } else {
                    view.clear();
                    if !silent {
                        eprintln!(
                            "capture: compactor silent (no compact_status within 6 h); retention falls back to age + cap"
                        );
                        silent = true;
                    }
                }
            }
            (Err(e), _) | (_, Err(e)) => {
                view.read_failed();
                if !failing {
                    eprintln!(
                        "capture: compact_hours unreadable ({e}); retention falls back to age + cap"
                    );
                    failing = true;
                }
            }
        }
    }
}
