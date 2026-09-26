#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use std::time::Duration;
use store::*;

fn episode(id: &str, run_id: i64) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(),
        run_id,
        source: "live".into(),
        kind: "no_basket".into(),
        realization: "converter".into(),
        event_id: "ev1".into(),
        event_slug: Some("slug".into()),
        event_title: Some("title".into()),
        neg_risk_market_id: Some("0xnr".into()),
        pair_id: None,
        category: Some("politics".into()),
        n_outcomes: 11,
        n_legs: 12,
        legs_dropped: 13,
        augmented: true,
        incomplete_set: false,
        uncovered_legs: 14,
        opened_server_ms: 1_000,
        closed_server_ms: Some(2_300),
        duration_ms: Some(1_300),
        censor_left: true,
        censor_right: false,
        censor_gap: true,
        detect_lag_ms: Some(15),
        n_updates: 16,
        peak_server_ms: 1_500,
        peak_edge_top: dec!(0.000001),
        peak_edge_top_gross: dec!(0.000002),
        peak_max_size: dec!(3.123456789012),
        peak_cost_usd: dec!(4.4),
        peak_payout_usd: dec!(5.5),
        peak_fees_usd: dec!(0),
        peak_pnl_net_usd: dec!(6.6),
        peak_pnl_gross_usd: dec!(7.7),
        peak_profit_per_dollar: Some(dec!(8.8)),
        pnl_net_budget100_usd: Some(dec!(11.11)),
        bottleneck_leg_idx: Some(17),
        bottleneck_notional_usd: Some(dec!(9.9)),
        below_min_size: Some(false),
        max_seconds_delay: Some(18),
        days_to_end: Some(dec!(10.1)),
        annualized_return: None,
        post_end: Some(true),
        engine_suspect: false,
        stale_transport: true,
        open_lag_ms: Some(19),
        oracle_downgraded: None,
        scanner_version: "sha".into(),
        config_hash: "cfg".into(),
    }
}

fn legs(id: &str) -> Vec<EpisodeLegRow> {
    (0..2)
        .map(|i| EpisodeLegRow {
            episode_id: id.into(),
            leg_idx: i,
            market_id: format!("m{i}"),
            token: format!("t{i}"),
            action: "buy_no".into(),
            best_price: Some(dec!(0.412)),
            vwap: Some(dec!(0.41234567)),
            levels: 3 + i,
            size_at_best: Some(dec!(12.5)),
            filled: dec!(20.25),
            notional: dec!(8.123456),
            fees: dec!(0.00001),
            in_s: i == 0,
            fee_rate: dec!(0.07),
            fee_exponent: 1,
        })
        .collect()
}

async fn new_run(store: &Store) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn episode_round_trips_exactly_with_zeros_and_nulls() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let row = episode("01J0000000000000000000000A", run);
    let batch = Batch {
        episodes: vec![EpisodeWrite {
            row: row.clone(),
            legs: legs(&row.episode_id),
        }],
        ..Batch::default()
    };
    store.write_batch(&batch).await.unwrap();
    let back = store.episodes_for_run(run).await.unwrap();
    assert_eq!(back, vec![row.clone()]);
    assert_eq!(back[0].peak_fees_usd, dec!(0));
    assert_eq!(back[0].annualized_return, None);
    assert_eq!(
        store.episode_legs(&row.episode_id).await.unwrap(),
        legs(&row.episode_id)
    );
    db.drop(store).await;
}

#[tokio::test]
async fn upsert_rewrites_the_episode_and_replaces_its_legs() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut row = episode("01J0000000000000000000000B", run);
    row.closed_server_ms = None;
    row.duration_ms = None;
    store
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: row.clone(),
                legs: legs(&row.episode_id),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    row.closed_server_ms = Some(9_000);
    row.duration_ms = Some(8_000);
    let one_leg = vec![legs(&row.episode_id).remove(1)];
    store
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: row.clone(),
                legs: one_leg.clone(),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        store.episodes_for_run(run).await.unwrap(),
        vec![row.clone()]
    );
    assert_eq!(store.episode_legs(&row.episode_id).await.unwrap(), one_leg);
    db.drop(store).await;
}

#[tokio::test]
async fn observations_integrity_and_health_land_and_health_upserts() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let row = episode("01J0000000000000000000000C", run);
    let health = HealthRow {
        run_id: run,
        minute_ms: 60_000,
        tokens: 1,
        shards: 2,
        msgs: 3,
        bytes: 4,
        drops: 0,
        reconnects: 0,
        rest_p50_ms: None,
        rest_p99_ms: None,
        lag_p50_ms: Some(150),
        lag_p99_ms: Some(290),
        clock_offset_ms: None,
        ok: Some(true),
        capture_paused: false,
        parked_shards: 0,
        dropped_groups: 0,
    };
    let batch = Batch {
        episodes: vec![EpisodeWrite {
            row: row.clone(),
            legs: vec![],
        }],
        observations: vec![ObservationRow {
            episode_id: row.episode_id.clone(),
            offset_ms: 1_000,
            source: "rest".into(),
            observed_ms: Some(2_000),
            skipped: false,
            skip_reason: None,
            edge: Some(dec!(0)),
            q_max: Some(dec!(0)),
            pnl: Some(dec!(0)),
            still_profitable: Some(false),
            fill_at_original_size: Some(false),
        }],
        integrity: vec![IntegrityRow {
            run_id: run,
            ts_ms: 5,
            kind: "top_mismatch".into(),
            shard: Some(3),
            token: Some("t".into()),
            event_id: None,
            detail: Some("bid 0.41 vs 0.42".into()),
        }],
        health: vec![health.clone()],
        ..Batch::default()
    };
    store.write_batch(&batch).await.unwrap();
    let mut again = health.clone();
    again.msgs = 30;
    store
        .write_batch(&Batch {
            health: vec![again],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(store.count(Table::Observations).await.unwrap(), 1);
    assert_eq!(store.count(Table::IntegrityEvents).await.unwrap(), 1);
    assert_eq!(store.count(Table::Health).await.unwrap(), 1);
    db.drop(store).await;
}

#[tokio::test]
async fn the_same_episode_twice_in_one_batch_keeps_the_last_version() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut first = episode("01J0000000000000000000000E", run);
    first.closed_server_ms = None;
    first.duration_ms = None;
    let last = episode("01J0000000000000000000000E", run);
    let batch = Batch {
        episodes: vec![
            EpisodeWrite {
                row: first,
                legs: legs("01J0000000000000000000000E"),
            },
            EpisodeWrite {
                row: last.clone(),
                legs: vec![],
            },
        ],
        ..Batch::default()
    };
    store.write_batch(&batch).await.unwrap();
    assert_eq!(store.episodes_for_run(run).await.unwrap(), vec![last]);
    assert!(
        store
            .episode_legs("01J0000000000000000000000E")
            .await
            .unwrap()
            .is_empty()
    );
    db.drop(store).await;
}

#[tokio::test]
async fn empty_batch_is_a_no_op() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    store.write_batch(&Batch::default()).await.unwrap();
    db.drop(store).await;
}

#[tokio::test]
async fn writing_an_episode_notifies_the_episode_channel() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut listener = store.listen("episode").await.unwrap();
    let row = episode("01J0000000000000000000000D", run);
    store
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: row.clone(),
                legs: vec![],
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    let payload = tokio::time::timeout(Duration::from_secs(5), listener.recv_payload())
        .await
        .expect("notification within 5 s")
        .unwrap();
    assert_eq!(payload, row.episode_id);
    // The listener holds a pooled connection; Pool::close waits for it, so release it first.
    drop(listener);
    db.drop(store).await;
}

/// Bool columns can't all hold distinct values in one row, so write 4 rows where bool column j of row e
/// is bit e of j: any two bool columns differ in at least one row, so a swapped bind cannot round-trip.
#[tokio::test]
async fn bool_columns_round_trip_under_distinct_bit_patterns() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let bit = |j: u32, e: u32| (j >> e) & 1 == 1;
    let rows: Vec<EpisodeRow> = (0..4u32)
        .map(|e| {
            let mut r = episode(&format!("01J00000000000000000000B{e}"), run);
            r.opened_server_ms = 1_000 + i64::from(e); // keep the read-back order deterministic
            r.augmented = bit(1, e);
            r.incomplete_set = bit(2, e);
            r.censor_left = bit(3, e);
            r.censor_right = bit(4, e);
            r.censor_gap = bit(5, e);
            r.below_min_size = Some(bit(6, e));
            r.post_end = Some(bit(7, e));
            r.engine_suspect = bit(8, e);
            r.stale_transport = bit(9, e);
            r.oracle_downgraded = Some(bit(10, e));
            r
        })
        .collect();
    let batch = Batch {
        episodes: rows
            .iter()
            .map(|r| EpisodeWrite {
                row: r.clone(),
                legs: vec![],
            })
            .collect(),
        ..Batch::default()
    };
    store.write_batch(&batch).await.unwrap();
    assert_eq!(store.episodes_for_run(run).await.unwrap(), rows);
    assert_eq!(store.count(Table::EpisodeLegs).await.unwrap(), 0);
    db.drop(store).await;
}

#[tokio::test]
async fn a_poison_row_rolls_back_the_whole_batch_and_is_flagged_as_data_error() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let row = episode("01J0000000000000000000000F", run);
    let batch = Batch {
        episodes: vec![EpisodeWrite {
            row: row.clone(),
            legs: vec![],
        }],
        integrity: vec![IntegrityRow {
            run_id: run,
            ts_ms: 1,
            kind: "not_a_kind".into(),
            shard: None,
            token: None,
            event_id: None,
            detail: None,
        }],
        ..Batch::default()
    };
    let err = store.write_batch(&batch).await.unwrap_err();
    assert!(err.is_data_error());
    assert!(store.episodes_for_run(run).await.unwrap().is_empty());
    db.drop(store).await;
}

#[tokio::test]
async fn nul_bytes_are_replaced_not_rejected() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut row = episode("01J0000000000000000000000G", run);
    row.event_title = Some("a\0b".into());
    let batch = Batch {
        episodes: vec![EpisodeWrite {
            row: row.clone(),
            legs: vec![],
        }],
        integrity: vec![IntegrityRow {
            run_id: run,
            ts_ms: 1,
            kind: "drift".into(),
            shard: None,
            token: None,
            event_id: None,
            detail: Some("x\0".into()),
        }],
        ..Batch::default()
    };
    store.write_batch(&batch).await.unwrap();
    let back = store.episodes_for_run(run).await.unwrap();
    assert_eq!(back[0].event_title, Some("a\u{FFFD}b".into()));
    assert_eq!(store.count(Table::IntegrityEvents).await.unwrap(), 1);
    db.drop(store).await;
}

#[tokio::test]
async fn duplicate_health_minute_in_one_batch_keeps_the_last() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let first = HealthRow {
        run_id: run,
        minute_ms: 60_000,
        tokens: 1,
        shards: 1,
        msgs: 1,
        bytes: 1,
        drops: 0,
        reconnects: 0,
        rest_p50_ms: None,
        rest_p99_ms: None,
        lag_p50_ms: None,
        lag_p99_ms: None,
        clock_offset_ms: None,
        ok: Some(true),
        capture_paused: false,
        parked_shards: 0,
        dropped_groups: 0,
    };
    let mut second = first.clone();
    second.msgs = 2;
    store
        .write_batch(&Batch {
            health: vec![first, second],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(store.count(Table::Health).await.unwrap(), 1);
    db.drop(store).await;
}

#[tokio::test]
async fn observation_zero_reads_back_distinct_from_null() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let row = episode("01J0000000000000000000000H", run);
    let obs = ObservationRow {
        episode_id: row.episode_id.clone(),
        offset_ms: 1_000,
        source: "rest".into(),
        observed_ms: Some(2_000),
        skipped: false,
        skip_reason: None,
        edge: None,
        q_max: None,
        pnl: Some(dec!(0)),
        still_profitable: Some(true),
        fill_at_original_size: Some(true),
    };
    store
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: row.clone(),
                legs: vec![],
            }],
            observations: vec![obs.clone()],
            ..Batch::default()
        })
        .await
        .unwrap();
    let back = store.observations_for(&row.episode_id).await.unwrap();
    assert_eq!(back, vec![obs]);
    assert_eq!(back[0].pnl, Some(dec!(0)));
    assert_eq!(back[0].edge, None);
    db.drop(store).await;
}
