//! Paper-trading rows (S3c, migration 0004; epochs 0009). Every table but `paper_state` is append-only and
//! idempotent (`ON CONFLICT DO NOTHING` on its natural key), so a batch the writer re-sends after
//! a commit whose ack was lost changes nothing. `paper_state` keeps the newest save per account.
use crate::text::{clean, clean_opt};
use crate::{Batch, PaperLedgerRow, PaperMarkRow, PaperStateRow, Store, StoreError};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use std::collections::HashMap;

/// ≤ 16 binds per row: 1000 rows stay far under Postgres's 65,535-parameter limit.
const CHUNK: usize = 1000;

const LEDGER_COLS: &str = "run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
                           qty, price, cash_delta, fees, realized_delta, cash_after, equity";
const MARK_COLS: &str = "run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
                         max_drawdown, n_positions";

/// Keep only the last save of each account (one statement can't upsert the same key twice).
fn dedupe_last_state(rows: &[PaperStateRow]) -> Vec<&PaperStateRow> {
    let mut idx: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<&PaperStateRow> = Vec::new();
    for r in rows {
        match idx.get(r.account_id.as_str()) {
            Some(&i) => out[i] = r,
            None => {
                idx.insert(r.account_id.as_str(), out.len());
                out.push(r);
            }
        }
    }
    out
}

/// Inside `write_batch`'s transaction. Accounts first: every other paper row references one.
pub(crate) async fn write_paper(conn: &mut PgConnection, b: &Batch) -> Result<(), StoreError> {
    for chunk in b.paper_accounts.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, \
             per_trade, policies_sha256) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.policy))
                .push_bind(clean(&r.scenario))
                .push_bind(r.fee_bips)
                .push_bind(r.bankroll)
                .push_bind(r.per_trade)
                .push_bind(clean(&r.policies_sha256));
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_intents.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_intents (run_id, account_id, episode_id, decided_ms, exec_ms, skip_reason, \
             q_target, detected_q, detected_pnl, reserve) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(r.decided_ms)
                .push_bind(r.exec_ms)
                .push_bind(clean_opt(&r.skip_reason))
                .push_bind(r.q_target)
                .push_bind(r.detected_q)
                .push_bind(r.detected_pnl)
                .push_bind(r.reserve);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_execs.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_execs (run_id, account_id, episode_id, exec_ms, q_eff, leg_risk_qty, \
             pnl_at_tx, hit) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(r.exec_ms)
                .push_bind(r.q_eff)
                .push_bind(r.leg_risk_qty)
                .push_bind(r.pnl_at_tx)
                .push_bind(r.hit);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_fills.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_fills (run_id, account_id, episode_id, market_id, yes_token, outcome, side, \
             t_ms, limit_price, requested, filled, vwap, fees, levels_used) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(clean(&r.market_id))
                .push_bind(clean(&r.yes_token))
                .push_bind(clean(&r.outcome))
                .push_bind(clean(&r.side))
                .push_bind(r.t_ms)
                .push_bind(r.limit_price)
                .push_bind(r.requested)
                .push_bind(r.filled)
                .push_bind(r.vwap)
                .push_bind(r.fees)
                .push_bind(r.levels_used);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_ledger.chunks(CHUNK) {
        let mut qb =
            QueryBuilder::<Postgres>::new(format!("INSERT INTO paper_ledger ({LEDGER_COLS}) "));
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(r.seq)
                .push_bind(r.t_ms)
                .push_bind(clean(&r.kind))
                .push_bind(clean_opt(&r.episode_id))
                .push_bind(clean_opt(&r.market_id))
                .push_bind(clean_opt(&r.yes_token))
                .push_bind(clean_opt(&r.outcome))
                .push_bind(r.qty)
                .push_bind(r.price)
                .push_bind(r.cash_delta)
                .push_bind(r.fees)
                .push_bind(r.realized_delta)
                .push_bind(r.cash_after)
                .push_bind(r.equity);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_markouts.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_markouts (run_id, account_id, episode_id, market_id, yes_token, outcome, \
             fill_vwap, offset_ms, mid, markout) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(clean(&r.market_id))
                .push_bind(clean(&r.yes_token))
                .push_bind(clean(&r.outcome))
                .push_bind(r.fill_vwap)
                .push_bind(r.offset_ms)
                .push_bind(r.mid)
                .push_bind(r.markout);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_marks.chunks(CHUNK) {
        let mut qb =
            QueryBuilder::<Postgres>::new(format!("INSERT INTO paper_marks ({MARK_COLS}) "));
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(r.t_ms)
                .push_bind(r.cash)
                .push_bind(r.equity)
                .push_bind(r.realized)
                .push_bind(r.unrealized)
                .push_bind(r.fees_paid)
                .push_bind(r.locked)
                .push_bind(r.max_drawdown)
                .push_bind(r.n_positions);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_epochs.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.epoch)
                .push_bind(r.start_ms)
                .push_bind(clean(&r.cause))
                .push_bind(r.request_id)
                .push_bind(r.run_id)
                .push_bind(clean(&r.policies_sha256));
        });
        // a resend after a lost ack changes nothing, as for every other paper table
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in dedupe_last_state(&b.paper_state).chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_state (account_id, state, updated_ms, policies_sha256) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(clean(&r.account_id))
                .push_bind(clean(&r.state))
                .push_bind(r.updated_ms)
                .push_bind(clean_opt(&r.policies_sha256));
        });
        // A restored (older) batch must never overwrite a newer save.
        qb.push(
            " ON CONFLICT (account_id) DO UPDATE SET state = EXCLUDED.state, updated_ms = EXCLUDED.updated_ms, \
             policies_sha256 = EXCLUDED.policies_sha256 \
             WHERE paper_state.updated_ms <= EXCLUDED.updated_ms",
        );
        qb.build().execute(&mut *conn).await?;
    }
    Ok(())
}

impl Store {
    pub async fn paper_ledger_for_run(
        &self,
        run_id: i64,
    ) -> Result<Vec<PaperLedgerRow>, StoreError> {
        let sql = format!(
            "SELECT {LEDGER_COLS} FROM paper_ledger WHERE run_id = $1 ORDER BY account_id, seq"
        );
        Ok(sqlx::query_as::<_, PaperLedgerRow>(&sql)
            .bind(run_id)
            .fetch_all(self.pool())
            .await?)
    }

    pub async fn paper_marks_for_run(&self, run_id: i64) -> Result<Vec<PaperMarkRow>, StoreError> {
        let sql = format!(
            "SELECT {MARK_COLS} FROM paper_marks WHERE run_id = $1 ORDER BY account_id, t_ms"
        );
        Ok(sqlx::query_as::<_, PaperMarkRow>(&sql)
            .bind(run_id)
            .fetch_all(self.pool())
            .await?)
    }

    /// The live accounts saved by the last run (replay never calls this).
    pub async fn load_paper_state(&self) -> Result<Vec<PaperStateRow>, StoreError> {
        Ok(sqlx::query_as::<_, PaperStateRow>(
            "SELECT account_id, state, updated_ms, policies_sha256 FROM paper_state ORDER BY account_id",
        )
        .fetch_all(self.pool())
        .await?)
    }
}
