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

#[tokio::test]
async fn scanner_appends_paper_rows_and_upserts_state_but_analyze_only_reads() {
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
        .expect("run");
    let acct = PaperAccountRow {
        run_id: run,
        account_id: "P0-converter/sp-idle".into(),
        policy: "P0-converter".into(),
        scenario: "sp-idle".into(),
        fee_bips: 0.into(),
        bankroll: 1000.into(),
        per_trade: 100.into(),
        policies_sha256: "ab".repeat(32),
    };
    let mark = PaperLedgerRow {
        run_id: run,
        account_id: acct.account_id.clone(),
        seq: 1,
        t_ms: 60_000,
        kind: "mark".into(),
        episode_id: None,
        market_id: None,
        yes_token: None,
        outcome: None,
        qty: 0.into(),
        price: None,
        cash_delta: 0.into(),
        fees: 0.into(),
        realized_delta: 0.into(),
        cash_after: 1000.into(),
        equity: Some(1000.into()),
    };
    let state = |ms: i64| PaperStateRow {
        account_id: "P0-converter/sp-idle".into(),
        state: format!("{{\"saved\":{ms}}}"),
        updated_ms: ms,
        policies_sha256: Some("ab".repeat(32)),
    };
    scanner
        .write_batch(&Batch {
            paper_accounts: vec![acct],
            paper_ledger: vec![mark],
            paper_state: vec![state(1)],
            ..Batch::default()
        })
        .await
        .expect("scanner appends paper rows");
    scanner
        .write_batch(&Batch {
            paper_state: vec![state(2)],
            ..Batch::default()
        })
        .await
        .expect("scanner upserts paper_state (needs UPDATE)");
    assert_eq!(
        scanner.load_paper_state().await.expect("reads its state")[0].updated_ms,
        2
    );
    for sql in [
        "UPDATE paper_ledger SET qty = qty",
        "DELETE FROM paper_ledger",
        "UPDATE paper_accounts SET bankroll = bankroll",
        "DELETE FROM paper_state",
    ] {
        let e = scanner.raw_exec(sql).await.unwrap_err();
        assert!(
            is_permission_denied(&e),
            "history is append-only, state never deleted: {sql}: {e}"
        );
    }
    let analyze = db.role("analyze_ro").await;
    assert_eq!(
        analyze
            .paper_ledger_for_run(run)
            .await
            .expect("analyze reads")
            .len(),
        1
    );
    for sql in [
        "INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('x', '{}', 1)",
        "UPDATE paper_state SET updated_ms = 3",
        "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, \
         fees_paid, locked, max_drawdown, n_positions) VALUES (1, 'x', 1, 0, 0, 0, 0, 0, 0, 0, 0)",
    ] {
        let e = analyze.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    analyze.close().await;
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn analyze_rw_reads_the_scanner_tables_and_writes_only_card_tables() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let rw = db.role("analyze_rw").await;
    for t in [
        "episodes",
        "runs",
        "run_premises",
        "health",
        "paper_marks",
        "paper_accounts",
    ] {
        rw.raw_exec(&format!("SELECT count(*) FROM {t}"))
            .await
            .unwrap_or_else(|e| panic!("analyze_rw reads {t}: {e}"));
    }
    rw.raw_exec(
        "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) VALUES ('g1', 'k', 0, '{}', 1); \
         UPDATE card_cells SET computed_ms = 2; \
         INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
             VALUES ('G1', '7d', 'k', 'live', '{}', 1); \
         UPDATE card_results SET computed_ms = 2; \
         INSERT INTO card_meta (key, value) VALUES ('tick', '{}'); \
         UPDATE card_meta SET value = '{\"a\":1}'; \
         DELETE FROM card_cells; DELETE FROM card_results; DELETE FROM card_meta",
    )
    .await
    .expect("analyze_rw writes the card tables");
    for sql in [
        "INSERT INTO runs (service, started_ms, version, config_hash, mode) VALUES ('x', 1, 'v', 'c', 'live')",
        "UPDATE episodes SET kind = kind",
        "DELETE FROM episodes",
        "UPDATE health SET msgs = msgs",
        "INSERT INTO run_premises (run_id, area, version) VALUES (1, 'fees', '1')",
        "UPDATE paper_marks SET equity = equity",
        "DELETE FROM paper_accounts",
        "UPDATE paper_state SET updated_ms = 1",
        "UPDATE legs SET status = status",
    ] {
        let e = rw.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    let ddl = rw.raw_exec("CREATE TABLE nope (a int)").await.unwrap_err();
    assert!(is_permission_denied(&ddl), "{ddl}");
    let settings = owner.role_db_settings("analyze_rw").await.unwrap();
    assert!(
        settings.iter().any(|s| s == "statement_timeout=10s"),
        "{settings:?}"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn analyze_ro_reads_card_results_and_meta_but_still_cannot_write() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ro = db.role("analyze_ro").await;
    ro.raw_exec("SELECT count(*) FROM card_results; SELECT count(*) FROM card_meta")
        .await
        .expect("analyze_ro reads results");
    for sql in [
        "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) VALUES ('G1', '7d', 'k', 'live', '{}', 1)",
        "INSERT INTO card_meta (key, value) VALUES ('x', '{}')",
        "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) VALUES ('g1', 'k', 0, '{}', 1)",
        "INSERT INTO run_premises (run_id, area, version) VALUES (1, 'fees', '1')",
    ] {
        let e = ro.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    ro.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn scanner_appends_run_premises_but_never_rewrites_them() {
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
        .unwrap();
    let rows = vec![
        RunPremiseRow {
            run_id: run,
            area: "fees".into(),
            version: "1+e3b0c442".into(),
        },
        RunPremiseRow {
            run_id: run,
            area: "depth".into(),
            version: "1+e3b0c442".into(),
        },
    ];
    scanner
        .insert_run_premises(&rows)
        .await
        .expect("scanner appends");
    scanner
        .insert_run_premises(&rows)
        .await
        .expect("a re-sent row is a no-op, not an error");
    let back = scanner.run_premises_for(run).await.unwrap();
    assert_eq!(
        back.iter().map(|r| r.area.as_str()).collect::<Vec<_>>(),
        ["depth", "fees"]
    );
    for sql in [
        "UPDATE run_premises SET version = 'x'",
        "DELETE FROM run_premises",
    ] {
        let e = scanner.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    let cards = scanner
        .raw_exec("INSERT INTO card_meta (key, value) VALUES ('x', '{}')")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&cards), "{cards}");
    scanner.close().await;
    db.drop(owner).await;
}
