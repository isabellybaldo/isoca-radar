//! Paper-trading reads for the dashboard (Phase 4 spec §4), over S3c's migration 0004 tables.
//! Until 0004 is applied every reader returns an empty result, never an error. Live runs only:
//! replay runs write their own paper rows and never show here.
use crate::{Store, StoreError};
use rust_decimal::Decimal;

const LEDGER_MAX: i64 = 200;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperAccountNow {
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Decimal,
    pub bankroll: Decimal,
    pub run_id: i64,
    /// The latest mark; all None until the account's first mark in this run.
    pub t_ms: Option<i64>,
    pub cash: Option<Decimal>,
    pub equity: Option<Decimal>,
    pub realized: Option<Decimal>,
    pub unrealized: Option<Decimal>,
    pub fees_paid: Option<Decimal>,
    pub locked: Option<Decimal>,
    pub max_drawdown: Option<Decimal>,
    pub n_positions: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EquityPoint {
    pub t_ms: i64,
    pub equity: Decimal,
}

/// A holding awaiting resolution, from the live `paper_state` save. `returned` = YES returned by a
/// NO→collateral conversion (basis 0, spec §5.6).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperPosition {
    pub account_id: String,
    pub market_id: String,
    pub outcome: String,
    pub qty: Decimal,
    pub basis: Decimal,
    pub returned: bool,
    pub event_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperLedgerEntry {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub outcome: Option<String>,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub cash_delta: Decimal,
    pub fees: Decimal,
    pub realized_delta: Decimal,
    pub cash_after: Decimal,
    pub equity: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerPage {
    pub rows: Vec<PaperLedgerEntry>,
    /// Cursor (run_id, seq) of the next (older) page; None on the last page.
    pub next: Option<(i64, i64)>,
}

impl Store {
    /// True once S3c's migration 0004 is applied (every paper table the dashboard reads exists).
    pub async fn paper_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT to_regclass('public.paper_accounts') IS NOT NULL \
                AND to_regclass('public.paper_marks') IS NOT NULL \
                AND to_regclass('public.paper_ledger') IS NOT NULL \
                AND to_regclass('public.paper_state') IS NOT NULL",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// Every account of the newest live run that has paper accounts, with its latest mark.
    pub async fn paper_accounts_now(&self) -> Result<Vec<PaperAccountNow>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, PaperAccountNow>(
            "SELECT a.account_id, a.policy, a.scenario, a.fee_bips, a.bankroll, a.run_id, \
                    m.t_ms, m.cash, m.equity, m.realized, m.unrealized, m.fees_paid, m.locked, \
                    m.max_drawdown, m.n_positions \
             FROM paper_accounts a \
             LEFT JOIN LATERAL ( \
                 SELECT pm.t_ms, pm.cash, pm.equity, pm.realized, pm.unrealized, pm.fees_paid, \
                        pm.locked, pm.max_drawdown, pm.n_positions \
                 FROM paper_marks pm \
                 WHERE pm.run_id = a.run_id AND pm.account_id = a.account_id \
                 ORDER BY pm.t_ms DESC LIMIT 1) m ON true \
             WHERE a.run_id = (SELECT max(a2.run_id) FROM paper_accounts a2 \
                               JOIN runs r2 ON r2.run_id = a2.run_id WHERE r2.mode = 'live') \
             ORDER BY a.account_id LIMIT 100",
        )
        .fetch_all(self.pool())
        .await?)
    }

    /// Minute marks of one account across live runs (restarts restore the account), oldest first.
    pub async fn paper_equity(
        &self,
        account_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<EquityPoint>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, EquityPoint>(
            "SELECT m.t_ms, m.equity FROM paper_marks m \
             WHERE m.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND m.account_id = $1 AND m.t_ms >= $2 AND m.t_ms < $3 \
             ORDER BY m.t_ms, m.run_id LIMIT 20160",
        )
        .bind(account_id)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// Holdings in the live `paper_state` saves (sim::Account JSON: `positions` and `returned` are
    /// arrays of [LegKey, value] pairs). A state that isn't a JSON object, or has no such arrays,
    /// contributes nothing.
    pub async fn paper_positions(&self) -> Result<Vec<PaperPosition>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, PaperPosition>(
            "WITH s AS ( \
                 SELECT account_id, state::jsonb AS j FROM paper_state WHERE state IS JSON OBJECT \
             ), held AS ( \
                 SELECT s.account_id, p.value->0->>'market_id' AS market_id, \
                        p.value->0->>'outcome' AS outcome, \
                        (p.value->1->>'qty')::numeric AS qty, \
                        (p.value->1->>'basis')::numeric AS basis, false AS returned \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'positions') = 'array' THEN s.j->'positions' \
                          ELSE '[]'::jsonb END) p \
                 WHERE p.value->1->>'qty' IS NOT NULL AND p.value->1->>'basis' IS NOT NULL \
                 UNION ALL \
                 SELECT s.account_id, r.value->0->>'market_id', r.value->0->>'outcome', \
                        (r.value->>1)::numeric, 0::numeric, true \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'returned') = 'array' THEN s.j->'returned' \
                          ELSE '[]'::jsonb END) r \
                 WHERE r.value->>1 IS NOT NULL \
             ) \
             SELECT h.account_id, h.market_id, h.outcome, h.qty, h.basis, h.returned, \
                    e.title AS event_title \
             FROM held h \
             LEFT JOIN legs l ON l.market_id = h.market_id \
             LEFT JOIN events e ON e.event_id = l.event_id \
             WHERE h.market_id IS NOT NULL AND h.outcome IS NOT NULL \
             ORDER BY h.account_id, h.returned, h.market_id, h.outcome LIMIT 1000",
        )
        .fetch_all(self.pool())
        .await?)
    }

    /// One account's ledger across live runs, newest first, keyset-paged on (run_id, seq).
    pub async fn paper_ledger_page(
        &self,
        account_id: &str,
        before: Option<(i64, i64)>,
        limit: i64,
    ) -> Result<LedgerPage, StoreError> {
        if !self.paper_ready().await? {
            return Ok(LedgerPage {
                rows: Vec::new(),
                next: None,
            });
        }
        let limit = limit.clamp(1, LEDGER_MAX);
        let (b_run, b_seq) = match before {
            Some((r, s)) => (Some(r), Some(s)),
            None => (None, None),
        };
        let mut rows = sqlx::query_as::<_, PaperLedgerEntry>(
            "SELECT l.run_id, l.account_id, l.seq, l.t_ms, l.kind, l.episode_id, l.market_id, \
                    l.outcome, l.qty, l.price, l.cash_delta, l.fees, l.realized_delta, \
                    l.cash_after, l.equity \
             FROM paper_ledger l \
             WHERE l.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND l.account_id = $1 \
               AND ($2::int8 IS NULL OR (l.run_id, l.seq) < ($2::int8, $3::int8)) \
             ORDER BY l.run_id DESC, l.seq DESC LIMIT $4",
        )
        .bind(account_id)
        .bind(b_run)
        .bind(b_seq)
        .bind(limit + 1)
        .fetch_all(self.pool())
        .await?;
        let keep = usize::try_from(limit).unwrap_or(usize::MAX);
        let next = if rows.len() > keep {
            rows.truncate(keep);
            rows.last().map(|r| (r.run_id, r.seq))
        } else {
            None
        };
        Ok(LedgerPage { rows, next })
    }
}
