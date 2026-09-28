#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
mod fixture;
use compact::archive::{daily_path, day_dir, merge_day, merged, part_path, sidecar_path};
use compact::pq::read_all;
use compact::rows::*;
use compact::run::{Service, lock_archive, ready};
use fixture::*;
use rust_decimal_macros::dec;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use store::*;
use tape::files::HourFile;

/// Raw hours `present` (hour indexes from T0): hour 0 has a universe and two books, later ones one
/// change each, so each file exists and depends on the books carried over.
fn raw_hours(raw: &Path, present: &[i64]) {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(raw, &r);
    for &i in present {
        let t = T0 + i * H;
        let lines = if i == 0 {
            vec![
                uni(t + 1_000, &r),
                frame(
                    t + 2_000,
                    0,
                    book("y1", t + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
                ),
                frame(t + 2_100, 0, book("y2", t + 2_090, &[("0.20", "3")], &[])),
                frame(
                    t + 700_000,
                    0,
                    change("y1", t + 699_995, "BUY", "0.41", "1", "0.41", "0.45"),
                ),
            ]
        } else {
            vec![frame(
                t + 1_000,
                0,
                change(
                    "y2",
                    t + 995,
                    "BUY",
                    &format!("0.{:02}", 20 + i),
                    "1",
                    &format!("0.{:02}", 20 + i),
                    "1",
                ),
            )]
        };
        write_hour(raw, t, &lines);
    }
}

async fn compact_store(db: &common::TestDb) -> Store {
    Store::connect_compact(db.opts.clone(), 2, 10_000, Some("compact_rw"))
        .await
        .unwrap()
}

fn service(raw: &Path, arch: &Path) -> Service {
    Service::new(cfg(raw, arch), "cfgsha".into())
}

fn hf(i: i64) -> HourFile {
    HourFile {
        start_ms: T0 + i * H,
        path: PathBuf::from(format!("{i}")),
        bytes: 1,
    }
}

#[test]
fn an_hour_is_ready_once_the_next_one_has_closed() {
    assert!(!ready(T0, &[hf(0), hf(1)]), "hour 1 is still being written");
    assert!(ready(T0, &[hf(0), hf(1), hf(2)]));
    assert!(
        ready(T0, &[hf(0), hf(5)]),
        "the scanner skipped hours: 1 and later are closed"
    );
}

#[tokio::test]
async fn the_service_compacts_ready_hours_in_order_and_records_them() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("svc-raw"), tmp("svc-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    let now = T0 + 4 * H;
    assert!(svc.step(&c, now).await.unwrap(), "hour 0");
    assert!(svc.step(&c, now).await.unwrap(), "hour 1");
    assert!(!svc.step(&c, now).await.unwrap(), "hour 2 waits for hour 4");
    assert_eq!(svc.next_hour(), Some(T0 + 2 * H));
    assert_eq!(
        owner.compact_hour_statuses(T0, T0 + 10 * H).await.unwrap(),
        vec![(T0, "done".to_string()), (T0 + H, "done".to_string())]
    );
    let day = day_dir(&arch, T0);
    assert!(
        sidecar_path(&day, 0).exists()
            && sidecar_path(&day, 1).exists()
            && !sidecar_path(&day, 2).exists()
    );
    assert_eq!(compact::state::list(&arch).len(), 2);
    let st = svc.status(now);
    assert_eq!(
        (
            st.lag_hours,
            st.oldest_day.as_deref(),
            st.last_error.as_deref()
        ),
        (2, Some("2026-09-23"), None)
    );
    assert!(st.archive_bytes > 0);
    c.compact_status_put(&st).await.unwrap();
    assert_eq!(owner.compact_status().await.unwrap(), Some(st));
    // a restarted service resumes after its checkpoint, never redoing hour 0 or 1
    raw_hours(&raw, &[4]);
    let mut again = service(&raw, &arch);
    assert!(again.step(&c, now + H).await.unwrap());
    assert_eq!(again.next_hour(), Some(T0 + 3 * H));
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_missing_hour_marked_lost_is_compacted_as_lost() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("lost-raw"), tmp("lost-arch"));
    raw_hours(&raw, &[0, 2, 3]);
    owner.mark_hours_lost(&[T0 + H], T0 + 2 * H).await.unwrap();
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap());
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap());
    assert_eq!(
        owner
            .compact_hour_statuses(T0 + H, T0 + 2 * H)
            .await
            .unwrap(),
        vec![(T0 + H, "lost".to_string())]
    );
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(
        gaps,
        vec![GapRow {
            start_ms: T0 + H,
            end_ms: T0 + 2 * H,
            shard: None,
            reason: reason::RAW_HOUR_LOST.into()
        }]
    );
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_missing_hour_nobody_marked_is_scanner_stopped_and_done() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("stop-raw"), tmp("stop-arch"));
    raw_hours(&raw, &[0, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 0");
    assert!(
        !svc.step(&c, T0 + 4 * H).await.unwrap(),
        "hour 1 never listed, no mark: wait for a mark to land"
    );
    assert!(
        !svc.step(&c, T0 + 4 * H + 299_999).await.unwrap(),
        "still inside the 5-minute grace"
    );
    assert!(
        svc.step(&c, T0 + 4 * H + 300_000).await.unwrap(),
        "then: the scanner was not running"
    );
    assert_eq!(
        owner
            .compact_hour_statuses(T0 + H, T0 + 2 * H)
            .await
            .unwrap(),
        vec![(T0 + H, "done".to_string())]
    );
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(gaps[0].reason, reason::SCANNER_STOPPED);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_file_deleted_before_its_lost_mark_is_compacted_as_lost() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("race-raw"), tmp("race-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(
        svc.step(&c, T0 + 4 * H).await.unwrap(),
        "hour 0; hours 1–3 listed"
    );
    // the scanner deletes hour 1 at its cap; its lost mark has not reached the database yet
    std::fs::remove_file(tape::files::hour_path(&raw, T0 + H)).unwrap();
    assert!(
        svc.step(&c, T0 + 4 * H).await.unwrap(),
        "hour 1 at once: it was listed, so it was deleted"
    );
    assert_eq!(
        owner
            .compact_hour_statuses(T0 + H, T0 + 2 * H)
            .await
            .unwrap(),
        vec![(T0 + H, "lost".to_string())]
    );
    // the mark arrives afterwards: nothing changes, and the loss stays visible
    assert_eq!(
        owner.mark_hours_lost(&[T0 + H], T0 + 4 * H).await.unwrap(),
        0
    );
    assert_eq!(
        owner
            .compact_hour_statuses(T0 + H, T0 + 2 * H)
            .await
            .unwrap(),
        vec![(T0 + H, "lost".to_string())]
    );
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(gaps[0].reason, reason::RAW_HOUR_LOST);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn l2_windows_come_from_live_net_positive_episodes() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("l2-raw"), tmp("l2-arch"));
    raw_hours(&raw, &[0, 1, 2]);
    let run_id = owner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: T0,
            version: "t".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap();
    let e = EpisodeRow {
        episode_id: "live-1-00000001".into(),
        run_id,
        source: "live".into(),
        kind: "no_basket".into(),
        realization: "converter".into(),
        event_id: "e1".into(),
        event_slug: None,
        event_title: None,
        neg_risk_market_id: None,
        pair_id: None,
        category: None,
        n_outcomes: 2,
        n_legs: 2,
        legs_dropped: 0,
        augmented: false,
        incomplete_set: false,
        uncovered_legs: 0,
        opened_server_ms: T0 + 600_000,
        closed_server_ms: Some(T0 + 650_000),
        duration_ms: Some(50_000),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        detect_lag_ms: None,
        n_updates: 1,
        peak_server_ms: T0 + 600_000,
        peak_edge_top: dec!(0.0049),
        peak_edge_top_gross: dec!(0.0137),
        peak_max_size: dec!(83),
        peak_cost_usd: dec!(99.59),
        peak_payout_usd: dec!(100),
        peak_fees_usd: dec!(0.96),
        peak_pnl_net_usd: dec!(0.41),
        peak_pnl_gross_usd: dec!(1.37),
        peak_profit_per_dollar: None,
        pnl_net_budget100_usd: None,
        bottleneck_leg_idx: None,
        bottleneck_notional_usd: None,
        below_min_size: None,
        max_seconds_delay: None,
        days_to_end: None,
        annualized_return: None,
        post_end: None,
        engine_suspect: false,
        stale_transport: false,
        open_lag_ms: None,
        oracle_downgraded: None,
        scanner_version: "t".into(),
        config_hash: "c".into(),
    };
    let losing = EpisodeRow {
        episode_id: "live-1-00000002".into(),
        peak_pnl_net_usd: dec!(-0.1),
        ..e.clone()
    };
    owner
        .write_batch(&Batch {
            episodes: vec![
                EpisodeWrite {
                    row: e,
                    legs: vec![],
                },
                EpisodeWrite {
                    row: losing,
                    legs: vec![],
                },
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 3 * H).await.unwrap());
    let l2 = read_all::<L2Row>(&part_path(&day_dir(&arch, T0), 0, "l2")).unwrap();
    assert!(
        !l2.is_empty(),
        "the window [300 000, 950 000) covers the 700 000 change"
    );
    assert!(
        l2.iter().all(|r| r.episode_id == "live-1-00000001"),
        "only the net-positive episode"
    );
    assert!(l2[0].snapshot);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn the_service_refuses_a_role_that_writes_other_tables() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    assert!(
        compact::refuse_other_writes(&owner.role_writes_outside_compact().await.unwrap()).is_err()
    );
    let c = compact_store(&db).await;
    assert!(compact::refuse_other_writes(&c.role_writes_outside_compact().await.unwrap()).is_ok());
    c.close().await;
    db.drop(owner).await;
}

// ---- rulings after the brief (Task 16 CARRY, Task 18 dispatch) ----

#[test]
fn a_second_run_cannot_lock_the_archive() {
    let arch = tmp("lock-arch");
    let held = lock_archive(&arch).unwrap();
    let e = lock_archive(&arch).unwrap_err().to_string();
    assert!(e.contains("another `compact run`"), "{e}");
    drop(held);
    assert!(
        lock_archive(&arch).is_ok(),
        "the lock goes with the process that held it"
    );
    std::fs::remove_dir_all(&arch).ok();
}

/// The same hour compacted by a service that never failed: its tob rows and sidecar row counts.
async fn clean_hour_1(c: &Store) -> (Vec<TobRow>, compact::hour::RowCounts) {
    let (raw, arch) = (tmp("clean-raw"), tmp("clean-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let mut svc = service(&raw, &arch);
    assert!(svc.step(c, T0 + 4 * H).await.unwrap());
    assert!(svc.step(c, T0 + 4 * H).await.unwrap());
    let day = day_dir(&arch, T0);
    let tob = read_all::<TobRow>(&part_path(&day, 1, "tob")).unwrap();
    let side: compact::hour::HourReport =
        serde_json::from_slice(&std::fs::read(sidecar_path(&day, 1)).unwrap()).unwrap();
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
    (tob, side.rows)
}

#[tokio::test]
async fn an_hour_that_fails_drops_the_reducer_and_is_redone_from_the_checkpoint() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let c = compact_store(&db).await;
    let (want_tob, want_rows) = clean_hour_1(&c).await;
    assert!(!want_tob.is_empty());
    let (raw, arch) = (tmp("fail-raw"), tmp("fail-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 0");
    // hour 1 is read in full, then its sidecar can't be published: the reducer has seen the hour
    let day = day_dir(&arch, T0);
    std::fs::create_dir_all(sidecar_path(&day, 1)).unwrap();
    assert!(svc.step(&c, T0 + 4 * H).await.is_err());
    assert!(
        !svc.loaded(),
        "a reducer left part-way (or past) an hour is never reused"
    );
    assert_eq!(svc.next_hour(), Some(T0 + H), "hour 1 is not done");
    std::fs::remove_dir_all(sidecar_path(&day, 1)).unwrap();
    assert!(
        svc.step(&c, T0 + 4 * H).await.unwrap(),
        "hour 1 again, from hour 0's checkpoint"
    );
    assert_eq!(
        read_all::<TobRow>(&part_path(&day, 1, "tob")).unwrap(),
        want_tob
    );
    let side: compact::hour::HourReport =
        serde_json::from_slice(&std::fs::read(sidecar_path(&day, 1)).unwrap()).unwrap();
    assert_eq!(side.rows, want_rows);
    assert_eq!(
        owner
            .compact_hour_statuses(T0, T0 + 10 * H)
            .await
            .unwrap()
            .len(),
        2
    );
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

const DAY: i64 = 24 * H;

#[tokio::test]
async fn unreadable_checkpoints_stop_compaction_but_not_the_merge_budget_or_status() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch, other) = (
        tmp("corrupt-raw"),
        tmp("corrupt-arch"),
        tmp("corrupt-other"),
    );
    raw_hours(&raw, &[0, 1, 2, 3]);
    // 2026-09-21 merged (the budget may delete it); 2026-09-22 complete but missing a part
    build_day(&other, &arch, T0 - 2 * DAY, &[]);
    merge_day(
        &day_dir(&arch, T0 - 2 * DAY),
        "2026-09-21",
        &cfg(&raw, &arch),
        "cfgsha",
    )
    .unwrap();
    build_day(&other, &arch, T0 - DAY, &[]);
    std::fs::remove_file(part_path(&day_dir(&arch, T0 - DAY), 5, "tob")).unwrap();
    let state = arch.join(compact::state::STATE_DIR);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join(format!("checkpoint-{}.bin.zst", T0 - H)),
        b"garbage",
    )
    .unwrap();
    std::fs::write(
        state.join(format!("checkpoint-{}.bin.zst", T0 - 2 * H)),
        b"garbage",
    )
    .unwrap();
    let c = compact_store(&db).await;
    let mut svc = Service::new(
        compact::config::Config {
            archive_budget_gb: 0,
            ..cfg(&raw, &arch)
        },
        "cfgsha".into(),
    );
    let stop = AtomicBool::new(false);
    for _ in 0..2 {
        let st = svc.poll(&c, || T0 + 4 * H, &stop).await;
        let e = st.last_error.clone().unwrap();
        assert!(
            e.contains("every checkpoint") && e.contains("unreadable"),
            "{e}"
        );
        assert!(
            e.contains("2026-09-22"),
            "the failed merge is reported: {e}"
        );
        assert!(
            e.contains("over budget"),
            "the stuck budget is reported: {e}"
        );
        assert!(!svc.loaded(), "never a cold start over corrupt checkpoints");
        assert!(!day_dir(&arch, T0).exists(), "no hour compacted");
        assert!(
            !day_dir(&arch, T0 - 2 * DAY).exists(),
            "the budget ran anyway"
        );
        assert!(
            day_dir(&arch, T0 - DAY).exists(),
            "an unmerged day is never deleted"
        );
        c.compact_status_put(&st).await.unwrap();
        assert_eq!(owner.compact_status().await.unwrap(), Some(st));
    }
    c.close().await;
    db.drop(owner).await;
    for d in [raw, arch, other] {
        std::fs::remove_dir_all(&d).ok();
    }
}

#[tokio::test]
async fn a_cold_start_refuses_a_day_that_already_has_published_hours() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let raw = tmp("refuse-raw");
    raw_hours(&raw, &[0, 1, 2, 3]);
    let c = compact_store(&db).await;
    for (name, published) in [
        ("sidecar", "00.hour.json"),
        ("daily", "tob.parquet"),
        ("manifest", "manifest.json"),
    ] {
        let arch = tmp(&format!("refuse-{name}"));
        let day = day_dir(&arch, T0);
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join(published), b"x").unwrap();
        let mut svc = service(&raw, &arch);
        let e = format!("{:#}", svc.step(&c, T0 + 4 * H).await.unwrap_err());
        assert!(
            e.contains("refusing a cold start") && e.contains("2026-09-23"),
            "{e}"
        );
        assert!(!svc.loaded());
        assert!(
            compact::state::list(&arch).is_empty() && !sidecar_path(&day, 1).exists(),
            "nothing compacted"
        );
        std::fs::remove_dir_all(&arch).ok();
    }
    assert!(
        owner
            .compact_hour_statuses(T0, T0 + DAY)
            .await
            .unwrap()
            .is_empty()
    );
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
}

/// Raw hours `first..=last` (indexes from T0): the first has a universe and two books.
fn raw_from(raw: &Path, first: i64, last: i64) {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(raw, &r);
    for i in first..=last {
        let t = T0 + i * H;
        let lines = if i == first {
            vec![
                uni(t + 1_000, &r),
                frame(
                    t + 2_000,
                    0,
                    book("y1", t + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
                ),
                frame(t + 2_100, 0, book("y2", t + 2_090, &[("0.20", "3")], &[])),
            ]
        } else {
            let p = format!("0.{:02}", 20 + i);
            vec![frame(
                t + 1_000,
                0,
                change("y2", t + 995, "BUY", &p, "1", &p, "1"),
            )]
        };
        write_hour(raw, t, &lines);
    }
}

#[tokio::test]
async fn a_first_deploy_mid_day_fills_the_earlier_hours_and_the_day_merges() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (raw, arch) = (tmp("first-raw"), tmp("first-arch"));
    raw_from(&raw, 13, 25);
    owner
        .mark_hours_lost(&[T0 + 5 * H], T0 + 6 * H)
        .await
        .unwrap();
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    let stop = AtomicBool::new(false);
    let st = svc.poll(&c, || T0 + 26 * H, &stop).await;
    assert_eq!(st.last_error, None);
    assert_eq!(
        svc.next_hour(),
        Some(T0 + 24 * H),
        "hours 0–12 at once (no grace: before the first file), then 13–23"
    );
    let hours = owner.compact_hour_statuses(T0, T0 + DAY).await.unwrap();
    assert_eq!(hours.len(), 24);
    assert!(
        hours
            .iter()
            .all(|(h, s)| s == if *h == T0 + 5 * H { "lost" } else { "done" }),
        "{hours:?}"
    );
    let day = day_dir(&arch, T0);
    assert!(
        merged(&day),
        "one reducer from 00:00, so the day's dictionary holds and it merges"
    );
    assert_eq!(
        compact::verify::verify_day(&arch, "2026-09-23").unwrap(),
        Vec::<String>::new()
    );
    let gaps = read_all::<GapRow>(&daily_path(&day, "gaps")).unwrap();
    assert!(
        gaps.iter()
            .any(|g| g.start_ms == T0 && g.reason == reason::SCANNER_STOPPED),
        "{gaps:?}"
    );
    assert!(
        gaps.iter()
            .any(|g| g.start_ms == T0 + 5 * H && g.reason == reason::RAW_HOUR_LOST),
        "{gaps:?}"
    );
    assert!(
        !read_all::<TobRow>(&daily_path(&day, "tob"))
            .unwrap()
            .is_empty()
    );
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
