#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.3: `guard_task` records every lost mark the capture thread sent before it ended,
//! then ends by itself (the shutdown awaits it after joining the capture thread), even when the
//! database refuses the marks.
#[path = "../../compact/tests/common/mod.rs"]
mod db;
use scanner::retention::{GuardView, guard_task};
use std::sync::Arc;
use std::time::Duration;

const T0: i64 = 1_790_121_600_000;
const H: i64 = 3_600_000;

#[tokio::test]
async fn the_marks_sent_before_the_capture_thread_ends_are_recorded_then_the_task_ends() {
    let Some((owner, db)) = db::fresh().await else {
        return;
    };
    let scanner = db.role("scanner_rw").await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(guard_task(
        scanner.clone(),
        Arc::new(GuardView::default()),
        rx,
    ));
    tx.send(T0).unwrap();
    tx.send(T0 + H).unwrap();
    drop(tx); // the capture thread ends
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("guard_task ends once the sender is gone")
        .unwrap();
    assert_eq!(
        owner.compact_hour_statuses(T0, T0 + 2 * H).await.unwrap(),
        vec![(T0, "lost".to_string()), (T0 + H, "lost".to_string())]
    );
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_refused_mark_does_not_keep_the_task_alive_at_shutdown() {
    let Some((owner, db)) = db::fresh().await else {
        return;
    };
    // analyze_ro reads compact_status but can't insert: every flush fails, the last one too
    let ro = db.role("analyze_ro").await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(guard_task(ro.clone(), Arc::new(GuardView::default()), rx));
    tx.send(T0).unwrap();
    drop(tx);
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .expect("guard_task ends with the marks unrecorded")
        .unwrap();
    assert!(
        owner
            .compact_hour_statuses(T0, T0 + H)
            .await
            .unwrap()
            .is_empty()
    );
    ro.close().await;
    db.drop(owner).await;
}
