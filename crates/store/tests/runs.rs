#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use store::NewRun;

fn run(mode: &str, started_ms: i64) -> NewRun {
    NewRun {
        service: "scanner".into(),
        started_ms,
        version: "test".into(),
        config_hash: "cfg".into(),
        mode: mode.into(),
    }
}

#[tokio::test]
async fn counts_recent_starts_and_honours_breaker_reset() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let t0 = 1_800_000_000_000_i64;
    store.start_run(&run("live", t0 - 700_000)).await.unwrap(); // outside a 10-min window
    for i in 0..4 {
        store.start_run(&run("live", t0 + i * 1_000)).await.unwrap();
    }
    store.start_run(&run("safe", t0 + 5_000)).await.unwrap();
    store.start_run(&run("replay", t0 + 6_000)).await.unwrap(); // not a scanner start
    let since = t0 + 10_000 - 600_000;
    assert_eq!(store.starts_since("scanner", since).await.unwrap(), 5);
    assert_eq!(store.starts_since("api", since).await.unwrap(), 0);

    store
        .record_breaker_reset("scanner", t0 + 20_000)
        .await
        .unwrap();
    assert_eq!(
        store.starts_since("scanner", since).await.unwrap(),
        0,
        "reset row must not count"
    );
    store.start_run(&run("live", t0 + 30_000)).await.unwrap();
    assert_eq!(store.starts_since("scanner", since).await.unwrap(), 1);
    db.drop(store).await;
}

#[tokio::test]
async fn end_run_records_exit() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let id = store.start_run(&run("live", 1)).await.unwrap();
    store.end_run(id, 99, "sigterm").await.unwrap();
    let (ended, reason): (Option<i64>, Option<String>) = sqlx_row(&store, id).await;
    assert_eq!(ended, Some(99));
    assert_eq!(reason.as_deref(), Some("sigterm"));
    db.drop(store).await;
}

async fn sqlx_row(store: &store::Store, id: i64) -> (Option<i64>, Option<String>) {
    store.run_exit(id).await.unwrap()
}
