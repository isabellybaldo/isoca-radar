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
}
