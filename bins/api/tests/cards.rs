#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use serde_json::json;

async fn seed(owner: &store::Store, card: &str, win: &str, body: serde_json::Value) {
    let sql = format!(
        "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('{card}', '{win}', 'k', '{}', '{}'::jsonb, 1)",
        body["status"].as_str().unwrap_or("live"),
        body.to_string().replace('\'', "''")
    );
    owner.raw_exec(&sql).await.unwrap();
}

fn body(card: &str, win: &str, ours: Option<&str>) -> serde_json::Value {
    json!({
        "card": card, "paper": "gebele-2026", "title": format!("{card} · x"), "win": win, "status": "thin",
        "premise_key": "k", "hash": "ab".repeat(32), "data_from_ms": 1, "data_to_ms": 2, "since_ms": 1, "n": 4,
        "columns": [
            { "label": "NO-side episodes", "unit": "count", "paper": "36", "their": "0", "ours": "0" },
            { "label": "YES : NO", "unit": "×", "paper": "58.28", "their": null, "ours": ours }
        ],
        "series": {}, "notes": [], "thin_reasons": ["NO side: n = 0 < 10, so the ratio is not reliable"],
        "episode_link": null, "computed_ms": 2
    })
}

#[tokio::test]
async fn cards_returns_every_card_of_a_window_as_stored() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    for card in ["G2", "G1", "EDGE"] {
        seed(
            &owner,
            card,
            "7d",
            body(card, "7d", Some("12345678901234567.89")),
        )
        .await;
    }
    seed(&owner, "G1", "30d", body("G1", "30d", None)).await;
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards?win=7d").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["win"], "7d");
    let cards: Vec<&str> = v["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["card"].as_str().unwrap())
        .collect();
    assert_eq!(cards, ["EDGE", "G1", "G2"]);
    let (_, d) = common::get(&app, "/api/cards").await;
    assert_eq!(d["win"], "7d", "win defaults to 7d");
    let (_, m) = common::get(&app, "/api/cards?win=30d").await;
    assert_eq!(m["cards"].as_array().unwrap().len(), 1);
    let (_, e) = common::get(&app, "/api/cards?win=24h").await;
    assert_eq!(e["cards"], json!([]));
    db.drop(owner).await;
}

#[tokio::test]
async fn decimals_stay_strings_and_null_stays_distinct_from_zero() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    seed(
        &owner,
        "G1",
        "7d",
        body("G1", "7d", Some("12345678901234567.89")),
    )
    .await;
    seed(&owner, "G1", "all", body("G1", "all", None)).await;
    let (app, _) = common::app(&db).await;
    let (_, v) = common::get(&app, "/api/cards?win=7d").await;
    let cols = &v["cards"][0]["columns"];
    assert_eq!(cols[0]["ours"], "0", "a measured zero is the string \"0\"");
    assert_eq!(
        cols[1]["ours"], "12345678901234567.89",
        "never rounded through a float"
    );
    assert!(cols[1]["their"].is_null());
    let (_, a) = common::get(&app, "/api/cards?win=all").await;
    assert!(
        a["cards"][0]["columns"][1]["ours"].is_null(),
        "null (not measured) is not \"0\""
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn today_returns_edge_capture_only() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    seed(&owner, "EDGE", "today", body("EDGE", "today", Some("1.5"))).await;
    seed(&owner, "G1", "today", body("G1", "today", Some("2"))).await; // never written by analyze; filtered anyway
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards?win=today").await;
    assert_eq!(s, StatusCode::OK);
    let cards: Vec<&str> = v["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["card"].as_str().unwrap())
        .collect();
    assert_eq!(cards, ["EDGE"]);
    db.drop(owner).await;
}

#[tokio::test]
async fn a_bad_win_is_a_json_400() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _) = common::app(&db).await;
    for q in ["1h", "7D", "all%20", "%27%20OR%201%3D1", "7d%00"] {
        let (s, v) = common::get(&app, &format!("/api/cards?win={q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
        assert_eq!(v["error"], "bad_win", "{q}");
        assert_eq!(v["detail"], "win must be 24h, 7d, 30d, all or today.");
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn meta_is_empty_before_analyze_runs_and_complete_after() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards/meta").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v,
        json!({ "tick": null, "last_error": null, "catch_up": {}, "papers": [], "not_replicated": [] })
    );
    owner
        .raw_exec(
            "INSERT INTO card_meta (key, value) VALUES \
             ('tick', '{\"last_tick_ms\": 5, \"analyze_version\": \"abc\", \"tick_ms\": 12}'), \
             ('last_error', 'null'), \
             ('cursor', '{\"updated_ms\": 9}'), \
             ('fam:g1', '{\"key\": \"k\", \"next_hour\": 3600000, \"done\": false}'), \
             ('papers', '[{\"id\": \"gebele-2026\", \"status\": \"live\"}]'), \
             ('not_replicated', '[{\"item\": \"Converter profits\"}]')",
        )
        .await
        .unwrap();
    let (_, v) = common::get(&app, "/api/cards/meta").await;
    assert_eq!(v["tick"]["analyze_version"], "abc");
    assert!(v["last_error"].is_null());
    assert_eq!(v["catch_up"]["g1"]["done"], false);
    assert_eq!(v["papers"][0]["status"], "live");
    assert_eq!(v["not_replicated"][0]["item"], "Converter profits");
    assert!(v.get("cursor").is_none(), "internal keys stay internal");
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_role_still_cannot_write_card_results() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ro = store::Store::connect_read_only(db.opts.clone(), 1, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let err = ro
        .raw_exec("INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) VALUES ('G1', '7d', 'k', 'live', '{}', 1)")
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("read-only transaction")
            || err.to_string().contains("permission denied"),
        "{err}"
    );
    api::ensure_read_only(&ro).await.unwrap();
    ro.close().await;
    db.drop(owner).await;
}
