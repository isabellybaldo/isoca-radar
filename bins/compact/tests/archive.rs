#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::*;
use compact::pq::{num_rows, read_all};
use compact::rows::*;
use fixture::*;
use std::path::Path;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn a_complete_day_merges_into_daily_files_with_a_manifest_and_drops_the_parts() {
    let (raw, arch) = (tmp("merge-raw"), tmp("merge-arch"));
    let reports = build_day(&raw, &arch, T0, &[5]);
    let c = cfg(&raw, &arch);
    let ms = merge_ready(&arch, &c, "cfgsha").unwrap();
    assert_eq!(ms.len(), 1);
    let m = &ms[0];
    let day = day_dir(&arch, T0);
    assert_eq!(
        names(&day),
        vec![
            "depth.parquet",
            "gaps.parquet",
            "l2.parquet",
            MANIFEST,
            "tob.parquet",
            "tokens.parquet"
        ]
    );
    assert_eq!(m.day, "2026-09-23");
    assert_eq!(m.config_sha256, "cfgsha");
    assert_eq!(m.hours, reports);
    assert_eq!(m.hours[5].status, "lost");
    assert_eq!(m.truncated_windows, vec![format!("live-1-{T0}")]);
    assert_eq!(
        m.ts_clamped,
        reports.iter().map(|r| r.stats.ts_clamped).sum::<u64>(),
        "R4: clamps are counted per day"
    );
    assert_eq!(
        m.late_rows,
        reports.iter().map(|r| r.late_rows).sum::<u64>()
    );
    assert_eq!(
        m.totals.tob,
        reports.iter().map(|r| r.rows.tob).sum::<u64>()
    );
    assert_eq!(num_rows(&daily_path(&day, "tob")).unwrap(), m.totals.tob);
    assert_eq!(num_rows(&daily_path(&day, "tokens")).unwrap(), 2);
    let gaps = read_all::<GapRow>(&daily_path(&day, "gaps")).unwrap();
    assert!(gaps.contains(&GapRow {
        start_ms: T0 + 5 * H,
        end_ms: T0 + 6 * H,
        shard: None,
        reason: reason::RAW_HOUR_LOST.into()
    }));
    let written: Manifest =
        serde_json::from_slice(&std::fs::read(day.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(&written, m);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_day_missing_an_hour_is_not_merged() {
    let (raw, arch) = (tmp("partial-raw"), tmp("partial-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    std::fs::remove_file(day.join("23.hour.json")).unwrap();
    assert!(!complete(&day));
    assert!(
        merge_ready(&arch, &cfg(&raw, &arch), "s")
            .unwrap()
            .is_empty()
    );
    assert!(!merged(&day));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_merge_interrupted_at_any_point_is_finished_by_recover() {
    let (raw, arch) = (tmp("recover-raw"), tmp("recover-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    let copy = tmp("recover-copy");
    for n in names(&day) {
        std::fs::copy(day.join(&n), copy.join(&n)).unwrap();
    }
    let c = cfg(&raw, &arch);
    let first = merge_ready(&arch, &c, "s").unwrap().remove(0);
    // crash after the daily files were renamed, before the manifest: parts come back, no manifest
    std::fs::remove_file(day.join(MANIFEST)).unwrap();
    for n in names(&copy) {
        std::fs::copy(copy.join(&n), day.join(&n)).unwrap();
    }
    let again = recover(&arch, &c, "s").unwrap();
    assert_eq!(
        again,
        vec![first.clone()],
        "merged again from its parts, same manifest"
    );
    // crash after the manifest, before the parts were deleted: recover only cleans up
    std::fs::copy(copy.join("07.tob.parquet"), day.join("07.tob.parquet")).unwrap();
    std::fs::write(day.join("tob.parquet.tmp"), b"half").unwrap();
    assert!(recover(&arch, &c, "s").unwrap().is_empty());
    assert_eq!(
        names(&day),
        vec![
            "depth.parquet",
            "gaps.parquet",
            "l2.parquet",
            MANIFEST,
            "tob.parquet",
            "tokens.parquet"
        ]
    );
    for d in [&raw, &arch, &copy] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn over_budget_the_oldest_merged_day_goes_first_and_oldest_day_moves() {
    let arch = tmp("budget");
    let mk = |day: &str, merged: bool| {
        let d = arch.join(day);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("tob.parquet"), vec![0u8; 1_000]).unwrap();
        if merged {
            std::fs::write(d.join(MANIFEST), b"{}").unwrap();
        }
    };
    mk("2026-09-20", true);
    mk("2026-09-21", true);
    mk("2026-09-22", true);
    mk("2026-09-23", false); // today, still hourly: never deleted by the budget
    std::fs::create_dir_all(arch.join("_state")).unwrap();
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-20"));
    let gone = enforce_budget(&arch, 2_100).unwrap();
    assert_eq!(
        gone.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(),
        vec!["2026-09-20", "2026-09-21"]
    );
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-22"));
    assert!(archive_bytes(&arch) <= 2_100);
    let none_left = enforce_budget(&arch, 0).unwrap();
    assert_eq!(none_left.len(), 1, "only merged days go");
    assert!(arch.join("2026-09-23").exists());
    assert!(
        arch.join("_state").exists(),
        "the checkpoints are never budgeted away"
    );
    std::fs::remove_dir_all(&arch).ok();
}

// ---- beyond the brief: row order, more crash points, the unmerged-only budget ----

/// Day T0 from `three_hours` (books, a disconnect, an l2 window in hour 1), hours 3..=23 Stopped.
fn three_hour_day(raw: &Path, arch: &Path) {
    use compact::hour::{Source, process_hour};
    three_hours(raw);
    let mut red = compact::reduce::Reducer::new(rcfg());
    for i in 0..24 {
        let mut job = hour_job(raw, arch, i);
        if i >= 3 {
            job.source = Source::Stopped;
        }
        process_hour(&mut red, &job).unwrap().unwrap();
    }
}

fn concat_parts<R: compact::pq::Table>(day: &Path) -> Vec<R> {
    (0..24u32)
        .flat_map(|hh| read_all::<R>(&part_path(day, hh, R::NAME)).unwrap())
        .collect()
}

#[test]
fn the_daily_files_are_the_hour_parts_concatenated_in_order_never_resorted() {
    let (raw, arch) = (tmp("order-raw"), tmp("order-arch"));
    three_hour_day(&raw, &arch);
    let day = day_dir(&arch, T0);
    let tob: Vec<TobRow> = concat_parts(&day);
    let l2: Vec<L2Row> = concat_parts(&day);
    let depth: Vec<DepthRow> = concat_parts(&day);
    let gaps: Vec<GapRow> = concat_parts(&day);
    let tokens = read_all::<TokenRow>(&part_path(&day, 23, "tokens")).unwrap();
    assert!(!l2.is_empty(), "the fixture has an l2 window");
    let tob_hours: std::collections::BTreeSet<i64> =
        tob.iter().map(|r| r.recv_ms.div_euclid(H)).collect();
    assert!(
        tob_hours.len() >= 2,
        "tob rows come from several hours: {tob_hours:?}"
    );
    let m = merge_ready(&arch, &cfg(&raw, &arch), "s")
        .unwrap()
        .remove(0);
    assert_eq!(read_all::<TobRow>(&daily_path(&day, "tob")).unwrap(), tob);
    assert_eq!(read_all::<L2Row>(&daily_path(&day, "l2")).unwrap(), l2);
    assert_eq!(
        read_all::<DepthRow>(&daily_path(&day, "depth")).unwrap(),
        depth
    );
    assert_eq!(read_all::<GapRow>(&daily_path(&day, "gaps")).unwrap(), gaps);
    assert_eq!(
        read_all::<TokenRow>(&daily_path(&day, "tokens")).unwrap(),
        tokens
    );
    assert_eq!(m.totals.l2, u64::try_from(l2.len()).unwrap());
    assert_eq!(m.truncated_windows, vec!["ep".to_string()]);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_crash_while_staging_or_publishing_the_daily_files_is_merged_again() {
    let (raw, arch) = (tmp("stage-raw"), tmp("stage-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    let c = cfg(&raw, &arch);
    // a half-written daily file, a stale published one, and an hour's temp part: no manifest yet
    std::fs::write(day.join("l2.parquet.tmp"), b"half").unwrap();
    std::fs::write(daily_path(&day, "tob"), b"stale").unwrap();
    std::fs::write(day.join("04.tob.parquet.tmp"), b"half").unwrap();
    let ms = recover(&arch, &c, "s").unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!(
        names(&day),
        vec![
            "depth.parquet",
            "gaps.parquet",
            "l2.parquet",
            MANIFEST,
            "tob.parquet",
            "tokens.parquet"
        ]
    );
    assert_eq!(
        num_rows(&daily_path(&day, "tob")).unwrap(),
        ms[0].totals.tob
    );
    // idempotent: a second recover finds nothing to do and changes nothing
    let before = day_files(&arch);
    assert!(recover(&arch, &c, "s").unwrap().is_empty());
    assert_eq!(day_files(&arch), before);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn recover_never_deletes_the_parts_of_an_unmerged_day() {
    let (raw, arch) = (tmp("unmerged-raw"), tmp("unmerged-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    // hour 23 failed mid-way: its sidecar is missing and its temp parts are left behind
    std::fs::remove_file(sidecar_path(&day, 23)).unwrap();
    std::fs::write(day.join("23.tob.parquet.tmp"), b"half").unwrap();
    let parts: Vec<String> = names(&day)
        .into_iter()
        .filter(|n| !n.ends_with(".tmp"))
        .collect();
    assert!(recover(&arch, &cfg(&raw, &arch), "s").unwrap().is_empty());
    assert_eq!(names(&day), parts, "only the stale temp file goes");
    assert!(!merged(&day));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_manifest_that_does_not_verify_never_lets_recover_delete_the_parts() {
    let (raw, arch) = (tmp("badman-raw"), tmp("badman-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    let c = cfg(&raw, &arch);
    // a manifest that doesn't parse beside a complete day: the day is merged again from its parts
    std::fs::write(day.join(MANIFEST), b"{").unwrap();
    let ms = recover(&arch, &c, "s").unwrap();
    assert_eq!(ms.len(), 1, "merged again");
    assert_eq!(
        names(&day),
        vec![
            "depth.parquet",
            "gaps.parquet",
            "l2.parquet",
            MANIFEST,
            "tob.parquet",
            "tokens.parquet"
        ]
    );
    // a manifest whose totals disagree with the daily files, beside an incomplete set of parts:
    // nothing to merge from, so everything stays
    let (raw2, arch2) = (tmp("badman-raw2"), tmp("badman-arch2"));
    build_day(&raw2, &arch2, T0, &[]);
    let day2 = day_dir(&arch2, T0);
    let mut m = ms[0].clone();
    m.totals.tob += 1;
    std::fs::write(day2.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
    std::fs::remove_file(sidecar_path(&day2, 3)).unwrap();
    let before = names(&day2);
    assert!(
        recover(&arch2, &cfg(&raw2, &arch2), "s")
            .unwrap()
            .is_empty()
    );
    assert_eq!(names(&day2), before);
    for d in [&raw, &arch, &raw2, &arch2] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn over_budget_with_only_unmerged_data_nothing_is_deleted() {
    let arch = tmp("budget-unmerged");
    for d in ["2026-09-22", "2026-09-23"] {
        std::fs::create_dir_all(arch.join(d)).unwrap();
        std::fs::write(arch.join(d).join("00.tob.parquet"), vec![0u8; 1_000]).unwrap();
    }
    std::fs::create_dir_all(arch.join("_state")).unwrap();
    std::fs::write(
        arch.join("_state").join("checkpoint-1.bin.zst"),
        vec![0u8; 500],
    )
    .unwrap();
    assert!(enforce_budget(&arch, 100).unwrap().is_empty());
    assert_eq!(archive_bytes(&arch), 2_500);
    assert_eq!(names(&arch), vec!["2026-09-22", "2026-09-23", "_state"]);
    std::fs::remove_dir_all(&arch).ok();
}
