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
