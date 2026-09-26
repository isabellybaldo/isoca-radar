//! analyze: the live-cards job (Phase 5a spec §7). Every ANALYZE_EVERY_SECS it rebuilds the dirty
//! hourly cells of each card family through `store`, updates its windows, and writes card_results.
//! No network client (spec §2), paper only (parent §16), writes only the card tables.
#![forbid(unsafe_code)]

pub mod config;
pub mod guard;
pub mod scenarios;
pub mod tick;

use store::Store;

/// git sha baked in at image build time (the image's SCANNER_VERSION build arg), "dev" otherwise.
pub fn version() -> &'static str {
    option_env!("SCANNER_VERSION").unwrap_or("dev")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Spec §2: refuse to run with a role that can write anything but the card tables.
pub async fn ensure_writes_only_cards(store: &Store) -> anyhow::Result<()> {
    refuse_other_writes(&store.role_writes_outside_cards().await?)
}

/// The refusal itself, given the tables the role could write outside the card tables (main uses
/// it to tell a refusal, which is fatal, from a database hiccup, which is retried).
pub fn refuse_other_writes(bad: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: the database role can write {} (connect as analyze_rw, which writes only \
         card_cells, card_results and card_meta)",
        bad.join(", ")
    );
    Ok(())
}
