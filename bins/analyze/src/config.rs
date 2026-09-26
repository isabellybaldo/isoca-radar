//! Environment-only configuration. The database URL carries a password, so Debug redacts it.
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// postgres://analyze_rw:…@postgres:5432/cute_trading
    pub database_url: String,
    pub every_secs: u64,
    pub statement_timeout_ms: u32,
    pub catchup_pause_ms: u64,
    pub policies_path: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("every_secs", &self.every_secs)
            .field("statement_timeout_ms", &self.statement_timeout_ms)
            .field("catchup_pause_ms", &self.catchup_pause_ms)
            .field("policies_path", &self.policies_path)
            .finish()
    }
}

fn num<T: std::str::FromStr>(v: Option<String>, default: T, name: &str) -> anyhow::Result<T> {
    match v {
        None => Ok(default),
        Some(s) => s
            .parse()
            .map_err(|_| anyhow::anyhow!("{name} must be a non-negative integer")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let database_url = get("DATABASE_URL")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the analyze_rw URL)"))?;
        let every_secs: u64 = num(get("ANALYZE_EVERY_SECS"), 900, "ANALYZE_EVERY_SECS")?;
        let statement_timeout_ms: u32 = num(
            get("ANALYZE_STATEMENT_TIMEOUT_MS"),
            10_000,
            "ANALYZE_STATEMENT_TIMEOUT_MS",
        )?;
        let catchup_pause_ms: u64 = num(
            get("ANALYZE_CATCHUP_PAUSE_MS"),
            2_000,
            "ANALYZE_CATCHUP_PAUSE_MS",
        )?;
        let policies_path = get("ANALYZE_POLICIES")
            .unwrap_or_else(|| "/etc/cute-trading/policies.toml".to_string());
        anyhow::ensure!(
            (60..=3_600).contains(&every_secs),
            "ANALYZE_EVERY_SECS must be 60..=3600 (spec §7)"
        );
        anyhow::ensure!(
            (1_000..=60_000).contains(&statement_timeout_ms),
            "ANALYZE_STATEMENT_TIMEOUT_MS must be 1000..=60000"
        );
        anyhow::ensure!(
            catchup_pause_ms <= 10_000,
            "ANALYZE_CATCHUP_PAUSE_MS must be 0..=10000"
        );
        Ok(Self {
            database_url,
            every_secs,
            statement_timeout_ms,
            catchup_pause_ms,
            policies_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_bounds_and_a_redacted_url() {
        let url = (
            "DATABASE_URL",
            "postgres://analyze_rw:hunter2@postgres/cute_trading",
        );
        let c = cfg(&[url]).unwrap();
        assert_eq!(
            (c.every_secs, c.statement_timeout_ms, c.catchup_pause_ms),
            (900, 10_000, 2_000)
        );
        assert_eq!(c.policies_path, "/etc/cute-trading/policies.toml");
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "59")]).is_err());
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "3601")]).is_err());
        assert_eq!(
            cfg(&[url, ("ANALYZE_EVERY_SECS", "60")])
                .unwrap()
                .every_secs,
            60
        );
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "x")]).is_err());
        assert!(cfg(&[url, ("ANALYZE_CATCHUP_PAUSE_MS", "10001")]).is_err());
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
