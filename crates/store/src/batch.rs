//! One write transaction per batch (spec §10: ~100 ms batches from the scanner's store writer).
use crate::text::{clean, clean_opt};
use crate::{
    Batch, EpisodeLegRow, EpisodeRow, EpisodeWrite, HealthRow, ObservationRow, Store, StoreError,
};
use sqlx::postgres::PgListener;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;

/// Column order used for every episode insert/select. `push_episode` binds in exactly this order.
const EPISODE_COLS: [&str; 49] = [
    "episode_id",
    "run_id",
    "source",
    "kind",
    "realization",
    "event_id",
    "event_slug",
    "event_title",
    "neg_risk_market_id",
    "pair_id",
    "category",
    "n_outcomes",
    "n_legs",
    "legs_dropped",
    "augmented",
    "incomplete_set",
    "uncovered_legs",
    "opened_server_ms",
    "closed_server_ms",
    "duration_ms",
    "censor_left",
    "censor_right",
    "censor_gap",
    "detect_lag_ms",
    "n_updates",
    "peak_server_ms",
    "peak_edge_top",
    "peak_edge_top_gross",
    "peak_max_size",
    "peak_cost_usd",
    "peak_payout_usd",
    "peak_fees_usd",
    "peak_pnl_net_usd",
    "peak_pnl_gross_usd",
    "peak_profit_per_dollar",
    "pnl_net_budget100_usd",
    "bottleneck_leg_idx",
    "bottleneck_notional_usd",
    "below_min_size",
    "max_seconds_delay",
    "days_to_end",
    "annualized_return",
    "post_end",
    "engine_suspect",
    "stale_transport",
    "open_lag_ms",
    "oracle_downgraded",
    "scanner_version",
    "config_hash",
];

const LEG_COLS: &str = "episode_id, leg_idx, market_id, token, action, best_price, vwap, levels, size_at_best, \
                        filled, notional, fees, in_s, fee_rate, fee_exponent";

/// 49 binds per episode; 500 rows stays well under Postgres's 65,535-parameter limit.
const EP_CHUNK: usize = 500;
const CHUNK: usize = 1000;

fn push_episode(
    b: &mut sqlx::query_builder::Separated<'_, '_, Postgres, &'static str>,
    r: &EpisodeRow,
) {
    b.push_bind(clean(&r.episode_id))
        .push_bind(r.run_id)
        .push_bind(clean(&r.source))
        .push_bind(clean(&r.kind))
        .push_bind(clean(&r.realization))
        .push_bind(clean(&r.event_id))
        .push_bind(clean_opt(&r.event_slug))
        .push_bind(clean_opt(&r.event_title))
        .push_bind(clean_opt(&r.neg_risk_market_id))
        .push_bind(clean_opt(&r.pair_id))
        .push_bind(clean_opt(&r.category))
        .push_bind(r.n_outcomes)
        .push_bind(r.n_legs)
        .push_bind(r.legs_dropped)
        .push_bind(r.augmented)
        .push_bind(r.incomplete_set)
        .push_bind(r.uncovered_legs)
        .push_bind(r.opened_server_ms)
        .push_bind(r.closed_server_ms)
        .push_bind(r.duration_ms)
        .push_bind(r.censor_left)
        .push_bind(r.censor_right)
        .push_bind(r.censor_gap)
        .push_bind(r.detect_lag_ms)
        .push_bind(r.n_updates)
        .push_bind(r.peak_server_ms)
        .push_bind(r.peak_edge_top)
        .push_bind(r.peak_edge_top_gross)
        .push_bind(r.peak_max_size)
        .push_bind(r.peak_cost_usd)
        .push_bind(r.peak_payout_usd)
        .push_bind(r.peak_fees_usd)
        .push_bind(r.peak_pnl_net_usd)
        .push_bind(r.peak_pnl_gross_usd)
        .push_bind(r.peak_profit_per_dollar)
        .push_bind(r.pnl_net_budget100_usd)
        .push_bind(r.bottleneck_leg_idx)
        .push_bind(r.bottleneck_notional_usd)
        .push_bind(r.below_min_size)
        .push_bind(r.max_seconds_delay)
        .push_bind(r.days_to_end)
        .push_bind(r.annualized_return)
        .push_bind(r.post_end)
        .push_bind(r.engine_suspect)
        .push_bind(r.stale_transport)
        .push_bind(r.open_lag_ms)
        .push_bind(r.oracle_downgraded)
        .push_bind(clean(&r.scanner_version))
        .push_bind(clean(&r.config_hash));
}

/// Keep only the last write of each episode, in first-seen order (the scanner may upsert the same
/// episode at open and at peak within one batch).
fn dedupe_last(ws: &[EpisodeWrite]) -> Vec<&EpisodeWrite> {
    let mut idx: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<&EpisodeWrite> = Vec::new();
    for w in ws {
        match idx.get(w.row.episode_id.as_str()) {
            Some(&i) => out[i] = w,
            None => {
                idx.insert(w.row.episode_id.as_str(), out.len());
                out.push(w);
            }
        }
    }
    out
}

/// Keep only the last write of each (run_id, minute_ms), in first-seen order (the scanner may
/// upsert the same health minute twice within one batch, which would otherwise fail ON CONFLICT).
fn dedupe_last_health(hs: &[HealthRow]) -> Vec<&HealthRow> {
    let mut idx: HashMap<(i64, i64), usize> = HashMap::new();
    let mut out: Vec<&HealthRow> = Vec::new();
    for h in hs {
        match idx.get(&(h.run_id, h.minute_ms)) {
            Some(&i) => out[i] = h,
            None => {
                idx.insert((h.run_id, h.minute_ms), out.len());
                out.push(h);
            }
        }
    }
    out
}

/// Tables callers may count (no caller-built SQL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Observations,
    IntegrityEvents,
    Health,
    EpisodeLegs,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Observations => "observations",
            Table::IntegrityEvents => "integrity_events",
            Table::Health => "health",
            Table::EpisodeLegs => "episode_legs",
        }
    }
}

/// A LISTEN connection on one channel.
pub struct Listener(PgListener);

impl Listener {
    /// Wait for the next notification and return its payload.
    pub async fn recv_payload(&mut self) -> Result<String, StoreError> {
        Ok(self.0.recv().await?.payload().to_string())
    }
}

impl Store {
    pub async fn write_batch(&self, b: &Batch) -> Result<(), StoreError> {
        if b.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await?;

        let update_set = EPISODE_COLS[1..]
            .iter()
            .map(|c| format!("{c} = EXCLUDED.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let episodes = dedupe_last(&b.episodes);
        for chunk in episodes.chunks(EP_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(format!(
                "INSERT INTO episodes ({}) ",
                EPISODE_COLS.join(", ")
            ));
            qb.push_values(chunk, |mut sep, w| push_episode(&mut sep, &w.row));
            qb.push(format!(
                " ON CONFLICT (episode_id) DO UPDATE SET {update_set}"
            ));
            qb.build().execute(&mut *tx).await?;
        }

        if !episodes.is_empty() {
            let ids: Vec<String> = episodes.iter().map(|w| w.row.episode_id.clone()).collect();
            sqlx::query("DELETE FROM episode_legs WHERE episode_id = ANY($1)")
                .bind(ids)
                .execute(&mut *tx)
                .await?;
            let all_legs: Vec<&EpisodeLegRow> =
                episodes.iter().flat_map(|w| w.legs.iter()).collect();
            for chunk in all_legs.chunks(CHUNK) {
                let mut qb = QueryBuilder::<Postgres>::new(format!(
                    "INSERT INTO episode_legs ({LEG_COLS}) "
                ));
                qb.push_values(chunk, |mut s, l| {
                    s.push_bind(clean(&l.episode_id))
                        .push_bind(l.leg_idx)
                        .push_bind(clean(&l.market_id))
                        .push_bind(clean(&l.token))
                        .push_bind(clean(&l.action))
                        .push_bind(l.best_price)
                        .push_bind(l.vwap)
                        .push_bind(l.levels)
                        .push_bind(l.size_at_best)
                        .push_bind(l.filled)
                        .push_bind(l.notional)
                        .push_bind(l.fees)
                        .push_bind(l.in_s)
                        .push_bind(l.fee_rate)
                        .push_bind(l.fee_exponent);
                });
                qb.build().execute(&mut *tx).await?;
            }
        }

        for chunk in b.observations.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO observations (episode_id, offset_ms, source, observed_ms, skipped, skip_reason, edge, \
                 q_max, pnl, still_profitable, fill_at_original_size) ",
            );
            qb.push_values(chunk, |mut s, o| {
                s.push_bind(clean(&o.episode_id))
                    .push_bind(o.offset_ms)
                    .push_bind(clean(&o.source))
                    .push_bind(o.observed_ms)
                    .push_bind(o.skipped)
                    .push_bind(clean_opt(&o.skip_reason))
                    .push_bind(o.edge)
                    .push_bind(o.q_max)
                    .push_bind(o.pnl)
                    .push_bind(o.still_profitable)
                    .push_bind(o.fill_at_original_size);
            });
            qb.build().execute(&mut *tx).await?;
        }

        for chunk in b.integrity.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO integrity_events (run_id, ts_ms, kind, shard, token, event_id, detail) ",
            );
            qb.push_values(chunk, |mut s, i| {
                s.push_bind(i.run_id)
                    .push_bind(i.ts_ms)
                    .push_bind(clean(&i.kind))
                    .push_bind(i.shard)
                    .push_bind(clean_opt(&i.token))
                    .push_bind(clean_opt(&i.event_id))
                    .push_bind(clean_opt(&i.detail));
            });
            qb.build().execute(&mut *tx).await?;
        }

        let health = dedupe_last_health(&b.health);
        for chunk in health.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO health (run_id, minute_ms, tokens, shards, msgs, bytes, drops, reconnects, rest_p50_ms, \
                 rest_p99_ms, lag_p50_ms, lag_p99_ms, clock_offset_ms, ok, capture_paused, parked_shards, dropped_groups) ",
            );
            qb.push_values(chunk, |mut s, h| {
                s.push_bind(h.run_id)
                    .push_bind(h.minute_ms)
                    .push_bind(h.tokens)
                    .push_bind(h.shards)
                    .push_bind(h.msgs)
                    .push_bind(h.bytes)
                    .push_bind(h.drops)
                    .push_bind(h.reconnects)
                    .push_bind(h.rest_p50_ms)
                    .push_bind(h.rest_p99_ms)
                    .push_bind(h.lag_p50_ms)
                    .push_bind(h.lag_p99_ms)
                    .push_bind(h.clock_offset_ms)
                    .push_bind(h.ok)
                    .push_bind(h.capture_paused)
                    .push_bind(h.parked_shards)
                    .push_bind(h.dropped_groups);
            });
            qb.push(
                " ON CONFLICT (run_id, minute_ms) DO UPDATE SET tokens = EXCLUDED.tokens, shards = EXCLUDED.shards, \
                 msgs = EXCLUDED.msgs, bytes = EXCLUDED.bytes, drops = EXCLUDED.drops, reconnects = EXCLUDED.reconnects, \
                 rest_p50_ms = EXCLUDED.rest_p50_ms, rest_p99_ms = EXCLUDED.rest_p99_ms, lag_p50_ms = EXCLUDED.lag_p50_ms, \
                 lag_p99_ms = EXCLUDED.lag_p99_ms, clock_offset_ms = EXCLUDED.clock_offset_ms, ok = EXCLUDED.ok, \
                 capture_paused = EXCLUDED.capture_paused, parked_shards = EXCLUDED.parked_shards, \
                 dropped_groups = EXCLUDED.dropped_groups",
            );
            qb.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn episodes_for_run(&self, run_id: i64) -> Result<Vec<EpisodeRow>, StoreError> {
        let sql = format!(
            "SELECT {} FROM episodes WHERE run_id = $1 ORDER BY opened_server_ms, episode_id",
            EPISODE_COLS.join(", ")
        );
        Ok(sqlx::query_as::<_, EpisodeRow>(&sql)
            .bind(run_id)
            .fetch_all(self.pool())
            .await?)
    }

    pub async fn episode_legs(&self, episode_id: &str) -> Result<Vec<EpisodeLegRow>, StoreError> {
        let sql =
            format!("SELECT {LEG_COLS} FROM episode_legs WHERE episode_id = $1 ORDER BY leg_idx");
        Ok(sqlx::query_as::<_, EpisodeLegRow>(&sql)
            .bind(episode_id)
            .fetch_all(self.pool())
            .await?)
    }

    /// All observations for one episode, in write order.
    pub async fn observations_for(
        &self,
        episode_id: &str,
    ) -> Result<Vec<ObservationRow>, StoreError> {
        Ok(sqlx::query_as::<_, ObservationRow>(
            "SELECT episode_id, offset_ms, source, observed_ms, skipped, skip_reason, edge, \
             q_max, pnl, still_profitable, fill_at_original_size FROM observations \
             WHERE episode_id = $1 ORDER BY id",
        )
        .bind(episode_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn count(&self, table: Table) -> Result<i64, StoreError> {
        let sql = format!("SELECT count(*) FROM {}", table.name());
        Ok(sqlx::query_scalar(&sql).fetch_one(self.pool()).await?)
    }

    /// LISTEN on `channel` (the api's SSE bridge uses "episode"). The listener holds one pooled
    /// connection: drop it before `Store::close`, which waits for every checked-out connection.
    pub async fn listen(&self, channel: &str) -> Result<Listener, StoreError> {
        let mut l = PgListener::connect_with(self.pool()).await?;
        l.listen(channel).await?;
        Ok(Listener(l))
    }
}
