//! Generic retry-with-backoff for a single flaky request. Gamma is intermittently slow: most
//! `/events/keyset` pages return in well under a second, but some take tens of seconds and a rare
//! one longer still (measured on the VPS 2026-09-27). `with_retry` wraps one page fetch (never
//! `fetch_all`, so a retry can't re-send earlier pages) and is pure aside from the two closures
//! the caller supplies, so the backoff sequence and the retry/give-up decision are testable
//! without a real network or a real sleep.
use crate::ApiError;
use std::future::Future;
use std::time::Duration;

/// Attempt `op` up to `attempts` times. After a failed *retryable* attempt (see [`retryable`]),
/// awaits `sleep` with `backoff[attempt - 1]` (1-based attempt, so the delay after the first
/// failure is `backoff[0]`) before trying again; running out of `backoff` entries just skips the
/// sleep. Gives up at once — no sleep — on a non-retryable error, or once `attempts` have been
/// made, returning the last error either way.
pub(crate) async fn with_retry<T, F, Fut, S, SFut>(
    attempts: usize,
    backoff: &[Duration],
    mut op: F,
    mut sleep: S,
) -> Result<T, ApiError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
    S: FnMut(Duration) -> SFut,
    SFut: Future<Output = ()>,
{
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if !retryable(&e) || attempt >= attempts {
                    return Err(e);
                }
                if let Some(&d) = backoff.get(attempt - 1) {
                    sleep(d).await;
                }
            }
        }
    }
}

/// Whether a failed attempt is worth retrying: timeouts, connect/transport errors, and HTTP
/// 429/5xx. Anything else — other 4xx, a JSON/field parse error, `Stuck`, ... — means the response
/// itself is the problem, not the attempt, so retrying would just repeat it.
pub(crate) fn retryable(e: &ApiError) -> bool {
    match e {
        ApiError::Http(err) => {
            err.is_timeout()
                || err.is_connect()
                || err.is_request()
                || err.is_body()
                || err.is_decode()
                || err
                    .status()
                    .is_some_and(|s| s.as_u16() == 429 || s.is_server_error())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::routes::{CLOB_OK, LOCAL_TEST_SCHEME};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A cheap, fast, *real* `ApiError::Http` (connection refused): reqwest errors have no public
    /// constructor, so the local-server pattern already used in `http.rs`/`clob.rs` tests is the
    /// only way to fabricate one. Connect-refused resolves near-instantly, so this never behaves
    /// like a real sleep.
    async fn connect_refused_err() -> ApiError {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // nothing listens: connection refused
        let client = reqwest::Client::builder().build().unwrap();
        let err = client
            .get(format!("{LOCAL_TEST_SCHEME}{addr}{CLOB_OK}"))
            .send()
            .await
            .unwrap_err();
        ApiError::from(err)
    }

    fn recording_sleep() -> (
        impl FnMut(Duration) -> std::future::Ready<()>,
        Arc<Mutex<Vec<Duration>>>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sleep = move |d: Duration| {
            s2.lock().unwrap().push(d);
            std::future::ready(())
        };
        (sleep, seen)
    }

    const BACKOFF: [Duration; 3] = [
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_secs(10),
    ];

    #[tokio::test]
    async fn succeeds_on_the_third_attempt() {
        let calls = AtomicUsize::new(0);
        let (sleep, seen) = recording_sleep();
        let out = with_retry(
            4,
            &BACKOFF,
            || async {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(connect_refused_err().await)
                } else {
                    Ok(42)
                }
            },
            sleep,
        )
        .await
        .unwrap();
        assert_eq!(out, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(seen.lock().unwrap().len(), 2, "two sleeps before try 3");
    }

    #[tokio::test]
    async fn gives_up_after_4_and_returns_the_last_error() {
        let calls = AtomicUsize::new(0);
        let (sleep, seen) = recording_sleep();
        let r: Result<(), ApiError> = with_retry(
            4,
            &BACKOFF,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(connect_refused_err().await)
            },
            sleep,
        )
        .await;
        assert!(matches!(r, Err(ApiError::Http(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 4, "exactly 4 attempts");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(10)
            ],
            "backoff sequence is exactly 2s, 5s, 10s"
        );
    }

    #[tokio::test]
    async fn a_non_retryable_error_returns_immediately_with_no_sleep() {
        let calls = AtomicUsize::new(0);
        let (sleep, seen) = recording_sleep();
        let r: Result<(), ApiError> = with_retry(
            4,
            &BACKOFF,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ApiError::field("x", "not retryable"))
            },
            sleep,
        )
        .await;
        assert!(matches!(r, Err(ApiError::Field { .. })));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no retry attempted");
        assert!(seen.lock().unwrap().is_empty(), "no sleep on give-up");
    }

    #[tokio::test]
    async fn a_reqwest_timeout_is_retryable() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let err = client
            .get(format!("{LOCAL_TEST_SCHEME}{addr}{CLOB_OK}"))
            .send()
            .await
            .unwrap_err();
        assert!(err.is_timeout());
        assert!(retryable(&ApiError::from(err)));
    }

    async fn status_err(status_line: &str) -> ApiError {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status_line = status_line.to_string();
        tokio::spawn(async move {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let resp = format!("{status_line}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            let _ = s.write_all(resp.as_bytes()).await;
        });
        let client = reqwest::Client::builder().build().unwrap();
        let err = client
            .get(format!("{LOCAL_TEST_SCHEME}{addr}{CLOB_OK}"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap_err();
        ApiError::from(err)
    }

    #[tokio::test]
    async fn a_503_is_retryable() {
        assert!(retryable(
            &status_err("HTTP/1.1 503 Service Unavailable").await
        ));
    }

    #[tokio::test]
    async fn a_429_is_retryable() {
        assert!(retryable(
            &status_err("HTTP/1.1 429 Too Many Requests").await
        ));
    }

    #[tokio::test]
    async fn a_404_is_not_retryable() {
        assert!(!retryable(&status_err("HTTP/1.1 404 Not Found").await));
    }

    #[test]
    fn a_json_error_is_not_retryable() {
        let e: ApiError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        assert!(!retryable(&e));
    }
}
