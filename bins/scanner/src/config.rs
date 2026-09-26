//! scanner.toml (spec §8). Integers only: no floats anywhere in the scanner.
use anyhow::Context;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub universe: UniverseCfg,
    pub shards: ShardsCfg,
    pub connect: ConnectCfg,
    pub engine: EngineCfg,
    pub recheck: RecheckCfg,
    pub capture: CaptureCfg,
    pub breaker: BreakerCfg,
    pub store: StoreCfg,
    pub paper: PaperCfg,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UniverseCfg {
    pub refresh_secs: u64,
    pub warm_start_max_age_secs: u64,
    pub canary_pairs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardsCfg {
    pub max_connections: usize,
    pub hot_events: usize,
    pub hot_shard_tokens: usize,
    pub cold_shard_tokens: usize,
    pub retier_every_refreshes: u32,
    pub per_shard_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectCfg {
    pub per_sec: usize,
    pub per_min: usize,
    pub backoff_base_ms: u64,
    pub backoff_cap_ms: u64,
    pub healthy_session_ms: i64,
    pub park_after_failures: usize,
    pub park_window_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineCfg {
    pub gap_ms: i64,
    pub stale_lag_ms: i64,
    pub suspect_window_ms: i64,
    pub l1_suspect_window_ms: i64,
    pub drift_settle_ms: i64,
    pub channel_capacity: usize,
    /// Per-(kind, token) rate limit on top_mismatch/crossed_book integrity rows (S3a).
    pub issue_throttle_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecheckCfg {
    pub offsets_ms: Vec<i64>,
    pub rest_per_sec: usize,
    pub drift_tokens_per_min: usize,
    pub l1_legs_per_min: usize,
    /// Episodes re-checked at once; more are recorded as skipped (S3a).
    pub max_in_flight: usize,
    /// REST requests/s only resnapshots may use (S3a): re-checks can't starve a book repair.
    pub resnapshot_reserved_per_sec: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureCfg {
    pub dir: String,
    pub pause_below_gb: u64,
    pub resume_above_gb: u64,
    pub retention_hours: i64,
    /// Hard cap on the capture directory. The container can't see the HOST drive's free space
    /// (Docker Desktop's VM reports its own huge virtual disk), so free-space checks alone can
    /// fill a Windows drive: over budget, the oldest hour files are deleted.
    pub max_capture_gb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakerCfg {
    pub max_starts: i64,
    pub window_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreCfg {
    pub flush_ms: u64,
    pub max_pending_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperCfg {
    /// Run the paper trader (live and replay). Off = scanner only, as in S2.
    pub enabled: bool,
    /// The frozen policies file (spec §11). Its sha256 joins the run's config_hash.
    pub policies: String,
    /// Ask Gamma about held markets this often (and soon after a ws market_resolved).
    pub resolver_every_secs: u64,
    /// Save live account state to paper_state this often (restored at start).
    pub state_save_every_secs: u64,
}

impl Config {
    /// Parse and validate; returns the config and its hash (hex sha256 of the exact text).
    pub fn parse(text: &str) -> anyhow::Result<(Config, String)> {
        let cfg: Config = toml::from_str(text).context("parse scanner config")?;
        cfg.validate()?;
        let mut hash = String::with_capacity(64);
        for b in Sha256::digest(text.as_bytes()) {
            let _ = write!(hash, "{b:02x}");
        }
        Ok((cfg, hash))
    }

    pub fn load(path: &Path) -> anyhow::Result<(Config, String)> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&text)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let s = &self.shards;
        anyhow::ensure!(
            (1..=40).contains(&s.max_connections),
            "shards.max_connections must be 1..=40 (Phase 0 measured the cap only up to 40)"
        );
        anyhow::ensure!(
            s.cold_shard_tokens >= 1 && s.cold_shard_tokens <= 1000,
            "shards.cold_shard_tokens must be 1..=1000 (Phase 0: 5,000/conn fails)"
        );
        anyhow::ensure!(
            s.hot_shard_tokens >= 1 && s.hot_shard_tokens <= s.cold_shard_tokens,
            "shards.hot_shard_tokens must be 1..=cold_shard_tokens"
        );
        anyhow::ensure!(
            s.per_shard_capacity >= 1,
            "shards.per_shard_capacity must be ≥ 1"
        );
        anyhow::ensure!(
            s.retier_every_refreshes >= 1,
            "shards.retier_every_refreshes must be ≥ 1"
        );
        let c = &self.connect;
        anyhow::ensure!(
            (1..=2).contains(&c.per_sec),
            "connect.per_sec must be 1..=2 (spec §8.1.2)"
        );
        anyhow::ensure!(
            c.per_min >= c.per_sec && c.per_min <= 20,
            "connect.per_min must be per_sec..=20"
        );
        anyhow::ensure!(
            c.backoff_base_ms >= 1000 && c.backoff_cap_ms >= c.backoff_base_ms,
            "connect backoff 1 s → cap"
        );
        anyhow::ensure!(
            c.park_after_failures >= 1,
            "connect.park_after_failures must be ≥ 1"
        );
        anyhow::ensure!(
            !self.recheck.offsets_ms.is_empty() && self.recheck.offsets_ms.iter().all(|&o| o > 0),
            "recheck.offsets_ms must be non-empty and positive"
        );
        anyhow::ensure!(
            self.recheck.rest_per_sec >= 1,
            "recheck.rest_per_sec must be ≥ 1"
        );
        anyhow::ensure!(
            self.recheck.max_in_flight >= 1,
            "recheck.max_in_flight must be ≥ 1"
        );
        anyhow::ensure!(
            self.recheck.resnapshot_reserved_per_sec >= 1
                && self.recheck.resnapshot_reserved_per_sec < self.recheck.rest_per_sec,
            "recheck.resnapshot_reserved_per_sec must be 1..rest_per_sec (re-checks need a share too)"
        );
        anyhow::ensure!(
            self.capture.resume_above_gb > self.capture.pause_below_gb,
            "capture.resume_above_gb must exceed pause_below_gb"
        );
        anyhow::ensure!(
            self.capture.max_capture_gb >= 1,
            "capture.max_capture_gb must be ≥ 1"
        );
        anyhow::ensure!(
            self.capture.retention_hours >= 1,
            "capture.retention_hours must be ≥ 1"
        );
        anyhow::ensure!(
            self.breaker.max_starts >= 1 && self.breaker.window_secs >= 60,
            "breaker limits"
        );
        anyhow::ensure!(
            self.engine.channel_capacity >= 1024,
            "engine.channel_capacity must be ≥ 1024"
        );
        anyhow::ensure!(
            self.engine.issue_throttle_ms >= 0,
            "engine.issue_throttle_ms must be ≥ 0"
        );
        anyhow::ensure!(
            self.paper.resolver_every_secs >= 60,
            "paper.resolver_every_secs must be ≥ 60 (Gamma is polled, not hammered)"
        );
        anyhow::ensure!(
            (10..=3600).contains(&self.paper.state_save_every_secs),
            "paper.state_save_every_secs must be 10..=3600"
        );
        anyhow::ensure!(
            !self.paper.enabled || !self.paper.policies.trim().is_empty(),
            "paper.policies must name the policies file"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const REPO: &str = include_str!("../../../config/scanner.toml");

    #[test]
    fn repo_config_parses_with_a_stable_hash() {
        let (cfg, h1) = Config::parse(REPO).unwrap();
        let (_, h2) = Config::parse(REPO).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert_eq!(cfg.shards.max_connections, 40);
        assert_eq!(cfg.recheck.offsets_ms, vec![1000, 2000, 5000]);
        assert_eq!(
            (
                cfg.recheck.max_in_flight,
                cfg.recheck.resnapshot_reserved_per_sec
            ),
            (32, 3)
        );
        assert!(cfg.paper.enabled);
        assert_eq!(cfg.paper.policies, "/etc/cute-trading/policies.toml");
        assert_eq!(
            (
                cfg.paper.resolver_every_secs,
                cfg.paper.state_save_every_secs
            ),
            (600, 60)
        );
    }

    #[test]
    fn rejects_a_hammering_resolver_or_odd_save_cadence() {
        let fast = REPO.replace("resolver_every_secs = 600", "resolver_every_secs = 10");
        assert!(Config::parse(&fast).is_err());
        let never = REPO.replace("state_save_every_secs = 60", "state_save_every_secs = 0");
        assert!(Config::parse(&never).is_err());
    }

    #[test]
    fn rejects_unknown_fields_and_phase0_violations() {
        assert!(Config::parse(&format!("{REPO}\n[extra]\nx = 1\n")).is_err());
        let too_many = REPO.replace("max_connections = 40", "max_connections = 41");
        assert!(Config::parse(&too_many).is_err());
        let too_big = REPO.replace("cold_shard_tokens = 1000", "cold_shard_tokens = 5000");
        assert!(Config::parse(&too_big).is_err());
        let fast = REPO.replace("per_sec = 2", "per_sec = 5");
        assert!(Config::parse(&fast).is_err());
        let all_reserved = REPO.replace(
            "resnapshot_reserved_per_sec = 3",
            "resnapshot_reserved_per_sec = 10",
        );
        assert!(
            Config::parse(&all_reserved).is_err(),
            "re-checks need a share too"
        );
        let no_cap = REPO.replace("max_in_flight = 32", "max_in_flight = 0");
        assert!(Config::parse(&no_cap).is_err());
    }
}
