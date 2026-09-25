//! pm-api: the ONLY crate that touches the network. Read-only Polymarket access (spec §16).
#![forbid(unsafe_code)]

pub mod backoff;
pub mod clob;
pub mod error;
pub mod gamma;
pub mod routes;
pub mod wire;

pub use error::ApiError;

/// Install the ring crypto provider for rustls once per process. Call before any client is built:
/// with more than one provider in the dependency graph rustls cannot choose on its own.
pub fn init_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
