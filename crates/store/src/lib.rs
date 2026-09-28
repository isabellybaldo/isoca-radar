//! store: the ONLY crate that talks to Postgres (spec §9.3: "store owns the schema, and nothing
//! else writes SQL"; deny.toml enforces that no other crate depends on sqlx).
#![forbid(unsafe_code)]

mod batch;
mod card_io;
mod compact_io;
pub mod error;
mod paper;
mod read;
mod read_paper;
pub mod rows;
mod runs;
mod text;
mod universe;

pub use batch::{Listener, Table};
pub use card_io::{
    CARD_TABLES, CardCellRow, CardEpisode, CardResultRow, ChangedSpan, LatencyMinute,
    MAX_CARD_EPISODES, MAX_CHANGED_PAGE, MAX_SWEET_ROWS, MarkPoint, PremiseRun, SweetAccount,
    SweetEpisodeRow, SweetIntentRow, SweetLedgerRow, SweetMidRow,
};
pub use compact_io::{
    COMPACT_TABLES, CompactEpisode, CompactHourRow, CompactStatusRow, MAX_COMPACT_EPISODES,
    MAX_COMPACT_HOURS,
};
pub use error::StoreError;
pub use read::{
    DAY_MS, EpisodeFilter, EpisodePage, EpisodeSummary, HealthNow, IntegritySummary, MAX_OPEN,
    MAX_PAGE, MAX_SERIES_ROWS, Today,
};
pub use read_paper::{EquityPoint, LedgerPage, PaperAccountNow, PaperLedgerEntry, PaperPosition};
pub use rows::*;
pub use sqlx::postgres::PgConnectOptions;

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::str::FromStr;
use std::time::Duration;

/// Migrations embedded at compile time from `crates/store/migrations`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub fn options(url: &str) -> Result<PgConnectOptions, StoreError> {
        Ok(PgConnectOptions::from_str(url)?)
    }

    pub async fn connect(url: &str, max_conns: u32) -> Result<Self, StoreError> {
        Self::connect_with(Self::options(url)?, max_conns).await
    }

    pub async fn connect_with(opts: PgConnectOptions, max_conns: u32) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_conns)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    /// Apply pending migrations. Must run as the owner role; the scanner role cannot create tables.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Admin/test tooling: a pool whose every connection runs `SET ROLE role`, so privilege checks apply
    /// exactly as for that role. The connecting user must be allowed to SET ROLE (the owner/superuser).
    pub async fn connect_with_role(
        opts: PgConnectOptions,
        max_conns: u32,
        role: &str,
    ) -> Result<Self, StoreError> {
        if role.is_empty() || !role.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(StoreError::Db(sqlx::Error::Configuration(
                format!("invalid role name {role:?}").into(),
            )));
        }
        let set_role = format!("SET ROLE {role}");
        let pool = PgPoolOptions::new()
            .max_connections(max_conns)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(move |conn, _meta| {
                // sqlx 0.8.6: `sqlx::raw_sql(&sql).execute(conn)` fails to type-check here
                // ("implementation of `Executor` is not general enough") because the HRTB
                // `for<'c> Fn(&'c mut PgConnection, ...) -> BoxFuture<'c, ...>` on
                // `after_connect` doesn't unify with `raw_sql`'s generic `Executor` bound.
                // Calling the `Executor::execute` inherent method on `conn` directly (as
                // sqlx's own `after_connect` doc example does) avoids the issue and has the
                // same effect (simple-protocol execute, no bind parameters).
                use sqlx::Executor;
                let sql = set_role.clone();
                Box::pin(async move {
                    conn.execute(sql.as_str()).await?;
                    Ok(())
                })
            })
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    /// Execute raw SQL with no parameters (simple protocol). For test harnesses and admin tooling only.
    pub async fn raw_exec(&self, sql: &str) -> Result<(), StoreError> {
        sqlx::raw_sql(sql).execute(&self.pool).await?;
        Ok(())
    }

    /// Names of the tables in the public schema, sorted.
    pub async fn table_names(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY 1",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Admin/test tooling: database names in `pg_database` matching a `LIKE` pattern, sorted.
    pub async fn database_names_like(&self, pattern: &str) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT datname::text FROM pg_database WHERE datname LIKE $1 ORDER BY 1",
        )
        .bind(pattern)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Admin/test tooling: one episode's `updated_ms`.
    pub async fn episode_updated_ms(&self, episode_id: &str) -> Result<Option<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT updated_ms FROM episodes WHERE episode_id = $1")
                .bind(episode_id)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    /// Admin/test tooling: `role`'s settings for THIS database (`ALTER ROLE … IN DATABASE … SET`).
    pub async fn role_db_settings(&self, role: &str) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT unnest(s.setconfig)::text FROM pg_db_role_setting s \
             JOIN pg_roles r ON r.oid = s.setrole JOIN pg_database d ON d.oid = s.setdatabase \
             WHERE r.rolname = $1 AND d.datname = current_database() ORDER BY 1",
        )
        .bind(role)
        .fetch_all(self.pool())
        .await?)
    }
}
