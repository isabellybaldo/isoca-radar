#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.3: `guard_task` records every lost mark the capture thread sent before it ended,
//! then ends by itself (the shutdown awaits it after joining the capture thread), even when the
//! database refuses the marks. Its first read turns the capture thread's view from Unread (cap
//! only) into Compacted (alive compactor) or Blind (database down: age + cap), final review M2.
#[path = "../../compact/tests/common/mod.rs"]
mod db;
use scanner::retention::{GuardView, View, guard_task};
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

async fn first_view(view: &GuardView) -> View {
    for _ in 0..200 {
        let v = view.get(scanner::now_ms());
        if v != View::Unread {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    View::Unread
}

#[tokio::test]
async fn the_first_read_of_an_alive_compactor_makes_old_hours_wait() {
    let Some((owner, db)) = db::fresh().await else {
        return;
    };
    owner
        .compact_status_put(&store::CompactStatusRow {
            updated_ms: scanner::now_ms(),
            archive_bytes: 0,
            oldest_day: None,
            lag_hours: 0,
            last_error: None,
        })
        .await
        .unwrap();
    // An hour inside guard_task's LOOKBACK_MS of now: a fixed T0 fell out of it 14 days later.
    let t0 = (scanner::now_ms() / H - 24) * H;
    owner.mark_hours_lost(&[t0], t0).await.unwrap();
    let scanner = db.role("scanner_rw").await;
    let view = Arc::new(GuardView::default());
    assert_eq!(view.get(scanner::now_ms()), View::Unread);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(guard_task(scanner.clone(), view.clone(), rx));
    let v = first_view(&view).await;
    drop(tx);
    task.await.unwrap();
    let View::Compacted(hours) = v else {
        panic!("an alive compactor gives a compacted view, got {v:?}");
    };
    assert!(hours.contains(&t0));
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_database_down_at_the_first_read_falls_back_to_todays_rule() {
    let Some((owner, db)) = db::fresh().await else {
        return;
    };
    let scanner = db.role("scanner_rw").await;
    scanner.close().await; // every read fails
    let view = Arc::new(GuardView::default());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(guard_task(scanner, view.clone(), rx));
    let v = first_view(&view).await;
    drop(tx);
    task.await.unwrap();
    assert_eq!(v, View::Blind);
    db.drop(owner).await;
}
