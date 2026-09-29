#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

const URI: &str = "/api/paper/reset";
const OK_BODY: &str = r#"{"confirm": "reset"}"#;
const INTENT: (&str, &str) = ("X-CT-Intent", "reset-paper");
const SAME: (&str, &str) = ("Sec-Fetch-Site", "same-origin");

async fn apply(owner: &Store, request_id: i64) {
    let run = common::run(owner, "live").await;
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
             VALUES ((SELECT COALESCE(max(epoch), 0) + 1 FROM paper_epochs), {}, 'reset', {request_id}, {run}, 'sha')",
            common::now()
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn reset_needs_the_intent_header_and_a_same_origin_fetch() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    for headers in [
        vec![],
        vec![("X-CT-Intent", "yes")],
        vec![INTENT, ("Sec-Fetch-Site", "cross-site")],
        vec![INTENT, ("Sec-Fetch-Site", "same-site")],
        vec![INTENT, ("Sec-Fetch-Site", "none")],
    ] {
        let (status, v) = common::post(&app, URI, &headers, OK_BODY).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}: {v}");
        assert_eq!(v["error"], "forbidden");
    }
    assert!(owner.reset_requests_after(0).await.unwrap().is_empty());
    db.drop(owner).await;
}

#[tokio::test]
async fn a_bad_body_is_a_400() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    for body in [
        "",
        "{}",
        "not json",
        r#"{"confirm": "yes"}"#,
        r#"{"confirm": "reset", "also": 1}"#,
    ] {
        let (status, v) = common::post(&app, URI, &[INTENT, SAME], body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {v}");
        assert_eq!(v["error"], "bad_request");
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn one_reset_is_queued_then_pending_then_the_daily_limit() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v}");
    let id = v["request_id"].as_i64().unwrap();
    let requested_ms = v["requested_ms"].as_i64().unwrap();
    assert!(requested_ms > 0);
    // no Sec-Fetch-Site at all is fine (a non-browser client still needs the custom header)
    let (status, v) = common::post(&app, URI, &[INTENT], OK_BODY).await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::CONFLICT, Some("reset_pending"))
    );
    apply(&owner, id).await; // the scanner applied it
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert_eq!(v["error"], "reset_daily_limit");
    // From the stored request's own instant, never a later `now`: no flake across UTC midnight.
    let stored = owner.latest_reset_request().await.unwrap().unwrap();
    assert_eq!((stored.id, stored.requested_ms), (id, requested_ms));
    let tomorrow = api::reset::utc_date(api::reset::next_midnight(stored.requested_ms));
    assert!(
        v["detail"].as_str().unwrap().contains(&tomorrow),
        "{v} names {tomorrow}"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn without_a_reset_pool_the_route_is_503() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("reset_disabled"))
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_refuses_an_over_granted_reset_role() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .unwrap();
    api::ensure_reset_role(&reset).await.expect("as migrated");
    let as_owner = Store::connect_reset(db.opts.clone(), 5_000, None)
        .await
        .unwrap();
    assert!(api::ensure_reset_role(&as_owner).await.is_err());
    owner
        .raw_exec("GRANT INSERT ON episodes TO paper_reset_rw")
        .await
        .unwrap();
    let e = api::ensure_reset_role(&reset).await.unwrap_err();
    assert!(e.to_string().contains("episodes"), "{e}");
    reset.close().await;
    as_owner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn an_unreachable_reset_pool_leaves_the_dashboard_up_and_the_post_a_503() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    // A role that can't log in, a bad password or a restored cluster: the connect fails.
    let failed = Store::connect_reset(
        Store::options("postgres://nobody:wrong@127.0.0.1:1/none").unwrap(),
        5_000,
        None,
    )
    .await
    .map_err(anyhow::Error::from);
    assert!(failed.is_err());
    let reset = api::reset_pool(failed)
        .await
        .expect("a connect failure is not fatal");
    assert!(reset.is_none());
    let ro = Store::connect_read_only(db.opts.clone(), 4, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let app = api::router(api::AppState::new(ro).with_reset(reset));
    let (status, _) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(status, StatusCode::OK);
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("reset_disabled"))
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn the_reset_pool_is_kept_as_migrated_and_refused_only_when_over_granted() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ok = Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .map_err(anyhow::Error::from);
    let kept = api::reset_pool(ok).await.expect("as migrated");
    assert!(kept.is_some());
    let as_owner = Store::connect_reset(db.opts.clone(), 5_000, None)
        .await
        .map_err(anyhow::Error::from);
    let e = api::reset_pool(as_owner).await.unwrap_err();
    assert!(e.to_string().contains("refusing to start"), "{e}");
    if let Some(r) = kept {
        r.close().await;
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn before_migration_0009_the_epochs_are_empty_not_a_500() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec("DROP TABLE paper_epochs, paper_reset_requests CASCADE")
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let before = common::now();
    let (status, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["epochs"].clone(),
            v["current"].clone(),
            v["pending_request"].clone()
        ),
        (
            serde_json::json!([]),
            serde_json::json!(0),
            serde_json::Value::Null
        )
    );
    assert!(v["next_allowed_ms"].as_i64().unwrap() >= before, "{v}");
    db.drop(owner).await;
}

#[tokio::test]
async fn epochs_list_current_pending_and_next_allowed() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    let before = common::now();
    let (status, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["epochs"], serde_json::json!([]));
    assert_eq!(v["current"], 0);
    assert!(v["pending_request"].is_null());
    assert!(
        v["next_allowed_ms"].as_i64().unwrap() >= before,
        "now: allowed"
    );
    let (_, q) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    let id = q["request_id"].as_i64().unwrap();
    let midnight = api::reset::next_midnight(q["requested_ms"].as_i64().unwrap());
    let (_, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(v["pending_request"]["id"], id);
    assert_eq!(v["next_allowed_ms"], midnight);
    apply(&owner, id).await;
    let (_, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(v["current"], 1);
    assert_eq!(v["epochs"][0]["cause"], "reset");
    assert!(v["epochs"][0]["start_ms"].is_i64());
    assert!(v["pending_request"].is_null());
    assert_eq!(v["next_allowed_ms"], midnight);
    db.drop(owner).await;
}
