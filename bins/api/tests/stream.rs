#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::*;
use tower::ServiceExt;

/// Open a stream client and collect everything it receives into a shared string.
async fn client(app: &Router) -> Arc<Mutex<String>> {
    let resp = app
        .clone()
        .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let buf = Arc::new(Mutex::new(String::new()));
    let b = buf.clone();
    tokio::spawn(async move {
        let mut body = resp.into_body().into_data_stream();
        while let Some(Ok(chunk)) = body.next().await {
            b.lock().unwrap().push_str(&String::from_utf8_lossy(&chunk));
        }
    });
    buf
}

/// NOTIFY `id` every 200 ms until every client saw it (the bridge may still be connecting).
async fn notify_until_seen(owner: &Store, id: &str, clients: &[Arc<Mutex<String>>]) {
    let want = format!("event: episode\ndata: {id}\n");
    let seen = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            owner
                .raw_exec(&format!("SELECT pg_notify('episode', '{id}')"))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            if clients.iter().all(|c| c.lock().unwrap().contains(&want)) {
                break;
            }
        }
    })
    .await;
    assert!(seen.is_ok(), "{id} never reached every client");
}

#[tokio::test]
async fn a_notify_episode_reaches_two_clients_over_one_listen() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(
        st.store.clone(),
        st.events.clone(),
        Duration::from_millis(100),
    );
    let a = client(&app).await;
    let b = client(&app).await;
    notify_until_seen(&owner, "live-9-00000001", &[a, b]).await;
    assert_eq!(
        owner.listen_backends().await.unwrap(),
        1,
        "one shared LISTEN for all clients"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn a_new_health_minute_emits_a_health_event() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(
        st.store.clone(),
        st.events.clone(),
        Duration::from_millis(100),
    );
    let c = client(&app).await;
    let live = common::run(&owner, "live").await;
    owner
        .write_batch(&Batch {
            health: vec![common::health(live, 120_000, 0)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(10), async {
        while !c.lock().unwrap().contains("event: health\ndata: 120000\n") {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(got.is_ok(), "no health event: {}", c.lock().unwrap());
    db.drop(owner).await;
}

#[tokio::test]
async fn the_bridge_survives_a_killed_listen_connection() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(
        st.store.clone(),
        st.events.clone(),
        Duration::from_millis(100),
    );
    let c = client(&app).await;
    notify_until_seen(&owner, "live-9-00000002", std::slice::from_ref(&c)).await;
    owner
        .raw_exec(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .await
        .unwrap();
    notify_until_seen(&owner, "live-9-00000003", &[c]).await;
    db.drop(owner).await;
}

#[tokio::test]
async fn clients_past_the_cap_get_a_json_503() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, st) = common::app(&db).await;
    let _all = st
        .stream_slots
        .clone()
        .acquire_many_owned(api::MAX_STREAMS as u32)
        .await
        .unwrap();
    let (status, v) = common::get(&app, "/api/stream").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"], "too_many_streams");
    db.drop(owner).await;
}
