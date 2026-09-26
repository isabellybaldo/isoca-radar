#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

#[tokio::test]
async fn integrity_summary_counts_kinds_and_rates_per_million() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let row = |kind: &str| IntegrityRow {
        run_id: live,
        ts_ms: n - 1_000,
        kind: kind.into(),
        shard: None,
        token: None,
        event_id: None,
        detail: None,
    };
    let (app, _st) = common::app(&db).await;
    let (_, empty) = common::get(&app, "/api/integrity/summary").await;
    assert_eq!(empty["total"], 0);
    assert!(empty["per_million"].is_null());
    owner
        .write_batch(&Batch {
            integrity: vec![row("top_mismatch"), row("top_mismatch"), row("drift")],
            health: vec![common::health(live, n - n % 60_000 - 60_000, 0)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (status, v) = common::get(&app, "/api/integrity/summary?hours=24").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["by_kind"][0]["kind"], "top_mismatch");
    assert_eq!(v["by_kind"][0]["count"], 2);
    assert_eq!(v["by_kind"][1]["kind"], "drift");
    assert_eq!(
        (v["total"].as_i64(), v["msgs"].as_i64()),
        (Some(3), Some(258_000))
    );
    assert!(v["per_million"].is_string());
    assert_eq!(v["per_million"], "11.63");
    let (status, v) = common::get(&app, "/api/integrity/summary?hours=0").await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("bad_hours"))
    );
    db.drop(owner).await;
}
