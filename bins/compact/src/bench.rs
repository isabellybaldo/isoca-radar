//! `compact bench <HOUR_FILE> [--next <HOUR_FILE>] [--from <DIR>] [--episodes <FILE>]` (spec 5b
//! §8): compact one raw hour with no database, cold or from the newest checkpoint in
//! `<DIR>/_state` (`--from`, a copy of the live `_state`), with l2 windows from `--episodes` (the
//! service's episode query exported by psql; none without it), then do what the service does after
//! it: save the checkpoint, drop the reducer, load the checkpoint back and compact the next hour
//! (the same one again when none is given) from the loaded state. Prints the first hour's time, rows, bytes per table, a per-day
//! projection, the checkpoint's size, the second hour's time and the peak memory of all of it.
//! The deploy runbook runs it in the release image with --cpus 0.25 --memory 512m and the
//! service's MALLOC_ARENA_MAX=1 (compose.yml): each hour runs on its own thread, as in the service.
use crate::config::Config;
use crate::hour::{HourJob, HourReport, Source, process_hour};
use crate::reduce::Reducer;
use crate::state::{Checkpoint, load_newest, save};
use crate::windows::windows_for_hour;
use anyhow::Context;
use std::path::Path;
use store::CompactEpisode;

/// Peak resident memory of this process (Linux VmHWM), in kB.
pub fn peak_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// Like the service: the reducer is loaded on the calling thread and each hour runs on another
/// (`spawn_blocking` there), so the allocator sees the same threads as in production.
fn hour_on_thread(red: &mut Reducer, job: &HourJob) -> anyhow::Result<Option<HourReport>> {
    std::thread::scope(|s| s.spawn(|| process_hour(red, job)).join())
        .map_err(|_| anyhow::anyhow!("the hour thread panicked"))?
}

fn kb(v: Option<u64>) -> String {
    v.map_or_else(|| "n/a".to_string(), |k| k.to_string())
}

/// Episodes as `psql -tA` prints the service's query: `episode_id|event_id|opened_ms|closed_ms`,
/// closed_ms empty while the episode is still open.
pub fn parse_episodes(txt: &str) -> anyhow::Result<Vec<CompactEpisode>> {
    let mut out = Vec::new();
    for (i, l) in txt.lines().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        let ep = (|| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('|').collect();
            anyhow::ensure!(f.len() == 4, "expected 4 fields, got {}", f.len());
            Ok(CompactEpisode {
                episode_id: f[0].to_string(),
                event_id: f[1].to_string(),
                opened_ms: f[2].parse()?,
                closed_ms: if f[3].is_empty() {
                    None
                } else {
                    Some(f[3].parse()?)
                },
            })
        })()
        .with_context(|| format!("episodes line {}: {l}", i + 1))?;
        out.push(ep);
    }
    Ok(out)
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
    from: Option<&Path>,
    episodes: &[CompactEpisode],
) -> anyhow::Result<String> {
    let (mut job, label) = job_for(hour_file, out)?;
    let (mut job2, label2) = match next {
        Some(n) => job_for(n, out)?,
        None => (job.clone(), label.clone()),
    };
    job.windows = windows_for_hour(episodes, job.hour_ms, cfg);
    job2.windows = windows_for_hour(episodes, job2.hour_ms, cfg);
    std::fs::create_dir_all(out)?;
    let start = match from {
        Some(d) => {
            let cp =
                load_newest(d)?.with_context(|| format!("no checkpoint in {}", d.display()))?;
            Reducer::from_state(cfg.reduce(), cp.reducer)?
        }
        None => Reducer::new(cfg.reduce()),
    };
    let rss_start = peak_rss_kb();
    let t0 = std::time::Instant::now();
    let mut red = start;
    let r = hour_on_thread(&mut red, &job)?.context("not published")?;
    let ms = t0.elapsed().as_millis();
    let rss_hour = peak_rss_kb();
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
    hour_on_thread(&mut red, &job2)?.context("not published")?;
    let ms2 = t1.elapsed().as_millis();
    let per_day = (r.bytes.tokens + r.bytes.tob + r.bytes.depth + r.bytes.gaps).saturating_mul(24);
    Ok(format!(
        "bench: {label} {ms} ms, lines={} bad_lines={} universe_missing={} late_rows={}, tob={} depth={} gaps={} tokens={}, bytes tob={} depth={} gaps={} tokens={}, per_day_bytes={per_day}, windows={} l2={}, peak_rss_kb after start={} after the hour={}, checkpoint_bytes={cp_bytes}, then {label2} from the loaded checkpoint {ms2} ms, peak_rss_kb={}",
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
        job.windows.len(),
        r.rows.l2,
        kb(rss_start),
        kb(rss_hour),
        kb(peak_rss_kb())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodes_come_from_a_psql_unaligned_export() {
        let txt = "ep-1|ev-1|1790650000000|1790650004000\nep-2|ev-2|1790650100000|\n\n";
        let eps = parse_episodes(txt).unwrap();
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].episode_id, "ep-1");
        assert_eq!(eps[0].event_id, "ev-1");
        assert_eq!(eps[0].opened_ms, 1_790_650_000_000);
        assert_eq!(eps[0].closed_ms, Some(1_790_650_004_000));
        assert_eq!(eps[1].closed_ms, None); // still open
    }

    #[test]
    fn a_malformed_episode_line_names_its_line() {
        let e = parse_episodes("ep-1|ev-1|soon|\n").unwrap_err();
        assert!(format!("{e:#}").contains("line 1"), "{e:#}");
    }
}
