//! compact: every closed raw capture hour → the Parquet archive (spec 5b). No network client
//! (spec §2), paper only (parent §16); Postgres only through store as compact_rw, which writes
//! only compact_hours and compact_status.
#![forbid(unsafe_code)]

pub mod archive;
pub mod bench;
pub mod config;
pub mod guard;
pub mod hour;
pub mod pq;
pub mod reduce;
pub mod rows;
pub mod run;
pub mod state;
pub mod verify;
pub mod windows;

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

/// Refuse to run with a role that can write anything but compact_hours and compact_status.
pub fn refuse_other_writes(bad: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: the database role can write {} (connect as compact_rw, which writes only \
         compact_hours and compact_status)",
        bad.join(", ")
    );
    Ok(())
}
