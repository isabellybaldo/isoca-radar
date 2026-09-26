#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use rust_decimal_macros::dec;
use store::*;

fn w(row: EpisodeRow) -> EpisodeWrite {
    EpisodeWrite { row, legs: vec![] }
}

async fn seed(owner: &Store, episodes: Vec<EpisodeWrite>) {
    owner
        .write_batch(&Batch {
            episodes,
            ..Batch::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn open_lists_open_live_episodes_with_money_as_strings() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let now = common::now();
    let mut open = common::ep("live-1-00000001", live, now - 4_200);
    open.closed_server_ms = None;
    open.duration_ms = None;
    let closed = common::ep("live-1-00000002", live, now - 9_000);
    seed(
        &owner,
        vec![
            EpisodeWrite {
                row: open,
                legs: vec![
                    common::leg("live-1-00000001", 0, true),
                    common::leg("live-1-00000001", 1, true),
                ],
            },
            w(closed),
        ],
    )
    .await;
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/episodes/open").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["now_ms"].is_i64());
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let e = &items[0];
    assert_eq!(e["id"], "live-1-00000001");
    for (k, want) in [
        ("net", "0.41"),
        ("gross", "1.37"),
        ("q_star", "83"),
        ("edge", "0.0049"),
    ] {
        assert!(e[k].is_string(), "{k} must be a JSON string: {e}");
        assert_eq!(e[k], want);
    }
    assert_eq!(e["s_legs"], 2);
    assert!(e["closed_ms"].is_null());
    assert_eq!(e["suspect"], false);
    db.drop(owner).await;
}

#[tokio::test]
async fn zero_and_null_stay_distinct_in_the_detail() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let mut row = common::ep("live-1-00000003", live, 1_000);
    row.peak_fees_usd = dec!(0);
    row.peak_profit_per_dollar = Some(dec!(0.000));
    row.annualized_return = None;
    row.max_seconds_delay = Some(3);
    let mut l0 = common::leg("live-1-00000003", 0, true);
    l0.best_price = None;
    l0.fees = dec!(0);
    let l1 = common::leg("live-1-00000003", 1, false);
    let obs = |offset_ms: i32, edge: Option<rust_decimal::Decimal>, skipped: bool| ObservationRow {
        episode_id: "live-1-00000003".into(),
        offset_ms,
        source: "rest".into(),
        observed_ms: (!skipped).then_some(1_000 + i64::from(offset_ms)),
        skipped,
        skip_reason: skipped.then(|| "recheck capacity".to_string()),
        edge,
        q_max: edge.map(|_| dec!(61)),
        pnl: edge.map(|_| dec!(0)),
        still_profitable: edge.map(|e| e > dec!(0)),
        fill_at_original_size: None,
    };
    owner
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row,
                legs: vec![l0, l1],
            }],
            observations: vec![
                obs(3_000, Some(dec!(-0.0008)), false),
                obs(1_000, Some(dec!(0.0022)), false),
                obs(5_000, None, true),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/episodes/live-1-00000003").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let e = &v["episode"];
    assert_eq!(e["fees"], "0");
    assert_eq!(e["profit_per_dollar"], "0");
    assert!(e["annualized_return"].is_null());
    assert_eq!(e["net"], "0.41");
    assert_eq!(e["max_seconds_delay"], 3);
    let legs = v["legs"].as_array().unwrap();
    assert_eq!(legs.len(), 2);
    assert!(legs[0]["best_price"].is_null());
    assert_eq!(legs[0]["fees"], "0");
    assert_eq!(legs[1]["in_s"], false);
    let o = v["observations"].as_array().unwrap();
    assert_eq!(
        o.iter()
            .map(|x| x["offset_ms"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![1_000, 3_000, 5_000]
    );
    assert_eq!(o[0]["edge"], "0.0022");
    assert_eq!(o[0]["pnl"], "0");
    assert_eq!(o[1]["still_profitable"], false);
    assert!(o[2]["edge"].is_null());
    assert_eq!(o[2]["skipped"], true);
    db.drop(owner).await;
}

#[tokio::test]
async fn list_pages_with_cursor_until_the_end() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let t = common::now() - 1_000;
    seed(
        &owner,
        (0..5)
            .map(|i| w(common::ep(&format!("live-1-{i:08}"), live, t)))
            .collect(),
    )
    .await;
    let (app, _st) = common::app(&db).await;
    let mut uri = "/api/episodes?limit=2".to_string();
    let mut ids = Vec::new();
    for _ in 0..5 {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        ids.extend(
            v["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_str().unwrap().to_string()),
        );
        match v["next_cursor"].as_str() {
            Some(c) => uri = format!("/api/episodes?limit=2&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(
        ids,
        [
            "live-1-00000004",
            "live-1-00000003",
            "live-1-00000002",
            "live-1-00000001",
            "live-1-00000000"
        ]
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn list_filters_by_kind_net_and_flags() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let t = common::now() - 60_000;
    let a = common::ep("live-1-a", live, t);
    let mut b = common::ep("live-1-b", live, t + 1);
    b.kind = "yes_basket".into();
    b.peak_pnl_net_usd = dec!(-0.12);
    b.engine_suspect = true;
    let mut c = common::ep("live-1-c", live, t + 2);
    c.censor_left = true;
    seed(&owner, vec![w(a), w(b), w(c)]).await;
    let (app, _st) = common::app(&db).await;
    let ids = |v: serde_json::Value| {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(common::get(&app, "/api/episodes").await.1),
        ["live-1-c", "live-1-b", "live-1-a"]
    );
    assert_eq!(
        ids(common::get(&app, "/api/episodes?kind=yes_basket").await.1),
        ["live-1-b"]
    );
    assert_eq!(
        ids(common::get(&app, "/api/episodes?net=1").await.1),
        ["live-1-c", "live-1-a"]
    );
    assert_eq!(
        ids(common::get(&app, "/api/episodes?flags=suspect").await.1),
        ["live-1-b"]
    );
    assert_eq!(
        ids(common::get(&app, "/api/episodes?flags=censored").await.1),
        ["live-1-c"]
    );
    assert!(
        ids(common::get(&app, "/api/episodes?flags=suspect,censored")
            .await
            .1)
        .is_empty()
    );
    let (_, v) = common::get(&app, "/api/episodes?kind=no_basket").await;
    assert_eq!(v["items"][0]["censored"], true);
    db.drop(owner).await;
}

#[tokio::test]
async fn bad_filters_are_json_400s() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    for (q, code) in [
        ("kind=nope", "bad_kind"),
        ("limit=0", "bad_limit"),
        ("limit=201", "bad_limit"),
        ("limit=99999999999999999999", "bad_limit"),
        ("from=2000&to=1000", "bad_range"),
        ("from=0&to=691200000", "bad_range"),
        ("from=abc", "bad_range"),
        ("flags=x", "bad_flags"),
        ("flags=suspect,,stale", "bad_flags"),
        ("cursor=garbage", "bad_cursor"),
        ("cursor=12_bad%20id", "bad_cursor"),
        ("net=maybe", "bad_flag"),
    ] {
        let (status, v) = common::get(&app, &format!("/api/episodes?{q}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
        assert_eq!(v["error"], code, "{q}");
        assert!(v["detail"].is_string());
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn hostile_ids_are_404s() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let long = "a".repeat(10_000);
    for id in [
        "%27%3B%20DROP%20TABLE%20episodes%3B--",
        long.as_str(),
        "nope",
        "..%2F..%2Fetc",
    ] {
        let (status, v) = common::get(&app, &format!("/api/episodes/{id}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
        assert_eq!(v["error"], "not_found");
    }
    assert!(
        owner.episodes_for_run(1).await.unwrap().is_empty(),
        "episodes table still exists"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn today_counts_via_the_api() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let day = n - n.rem_euclid(DAY_MS);
    let mut gross_only = common::ep("live-1-g", live, day + 2_000);
    gross_only.peak_pnl_net_usd = dec!(0);
    seed(
        &owner,
        vec![w(common::ep("live-1-n", live, day + 1_000)), w(gross_only)],
    )
    .await;
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/today").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((v["gross"].as_i64(), v["net"].as_i64()), (Some(2), Some(1)));
    assert_eq!(v["day_start_ms"], day);
    let hourly = v["hourly"].as_array().unwrap();
    assert_eq!(hourly.len(), 24);
    assert_eq!(
        (
            hourly[0]["hour"].as_i64(),
            hourly[0]["gross"].as_i64(),
            hourly[0]["net"].as_i64()
        ),
        (Some(0), Some(2), Some(1))
    );
    db.drop(owner).await;
}
