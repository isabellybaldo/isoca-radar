//! Raw capture (spec §7): <dir>/YYYY-MM-DD/HH.jsonl.zst, one JSON line per engine input:
//! frames {recv_ts_us, shard, raw} and control lines {recv_ts_us, shard, ctl} (down, universe,
//! rest_snapshot, suspect, shutdown), so replay reproduces the live engine exactly. Universe
//! records live next to them as universe-<taken_ms>.json. Capture never blocks the scanner: it
//! pauses on low disk (hysteresis) and deletes hour files older than the retention window.
use crate::config::CaptureCfg;
use crate::metrics::Metrics;
use crate::universe::{ConnMap, Universe};
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub recv_ts_us: i64,
    pub shard: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctl: Option<Ctl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ctl {
    Down,
    Universe {
        taken_ms: i64,
    },
    RestSnapshot {
        token: String,
        server_ts_ms: i64,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
    },
    Suspect {
        event_id: String,
        until_ms: i64,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseRecord {
    pub universe: Universe,
    pub conn_of: ConnMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureMsg {
    Line(Line),
    Universe(Box<UniverseRecord>),
}

/// UTC (year, month, day, hour) of a millisecond timestamp (Howard Hinnant's civil_from_days).
pub fn civil(ms: i64) -> (i64, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let hour = u32::try_from(secs.rem_euclid(86_400) / 3600).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, hour)
}

fn day_dir(dir: &Path, ms: i64) -> PathBuf {
    let (y, m, d, _) = civil(ms);
    dir.join(format!("{y:04}-{m:02}-{d:02}"))
}

pub fn hour_path(dir: &Path, ms: i64) -> PathBuf {
    let (_, _, _, h) = civil(ms);
    day_dir(dir, ms).join(format!("{h:02}.jsonl.zst"))
}

pub fn universe_path(dir: &Path, taken_ms: i64) -> PathBuf {
    day_dir(dir, taken_ms).join(format!("universe-{taken_ms}.json"))
}

pub fn book_ctl(token: &str, book: &Book) -> Ctl {
    let lv = |v: Vec<(Price, Shares)>| -> Vec<(String, String)> {
        v.into_iter()
            .map(|(p, s)| (p.get().to_string(), s.get().to_string()))
            .collect()
    };
    Ctl::RestSnapshot {
        token: token.to_string(),
        server_ts_ms: book.server_ts_ms(),
        bids: lv(book.bids_best_first()),
        asks: lv(book.asks_best_first()),
    }
}

pub fn ctl_book(
    bids: &[(String, String)],
    asks: &[(String, String)],
    ts: i64,
) -> anyhow::Result<Book> {
    let parse = |v: &[(String, String)]| -> anyhow::Result<Vec<(Price, Shares)>> {
        v.iter()
            .map(|(p, s)| -> anyhow::Result<(Price, Shares)> {
                Ok((Price::parse(p)?, Shares::parse(s)?))
            })
            .collect()
    };
    let mut b = Book::default();
    b.apply_snapshot(&parse(bids)?, &parse(asks)?, ts);
    Ok(b)
}

pub struct Writer {
    dir: PathBuf,
    cur: Option<(PathBuf, zstd::stream::AutoFinishEncoder<'static, File>)>,
}

impl Writer {
    pub fn new(dir: PathBuf) -> Writer {
        Writer { dir, cur: None }
    }

    pub fn write(&mut self, msg: &CaptureMsg) -> std::io::Result<()> {
        match msg {
            CaptureMsg::Universe(rec) => {
                let p = universe_path(&self.dir, rec.universe.taken_ms);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&p, serde_json::to_vec(rec.as_ref())?)
            }
            CaptureMsg::Line(line) => {
                let path = hour_path(&self.dir, line.recv_ts_us.div_euclid(1000));
                if self.cur.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
                    self.cur = None; // finishes the previous hour's zstd frame
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    // Appending after a restart adds a new zstd frame; multi-frame files decode fine.
                    let f = OpenOptions::new().create(true).append(true).open(&path)?;
                    let enc = zstd::Encoder::new(f, 3)?.auto_finish();
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

/// Parse "<dir>/YYYY-MM-DD/HH.jsonl.zst" back to the hour's start in ms.
fn hour_start_ms(day: &str, file: &str) -> Option<i64> {
    let hh: i64 = file.strip_suffix(".jsonl.zst")?.parse().ok()?;
    let mut it = day.split('-');
    let (y, m, d): (i64, i64, i64) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    // days_from_civil (inverse of civil)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hh * 3600) * 1000)
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
/// name). `keep` (the file being written) is never returned.
pub fn over_budget_hours(dir: &Path, max_bytes: u64, keep: Option<&Path>) -> Vec<PathBuf> {
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
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
            }
        }
    }
    files.sort();
    let mut total: u64 = files.iter().map(|(_, s)| *s).sum();
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
        let mut last_check = i64::MIN;
        while let Some(msg) = rx.blocking_recv() {
            let now = crate::now_ms();
            if now - last_check >= 30_000 {
                last_check = now;
                if let Ok(free) = fs4::available_space(&dir) {
                    let paused = guard.update(free);
                    if paused {
                        writer.finish();
                    }
                    metrics.capture_paused.store(paused, Ordering::Relaxed);
                }
                for p in expired_hours(&dir, now, cfg.retention_hours) {
                    let _ = std::fs::remove_file(p);
                }
                let budget = cfg.max_capture_gb.saturating_mul(1_000_000_000);
                for p in over_budget_hours(&dir, budget, writer.current_path()) {
                    let _ = std::fs::remove_file(p);
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
