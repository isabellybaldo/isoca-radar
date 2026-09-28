//! The archive layout (spec 5b §3, §4.4): <archive>/YYYY-MM-DD/ holds hourly parts
//! HH.<table>.parquet and HH.hour.json until the day is complete, then the daily
//! <table>.parquet files and manifest.json. <archive>/_state/ holds the checkpoints.
//!
//! The daily merge (§4.4) is crash-safe in this order: the five daily files are written under
//! temporary names, fsync'd, their row counts checked against the sidecars, then published
//! (durable renames); `manifest.json` is published last, and only then are the hourly parts
//! deleted. A day is "merged" once its manifest exists. `recover` finishes whatever a crash left:
//! a day without a manifest is merged again from its parts; a merged day's leftover parts are
//! deleted only when its manifest parses and matches the daily files. The merge concatenates the
//! hours in order and each part's rows in file order: it never re-sorts (readers rely on each
//! token's order inside a part: l2 snapshot sets before their diffs, tob ts non-decreasing).
use crate::config::Config;
use crate::hour::{HourReport, RowCounts};
use crate::pq::{Staged, Table, for_each_batch, num_rows, props, tmp_of, write_atomic};
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use tape::time::{civil, day_name, parse_day};

pub const MANIFEST: &str = "manifest.json";

const HOUR_MS: i64 = 3_600_000;

pub fn day_dir(archive: &Path, ms: i64) -> PathBuf {
    archive.join(day_name(ms))
}

pub fn hour_number(ms: i64) -> u32 {
    civil(ms).3
}

pub fn part_path(day: &Path, hh: u32, table: &str) -> PathBuf {
    day.join(format!("{hh:02}.{table}.parquet"))
}

pub fn sidecar_path(day: &Path, hh: u32) -> PathBuf {
    day.join(format!("{hh:02}.hour.json"))
}

pub fn daily_path(day: &Path, table: &str) -> PathBuf {
    day.join(format!("{table}.parquet"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub compact_version: String,
    pub day: String,
    pub config: Config,
    pub config_sha256: String,
    /// All 24 hours, in order (each sidecar as written, read_error and late_rows included).
    pub hours: Vec<HourReport>,
    pub truncated_windows: Vec<String>,
    pub totals: RowCounts,
    pub bytes: RowCounts,
    /// tob rows whose server ts_ms was raised to the token's previous one (ruling R4), all hours.
    pub ts_clamped: u64,
    /// tob/l2 rows written after the reorder window (ruling R4), all hours.
    pub late_rows: u64,
}

/// Day folders (YYYY-MM-DD names only; `_state` and strays are ignored), oldest first.
pub fn days(archive: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(archive)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
                .filter(|(n, _)| parse_day(n).is_some())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn merged(day: &Path) -> bool {
    day.join(MANIFEST).exists()
}

/// All 24 hours published (each has its sidecar).
pub fn complete(day: &Path) -> bool {
    (0..24u32).all(|hh| sidecar_path(day, hh).exists())
}

/// `HH.<anything>` (hourly parts, sidecars and their temporary files).
fn hourly(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 3 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[2] == b'.'
}

fn names(day: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    Ok(std::fs::read_dir(day)
        .with_context(|| format!("list {}", day.display()))?
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect())
}

/// Delete the files `pick` selects; a file already gone is fine (idempotent).
fn remove_where(day: &Path, pick: impl Fn(&str) -> bool) -> anyhow::Result<()> {
    for (n, p) in names(day)? {
        if pick(&n) {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("remove {}", p.display())),
            }
        }
    }
    Ok(())
}

/// A merged day's leftovers: hourly parts, sidecars, and every temporary file.
fn remove_leftovers(day: &Path) -> anyhow::Result<()> {
    remove_where(day, |n| hourly(n) || n.ends_with(".tmp"))
}

/// Temporary files only (a crash mid-hour or mid-merge; nothing is in progress at startup).
fn remove_temps(day: &Path) -> anyhow::Result<()> {
    remove_where(day, |n| n.ends_with(".tmp"))
}

fn sync_file(p: &Path) -> anyhow::Result<()> {
    File::options()
        .write(true)
        .open(p)
        .with_context(|| format!("reopen {} to fsync", p.display()))?
        .sync_all()
        .with_context(|| format!("fsync {}", p.display()))
}

/// One daily file under its temporary name, fsync'd: the 24 parts' batches in hour order, each
/// part's rows in file order (never re-sorted).
fn merge_table<R: Table>(day: &Path) -> anyhow::Result<Staged> {
    let fin = daily_path(day, R::NAME);
    let tmp = tmp_of(&fin);
    let file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut w = ArrowWriter::try_new(file, R::schema(), Some(props()?))?;
    let mut rows = 0u64;
    for hh in 0..24u32 {
        let part = part_path(day, hh, R::NAME);
        for_each_batch(&part, |b| {
            let b = RecordBatch::try_new(R::schema(), b.columns().to_vec())?;
            rows += u64::try_from(b.num_rows())?;
            w.write(&b)?;
            Ok(())
        })
        .with_context(|| format!("merge {}", part.display()))?;
    }
    let _meta = w.close()?;
    sync_file(&tmp)?;
    Ok(Staged { tmp, fin, rows })
}

/// The tokens file: the last hour's part holds the whole day's dictionary.
fn stage_tokens(day: &Path) -> anyhow::Result<Staged> {
    let fin = daily_path(day, TokenRow::NAME);
    let tmp = tmp_of(&fin);
    let src = part_path(day, 23, TokenRow::NAME);
    std::fs::copy(&src, &tmp)
        .with_context(|| format!("copy {} to {}", src.display(), tmp.display()))?;
    sync_file(&tmp)?;
    Ok(Staged {
        rows: num_rows(&tmp)?,
        tmp,
        fin,
    })
}

fn stage_all(day: &Path) -> anyhow::Result<Vec<Staged>> {
    Ok(vec![
        stage_tokens(day)?,
        merge_table::<TobRow>(day)?,
        merge_table::<DepthRow>(day)?,
        merge_table::<L2Row>(day)?,
        merge_table::<GapRow>(day)?,
    ])
}

fn read_sidecars(day: &Path, name: &str) -> anyhow::Result<Vec<HourReport>> {
    let day_ms = parse_day(name).with_context(|| format!("{name} is not a day"))?;
    let mut hours: Vec<HourReport> = Vec::with_capacity(24);
    for hh in 0..24u32 {
        let p = sidecar_path(day, hh);
        let bytes = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
        let h: HourReport =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", p.display()))?;
        let want = day_ms + i64::from(hh) * HOUR_MS;
        anyhow::ensure!(
            h.hour_ms == want,
            "{} is for hour_ms {}, expected {want}",
            p.display(),
            h.hour_ms
        );
        hours.push(h);
    }
    Ok(hours)
}

/// Merge a complete day (spec §4.4): daily files under temporary names, row counts checked
/// against the sidecars, then published, the manifest written last, the hourly parts deleted.
pub fn merge_day(day: &Path, name: &str, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Manifest> {
    let hours = read_sidecars(day, name)?;
    let sum = |f: fn(&RowCounts) -> u64| hours.iter().map(|h| f(&h.rows)).sum::<u64>();
    let want = RowCounts {
        tokens: hours.last().map_or(0, |h| h.rows.tokens),
        tob: sum(|r| r.tob),
        depth: sum(|r| r.depth),
        l2: sum(|r| r.l2),
        gaps: sum(|r| r.gaps),
    };
    let clean = || {
        for t in [
            TokenRow::NAME,
            TobRow::NAME,
            DepthRow::NAME,
            L2Row::NAME,
            GapRow::NAME,
        ] {
            let _ = std::fs::remove_file(tmp_of(&daily_path(day, t)));
        }
    };
    let staged = match stage_all(day) {
        Ok(s) => s,
        Err(e) => {
            clean();
            return Err(e.context(format!("merge {name}")));
        }
    };
    let wants = [want.tokens, want.tob, want.depth, want.l2, want.gaps];
    for (s, w) in staged.iter().zip(wants) {
        let got = num_rows(&s.tmp)?;
        if got != w || s.rows != w {
            clean();
            anyhow::bail!(
                "merge {name}: {} has {got} rows, the hours say {w}",
                s.fin.display()
            );
        }
    }
    for s in &staged {
        s.publish()
            .with_context(|| format!("publish {}", s.fin.display()))?;
    }
    let size = |t: &str| std::fs::metadata(daily_path(day, t)).map_or(0, |m| m.len());
    let mut truncated: Vec<String> = hours
        .iter()
        .flat_map(|h| h.truncated_windows.clone())
        .collect();
    truncated.sort();
    truncated.dedup();
    let m = Manifest {
        compact_version: crate::version().to_string(),
        day: name.to_string(),
        config: cfg.clone(),
        config_sha256: cfg_sha.to_string(),
        totals: want,
        bytes: RowCounts {
            tokens: size(TokenRow::NAME),
            tob: size(TobRow::NAME),
            depth: size(DepthRow::NAME),
            l2: size(L2Row::NAME),
            gaps: size(GapRow::NAME),
        },
        ts_clamped: hours.iter().map(|h| h.stats.ts_clamped).sum(),
        late_rows: hours.iter().map(|h| h.late_rows).sum(),
        hours,
        truncated_windows: truncated,
    };
    let mp = day.join(MANIFEST);
    write_atomic(&mp, &serde_json::to_vec_pretty(&m)?)
        .with_context(|| format!("publish {}", mp.display()))?;
    remove_leftovers(day)?;
    Ok(m)
}

/// Merge every complete, unmerged day.
pub fn merge_ready(archive: &Path, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Vec<Manifest>> {
    let mut out = Vec::new();
    for (name, dir) in days(archive) {
        if !merged(&dir) && complete(&dir) {
            out.push(merge_day(&dir, &name, cfg, cfg_sha)?);
        }
    }
    Ok(out)
}

/// A merged day's manifest parses and every daily file holds the rows it states.
fn verified(day: &Path) -> anyhow::Result<()> {
    let p = day.join(MANIFEST);
    let m: Manifest = serde_json::from_slice(
        &std::fs::read(&p).with_context(|| format!("read {}", p.display()))?,
    )
    .with_context(|| format!("parse {}", p.display()))?;
    for (t, want) in [
        (TokenRow::NAME, m.totals.tokens),
        (TobRow::NAME, m.totals.tob),
        (DepthRow::NAME, m.totals.depth),
        (L2Row::NAME, m.totals.l2),
        (GapRow::NAME, m.totals.gaps),
    ] {
        let got = num_rows(&daily_path(day, t))?;
        anyhow::ensure!(
            got == want,
            "{t}.parquet has {got} rows, the manifest says {want}"
        );
    }
    Ok(())
}

/// At startup: finish what an interrupted hour or merge left, then merge what is ready.
/// Idempotent. Temporary files go everywhere (nothing is in progress at startup). A merged day's
/// leftover hourly parts go only once its manifest verifies; if it doesn't and every sidecar is
/// there, the day is merged again from its parts; otherwise everything stays and it is logged.
/// An unmerged day's parts are never touched.
pub fn recover(archive: &Path, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Vec<Manifest>> {
    let mut out = Vec::new();
    for (name, dir) in days(archive) {
        remove_temps(&dir)?;
        if !merged(&dir) || !names(&dir)?.iter().any(|(n, _)| hourly(n)) {
            continue;
        }
        match verified(&dir) {
            Ok(()) => remove_leftovers(&dir)?,
            Err(e) if complete(&dir) => {
                eprintln!("compact: {name}: manifest does not verify ({e:#}); merging it again");
                out.push(merge_day(&dir, &name, cfg, cfg_sha)?);
            }
            Err(e) => eprintln!(
                "compact: {name}: manifest does not verify ({e:#}) and some hours are missing; \
                 leaving its files alone"
            ),
        }
    }
    out.extend(merge_ready(archive, cfg, cfg_sha)?);
    Ok(out)
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Every byte under the archive, checkpoints included.
pub fn archive_bytes(archive: &Path) -> u64 {
    dir_bytes(archive)
}

pub fn oldest_day(archive: &Path) -> Option<String> {
    days(archive).into_iter().next().map(|(n, _)| n)
}

/// While over `budget`, delete the oldest merged day (spec §4.4). Returns (day, bytes) deleted.
/// Never an unmerged day (today's included) and never `_state`: if only those remain over the
/// budget, it is logged and nothing more is deleted.
pub fn enforce_budget(archive: &Path, budget: u64) -> anyhow::Result<Vec<(String, u64)>> {
    let mut gone = Vec::new();
    loop {
        let total = archive_bytes(archive);
        if total <= budget {
            break;
        }
        let Some((name, dir)) = days(archive).into_iter().find(|(_, d)| merged(d)) else {
            eprintln!(
                "compact: archive is {total} B, over its {budget} B budget, but no merged day is \
                 left to delete (unmerged days and checkpoints are kept)"
            );
            break;
        };
        let bytes = dir_bytes(&dir);
        std::fs::remove_dir_all(&dir).with_context(|| format!("delete {}", dir.display()))?;
        gone.push((name, bytes));
    }
    Ok(gone)
}
