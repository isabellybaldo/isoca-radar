#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

const P0: &str = "P0-converter/sp-idle";
const P0Q: &str = "P0-converter%2Fsp-idle";

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
             VALUES ({run_id}, '{id}', {t_ms}, 900, {equity}, 2.56, 0, 0, 95.000000, 0.40, 2)"
        ))
        .await
        .unwrap();
}

async fn ledger(owner: &Store, run_id: i64, id: &str, seq: i64) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run_id}, '{id}', {seq}, {}, 'buy', 'live-1-00000001', 'm1', 'y1', 'no', 51.2, 0.62, -99.84, 0.21, 0, 900.16, NULL)",
            seq * 1_000
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn paper_endpoints_are_empty_lists_when_the_tables_are_missing() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "DROP TABLE paper_markouts, paper_fills, paper_execs, paper_intents, paper_ledger, \
             paper_marks, paper_state, paper_accounts CASCADE",
        )
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    for uri in [
        "/api/paper/accounts".to_string(),
        "/api/paper/positions".to_string(),
    ] {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {v}");
        assert_eq!(v["items"], serde_json::json!([]));
    }
    let (status, v) = common::get(&app, &format!("/api/paper/equity?account={P0Q}")).await;
    assert_eq!(
        (status, v["points"].clone()),
        (StatusCode::OK, serde_json::json!([]))
    );
    let (status, v) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["items"], serde_json::json!([]));
    assert!(v["next_cursor"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn accounts_equity_and_ledger_carry_money_as_strings() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    account(&owner, live, P0, "P0-converter", "sp-idle").await;
    mark(&owner, live, P0, n - 120_000, "1001.00").await;
    mark(&owner, live, P0, n - 60_000, "1002.56").await;
    for seq in 1..=3 {
        ledger(&owner, live, P0, seq).await;
    }
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/accounts").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let a = &v["items"][0];
    assert_eq!(a["account_id"], P0);
    for (k, want) in [
        ("equity", "1002.56"),
        ("bankroll", "1000"),
        ("fee_bips", "0"),
        ("locked", "95"),
        ("realized", "2.56"),
        ("unrealized", "0"),
    ] {
        assert!(a[k].is_string(), "{k}: {a}");
        assert_eq!(a[k], want, "{k}");
    }
    assert_eq!(a["marked_ms"], n - 60_000);
    assert_eq!(a["n_positions"], 2);
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}&hours=24")).await;
    let pts = eq["points"].as_array().unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[1]["equity"], "1002.56");
    let (_, p1) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&limit=2")).await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 2);
    assert_eq!(p1["items"][0]["seq"], 3);
    assert_eq!(p1["items"][0]["cash_delta"], "-99.84");
    assert!(p1["items"][0]["equity"].is_null());
    let cursor = p1["next_cursor"].as_str().unwrap().to_string();
    assert_eq!(cursor, format!("{live}_2"));
    let (_, p2) = common::get(
        &app,
        &format!("/api/paper/ledger?account={P0Q}&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 1);
    assert!(p2["next_cursor"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_come_from_the_live_paper_state() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            r#"INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('P1-settlement/sp-idle', '{"positions":[[{"market_id":"m1","yes_token":"y1","outcome":"no"},{"qty":"51.2","basis":"31.74"}]],"returned":[]}', 1)"#,
        )
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/positions").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let p = &v["items"][0];
    assert_eq!(
        (p["market_id"].as_str(), p["outcome"].as_str()),
        (Some("m1"), Some("no"))
    );
    assert_eq!(
        (p["qty"].as_str(), p["basis"].as_str()),
        (Some("51.2"), Some("31.74"))
    );
    assert_eq!(p["returned"], false);
    assert!(p["event_title"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn paper_parameters_are_validated() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let long = "a".repeat(200);
    for (uri, code) in [
        ("/api/paper/equity".to_string(), "bad_account"),
        ("/api/paper/ledger".to_string(), "bad_account"),
        (format!("/api/paper/equity?account={long}"), "bad_account"),
        (
            format!("/api/paper/equity?account={P0Q}&hours=0"),
            "bad_hours",
        ),
        (
            format!("/api/paper/ledger?account={P0Q}&cursor=bad"),
            "bad_cursor",
        ),
        (
            format!("/api/paper/ledger?account={P0Q}&limit=500"),
            "bad_limit",
        ),
    ] {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {v}");
        assert_eq!(v["error"], code, "{uri}");
    }
    db.drop(owner).await;
}
