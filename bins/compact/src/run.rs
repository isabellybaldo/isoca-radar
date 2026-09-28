//! The service (spec 5b §4): compact every ready raw hour in order, record it in compact_hours,
//! merge whole days, keep the archive under its budget, and report compact_status. A database or
//! disk hiccup never stops it: the error is reported and the next poll resumes from the last
//! checkpoint.
//!
//! Startup: the newest checkpoint is loaded. With no checkpoint file at all (first deploy) the
//! service starts at 00:00 of the first raw hour's day, so one reducer numbers the whole day's
//! tokens: the hours before the first raw file are written at once as `lost` (when compact_hours
//! says so) or `scanner_stopped`. It refuses a cold start into a day that already has published
//! hours or daily files (a fresh reducer would renumber tokens mid-day), and never starts cold
//! when checkpoint files exist but none is readable (`state::load_newest`).
use crate::archive;
use crate::config::Config;
use crate::hour::{HourJob, HourReport, Source, process_hour};
use crate::pq::Table;
use crate::reduce::Reducer;
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use crate::state::{self, Checkpoint};
use crate::windows::{query_range, windows_for_hour};
use anyhow::Context;
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use store::{CompactHourRow, CompactStatusRow, Store};
use tape::files::{HourFile, Listing};
use tape::time::{HOUR_MS, day_name, day_of, hour_label};

pub const POLL_SECS: u64 = 60;
pub const STATEMENT_TIMEOUT_MS: u32 = 10_000;
/// A ready hour with no file ever listed and no lost mark waits this long before it is taken as
/// `scanner_stopped` (a just-deleted hour's mark needs time to land; ruling R9).
pub const MISSING_GRACE_MS: i64 = 300_000;
/// `<archive>/_state/.lock`: held by `compact run` for its whole life.
pub const LOCK_FILE: &str = ".lock";

/// Hour H is ready once the scanner has started an hour ≥ H + 2 h: H + 1 is closed, so every
/// l2 window that reaches into H has its episode row (spec §4.2).
pub fn ready(next_hour: i64, hours: &[HourFile]) -> bool {
    hours.iter().any(|h| h.start_ms >= next_hour + 2 * HOUR_MS)
}

pub fn row_of(r: &HourReport, now_ms: i64) -> anyhow::Result<CompactHourRow> {
    Ok(CompactHourRow {
        hour_ms: r.hour_ms,
        status: r.status.clone(),
        source_file: r.source_file.clone(),
        source_bytes: r.source_bytes.map(i64::try_from).transpose()?,
        source_sha256: r.source_sha256.clone(),
        rows: Some(serde_json::to_string(&r.rows)?),
        done_ms: now_ms,
    })
}

/// One `compact run` per archive: an exclusive lock on `_state/.lock`, released when the returned
/// file is dropped (or the process dies). A second `run` gets a clear error.
pub fn lock_archive(archive: &Path) -> anyhow::Result<File> {
    let dir = archive.join(state::STATE_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let p = dir.join(LOCK_FILE);
    let f = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&p)
        .with_context(|| format!("open {}", p.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!(
            "refusing to start: another `compact run` holds {} (one compactor per archive)",
            p.display()
        ),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("lock {}", p.display()))
        }
    }
}

/// A day already written to: an hour's sidecar, a daily file or the manifest.
fn published(day: &Path) -> bool {
    archive::merged(day)
        || (0..24u32).any(|hh| archive::sidecar_path(day, hh).exists())
        || [
            TokenRow::NAME,
            TobRow::NAME,
            DepthRow::NAME,
            L2Row::NAME,
            GapRow::NAME,
        ]
        .iter()
        .any(|t| archive::daily_path(day, t).exists())
}

pub struct Service {
    cfg: Config,
    cfg_sha: String,
    raw: PathBuf,
    archive: PathBuf,
    /// None until loaded, and again after any hour error (never reuse a reducer left mid-hour).
    red: Option<Reducer>,
    next_hour: Option<i64>,
    pending_row: Option<CompactHourRow>,
    /// Hour and database errors of the last step (set by `poll`; brief: the caller).
    pub last_error: Option<String>,
    /// What the last `recover` could not finish (kept until the next load).
    recover_error: Option<String>,
    /// Merge failures and a stuck budget from the last `maintain`.
    archive_error: Option<String>,
    lag_alarm: bool,
    /// Hour files seen in any listing (≥ next_hour): a seen hour that is gone was deleted.
    seen: BTreeSet<i64>,
    /// (hour, when it was first seen ready with no file and no mark).
    missing_since: Option<(i64, i64)>,
    /// The day this process started cold (from its 00:00): reloading after an hour error may
    /// start it cold again, over the hours this process published itself.
    cold_day: Option<i64>,
    /// The hour of the newest durable checkpoint (loaded or saved by this process): a day merges
    /// only once its hour 23 is covered (`archive::may_merge`).
    checkpoint_ms: Option<i64>,
    /// The first raw hour at a cold start: the hours before it were never seen by the service,
    /// so they are `lost` or `scanner_stopped` at once (no grace).
    first_file: Option<i64>,
}

impl Service {
    pub fn new(cfg: Config, cfg_sha: String) -> Service {
        Service {
            raw: PathBuf::from(&cfg.raw_dir),
            archive: PathBuf::from(&cfg.archive_dir),
            cfg,
            cfg_sha,
            red: None,
            next_hour: None,
            pending_row: None,
            last_error: None,
            recover_error: None,
            archive_error: None,
            lag_alarm: false,
            seen: BTreeSet::new(),
            missing_since: None,
            cold_day: None,
            first_file: None,
            checkpoint_ms: None,
        }
    }

    pub fn next_hour(&self) -> Option<i64> {
        self.next_hour
    }

    /// A reducer is loaded (from a checkpoint or a cold start); false after any hour error.
    pub fn loaded(&self) -> bool {
        self.red.is_some()
    }

    /// Load the newest checkpoint (or start cold), finishing any merge a crash interrupted.
    /// Ok(false): no checkpoint and no raw hour yet, nothing to start from.
    fn load(&mut self, listing: &Listing, now_ms: i64) -> anyhow::Result<bool> {
        std::fs::create_dir_all(&self.archive)
            .with_context(|| format!("create {}", self.archive.display()))?;
        // Err when checkpoint files exist but none is readable: never a cold start over them.
        let cp = state::load_newest(&self.archive)?;
        self.checkpoint_ms = cp.as_ref().map(|c| c.hour_ms);
        let rec = archive::recover(&self.archive, &self.cfg, &self.cfg_sha, self.checkpoint_ms);
        for m in &rec.manifests {
            eprintln!(
                "compact: merged {} (recovered), {} B",
                m.day,
                m.bytes.total()
            );
        }
        self.recover_error = failures(&rec.failed);
        match cp {
            Some(cp) => {
                self.pending_row = Some(row_of(&cp.report, now_ms)?);
                self.red = Some(Reducer::from_state(self.cfg.reduce(), cp.reducer)?);
                self.next_hour = Some(cp.hour_ms + HOUR_MS);
            }
            None => {
                let Some(first) = listing.hours.first().map(|h| h.start_ms) else {
                    return Ok(false);
                };
                let day = day_of(first);
                let dir = archive::day_dir(&self.archive, day);
                anyhow::ensure!(
                    self.cold_day == Some(day) || !published(&dir),
                    "refusing a cold start: no checkpoint in {}, but {} already has published \
                     hours or daily files (a fresh reducer would renumber its tokens); restore a \
                     checkpoint or move {} aside",
                    self.archive.join(state::STATE_DIR).display(),
                    day_name(day),
                    dir.display()
                );
                eprintln!(
                    "compact: no checkpoint: starting cold at {} (first raw hour {})",
                    hour_label(day),
                    hour_label(first)
                );
                self.cold_day = Some(day);
                self.first_file = Some(first);
                self.pending_row = None;
                self.red = Some(Reducer::new(self.cfg.reduce()));
                self.next_hour = Some(day);
            }
        }
        Ok(true)
    }

    /// Compact the next hour if it is ready. Ok(true) = one hour done; Ok(false) = nothing ready.
    /// On an error the reducer is dropped: the next step reloads the last checkpoint.
    pub async fn step(&mut self, store: &Store, now_ms: i64) -> anyhow::Result<bool> {
        let listing = tape::files::listing(&self.raw);
        if self.red.is_none() && !self.load(&listing, now_ms)? {
            return Ok(false);
        }
        if let Some(row) = self.pending_row.clone() {
            store
                .compact_hour_put(&row)
                .await
                .context("record the last hour")?;
            self.pending_row = None;
        }
        let next = self.next_hour.context("no next hour after loading")?;
        for h in &listing.hours {
            if h.start_ms >= next {
                self.seen.insert(h.start_ms);
            }
        }
        self.seen.retain(|h| *h >= next);
        if !ready(next, &listing.hours) {
            return Ok(false);
        }
        let (from, to) = query_range(next, &self.cfg);
        let eps = match store.compact_episodes(from, to).await {
            Ok(e) => e,
            Err(e) => return Err(self.db_failed(e, "read episodes")),
        };
        let windows = windows_for_hour(&eps, next, &self.cfg);
        let source = match listing.hours.iter().find(|h| h.start_ms == next) {
            Some(h) => Source::File(h.path.clone()),
            None => {
                let st = match store.compact_hour_statuses(next, next + HOUR_MS).await {
                    Ok(s) => s,
                    Err(e) => return Err(self.db_failed(e, "read compact_hours")),
                };
                // never overwrite an hour archived from its raw file with a gap
                keep_archived(&self.archive, next)?;
                if self.seen.contains(&next) {
                    // listed before, gone now: only the scanner deletes raw hours (its mark may be
                    // in flight), once the file is really gone, not a day folder that failed to list
                    let p = tape::files::hour_path(&self.raw, next);
                    confirm_gone(std::fs::metadata(&p).map(|_| ()), &p)?;
                    Source::Lost
                } else if st.iter().any(|(_, s)| s == "lost") {
                    Source::Lost
                } else if self.first_file.is_some_and(|f| next < f) {
                    // before the first file this service ever saw: nothing can be in flight
                    Source::Stopped
                } else {
                    let since = match self.missing_since {
                        Some((h, t)) if h == next => t,
                        _ => {
                            self.missing_since = Some((next, now_ms));
                            now_ms
                        }
                    };
                    if now_ms.saturating_sub(since) < MISSING_GRACE_MS {
                        return Ok(false);
                    }
                    // one grace per run of consecutive missing hours: the next one inherits it
                    self.missing_since = Some((next + HOUR_MS, since));
                    Source::Stopped
                }
            }
        };
        let job = HourJob {
            hour_ms: next,
            raw_dir: self.raw.clone(),
            archive_dir: self.archive.clone(),
            source,
            windows,
            stop_before_publish: false,
        };
        // Taken out: on any error below it is dropped, and the next step reloads the checkpoint.
        let mut red = self.red.take().context("reducer not loaded")?;
        let t0 = std::time::Instant::now();
        let (red, res) = tokio::task::spawn_blocking(move || {
            let r = process_hour(&mut red, &job);
            (red, r)
        })
        .await
        .context("hour task")?;
        let report = res?.context("hour stopped before publish")?;
        state::save(
            &self.archive,
            &Checkpoint {
                hour_ms: next,
                report: report.clone(),
                reducer: red.state(),
            },
        )?;
        self.checkpoint_ms = Some(next);
        self.red = Some(red);
        self.next_hour = Some(next + HOUR_MS);
        let row = row_of(&report, now_ms)?;
        self.pending_row = Some(row.clone());
        store
            .compact_hour_put(&row)
            .await
            .context("record the hour")?;
        self.pending_row = None;
        eprintln!(
            "compact: {} done in {} ms, tob={} depth={} l2={} gaps={}, {} B",
            hour_label(next),
            t0.elapsed().as_millis(),
            report.rows.tob,
            report.rows.depth,
            report.rows.l2,
            report.rows.gaps,
            report.bytes.total()
        );
        if let Some(e) = &report.read_error {
            eprintln!(
                "compact: {} read error ({e}); the rest of the hour is a raw_hour_lost gap",
                hour_label(next)
            );
        }
        Ok(true)
    }

    /// A database read failed: the scanner's lost marks may be held back by the same outage, so a
    /// running grace starts again once reads work.
    fn db_failed(&mut self, e: store::StoreError, what: &'static str) -> anyhow::Error {
        self.missing_since = None;
        anyhow::Error::from(e).context(what)
    }

    /// Write compact_status now (spec 5: after each hour and each merge). A failed write is
    /// logged and shown in last_error; it never fails the hour.
    async fn write_status(&mut self, store: &Store, now_ms: i64) {
        let st = self.status(now_ms);
        if let Err(e) = store.compact_status_put(&st).await {
            eprintln!("compact: status not written: {e:#}");
            self.last_error = Some(format!("status not written: {e:#}"));
        }
    }

    /// Merge every complete day and keep the archive under its budget. Never fails: merge
    /// failures and a budget stuck over its limit go to the status row's last_error.
    pub async fn maintain(&mut self) {
        let (arch, cfg, sha) = (self.archive.clone(), self.cfg.clone(), self.cfg_sha.clone());
        let through = self.checkpoint_ms;
        let res = tokio::task::spawn_blocking(move || {
            let merged = archive::merge_ready(&arch, &cfg, &sha, through);
            let budget = archive::enforce_budget(&arch, cfg.budget_bytes());
            (merged, budget)
        })
        .await;
        let (merged, budget) = match res {
            Ok(r) => r,
            Err(e) => {
                self.archive_error = Some(format!("merge and budget task: {e}"));
                return;
            }
        };
        for m in &merged.manifests {
            eprintln!(
                "compact: merged {}, tob={} depth={} l2={} gaps={}, {} B",
                m.day,
                m.totals.tob,
                m.totals.depth,
                m.totals.l2,
                m.totals.gaps,
                m.bytes.total()
            );
        }
        let mut problems: Vec<String> = failures(&merged.failed).into_iter().collect();
        match budget {
            Ok(b) => {
                for (day, bytes) in &b.deleted {
                    eprintln!(
                        "compact: budget: deleted {day} ({bytes} B) to stay under {} GB",
                        self.cfg.archive_budget_gb
                    );
                }
                if let Some(total) = b.stuck_at {
                    problems.push(format!(
                        "archive over budget: {total} B > {} GB with no merged day left to delete",
                        self.cfg.archive_budget_gb
                    ));
                }
            }
            Err(e) => problems.push(format!("budget: {e:#}")),
        }
        self.archive_error = (!problems.is_empty()).then(|| problems.join("; "));
    }

    /// One poll (spec §4, §5): compact every ready hour (merging, then writing compact_status after
    /// each one), stopping at the first error, which goes to last_error and is retried next poll;
    /// then merge and enforce the budget at least once whatever happened; then the status row for
    /// the caller to write.
    pub async fn poll(
        &mut self,
        store: &Store,
        now: impl Fn() -> i64,
        stop: &AtomicBool,
    ) -> CompactStatusRow {
        let mut maintained = false;
        while !stop.load(Ordering::Relaxed) {
            match self.step(store, now()).await {
                Ok(true) => {
                    self.last_error = None;
                    self.maintain().await;
                    maintained = true;
                    self.write_status(store, now()).await;
                }
                Ok(false) => {
                    self.last_error = None;
                    break;
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    eprintln!("compact: {msg}");
                    self.last_error = Some(msg);
                    break;
                }
            }
        }
        if !maintained {
            self.maintain().await;
        }
        self.status(now())
    }

    /// Whole hours behind: from the next hour to compact; with none loaded (a refused cold start,
    /// unreadable checkpoints), from the end of the oldest raw hour on disk, so a stuck compactor
    /// lights the lag heart; with no raw hour either, just above lag_alarm_hours.
    fn lag_hours(&self, now_ms: i64) -> i64 {
        let behind = |end: i64| now_ms.saturating_sub(end).div_euclid(HOUR_MS).max(0);
        match self.next_hour {
            Some(n) => behind(n),
            None => tape::files::listing(&self.raw)
                .hours
                .first()
                .map_or(self.cfg.lag_alarm_hours + 1, |h| {
                    behind(h.start_ms + HOUR_MS)
                }),
        }
    }

    /// The compact_status row (spec §5). Lag = whole hours between the end of the last compacted
    /// hour and now (1–2 in steady state). last_error joins the hour, recovery and archive errors.
    pub fn status(&mut self, now_ms: i64) -> CompactStatusRow {
        let lag = self.lag_hours(now_ms);
        if lag > self.cfg.lag_alarm_hours && !self.lag_alarm {
            eprintln!(
                "compact: {lag} h behind (lag_alarm_hours {}): raw hours are at risk",
                self.cfg.lag_alarm_hours
            );
        }
        self.lag_alarm = lag > self.cfg.lag_alarm_hours;
        let errors: Vec<&str> = [&self.last_error, &self.recover_error, &self.archive_error]
            .into_iter()
            .filter_map(|e| e.as_deref())
            .collect();
        CompactStatusRow {
            updated_ms: now_ms,
            archive_bytes: i64::try_from(archive::archive_bytes(&self.archive)).unwrap_or(i64::MAX),
            oldest_day: archive::oldest_day(&self.archive),
            lag_hours: i32::try_from(lag).unwrap_or(i32::MAX),
            last_error: (!errors.is_empty()).then(|| errors.join("; ")),
        }
    }
}

/// An hour listed earlier and missing now: only a file that is really gone (`NotFound`) is lost.
/// A file still there (its day folder failed to list) or any other error is retried next poll.
pub fn confirm_gone(stat: std::io::Result<()>, path: &Path) -> anyhow::Result<()> {
    match stat {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(()) => anyhow::bail!(
            "{} was not listed but still exists (its day folder could not be read?); retrying",
            path.display()
        ),
        Err(e) => {
            Err(e).with_context(|| format!("stat {} (listed before, missing now)", path.display()))
        }
    }
}

/// Refuse to redo an hour with no raw file when the archive already holds it from its raw file:
/// its sidecar, or on a merged day its manifest entry, records a source file (e.g. resumed from an
/// older checkpoint after the newest one was corrupt, with the raw hour deleted since). Redoing it
/// would replace archived rows with a gap. A gap hour may be redone: it is deterministic, and on a
/// merged day the daily files stay the verified copy (recover deletes the stray hourly parts).
fn keep_archived(archive_dir: &Path, hour_ms: i64) -> anyhow::Result<()> {
    let day = archive::day_dir(archive_dir, hour_ms);
    let side = archive::sidecar_path(&day, archive::hour_number(hour_ms));
    // unparsable: assume it holds data (a visible error beats a silent overwrite)
    let side_from_file = match std::fs::read(&side) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("read {}", side.display())),
        Ok(b) => serde_json::from_slice::<HourReport>(&b).map_or(true, |r| r.source_file.is_some()),
    };
    let mp = day.join(archive::MANIFEST);
    let manifest_from_file = match std::fs::read(&mp) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("read {}", mp.display())),
        Ok(b) => serde_json::from_slice::<archive::Manifest>(&b).map_or(true, |m| {
            m.hours
                .iter()
                .find(|h| h.hour_ms == hour_ms)
                .is_none_or(|h| h.source_file.is_some())
        }),
    };
    anyhow::ensure!(
        !side_from_file && !manifest_from_file,
        "refusing to redo {}: it is already archived from its raw file ({}), but the raw file is \
         gone (resumed from an older checkpoint?); the archived hour is kept: restore the newest \
         checkpoint or the raw file",
        hour_label(hour_ms),
        if side_from_file {
            side.display()
        } else {
            mp.display()
        }
    );
    Ok(())
}

/// `Merged::failed` as one message, or None.
fn failures(failed: &[(String, String)]) -> Option<String> {
    (!failed.is_empty()).then(|| {
        failed
            .iter()
            .map(|(day, e)| format!("day {day} not merged: {e}"))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_file_that_is_really_gone_counts_as_gone() {
        let p = Path::new("raw/2026-09-23/01.jsonl.zst");
        assert!(confirm_gone(Err(std::io::ErrorKind::NotFound.into()), p).is_ok());
        let e = confirm_gone(Ok(()), p).unwrap_err().to_string();
        assert!(e.contains("still exists"), "{e}");
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
        ] {
            assert!(confirm_gone(Err(kind.into()), p).is_err(), "{kind:?}");
        }
    }
}
