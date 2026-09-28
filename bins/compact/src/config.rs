//! config/compact.toml (spec 5b §7). Integers only; validated at startup; its sha256 goes into
//! every day's manifest.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub raw_dir: String,
    pub archive_dir: String,
    pub depth_every_secs: i64,
    pub depth_levels: usize,
    pub l2_before_secs: i64,
    pub l2_after_secs: i64,
    pub l2_max_window_secs: i64,
    pub archive_budget_gb: u64,
    pub lag_alarm_hours: i64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Config {
    /// Parse and validate; returns the config and the sha256 of its text.
    pub fn parse(text: &str) -> anyhow::Result<(Config, String)> {
        let c: Config = toml::from_str(text).context("compact.toml")?;
        c.validate()?;
        Ok((c, sha256_hex(text.as_bytes())))
    }

    pub fn load(path: &Path) -> anyhow::Result<(Config, String)> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&text)
    }

    pub fn budget_bytes(&self) -> u64 {
        self.archive_budget_gb.saturating_mul(1_000_000_000)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.raw_dir.is_empty() && !self.archive_dir.is_empty(),
            "raw_dir and archive_dir must be set"
        );
        anyhow::ensure!(
            (60..=3_600).contains(&self.depth_every_secs) && 3_600 % self.depth_every_secs == 0,
            "depth_every_secs must be 60..=3600 and divide 3600 (snapshots line up with hours)"
        );
        anyhow::ensure!(
            (1..=50).contains(&self.depth_levels),
            "depth_levels must be 1..=50"
        );
        anyhow::ensure!(
            (0..=3_600).contains(&self.l2_before_secs) && (0..=3_600).contains(&self.l2_after_secs),
            "l2_before_secs and l2_after_secs must be 0..=3600 (an hour is compacted once the next one closes)"
        );
        anyhow::ensure!(
            (60..=7_200).contains(&self.l2_max_window_secs),
            "l2_max_window_secs must be 60..=7200"
        );
        anyhow::ensure!(
            (1..=70).contains(&self.archive_budget_gb),
            "archive_budget_gb must be 1..=70 (the project's 75 GB ceiling, spec §4.4)"
        );
        anyhow::ensure!(
            (1..=48).contains(&self.lag_alarm_hours),
            "lag_alarm_hours must be 1..=48"
        );
        Ok(())
    }
}
