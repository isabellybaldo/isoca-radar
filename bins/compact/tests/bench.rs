#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::{day_dir, part_path};
use compact::pq::read_all;
use compact::rows::TobRow;
use fixture::*;
use std::path::Path;

fn bench_hours(raw: &Path) {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    write_record(raw, &r);
    write_hour(
        raw,
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
    write_hour(
        raw,
        T0 + 14 * H,
        &[frame(
            T0 + 14 * H + 1_000,
            0,
            change("y1", T0 + 14 * H + 995, "BUY", "0.41", "2", "0.41", "1"),
        )],
    );
}

fn config() -> compact::config::Config {
    compact::config::Config::parse(include_str!("../../../config/compact.toml"))
        .unwrap()
        .0
}

#[test]
fn bench_reports_time_rows_and_bytes_for_one_hour_then_its_checkpoint_round_trip() {
    let (raw, out) = (tmp("bench-raw"), tmp("bench-out"));
    bench_hours(&raw);
    let line = compact::bench::bench(
        &config(),
        &tape::files::hour_path(&raw, T0 + 13 * H),
        None,
        &out,
        None,
        &[],
    )
    .unwrap();
    assert!(
        line.starts_with("bench: 2026-09-23/13.jsonl.zst "),
        "{line}"
    );
    assert!(
        line.contains(" tob=1 ")
            && line.contains("per_day_bytes=")
            && line.contains("then 2026-09-23/13.jsonl.zst from the loaded checkpoint ")
            && line
                .rsplit(", ")
                .next()
                .unwrap()
                .starts_with("peak_rss_kb="),
        "{line}"
    );
    let cp_bytes: u64 = line
        .split("checkpoint_bytes=")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(cp_bytes > 0, "{line}");
    assert_eq!(
        compact::state::load_newest(&out).unwrap().unwrap().hour_ms,
        T0 + 13 * H,
        "the checkpoint is saved into --out"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&out).ok();
}

#[test]
fn bench_runs_the_next_hour_from_the_loaded_checkpoint() {
    let (raw, out) = (tmp("bench-next-raw"), tmp("bench-next-out"));
    bench_hours(&raw);
    let line = compact::bench::bench(
        &config(),
        &tape::files::hour_path(&raw, T0 + 13 * H),
        Some(&tape::files::hour_path(&raw, T0 + 14 * H)),
        &out,
        None,
        &[],
    )
    .unwrap();
    assert!(
        line.contains("then 2026-09-23/14.jsonl.zst from the loaded checkpoint "),
        "{line}"
    );
    let tob = read_all::<TobRow>(&part_path(&day_dir(&out, T0), 14, "tob")).unwrap();
    assert_eq!(
        tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.41"))],
        "hour 14's change applies to the book carried by the checkpoint: {tob:?}"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&out).ok();
}

#[test]
fn bench_starts_from_a_live_checkpoint_with_the_episodes_l2_windows() {
    let (raw, live, out) = (
        tmp("bench-from-raw"),
        tmp("bench-from-live"),
        tmp("bench-from-out"),
    );
    bench_hours(&raw);
    // the live _state: hour 13's checkpoint
    compact::bench::bench(
        &config(),
        &tape::files::hour_path(&raw, T0 + 13 * H),
        None,
        &live,
        None,
        &[],
    )
    .unwrap();
    let eps = compact::bench::parse_episodes(&format!(
        "ep-1|e1|{}|{}
",
        T0 + 14 * H + 500,
        T0 + 14 * H + 2_000
    ))
    .unwrap();
    let line = compact::bench::bench(
        &config(),
        &tape::files::hour_path(&raw, T0 + 14 * H),
        None,
        &out,
        Some(&live),
        &eps,
    )
    .unwrap();
    assert!(line.contains(" windows=1 "), "{line}");
    assert!(line.contains("peak_rss_kb after start="), "{line}");
    // cold, hour 14's change has no book (y1's snapshot is in hour 13): no tob row
    assert!(
        line.contains(" tob=1 "),
        "hour 14 starts from the book in --from's checkpoint: {line}"
    );
    eprintln!("{line}");
    for d in [&raw, &live, &out] {
        std::fs::remove_dir_all(d).ok();
    }
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
    let out = tmp("bench-real");
    eprintln!(
        "{}",
        compact::bench::bench(&config(), Path::new(&p), None, &out, None, &[]).unwrap()
    );
    std::fs::remove_dir_all(&out).ok();
}
