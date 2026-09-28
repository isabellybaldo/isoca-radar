#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::*;
use compact::pq::{num_rows, read_all};
use compact::rows::*;
use fixture::*;

/// A checkpoint past every test day: merging is allowed.
const ALL: Option<i64> = Some(i64::MAX);
use std::path::Path;

/// The manifests of a pass that must not have failed on any day.
fn ok(m: Merged) -> Vec<Manifest> {
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    m.manifests
}

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
    let ms = ok(merge_ready(&arch, &c, "cfgsha", ALL));
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
    assert!(ok(merge_ready(&arch, &cfg(&raw, &arch), "s", ALL)).is_empty());
    assert!(!merged(&day));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_day_merges_only_once_its_hour_23_is_checkpointed() {
    let (raw, arch) = (tmp("gate-raw"), tmp("gate-arch"));
    build_day(&raw, &arch, T0, &[]);
    let (c, day) = (cfg(&raw, &arch), day_dir(&arch, T0));
    for through in [None, Some(T0 + 22 * H)] {
        assert!(
            ok(merge_ready(&arch, &c, "s", through)).is_empty(),
            "{through:?}"
        );
        assert!(
            ok(recover(&arch, &c, "s", through)).is_empty(),
            "{through:?}"
        );
        assert!(!merged(&day));
    }
    assert!(!may_merge("2026-09-23", Some(T0 + 23 * H - 1)));
    assert_eq!(ok(merge_ready(&arch, &c, "s", Some(T0 + 23 * H))).len(), 1);
    assert!(merged(&day));
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
    let first = ok(merge_ready(&arch, &c, "s", ALL)).remove(0);
    // crash after the daily files were renamed, before the manifest: parts come back, no manifest
    std::fs::remove_file(day.join(MANIFEST)).unwrap();
    for n in names(&copy) {
        std::fs::copy(copy.join(&n), day.join(&n)).unwrap();
    }
    let again = ok(recover(&arch, &c, "s", ALL));
    assert_eq!(
        again,
        vec![first.clone()],
        "merged again from its parts, same manifest"
    );
    // crash after the manifest, before the parts were deleted: recover only cleans up
    std::fs::copy(copy.join("07.tob.parquet"), day.join("07.tob.parquet")).unwrap();
    std::fs::write(day.join("tob.parquet.tmp"), b"half").unwrap();
    assert!(ok(recover(&arch, &c, "s", ALL)).is_empty());
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
    let gone = enforce_budget(&arch, 2_100).unwrap().deleted;
    assert_eq!(
        gone.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(),
        vec!["2026-09-20", "2026-09-21"]
    );
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-22"));
    assert!(archive_bytes(&arch) <= 2_100);
    let none_left = enforce_budget(&arch, 0).unwrap().deleted;
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
    let m = ok(merge_ready(&arch, &cfg(&raw, &arch), "s", ALL)).remove(0);
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
    let ms = ok(recover(&arch, &c, "s", ALL));
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
    assert!(ok(recover(&arch, &c, "s", ALL)).is_empty());
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
    assert!(ok(recover(&arch, &cfg(&raw, &arch), "s", ALL)).is_empty());
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
    let ms = ok(recover(&arch, &c, "s", ALL));
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
    let r = recover(&arch2, &cfg(&raw2, &arch2), "s", ALL);
    assert!(r.manifests.is_empty());
    assert_eq!(
        r.failed.len(),
        1,
        "reported, not silently skipped: {:?}",
        r.failed
    );
    assert_eq!(r.failed[0].0, "2026-09-23");
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
    let b = enforce_budget(&arch, 100).unwrap();
    assert!(b.deleted.is_empty());
    assert_eq!(
        b.stuck_at,
        Some(2_500),
        "the caller learns it is stuck over budget"
    );
    assert_eq!(archive_bytes(&arch), 2_500);
    assert_eq!(names(&arch), vec!["2026-09-22", "2026-09-23", "_state"]);
    std::fs::remove_dir_all(&arch).ok();
}

// ---- fix round 1: the dictionary, per-day failures, the budget's rename ----

fn stopped_cold(raw: &Path, arch: &Path, hours: &[i64]) {
    use compact::hour::{HourJob, Source, process_hour};
    let mut cold = compact::reduce::Reducer::new(rcfg());
    for hh in hours {
        let job = HourJob {
            hour_ms: T0 + hh * H,
            raw_dir: raw.to_path_buf(),
            archive_dir: arch.to_path_buf(),
            source: Source::Stopped,
            windows: vec![],
            stop_before_publish: false,
        };
        process_hour(&mut cold, &job).unwrap().unwrap();
    }
}

#[test]
fn a_cold_start_mid_day_fails_the_merge_and_keeps_every_hourly_part() {
    let (raw, arch) = (tmp("cold-raw"), tmp("cold-arch"));
    build_day(&raw, &arch, T0, &[]);
    // no checkpoint: hours 22 and 23 redone by a fresh reducer, whose dictionary is empty
    stopped_cold(&raw, &arch, &[22, 23]);
    let day = day_dir(&arch, T0);
    let before = names(&day);
    let r = merge_ready(&arch, &cfg(&raw, &arch), "s", ALL);
    assert!(r.manifests.is_empty());
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].0, "2026-09-23");
    assert!(
        r.failed[0].1.contains("not a prefix of 23.tokens.parquet"),
        "{}",
        r.failed[0].1
    );
    assert_eq!(names(&day), before, "nothing published, nothing deleted");
    assert!(!merged(&day));
    // and recover reports the same, keeping everything
    let r = recover(&arch, &cfg(&raw, &arch), "s", ALL);
    assert_eq!(r.failed.len(), 1);
    assert_eq!(names(&day), before);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_token_idx_outside_the_days_dictionary_fails_the_merge() {
    let (raw, arch) = (tmp("idx-raw"), tmp("idx-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    // every hour's dictionary cut to its first token: consistent prefixes, but tob uses token 1
    let first = read_all::<TokenRow>(&part_path(&day, 0, "tokens"))
        .unwrap()
        .remove(0);
    for hh in 0..24u32 {
        let mut w =
            compact::pq::PartWriter::<TokenRow>::create(part_path(&day, hh, "tokens")).unwrap();
        w.push(first.clone()).unwrap();
        w.close().unwrap().publish().unwrap();
    }
    let before = names(&day);
    let r = merge_ready(&arch, &cfg(&raw, &arch), "s", ALL);
    assert_eq!(r.failed.len(), 1);
    assert!(
        r.failed[0].1.contains("outside the day's dictionary"),
        "{}",
        r.failed[0].1
    );
    assert_eq!(names(&day), before);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn one_bad_day_is_reported_and_a_healthy_later_day_still_merges() {
    let (raw, arch) = (tmp("badday-raw"), tmp("badday-arch"));
    build_day(&raw, &arch, T0, &[]);
    build_day(&raw, &arch, T0 + 24 * H, &[]);
    let (d1, d2) = (day_dir(&arch, T0), day_dir(&arch, T0 + 24 * H));
    std::fs::write(part_path(&d1, 7, "tob"), b"garbage").unwrap();
    let before = names(&d1);
    for _ in 0..2 {
        // every restart: the bad day fails the same way, the good one is (or stays) merged
        let r = recover(&arch, &cfg(&raw, &arch), "s", ALL);
        assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
        assert_eq!(r.failed[0].0, "2026-09-23");
        assert!(
            r.failed[0].1.contains("07.tob.parquet"),
            "{}",
            r.failed[0].1
        );
        assert_eq!(names(&d1), before, "nothing of the bad day is deleted");
        assert!(merged(&d2));
    }
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_manifest_that_does_not_verify_beside_a_day_that_lost_parts_is_reported_and_kept() {
    let (raw, arch) = (tmp("lostparts-raw"), tmp("lostparts-arch"));
    build_day(&raw, &arch, T0, &[]);
    let d1 = day_dir(&arch, T0);
    let keep = tmp("lostparts-keep");
    for n in names(&d1) {
        std::fs::copy(d1.join(&n), keep.join(&n)).unwrap();
    }
    ok(merge_ready(&arch, &cfg(&raw, &arch), "s", ALL));
    // crash part-way through deleting the parts (00.depth already gone, 00.hour.json still
    // there, so the day looks complete), and the manifest does not verify
    for n in names(&keep) {
        std::fs::copy(keep.join(&n), d1.join(&n)).unwrap();
    }
    std::fs::remove_file(part_path(&d1, 0, "depth")).unwrap();
    std::fs::write(d1.join(MANIFEST), b"{").unwrap();
    build_day(&raw, &arch, T0 + 24 * H, &[]);
    let before = names(&d1);
    let r = recover(&arch, &cfg(&raw, &arch), "s", ALL);
    assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
    assert_eq!(r.failed[0].0, "2026-09-23");
    assert_eq!(names(&d1), before, "nothing deleted");
    assert_eq!(r.manifests.len(), 1);
    assert_eq!(r.manifests[0].day, "2026-09-24", "the other day is fine");
    for d in [&raw, &arch, &keep] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn a_crash_after_the_budget_renamed_a_day_leaves_nothing_day_shaped() {
    let (raw, arch) = (tmp("deleting-raw"), tmp("deleting-arch"));
    // the budget renamed 2026-09-20 and crashed part-way through removing it
    let doomed = arch.join(".deleting-2026-09-20");
    std::fs::create_dir_all(&doomed).unwrap();
    std::fs::write(doomed.join("tob.parquet"), vec![0u8; 1_000]).unwrap();
    std::fs::create_dir_all(arch.join("2026-09-21")).unwrap();
    std::fs::write(arch.join("2026-09-21").join(MANIFEST), b"{}").unwrap();
    assert_eq!(days(&arch).len(), 1);
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-21"));
    ok(recover(&arch, &cfg(&raw, &arch), "s", ALL));
    assert_eq!(names(&arch), vec!["2026-09-21"]);
    // and the budget's own deletions go through the rename, leaving nothing behind
    let b = enforce_budget(&arch, 0).unwrap();
    assert_eq!(
        b.deleted
            .iter()
            .map(|(d, _)| d.as_str())
            .collect::<Vec<_>>(),
        vec!["2026-09-21"]
    );
    assert!(names(&arch).is_empty());
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
