#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
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
    assert_eq!(
        owner.terminate_listen_backends().await.unwrap(),
        1,
        "expected to kill exactly one LISTEN backend"
    );
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

#[tokio::test]
async fn a_dropped_stream_releases_its_slot_for_a_new_client() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let mut handles = Vec::new();
    for _ in 0..api::MAX_STREAMS {
        let resp = app
            .clone()
            .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        handles.push(tokio::spawn(async move {
            let mut body = resp.into_body().into_data_stream();
            while body.next().await.is_some() {}
        }));
    }
    let (status, _) = common::get(&app, "/api/stream").await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "all {} slots should be taken",
        api::MAX_STREAMS
    );

    // Drop one held stream (abort its reader task) and give the permit a moment to release.
    // Note: an SSE body never ends on its own, so probing with `common::get` (which reads the
    // full body via `to_bytes`) would hang forever on a 200; check the status only and let the
    // probe response drop immediately, which releases any permit it briefly held.
    handles.remove(0).abort();
    let freed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = app
                .clone()
                .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status();
            if status == StatusCode::OK {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(freed.is_ok(), "a dropped stream never freed its slot");

    for h in handles {
        h.abort();
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn graceful_shutdown_ends_open_streams_and_closes_the_store() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ro = Store::connect_read_only(db.opts.clone(), 4, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ro_for_run = ro.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let run = tokio::spawn(async move {
        api::run_on(listener, ro_for_run, Duration::from_millis(100), async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    // Open one real, held-open SSE client over a real socket (not `oneshot`, so this exercises
    // the actual `axum::serve` graceful-shutdown path) and confirm it connected.
    let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
    sock.write_all(
        format!("GET /api/stream HTTP/1.1\r\nHost: {addr}\r\nConnection: keep-alive\r\n\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let head = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(head.contains("200"), "unexpected response head: {head}");
    assert!(
        head.contains("text/event-stream"),
        "not an SSE response: {head}"
    );

    shutdown_tx.send(()).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), run).await;
    let joined = outcome.expect(
        "graceful shutdown did not complete within 3s (an open SSE stream or the LISTEN pool \
         connection is blocking axum::serve/store.close)",
    );
    joined
        .expect("run_on task panicked")
        .expect("run_on returned an error");
    assert!(ro.pool().is_closed(), "store pool was not closed by run_on");

    drop(sock);
    db.drop(owner).await;
}
