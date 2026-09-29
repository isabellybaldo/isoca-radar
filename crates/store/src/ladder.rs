//! Phase 7a (spec 2026-09-29 §7.1): what `GET /api/paper/ladder` reads, live runs only. Every
//! sum stops at ONE common cut, the latest instant every ladder account has a ledger `mark` row,
//! and within it at each account's own mark row by (run_id, seq). So equity − bankroll =
//! Σ realized_delta + unrealized holds exactly even when a trade lands in the mark's own
//! millisecond (a 0 ms rung) or a resolution marks one account later. The counts use the same
//! cut: an exec counts when its buys come at or before that mark row.
use crate::{Store, StoreError};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LadderSums {
    pub account_id: String,
    /// Executed attempts from the start up to the cut: those whose `buy` rows come at or before
    /// the account's mark row by (run_id, seq); one that bought nothing (no row to order) when
    /// `exec_ms` is before the cut's millisecond.
    pub n_exec: i64,
    /// … with leg risk (`leg_risk_qty > 0`).
    pub n_leg_risk: i64,
    /// … that missed (`q_eff = 0`, SWEET's meaning).
    pub n_missed: i64,
    /// Σ realized_delta of `convert` rows (basket gains, fees included).
    pub gains: Decimal,
    /// Σ realized_delta of `sell` rows (unwinds, fees included).
    pub leg_risk: Decimal,
    /// Σ realized_delta of `settle` rows.
    pub settle: Decimal,
    /// The account's `paper_marks` row at the cut (None: no mark there).
    pub unrealized: Option<Decimal>,
    pub equity: Option<Decimal>,
}

impl Store {
    /// The latest t_ms ≥ `start_ms` at which EVERY one of `accounts` has a live ledger `mark`
    /// row (minute marks, closes and `mark_now` mark every account; a resolution row does not).
    /// None before the first common mark, or with no accounts.
    pub async fn ladder_as_of(
        &self,
        accounts: &[String],
        start_ms: i64,
    ) -> Result<Option<i64>, StoreError> {
        if accounts.is_empty() {
            return Ok(None);
        }
        Ok(sqlx::query_scalar(
            "SELECT l.t_ms FROM paper_ledger l JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
             WHERE l.account_id = ANY($1) AND l.kind = 'mark' AND l.t_ms >= $2 \
             GROUP BY l.t_ms HAVING count(DISTINCT l.account_id) = $3 \
             ORDER BY l.t_ms DESC LIMIT 1",
        )
        .bind(accounts.to_vec())
        .bind(start_ms)
        .bind(i64::try_from(accounts.len()).unwrap_or(i64::MAX))
        .fetch_optional(self.pool())
        .await?)
    }

    /// Per account, sorted by id: from `start_ms` up to the account's own mark row at
    /// `as_of_ms` (the cut), the counts of execs (see [`LadderSums::n_exec`]), Σ realized_delta
    /// per kind, and that mark. An account with no mark row at `as_of_ms` sums and counts nothing.
    pub async fn ladder_sums(
        &self,
        accounts: &[String],
        start_ms: i64,
        as_of_ms: i64,
    ) -> Result<Vec<LadderSums>, StoreError> {
        Ok(sqlx::query_as::<_, LadderSums>(
            "SELECT a.account_id, \
                    COALESCE(ex.n_exec, 0) AS n_exec, COALESCE(ex.n_leg_risk, 0) AS n_leg_risk, \
                    COALESCE(ex.n_missed, 0) AS n_missed, \
                    COALESCE(lg.gains, 0) AS gains, COALESCE(lg.leg_risk, 0) AS leg_risk, \
                    COALESCE(lg.settle, 0) AS settle, mk.unrealized, mk.equity \
             FROM unnest($1::text[]) AS a(account_id) \
             LEFT JOIN LATERAL ( \
                 SELECT l.run_id, l.seq FROM paper_ledger l \
                 JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
                 WHERE l.account_id = a.account_id AND l.kind = 'mark' AND l.t_ms = $3 \
                 ORDER BY l.run_id DESC, l.seq DESC LIMIT 1) cut ON true \
             LEFT JOIN LATERAL ( \
                 SELECT m.unrealized, m.equity FROM paper_marks m \
                 WHERE m.run_id = cut.run_id AND m.account_id = a.account_id AND m.t_ms = $3) mk ON true \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) AS n_exec, \
                        count(*) FILTER (WHERE x.leg_risk_qty > 0) AS n_leg_risk, \
                        count(*) FILTER (WHERE x.q_eff = 0) AS n_missed \
                 FROM paper_execs x JOIN runs r ON r.run_id = x.run_id AND r.mode = 'live' \
                 CROSS JOIN LATERAL ( \
                     SELECT min(b.seq) AS seq FROM paper_ledger b \
                     WHERE b.run_id = x.run_id AND b.account_id = x.account_id \
                       AND b.episode_id = x.episode_id AND b.kind = 'buy') fb \
                 WHERE x.account_id = a.account_id AND cut.run_id IS NOT NULL \
                   AND x.exec_ms >= $2 \
                   AND CASE WHEN fb.seq IS NULL THEN x.exec_ms < $3 \
                            ELSE (x.run_id, fb.seq) <= (cut.run_id, cut.seq) END) ex ON true \
             LEFT JOIN LATERAL ( \
                 SELECT sum(l.realized_delta) FILTER (WHERE l.kind = 'convert') AS gains, \
                        sum(l.realized_delta) FILTER (WHERE l.kind = 'sell') AS leg_risk, \
                        sum(l.realized_delta) FILTER (WHERE l.kind = 'settle') AS settle \
                 FROM paper_ledger l JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
                 WHERE l.account_id = a.account_id AND l.t_ms >= $2 \
                   AND (l.run_id, l.seq) <= (cut.run_id, cut.seq)) lg ON true \
             ORDER BY a.account_id",
        )
        .bind(accounts.to_vec())
        .bind(start_ms)
        .bind(as_of_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// Live scanner runs that overlap [from_ms, to_ms) (Edge capture's measured latency).
    pub async fn live_run_ids_between(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<i64>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT run_id FROM runs WHERE service = 'scanner' AND mode = 'live' \
               AND started_ms < $2 AND (ended_ms IS NULL OR ended_ms >= $1) \
             ORDER BY run_id LIMIT 1000",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// The host version (`1:<label>|<band>`) of the newest live scanner run that recorded one.
    pub async fn newest_live_host(&self) -> Result<Option<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT p.version FROM run_premises p JOIN runs r ON r.run_id = p.run_id \
             WHERE p.area = 'host' AND r.service = 'scanner' AND r.mode = 'live' \
             ORDER BY p.run_id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }
}
