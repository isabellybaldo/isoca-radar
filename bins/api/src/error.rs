//! Every error is JSON `{ "error": code, "detail": text }` (spec §4). Store internals are logged
//! to stderr and never sent to the client.
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use store::StoreError;

/// Spec §4, verbatim.
pub const BUSY_DETAIL: &str = "The database is thinking. Try again in a moment.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// 400: (code, detail)
    BadRequest(&'static str, String),
    /// 404
    NotFound,
    /// 503: (code, detail)
    Unavailable(&'static str, &'static str),
    /// 500
    Internal,
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        if e.is_unavailable() {
            eprintln!("api: database unavailable: {e}");
            ApiError::Unavailable("db_unavailable", BUSY_DETAIL)
        } else {
            eprintln!("api: store error: {e}");
            ApiError::Internal
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, detail) = match self {
            ApiError::BadRequest(code, detail) => (StatusCode::BAD_REQUEST, code, detail),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Nothing here. It may have been a dream.".to_string(),
            ),
            ApiError::Unavailable(code, detail) => {
                (StatusCode::SERVICE_UNAVAILABLE, code, detail.to_string())
            }
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Something broke on our side. It has been written down.".to_string(),
            ),
        };
        (status, Json(json!({ "error": code, "detail": detail }))).into_response()
    }
}

/// Router fallback: unknown paths are JSON 404s too.
pub async fn not_found() -> ApiError {
    ApiError::NotFound
}

/// `method_not_allowed_fallback`: a route exists, but not for this HTTP method. Without this,
/// axum answers with an empty 405 body.
pub async fn method_not_allowed() -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "error": "method_not_allowed",
            "detail": "This method is not supported on this route."
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unavailable_body_is_exactly_the_busy_detail() {
        let resp = ApiError::Unavailable("db_unavailable", BUSY_DETAIL).into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v,
            json!({ "error": "db_unavailable", "detail": "The database is thinking. Try again in a moment." })
        );
    }

    #[tokio::test]
    async fn internal_body_is_exact() {
        let resp = ApiError::Internal.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v,
            json!({ "error": "internal", "detail": "Something broke on our side. It has been written down." })
        );
    }
}
