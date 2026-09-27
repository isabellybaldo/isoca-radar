#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::{EventRow, LegRow};

fn event(id: &str) -> EventRow {
    EventRow {
        event_id: id.into(),
        slug: Some(format!("slug-{id}")),
        title: None,
        neg_risk_market_id: Some("0xabc".into()),
        augmented: false,
        n_legs: 1,
        tags: None,
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
async fn a_universe_transaction_gets_a_60s_statement_timeout_that_does_not_leak() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    // Opened the same way the store opens its universe transactions (save_universe,
    // upsert_universe, mark_gone): begin(), then SET LOCAL statement_timeout = '60s'.
    let baseline: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_ne!(baseline, "1min", "baseline should not be 60s");

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

    // The SET LOCAL setting must not leak past the transaction rollback.
    // The role's own 5 s default (docker/postgres/init/01-roles.sh) is set in real deployments
    // but CI doesn't run that script, so we only assert that the baseline is restored.
    let after_tx: String = sqlx::query_scalar("SHOW statement_timeout")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        after_tx, baseline,
        "timeout setting leaked past transaction"
    );
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

#[tokio::test]
async fn event_tags_are_written_once_and_frozen() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let mut e = event("ev1");
    e.tags = Some(vec!["politics".into(), "us-election".into()]);
    store
        .upsert_universe(&[e.clone()], &[], 1_000)
        .await
        .unwrap();
    e.tags = Some(vec!["sports".into()]); // a later refresh with other tags changes nothing
    store
        .upsert_universe(&[e.clone()], &[], 2_000)
        .await
        .unwrap();
    let mut f = event("ev2"); // first seen without tags, gains them later
    store
        .upsert_universe(&[f.clone()], &[], 1_000)
        .await
        .unwrap();
    f.tags = Some(vec!["crypto".into()]);
    store.upsert_universe(&[f], &[], 2_000).await.unwrap();
    assert_eq!(
        store
            .event_tags(&["ev1".into(), "ev2".into()])
            .await
            .unwrap(),
        vec![
            (
                "ev1".to_string(),
                Some(vec!["politics".to_string(), "us-election".to_string()])
            ),
            ("ev2".to_string(), Some(vec!["crypto".to_string()])),
        ]
    );
    db.drop(store).await;
}
