use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bad field {field}: {detail}")]
    Field { field: &'static str, detail: String },
    #[error("websocket: {0}")]
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("pagination made no progress after {0} pages (cursor ignored?)")]
    Stuck(usize),
    #[error("no frame for {silent_ms} ms on conn {conn}")]
    Stale { conn: usize, silent_ms: u64 },
    #[error("timed out: {0}")]
    Timeout(&'static str),
    #[error("receiver full; frame dropped on conn {conn}")]
    Dropped { conn: usize },
}

impl From<tokio_tungstenite::tungstenite::Error> for ApiError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        ApiError::Ws(Box::new(e))
    }
}

impl ApiError {
    pub fn field(field: &'static str, detail: impl Into<String>) -> Self {
        ApiError::Field {
            field,
            detail: detail.into(),
        }
    }

    /// A short, safe-to-log description that never includes a request URL — unlike this error's
    /// own `Display` (`{0}` on `Http`/`Json` delegates to the underlying error, and `reqwest::Error`
    /// prints the URL it failed on, which for a Gamma page includes the pagination cursor).
    /// `Http` is the only variant `retry::retryable` ever calls this on in practice, but every
    /// variant has a safe answer.
    pub fn short(&self) -> String {
        match self {
            ApiError::Http(e) => {
                if let Some(status) = e.status() {
                    format!("http {status}")
                } else if e.is_timeout() {
                    "timeout".to_string()
                } else if e.is_connect() {
                    "connect error".to_string()
                } else if e.is_body() || e.is_decode() {
                    "body read error".to_string()
                } else {
                    "request error".to_string()
                }
            }
            ApiError::Json(_) => "json error".to_string(),
            ApiError::Field { field, .. } => format!("bad field {field}"),
            ApiError::Ws(_) => "websocket error".to_string(),
            ApiError::Stuck(pages) => format!("stuck after {pages} pages"),
            ApiError::Stale { conn, .. } => format!("stale conn {conn}"),
            ApiError::Timeout(what) => format!("timed out: {what}"),
            ApiError::Dropped { conn } => format!("dropped on conn {conn}"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::routes::{CLOB_OK, LOCAL_TEST_SCHEME};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// `short()` must never include the request URL — proven here by putting a "cursor" in the
    /// query string and asserting it's absent from `short()` while still present in `Display`
    /// (otherwise this test would prove nothing).
    #[tokio::test]
    async fn short_never_leaks_the_url_or_its_cursor() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await;
        });
        let client = reqwest::Client::builder().build().unwrap();
        let err = client
            .get(format!(
                "{LOCAL_TEST_SCHEME}{addr}{CLOB_OK}?after_cursor=SUPER_SECRET_CURSOR"
            ))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap_err();
        let e = ApiError::from(err);
        assert!(
            e.to_string().contains("SUPER_SECRET_CURSOR"),
            "sanity check: Display does include the cursor"
        );
        assert!(!e.short().contains("SUPER_SECRET_CURSOR"));
        assert_eq!(e.short(), "http 503 Service Unavailable");
    }

    #[test]
    fn short_covers_every_variant_without_panicking() {
        assert_eq!(ApiError::field("f", "d").short(), "bad field f");
        assert_eq!(ApiError::Stuck(3).short(), "stuck after 3 pages");
        assert_eq!(ApiError::Timeout("x").short(), "timed out: x");
        assert_eq!(ApiError::Dropped { conn: 2 }.short(), "dropped on conn 2");
        assert_eq!(
            ApiError::Stale {
                conn: 1,
                silent_ms: 5
            }
            .short(),
            "stale conn 1"
        );
        let json_err: ApiError = serde_json::from_str::<serde_json::Value>("bad")
            .unwrap_err()
            .into();
        assert_eq!(json_err.short(), "json error");
    }
}
