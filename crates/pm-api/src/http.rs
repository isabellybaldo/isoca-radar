//! pm-api owns its HTTP client (spec F1/F2): no redirects, HTTPS only, a fixed identifying
//! user agent, explicit timeouts, and no default headers. `reqwest` is banned everywhere else
//! in the workspace (deny.toml `[bans]`), so this is the only place a `reqwest::Client` is built.
use crate::ApiError;
use std::time::Duration;

use std::sync::Arc;
use std::time::Instant;

/// Where pm-api reports each CLOB request's duration in ms (send → response headers), so the
/// scanner can put p50/p99 in its health row (Phase 5a spec §3). It times requests it already
/// makes and adds none. Failed requests (timeouts, refusals) report nothing.
pub type RttSink = Arc<dyn Fn(i64) + Send + Sync>;

pub(crate) async fn send_timed(
    req: reqwest::RequestBuilder,
    sink: Option<&RttSink>,
) -> Result<reqwest::Response, reqwest::Error> {
    let t = Instant::now();
    let resp = req.send().await;
    if let (Some(s), Ok(_)) = (sink, &resp) {
        s(i64::try_from(t.elapsed().as_millis()).unwrap_or(i64::MAX));
    }
    resp
}

pub(crate) fn http_client() -> Result<reqwest::Client, ApiError> {
    crate::init_tls();
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent("isoca-radar-research/0.1 (read-only)")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::field("http_client", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn recorder() -> (RttSink, Arc<Mutex<Vec<i64>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sink: RttSink = Arc::new(move |ms| s2.lock().unwrap().push(ms));
        (sink, seen)
    }

    #[tokio::test]
    async fn a_successful_request_reports_its_round_trip_to_the_sink() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        });
        let (sink, seen) = recorder();
        // A plain-http test client: the production client is https-only, the timing is the same code.
        let client = reqwest::Client::builder().build().unwrap();
        let resp = send_timed(client.get(format!("http://{addr}/ok")), Some(&sink))
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let v = seen.lock().unwrap().clone();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!((20..5_000).contains(&v[0]), "{v:?}");
    }

    #[tokio::test]
    async fn a_failed_request_reports_nothing_and_no_sink_is_fine() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // nothing listens: connection refused
        let (sink, seen) = recorder();
        let client = reqwest::Client::builder().build().unwrap();
        assert!(
            send_timed(client.get(format!("http://{addr}/ok")), Some(&sink))
                .await
                .is_err()
        );
        assert!(seen.lock().unwrap().is_empty());
        assert!(
            send_timed(client.get(format!("http://{addr}/ok")), None)
                .await
                .is_err()
        );
    }
}
