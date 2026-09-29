//! Paper-trading reads for the dashboard (Phase 4 spec §4), over S3c's migration 0004 tables.
//! Until 0004 is applied every reader returns an empty result, never an error. Live runs only:
//! replay runs write their own paper rows and never show here.
use crate::{Store, StoreError};
use rust_decimal::Decimal;

const LEDGER_MAX: i64 = 200;

/// Phase 7a (spec §4.1, §6): the bounds of epoch `$k` (NULL: the current, newest one) as a CTE
/// `eb(lo, hi)`: a row belongs to it when lo ≤ t < hi (hi NULL: open). Epoch 0 is everything
/// before epoch 1; an unknown epoch has lo NULL, so it matches nothing (empty, never an error).
fn epoch_bounds(k: usize) -> String {
    format!(
        "n AS (SELECT COALESCE(${k}::int4, (SELECT max(epoch) FROM paper_epochs), 0) AS e), \
         eb AS (SELECT CASE WHEN n.e = 0 THEN '-9223372036854775808'::int8 \
                            ELSE (SELECT p.start_ms FROM paper_epochs p WHERE p.epoch = n.e) END AS lo, \
                       (SELECT min(p.start_ms) FROM paper_epochs p WHERE p.epoch > n.e) AS hi \
                FROM n)"
    )
}

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
    /// The market's end date (`legs.end_date`, as Gamma sent it), for days to resolution. The
    /// 09-27 spec §7.4 says `events.end_date`; the column lives on `legs` (0001_init.sql:58).
    pub end_date: Option<String>,
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
    /// True once S3c's migration 0004 and Phase 7a's 0009 are applied (every table the dashboard reads).
    pub async fn paper_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT to_regclass('public.paper_accounts') IS NOT NULL \
                AND to_regclass('public.paper_marks') IS NOT NULL \
                AND to_regclass('public.paper_ledger') IS NOT NULL \
                AND to_regclass('public.paper_state') IS NOT NULL \
                AND to_regclass('public.paper_epochs') IS NOT NULL",
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

    /// Minute marks of one account across live runs (restarts restore the account), oldest
    /// first, inside epoch `epoch` (None: the current one, spec 7a §6).
    pub async fn paper_equity(
        &self,
        account_id: &str,
        from_ms: i64,
        to_ms: i64,
        epoch: Option<i32>,
    ) -> Result<Vec<EquityPoint>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        let sql = format!(
            "WITH {} \
             SELECT m.t_ms, m.equity FROM paper_marks m CROSS JOIN eb \
             WHERE m.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND m.account_id = $1 AND m.t_ms >= $2 AND m.t_ms < $3 \
               AND m.t_ms >= eb.lo AND (eb.hi IS NULL OR m.t_ms < eb.hi) \
             ORDER BY m.t_ms, m.run_id LIMIT 20160",
            epoch_bounds(4)
        );
        Ok(sqlx::query_as::<_, EquityPoint>(&sql)
            .bind(account_id)
            .bind(from_ms)
            .bind(to_ms)
            .bind(epoch)
            .fetch_all(self.pool())
            .await?)
    }

    /// Holdings in the live `paper_state` saves (sim::Account JSON: `positions` and `returned` are
    /// arrays of [LegKey, value] pairs). A state that isn't a JSON object, or has no such arrays,
    /// contributes nothing. A row whose `qty`/`basis` isn't a plain decimal (corrupt state, a
    /// stray object or string) is skipped rather than failing the whole query with a cast error:
    /// the regex guards the `::numeric` cast the same way a `jsonb_typeof(...) = 'number'` check
    /// would, since these values come out of `->>` as text regardless of the underlying JSON type.
    /// Only the newest live run's account ids (spec 7a §6: a removed account's stale row never shows).
    pub async fn paper_positions(&self) -> Result<Vec<PaperPosition>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        const NUM: &str = r"^-?[0-9]+(\.[0-9]+)?$";
        Ok(sqlx::query_as::<_, PaperPosition>(
            "WITH s AS ( \
                 SELECT account_id, state::jsonb AS j FROM paper_state WHERE state IS JSON OBJECT \
                   AND account_id IN (SELECT a.account_id FROM paper_accounts a \
                       WHERE a.run_id = (SELECT max(a2.run_id) FROM paper_accounts a2 \
                                         JOIN runs r2 ON r2.run_id = a2.run_id WHERE r2.mode = 'live')) \
             ), held AS ( \
                 SELECT s.account_id, p.value->0->>'market_id' AS market_id, \
                        p.value->0->>'outcome' AS outcome, \
                        (p.value->1->>'qty')::numeric AS qty, \
                        (p.value->1->>'basis')::numeric AS basis, false AS returned \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'positions') = 'array' THEN s.j->'positions' \
                          ELSE '[]'::jsonb END) p \
                 WHERE p.value->1->>'qty' ~ $1 AND p.value->1->>'basis' ~ $1 \
                 UNION ALL \
                 SELECT s.account_id, r.value->0->>'market_id', r.value->0->>'outcome', \
                        (r.value->>1)::numeric, 0::numeric, true \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'returned') = 'array' THEN s.j->'returned' \
                          ELSE '[]'::jsonb END) r \
                 WHERE r.value->>1 ~ $1 \
             ) \
             SELECT h.account_id, h.market_id, h.outcome, h.qty, h.basis, h.returned, \
                    e.title AS event_title, l.end_date \
             FROM held h \
             LEFT JOIN legs l ON l.market_id = h.market_id \
             LEFT JOIN events e ON e.event_id = l.event_id \
             WHERE h.market_id IS NOT NULL AND h.outcome IS NOT NULL \
             ORDER BY h.account_id, h.returned, h.market_id, h.outcome LIMIT 1000",
        )
        .bind(NUM)
        .fetch_all(self.pool())
        .await?)
    }

    /// One account's ledger across live runs, newest first, keyset-paged on (run_id, seq).
    /// `hide_marks` drops the minute `mark` rows before paging, so every page is full of the rest.
    /// Inside epoch `epoch` (None: the current one, spec 7a §6).
    pub async fn paper_ledger_page(
        &self,
        account_id: &str,
        before: Option<(i64, i64)>,
        limit: i64,
        hide_marks: bool,
        epoch: Option<i32>,
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
        let sql = format!(
            "WITH {} \
             SELECT l.run_id, l.account_id, l.seq, l.t_ms, l.kind, l.episode_id, l.market_id, \
                    l.outcome, l.qty, l.price, l.cash_delta, l.fees, l.realized_delta, \
                    l.cash_after, l.equity \
             FROM paper_ledger l CROSS JOIN eb \
             WHERE l.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND l.account_id = $1 \
               AND ($2::int8 IS NULL OR (l.run_id, l.seq) < ($2::int8, $3::int8)) \
               AND (NOT $5 OR l.kind <> 'mark') \
               AND l.t_ms >= eb.lo AND (eb.hi IS NULL OR l.t_ms < eb.hi) \
             ORDER BY l.run_id DESC, l.seq DESC LIMIT $4",
            epoch_bounds(6)
        );
        let mut rows = sqlx::query_as::<_, PaperLedgerEntry>(&sql)
            .bind(account_id)
            .bind(b_run)
            .bind(b_seq)
            .bind(limit + 1)
            .bind(hide_marks)
            .bind(epoch)
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
