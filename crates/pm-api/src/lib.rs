//! pm-api: the ONLY crate that touches the network. Read-only Polymarket access (spec §16).
#![forbid(unsafe_code)]

pub mod backoff;
pub mod clob;
pub mod error;
pub mod gamma;
mod http;
mod retry;
pub mod routes;
pub mod wire;
pub mod ws;

pub use error::ApiError;
pub use http::RttSink;

static TLS_INIT: std::sync::Once = std::sync::Once::new();

/// Install the ring crypto provider for rustls once per process (idempotent).
/// Consumers no longer need to call this themselves: `http_client()` and `run_connection`
/// both call it internally. Kept public for compatibility with existing callers.
pub fn init_tls() {
    TLS_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
