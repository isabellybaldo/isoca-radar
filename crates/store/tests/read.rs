#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

const MIN: i64 = 60_000;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn ep(id: &str, run_id: i64, opened: i64) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(),
        run_id,
        source: "live".into(),
        kind: "no_basket".into(),
        realization: "converter".into(),
        event_id: "ev1".into(),
        event_slug: Some("slug".into()),
        event_title: Some("Bitcoin price on September 26?".into()),
        neg_risk_market_id: None,
        pair_id: None,
        category: None,
        n_outcomes: 7,
        n_legs: 7,
        legs_dropped: 0,
        augmented: false,
        incomplete_set: false,
        uncovered_legs: 0,
        opened_server_ms: opened,
        closed_server_ms: Some(opened + 4_190),
        duration_ms: Some(4_190),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        detect_lag_ms: Some(84),
        n_updates: 3,
        peak_server_ms: opened + 1_000,
        peak_edge_top: dec!(0.0049),
        peak_edge_top_gross: dec!(0.0137),
        peak_max_size: dec!(83),
        peak_cost_usd: dec!(99.59),
        peak_payout_usd: dec!(100),
        peak_fees_usd: dec!(0.96),
        peak_pnl_net_usd: dec!(0.41),
        peak_pnl_gross_usd: dec!(1.37),
        peak_profit_per_dollar: Some(dec!(0.0041)),
        pnl_net_budget100_usd: Some(dec!(0.33)),
        bottleneck_leg_idx: Some(2),
        bottleneck_notional_usd: Some(dec!(68.9)),
        below_min_size: Some(false),
        max_seconds_delay: Some(0),
        days_to_end: None,
        annualized_return: None,
        post_end: Some(false),
        engine_suspect: false,
        stale_transport: false,
        open_lag_ms: Some(12),
        oracle_downgraded: None,
        scanner_version: "sha".into(),
        config_hash: "cfg".into(),
    }
}

fn leg(id: &str, idx: i32, in_s: bool) -> EpisodeLegRow {
    EpisodeLegRow {
        episode_id: id.into(),
        leg_idx: idx,
        market_id: format!("m{idx}"),
        token: format!("t{idx}"),
        action: "buy_no".into(),
        best_price: Some(dec!(0.62)),
        vwap: Some(dec!(0.6231)),
        levels: 2,
        size_at_best: Some(dec!(140)),
        filled: dec!(83),
        notional: dec!(51.7),
        fees: dec!(0.21),
        in_s,
        fee_rate: dec!(0.07),
        fee_exponent: 1,
    }
}

fn health(run_id: i64, minute_ms: i64, drops: i32) -> HealthRow {
    HealthRow {
        run_id,
        minute_ms,
        tokens: 4_000,
        shards: 40,
        msgs: 258_000,
        bytes: 1,
        drops,
        reconnects: 0,
        rest_p50_ms: Some(80),
        rest_p99_ms: Some(300),
        lag_p50_ms: Some(78),
        lag_p99_ms: Some(1_900),
        clock_offset_ms: Some(3),
        ok: Some(true),
        capture_paused: false,
        parked_shards: 0,
        dropped_groups: 0,
    }
}

async fn run(store: &Store, mode: &str) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .unwrap()
}

async fn write_eps(store: &Store, rows: Vec<EpisodeRow>) {
    store
        .write_batch(&Batch {
            episodes: rows
                .into_iter()
                .map(|row| EpisodeWrite { row, legs: vec![] })
                .collect(),
            ..Batch::default()
        })
        .await
        .unwrap();
}

async fn ro(db: &common::TestDb) -> Store {
    Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_read_only_pool_cannot_write_even_as_the_owner() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let rw_owner = Store::connect_read_only(db.opts.clone(), 1, 5_000, None)
        .await
        .unwrap();
    let err = rw_owner
        .raw_exec("CREATE TABLE nope (a int)")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("read-only transaction"), "{err}");
    rw_owner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn role_can_write_is_false_only_for_analyze() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = ro(&db).await;
    assert!(!analyze.role_can_write().await.unwrap());
    let scanner = db.role("scanner_rw").await;
    assert!(scanner.role_can_write().await.unwrap());
    assert!(owner.role_can_write().await.unwrap());
    analyze.close().await;
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_statement_timeout_is_classified_unavailable() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let slow = Store::connect_read_only(db.opts.clone(), 1, 100, Some("analyze_ro"))
        .await
        .unwrap();
    let err = slow.raw_exec("SELECT pg_sleep(2)").await.unwrap_err();
    assert!(err.is_unavailable(), "{err}");
    assert!(!err.is_data_error());
    slow.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn health_now_is_the_newest_live_row_with_drops_over_5_minutes() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = ro(&db).await;
    assert_eq!(analyze.health_now(now()).await.unwrap(), None);
    assert_eq!(analyze.latest_health_minute().await.unwrap(), None);
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let n = now();
    let t = n - n % MIN;
    owner
        .write_batch(&Batch {
            health: vec![
                health(old, t, 99),
                health(live, t - 10 * MIN, 50), // outside the 5-minute window
                health(live, t - 2 * MIN, 2),
                health(live, t - MIN, 1),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let h = analyze.health_now(t + 30_000).await.unwrap().unwrap();
    assert_eq!(
        (h.row.run_id, h.row.minute_ms, h.drops_5m),
        (live, t - MIN, 3)
    );
    assert_eq!(analyze.latest_health_minute().await.unwrap(), Some(t - MIN));
    let series = analyze.health_series(t - 3 * MIN, t + MIN).await.unwrap();
    assert_eq!(
        series
            .iter()
            .map(|r| (r.run_id, r.minute_ms))
            .collect::<Vec<_>>(),
        vec![(live, t - 2 * MIN), (live, t - MIN), (old, t)]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn open_episodes_come_only_from_the_newest_live_run() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let crashed = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    let mut stale = ep("live-1-00000001", crashed, 1_000);
    stale.closed_server_ms = None;
    let mut open = ep("live-2-00000001", live, 2_000);
    open.closed_server_ms = None;
    open.duration_ms = None;
    let closed = ep("live-2-00000002", live, 3_000);
    let mut rep = ep("replay-3-00000001", replay, 4_000);
    rep.source = "replay".into();
    rep.closed_server_ms = None;
    owner
        .write_batch(&Batch {
            episodes: vec![
                EpisodeWrite {
                    row: stale,
                    legs: vec![],
                },
                EpisodeWrite {
                    row: open,
                    legs: vec![
                        leg("live-2-00000001", 0, true),
                        leg("live-2-00000001", 1, true),
                        leg("live-2-00000001", 2, false),
                    ],
                },
                EpisodeWrite {
                    row: closed,
                    legs: vec![],
                },
                EpisodeWrite {
                    row: rep,
                    legs: vec![],
                },
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let analyze = ro(&db).await;
    let rows = analyze.open_episodes().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].episode_id, "live-2-00000001");
    assert_eq!(rows[0].s_legs, 2);
    assert_eq!(rows[0].peak_pnl_net_usd, dec!(0.41));
    assert!(!rows[0].censored);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn episodes_page_filters_by_kind_net_and_flags_inside_the_range() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let a = ep("live-1-a", live, 1_000);
    let mut b = ep("live-1-b", live, 2_000);
    b.kind = "yes_basket".into();
    b.peak_pnl_net_usd = dec!(-0.12);
    b.engine_suspect = true;
    let mut c = ep("live-1-c", live, 3_000);
    c.censor_gap = true;
    c.stale_transport = true;
    let d = ep("live-1-d", live, 10_000); // outside [0, 5000)
    write_eps(&owner, vec![a, b, c, d]).await;
    let analyze = ro(&db).await;
    let base = EpisodeFilter {
        kind: None,
        from_ms: 0,
        to_ms: 5_000,
        net_positive: false,
        suspect: false,
        stale: false,
        censored: false,
        before: None,
        limit: 50,
    };
    let ids = |p: EpisodePage| p.rows.into_iter().map(|r| r.episode_id).collect::<Vec<_>>();
    assert_eq!(
        ids(analyze.episodes_page(&base).await.unwrap()),
        ["live-1-c", "live-1-b", "live-1-a"]
    );
    let f = EpisodeFilter {
        kind: Some("yes_basket".into()),
        ..base.clone()
    };
    assert_eq!(ids(analyze.episodes_page(&f).await.unwrap()), ["live-1-b"]);
    let f = EpisodeFilter {
        net_positive: true,
        ..base.clone()
    };
    assert_eq!(
        ids(analyze.episodes_page(&f).await.unwrap()),
        ["live-1-c", "live-1-a"]
    );
    let f = EpisodeFilter {
        suspect: true,
        ..base.clone()
    };
    assert_eq!(ids(analyze.episodes_page(&f).await.unwrap()), ["live-1-b"]);
    let f = EpisodeFilter {
        stale: true,
        censored: true,
        ..base.clone()
    };
    let page = analyze.episodes_page(&f).await.unwrap();
    assert!(page.rows[0].censored);
    assert_eq!(ids(page), ["live-1-c"]);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn episodes_page_keyset_has_no_duplicates_or_gaps_on_ties() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let rows: Vec<EpisodeRow> = (0..5)
        .map(|i| ep(&format!("live-1-{i:08}"), live, 7_000))
        .collect();
    write_eps(&owner, rows).await;
    let analyze = ro(&db).await;
    let mut f = EpisodeFilter {
        kind: None,
        from_ms: 0,
        to_ms: 10_000,
        net_positive: false,
        suspect: false,
        stale: false,
        censored: false,
        before: None,
        limit: 2,
    };
    let mut seen = Vec::new();
    for _ in 0..5 {
        let page = analyze.episodes_page(&f).await.unwrap();
        seen.extend(page.rows.iter().map(|r| r.episode_id.clone()));
        match page.next {
            Some(n) => f.before = Some(n),
            None => break,
        }
    }
    assert_eq!(
        seen,
        [
            "live-1-00000004",
            "live-1-00000003",
            "live-1-00000002",
            "live-1-00000001",
            "live-1-00000000"
        ]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn episode_detail_round_trips_and_a_missing_one_is_none() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let mut row = ep("live-1-00000009", live, 1_000);
    row.peak_fees_usd = dec!(0);
    write_eps(&owner, vec![row.clone()]).await;
    let analyze = ro(&db).await;
    assert_eq!(analyze.episode("live-1-00000009").await.unwrap(), Some(row));
    assert_eq!(analyze.episode("nope").await.unwrap(), None);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn today_counts_gross_and_net_per_utc_hour_excluding_not_arbitrage() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let t = now();
    let day = t - t.rem_euclid(DAY_MS);
    let a = ep("live-1-a", live, day + 1_000); // hour 0: gross and net
    let mut b = ep("live-1-b", live, day + 3_600_000 + 5); // hour 1: gross only
    b.peak_pnl_net_usd = dec!(-0.2);
    let mut c = ep("live-1-c", live, day + 2_000); // not arbitrage: never counted
    c.realization = "not_arbitrage".into();
    c.kind = "yes_basket_incomplete".into();
    let d = ep("live-1-d", live, day - 1_000); // yesterday
    write_eps(&owner, vec![a, b, c, d]).await;
    let analyze = ro(&db).await;
    let today = analyze.today(t).await.unwrap();
    assert_eq!((today.day_start_ms, today.gross, today.net), (day, 2, 1));
    assert_eq!(today.hourly.len(), 24);
    assert_eq!(
        (today.hourly[0], today.hourly[1], today.hourly[2]),
        ((1, 1), (1, 0), (0, 0))
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn integrity_summary_counts_by_kind_and_per_million_messages() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    let row = |run_id: i64, ts_ms: i64, kind: &str| IntegrityRow {
        run_id,
        ts_ms,
        kind: kind.into(),
        shard: None,
        token: None,
        event_id: None,
        detail: None,
    };
    owner
        .write_batch(&Batch {
            integrity: vec![
                row(live, 1_000, "top_mismatch"),
                row(live, 2_000, "top_mismatch"),
                row(live, 3_000, "drift"),
                row(live, 99_000_000, "drift"), // outside the window
                row(replay, 1_500, "crossed_book"), // replay: never counted
            ],
            health: vec![health(live, 0, 0)], // 258,000 msgs
            ..Batch::default()
        })
        .await
        .unwrap();
    let analyze = ro(&db).await;
    let s = analyze.integrity_summary(0, 60_000).await.unwrap();
    assert_eq!(
        s.by_kind,
        vec![("top_mismatch".to_string(), 2), ("drift".to_string(), 1)]
    );
    assert_eq!((s.total, s.msgs), (3, 258_000));
    assert_eq!(s.per_million, Some(dec!(11.63))); // 3e6 / 258000 = 11.627…
    let empty = analyze
        .integrity_summary(200_000_000, 200_060_000)
        .await
        .unwrap();
    assert_eq!((empty.total, empty.msgs, empty.per_million), (0, 0, None));
    analyze.close().await;
    db.drop(owner).await;
}
