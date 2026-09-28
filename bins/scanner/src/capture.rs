//! Raw capture (spec §7): <dir>/YYYY-MM-DD/HH.jsonl.zst, one JSON line per engine input:
//! frames {recv_ts_us, shard, raw} and control lines {recv_ts_us, shard, ctl} (down, universe,
//! rest_snapshot, suspect, resolved, shutdown), so replay reproduces the live engine exactly. Universe
//! records live next to them, zstd-compressed, as universe-<taken_ms>-<seq>.json.zst (`<seq>` is
//! a per-run counter so a warm start reusing `taken_ms` can never overwrite an earlier record).
//! Capture never blocks the scanner: it pauses on low disk (hysteresis) and deletes hour files
//! older than the retention window, and universe files older than the oldest remaining hour file.
use crate::config::CaptureCfg;
use crate::metrics::Metrics;
use crate::universe::{ConnMap, Universe};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
pub use tape::files::{hour_path, universe_file_name, universe_path};
use tape::files::{hour_start_ms, universe_taken_ms};
pub use tape::line::{Ctl, Line, book_ctl, ctl_book};
pub use tape::time::civil;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseRecord {
    pub universe: Universe,
    pub conn_of: ConnMap,
    /// The exact file name this record is (or will be) written as; matches `Ctl::Universe.file`.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureMsg {
    Line(Line),
    Universe(Box<UniverseRecord>),
}

/// zstd level for hour files and universe records. Ruling S3a-2 (2026-09-25): it stays 3. Phase 0
/// measured ~123 GB/day raw → ~33 GB/day at level 3 (spikes/probe/RESULTS.md Q7c). That is
/// ~1.4 MB/s of input on the capture thread, around 1 % of one core at level 3 (hundreds of MB/s).
/// Level 1 would save a fraction of that 1 % for bigger files, and it would drop the measured basis
/// behind the retention/disk budget. The CPU pressure is in the engine loop, not here.
const ZSTD_LEVEL: i32 = 3;

pub struct Writer {
    dir: PathBuf,
    cur: Option<(PathBuf, zstd::stream::AutoFinishEncoder<'static, File>)>,
    /// Running maximum of every line's ms-truncated timestamp seen so far: the hour file to
    /// write into is chosen from THIS, never from a single line's own ts, so a control line
    /// captured slightly out of order near an hour boundary can never file into an EARLIER hour
    /// than one already open.
    last_ms: i64,
}

impl Writer {
    pub fn new(dir: PathBuf) -> Writer {
        Writer {
            dir,
            cur: None,
            last_ms: i64::MIN,
        }
    }

    pub fn write(&mut self, msg: &CaptureMsg) -> std::io::Result<()> {
        match msg {
            CaptureMsg::Universe(rec) => {
                let p = universe_path(&self.dir, rec.universe.taken_ms, &rec.file);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut enc = zstd::Encoder::new(Vec::new(), ZSTD_LEVEL)?;
                enc.write_all(&serde_json::to_vec(rec.as_ref())?)?;
                std::fs::write(&p, enc.finish()?)
            }
            CaptureMsg::Line(line) => {
                self.last_ms = self.last_ms.max(line.recv_ts_us.div_euclid(1000));
                let path = hour_path(&self.dir, self.last_ms);
                if self.cur.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
                    self.cur = None; // finishes the previous hour's zstd frame
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    // Appending after a restart adds a new zstd frame; multi-frame files decode fine.
                    let f = OpenOptions::new().create(true).append(true).open(&path)?;
                    let enc = zstd::Encoder::new(f, ZSTD_LEVEL)?.auto_finish();
                    self.cur = Some((path, enc));
                }
                if let Some((_, enc)) = self.cur.as_mut() {
                    serde_json::to_writer(&mut *enc, line)?;
                    enc.write_all(b"\n")?;
                }
                Ok(())
            }
        }
    }

    /// The hour file currently open for appending (never deleted by the budget).
    pub fn current_path(&self) -> Option<&Path> {
        self.cur.as_ref().map(|(p, _)| p.as_path())
    }

    pub fn finish(&mut self) {
        self.cur = None;
    }
}

#[derive(Debug, Clone)]
pub struct DiskGuard {
    pause_below: u64,
    resume_above: u64,
    paused: bool,
}

impl DiskGuard {
    pub fn new(pause_below_gb: u64, resume_above_gb: u64) -> DiskGuard {
        let gb = 1_000_000_000_u64;
        DiskGuard {
            pause_below: pause_below_gb.saturating_mul(gb),
            resume_above: resume_above_gb.saturating_mul(gb),
            paused: false,
        }
    }

    pub fn update(&mut self, free_bytes: u64) -> bool {
        if self.paused {
            if free_bytes >= self.resume_above {
                self.paused = false;
            }
        } else if free_bytes < self.pause_below {
            self.paused = true;
        }
        self.paused
    }
}

pub fn expired_hours(dir: &Path, now_ms: i64, retention_hours: i64) -> Vec<PathBuf> {
    let cutoff = now_ms - retention_hours * 3_600_000;
    let mut out = Vec::new();
    let Ok(days) = std::fs::read_dir(dir) else {
        return out;
    };
    for day in days.flatten() {
        let day_name = day.file_name().to_string_lossy().to_string();
        let Ok(files) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if let Some(start) = hour_start_ms(&day_name, &name)
                && start + 3_600_000 <= cutoff
            {
                out.push(f.path());
            }
        }
    }
    out.sort();
    out
}

/// Oldest hour files to delete so the capture directory fits `max_bytes` (chronological by
/// name). `keep` (the file being written) is never returned. Universe record bytes count toward
/// the budget total (so a large backlog of them evicts hour files sooner), but universe files
/// themselves are never returned here — they're evicted separately by `stale_universe_files`.
pub fn over_budget_hours(dir: &Path, max_bytes: u64, keep: Option<&Path>) -> Vec<PathBuf> {
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let mut universe_bytes: u64 = 0;
    let Ok(days) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for day in days.flatten() {
        let day_name = day.file_name().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in entries.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if hour_start_ms(&day_name, &name).is_some() {
                let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((f.path(), size));
            } else if universe_taken_ms(&name).is_some() {
                universe_bytes += f.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    files.sort();
    let mut total: u64 = files.iter().map(|(_, s)| *s).sum::<u64>() + universe_bytes;
    let mut out = Vec::new();
    for (p, size) in files {
        if total <= max_bytes {
            break;
        }
        if keep == Some(p.as_path()) {
            continue;
        }
        total = total.saturating_sub(size);
        out.push(p);
    }
    out
}

/// Universe record files older than the oldest hour file still on disk. Once the retention/
/// budget housekeeping has decided which hour files survive, any universe record whose
/// `taken_ms` predates all of them documents a universe no replay can reach anymore.
/// `None` (no hour files at all) deletes nothing: there is nothing yet to be "older than".
pub fn stale_universe_files(dir: &Path) -> Vec<PathBuf> {
    let mut hour_starts: Vec<i64> = Vec::new();
    let mut universes: Vec<(PathBuf, i64)> = Vec::new();
    let Ok(days) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for day in days.flatten() {
        let day_name = day.file_name().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in entries.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if let Some(start) = hour_start_ms(&day_name, &name) {
                hour_starts.push(start);
            } else if let Some(taken_ms) = universe_taken_ms(&name) {
                universes.push((f.path(), taken_ms));
            }
        }
    }
    let Some(oldest_hour) = hour_starts.into_iter().min() else {
        return Vec::new();
    };
    universes.retain(|(_, taken_ms)| *taken_ms < oldest_hour);
    let mut out: Vec<PathBuf> = universes.into_iter().map(|(p, _)| p).collect();
    out.sort();
    out
}

/// How often the capture thread runs its housekeeping (disk-pause guard, retention, the byte
/// budget, stale universe files).
pub const HOUSEKEEPING_EVERY_MS: i64 = 30_000;

/// Whether housekeeping is due: always on the first message (`last` = None), then every
/// [`HOUSEKEEPING_EVERY_MS`]; a wall clock that stepped backwards also counts as due, so a clock
/// step never pauses housekeeping. Overflow-free (a sentinel `i64::MIN` start once wrapped in
/// release and silently disabled the 10 GB cap, soak run 3).
pub fn housekeeping_due(last: Option<i64>, now_ms: i64) -> bool {
    match last {
        None => true,
        Some(t) => now_ms < t || now_ms.saturating_sub(t) >= HOUSEKEEPING_EVERY_MS,
    }
}

/// The capture thread: never panics, never blocks the engine (the engine uses try_send).
pub fn spawn(
    cfg: CaptureCfg,
    mut rx: tokio::sync::mpsc::Receiver<CaptureMsg>,
    metrics: Arc<Metrics>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let dir = PathBuf::from(&cfg.dir);
        let mut writer = Writer::new(dir.clone());
        let mut guard = DiskGuard::new(cfg.pause_below_gb, cfg.resume_above_gb);
        let mut last_check: Option<i64> = None;
        while let Some(msg) = rx.blocking_recv() {
            let now = crate::now_ms();
            if housekeeping_due(last_check, now) {
                last_check = Some(now);
                if let Ok(free) = fs4::available_space(&dir) {
                    let paused = guard.update(free);
                    if paused {
                        writer.finish();
                    }
                    metrics.capture_paused.store(paused, Ordering::Relaxed);
                }
                for p in expired_hours(&dir, now, cfg.retention_hours) {
                    if std::fs::remove_file(p).is_ok() {
                        metrics.capture_deletes.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let budget = cfg.max_capture_gb.saturating_mul(1_000_000_000);
                for p in over_budget_hours(&dir, budget, writer.current_path()) {
                    if std::fs::remove_file(p).is_ok() {
                        metrics.capture_deletes.fetch_add(1, Ordering::Relaxed);
                    }
                }
                for p in stale_universe_files(&dir) {
                    if std::fs::remove_file(p).is_ok() {
                        metrics.capture_deletes.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            if metrics.capture_paused.load(Ordering::Relaxed) {
                metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if let Err(e) = writer.write(&msg) {
                metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
                eprintln!("capture: write failed: {e}");
            }
        }
        writer.finish();
    })
}
