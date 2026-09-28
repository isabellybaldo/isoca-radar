//! One raw hour → its Parquet parts and sidecar, atomically (spec 5b §4.1, §4.3): the five parts
//! are written under temporary names and renamed only when all are complete; the sidecar
//! HH.hour.json (this hour's manifest entry) is renamed last, so a sidecar always describes the
//! parts beside it. Every publish is durable (pq: fsync before the rename, the folder after).
//! Re-running an hour overwrites its temporary files and its parts.
use crate::archive::{day_dir, hour_number, part_path, sidecar_path};
use crate::pq::{PartWriter, Staged, Table, write_atomic};
use crate::reduce::{HourStats, Reducer};
use crate::rows::{DepthRow, GapRow, L2Row, Sink, TobRow, TokenRow, reason};
use crate::windows::Window;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use tape::line::Line;
use tape::universe_view::load_record;

/// Rows leave in recv_ms order within this window (plan ruling R4).
pub const REORDER_MS: i64 = 10_000;

/// Sorts rows by recv_ms within `REORDER_MS` of the newest one seen. Stable: rows with equal
/// recv_ms leave in the order they were pushed (l2 readers apply rows in file order, and a
/// token's zeros, snapshot set and diff can share one recv_ms). A row older than one already
/// written is written at once and counted as late, never dropped.
pub struct Reorder<R> {
    buf: BTreeMap<(i64, u64), R>,
    seq: u64,
    high: i64,
    flushed: i64,
    late: u64,
}

impl<R> Default for Reorder<R> {
    fn default() -> Self {
        Reorder {
            buf: BTreeMap::new(),
            seq: 0,
            high: i64::MIN,
            flushed: i64::MIN,
            late: 0,
        }
    }
}

impl<R> Reorder<R> {
    pub fn push(
        &mut self,
        recv_ms: i64,
        row: R,
        out: &mut dyn FnMut(R) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if recv_ms < self.flushed {
            self.late += 1;
            return out(row);
        }
        self.buf.insert((recv_ms, self.seq), row);
        self.seq += 1;
        self.high = self.high.max(recv_ms);
        let cut = self.high.saturating_sub(REORDER_MS);
        while let Some(e) = self.buf.first_entry() {
            if e.key().0 >= cut {
                break;
            }
            let ((k, _), v) = e.remove_entry();
            self.flushed = k;
            out(v)?;
        }
        Ok(())
    }

    pub fn drain(&mut self, out: &mut dyn FnMut(R) -> anyhow::Result<()>) -> anyhow::Result<()> {
        while let Some(((k, _), v)) = self.buf.pop_first() {
            self.flushed = k;
            out(v)?;
        }
        Ok(())
    }

    pub fn late(&self) -> u64 {
        self.late
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowCounts {
    pub tokens: u64,
    pub tob: u64,
    pub depth: u64,
    pub l2: u64,
    pub gaps: u64,
}

impl RowCounts {
    pub fn total(&self) -> u64 {
        self.tokens + self.tob + self.depth + self.l2 + self.gaps
    }
}

/// One hour's manifest entry (spec §3 manifest.json) and its compact_hours row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourReport {
    pub hour_ms: i64,
    /// "done" or "lost".
    pub status: String,
    /// Relative to raw_dir: YYYY-MM-DD/HH.jsonl.zst; None when there was no file.
    pub source_file: Option<String>,
    pub source_bytes: Option<u64>,
    pub source_sha256: Option<String>,
    pub rows: RowCounts,
    /// Bytes of each part.
    pub bytes: RowCounts,
    pub truncated_windows: Vec<String>,
    pub stats: HourStats,
    pub late_rows: u64,
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    /// No file, and compact_hours says lost (the scanner deleted it at its cap).
    Lost,
    /// No file and no record: the scanner was not running.
    Stopped,
}

#[derive(Debug, Clone)]
pub struct HourJob {
    pub hour_ms: i64,
    pub raw_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub source: Source,
    pub windows: Vec<Window>,
    /// Tests: stop after writing every part, before any rename (a crash at the worst moment).
    pub stop_before_publish: bool,
}

/// The five parts of one hour, tob and l2 through the reorder buffers.
struct PartSink {
    tob: PartWriter<TobRow>,
    depth: PartWriter<DepthRow>,
    l2: PartWriter<L2Row>,
    gaps: PartWriter<GapRow>,
    tob_order: Reorder<TobRow>,
    l2_order: Reorder<L2Row>,
    tokens_fin: PathBuf,
}

impl PartSink {
    fn create(day: &Path, hh: u32) -> anyhow::Result<Self> {
        Ok(PartSink {
            tob: PartWriter::create(part_path(day, hh, TobRow::NAME))?,
            depth: PartWriter::create(part_path(day, hh, DepthRow::NAME))?,
            l2: PartWriter::create(part_path(day, hh, L2Row::NAME))?,
            gaps: PartWriter::create(part_path(day, hh, GapRow::NAME))?,
            tob_order: Reorder::default(),
            l2_order: Reorder::default(),
            tokens_fin: part_path(day, hh, TokenRow::NAME),
        })
    }

    /// Drain the buffers, write the tokens part, close everything under temporary names.
    fn close(mut self, tokens: &[TokenRow]) -> anyhow::Result<(Vec<Staged>, u64)> {
        let w = &mut self.tob;
        self.tob_order.drain(&mut |x| w.push(x))?;
        let w = &mut self.l2;
        self.l2_order.drain(&mut |x| w.push(x))?;
        let late = self.tob_order.late() + self.l2_order.late();
        let mut t = PartWriter::<TokenRow>::create(self.tokens_fin.clone())?;
        for row in tokens {
            t.push(row.clone())?;
        }
        Ok((
            vec![
                t.close()?,
                self.tob.close()?,
                self.depth.close()?,
                self.l2.close()?,
                self.gaps.close()?,
            ],
            late,
        ))
    }
}

impl Sink for PartSink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()> {
        let w = &mut self.tob;
        self.tob_order.push(r.recv_ms, r, &mut |x| w.push(x))
    }
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()> {
        self.depth.push(r)
    }
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()> {
        let w = &mut self.l2;
        self.l2_order.push(r.recv_ms, r, &mut |x| w.push(x))
    }
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()> {
        self.gaps.push(r)
    }
}

/// Hashes everything read through it (the raw file's sha256 in one pass).
struct Hashing<R> {
    inner: R,
    h: Sha256,
    n: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.h.update(&buf[..k]);
        self.n += u64::try_from(k).unwrap_or(0);
        Ok(k)
    }
}

struct SourceInfo {
    file: String,
    bytes: u64,
    sha256: String,
    read_error: Option<String>,
}

/// Feed the hour file to the reducer. A file that can't be decoded to the end keeps the lines
/// before the break and turns the rest of the hour into `raw_hour_lost` (the reducer's `cut`).
fn read_hour(
    red: &mut Reducer,
    raw_dir: &Path,
    path: &Path,
    sink: &mut PartSink,
) -> anyhow::Result<SourceInfo> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hr = Hashing {
        inner: f,
        h: Sha256::new(),
        n: 0,
    };
    let mut read_error = None;
    {
        let mut load = |taken: i64, file: &str| load_record(raw_dir, taken, file);
        match zstd::Decoder::new(&mut hr) {
            Err(e) => read_error = Some(format!("zstd: {e}")),
            Ok(dec) => {
                for text in BufReader::new(dec).lines() {
                    let text = match text {
                        Ok(t) => t,
                        Err(e) => {
                            read_error = Some(format!("read: {e}"));
                            break;
                        }
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Line>(&text) {
                        Ok(line) => red.line(&line, &mut load, sink)?,
                        Err(_) => red.count_bad_line(),
                    }
                }
            }
        }
    }
    if read_error.is_some() {
        red.cut(reason::RAW_HOUR_LOST, sink)?;
    }
    // hash the bytes past a break too: the sha256 and size are of the whole file
    std::io::copy(&mut hr, &mut std::io::sink())
        .with_context(|| format!("read {}", path.display()))?;
    let file = path
        .strip_prefix(raw_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(SourceInfo {
        file,
        bytes: hr.n,
        sha256: hr.h.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        read_error,
    })
}

/// Compact one hour into the archive. `Ok(None)` only when `job.stop_before_publish`.
///
/// On an error the reducer is part-way through the hour: the caller restarts from its checkpoint.
pub fn process_hour(red: &mut Reducer, job: &HourJob) -> anyhow::Result<Option<HourReport>> {
    let day = day_dir(&job.archive_dir, job.hour_ms);
    std::fs::create_dir_all(&day).with_context(|| format!("create {}", day.display()))?;
    let hh = hour_number(job.hour_ms);
    let mut sink = PartSink::create(&day, hh)?;
    red.begin_hour(job.hour_ms, job.windows.clone());
    let (status, src) = match &job.source {
        Source::File(p) => ("done", Some(read_hour(red, &job.raw_dir, p, &mut sink)?)),
        Source::Lost => {
            red.missing_hour(reason::RAW_HOUR_LOST, &mut sink)?;
            ("lost", None)
        }
        Source::Stopped => {
            red.missing_hour(reason::SCANNER_STOPPED, &mut sink)?;
            ("done", None)
        }
    };
    let stats = red.end_hour(&mut sink)?;
    let (staged, late_rows) = sink.close(red.tokens())?;
    if job.stop_before_publish {
        return Ok(None);
    }
    // A re-run's old sidecar goes first: a sidecar present always describes the parts beside it.
    let side = sidecar_path(&day, hh);
    match std::fs::remove_file(&side) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("remove {}", side.display())),
    }
    for s in &staged {
        s.publish()
            .with_context(|| format!("publish {}", s.fin.display()))?;
    }
    let count = |t: &str| {
        staged
            .iter()
            .find(|s| s.fin == part_path(&day, hh, t))
            .map_or(0, |s| s.rows)
    };
    let size = |t: &str| -> anyhow::Result<u64> {
        let p = part_path(&day, hh, t);
        Ok(std::fs::metadata(&p)
            .with_context(|| format!("stat {}", p.display()))?
            .len())
    };
    let rows = RowCounts {
        tokens: count(TokenRow::NAME),
        tob: count(TobRow::NAME),
        depth: count(DepthRow::NAME),
        l2: count(L2Row::NAME),
        gaps: count(GapRow::NAME),
    };
    let bytes = RowCounts {
        tokens: size(TokenRow::NAME)?,
        tob: size(TobRow::NAME)?,
        depth: size(DepthRow::NAME)?,
        l2: size(L2Row::NAME)?,
        gaps: size(GapRow::NAME)?,
    };
    let mut truncated: Vec<String> = job
        .windows
        .iter()
        .filter(|w| w.truncated)
        .map(|w| w.episode_id.clone())
        .collect();
    truncated.sort();
    truncated.dedup();
    let report = HourReport {
        hour_ms: job.hour_ms,
        status: status.to_string(),
        source_file: src.as_ref().map(|s| s.file.clone()),
        source_bytes: src.as_ref().map(|s| s.bytes),
        source_sha256: src.as_ref().map(|s| s.sha256.clone()),
        rows,
        bytes,
        truncated_windows: truncated,
        stats,
        late_rows,
        read_error: src.and_then(|s| s.read_error),
    };
    write_atomic(&side, &serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("publish {}", side.display()))?;
    Ok(Some(report))
}
