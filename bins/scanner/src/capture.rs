//! Raw capture (spec §7): <dir>/YYYY-MM-DD/HH.jsonl.zst, one JSON line per engine input:
//! frames {recv_ts_us, shard, raw} and control lines {recv_ts_us, shard, ctl} (down, universe,
//! rest_snapshot, suspect, resolved, shutdown), so replay reproduces the live engine exactly. Universe
//! records live next to them, zstd-compressed, as universe-<taken_ms>-<seq>.json.zst (`<seq>` is
//! a per-run counter so a warm start reusing `taken_ms` can never overwrite an earlier record).
//! Capture never blocks the scanner: it pauses on low disk (hysteresis), deletes hour files past
//! the retention window once compacted (retention.rs), always deletes the oldest at the byte cap,
//! and deletes universe records no remaining hour can reference.
use crate::config::CaptureCfg;
use crate::metrics::Metrics;
use crate::universe::{ConnMap, Universe};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

/// What to capture for a universe refresh (spec 5b §4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniversePlan {
    /// The content changed: write a new record (then call `UniverseDedup::written`).
    Write { hash: String },
    /// Same content as the last record written: capture only a control line naming it.
    Reuse { taken_ms: i64, file: String },
}

/// sha256 of the content a replay uses: the groups, the canary pairs and the connection map.
/// `taken_ms` is left out on purpose: it changes every refresh, the content often does not.
pub fn universe_hash(u: &Universe, conn_of: &ConnMap) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(&(&u.groups, &u.canary, conn_of))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// What the dedup needs to know about the capture thread (spec 5b §4.5, ruling R16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureState {
    /// Seq of the newest universe record the capture thread wrote to disk.
    pub acked_seq: u64,
    pub paused: bool,
    /// Capture messages dropped so far.
    pub drops: u64,
}

impl CaptureState {
    pub fn of(m: &Metrics) -> CaptureState {
        CaptureState {
            acked_seq: m.universe_acked_seq.load(Ordering::Acquire),
            paused: m.capture_paused.load(Ordering::Relaxed),
            drops: m.capture_drops.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct LastRecord {
    hash: String,
    taken_ms: i64,
    file: String,
    seq: u64,
    drops: u64,
}

/// The last universe record handed to the capture thread.
#[derive(Debug, Clone, Default)]
pub struct UniverseDedup {
    last: Option<LastRecord>,
}

impl UniverseDedup {
    /// Reuse the last record only when its content is the same AND capture really wrote it
    /// (acknowledged), is not paused, and dropped nothing since; otherwise write a new record.
    pub fn plan(&self, u: &Universe, conn_of: &ConnMap, st: CaptureState) -> UniversePlan {
        match universe_hash(u, conn_of) {
            Ok(h) => match &self.last {
                Some(l)
                    if l.hash == h
                        && st.acked_seq >= l.seq
                        && !st.paused
                        && st.drops == l.drops =>
                {
                    UniversePlan::Reuse {
                        taken_ms: l.taken_ms,
                        file: l.file.clone(),
                    }
                }
                _ => UniversePlan::Write { hash: h },
            },
            // Can't hash it: always write (an empty hash is never remembered).
            Err(_) => UniversePlan::Write {
                hash: String::new(),
            },
        }
    }

    /// Call once record `seq` was handed to the capture thread (`drops`: the drop count then).
    pub fn written(&mut self, hash: String, taken_ms: i64, file: String, seq: u64, drops: u64) {
        self.last = (!hash.is_empty()).then_some(LastRecord {
            hash,
            taken_ms,
            file,
            seq,
            drops,
        });
    }
}

/// `<seq>` of a `universe-<taken_ms>-<seq>.json.zst` file name.
fn universe_seq(name: &str) -> Option<u64> {
    name.strip_prefix("universe-")?
        .strip_suffix(".json.zst")?
        .rsplit_once('-')?
        .1
        .parse()
        .ok()
}

/// Write one capture message unless capture is paused. A universe record that reaches disk is
/// acknowledged (`metrics.universe_acked_seq`), so the dedup never names a record that was
/// dropped. Returns whether it was written.
pub fn deliver(writer: &mut Writer, msg: &CaptureMsg, metrics: &Metrics) -> bool {
    if metrics.capture_paused.load(Ordering::Relaxed) {
        metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    match writer.write(msg) {
        Ok(()) => {
            if let CaptureMsg::Universe(rec) = msg
                && let Some(seq) = universe_seq(&rec.file)
            {
                metrics.universe_acked_seq.fetch_max(seq, Ordering::Release);
            }
            true
        }
        Err(e) => {
            metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
            eprintln!("capture: write failed: {e}");
            false
        }
    }
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

/// Universe records no remaining hour can name (spec 5b §4.5). A control line names the record in
/// force when it was captured: one written during or after the oldest remaining hour, or the newest
/// one written before it (an unchanged refresh keeps naming that older record). So every record
/// written (mtime) at or after the oldest hour's start is kept, plus the newest one before it; the
/// rest are returned. A record whose write time can't be read is kept. No hour files: nothing.
pub fn stale_universe_files(dir: &Path) -> Vec<PathBuf> {
    let l = tape::files::listing(dir);
    let Some(oldest) = l.hours.iter().map(|h| h.start_ms).min() else {
        return Vec::new();
    };
    let mut before: Vec<(i64, PathBuf)> = l
        .universes
        .into_iter()
        .filter_map(|u| u.mtime_ms.filter(|m| *m < oldest).map(|m| (m, u.path)))
        .collect();
    before.sort();
    before.pop(); // the newest record written before the oldest hour is still in force
    let mut out: Vec<PathBuf> = before.into_iter().map(|(_, p)| p).collect();
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
    guard: Arc<crate::retention::GuardView>,
    lost: tokio::sync::mpsc::UnboundedSender<i64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let dir = PathBuf::from(&cfg.dir);
        let mut writer = Writer::new(dir.clone());
        let mut disk_guard = DiskGuard::new(cfg.pause_below_gb, cfg.resume_above_gb);
        let mut last_check: Option<i64> = None;
        while let Some(msg) = rx.blocking_recv() {
            let now = crate::now_ms();
            if housekeeping_due(last_check, now) {
                last_check = Some(now);
                if let Ok(free) = fs4::available_space(&dir) {
                    let paused = disk_guard.update(free);
                    if paused {
                        writer.finish();
                    }
                    metrics.capture_paused.store(paused, Ordering::Relaxed);
                }
                let view = guard.get(now);
                let n = crate::retention::housekeep(
                    &dir,
                    now,
                    &cfg,
                    view.as_ref(),
                    &lost,
                    writer.current_path(),
                );
                metrics.capture_deletes.fetch_add(n, Ordering::Relaxed);
            }
            deliver(&mut writer, &msg, &metrics);
        }
        writer.finish();
    })
}
