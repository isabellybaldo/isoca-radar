#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use fixture::*;
use std::path::Path;

#[test]
fn bench_reports_time_rows_and_bytes_for_one_hour() {
    let (raw, out) = (tmp("bench-raw"), tmp("bench-out"));
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    write_record(&raw, &r);
    write_hour(
        &raw,
        T0 + 13 * H,
        &[
            uni(T0 + 13 * H + 1_000, &r),
            frame(
                T0 + 13 * H + 2_000,
                0,
                book("y1", T0 + 13 * H + 1_990, &[("0.40", "1")], &[]),
            ),
        ],
    );
    let (c, _) =
        compact::config::Config::parse(include_str!("../../../config/compact.toml")).unwrap();
    let line = compact::bench::bench(&c, &tape::files::hour_path(&raw, T0 + 13 * H), &out).unwrap();
    assert!(
        line.starts_with("bench: 2026-09-23/13.jsonl.zst "),
        "{line}"
    );
    assert!(
        line.contains(" tob=1 ")
            && line.contains("per_day_bytes=")
            && line.contains("peak_rss_kb="),
        "{line}"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&out).ok();
}

/// Spec §8 benchmark, by hand: `COMPACT_BENCH_HOUR=<dir>/YYYY-MM-DD/HH.jsonl.zst` (the universe
/// records the hour names in the same tree). Debug build: a correctness run on real data, not the
/// timing (the timing is the release run in the runbook, §1).
#[test]
#[ignore]
fn bench_one_real_hour() {
    let Ok(p) = std::env::var("COMPACT_BENCH_HOUR") else {
        eprintln!("SKIPPED: COMPACT_BENCH_HOUR not set");
        return;
    };
    let (c, _) =
        compact::config::Config::parse(include_str!("../../../config/compact.toml")).unwrap();
    let out = tmp("bench-real");
    eprintln!(
        "{}",
        compact::bench::bench(&c, Path::new(&p), &out).unwrap()
    );
    std::fs::remove_dir_all(&out).ok();
}
