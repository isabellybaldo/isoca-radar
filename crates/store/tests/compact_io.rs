#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

const H: i64 = 3_600_000;
const T0: i64 = 1_790_121_600_000;

fn ep(
    id: &str,
    run_id: i64,
    source: &str,
    kind: &str,
    opened: i64,
    closed: Option<i64>,
    pnl: rust_decimal::Decimal,
) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(),
        run_id,
        source: source.into(),
        kind: kind.into(),
        realization: "converter".into(),
        event_id: "ev1".into(),
        event_slug: None,
        event_title: None,
        neg_risk_market_id: None,
        pair_id: None,
        category: None,
        n_outcomes: 3,
        n_legs: 3,
        legs_dropped: 0,
        augmented: false,
        incomplete_set: false,
        uncovered_legs: 0,
        opened_server_ms: opened,
        closed_server_ms: closed,
        duration_ms: closed.map(|c| c - opened),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        detect_lag_ms: None,
        n_updates: 1,
        peak_server_ms: opened,
        peak_edge_top: dec!(0.0049),
        peak_edge_top_gross: dec!(0.0137),
        peak_max_size: dec!(83),
        peak_cost_usd: dec!(99.59),
        peak_payout_usd: dec!(100),
        peak_fees_usd: dec!(0.96),
        peak_pnl_net_usd: pnl,
        peak_pnl_gross_usd: dec!(1.37),
        peak_profit_per_dollar: None,
        pnl_net_budget100_usd: Some(dec!(0.33)),
        bottleneck_leg_idx: None,
        bottleneck_notional_usd: None,
        below_min_size: None,
        max_seconds_delay: None,
        days_to_end: None,
        annualized_return: None,
        post_end: None,
        engine_suspect: false,
        stale_transport: false,
        open_lag_ms: None,
        oracle_downgraded: None,
        scanner_version: "t".into(),
        config_hash: "c".into(),
    }
}

async fn run(store: &Store, mode: &str) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: T0,
            version: "t".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .unwrap()
}

async fn compact(db: &common::TestDb) -> Store {
    Store::connect_compact(db.opts.clone(), 2, 10_000, Some("compact_rw"))
        .await
        .unwrap()
}

fn done(hour_ms: i64) -> CompactHourRow {
    CompactHourRow {
        hour_ms,
        status: "done".into(),
        source_file: Some("2026-09-23/00.jsonl.zst".into()),
        source_bytes: Some(123),
        source_sha256: Some("ab".repeat(32)),
        rows: Some(r#"{"tob":5,"depth":1,"l2":0,"gaps":0,"tokens":2}"#.into()),
        done_ms: T0 + 2 * H,
    }
}

#[tokio::test]
async fn migration_0008_gives_each_role_exactly_its_compact_rights() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let c = compact(&db).await;
    assert_eq!(
        c.role_writes_outside_compact().await.unwrap(),
        Vec::<String>::new(),
        "compact_rw writes only compact tables"
    );
    assert!(
        c.compact_episodes(0, i64::MAX).await.unwrap().is_empty(),
        "compact_rw reads episodes"
    );
    c.raw_exec("SELECT 1 FROM runs LIMIT 1")
        .await
        .expect("compact_rw reads runs");
    c.compact_hour_put(&done(T0)).await.unwrap();
    let scanner = db.role("scanner_rw").await;
    assert_eq!(
        scanner.compact_hour_statuses(T0, T0 + H).await.unwrap(),
        vec![(T0, "done".to_string())]
    );
    assert_eq!(scanner.mark_hours_lost(&[T0 + H], T0).await.unwrap(), 1);
    let upd = scanner.compact_hour_put(&done(T0 + H)).await.unwrap_err();
    assert!(
        upd.to_string().contains("permission denied"),
        "scanner_rw cannot update compact_hours: {upd}"
    );
    assert_eq!(
        scanner.compact_status().await.unwrap(),
        None,
        "scanner_rw reads compact_status (rollback rule)"
    );
    let api = db.role("analyze_ro").await;
    assert_eq!(
        api.compact_status().await.unwrap(),
        None,
        "analyze_ro reads compact_status"
    );
    assert_eq!(
        api.compact_first_done_ms().await.unwrap(),
        Some(T0),
        "analyze_ro reads compact_hours"
    );
    let analyze = Store::connect_analyze(db.opts.clone(), 2, 10_000, Some("analyze_rw"))
        .await
        .unwrap();
    assert_eq!(
        analyze.role_writes_outside_cards().await.unwrap(),
        Vec::<String>::new(),
        "analyze_rw unchanged"
    );
    assert!(
        owner
            .role_writes_outside_compact()
            .await
            .unwrap()
            .contains(&"episodes".to_string())
    );
    for s in [c, scanner, api, analyze] {
        s.close().await;
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn mark_hours_lost_never_downgrades_a_done_hour() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let c = compact(&db).await;
    c.compact_hour_put(&done(T0)).await.unwrap();
    assert_eq!(
        owner
            .mark_hours_lost(&[T0, T0 + H], T0 + 3 * H)
            .await
            .unwrap(),
        1,
        "only the unknown hour is inserted"
    );
    assert_eq!(
        owner.compact_first_done_ms().await.unwrap(),
        Some(T0 + 2 * H),
        "the earliest done_ms"
    );
    assert_eq!(
        owner.compact_hour_statuses(T0, T0 + 5 * H).await.unwrap(),
        vec![(T0, "done".to_string()), (T0 + H, "lost".to_string())]
    );
    // the compactor may still finish a lost hour it had read: its word is final
    c.compact_hour_put(&done(T0 + H)).await.unwrap();
    assert_eq!(
        owner
            .compact_hour_statuses(T0 + H, T0 + 2 * H)
            .await
            .unwrap(),
        vec![(T0 + H, "done".to_string())]
    );
    c.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn compact_episodes_are_live_net_positive_baskets_opened_in_range() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    owner
        .write_batch(&Batch {
            episodes: [
                ep(
                    "a",
                    live,
                    "live",
                    "no_basket",
                    T0 + 10,
                    Some(T0 + 900),
                    dec!(0.41),
                ),
                ep("b", live, "live", "yes_basket", T0 + 20, None, dec!(0.01)),
                ep("c", live, "live", "no_basket", T0 + 30, None, dec!(0)),
                ep(
                    "d",
                    live,
                    "live",
                    "yes_basket_incomplete",
                    T0 + 40,
                    None,
                    dec!(3),
                ),
                ep("e", replay, "replay", "no_basket", T0 + 50, None, dec!(3)),
                ep("f", live, "live", "no_basket", T0 + H, None, dec!(3)),
            ]
            .into_iter()
            .map(|row| EpisodeWrite { row, legs: vec![] })
            .collect(),
            ..Batch::default()
        })
        .await
        .unwrap();
    let c = compact(&db).await;
    let got = c.compact_episodes(T0, T0 + H).await.unwrap();
    assert_eq!(
        got,
        vec![
            CompactEpisode {
                episode_id: "a".into(),
                event_id: "ev1".into(),
                opened_ms: T0 + 10,
                closed_ms: Some(T0 + 900)
            },
            CompactEpisode {
                episode_id: "b".into(),
                event_id: "ev1".into(),
                opened_ms: T0 + 20,
                closed_ms: None
            },
        ]
    );
    c.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn compact_status_is_one_row_that_round_trips() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let c = compact(&db).await;
    assert_eq!(
        owner.compact_first_done_ms().await.unwrap(),
        None,
        "no hour recorded yet"
    );
    let s = CompactStatusRow {
        updated_ms: T0,
        archive_bytes: 12_345,
        oldest_day: Some("2026-09-23".into()),
        lag_hours: 1,
        last_error: None,
    };
    c.compact_status_put(&s).await.unwrap();
    let s2 = CompactStatusRow {
        updated_ms: T0 + H,
        archive_bytes: 99,
        oldest_day: None,
        lag_hours: 4,
        last_error: Some("read episodes: timeout".into()),
    };
    c.compact_status_put(&s2).await.unwrap();
    assert_eq!(owner.compact_status().await.unwrap(), Some(s2));
    c.close().await;
    db.drop(owner).await;
}
