//! Phase 5a: what `analyze` reads (episodes, runs, run_premises, health, paper marks) and writes
//! (card_cells, card_results, card_meta), and the api's two card reads. JSON columns travel as
//! text (`$n::jsonb` in, `col::text` out), so `store` needs no JSON dependency. Every read is
//! parameterized and capped; a capped read that overflows is an error, never a truncation.
use crate::text::clean;
use crate::{Store, StoreError};
use rust_decimal::Decimal;
use sqlx::postgres::PgConnectOptions;
use sqlx::{Postgres, QueryBuilder};

pub const MAX_CARD_EPISODES: i64 = 200_000;
pub const MAX_CHANGED_PAGE: i64 = 5_000;
pub const CARD_TABLES: [&str; 3] = ["card_cells", "card_results", "card_meta"];
const MAX_ROWS: i64 = 200_000;
const CELL_CHUNK: usize = 1_000;
const MAX_PREMISE_RUNS: i64 = 100_000;
const MAX_RUN_PREMISE_ROWS: i64 = 1_000_000;
const MAX_P0_ACCOUNTS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PremiseRun {
    pub run_id: i64,
    pub started_ms: i64,
    /// `ended_ms`, else the start of the next scanner session (a crashed run), else None (running).
    pub end_ms: Option<i64>,
    /// (area, version), sorted by area.
    pub premises: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ChangedSpan {
    pub episode_id: String,
    pub run_id: i64,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub updated_ms: i64,
}

/// An episode as the cards need it. `prev_gap_ms` = this open minus the latest close of another
/// live episode with the same event and kind that closed within the second before (merge-gap
/// sensitivity, parent spec §5.5).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CardEpisode {
    pub episode_id: String,
    pub run_id: i64,
    pub kind: String,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    pub edge_net: Decimal,
    pub edge_gross: Decimal,
    pub pnl_net: Decimal,
    pub pnl_gross: Decimal,
    pub cost: Decimal,
    pub budget100: Option<Decimal>,
    pub suspect: bool,
    pub stale: bool,
    pub prev_gap_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LatencyMinute {
    pub run_id: i64,
    pub minute_ms: i64,
    pub lag_p50_ms: Option<i32>,
    pub rest_rtt_p50_ms: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct MarkPoint {
    pub run_id: i64,
    pub account_id: String,
    pub t_ms: i64,
    pub equity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCellRow {
    pub card: String,
    pub premise_key: String,
    pub hour_ms: i64,
    pub data: String,
    pub computed_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardResultRow {
    pub card: String,
    pub win: String,
    pub premise_key: String,
    pub status: String,
    pub body: String,
    pub computed_ms: i64,
}

fn capped<T>(mut rows: Vec<T>, cap: i64, what: &'static str) -> Result<Vec<T>, StoreError> {
    if rows.len() > usize::try_from(cap).unwrap_or(usize::MAX) {
        rows.clear();
        return Err(StoreError::TooManyRows(what));
    }
    Ok(rows)
}

impl Store {
    /// analyze's pool: every session carries its own statement timeout (on top of the role's
    /// 10 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_analyze(
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

    /// Tables outside the card tables this role could INSERT, UPDATE, DELETE or TRUNCATE (analyze
    /// refuses to start unless this is empty: spec §2).
    pub async fn role_writes_outside_cards(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT (c.relname::text = ANY($1)) \
               AND (has_table_privilege(current_user, c.oid, 'INSERT') \
                 OR has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE')) \
             ORDER BY 1",
        )
        .bind(CARD_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Live scanner runs that recorded premises, oldest first (runs without them never count).
    pub async fn premise_runs(&self) -> Result<Vec<PremiseRun>, StoreError> {
        self.premise_runs_capped(MAX_PREMISE_RUNS, MAX_RUN_PREMISE_ROWS)
            .await
    }

    /// Admin/test tooling: `premise_runs` with its two row caps as parameters, so a test can prove
    /// the overflow-is-an-error contract without seeding hundreds of thousands of rows.
    pub async fn premise_runs_capped(
        &self,
        run_cap: i64,
        premise_cap: i64,
    ) -> Result<Vec<PremiseRun>, StoreError> {
        let runs: Vec<(i64, i64, Option<i64>)> = capped(
            sqlx::query_as(
                "SELECT r.run_id, r.started_ms, \
                        COALESCE(r.ended_ms, (SELECT min(n.started_ms) FROM runs n \
                            WHERE n.service = 'scanner' AND n.mode IN ('live', 'safe') AND n.run_id > r.run_id)) \
                 FROM runs r \
                 WHERE r.service = 'scanner' AND r.mode = 'live' \
                   AND EXISTS (SELECT 1 FROM run_premises p WHERE p.run_id = r.run_id) \
                 ORDER BY r.run_id LIMIT $1",
            )
            .bind(run_cap + 1)
            .fetch_all(self.pool())
            .await?,
            run_cap,
            "premise_runs",
        )?;
        let rows: Vec<(i64, String, String)> = capped(
            sqlx::query_as(
                "SELECT run_id, area, version FROM run_premises ORDER BY run_id, area LIMIT $1",
            )
            .bind(premise_cap + 1)
            .fetch_all(self.pool())
            .await?,
            premise_cap,
            "premise_runs",
        )?;
        Ok(runs
            .into_iter()
            .map(|(run_id, started_ms, end_ms)| PremiseRun {
                run_id,
                started_ms,
                end_ms,
                premises: rows
                    .iter()
                    .filter(|r| r.0 == run_id)
                    .map(|r| (r.1.clone(), r.2.clone()))
                    .collect(),
            })
            .collect())
    }

    /// The database clock's high-water mark, live episodes only (F5: replay rows never advance
    /// analyze's cursor — a replay episode's `updated_ms` is stamped by the same clock, so leaving
    /// out the `source` filter here would let a replay write move the cursor forward).
    pub async fn max_episode_updated_ms(&self) -> Result<Option<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT max(updated_ms) FROM episodes WHERE source = 'live'")
                .fetch_one(self.pool())
                .await?,
        )
    }

    /// Live episodes changed after `(after_ms, after_id)`, keyset-paged on (updated_ms, episode_id).
    pub async fn changed_spans(
        &self,
        after_ms: i64,
        after_id: &str,
        limit: i64,
    ) -> Result<Vec<ChangedSpan>, StoreError> {
        Ok(sqlx::query_as::<_, ChangedSpan>(
            "SELECT episode_id, run_id, opened_server_ms AS opened_ms, closed_server_ms AS closed_ms, updated_ms \
             FROM episodes WHERE source = 'live' AND updated_ms >= $1 AND (updated_ms, episode_id) > ($1, $2) \
             ORDER BY updated_ms, episode_id LIMIT $3",
        )
        .bind(after_ms)
        .bind(after_id)
        .bind(limit.clamp(1, MAX_CHANGED_PAGE))
        .fetch_all(self.pool())
        .await?)
    }

    /// YES/NO-basket live episodes of `run_ids` that overlap [from_ms, to_ms): opened before
    /// `to_ms` and still open or closed at or after `from_ms`.
    pub async fn card_episodes(
        &self,
        run_ids: &[i64],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<CardEpisode>, StoreError> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, CardEpisode>(
            "SELECT e.episode_id, e.run_id, e.kind, e.opened_server_ms AS opened_ms, \
                    e.closed_server_ms AS closed_ms, e.duration_ms, e.censor_left, e.censor_right, e.censor_gap, \
                    e.peak_edge_top AS edge_net, e.peak_edge_top_gross AS edge_gross, \
                    e.peak_pnl_net_usd AS pnl_net, e.peak_pnl_gross_usd AS pnl_gross, e.peak_cost_usd AS cost, \
                    e.pnl_net_budget100_usd AS budget100, e.engine_suspect AS suspect, e.stale_transport AS stale, \
                    (SELECT e.opened_server_ms - max(p.closed_server_ms) FROM episodes p \
                      WHERE p.event_id = e.event_id AND p.kind = e.kind AND p.source = 'live' \
                        AND p.episode_id <> e.episode_id \
                        AND p.closed_server_ms BETWEEN e.opened_server_ms - 1000 AND e.opened_server_ms) AS prev_gap_ms \
             FROM episodes e \
             WHERE e.source = 'live' AND e.run_id = ANY($1) AND e.kind IN ('yes_basket', 'no_basket') \
               AND e.opened_server_ms < $3 AND (e.closed_server_ms IS NULL OR e.closed_server_ms >= $2) \
             ORDER BY e.opened_server_ms, e.episode_id LIMIT $4",
        )
        .bind(run_ids.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_CARD_EPISODES + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_CARD_EPISODES, "card_episodes")
    }

    pub async fn health_latency(
        &self,
        run_ids: &[i64],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<LatencyMinute>, StoreError> {
        let rows = sqlx::query_as::<_, LatencyMinute>(
            "SELECT run_id, minute_ms, lag_p50_ms, rest_rtt_p50_ms FROM health \
             WHERE run_id = ANY($1) AND minute_ms >= $2 AND minute_ms < $3 \
             ORDER BY minute_ms, run_id LIMIT $4",
        )
        .bind(run_ids.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_ROWS, "health_latency")
    }

    /// P0-converter accounts at the default fee: (account_id, scenario), one per scenario.
    pub async fn p0_accounts(&self) -> Result<Vec<(String, String)>, StoreError> {
        self.p0_accounts_capped(MAX_P0_ACCOUNTS).await
    }

    /// Admin/test tooling: `p0_accounts` with its row cap as a parameter, so a test can prove the
    /// overflow-is-an-error contract without seeding thousands of accounts.
    pub async fn p0_accounts_capped(&self, cap: i64) -> Result<Vec<(String, String)>, StoreError> {
        let rows = sqlx::query_as(
            "SELECT DISTINCT account_id, scenario FROM paper_accounts \
             WHERE policy = 'P0-converter' AND fee_bips = 0 ORDER BY 1 LIMIT $1",
        )
        .bind(cap + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, cap, "p0_accounts")
    }

    /// For each account: its last mark before `from_ms` (any live run), then its marks in
    /// [from_ms, to_ms). Live runs only: `scanner replay` writes marks under the same account ids
    /// and t_ms, which would otherwise become a live mark's `prev`. Sorted by (account_id, t_ms,
    /// run_id).
    pub async fn marks_with_prev(
        &self,
        accounts: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<MarkPoint>, StoreError> {
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = sqlx::query_as::<_, MarkPoint>(
            "SELECT m.run_id, m.account_id, m.t_ms, m.equity FROM unnest($1::text[]) AS a(account_id) \
             CROSS JOIN LATERAL (SELECT p.run_id, p.account_id, p.t_ms, p.equity FROM paper_marks p \
                 JOIN runs r ON r.run_id = p.run_id AND r.mode = 'live' \
                 WHERE p.account_id = a.account_id AND p.t_ms < $2 \
                 ORDER BY p.t_ms DESC, p.run_id DESC LIMIT 1) m",
        )
        .bind(accounts.to_vec())
        .bind(from_ms)
        .fetch_all(self.pool())
        .await?;
        let inside = sqlx::query_as::<_, MarkPoint>(
            "SELECT p.run_id, p.account_id, p.t_ms, p.equity FROM paper_marks p \
             JOIN runs r ON r.run_id = p.run_id AND r.mode = 'live' \
             WHERE p.account_id = ANY($1) AND p.t_ms >= $2 AND p.t_ms < $3 LIMIT $4",
        )
        .bind(accounts.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        rows.extend(capped(inside, MAX_ROWS, "marks_with_prev")?);
        rows.sort_by(|a, b| {
            (&a.account_id, a.t_ms, a.run_id).cmp(&(&b.account_id, b.t_ms, b.run_id))
        });
        Ok(rows)
    }

    /// Stored cells of one family and key with hour_ms in [from_hour, to_hour), oldest first.
    pub async fn card_cells_range(
        &self,
        card: &str,
        premise_key: &str,
        from_hour: i64,
        to_hour: i64,
    ) -> Result<Vec<(i64, String)>, StoreError> {
        let rows = sqlx::query_as(
            "SELECT hour_ms, data::text FROM card_cells \
             WHERE card = $1 AND premise_key = $2 AND hour_ms >= $3 AND hour_ms < $4 \
             ORDER BY hour_ms LIMIT $5",
        )
        .bind(card)
        .bind(premise_key)
        .bind(from_hour)
        .bind(to_hour)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_ROWS, "card_cells_range")
    }

    pub async fn card_cells_at(
        &self,
        card: &str,
        premise_key: &str,
        hours: &[i64],
    ) -> Result<Vec<(i64, String)>, StoreError> {
        if hours.is_empty() {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(
            "SELECT hour_ms, data::text FROM card_cells \
             WHERE card = $1 AND premise_key = $2 AND hour_ms = ANY($3) ORDER BY hour_ms",
        )
        .bind(card)
        .bind(premise_key)
        .bind(hours.to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Upsert cells in one transaction.
    pub async fn card_cells_put(&self, rows: &[CardCellRow]) -> Result<(), StoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await?;
        for chunk in rows.chunks(CELL_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) ",
            );
            qb.push_values(chunk, |mut s, r| {
                s.push_bind(clean(&r.card))
                    .push_bind(clean(&r.premise_key))
                    .push_bind(r.hour_ms)
                    .push_bind(r.data.clone())
                    .push_unseparated("::jsonb")
                    .push_bind(r.computed_ms);
            });
            qb.push(
                " ON CONFLICT (card, premise_key, hour_ms) DO UPDATE SET data = EXCLUDED.data, \
                 computed_ms = EXCLUDED.computed_ms",
            );
            qb.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Upsert results and meta rows in one transaction.
    pub async fn card_results_put(
        &self,
        rows: &[CardResultRow],
        meta: &[(String, String)],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await?;
        for chunk in rows.chunks(CELL_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) ",
            );
            qb.push_values(chunk, |mut s, r| {
                s.push_bind(clean(&r.card))
                    .push_bind(clean(&r.win))
                    .push_bind(clean(&r.premise_key))
                    .push_bind(clean(&r.status))
                    .push_bind(r.body.clone())
                    .push_unseparated("::jsonb")
                    .push_bind(r.computed_ms);
            });
            qb.push(
                " ON CONFLICT (card, win) DO UPDATE SET premise_key = EXCLUDED.premise_key, \
                 status = EXCLUDED.status, body = EXCLUDED.body, computed_ms = EXCLUDED.computed_ms",
            );
            qb.build().execute(&mut *tx).await?;
        }
        for (key, value) in meta {
            sqlx::query(
                "INSERT INTO card_meta (key, value) VALUES ($1, $2::jsonb) \
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            )
            .bind(clean(key))
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn card_meta_put(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.card_results_put(&[], &[(key.to_string(), value.to_string())])
            .await
    }

    pub async fn card_meta_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT value::text FROM card_meta WHERE key = $1")
                .bind(key)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    async fn cards_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT to_regclass('public.card_results') IS NOT NULL \
                AND to_regclass('public.card_meta') IS NOT NULL",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// The api: (card, body) of one window, by card. Empty before migration 0006.
    pub async fn card_results_for(&self, win: &str) -> Result<Vec<(String, String)>, StoreError> {
        if !self.cards_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(
            "SELECT card, body::text FROM card_results WHERE win = $1 ORDER BY card LIMIT 50",
        )
        .bind(win)
        .fetch_all(self.pool())
        .await?)
    }

    /// The api: every (key, value) of card_meta. Empty before migration 0006.
    pub async fn card_meta_all(&self) -> Result<Vec<(String, String)>, StoreError> {
        if !self.cards_ready().await? {
            return Ok(Vec::new());
        }
        Ok(
            sqlx::query_as("SELECT key, value::text FROM card_meta ORDER BY key LIMIT 100")
                .fetch_all(self.pool())
                .await?,
        )
    }
}
