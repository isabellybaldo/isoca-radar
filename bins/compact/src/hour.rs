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
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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
    /// Size and sha256 of the bytes actually read: the whole file, unless `read_error` reports an
    /// I/O failure, which stops them at the bytes read before it.
    pub source_bytes: Option<u64>,
    pub source_sha256: Option<String>,
    pub rows: RowCounts,
    /// Bytes of each part.
    pub bytes: RowCounts,
    pub truncated_windows: Vec<String>,
    pub stats: HourStats,
    pub late_rows: u64,
    /// The first decode or read error of the raw file. A decode error makes `raw_hour_lost` of
    /// the stretch up to the next readable zstd frame (or the hour's end); an I/O error, of the
    /// rest of the hour.
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

/// A `Reorder` that never changes the order of one token's rows. Each row sorts on
/// max(recv_ms, the token's previous key), so the reducer's order per token survives even when a
/// line from another shard stamped rows with a later recv_ms first (a window restart's snapshot
/// set, a token fed from two shards); the stored recv_ms is unchanged. A late row (key below what
/// was already written) cannot jump ahead of its own token: a token with rows still buffered has
/// a key at or above the flush cut, which is above everything written. One key per token seen
/// this hour (the sink lives for one hour).
struct TokenOrder<R> {
    order: Reorder<R>,
    last: HashMap<u32, i64>,
}

impl<R> Default for TokenOrder<R> {
    fn default() -> Self {
        TokenOrder {
            order: Reorder::default(),
            last: HashMap::new(),
        }
    }
}

impl<R> TokenOrder<R> {
    fn push(
        &mut self,
        token_idx: u32,
        recv_ms: i64,
        row: R,
        out: &mut dyn FnMut(R) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let key = self
            .last
            .get(&token_idx)
            .map_or(recv_ms, |k| (*k).max(recv_ms));
        self.last.insert(token_idx, key);
        self.order.push(key, row, out)
    }
}

/// The five parts of one hour, tob and l2 through the per-token reorder buffers.
struct PartSink {
    tob: PartWriter<TobRow>,
    depth: PartWriter<DepthRow>,
    l2: PartWriter<L2Row>,
    gaps: PartWriter<GapRow>,
    tob_order: TokenOrder<TobRow>,
    l2_order: TokenOrder<L2Row>,
    tokens_fin: PathBuf,
}

impl PartSink {
    fn create(day: &Path, hh: u32) -> anyhow::Result<Self> {
        Ok(PartSink {
            tob: PartWriter::create(part_path(day, hh, TobRow::NAME))?,
            depth: PartWriter::create(part_path(day, hh, DepthRow::NAME))?,
            l2: PartWriter::create(part_path(day, hh, L2Row::NAME))?,
            gaps: PartWriter::create(part_path(day, hh, GapRow::NAME))?,
            tob_order: TokenOrder::default(),
            l2_order: TokenOrder::default(),
            tokens_fin: part_path(day, hh, TokenRow::NAME),
        })
    }

    /// Drain the buffers, write the tokens part, close everything under temporary names.
    fn close(mut self, tokens: &[TokenRow]) -> anyhow::Result<(Vec<Staged>, u64)> {
        let w = &mut self.tob;
        self.tob_order.order.drain(&mut |x| w.push(x))?;
        let w = &mut self.l2;
        self.l2_order.order.drain(&mut |x| w.push(x))?;
        let late = self.tob_order.order.late() + self.l2_order.order.late();
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
        self.tob_order
            .push(r.token_idx, r.recv_ms, r, &mut |x| w.push(x))
    }
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()> {
        self.depth.push(r)
    }
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()> {
        let w = &mut self.l2;
        self.l2_order
            .push(r.token_idx, r.recv_ms, r, &mut |x| w.push(x))
    }
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()> {
        self.gaps.push(r)
    }
}

struct SourceInfo {
    file: String,
    bytes: u64,
    sha256: String,
    read_error: Option<String>,
}

/// Feed the hour file to the reducer. An undecodable stretch of the file becomes `raw_hour_lost`
/// up to the next readable frame, an I/O error the rest of the hour (the reducer's `cut`); a line
/// that is not a capture line (bad JSON or UTF-8) is only a bad line.
fn read_hour(
    red: &mut Reducer,
    raw_dir: &Path,
    path: &Path,
    sink: &mut PartSink,
) -> anyhow::Result<SourceInfo> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let (bytes, sha256, read_error) = feed(red, raw_dir, f, sink)?;
    let file = path
        .strip_prefix(raw_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(SourceInfo {
        file,
        bytes,
        sha256,
        read_error,
    })
}

/// The raw file, read by offset: hashes each byte once, in file order, however often a resync
/// re-reads it (sha256 in one pass), and keeps the first I/O error (as opposed to a decode error).
struct Src<R> {
    inner: R,
    pos: u64,
    hashed: u64,
    h: Sha256,
    io_error: Option<String>,
}

impl<R: Read + Seek> Src<R> {
    fn seek_to(&mut self, at: u64) -> std::io::Result<()> {
        if at != self.pos {
            if let Err(e) = self.inner.seek(SeekFrom::Start(at)) {
                self.io_error.get_or_insert(format!("read: {e}"));
                return Err(e);
            }
            self.pos = at;
        }
        Ok(())
    }
}

impl<R: Read> Read for Src<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = match self.inner.read(buf) {
            Ok(k) => k,
            Err(e) => {
                self.io_error.get_or_insert(format!("read: {e}"));
                return Err(e);
            }
        };
        let end = self.pos + u64::try_from(k).unwrap_or(0);
        if end > self.hashed {
            let fresh = usize::try_from(self.hashed.saturating_sub(self.pos)).unwrap_or(0);
            self.h.update(&buf[fresh..k]);
            self.hashed = end;
        }
        self.pos = end;
        Ok(k)
    }
}

/// A zstd frame starts with these bytes (0xFD2FB528, little-endian).
const FRAME_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The offset of the next frame magic at or after `from`, or None at the end of the file.
fn next_frame<R: Read + Seek>(src: &mut Src<R>, from: u64) -> std::io::Result<Option<u64>> {
    src.seek_to(from)?;
    let mut buf = vec![0u8; 64 * 1024];
    // the last bytes of the previous chunk (a magic may straddle two reads), from offset `base`
    let mut w: Vec<u8> = Vec::new();
    let mut base = from;
    loop {
        let k = src.read(&mut buf)?;
        if k == 0 {
            return Ok(None);
        }
        w.extend_from_slice(&buf[..k]);
        if let Some(i) = w.windows(4).position(|x| x == FRAME_MAGIC) {
            return Ok(Some(base + u64::try_from(i).unwrap_or(0)));
        }
        let keep = w.len().min(FRAME_MAGIC.len() - 1);
        let drop = w.len() - keep;
        w.drain(..drop);
        base += u64::try_from(drop).unwrap_or(0);
    }
}

/// How one frame's decoding ended.
enum FrameEnd {
    /// The frame ended; the next one starts at this offset.
    Done(u64),
    /// Decoding (or reading) failed with this message.
    Broken(String),
}

/// `read_hour` over any reader: (bytes read, their sha256, the first read error).
///
/// Frame by frame: a scanner restarting within the hour appends a new zstd frame, so a frame left
/// unfinished by an unclean exit must not cost the frames after it. On a decode error the rest of
/// the file is searched for the next frame magic and decoding resumes there (a false positive
/// only fails to decode, and the search goes on); the unreadable stretch is a whole-feed
/// `raw_hour_lost` gap from the last good line to the next one (the reducer's `cut`: every book
/// waits for a new snapshot). An I/O error ends the hour there.
fn feed(
    red: &mut Reducer,
    raw_dir: &Path,
    input: impl Read + Seek,
    sink: &mut PartSink,
) -> anyhow::Result<(u64, String, Option<String>)> {
    let mut src = Src {
        inner: input,
        pos: 0,
        hashed: 0,
        h: Sha256::new(),
        io_error: None,
    };
    let mut read_error: Option<String> = None;
    // the file ended in an error with no cut made for it yet
    let mut open_break = false;
    let mut load = |taken: i64, file: &str| load_record(raw_dir, taken, file);
    let mut at = 0u64;
    loop {
        if src.seek_to(at).is_err() {
            open_break = true;
            break;
        }
        let end = {
            let mut input = BufReader::with_capacity(128 * 1024, &mut src);
            match input.fill_buf() {
                Err(_) => {
                    open_break = true;
                    break;
                }
                Ok([]) => {
                    if at == 0 {
                        read_error.get_or_insert("zstd: empty file".to_string());
                        open_break = true;
                    }
                    break;
                }
                Ok(_) => {}
            }
            let dec = match zstd::stream::read::Decoder::with_buffer(input) {
                Ok(d) => d.single_frame(),
                Err(e) => {
                    read_error.get_or_insert(format!("zstd: {e}"));
                    open_break = true;
                    break;
                }
            };
            let mut rd = BufReader::new(dec);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match rd.read_until(b'\n', &mut buf) {
                    Ok(0) => {
                        let input = rd.into_inner().finish();
                        let unread = u64::try_from(input.buffer().len()).unwrap_or(0);
                        break FrameEnd::Done(input.get_ref().pos - unread);
                    }
                    Ok(_) => {}
                    Err(e) => break FrameEnd::Broken(format!("read: {e}")),
                }
                if buf.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice::<Line>(&buf) {
                    Ok(line) => red.line(&line, &mut load, sink)?,
                    Err(_) => red.count_bad_line(),
                }
            }
        };
        match end {
            // a frame always has a header, so decoding one moves forward
            FrameEnd::Done(next) if next > at => at = next,
            FrameEnd::Done(_) => {
                read_error.get_or_insert("zstd: a frame of no bytes".to_string());
                open_break = true;
                break;
            }
            FrameEnd::Broken(e) => {
                if src.io_error.is_some() {
                    open_break = true;
                    break;
                }
                read_error.get_or_insert(e);
                red.cut(reason::RAW_HOUR_LOST, sink)?;
                match next_frame(&mut src, at + 1) {
                    Ok(Some(x)) => at = x,
                    Ok(None) => break,
                    Err(_) => {
                        open_break = true;
                        break;
                    }
                }
            }
        }
    }
    if let Some(e) = &src.io_error {
        read_error.get_or_insert(e.clone());
    } else if src.seek_to(src.hashed).is_ok() {
        // Hash any bytes not read yet (only after a failure to start a decoder): size and sha256
        // are of the whole file, or of the bytes read before an I/O error. Never fails the hour.
        if let Err(e) = std::io::copy(&mut src, &mut std::io::sink()) {
            read_error.get_or_insert(format!("read: {e}"));
        }
    }
    if let Some(e) = &src.io_error {
        read_error.get_or_insert(e.clone());
        open_break = true;
    }
    if open_break {
        red.cut(reason::RAW_HOUR_LOST, sink)?;
    }
    let sha256 = src
        .h
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((src.hashed, sha256, read_error))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reduce::ReduceCfg;

    /// A raw file whose disk fails: every read errors, with a new message each time.
    struct Failing(u32);

    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Err(std::io::Error::other(format!("disk error {}", self.0)))
        }
    }

    impl Seek for Failing {
        fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
            Ok(0)
        }
    }

    #[test]
    fn an_io_error_keeps_the_first_error_and_does_not_fail_the_hour() {
        let day = std::env::temp_dir().join(format!(
            "compact-unit-ioerr-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        std::fs::create_dir_all(&day).unwrap();
        let mut red = Reducer::new(ReduceCfg {
            depth_every_ms: 300_000,
            depth_levels: 2,
        });
        let hour_ms = 1_790_121_600_000;
        let mut sink = PartSink::create(&day, 0).unwrap();
        red.begin_hour(hour_ms, vec![]);
        let (bytes, sha, err) = feed(&mut red, &day, Failing(0), &mut sink).unwrap();
        assert_eq!(bytes, 0);
        assert_eq!(sha, crate::config::sha256_hex(&[]));
        let err = err.unwrap();
        assert!(
            err.contains("disk error 1"),
            "the first error is kept: {err}"
        );
        red.end_hour(&mut sink).unwrap();
        let (staged, _) = sink.close(red.tokens()).unwrap();
        let gaps = staged
            .iter()
            .find(|s| s.fin.ends_with("00.gaps.parquet"))
            .unwrap();
        gaps.publish().unwrap();
        let rows = crate::pq::read_all::<GapRow>(&gaps.fin).unwrap();
        assert!(
            rows.iter()
                .any(|g| g.reason == reason::RAW_HOUR_LOST && g.end_ms == hour_ms + 3_600_000),
            "{rows:?}"
        );
        std::fs::remove_dir_all(&day).ok();
    }
}
