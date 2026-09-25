//! sim: the pure paper trader (spec §5.6, §11). No IO, no clock, no floats: time arrives on the
//! inputs, so live, replay and tests produce identical ledgers (spec §9.3). It produces ledger
//! entries only; there is no code path to an exchange (spec §16 #5).
#![forbid(unsafe_code)]

pub mod config;
pub mod consumed;
pub mod latency;
pub mod types;
pub mod venue;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SimError {
    #[error("invalid sim config: {0}")]
    Config(String),
}
