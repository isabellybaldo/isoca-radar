//! Checkpoints (spec 5b §4.1): after each hour, the reducer's whole state (books, pending checks,
//! trust, the day's dictionary, open gaps) goes to _state/checkpoint-<hour_ms>.bin.zst
//! (zstd-compressed JSON) via a temporary name and a rename. The newest two are kept; a
//! checkpoint that can't be read falls back to the older one. `load_newest` returns `Ok(None)`
//! only when no checkpoint file exists at all; if checkpoint files exist but every one of them is
//! unreadable, it returns `Err` naming them, so a caller never silently starts fresh after a
//! double corruption (which would re-emit or skip already-processed hours).
use crate::hour::HourReport;
use crate::pq::write_atomic;
use crate::reduce::ReducerState;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STATE_DIR: &str = "_state";
const KEEP: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The last hour processed.
    pub hour_ms: i64,
    /// Its report: re-recorded in compact_hours at startup if the database write was missed.
    pub report: HourReport,
    pub reducer: ReducerState,
}

fn path(archive: &Path, hour_ms: i64) -> PathBuf {
    archive
        .join(STATE_DIR)
        .join(format!("checkpoint-{hour_ms}.bin.zst"))
}

/// Checkpoints on disk, oldest first.
pub fn list(archive: &Path) -> Vec<(i64, PathBuf)> {
    let mut out: Vec<(i64, PathBuf)> = std::fs::read_dir(archive.join(STATE_DIR))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let h = name
                        .strip_prefix("checkpoint-")?
                        .strip_suffix(".bin.zst")?
                        .parse()
                        .ok()?;
                    Some((h, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub fn save(archive: &Path, cp: &Checkpoint) -> anyhow::Result<()> {
    std::fs::create_dir_all(archive.join(STATE_DIR))?;
    let json = serde_json::to_vec(cp)?;
    let z = zstd::encode_all(json.as_slice(), 3)?;
    let p = path(archive, cp.hour_ms);
    write_atomic(&p, &z).with_context(|| format!("write {}", p.display()))?;
    // The new checkpoint is already durable at this point: a failure to prune an old one is
    // logged, not propagated, so `save`'s `Err` always means "the new checkpoint may be missing,"
    // never "cleanup hiccup." A leftover old checkpoint self-heals on the next successful `save`.
    let all = list(archive);
    for (_, old) in all.iter().take(all.len().saturating_sub(KEEP)) {
        if let Err(e) = std::fs::remove_file(old) {
            eprintln!(
                "compact: could not prune old checkpoint {} ({e:#}); leaving it",
                old.display()
            );
        }
    }
    Ok(())
}

fn read(p: &Path) -> anyhow::Result<Checkpoint> {
    let z = std::fs::read(p)?;
    Ok(serde_json::from_slice(&zstd::decode_all(z.as_slice())?)?)
}

/// `Ok(None)` only when `_state/` holds no checkpoint file at all. If checkpoint files exist but
/// none is readable, `Err` names every file and its error: the caller must never silently start
/// fresh after a double corruption (that would re-emit or skip already-processed hours).
pub fn load_newest(archive: &Path) -> anyhow::Result<Option<Checkpoint>> {
    let all = list(archive);
    let mut errors = Vec::new();
    for (_, p) in all.iter().rev() {
        match read(p) {
            Ok(cp) => return Ok(Some(cp)),
            Err(e) => {
                eprintln!(
                    "compact: checkpoint {} unreadable ({e:#}); trying the older one",
                    p.display()
                );
                errors.push(format!("{}: {e:#}", p.display()));
            }
        }
    }
    if errors.is_empty() {
        Ok(None)
    } else {
        anyhow::bail!(
            "every checkpoint in {} is unreadable: {}",
            archive.join(STATE_DIR).display(),
            errors.join("; ")
        )
    }
}
