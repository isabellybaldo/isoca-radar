#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

const H: i64 = 3_600_000;

fn ep(id: &str, run_id: i64, kind: &str, opened: i64, closed: Option<i64>) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(),
        run_id,
        source: "live".into(),
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
        peak_pnl_net_usd: dec!(0.41),
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

async fn run_at(store: &Store, mode: &str, started_ms: i64) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms,
            version: "t".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .unwrap()
}

async fn premises(store: &Store, run_id: i64) {
    store
        .insert_run_premises(&[RunPremiseRow {
            run_id,
            area: "fees".into(),
            version: "1+e3b0c442".into(),
        }])
        .await
        .unwrap();
}

async fn write(store: &Store, rows: Vec<EpisodeRow>) {
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

async fn rw(db: &common::TestDb) -> Store {
    Store::connect_analyze(db.opts.clone(), 2, 10_000, Some("analyze_rw"))
        .await
        .unwrap()
}

#[tokio::test]
async fn the_startup_check_sees_writes_outside_the_card_tables() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = rw(&db).await;
    assert_eq!(
        analyze.role_writes_outside_cards().await.unwrap(),
        Vec::<String>::new()
    );
    let scanner = db.role("scanner_rw").await;
    assert!(
        scanner
            .role_writes_outside_cards()
            .await
            .unwrap()
            .contains(&"episodes".to_string())
    );
    assert!(
        owner
            .role_writes_outside_cards()
            .await
            .unwrap()
            .contains(&"runs".to_string())
    );
    analyze.close().await;
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn premise_runs_are_live_scanner_runs_with_premises_and_their_ends() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ended = run_at(&owner, "live", 1_000).await;
    owner.end_run(ended, 1_500, "shutdown").await.unwrap();
    let bare = run_at(&owner, "live", 2_000).await; // no premises: never listed
    let crashed = run_at(&owner, "live", 3_000).await; // no ended_ms
    let replay = run_at(&owner, "replay", 3_500).await;
    let running = run_at(&owner, "live", 4_000).await;
    for r in [ended, crashed, replay, running] {
        premises(&owner, r).await;
    }
    let analyze = rw(&db).await;
    let runs = analyze.premise_runs().await.unwrap();
    let got: Vec<(i64, i64, Option<i64>)> = runs
        .iter()
        .map(|r| (r.run_id, r.started_ms, r.end_ms))
        .collect();
    assert_eq!(
        got,
        vec![
            (ended, 1_000, Some(1_500)),
            (crashed, 3_000, Some(4_000)),
            (running, 4_000, None)
        ],
        "a crashed run ends where the next scanner session starts; bare {bare} and replay {replay} never count"
    );
    assert_eq!(
        runs[0].premises,
        vec![("fees".to_string(), "1+e3b0c442".to_string())]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn changed_spans_page_by_updated_ms_then_id_and_skip_replays() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = run_at(&owner, "live", 1).await;
    let mut replay = ep("replay-1", run, "no_basket", 10, None);
    replay.source = "replay".into();
    write(
        &owner,
        vec![
            ep("a", run, "no_basket", 10, Some(20)),
            ep("b", run, "yes_basket", 30, None),
            replay,
        ],
    )
    .await;
    let analyze = rw(&db).await;
    let all = analyze.changed_spans(0, "", 10).await.unwrap();
    assert_eq!(all.len(), 2);
    let ids: Vec<&str> = all.iter().map(|s| s.episode_id.as_str()).collect();
    assert!(ids.contains(&"a") && ids.contains(&"b"));
    let first = analyze.changed_spans(0, "", 1).await.unwrap();
    let rest = analyze
        .changed_spans(first[0].updated_ms, &first[0].episode_id, 10)
        .await
        .unwrap();
    assert_eq!(
        first.len() + rest.len(),
        2,
        "keyset pages never repeat a row"
    );
    let max = analyze.max_episode_updated_ms().await.unwrap().unwrap();
    assert_eq!(max, all.iter().map(|s| s.updated_ms).max().unwrap());
    analyze.close().await;
    db.drop(owner).await;
}

// F5 (controller ruling): max_episode_updated_ms must filter source = 'live', so a replay row
// (written strictly after, so its own updated_ms is provably later) never advances the cursor.
#[tokio::test]
async fn max_episode_updated_ms_never_advances_on_a_replay_row() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = run_at(&owner, "live", 1).await;
    write(&owner, vec![ep("a", run, "no_basket", 10, Some(20))]).await;
    let analyze = rw(&db).await;
    let live_only = analyze.max_episode_updated_ms().await.unwrap().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let mut replay = ep("replay-1", run, "no_basket", 10, None);
    replay.source = "replay".into();
    write(&owner, vec![replay]).await;
    let replay_updated_ms = owner.episode_updated_ms("replay-1").await.unwrap().unwrap();
    assert!(
        replay_updated_ms > live_only,
        "the replay row's own updated_ms must be strictly later, or this test proves nothing"
    );
    assert_eq!(
        analyze.max_episode_updated_ms().await.unwrap().unwrap(),
        live_only,
        "a later replay write must never move analyze's cursor forward"
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn card_episodes_return_hour_overlaps_with_the_previous_gap() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = run_at(&owner, "live", 1).await;
    let other = run_at(&owner, "live", 2).await;
    let mut imp = ep("imp", run, "implication", 1_600, Some(1_700));
    imp.event_id = "ev2".into();
    write(
        &owner,
        vec![
            ep("a", run, "no_basket", 1_000, Some(2_000)),
            ep("b", run, "no_basket", 2_200, Some(2_400)), // same event and kind: gap 200 ms
            ep("c", run, "yes_basket", 5_000, None),       // still open
            ep("old", run, "no_basket", 100, Some(1_400)), // closed before the range
            ep("late", run, "no_basket", 6_000, Some(6_100)), // opens at the range end
            ep("x", other, "no_basket", 1_500, Some(1_600)), // run not asked for
            imp,
        ],
    )
    .await;
    let analyze = rw(&db).await;
    let got = analyze.card_episodes(&[run], 1_500, 6_000).await.unwrap();
    let ids: Vec<&str> = got.iter().map(|e| e.episode_id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert_eq!(got[0].prev_gap_ms, None);
    assert_eq!(got[1].prev_gap_ms, Some(200));
    assert_eq!(
        (got[2].closed_ms, got[2].kind.as_str()),
        (None, "yes_basket")
    );
    assert_eq!(
        (got[0].pnl_net, got[0].budget100, got[0].cost),
        (dec!(0.41), Some(dec!(0.33)), dec!(99.59))
    );
    assert!(
        analyze
            .card_episodes(&[], 0, 10_000)
            .await
            .unwrap()
            .is_empty()
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn health_latency_and_marks_read_the_edge_inputs() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = run_at(&owner, "live", 1).await;
    let h = HealthRow {
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
        lag_p50_ms: Some(80),
        lag_p99_ms: None,
        clock_offset_ms: None,
        ok: None,
        capture_paused: false,
        parked_shards: 0,
        dropped_groups: 0,
        rest_rtt_p50_ms: Some(130),
        rest_rtt_p99_ms: Some(300),
    };
    let h2 = HealthRow {
        minute_ms: 120_000,
        rest_rtt_p50_ms: None,
        ..h.clone()
    };
    let acct =
        |id: &str, policy: &str, scenario: &str, fee: rust_decimal::Decimal| PaperAccountRow {
            run_id: run,
            account_id: id.into(),
            policy: policy.into(),
            scenario: scenario.into(),
            fee_bips: fee,
            bankroll: dec!(1000),
            per_trade: dec!(100),
            policies_sha256: "ab".repeat(32),
        };
    let mark = |id: &str, t: i64, eq: rust_decimal::Decimal| PaperMarkRow {
        run_id: run,
        account_id: id.into(),
        t_ms: t,
        cash: eq,
        equity: eq,
        realized: dec!(0),
        unrealized: dec!(0),
        fees_paid: dec!(0),
        locked: dec!(0),
        max_drawdown: dec!(0),
        n_positions: 0,
    };
    owner
        .write_batch(&Batch {
            health: vec![h, h2],
            paper_accounts: vec![
                acct("P0-converter/sp-idle", "P0-converter", "sp-idle", dec!(0)),
                acct(
                    "P0-converter/sp-idle/fee10",
                    "P0-converter",
                    "sp-idle",
                    dec!(10),
                ),
                acct("P3-naive/sp-idle", "P3-naive", "sp-idle", dec!(0)),
            ],
            paper_marks: vec![
                mark("P0-converter/sp-idle", 1_000, dec!(1000)),
                mark("P0-converter/sp-idle", 2_000, dec!(1000.5)),
                mark("P0-converter/sp-idle", 5_000, dec!(1001.25)),
                mark("P0-converter/sp-idle", 9_000, dec!(1002)),
            ],
            ..Batch::default()
        })
        .await
        .unwrap();
    let analyze = rw(&db).await;
    let lat = analyze.health_latency(&[run], 0, 180_000).await.unwrap();
    assert_eq!(
        lat.iter()
            .map(|m| (m.minute_ms, m.lag_p50_ms, m.rest_rtt_p50_ms))
            .collect::<Vec<_>>(),
        vec![(60_000, Some(80), Some(130)), (120_000, Some(80), None)]
    );
    assert_eq!(
        analyze.p0_accounts().await.unwrap(),
        vec![("P0-converter/sp-idle".to_string(), "sp-idle".to_string())],
        "P0 at the default fee only"
    );
    let marks = analyze
        .marks_with_prev(&["P0-converter/sp-idle".to_string()], 4_000, 9_000)
        .await
        .unwrap();
    assert_eq!(
        marks.iter().map(|m| (m.t_ms, m.equity)).collect::<Vec<_>>(),
        vec![(2_000, dec!(1000.5)), (5_000, dec!(1001.25))],
        "the last mark before the window, then the marks inside [from, to)"
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn card_cells_results_and_meta_round_trip_and_the_api_reads_them() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let analyze = rw(&db).await;
    let cell = |h: i64, data: &str| CardCellRow {
        card: "g1".into(),
        premise_key: "k1".into(),
        hour_ms: h,
        data: data.into(),
        computed_ms: 1,
    };
    analyze
        .card_cells_put(&[
            cell(0, r#"{"a":1}"#),
            cell(H, r#"{"a":2}"#),
            cell(2 * H, r#"{"a":3}"#),
        ])
        .await
        .unwrap();
    analyze
        .card_cells_put(&[cell(H, r#"{"a":22}"#)])
        .await
        .unwrap();
    let range = analyze
        .card_cells_range("g1", "k1", 0, 2 * H)
        .await
        .unwrap();
    assert_eq!(
        range,
        vec![
            (0, r#"{"a": 1}"#.to_string()),
            (H, r#"{"a": 22}"#.to_string())
        ]
    );
    let at = analyze
        .card_cells_at("g1", "k1", &[2 * H, 5 * H])
        .await
        .unwrap();
    assert_eq!(at, vec![(2 * H, r#"{"a": 3}"#.to_string())]);
    assert!(
        analyze
            .card_cells_range("g1", "other", 0, 10 * H)
            .await
            .unwrap()
            .is_empty()
    );
    let body = r#"{"card":"G1","columns":[{"ours":"0","their":null}]}"#;
    analyze
        .card_results_put(
            &[CardResultRow {
                card: "G1".into(),
                win: "7d".into(),
                premise_key: "k1".into(),
                status: "thin".into(),
                body: body.into(),
                computed_ms: 5,
            }],
            &[("tick".to_string(), r#"{"last_tick_ms":5}"#.to_string())],
        )
        .await
        .unwrap();
    analyze
        .card_meta_put("cursor", r#"{"updated_ms":7}"#)
        .await
        .unwrap();
    assert_eq!(
        analyze.card_meta_get("cursor").await.unwrap().as_deref(),
        Some(r#"{"updated_ms": 7}"#)
    );
    assert_eq!(analyze.card_meta_get("nope").await.unwrap(), None);
    let api = Store::connect_read_only(db.opts.clone(), 1, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let rows = api.card_results_for("7d").await.unwrap();
    assert_eq!(rows.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
    assert_eq!(v["columns"][0]["ours"], "0");
    assert!(v["columns"][0]["their"].is_null());
    assert!(api.card_results_for("24h").await.unwrap().is_empty());
    let meta = api.card_meta_all().await.unwrap();
    assert_eq!(
        meta.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(),
        ["cursor", "tick"]
    );
    api.close().await;
    analyze.close().await;
    db.drop(owner).await;
}
