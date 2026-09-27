//! scanner: read-only Polymarket arbitrage scanner (paper only; spec §8).
#![forbid(unsafe_code)]

pub mod capture;
pub mod checks;
pub mod clock;
pub mod config;
pub mod engine;
pub mod feed;
pub mod guard;
pub mod metrics;
pub mod paper;
pub mod premises;
pub mod replay;
pub mod run;
pub mod shards;
pub mod universe;
pub mod writer;

/// Wall-clock milliseconds (our receive clock).
pub fn now_ms() -> i64 {
    pm_api::ws::now_us() / 1000
}

/// git sha baked in at image build time (SCANNER_VERSION), "dev" otherwise.
pub fn version() -> &'static str {
    option_env!("SCANNER_VERSION").unwrap_or("dev")
}
