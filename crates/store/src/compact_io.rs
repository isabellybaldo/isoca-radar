//! Phase 5b: the compactor's progress (compact_hours) and status (compact_status), the episodes
//! that get l2 windows, and the scanner's delete-guard reads. JSON travels as text (`$n::jsonb` in),
//! so `store` needs no JSON dependency. Every read is parameterized and capped; a capped read that
//! overflows is an error, never a truncation.
use crate::text::{clean, clean_opt};
use crate::{Store, StoreError};
use sqlx::postgres::PgConnectOptions;

pub const COMPACT_TABLES: [&str; 2] = ["compact_hours", "compact_status"];
/// Episodes one hour's l2 windows can need: ~20 net-positive arbitrage episodes an hour in practice.
pub const MAX_COMPACT_EPISODES: i64 = 100_000;
/// compact_hours rows one read can return (14 days of hours is 336).
pub const MAX_COMPACT_HOURS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CompactEpisode {
    pub episode_id: String,
    pub event_id: String,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHourRow {
    pub hour_ms: i64,
    /// 'done' or 'lost'.
    pub status: String,
    pub source_file: Option<String>,
    pub source_bytes: Option<i64>,
    pub source_sha256: Option<String>,
    /// Row counts per table as JSON text.
    pub rows: Option<String>,
    pub done_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactStatusRow {
    pub updated_ms: i64,
    pub archive_bytes: i64,
    /// YYYY-MM-DD, the oldest day still in the archive.
    pub oldest_day: Option<String>,
    pub lag_hours: i32,
    pub last_error: Option<String>,
}

impl Store {
    /// The compactor's pool: every session carries its own statement timeout (on top of the role's
    /// 10 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_compact(
        opts: PgConnectOptions,
        max_conns: u32,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([("statement_timeout", timeout.as_str())]);
        match role {
            Some(r) => Self::connect_with_role(opts, max_conns, r).await,
            None => Self::connect_with(opts, max_conns).await,
        }
    }

    /// Tables outside compact_hours/compact_status this role could INSERT, UPDATE, DELETE or
    /// TRUNCATE (compact refuses to start unless this is empty).
    pub async fn role_writes_outside_compact(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT (c.relname::text = ANY($1)) \
               AND (has_table_privilege(current_user, c.oid, 'INSERT') \
                 OR has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE')) \
             ORDER BY 1",
        )
        .bind(COMPACT_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Live net-positive YES/NO-basket episodes opened in [from_ms, to_ms): the ones that get an
    /// l2 window (spec 5b §3). Uses the `episodes_opened` index.
    pub async fn compact_episodes(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<CompactEpisode>, StoreError> {
        let rows: Vec<CompactEpisode> = sqlx::query_as(
            "SELECT episode_id, event_id, opened_server_ms AS opened_ms, closed_server_ms AS closed_ms \
             FROM episodes WHERE source = 'live' AND kind IN ('yes_basket', 'no_basket') \
               AND peak_pnl_net_usd > 0 AND opened_server_ms >= $1 AND opened_server_ms < $2 \
             ORDER BY opened_server_ms, episode_id LIMIT $3",
        )
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_COMPACT_EPISODES + 1)
        .fetch_all(self.pool())
        .await?;
        if rows.len() > usize::try_from(MAX_COMPACT_EPISODES).unwrap_or(usize::MAX) {
            return Err(StoreError::TooManyRows("compact_episodes"));
        }
        Ok(rows)
    }

    /// (hour_ms, status) for hours in [from_ms, to_ms), oldest first.
    pub async fn compact_hour_statuses(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<(i64, String)>, StoreError> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT hour_ms, status FROM compact_hours WHERE hour_ms >= $1 AND hour_ms < $2 \
             ORDER BY hour_ms LIMIT $3",
        )
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_COMPACT_HOURS + 1)
        .fetch_all(self.pool())
        .await?;
        if rows.len() > usize::try_from(MAX_COMPACT_HOURS).unwrap_or(usize::MAX) {
            return Err(StoreError::TooManyRows("compact_hour_statuses"));
        }
        Ok(rows)
    }

    /// The compactor records an hour. Its word is final: a 'lost' row the scanner inserted while
    /// the compactor was still reading the (already open) file becomes 'done'.
    pub async fn compact_hour_put(&self, r: &CompactHourRow) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO compact_hours (hour_ms, status, source_file, source_bytes, source_sha256, rows, done_ms) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7) \
             ON CONFLICT (hour_ms) DO UPDATE SET status = EXCLUDED.status, source_file = EXCLUDED.source_file, \
               source_bytes = EXCLUDED.source_bytes, source_sha256 = EXCLUDED.source_sha256, \
               rows = EXCLUDED.rows, done_ms = EXCLUDED.done_ms",
        )
        .bind(r.hour_ms)
        .bind(clean(&r.status))
        .bind(clean_opt(&r.source_file))
        .bind(r.source_bytes)
        .bind(clean_opt(&r.source_sha256))
        .bind(r.rows.as_deref())
        .bind(r.done_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// The scanner deleted these raw hours: mark the ones nobody recorded as 'lost'. A 'done' hour
    /// is never downgraded (spec 5b §4.3). Returns the rows inserted.
    pub async fn mark_hours_lost(&self, hours: &[i64], now_ms: i64) -> Result<u64, StoreError> {
        Ok(sqlx::query(
            "INSERT INTO compact_hours (hour_ms, status, done_ms) \
             SELECT h, 'lost', $2 FROM unnest($1::bigint[]) AS h \
             ON CONFLICT (hour_ms) DO NOTHING",
        )
        .bind(hours)
        .bind(now_ms)
        .execute(self.pool())
        .await?
        .rows_affected())
    }

    pub async fn compact_status_put(&self, s: &CompactStatusRow) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO compact_status (id, updated_ms, archive_bytes, oldest_day, lag_hours, last_error) \
             VALUES (1, $1, $2, $3::date, $4, $5) \
             ON CONFLICT (id) DO UPDATE SET updated_ms = EXCLUDED.updated_ms, \
               archive_bytes = EXCLUDED.archive_bytes, oldest_day = EXCLUDED.oldest_day, \
               lag_hours = EXCLUDED.lag_hours, last_error = EXCLUDED.last_error",
        )
        .bind(s.updated_ms)
        .bind(s.archive_bytes)
        .bind(s.oldest_day.as_deref())
        .bind(s.lag_hours)
        .bind(clean_opt(&s.last_error))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn compact_status(&self) -> Result<Option<CompactStatusRow>, StoreError> {
        // clippy::type_complexity: named so the query_as annotation stays readable.
        type Row = (i64, i64, Option<String>, i32, Option<String>);
        let row: Option<Row> = sqlx::query_as(
            "SELECT updated_ms, archive_bytes, oldest_day::text, lag_hours, last_error \
             FROM compact_status WHERE id = 1",
        )
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(
            |(updated_ms, archive_bytes, oldest_day, lag_hours, last_error)| CompactStatusRow {
                updated_ms,
                archive_bytes,
                oldest_day,
                lag_hours,
                last_error,
            },
        ))
    }

    /// When the first hour was recorded (done or lost), if any.
    pub async fn compact_first_done_ms(&self) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query_scalar("SELECT min(done_ms) FROM compact_hours")
            .fetch_one(self.pool())
            .await?)
    }
}
