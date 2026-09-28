//! The archive layout (spec 5b §3, §4.4): <archive>/YYYY-MM-DD/ holds hourly parts
//! HH.<table>.parquet and HH.hour.json until the day is complete, then the daily
//! <table>.parquet files and manifest.json. <archive>/_state/ holds the checkpoints.
use std::path::{Path, PathBuf};
use tape::time::{civil, day_name};

pub const MANIFEST: &str = "manifest.json";

pub fn day_dir(archive: &Path, ms: i64) -> PathBuf {
    archive.join(day_name(ms))
}

pub fn hour_number(ms: i64) -> u32 {
    civil(ms).3
}

pub fn part_path(day: &Path, hh: u32, table: &str) -> PathBuf {
    day.join(format!("{hh:02}.{table}.parquet"))
}

pub fn sidecar_path(day: &Path, hh: u32) -> PathBuf {
    day.join(format!("{hh:02}.hour.json"))
}

pub fn daily_path(day: &Path, table: &str) -> PathBuf {
    day.join(format!("{table}.parquet"))
}
