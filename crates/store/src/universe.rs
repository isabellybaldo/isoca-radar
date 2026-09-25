//! Universe cache (warm start, §8.1.2) and the current events/legs tables (§10).
use crate::{EventRow, LegRow, Store, StoreError, UniverseSnapshot};
use sqlx::{Postgres, QueryBuilder};

const CHUNK: usize = 1000;

impl Store {
    pub async fn save_universe(
        &self,
        taken_ms: i64,
        n_events: i32,
        body: &str,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            "INSERT INTO universe_snapshots (taken_ms, n_events, body) VALUES ($1, $2, $3)",
        )
        .bind(taken_ms)
        .bind(n_events)
        .bind(body)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM universe_snapshots WHERE id NOT IN \
             (SELECT id FROM universe_snapshots ORDER BY taken_ms DESC, id DESC LIMIT 3)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn latest_universe(&self) -> Result<Option<UniverseSnapshot>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT taken_ms, n_events, body FROM universe_snapshots ORDER BY taken_ms DESC, id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn count_universe_snapshots(&self) -> Result<i64, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM universe_snapshots")
                .fetch_one(self.pool())
                .await?,
        )
    }

    /// Upsert one complete universe scan. Events first (legs reference them), one transaction.
    pub async fn upsert_universe(
        &self,
        events: &[EventRow],
        legs: &[LegRow],
        seen_ms: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await?;
        for chunk in events.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) ",
            );
            qb.push_values(chunk, |mut b, e| {
                b.push_bind(e.event_id.clone())
                    .push_bind(e.slug.clone())
                    .push_bind(e.title.clone())
                    .push_bind(e.neg_risk_market_id.clone())
                    .push_bind(e.augmented)
                    .push_bind(e.n_legs)
                    .push_bind(seen_ms)
                    .push_bind(seen_ms);
            });
            qb.push(
                " ON CONFLICT (event_id) DO UPDATE SET slug = EXCLUDED.slug, title = EXCLUDED.title, \
                 neg_risk_market_id = EXCLUDED.neg_risk_market_id, augmented = EXCLUDED.augmented, \
                 n_legs = EXCLUDED.n_legs, last_seen_ms = EXCLUDED.last_seen_ms, gone_ms = NULL",
            );
            qb.build().execute(&mut *tx).await?;
        }
        for chunk in legs.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, \
                 tick, min_size, seconds_delay, end_date, first_seen_ms, last_seen_ms) ",
            );
            qb.push_values(chunk, |mut b, l| {
                b.push_bind(l.market_id.clone())
                    .push_bind(l.event_id.clone())
                    .push_bind(l.yes_token.clone())
                    .push_bind(l.no_token.clone())
                    .push_bind(l.status.clone())
                    .push_bind(l.fee_rate)
                    .push_bind(l.fee_exponent)
                    .push_bind(l.tick)
                    .push_bind(l.min_size)
                    .push_bind(l.seconds_delay)
                    .push_bind(l.end_date.clone())
                    .push_bind(seen_ms)
                    .push_bind(seen_ms);
            });
            qb.push(
                " ON CONFLICT (market_id) DO UPDATE SET event_id = EXCLUDED.event_id, yes_token = EXCLUDED.yes_token, \
                 no_token = EXCLUDED.no_token, status = EXCLUDED.status, fee_rate = EXCLUDED.fee_rate, \
                 fee_exponent = EXCLUDED.fee_exponent, tick = EXCLUDED.tick, min_size = EXCLUDED.min_size, \
                 seconds_delay = EXCLUDED.seconds_delay, end_date = EXCLUDED.end_date, \
                 last_seen_ms = EXCLUDED.last_seen_ms, gone_ms = NULL",
            );
            qb.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Call only after a COMPLETE universe scan stamped `seen_ms` (spec §7: otherwise closed markets stay open forever).
    pub async fn mark_gone(&self, seen_ms: i64) -> Result<u64, StoreError> {
        let mut tx = self.pool().begin().await?;
        let events = sqlx::query(
            "UPDATE events SET gone_ms = $1 WHERE last_seen_ms < $1 AND gone_ms IS NULL",
        )
        .bind(seen_ms)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("UPDATE legs SET gone_ms = $1 WHERE last_seen_ms < $1 AND gone_ms IS NULL")
            .bind(seen_ms)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(events)
    }

    /// (first_seen_ms, last_seen_ms, gone_ms) of one event.
    pub async fn event_seen(&self, event_id: &str) -> Result<(i64, i64, Option<i64>), StoreError> {
        Ok(sqlx::query_as(
            "SELECT first_seen_ms, last_seen_ms, gone_ms FROM events WHERE event_id = $1",
        )
        .bind(event_id)
        .fetch_one(self.pool())
        .await?)
    }
}
