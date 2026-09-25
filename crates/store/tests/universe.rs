#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::{EventRow, LegRow, Store};

fn event(id: &str) -> EventRow {
    EventRow {
        event_id: id.into(),
        slug: Some(format!("slug-{id}")),
        title: None,
        neg_risk_market_id: Some("0xabc".into()),
        augmented: false,
        n_legs: 1,
    }
}

fn leg(market: &str, ev: &str) -> LegRow {
    LegRow {
        market_id: market.into(),
        event_id: ev.into(),
        yes_token: format!("{market}-y"),
        no_token: format!("{market}-n"),
        status: "live".into(),
        fee_rate: dec!(0.07),
        fee_exponent: 1,
        tick: Some(dec!(0.001)),
        min_size: Some(dec!(5)),
        seconds_delay: Some(0),
        end_date: None,
    }
}

#[tokio::test]
async fn keeps_three_newest_snapshots_and_returns_latest() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    assert!(store.latest_universe().await.unwrap().is_none());
    for t in 1..=5_i64 {
        store
            .save_universe(t * 1000, t as i32, &format!("{{\"v\":{t}}}"))
            .await
            .unwrap();
    }
    let latest = store.latest_universe().await.unwrap().unwrap();
    assert_eq!((latest.taken_ms, latest.n_events), (5000, 5));
    assert_eq!(latest.body, "{\"v\":5}");
    let n: i64 = store.count_universe_snapshots().await.unwrap();
    assert_eq!(n, 3);
    db.drop(store).await;
}

#[tokio::test]
async fn upsert_then_mark_gone_after_a_complete_scan() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    store
        .upsert_universe(
            &[event("e1"), event("e2")],
            &[leg("m1", "e1"), leg("m2", "e2")],
            100,
        )
        .await
        .unwrap();
    // second complete scan: e2 disappeared
    store
        .upsert_universe(&[event("e1")], &[leg("m1", "e1")], 200)
        .await
        .unwrap();
    assert_eq!(store.mark_gone(200).await.unwrap(), 1);
    assert_eq!(store.mark_gone(200).await.unwrap(), 0, "already gone");
    // e2 comes back: gone_ms cleared, first_seen kept
    store
        .upsert_universe(&[event("e2")], &[leg("m2", "e2")], 300)
        .await
        .unwrap();
    let (first, last, gone) = store.event_seen("e2").await.unwrap();
    assert_eq!((first, last, gone), (100, 300, None));
    db.drop(store).await;
}

#[tokio::test]
async fn universe_transactions_get_a_60s_statement_timeout_but_the_role_default_stays_5s() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    // Opened the same way the store opens its universe transactions (save_universe,
    // upsert_universe, mark_gone): begin(), then SET LOCAL statement_timeout = '60s'.
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '60s'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let shown: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(shown, "1min");
    tx.rollback().await.unwrap();

    // A fresh connection actually AUTHENTICATED as scanner_rw (not `SET ROLE`, which switches
    // privileges but never picks up a role's own `ALTER ROLE … SET` defaults) still reports its
    // 5 s default (docker/postgres/init/01-roles.sh): the SET LOCAL never leaks past its
    // transaction. Needs a real login, hence the password from .env (sourced by the store test
    // run recipe, never printed here).
    let scanner_pw = std::env::var("SCANNER_DB_PASSWORD")
        .expect("SCANNER_DB_PASSWORD must be set (source .env, as the store test recipe does)");
    let scanner_opts = db.opts.clone().username("scanner_rw").password(&scanner_pw);
    let scanner = Store::connect_with(scanner_opts, 1)
        .await
        .expect("login as scanner_rw");
    let shown: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(scanner.pool())
        .await
        .unwrap();
    assert_eq!(shown, "5s");
    scanner.close().await;
    db.drop(store).await;
}

#[tokio::test]
async fn duplicate_keys_in_one_scan_keep_the_last() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let first_event = event("e1");
    let mut last_event = event("e1");
    last_event.slug = Some("slug-e1-last".into());
    let first_leg = leg("m1", "e1");
    let mut last_leg = leg("m1", "e1");
    last_leg.min_size = Some(dec!(99));
    // Two rows with the same key in one scan must not fail ON CONFLICT DO UPDATE.
    store
        .upsert_universe(&[first_event, last_event], &[first_leg, last_leg], 100)
        .await
        .unwrap();
    // A later complete scan with the same keys is unaffected by the earlier duplicate.
    store
        .upsert_universe(&[event("e1")], &[leg("m1", "e1")], 200)
        .await
        .unwrap();
    assert_eq!(store.mark_gone(200).await.unwrap(), 0, "e1 was seen at 200");
    db.drop(store).await;
}
