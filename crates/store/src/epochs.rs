//! Phase 7a (spec 2026-09-29-phase7a-latency-answer-design.md §4, §5): paper epochs and reset
//! requests. The scanner reads the epoch status once at start and polls new requests off its
//! engine loop; the api's reset pool (paper_reset_rw) inserts one request per UTC day and reads
//! what is pending. Epoch rows are written by the scanner through `Batch::paper_epochs`.
use crate::{Store, StoreError};
use sqlx::postgres::PgConnectOptions;

/// The only table the reset role may write, and only INSERT (spec 7a §5.1).
pub const RESET_TABLES: [&str; 1] = ["paper_reset_requests"];
/// One epoch per UTC day at most: this is decades.
const MAX_EPOCHS: i64 = 10_000;
const MAX_REQUESTS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperEpoch {
    pub epoch: i32,
    pub start_ms: i64,
    /// 'policies' or 'reset'.
    pub cause: String,
    pub request_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct ResetRequest {
    pub id: i64,
    pub requested_ms: i64,
}

/// What the scanner reads once at start (spec 7a §4.2); the loop then keeps it in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochStatus {
    /// The newest epoch; 0 before the first (epoch 0 is implicit).
    pub current: i32,
    /// The highest request id an epoch applied.
    pub last_applied: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetInsert {
    Queued(ResetRequest),
    /// The unique `utc_day` refused it: one request per UTC day.
    DailyLimit,
}

impl Store {
    /// The api's reset pool: ONE writable connection with its own statement timeout (on top of
    /// the role's 5 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_reset(
        opts: PgConnectOptions,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([("statement_timeout", timeout.as_str())]);
        match role {
            Some(r) => Self::connect_with_role(opts, 1, r).await,
            None => Self::connect_with(opts, 1).await,
        }
    }

    /// Tables this role could write beyond an INSERT into paper_reset_requests: INSERT, UPDATE,
    /// DELETE or TRUNCATE on any other table, or UPDATE, DELETE or TRUNCATE on that one. The api
    /// refuses to start unless this is empty (spec 7a §5.1).
    pub async fn role_writes_outside_reset(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') \
               AND (has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE') \
                 OR (NOT (c.relname::text = ANY($1)) \
                     AND has_table_privilege(current_user, c.oid, 'INSERT'))) \
             ORDER BY 1",
        )
        .bind(RESET_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Every epoch, oldest first.
    pub async fn paper_epochs(&self) -> Result<Vec<PaperEpoch>, StoreError> {
        Ok(sqlx::query_as::<_, PaperEpoch>(
            "SELECT epoch, start_ms, cause, request_id FROM paper_epochs ORDER BY epoch LIMIT $1",
        )
        .bind(MAX_EPOCHS)
        .fetch_all(self.pool())
        .await?)
    }

    /// Every epoch's start, ascending (analyze's Edge capture, spec 7a §6).
    pub async fn paper_epoch_starts(&self) -> Result<Vec<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT start_ms FROM paper_epochs ORDER BY start_ms LIMIT $1")
                .bind(MAX_EPOCHS)
                .fetch_all(self.pool())
                .await?,
        )
    }

    pub async fn epoch_status(&self) -> Result<EpochStatus, StoreError> {
        let (current, last_applied): (i32, Option<i64>) = sqlx::query_as(
            "SELECT COALESCE(max(epoch), 0)::int4, max(request_id) FROM paper_epochs",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(EpochStatus {
            current,
            last_applied,
        })
    }

    /// Requests with id > `after_id`, oldest first (the scanner's poll).
    pub async fn reset_requests_after(
        &self,
        after_id: i64,
    ) -> Result<Vec<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests WHERE id > $1 ORDER BY id LIMIT $2",
        )
        .bind(after_id)
        .bind(MAX_REQUESTS)
        .fetch_all(self.pool())
        .await?)
    }

    /// The newest request no epoch has applied yet (`id > max(paper_epochs.request_id)`).
    pub async fn pending_reset_request(&self) -> Result<Option<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests \
             WHERE id > COALESCE((SELECT max(request_id) FROM paper_epochs), 0) \
             ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }

    /// The newest request, applied or not (`next_allowed_ms`).
    pub async fn latest_reset_request(&self) -> Result<Option<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }

    /// One reset request at `now_ms`; the unique `utc_day` makes a second one the same UTC day
    /// `DailyLimit`, even when two requests race the api's pending check.
    pub async fn insert_reset_request(&self, now_ms: i64) -> Result<ResetInsert, StoreError> {
        let r = sqlx::query_as::<_, ResetRequest>(
            "INSERT INTO paper_reset_requests (requested_ms) VALUES ($1) RETURNING id, requested_ms",
        )
        .bind(now_ms)
        .fetch_one(self.pool())
        .await;
        match r {
            Ok(q) => Ok(ResetInsert::Queued(q)),
            Err(e) => {
                let e = StoreError::from(e);
                if e.is_unique_violation() {
                    Ok(ResetInsert::DailyLimit)
                } else {
                    Err(e)
                }
            }
        }
    }
}
