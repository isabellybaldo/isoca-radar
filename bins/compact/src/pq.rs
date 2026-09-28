//! The archive's Parquet files (spec 5b §3): schemas, a writer that works under a temporary name
//! until `publish` renames it, and readers. zstd level 3; dictionary encoding (parquet's default)
//! for every column, which is what `token_idx` needs. Row groups of 262,144 rows bound memory
//! (plan ruling R2).
//!
//! Publish is durable, not just atomic: once an hour is marked done the scanner deletes the raw
//! hour, so a truncated part after a power loss would be unrecoverable. The tmp file's data is
//! `sync_all`'d before each rename, and the containing directory is fsync'd after, so a crash
//! leaves either the old state or the fully-written new one visible under the final name — never
//! a half-written file (a no-op on non-Unix, which cannot fsync a directory handle).
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use arrow::array::{
    Array, ArrayRef, BooleanArray, Int32Array, Int64Array, StringArray, UInt8Array, UInt16Array,
    UInt32Array,
};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const ROW_GROUP_ROWS: usize = 262_144;
pub const BATCH_ROWS: usize = 65_536;
pub const ZSTD_LEVEL: i32 = 3;

pub fn props() -> anyhow::Result<WriterProperties> {
    Ok(WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL)?))
        .set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
        .build())
}

/// `<path>.tmp`: where a file is written before it is published.
pub fn tmp_of(fin: &Path) -> PathBuf {
    let mut s = fin.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Write `bytes` under a temporary name, `fsync` it, then rename over `path` and `fsync` the
/// directory (never a half-written file, and durable across a crash right after the rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_of(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        sync_dir(dir)?;
    }
    Ok(())
}

/// `fsync` a directory so a prior rename inside it is durable across a crash. A no-op on
/// non-Unix platforms, which cannot open a directory as a syncable file handle.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

fn f(name: &str, t: DataType, nullable: bool) -> Field {
    Field::new(name, t, nullable)
}

fn col<T: Array + 'static>(b: &RecordBatch, i: usize) -> anyhow::Result<&T> {
    anyhow::ensure!(i < b.num_columns(), "column {i} missing");
    b.column(i)
        .as_any()
        .downcast_ref::<T>()
        .with_context(|| format!("column {i} has an unexpected type"))
}

fn opt(a: &Int64Array, i: usize) -> Option<i64> {
    if a.is_null(i) { None } else { Some(a.value(i)) }
}

fn arc<A: Array + 'static>(a: A) -> ArrayRef {
    Arc::new(a)
}

pub trait Table: Sized {
    const NAME: &'static str;
    fn schema() -> SchemaRef;
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch>;
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>>;
}

impl Table for TobRow {
    const NAME: &'static str = "tob";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("recv_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("bid", DataType::Int64, true),
            f("bid_sz", DataType::Int64, true),
            f("ask", DataType::Int64, true),
            f("ask_sz", DataType::Int64, true),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(
                    rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.recv_ms).collect::<Vec<_>>(),
                )),
                arc(UInt32Array::from(
                    rows.iter().map(|r| r.token_idx).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.bid).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.bid_sz).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.ask).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.ask_sz).collect::<Vec<_>>(),
                )),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, recv, idx) = (
            col::<Int64Array>(b, 0)?,
            col::<Int64Array>(b, 1)?,
            col::<UInt32Array>(b, 2)?,
        );
        let (bid, bsz, ask, asz) = (
            col::<Int64Array>(b, 3)?,
            col::<Int64Array>(b, 4)?,
            col::<Int64Array>(b, 5)?,
            col::<Int64Array>(b, 6)?,
        );
        Ok((0..b.num_rows())
            .map(|i| TobRow {
                ts_ms: ts.value(i),
                recv_ms: recv.value(i),
                token_idx: idx.value(i),
                bid: opt(bid, i),
                bid_sz: opt(bsz, i),
                ask: opt(ask, i),
                ask_sz: opt(asz, i),
            })
            .collect())
    }
}

impl Table for DepthRow {
    const NAME: &'static str = "depth";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("side", DataType::UInt8, false),
            f("level", DataType::UInt16, false),
            f("price", DataType::Int64, false),
            f("size", DataType::Int64, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(
                    rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>(),
                )),
                arc(UInt32Array::from(
                    rows.iter().map(|r| r.token_idx).collect::<Vec<_>>(),
                )),
                arc(UInt8Array::from(
                    rows.iter().map(|r| r.side).collect::<Vec<_>>(),
                )),
                arc(UInt16Array::from(
                    rows.iter().map(|r| r.level).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.price).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.size).collect::<Vec<_>>(),
                )),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, idx, side, level) = (
            col::<Int64Array>(b, 0)?,
            col::<UInt32Array>(b, 1)?,
            col::<UInt8Array>(b, 2)?,
            col::<UInt16Array>(b, 3)?,
        );
        let (price, size) = (col::<Int64Array>(b, 4)?, col::<Int64Array>(b, 5)?);
        Ok((0..b.num_rows())
            .map(|i| DepthRow {
                ts_ms: ts.value(i),
                token_idx: idx.value(i),
                side: side.value(i),
                level: level.value(i),
                price: price.value(i),
                size: size.value(i),
            })
            .collect())
    }
}

impl Table for L2Row {
    const NAME: &'static str = "l2";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("recv_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("side", DataType::UInt8, false),
            f("price", DataType::Int64, false),
            f("new_size", DataType::Int64, false),
            f("snapshot", DataType::Boolean, false),
            f("episode_id", DataType::Utf8, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(
                    rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.recv_ms).collect::<Vec<_>>(),
                )),
                arc(UInt32Array::from(
                    rows.iter().map(|r| r.token_idx).collect::<Vec<_>>(),
                )),
                arc(UInt8Array::from(
                    rows.iter().map(|r| r.side).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.price).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.new_size).collect::<Vec<_>>(),
                )),
                arc(BooleanArray::from(
                    rows.iter().map(|r| r.snapshot).collect::<Vec<_>>(),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.episode_id.as_str()),
                )),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, recv, idx, side) = (
            col::<Int64Array>(b, 0)?,
            col::<Int64Array>(b, 1)?,
            col::<UInt32Array>(b, 2)?,
            col::<UInt8Array>(b, 3)?,
        );
        let (price, size, snap, ep) = (
            col::<Int64Array>(b, 4)?,
            col::<Int64Array>(b, 5)?,
            col::<BooleanArray>(b, 6)?,
            col::<StringArray>(b, 7)?,
        );
        Ok((0..b.num_rows())
            .map(|i| L2Row {
                ts_ms: ts.value(i),
                recv_ms: recv.value(i),
                token_idx: idx.value(i),
                side: side.value(i),
                price: price.value(i),
                new_size: size.value(i),
                snapshot: snap.value(i),
                episode_id: ep.value(i).to_string(),
            })
            .collect())
    }
}

impl Table for GapRow {
    const NAME: &'static str = "gaps";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("start_ms", DataType::Int64, false),
            f("end_ms", DataType::Int64, false),
            f("shard", DataType::Int32, true),
            f("reason", DataType::Utf8, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(
                    rows.iter().map(|r| r.start_ms).collect::<Vec<_>>(),
                )),
                arc(Int64Array::from(
                    rows.iter().map(|r| r.end_ms).collect::<Vec<_>>(),
                )),
                arc(Int32Array::from(
                    rows.iter().map(|r| r.shard).collect::<Vec<_>>(),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.reason.as_str()),
                )),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (s, e, shard, why) = (
            col::<Int64Array>(b, 0)?,
            col::<Int64Array>(b, 1)?,
            col::<Int32Array>(b, 2)?,
            col::<StringArray>(b, 3)?,
        );
        Ok((0..b.num_rows())
            .map(|i| GapRow {
                start_ms: s.value(i),
                end_ms: e.value(i),
                shard: if shard.is_null(i) {
                    None
                } else {
                    Some(shard.value(i))
                },
                reason: why.value(i).to_string(),
            })
            .collect())
    }
}

impl Table for TokenRow {
    const NAME: &'static str = "tokens";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("token_idx", DataType::UInt32, false),
            f("token_id", DataType::Utf8, false),
            f("market_id", DataType::Utf8, false),
            f("event_id", DataType::Utf8, false),
            f("outcome", DataType::Utf8, false),
            f("neg_risk", DataType::Boolean, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(UInt32Array::from(
                    rows.iter().map(|r| r.token_idx).collect::<Vec<_>>(),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.token_id.as_str()),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.market_id.as_str()),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.event_id.as_str()),
                )),
                arc(StringArray::from_iter_values(
                    rows.iter().map(|r| r.outcome.as_str()),
                )),
                arc(BooleanArray::from(
                    rows.iter().map(|r| r.neg_risk).collect::<Vec<_>>(),
                )),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (idx, tok, mkt) = (
            col::<UInt32Array>(b, 0)?,
            col::<StringArray>(b, 1)?,
            col::<StringArray>(b, 2)?,
        );
        let (ev, out, nr) = (
            col::<StringArray>(b, 3)?,
            col::<StringArray>(b, 4)?,
            col::<BooleanArray>(b, 5)?,
        );
        Ok((0..b.num_rows())
            .map(|i| TokenRow {
                token_idx: idx.value(i),
                token_id: tok.value(i).to_string(),
                market_id: mkt.value(i).to_string(),
                event_id: ev.value(i).to_string(),
                outcome: out.value(i).to_string(),
                neg_risk: nr.value(i),
            })
            .collect())
    }
}

/// A file written under `tmp` and not yet visible under `fin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub tmp: PathBuf,
    pub fin: PathBuf,
    pub rows: u64,
}

impl Staged {
    /// Rename `tmp` to `fin`, then `fsync` the directory: the published file survives a crash.
    pub fn publish(&self) -> std::io::Result<()> {
        std::fs::rename(&self.tmp, &self.fin)?;
        if let Some(dir) = self.fin.parent() {
            sync_dir(dir)?;
        }
        Ok(())
    }
}

/// One Parquet file of one table, written in batches of `BATCH_ROWS` rows.
pub struct PartWriter<R: Table> {
    w: ArrowWriter<File>,
    buf: Vec<R>,
    rows: u64,
    tmp: PathBuf,
    fin: PathBuf,
}

impl<R: Table> PartWriter<R> {
    pub fn create(fin: PathBuf) -> anyhow::Result<Self> {
        let tmp = tmp_of(&fin);
        let file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let w = ArrowWriter::try_new(file, R::schema(), Some(props()?))?;
        Ok(Self {
            w,
            buf: Vec::new(),
            rows: 0,
            tmp,
            fin,
        })
    }

    pub fn push(&mut self, r: R) -> anyhow::Result<()> {
        self.buf.push(r);
        self.rows += 1;
        if self.buf.len() >= BATCH_ROWS {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let b = R::batch(&self.buf)?;
        self.w.write(&b)?;
        self.buf.clear();
        Ok(())
    }

    /// Finish the file under its temporary name and `fsync` it. Nothing is visible until
    /// `Staged::publish`.
    pub fn close(mut self) -> anyhow::Result<Staged> {
        self.flush()?;
        let _meta = self.w.close()?;
        // `ArrowWriter::close` writes the footer and consumes the inner `File` without handing
        // it back, so the tmp file is reopened here to `fsync` it before it can be renamed.
        File::options()
            .write(true)
            .open(&self.tmp)
            .with_context(|| format!("reopen {} to fsync", self.tmp.display()))?
            .sync_all()?;
        Ok(Staged {
            tmp: self.tmp,
            fin: self.fin,
            rows: self.rows,
        })
    }
}

fn open(path: &Path) -> anyhow::Result<ParquetRecordBatchReaderBuilder<File>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(ParquetRecordBatchReaderBuilder::try_new(file)?)
}

pub fn num_rows(path: &Path) -> anyhow::Result<u64> {
    Ok(u64::try_from(
        open(path)?.metadata().file_metadata().num_rows(),
    )?)
}

pub fn num_row_groups(path: &Path) -> anyhow::Result<usize> {
    Ok(open(path)?.metadata().num_row_groups())
}

/// Stream a file batch by batch (at most `BATCH_ROWS` rows in memory).
pub fn for_each_batch(
    path: &Path,
    mut f: impl FnMut(&RecordBatch) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let reader = open(path)?.with_batch_size(BATCH_ROWS).build()?;
    for b in reader {
        f(&b?)?;
    }
    Ok(())
}

/// Every row of a (small) file: tests, tokens, gaps.
pub fn read_all<R: Table>(path: &Path) -> anyhow::Result<Vec<R>> {
    let mut out = Vec::new();
    for_each_batch(path, |b| {
        out.extend(R::rows(b)?);
        Ok(())
    })?;
    Ok(out)
}
