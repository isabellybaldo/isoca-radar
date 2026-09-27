#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use sim::account::{Account, Position};
use sim::config::{AccountSpec, PolicyKind};
use sim::types::{LegKey, Outcome};
use store::*;

async fn run(store: &Store, mode: &str) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .unwrap()
}

async fn account(owner: &Store, run_id: i64, id: &str, policy: &str, scenario: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run_id}, '{id}', '{policy}', '{scenario}', 0, 1000, 100, 'sha')"
        ))
        .await
        .unwrap();
}

async fn mark(owner: &Store, run_id: i64, id: &str, t_ms: i64, equity: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown, n_positions) \
             VALUES ({run_id}, '{id}', {t_ms}, 900, {equity}, 0, 0, 0, 95.000000, 0.40, 2)"
        ))
        .await
        .unwrap();
}

async fn ledger(owner: &Store, run_id: i64, id: &str, seq: i64) {
    ledger_kind(owner, run_id, id, seq, "buy").await;
}

async fn ledger_kind(owner: &Store, run_id: i64, id: &str, seq: i64, kind: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run_id}, '{id}', {seq}, {}, '{kind}', 'live-1-00000001', 'm1', 'y1', 'no', 51.2, 0.62, -99.84, 0.21, 0, 900.16, NULL)",
            seq * 1_000
        ))
        .await
        .unwrap();
}

async fn ro(db: &common::TestDb) -> Store {
    Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap()
}

#[tokio::test]
async fn paper_reads_are_empty_when_the_tables_are_missing() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = ro(&db).await;
    assert!(
        analyze.paper_ready().await.unwrap(),
        "S3c migration 0004 must be merged first"
    );
    owner
        .raw_exec(
            "DROP TABLE paper_markouts, paper_fills, paper_execs, paper_intents, paper_ledger, \
             paper_marks, paper_state, paper_accounts CASCADE",
        )
        .await
        .unwrap();
    assert!(!analyze.paper_ready().await.unwrap());
    assert!(analyze.paper_accounts_now().await.unwrap().is_empty());
    assert!(
        analyze
            .paper_equity("P0-converter/sp-idle", 0, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(analyze.paper_positions().await.unwrap().is_empty());
    let page = analyze
        .paper_ledger_page("P0-converter/sp-idle", None, 50, false)
        .await
        .unwrap();
    assert!(page.rows.is_empty() && page.next.is_none());
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn accounts_show_the_latest_mark_of_the_newest_live_run() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    account(
        &owner,
        old,
        "P0-converter/sp-idle",
        "P0-converter",
        "sp-idle",
    )
    .await;
    account(
        &owner,
        live,
        "P0-converter/sp-idle",
        "P0-converter",
        "sp-idle",
    )
    .await;
    account(&owner, live, "P3-naive/sp-idle", "P3-naive", "sp-idle").await;
    account(
        &owner,
        replay,
        "P0-converter/sp-idle",
        "P0-converter",
        "sp-idle",
    )
    .await;
    mark(&owner, old, "P0-converter/sp-idle", 1_000, "990").await;
    mark(&owner, live, "P0-converter/sp-idle", 60_000, "1001.00").await;
    mark(&owner, live, "P0-converter/sp-idle", 120_000, "1002.56").await;
    mark(&owner, replay, "P0-converter/sp-idle", 180_000, "5").await;
    let analyze = ro(&db).await;
    let accts = analyze.paper_accounts_now().await.unwrap();
    assert_eq!(accts.len(), 2);
    assert_eq!(
        (accts[0].account_id.as_str(), accts[0].run_id, accts[0].t_ms),
        ("P0-converter/sp-idle", live, Some(120_000))
    );
    assert_eq!(accts[0].equity, Some(dec!(1002.56)));
    assert_eq!(accts[0].realized, Some(dec!(0)));
    assert_eq!(accts[0].bankroll, dec!(1000));
    assert_eq!(
        (accts[1].account_id.as_str(), accts[1].t_ms, accts[1].equity),
        ("P3-naive/sp-idle", None, None)
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn equity_spans_live_runs_in_the_window_and_skips_replays() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [old, live, replay] {
        account(&owner, r, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    }
    mark(&owner, old, "P0-converter/sp-idle", 1_000, "999").await;
    mark(&owner, live, "P0-converter/sp-idle", 2_000, "1000").await;
    mark(&owner, live, "P0-converter/sp-idle", 9_000, "1001").await; // outside [0, 5000)
    mark(&owner, replay, "P0-converter/sp-idle", 3_000, "7").await;
    let analyze = ro(&db).await;
    let pts = analyze
        .paper_equity("P0-converter/sp-idle", 0, 5_000)
        .await
        .unwrap();
    assert_eq!(
        pts,
        vec![
            EquityPoint {
                t_ms: 1_000,
                equity: dec!(999)
            },
            EquityPoint {
                t_ms: 2_000,
                equity: dec!(1000)
            }
        ]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_parse_a_real_sim_account_and_ignore_odd_states() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) \
             VALUES ('ev1', 's', 'Fed decision in October?', NULL, false, 2, 1, 1); \
             INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, first_seen_ms, last_seen_ms) \
             VALUES ('m1', 'ev1', 'y1', 'n1', 'live', 0.07, 1, 1, 1)",
        )
        .await
        .unwrap();
    let mut acct = Account::new(AccountSpec {
        policy: PolicyKind::P1Settlement,
        scenario: "sp-idle".into(),
        bankroll: dec!(1000),
        per_trade: dec!(100),
        fee_bips: dec!(0),
    });
    let held = LegKey {
        market_id: "m1".into(),
        yes_token: "y1".into(),
        outcome: Outcome::Yes,
    };
    acct.positions.insert(
        held,
        Position {
            qty: dec!(40),
            basis: dec!(18.00),
        },
    );
    let back = LegKey {
        market_id: "m9".into(),
        yes_token: "y9".into(),
        outcome: Outcome::Yes,
    };
    acct.returned.insert(back, dec!(5));
    let json = serde_json::to_string(&acct).unwrap();
    assert!(
        !json.contains('\''),
        "the test inlines the JSON in single quotes"
    );
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_state (account_id, state, updated_ms) VALUES \
             ('{}', '{json}', 1), ('odd', '{{\"v\":1}}', 1), ('weird', '{{\"positions\":{{\"a\":1}}}}', 1)",
            acct.id
        ))
        .await
        .unwrap();
    let analyze = ro(&db).await;
    let pos = analyze.paper_positions().await.unwrap();
    assert_eq!(
        pos,
        vec![
            PaperPosition {
                account_id: "P1-settlement/sp-idle".into(),
                market_id: "m1".into(),
                outcome: "yes".into(),
                qty: dec!(40),
                basis: dec!(18.00),
                returned: false,
                event_title: Some("Fed decision in October?".into()),
            },
            PaperPosition {
                account_id: "P1-settlement/sp-idle".into(),
                market_id: "m9".into(),
                outcome: "yes".into(),
                qty: dec!(5),
                basis: dec!(0),
                returned: true,
                event_title: None,
            },
        ]
    );
    analyze.close().await;
    db.drop(owner).await;
}

/// Ruling 12: a non-numeric `qty`/`basis` in a corrupt `paper_state` row must skip just that row,
/// not fail the whole call with a Postgres cast error.
#[tokio::test]
async fn positions_skip_a_row_with_non_numeric_qty_or_basis() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "INSERT INTO paper_state (account_id, state, updated_ms) VALUES \
             ('good', '{\"positions\":[[{\"market_id\":\"m1\",\"outcome\":\"yes\"},{\"qty\":\"40\",\"basis\":\"18\"}]],\"returned\":[]}', 1), \
             ('bad-qty', '{\"positions\":[[{\"market_id\":\"m2\",\"outcome\":\"no\"},{\"qty\":\"abc\",\"basis\":\"5\"}]],\"returned\":[]}', 1), \
             ('bad-basis', '{\"positions\":[[{\"market_id\":\"m3\",\"outcome\":\"no\"},{\"qty\":\"5\",\"basis\":[1,2]}]],\"returned\":[]}', 1), \
             ('bad-returned', '{\"positions\":[],\"returned\":[[{\"market_id\":\"m4\",\"outcome\":\"yes\"},\"NaN\"]]}', 1)",
        )
        .await
        .unwrap();
    let analyze = ro(&db).await;
    let pos = analyze.paper_positions().await.unwrap();
    assert_eq!(
        pos,
        vec![PaperPosition {
            account_id: "good".into(),
            market_id: "m1".into(),
            outcome: "yes".into(),
            qty: dec!(40),
            basis: dec!(18),
            returned: false,
            event_title: None,
        }]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn ledger_pages_newest_first_by_run_then_seq() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [old, live, replay] {
        account(&owner, r, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    }
    ledger(&owner, old, "P0-converter/sp-idle", 1).await;
    ledger(&owner, old, "P0-converter/sp-idle", 2).await;
    ledger(&owner, live, "P0-converter/sp-idle", 3).await;
    ledger(&owner, replay, "P0-converter/sp-idle", 9).await;
    let analyze = ro(&db).await;
    let p1 = analyze
        .paper_ledger_page("P0-converter/sp-idle", None, 2, false)
        .await
        .unwrap();
    assert_eq!(
        p1.rows
            .iter()
            .map(|r| (r.run_id, r.seq))
            .collect::<Vec<_>>(),
        vec![(live, 3), (old, 2)]
    );
    assert_eq!(p1.rows[0].cash_delta, dec!(-99.84));
    assert_eq!(p1.rows[0].equity, None);
    assert_eq!(p1.next, Some((old, 2)));
    let p2 = analyze
        .paper_ledger_page("P0-converter/sp-idle", p1.next, 2, false)
        .await
        .unwrap();
    assert_eq!(
        p2.rows
            .iter()
            .map(|r| (r.run_id, r.seq))
            .collect::<Vec<_>>(),
        vec![(old, 1)]
    );
    assert_eq!(p2.next, None);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn ledger_can_skip_minute_marks_and_still_page() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    account(
        &owner,
        live,
        "P0-converter/sp-idle",
        "P0-converter",
        "sp-idle",
    )
    .await;
    for (seq, kind) in [
        (1, "buy"),
        (2, "mark"),
        (3, "convert"),
        (4, "mark"),
        (5, "mark"),
        (6, "settle"),
    ] {
        ledger_kind(&owner, live, "P0-converter/sp-idle", seq, kind).await;
    }
    let analyze = ro(&db).await;
    let seqs = |p: &LedgerPage| p.rows.iter().map(|r| r.seq).collect::<Vec<_>>();
    let p1 = analyze
        .paper_ledger_page("P0-converter/sp-idle", None, 2, true)
        .await
        .unwrap();
    assert_eq!(seqs(&p1), vec![6, 3]);
    assert_eq!(p1.next, Some((live, 3)));
    let p2 = analyze
        .paper_ledger_page("P0-converter/sp-idle", p1.next, 2, true)
        .await
        .unwrap();
    assert_eq!(seqs(&p2), vec![1]);
    assert_eq!(p2.next, None);
    let all = analyze
        .paper_ledger_page("P0-converter/sp-idle", None, 50, false)
        .await
        .unwrap();
    assert_eq!(all.rows.len(), 6);
    analyze.close().await;
    db.drop(owner).await;
}
