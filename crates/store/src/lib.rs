//! store: the ONLY crate that talks to Postgres (spec §9.3: "store owns the schema, and nothing
//! else writes SQL"; deny.toml enforces that no other crate depends on sqlx).
#![forbid(unsafe_code)]

mod batch;
pub mod error;
pub mod rows;
mod runs;
mod text;
mod universe;

pub use batch::{Listener, Table};
pub use error::StoreError;
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
}
