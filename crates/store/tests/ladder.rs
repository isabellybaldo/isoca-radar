#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::{ro, run};
use rust_decimal_macros::dec;
use store::*;

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

async fn account(owner: &Store, run: i64, id: &str) {
    sql(owner, format!(
        "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
         VALUES ({run}, '{id}', 'P0-converter', 'x', 0, 1000, 100, 'sha')"
    ))
    .await;
}

async fn led(owner: &Store, run: i64, id: &str, seq: i64, t: i64, kind: &str, realized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
         qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
         VALUES ({run}, '{id}', {seq}, {t}, '{kind}', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, {realized}, 1000, NULL)"
    ))
    .await;
}

/// A buy of episode `ep` (realized_delta 0: fees go into the basis), written with its exec.
async fn buy(owner: &Store, run: i64, id: &str, seq: i64, t: i64, ep: &str) {
    sql(owner, format!(
        "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
         qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
         VALUES ({run}, '{id}', {seq}, {t}, 'buy', '{ep}', 'm1', 'y1', 'no', 10, 0.5, -5, 0, 0, 995, NULL)"
    ))
    .await;
}

async fn mark(owner: &Store, run: i64, id: &str, t: i64, equity: &str, unrealized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
         max_drawdown, n_positions) VALUES ({run}, '{id}', {t}, 1000, {equity}, 0, {unrealized}, 0, 0, 0, 0)"
    ))
    .await;
}

async fn exec(owner: &Store, run: i64, id: &str, ep: &str, t: i64, q_eff: &str, leg: &str) {
    sql(owner, format!(
        "INSERT INTO paper_execs (run_id, account_id, episode_id, exec_ms, q_eff, leg_risk_qty, pnl_at_tx, hit) \
         VALUES ({run}, '{id}', '{ep}', {t}, {q_eff}, {leg}, NULL, false)"
    ))
    .await;
}

#[tokio::test]
async fn the_cut_is_the_latest_common_mark_and_each_sum_stops_at_its_accounts_mark_row() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [live, replay] {
        account(&owner, r, "A").await;
        account(&owner, r, "B").await;
    }
    led(&owner, live, "A", 1, 50, "convert", "100").await; // before the epoch start (100)
    led(&owner, live, "A", 2, 100, "mark", "0").await;
    buy(&owner, live, "A", 3, 120, "e1").await;
    buy(&owner, live, "A", 4, 130, "e2").await;
    led(&owner, live, "A", 5, 150, "convert", "3").await;
    led(&owner, live, "A", 6, 200, "mark", "0").await;
    led(&owner, live, "A", 7, 200, "sell", "-7").await; // same ms, after A's mark: not in it
    buy(&owner, live, "A", 8, 200, "e4").await; // same ms, after A's mark: e4 is not counted
    buy(&owner, live, "A", 9, 250, "e3").await;
    led(&owner, live, "A", 10, 300, "mark", "0").await; // only A: not a common cut
    led(&owner, live, "B", 1, 100, "mark", "0").await;
    led(&owner, live, "B", 2, 180, "settle", "1").await;
    led(&owner, live, "B", 3, 200, "mark", "0").await;
    led(&owner, replay, "A", 1, 150, "convert", "999").await; // replay rows never count
    buy(&owner, replay, "A", 2, 150, "e1").await;
    mark(&owner, live, "A", 200, "1005", "2").await;
    mark(&owner, live, "B", 200, "1001", "0").await;
    mark(&owner, live, "B", 250, "1004", "3").await; // a resolution row: no ledger mark
    exec(&owner, live, "A", "e0", 60, "0", "0").await; // before the start
    exec(&owner, live, "A", "e1", 120, "0", "5").await; // one leg filled: leg risk, missed
    exec(&owner, live, "A", "e2", 130, "10", "0").await;
    exec(&owner, live, "A", "e5", 140, "0", "0").await; // nothing filled: no buy row, missed
    exec(&owner, live, "A", "e4", 200, "10", "0").await; // in the cut's ms, after A's mark
    exec(&owner, live, "A", "e6", 200, "0", "0").await; // no buy, in the cut's ms: not yet
    exec(&owner, live, "A", "e3", 250, "10", "0").await; // after the cut
    exec(&owner, replay, "A", "e1", 150, "10", "0").await;
    let ro = ro(&db).await;
    let ids = vec!["A".to_string(), "B".to_string()];
    assert_eq!(ro.ladder_as_of(&ids, 100).await.unwrap(), Some(200));
    assert_eq!(ro.ladder_as_of(&ids[..1], 100).await.unwrap(), Some(300));
    assert_eq!(ro.ladder_as_of(&ids, 250).await.unwrap(), None);
    assert_eq!(ro.ladder_as_of(&[], 0).await.unwrap(), None);
    assert_eq!(
        ro.ladder_sums(&ids, 100, 200).await.unwrap(),
        vec![
            LadderSums {
                account_id: "A".into(),
                n_exec: 3,
                n_leg_risk: 1,
                n_missed: 2,
                gains: dec!(3),
                leg_risk: dec!(0),
                settle: dec!(0),
                unrealized: Some(dec!(2)),
                equity: Some(dec!(1005)),
            },
            LadderSums {
                account_id: "B".into(),
                n_exec: 0,
                n_leg_risk: 0,
                n_missed: 0,
                gains: dec!(0),
                leg_risk: dec!(0),
                settle: dec!(1),
                unrealized: Some(dec!(0)),
                equity: Some(dec!(1001)),
            },
        ],
        "A: 1005 − 1000 = 3 + 2 over e1, e2, e5 (e4's buy and the sell follow A's mark row); \
         B: 1001 − 1000 = 1 + 0"
    );
    let a = &ro.ladder_sums(&ids[..1], 100, 300).await.unwrap()[0];
    assert_eq!(
        (a.n_exec, a.n_missed, a.leg_risk),
        (6, 3, dec!(-7)),
        "the next cut takes e4, e6 and e3 and the sell"
    );
    ro.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn measured_latency_reads_live_runs_in_the_window_and_the_newest_host() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let old = run(&owner, "live").await;
    owner.end_run(old, 500, "shutdown").await.unwrap();
    let _replay = run(&owner, "replay").await;
    let live = run(&owner, "live").await;
    let ro = ro(&db).await;
    assert_eq!(
        ro.live_run_ids_between(1_000, 2_000).await.unwrap(),
        vec![live]
    );
    assert_eq!(
        ro.live_run_ids_between(0, 2_000).await.unwrap(),
        vec![old, live]
    );
    assert_eq!(ro.newest_live_host().await.unwrap(), None);
    let host = |run_id: i64, v: &str| RunPremiseRow {
        run_id,
        area: "host".into(),
        version: v.into(),
    };
    owner
        .insert_run_premises(&[host(old, "1:pc-dev|lt25")])
        .await
        .unwrap();
    assert_eq!(
        ro.newest_live_host().await.unwrap().as_deref(),
        Some("1:pc-dev|lt25"),
        "the newest live run WITH a host row (it arrives after 10 health minutes)"
    );
    owner
        .insert_run_premises(&[host(live, "1:vps-kvm2|100-250")])
        .await
        .unwrap();
    assert_eq!(
        ro.newest_live_host().await.unwrap().as_deref(),
        Some("1:vps-kvm2|100-250")
    );
    ro.close().await;
    db.drop(owner).await;
}
