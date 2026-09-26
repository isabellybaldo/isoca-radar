#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

#[tokio::test]
async fn health_now_is_null_before_any_row() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/now").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["health"].is_null(), "{v}");
    db.drop(owner).await;
}

#[tokio::test]
async fn health_now_returns_the_latest_row_with_its_age_and_drops_5m() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let t = n - n % 60_000;
    owner
        .write_batch(&Batch {
            health: vec![
                common::health(live, t - 60_000, 2),
                common::health(live, t, 1),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/now").await;
    assert_eq!(status, StatusCode::OK);
    let h = &v["health"];
    assert_eq!(h["minute_ms"], t);
    assert_eq!(h["run_id"], live);
    assert_eq!(h["drops_5m"], 3);
    assert_eq!(h["lag_p99_ms"], 1_900);
    assert_eq!(h["capture_paused"], false);
    assert!(h["age_ms"].as_i64().unwrap() >= 0);
    assert!(h["now_ms"].as_i64().unwrap() >= t);
    db.drop(owner).await;
}

#[tokio::test]
async fn health_series_returns_the_window_and_rejects_bad_hours() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let t = n - n % 60_000;
    owner
        .write_batch(&Batch {
            health: vec![
                common::health(live, t - 2 * 3_600_000, 0),
                common::health(live, t, 0),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/series?hours=1").await;
    assert_eq!(status, StatusCode::OK);
    let pts = v["points"].as_array().unwrap();
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0]["minute_ms"], t);
    let (_, v24) = common::get(&app, "/api/health/series").await;
    assert_eq!(v24["points"].as_array().unwrap().len(), 2);
    for bad in ["0", "169", "abc", "-1", "99999999999999999999"] {
        let (status, v) = common::get(&app, &format!("/api/health/series?hours={bad}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "hours={bad}");
        assert_eq!(v["error"], "bad_hours");
        assert!(v["detail"].is_string());
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn an_unknown_route_is_a_json_404() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "not_found");
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_role_cannot_write_and_a_writer_role_is_refused() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (_app, st) = common::app(&db).await;
    api::ensure_read_only(&st.store).await.unwrap();
    let err = st
        .store
        .raw_exec("INSERT INTO runs (service, started_ms, version, config_hash, mode) VALUES ('x', 1, 'v', 'c', 'live')")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("read-only transaction") || err.contains("permission denied"),
        "{err}"
    );
    let owner_pool = Store::connect_read_only(db.opts.clone(), 1, 5_000, None)
        .await
        .unwrap();
    assert!(api::ensure_read_only(&owner_pool).await.is_err());
    owner_pool.close().await;
    db.drop(owner).await;
}
