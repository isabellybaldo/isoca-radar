#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

async fn new_run(store: &Store) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c+p".into(),
            mode: "replay".into(),
        })
        .await
        .unwrap()
}

const A: &str = "P0-converter/sp-idle";

fn ledger(run: i64) -> Vec<PaperLedgerRow> {
    vec![
        PaperLedgerRow {
            run_id: run,
            account_id: A.into(),
            seq: 1,
            t_ms: 1_300,
            kind: "buy".into(),
            episode_id: Some("replay-1-00000001".into()),
            market_id: Some("m0".into()),
            yes_token: Some("y0".into()),
            outcome: Some("no".into()),
            qty: dec!(51.2),
            price: Some(dec!(0.6)),
            cash_delta: dec!(-30.72),
            fees: dec!(0),
            realized_delta: dec!(0),
            cash_after: dec!(969.28),
            equity: None,
        },
        PaperLedgerRow {
            run_id: run,
            account_id: A.into(),
            seq: 2,
            t_ms: 60_000,
            kind: "mark".into(),
            episode_id: None,
            market_id: None,
            yes_token: None,
            outcome: None,
            qty: dec!(0),
            price: None,
            cash_delta: dec!(0),
            fees: dec!(0),
            realized_delta: dec!(0),
            cash_after: dec!(969.28),
            equity: Some(dec!(1000.000000000001)),
        },
    ]
}

fn paper_batch(run: i64) -> Batch {
    Batch {
        paper_accounts: vec![PaperAccountRow {
            run_id: run,
            account_id: A.into(),
            policy: "P0-converter".into(),
            scenario: "sp-idle".into(),
            fee_bips: dec!(0),
            bankroll: dec!(1000),
            per_trade: dec!(100),
            policies_sha256: "ab".repeat(32),
        }],
        paper_intents: vec![PaperIntentRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            decided_ms: 1_000,
            exec_ms: Some(1_300),
            skip_reason: None,
            q_target: Some(dec!(51.2)),
            detected_q: Some(dec!(100)),
            detected_pnl: Some(dec!(5)),
            reserve: Some(dec!(99.841)),
        }],
        paper_execs: vec![PaperExecRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            exec_ms: 1_300,
            q_eff: dec!(51.2),
            leg_risk_qty: dec!(0),
            pnl_at_tx: None,
            hit: false,
        }],
        paper_fills: vec![PaperFillRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            market_id: "m0".into(),
            yes_token: "y0".into(),
            outcome: "no".into(),
            side: "buy".into(),
            t_ms: 1_300,
            limit_price: Some(dec!(0.6)),
            requested: dec!(51.2),
            filled: dec!(51.2),
            vwap: Some(dec!(0.6)),
            fees: dec!(0),
            levels_used: 1,
        }],
        paper_ledger: ledger(run),
        paper_markouts: vec![PaperMarkoutRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            market_id: "m0".into(),
            yes_token: "y0".into(),
            outcome: "no".into(),
            fill_vwap: dec!(0.6),
            offset_ms: 1_000,
            mid: Some(dec!(0.48)),
            markout: Some(dec!(-0.12)),
        }],
        paper_marks: vec![PaperMarkRow {
            run_id: run,
            account_id: A.into(),
            t_ms: 60_000,
            cash: dec!(969.28),
            equity: dec!(1000),
            realized: dec!(0),
            unrealized: dec!(0),
            fees_paid: dec!(0),
            locked: dec!(30.72),
            max_drawdown: dec!(0),
            n_positions: 1,
        }],
        ..Batch::default()
    }
}

async fn counts(s: &Store) -> Vec<i64> {
    let mut v = Vec::new();
    for t in [
        Table::PaperAccounts,
        Table::PaperIntents,
        Table::PaperExecs,
        Table::PaperFills,
        Table::PaperLedger,
        Table::PaperMarkouts,
        Table::PaperMarks,
    ] {
        v.push(s.count(t).await.unwrap());
    }
    v
}

#[tokio::test]
async fn paper_rows_round_trip_exactly_and_a_rewrite_changes_nothing() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let b = paper_batch(run);
    store.write_batch(&b).await.unwrap();
    assert_eq!(counts(&store).await, vec![1, 1, 1, 1, 2, 1, 1]);
    let back = store.paper_ledger_for_run(run).await.unwrap();
    assert_eq!(
        back,
        ledger(run),
        "exact, zeros stay zeros and NULLs stay NULL"
    );
    assert_eq!(
        back[1].equity,
        Some(dec!(1000.000000000001)),
        "12 dp survive"
    );
    assert_eq!(store.paper_marks_for_run(run).await.unwrap(), b.paper_marks);
    // the writer re-sends a batch whose commit succeeded but whose ack was lost
    store.write_batch(&b).await.unwrap();
    assert_eq!(
        counts(&store).await,
        vec![1, 1, 1, 1, 2, 1, 1],
        "idempotent"
    );
    db.drop(store).await;
}

#[tokio::test]
async fn paper_state_keeps_the_newest_save_per_account() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let st = |id: &str, body: &str, ms: i64| PaperStateRow {
        account_id: id.into(),
        state: body.into(),
        updated_ms: ms,
        policies_sha256: Some("ab".repeat(32)),
    };
    store
        .write_batch(&Batch {
            paper_state: vec![
                st("a", "{\"v\":1}", 1),
                st("a", "{\"v\":2}", 2),
                st("b", "{}", 2),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        store.load_paper_state().await.unwrap(),
        vec![st("a", "{\"v\":2}", 2), st("b", "{}", 2)]
    );
    // an OLDER save (a restored batch from a failed write) never overwrites a newer one
    store
        .write_batch(&Batch {
            paper_state: vec![st("a", "{\"v\":0}", 0), st("b", "{\"v\":3}", 3)],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        store.load_paper_state().await.unwrap(),
        vec![st("a", "{\"v\":2}", 2), st("b", "{\"v\":3}", 3)]
    );
    db.drop(store).await;
}

#[tokio::test]
async fn paper_state_round_trips_the_policies_sha256_and_a_pre_migration_row_is_null() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let sha = "cd".repeat(32);
    store
        .write_batch(&Batch {
            paper_state: vec![
                PaperStateRow {
                    account_id: "a".into(),
                    state: "{}".into(),
                    updated_ms: 1,
                    policies_sha256: Some(sha.clone()),
                },
                PaperStateRow {
                    account_id: "b".into(),
                    state: "{}".into(),
                    updated_ms: 1,
                    policies_sha256: None,
                },
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let rows = store.load_paper_state().await.unwrap();
    assert_eq!(
        rows.iter()
            .find(|r| r.account_id == "a")
            .unwrap()
            .policies_sha256,
        Some(sha),
        "the sha it was saved under round-trips exactly"
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.account_id == "b")
            .unwrap()
            .policies_sha256,
        None,
        "a row saved before this column existed reads back NULL, never a fabricated match"
    );
    // a rewrite under a new sha updates the recorded sha (still guarded by updated_ms)
    let sha2 = "ef".repeat(32);
    store
        .write_batch(&Batch {
            paper_state: vec![PaperStateRow {
                account_id: "a".into(),
                state: "{}".into(),
                updated_ms: 2,
                policies_sha256: Some(sha2.clone()),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    let rows = store.load_paper_state().await.unwrap();
    assert_eq!(
        rows.iter()
            .find(|r| r.account_id == "a")
            .unwrap()
            .policies_sha256,
        Some(sha2)
    );
    db.drop(store).await;
}

#[tokio::test]
async fn a_paper_row_without_its_account_is_a_data_error() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut b = paper_batch(run);
    b.paper_accounts.clear();
    let e = store.write_batch(&b).await.unwrap_err();
    assert!(
        e.is_data_error(),
        "FK violation → quarantine path, not a retry loop: {e}"
    );
    db.drop(store).await;
}

#[tokio::test]
async fn integrity_accepts_non_binary_payout() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    store
        .write_batch(&Batch {
            integrity: vec![IntegrityRow {
                run_id: run,
                ts_ms: 1,
                kind: "non_binary_payout".into(),
                shard: None,
                token: None,
                event_id: Some("e1".into()),
                detail: Some("market m0: payout yes 0.5 no 0.5".into()),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(store.count(Table::IntegrityEvents).await.unwrap(), 1);
    db.drop(store).await;
}

#[test]
fn into_singles_puts_accounts_before_the_rows_that_reference_them() {
    let b = paper_batch(1);
    let n = b.rows();
    assert_eq!((n, b.paper_rows()), (8, 8));
    let singles = b.into_singles();
    assert_eq!(singles.len(), n);
    assert!(singles.iter().all(|s| s.rows() == 1));
    assert_eq!(singles[0].paper_accounts.len(), 1, "accounts first");
}

#[test]
fn take_and_extend_move_only_paper_rows() {
    let mut b = paper_batch(1);
    b.integrity.push(IntegrityRow {
        run_id: 1,
        ts_ms: 1,
        kind: "drop".into(),
        shard: None,
        token: None,
        event_id: None,
        detail: None,
    });
    let p = b.take_paper();
    assert_eq!((b.rows(), b.paper_rows(), p.rows()), (1, 0, 8));
    let mut c = Batch::default();
    c.extend_paper(p.clone());
    c.extend_paper(p);
    assert_eq!(c.paper_ledger.len(), 4);
    assert!(!c.is_empty());
    assert!(Batch::default().is_empty());
}
