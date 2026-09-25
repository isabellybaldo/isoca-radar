//! `runs`: every start/exit of a long-running service. Feeds the crash-loop breaker (§8.1.2).
use crate::text::clean;
use crate::{NewRun, Store, StoreError};

impl Store {
    pub async fn start_run(&self, r: &NewRun) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "INSERT INTO runs (service, started_ms, version, config_hash, mode) \
             VALUES ($1, $2, $3, $4, $5) RETURNING run_id",
        )
        .bind(clean(&r.service))
        .bind(r.started_ms)
        .bind(clean(&r.version))
        .bind(clean(&r.config_hash))
        .bind(clean(&r.mode))
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn end_run(
        &self,
        run_id: i64,
        ended_ms: i64,
        reason: &str,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE runs SET ended_ms = $2, exit_reason = $3 WHERE run_id = $1")
            .bind(run_id)
            .bind(ended_ms)
            .bind(clean(reason))
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Live and safe-mode starts of `service` since `since_ms`, not counting anything before the
    /// latest breaker reset of that service.
    pub async fn starts_since(&self, service: &str, since_ms: i64) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM runs \
             WHERE service = $1 AND mode IN ('live', 'safe') \
               AND started_ms >= GREATEST($2, COALESCE( \
                   (SELECT max(started_ms) + 1 FROM runs WHERE service = $1 AND mode = 'breaker_reset'), $2))",
        )
        .bind(service)
        .bind(since_ms)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn record_breaker_reset(&self, service: &str, now_ms: i64) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO runs (service, started_ms, ended_ms, version, config_hash, mode, exit_reason) \
             VALUES ($1, $2, $2, '', '', 'breaker_reset', 'manual reset')",
        )
        .bind(clean(service))
        .bind(now_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// (ended_ms, exit_reason) of one run.
    pub async fn run_exit(&self, run_id: i64) -> Result<(Option<i64>, Option<String>), StoreError> {
        Ok(
            sqlx::query_as("SELECT ended_ms, exit_reason FROM runs WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(self.pool())
                .await?,
        )
    }
}
