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

#[tokio::test]
async fn a_table_created_after_0003_is_not_writable_by_scanner_but_its_own_tables_are() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    // a later table (think paper_*, api or lab tables) created by the owner role
    owner
        .raw_exec("CREATE TABLE paper_probe (id BIGSERIAL PRIMARY KEY, note TEXT)")
        .await
        .expect("owner creates a later table");
    let scanner = db.role("scanner_rw").await;
    let ins = scanner
        .raw_exec("INSERT INTO paper_probe (note) VALUES ('x')")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&ins), "{ins}");
    let upd = scanner
        .raw_exec("UPDATE paper_probe SET note = 'y'")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&upd), "{upd}");
    scanner
        .raw_exec("SELECT count(*) FROM paper_probe")
        .await
        .expect("reading stays allowed");

    // the scanner's own tables keep their full write path
    let run = scanner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .expect("INSERT runs (+ sequence)");
    scanner.end_run(run, 2, "x").await.expect("UPDATE runs");
    scanner
        .save_universe(1, 0, "{}")
        .await
        .expect("universe_snapshots INSERT + DELETE");
    scanner
        .raw_exec("UPDATE events SET title = title")
        .await
        .expect("UPDATE events");
    scanner
        .raw_exec("UPDATE legs SET status = status")
        .await
        .expect("UPDATE legs");
    scanner
        .raw_exec("DELETE FROM episode_legs WHERE false")
        .await
        .expect("DELETE episode_legs");
    scanner
        .write_batch(&Batch {
            integrity: vec![IntegrityRow {
                run_id: run,
                ts_ms: 1,
                kind: "unit_oversize".into(),
                shard: None,
                token: None,
                event_id: Some("e1".into()),
                detail: Some("1234 tokens".into()),
            }],
            ..Batch::default()
        })
        .await
        .expect("integrity_events accepts unit_oversize");
    scanner.close().await;
    db.drop(owner).await;
}
