#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde_json::json;
use store::*;

/// Epoch 1's start.
const S: i64 = 1_790_121_600_000;
const R0: &str = "P0-converter/rung-0";
const COLO: &str = "P0-converter/colo-idle";
const BURST: &str = "P0-converter/sp-burst";

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

async fn accounts(owner: &Store, run: i64) {
    for (id, scenario, bips) in [
        (R0, "rung-0", 0),
        (COLO, "colo-idle", 0),
        (BURST, "sp-burst", 0),
        ("P0-converter/sp-idle/fee10", "sp-idle", 10), // a fee variant is never a rung
    ] {
        sql(owner, format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run}, '{id}', 'P0-converter', '{scenario}', {bips}, 1000, 100, 'sha')"
        ))
        .await;
    }
}

async fn led(owner: &Store, run: i64, id: &str, seq: i64, t: i64, kind: &str, realized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
         qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
         VALUES ({run}, '{id}', {seq}, {t}, '{kind}', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, {realized}, 1000, NULL)"
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

async fn sweet(owner: &Store, status: &str) {
    let body = json!({
        "status": status,
        "series": {"sweet": {"rows": [
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "category", "bucket": "crypto", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "days", "bucket": "[0,1)", "label": "thin"},
            {"side": "yes", "fee_bips": 0, "scenario": "colo-idle", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 10, "scenario": "sp-idle", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "sp-burst", "dimension": "all", "bucket": "all", "label": "thin"}
        ]}}
    });
    sql(
        owner,
        format!(
            "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('SWEET', '30d', 'k', '{status}', '{body}'::jsonb, 1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn the_ladder_sums_each_kind_since_the_epoch_start_and_reconciles_exactly() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = common::run(&owner, "live").await;
    let r2 = common::run(&owner, "live").await; // a restart inside the same epoch
    let rep = common::run(&owner, "replay").await;
    for r in [r1, r2, rep] {
        accounts(&owner, r).await;
    }
    sql(&owner, format!(
        "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
         VALUES (1, {S}, 'policies', NULL, {r1}, 'sha')"
    ))
    .await;
    for id in [R0, COLO, BURST] {
        led(&owner, r1, id, 1, S - 10, "convert", "100").await; // before the epoch
        led(&owner, r1, id, 2, S, "mark", "0").await;
        led(&owner, r1, id, 3, S + 10, "convert", "3").await;
        led(&owner, r1, id, 4, S + 20, "sell", "-5").await;
        led(&owner, r1, id, 5, S + 60_000, "mark", "0").await;
        led(&owner, r2, id, 1, S + 100_000, "settle", "1").await;
        led(&owner, r2, id, 2, S + 120_000, "mark", "0").await;
        led(&owner, r2, id, 3, S + 120_000, "sell", "-7").await; // its mark's ms, after it
        led(&owner, r2, id, 4, S + 130_000, "convert", "50").await; // after the cut
        mark(&owner, r2, id, S + 120_000, "1001", "2").await;
    }
    mark(&owner, r2, COLO, S + 125_000, "999", "0").await; // a resolution row: no ledger mark
    led(&owner, rep, R0, 1, S + 50, "convert", "999").await; // replay never counts
    exec(&owner, r1, R0, "e1", S + 5, "10", "0").await;
    exec(&owner, r1, R0, "e2", S + 15, "0", "5").await;
    exec(&owner, r2, R0, "e3", S + 130_000, "10", "0").await; // after the cut
    exec(&owner, rep, R0, "e4", S + 50, "10", "0").await;
    let mut h = common::health(r2, common::now() - 600_000, 0);
    h.rest_rtt_p50_ms = Some(150); // lag 78 + 150 / 2 = 153
    owner
        .write_batch(&Batch {
            health: vec![h],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .insert_run_premises(&[RunPremiseRow {
            run_id: r2,
            area: "host".into(),
            version: "1:vps-kvm2|100-250".into(),
        }])
        .await
        .unwrap();
    sweet(&owner, "live").await;
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["epoch"].clone(),
            v["start_ms"].clone(),
            v["as_of_ms"].clone()
        ),
        (json!(1), json!(S), json!(S + 120_000))
    );
    let rungs = v["rungs"].as_array().unwrap();
    let ids: Vec<&str> = rungs
        .iter()
        .map(|r| r["account_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![R0, COLO, BURST],
        "ladder by latency, then the extras"
    );
    assert_eq!(
        rungs
            .iter()
            .map(|r| (r["latency_ms"].clone(), r["extra"].clone()))
            .collect::<Vec<_>>(),
        vec![
            (json!(0), json!(false)),
            (json!(52), json!(false)),
            (json!(450), json!(true))
        ]
    );
    let r0 = &rungs[0];
    assert_eq!(
        (
            r0["n_exec"].clone(),
            r0["n_leg_risk"].clone(),
            r0["n_missed"].clone()
        ),
        (json!(2), json!(1), json!(1))
    );
    for (k, want) in [
        ("gains", "3"),
        ("leg_risk", "-5"),
        ("settle", "1"),
        ("unrealized", "2"),
        ("net", "1"),
    ] {
        assert_eq!(r0[k], want, "{k}");
    }
    for r in rungs {
        let d = |k: &str| r[k].as_str().unwrap().parse::<Decimal>().unwrap();
        assert_eq!(
            d("gains") + d("leg_risk") + d("settle") + d("unrealized"),
            d("net"),
            "equity − bankroll reconciles exactly: {r}"
        );
    }
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    assert_eq!(
        v["measured"],
        json!({"latency_ms": 153, "host": "vps-kvm2"})
    );
    assert_eq!(v["markets_status"], "live");
    let markets = v["markets"].as_array().unwrap();
    assert_eq!(
        markets
            .iter()
            .map(|m| (
                m["scenario"].as_str().unwrap(),
                m["dimension"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![("rung-0", "all"), ("rung-0", "category")],
        "NO side, fee 0, LADDER scenarios, dimensions all + category"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn before_any_epoch_the_ladder_is_empty_and_thin_and_under_review_markets_are_withheld() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["epoch"].clone(),
            v["start_ms"].clone(),
            v["as_of_ms"].clone()
        ),
        (json!(0), json!(null), json!(null))
    );
    assert_eq!(v["rungs"], json!([]));
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    assert!(v["measured"].is_null());
    assert_eq!(
        (v["markets_status"].clone(), v["markets"].clone()),
        (json!("missing"), json!([]))
    );
    sweet(&owner, "under_review").await;
    let (_, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(
        (v["markets_status"].clone(), v["markets"].clone()),
        (json!("under_review"), json!([])),
        "right after deploy A SWEET recomputes under the new key"
    );
    db.drop(owner).await;
}

/// One LADDER account at fee 0 with 30 executed attempts (none bought, so each counts before the
/// cut), a ledger mark at the cut and its paper_marks row: net = equity − 1000.
async fn full_rung(owner: &Store, run: i64, scenario: &str, cut: i64, equity: &str) {
    let id = format!("P0-converter/{scenario}");
    sql(owner, format!(
        "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
         VALUES ({run}, '{id}', 'P0-converter', '{scenario}', 0, 1000, 100, 'sha')"
    ))
    .await;
    for i in 0..30 {
        exec(owner, run, &id, &format!("e{i}"), S + 5 + i, "10", "0").await;
    }
    led(owner, run, &id, 1, cut, "mark", "0").await;
    mark(owner, run, &id, cut, equity, "0").await;
}

#[tokio::test]
async fn a_ladder_rung_missing_or_of_unknown_latency_keeps_the_break_even_thin() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = common::run(&owner, "live").await;
    sql(&owner, format!(
        "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
         VALUES (1, {S}, 'policies', NULL, {run}, 'sha')"
    ))
    .await;
    let cut = S + 60_000;
    // five of the six rungs, all paying: without the sixth the answer would be `all`
    for scenario in ["rung-0", "rung-25", "colo-idle", "rung-100", "sp-idle"] {
        full_rung(&owner, run, scenario, cut, "1010").await;
    }
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["rungs"].as_array().unwrap().len(), 5, "{v}");
    assert_eq!(v["rungs"][0]["n_exec"], 30, "{v}");
    assert_eq!(
        v["break_even"],
        json!({"kind": "thin"}),
        "rung-200 has no account yet"
    );
    full_rung(&owner, run, "rung-200", cut, "1010").await;
    let (_, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(
        v["break_even"],
        json!({"kind": "all"}),
        "all six rungs pay: {v}"
    );
    // no policies file: every latency is unknown, so nothing can be placed
    let (bare, _st) = common::app(&db).await;
    let (status, v) = common::get(&bare, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["rungs"][0]["latency_ms"].is_null(), "{v}");
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    db.drop(owner).await;
}

#[tokio::test]
async fn before_the_paper_migrations_the_ladder_is_epoch_0_not_a_500() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "DROP TABLE paper_epochs, paper_markouts, paper_fills, paper_execs, paper_intents, \
             paper_ledger, paper_marks, paper_state, paper_accounts CASCADE",
        )
        .await
        .unwrap();
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["epoch"].clone(),
            v["start_ms"].clone(),
            v["rungs"].clone()
        ),
        (json!(0), json!(null), json!([]))
    );
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    db.drop(owner).await;
}
