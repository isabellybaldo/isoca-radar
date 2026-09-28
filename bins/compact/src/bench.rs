//! `compact bench <HOUR_FILE>` (spec 5b §8): compact one raw hour with no database (no l2 windows)
//! and print the time, rows, bytes per table, a per-day projection and peak memory. The deploy
//! runbook runs it in the release image with --cpus 0.25 --memory 256m.
use crate::config::Config;
use crate::hour::{HourJob, Source, process_hour};
use crate::reduce::Reducer;
use anyhow::Context;
use std::path::Path;

/// Peak resident memory of this process (Linux VmHWM), in kB.
pub fn peak_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

pub fn bench(cfg: &Config, hour_file: &Path, out: &Path) -> anyhow::Result<String> {
    let name = hour_file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let day_dir = hour_file
        .parent()
        .context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    let day = day_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let raw = day_dir
        .parent()
        .context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    let hour_ms = tape::files::hour_start_ms(&day, &name)
        .context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    std::fs::create_dir_all(out)?;
    let job = HourJob {
        hour_ms,
        raw_dir: raw.to_path_buf(),
        archive_dir: out.to_path_buf(),
        source: Source::File(hour_file.to_path_buf()),
        windows: Vec::new(),
        stop_before_publish: false,
    };
    let t0 = std::time::Instant::now();
    let r = process_hour(&mut Reducer::new(cfg.reduce()), &job)?.context("not published")?;
    let ms = t0.elapsed().as_millis();
    let per_day = (r.bytes.tokens + r.bytes.tob + r.bytes.depth + r.bytes.gaps).saturating_mul(24);
    Ok(format!(
        "bench: {day}/{name} {ms} ms, lines={} bad_lines={} universe_missing={} late_rows={}, tob={} depth={} gaps={} tokens={}, bytes tob={} depth={} gaps={} tokens={}, per_day_bytes={per_day} (l2 not measured: no episodes), peak_rss_kb={}",
        r.stats.lines,
        r.stats.bad_lines,
        r.stats.universe_missing,
        r.late_rows,
        r.rows.tob,
        r.rows.depth,
        r.rows.gaps,
        r.rows.tokens,
        r.bytes.tob,
        r.bytes.depth,
        r.bytes.gaps,
        r.bytes.tokens,
        peak_rss_kb().map_or_else(|| "n/a".to_string(), |k| k.to_string())
    ))
}
