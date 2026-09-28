//! `compact bench <HOUR_FILE> [--next <HOUR_FILE>]` (spec 5b §8): compact one raw hour with no
//! database (no l2 windows), then do what the service does after it: save the checkpoint, drop the
//! reducer, load the checkpoint back and compact the next hour (the same one again when none is
//! given) from the loaded state. Prints the first hour's time, rows, bytes per table, a per-day
//! projection, the checkpoint's size, the second hour's time and the peak memory of all of it.
//! The deploy runbook runs it in the release image with --cpus 0.25 --memory 256m.
use crate::config::Config;
use crate::hour::{HourJob, Source, process_hour};
use crate::reduce::Reducer;
use crate::state::{Checkpoint, load_newest, save};
use anyhow::Context;
use std::path::Path;

/// Peak resident memory of this process (Linux VmHWM), in kB.
pub fn peak_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// The job for `<raw>/YYYY-MM-DD/HH.jsonl.zst`, and its "YYYY-MM-DD/HH.jsonl.zst".
fn job_for(hour_file: &Path, out: &Path) -> anyhow::Result<(HourJob, String)> {
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
    let job = HourJob {
        hour_ms,
        raw_dir: raw.to_path_buf(),
        archive_dir: out.to_path_buf(),
        source: Source::File(hour_file.to_path_buf()),
        windows: Vec::new(),
        stop_before_publish: false,
    };
    Ok((job, format!("{day}/{name}")))
}

pub fn bench(
    cfg: &Config,
    hour_file: &Path,
    next: Option<&Path>,
    out: &Path,
) -> anyhow::Result<String> {
    let (job, label) = job_for(hour_file, out)?;
    let (job2, label2) = match next {
        Some(n) => job_for(n, out)?,
        None => (job.clone(), label.clone()),
    };
    std::fs::create_dir_all(out)?;
    let t0 = std::time::Instant::now();
    let mut red = Reducer::new(cfg.reduce());
    let r = process_hour(&mut red, &job)?.context("not published")?;
    let ms = t0.elapsed().as_millis();
    // the service's checkpoint, then a restart from it
    let cp = Checkpoint {
        hour_ms: job.hour_ms,
        report: r.clone(),
        reducer: red.state(),
    };
    save(out, &cp)?;
    drop(cp);
    drop(red);
    let cp = load_newest(out)?.context("the checkpoint just saved is missing")?;
    let cp_bytes = crate::state::list(out)
        .iter()
        .find(|(h, _)| *h == job.hour_ms)
        .and_then(|(_, p)| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len());
    let mut red = Reducer::from_state(cfg.reduce(), cp.reducer)?;
    let t1 = std::time::Instant::now();
    process_hour(&mut red, &job2)?.context("not published")?;
    let ms2 = t1.elapsed().as_millis();
    let per_day = (r.bytes.tokens + r.bytes.tob + r.bytes.depth + r.bytes.gaps).saturating_mul(24);
    Ok(format!(
        "bench: {label} {ms} ms, lines={} bad_lines={} universe_missing={} late_rows={}, tob={} depth={} gaps={} tokens={}, bytes tob={} depth={} gaps={} tokens={}, per_day_bytes={per_day} (l2 not measured: no episodes), checkpoint_bytes={cp_bytes}, then {label2} from the loaded checkpoint {ms2} ms, peak_rss_kb={}",
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
