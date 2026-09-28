//! The capture layout (parent spec §7): <dir>/YYYY-MM-DD/HH.jsonl.zst hour files and
//! universe-<taken_ms>-<seq>.json.zst records in the day of their taken_ms.
use crate::line::Line;
use crate::time::{civil, days_from_civil};
use anyhow::Context;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn day_dir(dir: &Path, ms: i64) -> PathBuf {
    let (y, m, d, _) = civil(ms);
    dir.join(format!("{y:04}-{m:02}-{d:02}"))
}

pub fn hour_path(dir: &Path, ms: i64) -> PathBuf {
    let (_, _, _, h) = civil(ms);
    day_dir(dir, ms).join(format!("{h:02}.jsonl.zst"))
}

/// `<dir>/<day>/<file>`, where `file` is the exact name from `Ctl::Universe`; the day is derived
/// from `taken_ms` alone.
pub fn universe_path(dir: &Path, taken_ms: i64, file: &str) -> PathBuf {
    day_dir(dir, taken_ms).join(file)
}

pub fn universe_file_name(taken_ms: i64, seq: u64) -> String {
    format!("universe-{taken_ms}-{seq}.json.zst")
}

/// `<taken_ms>` of a `universe-<taken_ms>-<seq>.json.zst` file name.
pub fn universe_taken_ms(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("universe-")?.strip_suffix(".json.zst")?;
    let (taken, _seq) = rest.rsplit_once('-')?;
    taken.parse().ok()
}

/// "<day>/<HH>.jsonl.zst" back to the hour's start in ms.
pub fn hour_start_ms(day: &str, file: &str) -> Option<i64> {
    let hh: i64 = file.strip_suffix(".jsonl.zst")?.parse().ok()?;
    let mut it = day.split('-');
    let (y, m, d): (i64, i64, i64) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    Some((days_from_civil(y, m, d) * 86_400 + hh * 3600) * 1000)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourFile {
    pub start_ms: i64,
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseFile {
    pub path: PathBuf,
    pub taken_ms: i64,
    pub bytes: u64,
    /// When the file was written (its mtime); None when the filesystem can't say.
    pub mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Sorted by (start_ms, path).
    pub hours: Vec<HourFile>,
    /// Sorted by path.
    pub universes: Vec<UniverseFile>,
}

fn mtime_ms(m: &std::fs::Metadata) -> Option<i64> {
    let d = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

/// Every hour file and universe record one level below `dir` (the day folders). A missing or
/// unreadable folder lists nothing: callers never fail on a half-deleted capture tree.
pub fn listing(dir: &Path) -> Listing {
    let mut out = Listing::default();
    let Ok(days) = std::fs::read_dir(dir) else {
        return out;
    };
    for day in days.flatten() {
        let day_name = day.file_name().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in entries.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let meta = f.metadata().ok();
            let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            if let Some(start_ms) = hour_start_ms(&day_name, &name) {
                out.hours.push(HourFile {
                    start_ms,
                    path: f.path(),
                    bytes,
                });
            } else if let Some(taken_ms) = universe_taken_ms(&name) {
                out.universes.push(UniverseFile {
                    path: f.path(),
                    taken_ms,
                    bytes,
                    mtime_ms: meta.as_ref().and_then(mtime_ms),
                });
            }
        }
    }
    out.hours.sort_by(|a, b| {
        a.start_ms
            .cmp(&b.start_ms)
            .then_with(|| a.path.cmp(&b.path))
    });
    out.universes.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Every non-blank line of one hour file, in file order. A line that is not a capture line is an
/// error naming the file (replay's contract: a corrupt capture is never silently skipped).
pub fn for_each_line(
    path: &Path,
    mut f: impl FnMut(Line) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(zstd::Decoder::new(file)?);
    for text in reader.lines() {
        let text = text?;
        if text.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(&text)
            .with_context(|| format!("bad capture line in {}", path.display()))?;
        f(line)?;
    }
    Ok(())
}

pub fn read_zst_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut dec = zstd::Decoder::new(bytes.as_slice())
        .with_context(|| format!("decompress {}", path.display()))?;
    let mut json = Vec::new();
    dec.read_to_end(&mut json)
        .with_context(|| format!("decompress {}", path.display()))?;
    serde_json::from_slice(&json).with_context(|| format!("parse {}", path.display()))
}

/// Tests and tools: write one zstd-compressed JSON document (creates the parent folder).
pub fn write_zst_json<T: Serialize>(path: &Path, v: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        zstd::encode_all(serde_json::to_vec(v)?.as_slice(), 3)?,
    )?;
    Ok(())
}

/// Tests and tools: write capture lines as one zstd frame (creates the parent folder).
pub fn write_lines(path: &Path, lines: &[Line]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut enc = zstd::Encoder::new(File::create(path)?, 3)?;
    for l in lines {
        serde_json::to_writer(&mut enc, l)?;
        enc.write_all(b"\n")?;
    }
    enc.finish()?;
    Ok(())
}
