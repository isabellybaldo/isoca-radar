//! pm-api owns its HTTP client (spec F1/F2): no redirects, HTTPS only, a fixed identifying
//! user agent, explicit timeouts, and no default headers. `reqwest` is banned everywhere else
//! in the workspace (deny.toml `[bans]`), so this is the only place a `reqwest::Client` is built.
use crate::ApiError;
use std::time::Duration;

pub(crate) fn http_client() -> Result<reqwest::Client, ApiError> {
    crate::init_tls();
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent("cute-trading-research/0.1 (read-only)")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| ApiError::field("http_client", e.to_string()))
}
