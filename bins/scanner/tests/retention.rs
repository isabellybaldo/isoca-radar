#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.3: the scanner deletes a raw hour by age only once it is compacted (done or lost),
//! deletes the oldest hour at the hard cap whatever its status (and marks it lost), and falls back
//! to age + cap when it can't read compact_hours or the compactor is silent (rollback, §9.5).
use scanner::config::CaptureCfg;
use scanner::retention::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tape::files::HourFile;

const T0: i64 = 1_790_121_600_000;
const H: i64 = 3_600_000;

fn hf(i: i64, bytes: u64) -> HourFile {
    HourFile {
        start_ms: T0 + i * H,
        path: PathBuf::from(format!("/c/{i:02}.jsonl.zst")),
        bytes,
    }
}

fn hours(n: i64, bytes: u64) -> Vec<HourFile> {
    (0..n).map(|i| hf(i, bytes)).collect()
}

fn deleted(d: &[Deletion]) -> Vec<i64> {
    d.iter().map(|x| (x.hour_ms - T0) / H).collect()
}

#[test]
fn the_age_rule_deletes_only_compacted_hours() {
    let now = T0 + 30 * H; // hours 0..=5 are past the 24 h retention
    let compacted: BTreeSet<i64> = [0, 1, 2, 7].iter().map(|i| T0 + i * H).collect();
    let d = plan(&hours(30, 1), 0, now, 24, u64::MAX, None, Some(&compacted));
    assert_eq!(
        deleted(&d),
        vec![0, 1, 2],
        "3, 4, 5 wait for the compactor; 7 is too young"
    );
    assert!(d.iter().all(|x| !x.by_cap && !x.uncompacted));
}

#[test]
fn the_cap_deletes_the_oldest_whatever_its_status_and_flags_the_uncompacted_ones() {
    let now = T0 + 3 * H;
    let compacted: BTreeSet<i64> = [T0].into_iter().collect();
    let d = plan(&hours(3, 10), 4, now, 24, 15, None, Some(&compacted));
    assert_eq!(deleted(&d), vec![0, 1], "34 bytes → 24 → 14 ≤ 15");
    assert!(d.iter().all(|x| x.by_cap));
    assert_eq!(
        d.iter().map(|x| x.uncompacted).collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn without_a_view_retention_falls_back_to_age_and_cap_and_reports_every_deletion() {
    let now = T0 + 30 * H;
    let d = plan(&hours(30, 1), 0, now, 24, u64::MAX, None, None);
    assert_eq!(deleted(&d), vec![0, 1, 2, 3, 4, 5]);
    assert!(
        d.iter().all(|x| x.uncompacted),
        "each gets a lost mark; ON CONFLICT DO NOTHING keeps done rows"
    );
}

#[test]
fn the_hour_being_written_is_never_deleted() {
    let hs = hours(2, 10);
    let keep = hs[0].path.clone();
    let d = plan(&hs, 0, T0 + 40 * H, 24, 0, Some(Path::new(&keep)), None);
    assert_eq!(deleted(&d), vec![1]);
}

#[test]
fn the_guard_view_is_fresh_for_two_minutes_only_and_can_be_cleared() {
    let v = GuardView::default();
    assert_eq!(v.get(T0), None, "never read: fall back");
    v.set(T0, [T0].into_iter().collect());
    assert_eq!(v.get(T0 + VIEW_FRESH_MS), Some([T0].into_iter().collect()));
    assert_eq!(v.get(T0 + VIEW_FRESH_MS + 1), None, "stale: fall back");
    v.set(T0, [T0].into_iter().collect());
    v.clear();
    assert_eq!(v.get(T0), None, "cleared (compactor silent): fall back");
}

#[test]
fn the_guard_waits_only_for_a_compactor_that_reported_within_six_hours() {
    assert_eq!(COMPACTOR_STALE_MS, 6 * H);
    assert!(
        !compactor_alive(None, T0),
        "no status row: not deployed yet, today's rule"
    );
    assert!(compactor_alive(Some(T0), T0 + COMPACTOR_STALE_MS));
    assert!(
        !compactor_alive(Some(T0), T0 + COMPACTOR_STALE_MS + 1),
        "stopped (rollback): today's rule"
    );
}

fn capture_dir(name: &str, n: i64) -> (PathBuf, CaptureCfg) {
    let dir = std::env::temp_dir().join(format!(
        "scanner-hk-{name}-{}-{}",
        std::process::id(),
        scanner::now_ms()
    ));
    for i in 0..n {
        let p = tape::files::hour_path(&dir, T0 + i * H);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![0u8; 10]).unwrap();
    }
    let cfg = CaptureCfg {
        dir: dir.to_string_lossy().to_string(),
        pause_below_gb: 10,
        resume_above_gb: 15,
        retention_hours: 24,
        max_capture_gb: 15,
    };
    (dir, cfg)
}

fn marks(rx: &mut tokio::sync::mpsc::UnboundedReceiver<i64>) -> Vec<i64> {
    let mut v = Vec::new();
    while let Ok(h) = rx.try_recv() {
        v.push((h - T0) / H);
    }
    v
}

fn left(dir: &Path) -> usize {
    tape::files::listing(dir).hours.len()
}

#[test]
fn housekeeping_marks_hours_lost_at_the_cap() {
    let (dir, mut cfg) = capture_dir("cap", 3);
    cfg.max_capture_gb = 0; // everything over the cap
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let keep = tape::files::hour_path(&dir, T0 + 2 * H);
    let view: BTreeSet<i64> = [T0].into_iter().collect(); // hour 0 is done
    let n = housekeep(&dir, T0 + 3 * H, &cfg, Some(&view), &tx, Some(&keep));
    assert_eq!(n, 2);
    assert_eq!(
        marks(&mut rx),
        vec![1],
        "only the uncompacted hour gets a lost mark"
    );
    assert_eq!(left(&dir), 1, "the hour being written stays");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn housekeeping_keeps_uncompacted_old_hours_while_the_compactor_is_alive() {
    let (dir, cfg) = capture_dir("wait", 3);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let view: BTreeSet<i64> = [T0 + H].into_iter().collect();
    let n = housekeep(&dir, T0 + 40 * H, &cfg, Some(&view), &tx, None);
    assert_eq!(n, 1, "only hour 1 is done");
    assert!(marks(&mut rx).is_empty());
    assert_eq!(left(&dir), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn housekeeping_without_a_view_is_todays_rule_and_marks_every_deletion() {
    let (dir, cfg) = capture_dir("fallback", 3);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let n = housekeep(&dir, T0 + 40 * H, &cfg, None, &tx, None);
    assert_eq!(n, 3);
    assert_eq!(marks(&mut rx), vec![0, 1, 2]);
    std::fs::remove_dir_all(&dir).ok();
}
