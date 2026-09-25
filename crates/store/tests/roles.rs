#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use store::*;

fn is_permission_denied(e: &StoreError) -> bool {
    e.to_string().contains("permission denied")
}

#[tokio::test]
async fn scanner_can_write_its_tables_but_not_delete_episodes_or_ddl() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let scanner = db.role("scanner_rw").await;
    let run = scanner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .expect("scanner inserts runs");
    scanner
        .save_universe(1, 0, "{}")
        .await
        .expect("scanner writes universe cache (incl. DELETE of old rows)");
    scanner
        .write_batch(&Batch {
            integrity: vec![IntegrityRow {
                run_id: run,
                ts_ms: 1,
                kind: "drop".into(),
                shard: None,
                token: None,
                event_id: None,
                detail: None,
            }],
            ..Batch::default()
        })
        .await
        .expect("scanner writes integrity events");
    let del = scanner.raw_exec("DELETE FROM runs").await.unwrap_err();
    assert!(is_permission_denied(&del), "{del}");
    let ddl = scanner.raw_exec("DROP TABLE episodes").await.unwrap_err();
    assert!(ddl.to_string().contains("must be owner"), "{ddl}");
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn analyze_can_read_but_never_write() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = db.role("analyze_ro").await;
    assert!(
        analyze
            .episodes_for_run(1)
            .await
            .expect("analyze reads episodes")
            .is_empty()
    );
    let err = analyze
        .start_run(&NewRun {
            service: "analyze".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap_err();
    assert!(is_permission_denied(&err), "{err}");
    let err = analyze
        .raw_exec("UPDATE events SET title = 'x'")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&err), "{err}");
    analyze.close().await;
    db.drop(owner).await;
}
