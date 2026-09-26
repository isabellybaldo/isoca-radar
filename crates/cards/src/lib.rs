//! cards: the pure maths of the Phase 5a live cards (spec §5–§7). Hourly merge-exact cells, the
//! incremental windows over them, and the card bodies the api serves. No IO, no clock, no floats:
//! `bins/analyze` reads Postgres through `store` and calls into this crate.
#![forbid(unsafe_code)]

pub mod cells;
pub mod stats;
pub mod window;

pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;

/// The UTC hour `ms` falls in.
pub fn hour_of(ms: i64) -> i64 {
    ms.div_euclid(HOUR_MS) * HOUR_MS
}
