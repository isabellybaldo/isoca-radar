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
    for q in ["", "&epoch=1"] {
        let (status, v) = common::get(&app, &format!("/api/paper/equity?account={P0Q}{q}")).await;
        assert_eq!(
            (status, v["points"].clone()),
            (StatusCode::OK, serde_json::json!([])),
            "{q}"
        );
    }
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
    let live = common::run(&owner, "live").await;
    account(
        &owner,
        live,
        "P1-settlement/sp-idle",
        "P1-settlement",
        "sp-idle",
    )
    .await;
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

#[tokio::test]
async fn ledger_hides_minute_marks_on_request() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    account(&owner, live, P0, "P0-converter", "sp-idle").await;
    for (seq, kind) in [(1, "buy"), (2, "mark"), (3, "mark")] {
        ledger_kind(&owner, live, P0, seq, kind).await;
    }
    let (app, _st) = common::app(&db).await;
    let (_, all) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}")).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 3);
    let (status, hidden) = common::get(
        &app,
        &format!("/api/paper/ledger?account={P0Q}&hide_marks=1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    let items = hidden["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "buy");
    let (status, _) = common::get(
        &app,
        &format!("/api/paper/ledger?account={P0Q}&hide_marks=maybe"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    db.drop(owner).await;
}
async fn epochs(owner: &Store, run_id: i64, starts: &[(i32, i64)]) {
    for (e, s) in starts {
        owner
            .raw_exec(&format!(
                "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
                 VALUES ({e}, {s}, 'policies', NULL, {run_id}, 'sha')"
            ))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn equity_and_ledger_default_to_the_current_epoch_and_take_an_epoch() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    account(&owner, live, P0, "P0-converter", "sp-idle").await;
    mark(&owner, live, P0, n - 3 * 3_600_000, "1001").await;
    mark(&owner, live, P0, n - 2 * 3_600_000, "1000").await;
    mark(&owner, live, P0, n - 3_600_000, "1005").await;
    for seq in 1..=3 {
        ledger(&owner, live, P0, seq).await; // t = seq × 1 000
    }
    epochs(&owner, live, &[(1, 2_000), (2, n - 2 * 3_600_000)]).await;
    let (app, _st) = common::app(&db).await;
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}")).await;
    assert_eq!(eq["points"].as_array().unwrap().len(), 2, "{eq}");
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}&epoch=1")).await;
    assert_eq!(
        eq["to_ms"],
        n - 2 * 3_600_000,
        "an earlier epoch's window ends where the next begins"
    );
    assert_eq!(eq["points"][0]["equity"], "1001");
    assert_eq!(eq["points"].as_array().unwrap().len(), 1);
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}")).await;
    assert_eq!(
        l["items"],
        serde_json::json!([]),
        "no ledger row in epoch 2"
    );
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&epoch=1")).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 2);
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&epoch=99")).await;
    assert_eq!(l["items"], serde_json::json!([]));
    for bad in ["-1", "x", "99999999999"] {
        let (status, v) = common::get(
            &app,
            &format!("/api/paper/ledger?account={P0Q}&epoch={bad}"),
        )
        .await;
        assert_eq!(
            (status, v["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("bad_epoch")),
            "{bad}"
        );
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_carry_the_markets_end_date() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) \
             VALUES ('ev1', 's', 'BTC above 100k on October 3?', NULL, false, 2, 1, 1); \
             INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, end_date, first_seen_ms, last_seen_ms) \
             VALUES ('m1', 'ev1', 'y1', 'n1', 'live', 0.07, 1, '2026-10-03T12:00:00Z', 1, 1)",
        )
        .await
        .unwrap();
    let live = common::run(&owner, "live").await;
    account(
        &owner,
        live,
        "P0-converter/rung-0",
        "P0-converter",
        "rung-0",
    )
    .await;
    owner
        .raw_exec(r#"INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('P0-converter/rung-0', '{"positions":[[{"market_id":"m1","yes_token":"y1","outcome":"no"},{"qty":"10","basis":"6"}]],"returned":[]}', 1)"#)
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/positions").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["items"][0]["end_date"], "2026-10-03T12:00:00Z");
    db.drop(owner).await;
}
