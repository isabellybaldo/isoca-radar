//! arb-core: pure math for the read-only Polymarket arbitrage lab.
//! No IO, no clock, no floats. See docs/superpowers/specs/2026-09-22-arb-scanner-design.md §5.
#![forbid(unsafe_code)]

pub mod book;
pub mod detect;
pub mod episode;
pub mod fee;
pub mod fill;
pub mod money;
pub mod premises;
pub mod stats;
