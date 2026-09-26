//! Environment-only configuration (the api has no config file). The database URL carries a
//! password, so Debug redacts it.
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// postgres://analyze_ro:…@postgres:5432/cute_trading
    pub database_url: String,
    pub bind: String,
    /// Pool size; one connection is held by the shared LISTEN.
    pub max_conns: u32,
    pub statement_timeout_ms: u32,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("bind", &self.bind)
            .field("max_conns", &self.max_conns)
            .field("statement_timeout_ms", &self.statement_timeout_ms)
            .finish()
    }
}

fn parse_or(v: Option<String>, default: u32, name: &str) -> anyhow::Result<u32> {
    match v {
        None => Ok(default),
        Some(s) => s
            .parse()
            .map_err(|_| anyhow::anyhow!("{name} must be a positive integer")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let database_url = get("DATABASE_URL")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the analyze_ro URL)"))?;
        let bind = get("API_BIND").unwrap_or_else(|| "0.0.0.0:8081".to_string());
        let max_conns = parse_or(get("API_MAX_CONNS"), 4, "API_MAX_CONNS")?;
        let statement_timeout_ms = parse_or(
            get("API_STATEMENT_TIMEOUT_MS"),
            5_000,
            "API_STATEMENT_TIMEOUT_MS",
        )?;
        anyhow::ensure!(
            (2..=16).contains(&max_conns),
            "API_MAX_CONNS must be 2..=16 (one connection is the shared LISTEN)"
        );
        anyhow::ensure!(
            (100..=60_000).contains(&statement_timeout_ms),
            "API_STATEMENT_TIMEOUT_MS must be 100..=60000"
        );
        Ok(Self {
            database_url,
            bind,
            max_conns,
            statement_timeout_ms,
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
    fn defaults_apply_and_the_url_is_required() {
        let c = cfg(&[(
            "DATABASE_URL",
            "postgres://analyze_ro:pw@postgres/cute_trading",
        )])
        .unwrap();
        assert_eq!(
            (c.bind.as_str(), c.max_conns, c.statement_timeout_ms),
            ("0.0.0.0:8081", 4, 5_000)
        );
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[("DATABASE_URL", "")]).is_err());
    }

    #[test]
    fn bounds_are_enforced_and_debug_hides_the_password() {
        let url = (
            "DATABASE_URL",
            "postgres://analyze_ro:hunter2@postgres/cute_trading",
        );
        assert!(cfg(&[url, ("API_MAX_CONNS", "1")]).is_err());
        assert!(cfg(&[url, ("API_MAX_CONNS", "x")]).is_err());
        assert!(cfg(&[url, ("API_STATEMENT_TIMEOUT_MS", "99")]).is_err());
        let c = cfg(&[url, ("API_BIND", "127.0.0.1:9000")]).unwrap();
        assert_eq!(c.bind, "127.0.0.1:9000");
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
