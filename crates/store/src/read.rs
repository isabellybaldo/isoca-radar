//! Read-only queries for the dashboard api (Phase 4 spec §4). Every query is parameterized and has
//! a hard LIMIT here, on top of the api's own validation (page ≤ 200, ranges ≤ 7 days). Dashboard
//! reads are live-only: replay runs and `source = 'replay'` episodes never show.
use crate::batch::EPISODE_COLS;
use crate::{EpisodeRow, HealthRow, Store, StoreError};
use rust_decimal::Decimal;
use sqlx::postgres::PgConnectOptions;

pub const MAX_PAGE: i64 = 200;
pub const MAX_OPEN: i64 = 200;
/// Seven days of minutes.
pub const MAX_SERIES_ROWS: i64 = 7 * 24 * 60;
pub const DAY_MS: i64 = 86_400_000;

const HEALTH_COLS: &str = "run_id, minute_ms, tokens, shards, msgs, bytes, drops, reconnects, rest_p50_ms, \
    rest_p99_ms, lag_p50_ms, lag_p99_ms, clock_offset_ms, ok, capture_paused, parked_shards, dropped_groups";

/// The newest live run that has written at least one health row (uses the health primary key).
const LATEST_HEALTH_RUN: &str = "(SELECT r.run_id FROM runs r WHERE r.mode = 'live' \
    AND EXISTS (SELECT 1 FROM health h WHERE h.run_id = r.run_id) ORDER BY r.run_id DESC LIMIT 1)";

const SUMMARY_COLS: &str = "e.episode_id, e.kind, e.realization, e.event_title, e.event_slug, e.n_legs, \
    (SELECT count(*) FROM episode_legs l WHERE l.episode_id = e.episode_id AND l.in_s) AS s_legs, \
    e.opened_server_ms, e.closed_server_ms, e.duration_ms, e.peak_edge_top, e.peak_max_size, \
    e.peak_pnl_net_usd, e.peak_pnl_gross_usd, e.engine_suspect, e.stale_transport, \
    (e.censor_left OR e.censor_right OR e.censor_gap) AS censored";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthNow {
    pub row: HealthRow,
    /// Sum of `drops` over the same run's minutes in [now − 5 min, now].
    pub drops_5m: i64,
}

/// One episode as a table row (Radar, Episodes). `s_legs` = legs in S at peak.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EpisodeSummary {
    pub episode_id: String,
    pub kind: String,
    pub realization: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub n_legs: i32,
    pub s_legs: i64,
    pub opened_server_ms: i64,
    pub closed_server_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub peak_edge_top: Decimal,
    pub peak_max_size: Decimal,
    pub peak_pnl_net_usd: Decimal,
    pub peak_pnl_gross_usd: Decimal,
    pub engine_suspect: bool,
    pub stale_transport: bool,
    pub censored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeFilter {
    /// yes_basket | yes_basket_incomplete | no_basket | implication (None = all kinds)
    pub kind: Option<String>,
    /// opened_server_ms in [from_ms, to_ms)
    pub from_ms: i64,
    pub to_ms: i64,
    pub net_positive: bool,
    pub suspect: bool,
    pub stale: bool,
    pub censored: bool,
    /// Keyset cursor: only rows strictly older than (opened_server_ms, episode_id).
    pub before: Option<(i64, String)>,
    /// Validated by the caller to 1..=MAX_PAGE; clamped here as a last guard.
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodePage {
    pub rows: Vec<EpisodeSummary>,
    /// Cursor of the next (older) page; None when this page is the last one.
    pub next: Option<(i64, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Today {
    pub day_start_ms: i64,
    pub gross: i64,
    pub net: i64,
    /// Exactly 24 entries, hour 0..=23 UTC: (gross-positive, net-positive) episodes opened then.
    pub hourly: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegritySummary {
    /// (kind, count), most frequent first.
    pub by_kind: Vec<(String, i64)>,
    pub total: i64,
    /// Messages counted in `health` over the same window.
    pub msgs: i64,
    /// total × 1,000,000 / msgs, rounded to 2 dp; None when no messages were counted.
    pub per_million: Option<Decimal>,
}

impl Store {
    /// The api's pool: every session is read-only (`default_transaction_read_only`) with its own
    /// statement timeout, on top of the role's grants. `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_read_only(
        opts: PgConnectOptions,
        max_conns: u32,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([
            ("default_transaction_read_only", "on"),
            ("statement_timeout", timeout.as_str()),
        ]);
        match role {
            Some(r) => Self::connect_with_role(opts, max_conns, r).await,
            None => Self::connect_with(opts, max_conns).await,
        }
    }

    /// True when this connection's role could INSERT into the scanner's tables (the api refuses
    /// to start then: spec §3 "The API can't write").
    pub async fn role_can_write(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT has_table_privilege(current_user, 'episodes', 'INSERT') \
                 OR has_table_privilege(current_user, 'health', 'INSERT')",
        )
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn health_now(&self, now_ms: i64) -> Result<Option<HealthNow>, StoreError> {
        let sql = format!(
            "SELECT {HEALTH_COLS} FROM health WHERE run_id = {LATEST_HEALTH_RUN} \
             ORDER BY minute_ms DESC LIMIT 1"
        );
        let Some(row) = sqlx::query_as::<_, HealthRow>(&sql)
            .fetch_optional(self.pool())
            .await?
        else {
            return Ok(None);
        };
        let drops_5m: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(drops), 0)::int8 FROM health WHERE run_id = $1 AND minute_ms >= $2",
        )
        .bind(row.run_id)
        .bind(now_ms.saturating_sub(5 * 60_000))
        .fetch_one(self.pool())
        .await?;
        Ok(Some(HealthNow { row, drops_5m }))
    }

    /// Live health minutes in [from_ms, to_ms), oldest first.
    pub async fn health_series(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<HealthRow>, StoreError> {
        let sql = format!(
            "SELECT {HEALTH_COLS} FROM health \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND minute_ms >= $1 AND minute_ms < $2 \
             ORDER BY minute_ms, run_id LIMIT {MAX_SERIES_ROWS}"
        );
        Ok(sqlx::query_as::<_, HealthRow>(&sql)
            .bind(from_ms)
            .bind(to_ms)
            .fetch_all(self.pool())
            .await?)
    }

    /// The newest health minute of the newest live run (the SSE bridge polls this).
    pub async fn latest_health_minute(&self) -> Result<Option<i64>, StoreError> {
        let sql = format!("SELECT max(minute_ms) FROM health WHERE run_id = {LATEST_HEALTH_RUN}");
        Ok(sqlx::query_scalar::<_, Option<i64>>(&sql)
            .fetch_one(self.pool())
            .await?)
    }

    /// Admin/test tooling: backends in this database whose last statement was a LISTEN.
    pub async fn listen_backends(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// Admin/test tooling: terminate every LISTEN backend in this database (proves the bridge's
    /// reconnect path against a real dropped connection). Returns how many `pg_terminate_backend`
    /// calls actually returned `true` (a backend can vanish between being matched and being
    /// signaled, in which case it returns `false` and must not be counted), so a test can assert
    /// it actually killed something instead of a no-op query.
    pub async fn terminate_listen_backends(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FILTER (WHERE pg_terminate_backend(pid)) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// Unclosed live episodes of the newest live scanner run only: a crashed run's never-closed
    /// rows are history, not opportunities.
    pub async fn open_episodes(&self) -> Result<Vec<EpisodeSummary>, StoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM episodes e \
             WHERE e.source = 'live' AND e.closed_server_ms IS NULL \
               AND e.run_id = (SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live') \
             ORDER BY e.opened_server_ms DESC, e.episode_id DESC LIMIT {MAX_OPEN}"
        );
        Ok(sqlx::query_as::<_, EpisodeSummary>(&sql)
            .fetch_all(self.pool())
            .await?)
    }

    /// Newest first, keyset-paged on (opened_server_ms, episode_id).
    pub async fn episodes_page(&self, f: &EpisodeFilter) -> Result<EpisodePage, StoreError> {
        let limit = f.limit.clamp(1, MAX_PAGE);
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM episodes e WHERE e.source = 'live' \
               AND e.opened_server_ms >= $1 AND e.opened_server_ms < $2 \
               AND ($3::text IS NULL OR e.kind = $3) \
               AND (NOT $4 OR e.peak_pnl_net_usd > 0) \
               AND (NOT $5 OR e.engine_suspect) \
               AND (NOT $6 OR e.stale_transport) \
               AND (NOT $7 OR e.censor_left OR e.censor_right OR e.censor_gap) \
               AND ($8::int8 IS NULL OR (e.opened_server_ms, e.episode_id) < ($8::int8, $9::text)) \
             ORDER BY e.opened_server_ms DESC, e.episode_id DESC LIMIT $10"
        );
        let (b_ms, b_id) = match &f.before {
            Some((ms, id)) => (Some(*ms), Some(id.clone())),
            None => (None, None),
        };
        let mut rows = sqlx::query_as::<_, EpisodeSummary>(&sql)
            .bind(f.from_ms)
            .bind(f.to_ms)
            .bind(f.kind.clone())
            .bind(f.net_positive)
            .bind(f.suspect)
            .bind(f.stale)
            .bind(f.censored)
            .bind(b_ms)
            .bind(b_id)
            .bind(limit + 1)
            .fetch_all(self.pool())
            .await?;
        let keep = usize::try_from(limit).unwrap_or(usize::MAX);
        let next = if rows.len() > keep {
            rows.truncate(keep);
            rows.last()
                .map(|r| (r.opened_server_ms, r.episode_id.clone()))
        } else {
            None
        };
        Ok(EpisodePage { rows, next })
    }

    /// One episode (any source) by id.
    pub async fn episode(&self, episode_id: &str) -> Result<Option<EpisodeRow>, StoreError> {
        let sql = format!(
            "SELECT {} FROM episodes WHERE episode_id = $1",
            EPISODE_COLS.join(", ")
        );
        Ok(sqlx::query_as::<_, EpisodeRow>(&sql)
            .bind(episode_id)
            .fetch_optional(self.pool())
            .await?)
    }

    /// Today (UTC) in live episodes that could be arbitrage (realization ≠ not_arbitrage):
    /// gross = peak gross Π > 0, net = peak net Π > 0.
    pub async fn today(&self, now_ms: i64) -> Result<Today, StoreError> {
        let day_start_ms = now_ms - now_ms.rem_euclid(DAY_MS);
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT ((opened_server_ms - $1) / 3600000)::int8 AS hour, \
                    count(*) FILTER (WHERE peak_pnl_gross_usd > 0) AS gross, \
                    count(*) FILTER (WHERE peak_pnl_net_usd > 0) AS net \
             FROM episodes \
             WHERE source = 'live' AND realization <> 'not_arbitrage' \
               AND opened_server_ms >= $1 AND opened_server_ms < $2 \
             GROUP BY 1 ORDER BY 1",
        )
        .bind(day_start_ms)
        .bind(day_start_ms + DAY_MS)
        .fetch_all(self.pool())
        .await?;
        let mut hourly = vec![(0i64, 0i64); 24];
        for (hour, gross, net) in rows {
            if let Some(slot) = usize::try_from(hour).ok().and_then(|i| hourly.get_mut(i)) {
                *slot = (gross, net);
            }
        }
        let gross = hourly.iter().map(|h| h.0).sum();
        let net = hourly.iter().map(|h| h.1).sum();
        Ok(Today {
            day_start_ms,
            gross,
            net,
            hourly,
        })
    }

    /// Live integrity events in [from_ms, to_ms) by kind, plus the per-million-messages rate.
    pub async fn integrity_summary(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<IntegritySummary, StoreError> {
        let by_kind: Vec<(String, i64)> = sqlx::query_as(
            "SELECT kind, count(*) FROM integrity_events \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live' \
                              AND started_ms < $2 AND (ended_ms IS NULL OR ended_ms >= $1)) \
               AND ts_ms >= $1 AND ts_ms < $2 \
             GROUP BY kind ORDER BY 2 DESC, 1 LIMIT 100",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?;
        let msgs: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(msgs), 0)::int8 FROM health \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND minute_ms >= $1 AND minute_ms < $2",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_one(self.pool())
        .await?;
        let total: i64 = by_kind.iter().map(|k| k.1).sum();
        let per_million = if msgs > 0 {
            Decimal::from(total)
                .checked_mul(Decimal::from(1_000_000))
                .and_then(|x| x.checked_div(Decimal::from(msgs)))
                .map(|x| x.round_dp(2))
        } else {
            None
        };
        Ok(IntegritySummary {
            by_kind,
            total,
            msgs,
            per_million,
        })
    }
}
