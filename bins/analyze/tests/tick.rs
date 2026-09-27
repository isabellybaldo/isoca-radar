#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use analyze::tick::Analyzer;
use cards::body::ScenarioLat;
use rust_decimal_macros::dec;
use serde_json::Value;
use std::time::{Duration, Instant};
use store::*;

const H: i64 = 3_600_000;
const M: i64 = 60_000;
/// An hour boundary (497,220 h after the epoch).
const T0: i64 = 1_789_992_000_000;

fn scenarios() -> Vec<ScenarioLat> {
    analyze::scenarios::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/policies.toml"
    ))
    .unwrap()
}

fn premises(run_id: i64, episode_def: &str) -> Vec<RunPremiseRow> {
    [
        ("censoring", "1+c0"),
        ("depth", "1+e3b0c442"),
        ("detector_no", "1+e3b0c442"),
        ("detector_yes", "1+e3b0c442"),
        ("episode_def", episode_def),
        ("fees", "1+e3b0c442"),
        ("host", "1:vps-kvm2|100-250"),
        ("sim", "1+bb"),
    ]
    .iter()
    .map(|(a, v)| RunPremiseRow {
        run_id,
        area: a.to_string(),
        version: v.to_string(),
    })
    .collect()
}

fn ep(id: &str, run_id: i64, kind: &str, opened: i64, closed: Option<i64>) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(),
        run_id,
        source: "live".into(),
        kind: kind.into(),
        realization: "converter".into(),
        event_id: format!("ev-{id}"),
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

async fn run(store: &Store, started_ms: i64) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms,
            version: "t".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap()
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

async fn body(store: &Store, win: &str, card: &str) -> Value {
    let rows = store.card_results_for(win).await.unwrap();
    let (_, b) = rows
        .iter()
        .find(|(c, _)| c == card)
        .unwrap_or_else(|| panic!("no {card} for {win}: {rows:?}"));
    serde_json::from_str(b).unwrap()
}

fn cell(b: &Value, label: &str, col: &str) -> Value {
    b["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == label)
        .unwrap_or_else(|| panic!("no row {label}"))[col]
        .clone()
}

#[tokio::test]
async fn a_tick_writes_every_card_and_window_with_statuses_and_meta() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    let mut rows = Vec::new();
    for i in 0..12 {
        rows.push(ep(
            &format!("n{i:02}"),
            r,
            "no_basket",
            T0 - 2 * H + i * M,
            Some(T0 - 2 * H + i * M + 4_000),
        ));
    }
    for i in 0..3 {
        rows.push(ep(
            &format!("y{i}"),
            r,
            "yes_basket",
            T0 - H + i * M,
            Some(T0 - H + i * M + 16_000),
        ));
    }
    write(&owner, rows).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.write_static_meta().await.unwrap();
    let rep = a.tick(T0 + 30 * M).await.unwrap();
    assert_eq!(
        rep.results,
        4 * 4 + 5 + 4,
        "G1–G4 × 4 windows + EDGE × 5 windows + SWEET × 4 windows"
    );
    let g1 = body(&rw, "7d", "G1").await;
    assert_eq!(cell(&g1, "NO-side episodes", "ours"), "12");
    assert_eq!(cell(&g1, "YES-side episodes", "their"), "3");
    assert_eq!(cell(&g1, "YES-side episodes", "paper"), "2098");
    assert_eq!(
        g1["status"], "warming",
        "data since T0 − 3 h starts inside 7d"
    );
    assert!(
        g1["premise_key"]
            .as_str()
            .unwrap()
            .contains("episode_def=1+aa")
    );
    assert_eq!(g1["hash"].as_str().unwrap().len(), 64);
    let all = body(&rw, "all", "G1").await;
    assert_eq!(
        all["status"], "thin",
        "all is never warming; YES n = 3 < 10 is thin"
    );
    let today: Vec<String> = rw
        .card_results_for("today")
        .await
        .unwrap()
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(today, ["EDGE"]);
    let edge = body(&rw, "24h", "EDGE").await;
    assert_eq!(cell(&edge, "edge on the table", "ours"), "4.95");
    let meta: Vec<String> = rw
        .card_meta_all()
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.0)
        .collect();
    for k in [
        "cursor",
        "fam:dur",
        "fam:edge",
        "fam:g1",
        "fam:g4",
        "last_error",
        "not_replicated",
        "papers",
        "tick",
    ] {
        assert!(meta.contains(&k.to_string()), "{k} in {meta:?}");
    }
    let fam: Value =
        serde_json::from_str(&rw.card_meta_get("fam:g1").await.unwrap().unwrap()).unwrap();
    assert_eq!(
        (fam["done"].as_bool(), fam["next_hour"].as_i64()),
        (Some(true), Some(T0))
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_late_close_outside_the_recent_48h_rebuilds_the_old_hour_without_double_counting() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 70 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    let opened = T0 - 60 * H; // outside the 48 h in-memory cache, inside 7d
    write(
        &owner,
        vec![
            ep("long", r, "no_basket", opened, None),
            ep(
                "short",
                r,
                "no_basket",
                opened + M,
                Some(opened + M + 2_000),
            ),
        ],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0).await.unwrap();
    let g3 = body(&rw, "7d", "G3").await;
    assert_eq!(
        cell(&g3, "episodes observed (per hour)", "their"),
        "61",
        "open: observed in every hour to now"
    );
    assert_eq!(
        cell(&body(&rw, "7d", "G2").await, "NO durations (n)", "ours"),
        "1"
    );
    // the late close: two hours long
    write(
        &owner,
        vec![ep("long", r, "no_basket", opened, Some(opened + 2 * H))],
    )
    .await;
    a.tick(T0 + 15 * M).await.unwrap();
    // ruling T9 (c): 30d and all re-render once per clock hour, one per tick
    a.tick(T0 + H + 5 * M).await.unwrap();
    a.tick(T0 + H + 20 * M).await.unwrap();
    for win in ["7d", "30d", "all"] {
        let g3 = body(&rw, win, "G3").await;
        assert_eq!(
            cell(&g3, "episodes observed (per hour)", "their"),
            "3",
            "{win}: 2 hours of long + short"
        );
        let g2 = body(&rw, win, "G2").await;
        assert_eq!(
            cell(&g2, "NO durations (n)", "ours"),
            "2",
            "{win}: counted once, as a finished duration"
        );
    }
    let key = body(&rw, "7d", "G2").await["premise_key"]
        .as_str()
        .unwrap()
        .to_string();
    let stored = rw.card_cells_at("dur", &key, &[opened]).await.unwrap();
    assert!(
        stored[0].1.contains("7200000"),
        "the opening hour's cell has the 2 h duration: {}",
        stored[0].1
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_premise_bump_mid_window_never_mixes_keys() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 50 * H).await;
    owner
        .insert_run_premises(&premises(r1, "1+aa"))
        .await
        .unwrap();
    owner.end_run(r1, T0 - 10 * H, "shutdown").await.unwrap();
    write(
        &owner,
        (0..5)
            .map(|i| {
                ep(
                    &format!("old{i}"),
                    r1,
                    "no_basket",
                    T0 - 20 * H + i * M,
                    Some(T0 - 20 * H + i * M + 1_000),
                )
            })
            .collect(),
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0 - 6 * H).await.unwrap();
    let old_key = body(&rw, "7d", "G1").await["premise_key"]
        .as_str()
        .unwrap()
        .to_string();
    let old_cells = rw
        .card_cells_range("g1", &old_key, 0, i64::MAX)
        .await
        .unwrap();
    assert!(!old_cells.is_empty());
    // a new run under a bumped episode_def
    let r2 = run(&owner, T0 - 5 * H).await;
    owner
        .insert_run_premises(&premises(r2, "2+aa"))
        .await
        .unwrap();
    write(
        &owner,
        (0..3)
            .map(|i| {
                ep(
                    &format!("new{i}"),
                    r2,
                    "no_basket",
                    T0 - 4 * H + i * M,
                    Some(T0 - 4 * H + i * M + 1_000),
                )
            })
            .collect(),
    )
    .await;
    a.tick(T0).await.unwrap();
    for win in ["24h", "7d", "all"] {
        let g1 = body(&rw, win, "G1").await;
        assert!(
            g1["premise_key"]
                .as_str()
                .unwrap()
                .contains("episode_def=2+aa"),
            "{win}"
        );
        assert_eq!(
            cell(&g1, "NO-side episodes", "ours"),
            "3",
            "{win}: only the new key's run"
        );
        assert_eq!(g1["since_ms"].as_i64(), Some(T0 - 5 * H));
    }
    assert_eq!(body(&rw, "7d", "G1").await["status"], "warming");
    assert_eq!(
        rw.card_cells_range("g1", &old_key, 0, i64::MAX)
            .await
            .unwrap(),
        old_cells,
        "old cells untouched"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn runs_without_premises_are_never_counted() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 5 * H).await;
    owner
        .insert_run_premises(&premises(r1, "1+aa"))
        .await
        .unwrap();
    owner.end_run(r1, T0 - 3 * H, "shutdown").await.unwrap();
    let r2 = run(&owner, T0 - 2 * H).await; // a mixed-version run that wrote no premises
    write(
        &owner,
        vec![
            ep("a", r1, "no_basket", T0 - 4 * H, Some(T0 - 4 * H + 1_000)),
            ep(
                "b",
                r1,
                "no_basket",
                T0 - 4 * H + M,
                Some(T0 - 4 * H + M + 1_000),
            ),
            ep("c", r2, "no_basket", T0 - H, Some(T0 - H + 1_000)),
            ep("d", r2, "no_basket", T0 - H + M, Some(T0 - H + M + 1_000)),
        ],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0).await.unwrap();
    assert_eq!(
        cell(&body(&rw, "all", "G1").await, "NO-side episodes", "ours"),
        "2"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn catch_up_builds_history_in_24_hour_batches_and_pauses_between_them() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 60 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        (0..60)
            .map(|i| {
                ep(
                    &format!("e{i:02}"),
                    r,
                    "yes_basket",
                    T0 - 60 * H + i * H,
                    Some(T0 - 60 * H + i * H + 5_000),
                )
            })
            .collect(),
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::from_millis(30), "test", scenarios())
        .await
        .unwrap();
    let t = Instant::now();
    let rep = a.tick(T0).await.unwrap();
    assert_eq!(
        rep.catch_up_hours,
        5 * 61,
        "61 hours (T0 − 60 h … T0) for each of the 5 families"
    );
    // 61 hours = batches of 24 + 24 + 13: 2 pauses per family
    assert!(
        t.elapsed() >= Duration::from_millis(5 * 2 * 30),
        "{:?}",
        t.elapsed()
    );
    let g1 = body(&rw, "7d", "G1").await;
    assert_eq!(cell(&g1, "YES-side episodes", "ours"), "60");
    assert_ne!(
        g1["status"], "under_review",
        "catch-up finished inside the tick"
    );
    let rep2 = a.tick(T0 + 20 * M).await.unwrap();
    assert_eq!(rep2.catch_up_hours, 0, "a normal tick after catch-up");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn the_startup_check_refuses_a_role_that_can_write_elsewhere() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    assert!(analyze::ensure_writes_only_cards(&owner).await.is_err());
    let rw = rw(&db).await;
    analyze::ensure_writes_only_cards(&rw).await.unwrap();
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_failed_tick_records_the_error_and_keeps_the_last_results() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 2 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep("a", r, "no_basket", T0 - H, Some(T0 - H + 1_000))],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0).await.unwrap();
    owner
        .raw_exec("ALTER TABLE card_results RENAME TO card_results_away")
        .await
        .unwrap();
    let err = a.tick(T0 + 15 * M).await.unwrap_err();
    a.record_error(T0 + 15 * M, &format!("{err:#}")).await;
    a.reset();
    owner
        .raw_exec("ALTER TABLE card_results_away RENAME TO card_results")
        .await
        .unwrap();
    let e: Value =
        serde_json::from_str(&rw.card_meta_get("last_error").await.unwrap().unwrap()).unwrap();
    assert_eq!(e["ms"].as_i64(), Some(T0 + 15 * M));
    assert!(e["text"].as_str().unwrap().contains("card_results"), "{e}");
    assert_eq!(
        rw.card_results_for("7d").await.unwrap().len(),
        6,
        "the last results stay (G1–G4, EDGE and SWEET)"
    );
    a.tick(T0 + 30 * M).await.unwrap();
    assert_eq!(
        rw.card_meta_get("last_error").await.unwrap().as_deref(),
        Some("null")
    );
    assert_eq!(
        cell(&body(&rw, "7d", "G1").await, "NO-side episodes", "ours"),
        "1",
        "reloaded from card_cells, not doubled"
    );
    rw.close().await;
    db.drop(owner).await;
}

/// Fails any write of an under_review result, so a test can prove none happened mid-tick.
const NO_UNDER_REVIEW: &str = "CREATE FUNCTION no_under_review() RETURNS trigger LANGUAGE plpgsql AS $$ \
     BEGIN IF NEW.status = 'under_review' THEN RAISE EXCEPTION 'under_review written for %', NEW.card; END IF; \
     RETURN NEW; END $$; \
     CREATE TRIGGER no_under_review BEFORE INSERT OR UPDATE ON card_results \
     FOR EACH ROW EXECUTE FUNCTION no_under_review()";

#[tokio::test]
async fn a_restart_under_the_same_key_is_never_under_review_but_a_key_change_is() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r,
            "no_basket",
            T0 - 2 * H,
            Some(T0 - 2 * H + 1_000),
        )],
    )
    .await;
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0)
        .await
        .unwrap();
    owner.raw_exec(NO_UNDER_REVIEW).await.unwrap();
    // a restart two hours later: the gap is rebuilt, but the key did not change (F17)
    let mut b = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    let rep = b.tick(T0 + 2 * H).await.unwrap();
    assert!(rep.catch_up_hours > 0, "the downtime hours are rebuilt");
    assert_eq!(
        cell(&body(&rw, "7d", "G1").await, "NO-side episodes", "ours"),
        "1",
        "loaded, not doubled"
    );
    // a premise bump: under_review while the new key catches up
    let r2 = run(&owner, T0 + 2 * H).await;
    owner
        .insert_run_premises(&premises(r2, "2+aa"))
        .await
        .unwrap();
    let err = b.tick(T0 + 3 * H).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("under_review written"),
        "{err:#}"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_clock_that_went_back_before_the_key_began_never_panics() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 + 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r,
            "no_basket",
            T0 + 3 * H + M,
            Some(T0 + 3 * H + M + 1_000),
        )],
    )
    .await;
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0 + 4 * H)
        .await
        .unwrap();
    // restart with a clock 4 h behind: the key's first run starts after "now" (F10: no clamp panic)
    let mut b = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    b.tick(T0).await.unwrap();
    rw.close().await;
    db.drop(owner).await;
}

fn mark(run_id: i64, t_ms: i64, equity: rust_decimal::Decimal) -> PaperMarkRow {
    let z = dec!(0);
    PaperMarkRow {
        run_id,
        account_id: "p0-sp-idle".into(),
        t_ms,
        cash: equity,
        equity,
        realized: z,
        unrealized: z,
        fees_paid: z,
        locked: z,
        max_drawdown: z,
        n_positions: 0,
    }
}

#[tokio::test]
async fn equity_deltas_start_from_the_bankroll_for_an_account_begun_inside_the_window() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r,
            "no_basket",
            T0 - 2 * H,
            Some(T0 - 2 * H + 1_000),
        )],
    )
    .await;
    let account = PaperAccountRow {
        run_id: r,
        account_id: "p0-sp-idle".into(),
        policy: "P0-converter".into(),
        scenario: "sp-idle".into(),
        fee_bips: dec!(0),
        bankroll: dec!(1000),
        per_trade: dec!(100),
        policies_sha256: "x".into(),
    };
    let marks = vec![
        mark(r, T0 - H + 10 * M, dec!(1001)),
        mark(r, T0 - H + 20 * M, dec!(1003)),
    ];
    owner
        .write_batch(&Batch {
            paper_accounts: vec![account],
            paper_marks: marks,
            ..Batch::default()
        })
        .await
        .unwrap();
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0 + 30 * M)
        .await
        .unwrap();
    for win in ["24h", "all"] {
        let e = body(&rw, win, "EDGE").await;
        assert_eq!(
            cell(&e, "captured at sp-idle (145 ms)", "ours"),
            "3",
            "{win}: +1 vs the bankroll, then +2"
        );
    }
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_late_host_row_brings_an_older_run_into_edge_capture_once() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 10 * H).await;
    let no_host: Vec<RunPremiseRow> = premises(r1, "1+aa")
        .into_iter()
        .filter(|p| p.area != "host")
        .collect();
    owner.insert_run_premises(&no_host).await.unwrap();
    owner.end_run(r1, T0 - 6 * H, "shutdown").await.unwrap();
    let r2 = run(&owner, T0 - 5 * H).await;
    owner
        .insert_run_premises(&premises(r2, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![
            ep("a", r1, "no_basket", T0 - 8 * H, Some(T0 - 8 * H + 1_000)),
            ep(
                "b",
                r1,
                "no_basket",
                T0 - 8 * H + M,
                Some(T0 - 8 * H + M + 1_000),
            ),
            ep("c", r2, "no_basket", T0 - 4 * H, Some(T0 - 4 * H + 1_000)),
        ],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0).await.unwrap();
    assert_eq!(
        cell(&body(&rw, "all", "EDGE").await, "edge on the table", "ours"),
        "0.33",
        "run 1 has no host yet"
    );
    assert_eq!(
        cell(&body(&rw, "all", "G1").await, "NO-side episodes", "ours"),
        "3",
        "G1 needs no host"
    );
    owner
        .insert_run_premises(
            &premises(r1, "1+aa")
                .into_iter()
                .filter(|p| p.area == "host")
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    a.tick(T0 + 15 * M).await.unwrap();
    let e = body(&rw, "all", "EDGE").await;
    assert_eq!(
        cell(&e, "edge on the table", "ours"),
        "0.99",
        "run 1 now counts, once"
    );
    assert_eq!(e["since_ms"].as_i64(), Some(T0 - 10 * H));
    a.tick(T0 + 30 * M).await.unwrap();
    assert_eq!(
        cell(&body(&rw, "all", "EDGE").await, "edge on the table", "ours"),
        "0.99",
        "still once"
    );
    assert_eq!(
        cell(&body(&rw, "all", "G1").await, "NO-side episodes", "ours"),
        "3"
    );
    rw.close().await;
    db.drop(owner).await;
}

fn computed(b: &Value) -> i64 {
    b["computed_ms"].as_i64().unwrap()
}

#[tokio::test]
async fn long_windows_re_render_at_most_hourly_and_keep_their_own_computed_ms() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r,
            "no_basket",
            T0 - 2 * H,
            Some(T0 - 2 * H + 1_000),
        )],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0).await.unwrap();
    for win in ["24h", "7d", "30d", "all"] {
        assert_eq!(
            computed(&body(&rw, win, "G2").await),
            T0,
            "{win}: the first tick renders every window"
        );
    }
    write(
        &owner,
        vec![ep("b", r, "no_basket", T0 + M, Some(T0 + M + 1_000))],
    )
    .await;
    let rep = a.tick(T0 + 15 * M).await.unwrap();
    assert_eq!(
        rep.results,
        2 * 6 + 1,
        "24h + 7d for G1–G4, EDGE and SWEET, and EDGE's today"
    );
    assert_eq!(
        cell(&body(&rw, "7d", "G1").await, "NO-side episodes", "ours"),
        "2"
    );
    assert_eq!(computed(&body(&rw, "7d", "G1").await), T0 + 15 * M);
    let (d30, all) = (body(&rw, "30d", "G1").await, body(&rw, "all", "G1").await);
    assert_eq!(
        (computed(&d30), computed(&all)),
        (T0, T0),
        "same hour: 30d and all keep their last render"
    );
    assert_eq!(
        cell(&d30, "NO-side episodes", "ours"),
        "1",
        "and their numbers, honestly dated"
    );
    a.tick(T0 + H + 5 * M).await.unwrap();
    assert_eq!(
        computed(&body(&rw, "30d", "G1").await),
        T0 + H + 5 * M,
        "a new hour: 30d"
    );
    assert_eq!(
        computed(&body(&rw, "all", "G1").await),
        T0,
        "one long window per tick"
    );
    a.tick(T0 + H + 20 * M).await.unwrap();
    let all = body(&rw, "all", "G1").await;
    assert_eq!(computed(&all), T0 + H + 20 * M, "the next tick: all");
    assert_eq!(cell(&all, "NO-side episodes", "ours"), "2");
    assert_eq!(
        cell(&body(&rw, "30d", "G1").await, "NO-side episodes", "ours"),
        "2"
    );
    rw.close().await;
    db.drop(owner).await;
}

/// `premises` with a chosen `sim` version (it embeds the policies sha256) and optionally no host.
fn premises_sim(run_id: i64, sim: &str, host: bool) -> Vec<RunPremiseRow> {
    premises(run_id, "1+aa")
        .into_iter()
        .filter(|p| host || p.area != "host")
        .map(|mut p| {
            if p.area == "sim" {
                p.version = sim.to_string();
            }
            p
        })
        .collect()
}

fn p0(run_id: i64, bankroll: rust_decimal::Decimal) -> PaperAccountRow {
    PaperAccountRow {
        run_id,
        account_id: "p0-sp-idle".into(),
        policy: "P0-converter".into(),
        scenario: "sp-idle".into(),
        fee_bips: dec!(0),
        bankroll,
        per_trade: dec!(100),
        policies_sha256: "x".into(),
    }
}

async fn paper(owner: &Store, accounts: Vec<PaperAccountRow>, marks: Vec<PaperMarkRow>) {
    owner
        .write_batch(&Batch {
            paper_accounts: accounts,
            paper_marks: marks,
            ..Batch::default()
        })
        .await
        .unwrap();
}

const CAPTURED: &str = "captured at sp-idle (145 ms)";

#[tokio::test]
async fn a_mark_that_lands_just_after_its_hour_closed_still_counts() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+aa"))
        .await
        .unwrap();
    // no episodes: the episode cursor's own lookback must not be what re-dirties the hour
    paper(
        &owner,
        vec![p0(r, dec!(1000))],
        vec![mark(r, T0 - H + 10 * M, dec!(1001))],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap();
    a.tick(T0 + 30_000).await.unwrap(); // 30 s after the hour: its last rebuild
    assert_eq!(cell(&body(&rw, "24h", "EDGE").await, CAPTURED, "ours"), "1");
    // stamped 1 s before the hour closed, committed after that rebuild
    paper(&owner, vec![], vec![mark(r, T0 - 1_000, dec!(1004))]).await;
    a.tick(T0 + 15 * M).await.unwrap();
    assert_eq!(
        cell(&body(&rw, "24h", "EDGE").await, CAPTURED, "ours"),
        "4",
        "+1, then the late +3"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_newer_run_with_other_policies_never_lends_its_bankroll() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 5 * H).await;
    owner
        .insert_run_premises(&premises_sim(r1, "1+bb", true))
        .await
        .unwrap();
    owner.end_run(r1, T0 - 2 * H, "shutdown").await.unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r1,
            "no_basket",
            T0 - 4 * H,
            Some(T0 - 4 * H + 1_000),
        )],
    )
    .await;
    paper(
        &owner,
        vec![p0(r1, dec!(1000))],
        vec![
            mark(r1, T0 - 4 * H + 10 * M, dec!(1001)),
            mark(r1, T0 - 4 * H + 20 * M, dec!(1003)),
        ],
    )
    .await;
    // a newer run under other policies, no host row yet: Edge capture's key is still run 1's
    let r2 = run(&owner, T0 - H).await;
    owner
        .insert_run_premises(&premises_sim(r2, "1+cc", false))
        .await
        .unwrap();
    paper(&owner, vec![p0(r2, dec!(5000))], vec![]).await;
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0)
        .await
        .unwrap();
    let e = body(&rw, "all", "EDGE").await;
    assert!(e["premise_key"].as_str().unwrap().contains("sim=1+bb"));
    // run 1's bankroll is not the newest run's (5000): its first mark is skipped, never guessed
    assert_eq!(cell(&e, CAPTURED, "ours"), "2");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_fresh_start_under_new_policies_counts_from_the_bankroll() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 6 * H).await;
    owner
        .insert_run_premises(&premises_sim(r1, "1+bb", true))
        .await
        .unwrap();
    owner.end_run(r1, T0 - 4 * H, "shutdown").await.unwrap();
    paper(
        &owner,
        vec![p0(r1, dec!(1000))],
        vec![mark(r1, T0 - 5 * H, dec!(1200))],
    )
    .await;
    let r2 = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises_sim(r2, "1+cc", true))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r2,
            "no_basket",
            T0 - 2 * H,
            Some(T0 - 2 * H + 1_000),
        )],
    )
    .await;
    // new policies: every account starts fresh at the bankroll (scanner restore_trader)
    paper(
        &owner,
        vec![p0(r2, dec!(1000))],
        vec![
            mark(r2, T0 - 2 * H, dec!(1000.5)),
            mark(r2, T0 - 2 * H + 10 * M, dec!(1001.5)),
        ],
    )
    .await;
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0)
        .await
        .unwrap();
    let e = body(&rw, "all", "EDGE").await;
    assert!(e["premise_key"].as_str().unwrap().contains("sim=1+cc"));
    assert_eq!(
        cell(&e, CAPTURED, "ours"),
        "1.5",
        "+0.5 from the bankroll (not −199.5 from run 1's mark), then +1"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_sim_code_bump_under_the_same_policies_restores_and_telescopes() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = run(&owner, T0 - 6 * H).await;
    owner
        .insert_run_premises(&premises_sim(r1, "1+bb", true))
        .await
        .unwrap();
    owner.end_run(r1, T0 - 4 * H, "shutdown").await.unwrap();
    paper(
        &owner,
        vec![p0(r1, dec!(1000))],
        vec![mark(r1, T0 - 5 * H, dec!(1200))],
    )
    .await;
    // sim code 1 -> 2, same policies (+bb): scanner restore_trader restores every account
    let r2 = run(&owner, T0 - 3 * H).await;
    owner
        .insert_run_premises(&premises_sim(r2, "2+bb", true))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep(
            "a",
            r2,
            "no_basket",
            T0 - 2 * H,
            Some(T0 - 2 * H + 1_000),
        )],
    )
    .await;
    paper(
        &owner,
        vec![p0(r2, dec!(1000))],
        vec![
            mark(r2, T0 - 2 * H, dec!(1201)),
            mark(r2, T0 - 2 * H + 10 * M, dec!(1203)),
        ],
    )
    .await;
    let rw = rw(&db).await;
    Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
        .await
        .unwrap()
        .tick(T0)
        .await
        .unwrap();
    let e = body(&rw, "all", "EDGE").await;
    assert!(e["premise_key"].as_str().unwrap().contains("sim=2+bb"));
    assert_eq!(
        cell(&e, CAPTURED, "ours"),
        "3",
        "+1 from run 1's last mark (restored, not +201 from the bankroll), then +2"
    );
    rw.close().await;
    db.drop(owner).await;
}

const P0: &str = "P0-converter/sp-idle";

fn p0_account(run: i64) -> PaperAccountRow {
    PaperAccountRow {
        run_id: run,
        account_id: P0.into(),
        policy: "P0-converter".into(),
        scenario: "sp-idle".into(),
        fee_bips: dec!(0),
        bankroll: dec!(1000),
        per_trade: dec!(100),
        policies_sha256: "x".into(),
    }
}

/// One executed NO attempt on `episode` at `exec_ms` capturing `x` (two buys, one conversion).
fn no_attempt(run: i64, episode: &str, exec_ms: i64, x: rust_decimal::Decimal, seq: i64) -> Batch {
    let led = |seq: i64, kind: &str, leg: Option<&str>, qty, cash| PaperLedgerRow {
        run_id: run,
        account_id: P0.into(),
        seq,
        t_ms: exec_ms,
        kind: kind.into(),
        episode_id: Some(episode.into()),
        market_id: leg.map(|m| m.to_string()),
        yes_token: leg.map(|m| format!("{m}-y")),
        outcome: leg.map(|_| "no".to_string()),
        qty,
        price: None,
        cash_delta: cash,
        fees: dec!(0),
        realized_delta: dec!(0),
        cash_after: dec!(1000),
        equity: None,
    };
    Batch {
        paper_intents: vec![PaperIntentRow {
            run_id: run,
            account_id: P0.into(),
            episode_id: episode.into(),
            decided_ms: exec_ms - 300,
            exec_ms: Some(exec_ms),
            skip_reason: None,
            q_target: Some(dec!(10)),
            detected_q: None,
            detected_pnl: None,
            reserve: None,
        }],
        paper_execs: vec![PaperExecRow {
            run_id: run,
            account_id: P0.into(),
            episode_id: episode.into(),
            exec_ms,
            q_eff: dec!(10),
            leg_risk_qty: dec!(0),
            pnl_at_tx: None,
            hit: true,
        }],
        paper_ledger: vec![
            led(seq, "buy", Some("m0"), dec!(10), dec!(-4.00)),
            led(seq + 1, "buy", Some("m1"), dec!(10), dec!(-5.10)),
            led(seq + 2, "convert", None, dec!(10), dec!(9.10) + x),
        ],
        ..Batch::default()
    }
}

fn sweet_row(b: &Value, side: &str, scenario: &str, dim: &str, bucket: &str) -> Value {
    b["series"]["sweet"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["side"] == side
                && r["scenario"] == scenario
                && r["dimension"] == dim
                && r["bucket"] == bucket
                && r["fee_bips"] == 0
        })
        .cloned()
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn an_attempt_in_flight_at_one_tick_is_counted_once_at_the_next() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 2 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+ab"))
        .await
        .unwrap();
    // opened 10 s before the hour, closed 5 s later: its update is seen before the attempt lands
    let opened = T0 + H - 10_000;
    let mut e = ep("e1", r, "no_basket", opened, Some(opened + 5_000));
    e.days_to_end = Some(dec!(3));
    write(&owner, vec![e]).await;
    owner
        .write_batch(&Batch {
            paper_accounts: vec![p0_account(r)],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .write_batch(&no_attempt(r, "e1", T0 + H + 30_000, dec!(0.5), 1))
        .await
        .unwrap();
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap();
    a.tick(T0 + H + 45_000).await.unwrap(); // 15 s after exec: in flight
    let row = sweet_row(
        &body(&rw, "24h", "SWEET").await,
        "no",
        "sp-idle",
        "all",
        "all",
    );
    assert_eq!(
        (row["episodes"].as_i64(), row["n_attempts"].as_i64()),
        (Some(1), Some(0))
    );
    a.tick(T0 + H + 15 * M).await.unwrap(); // the late rule rebuilds T0 (hour of the episode)
    let row = sweet_row(
        &body(&rw, "24h", "SWEET").await,
        "no",
        "sp-idle",
        "all",
        "all",
    );
    assert_eq!(
        (
            row["n_attempts"].as_i64(),
            row["n_exec"].as_i64(),
            row["captured"].as_str()
        ),
        (Some(1), Some(1), Some("0.5"))
    );
    a.tick(T0 + H + 30 * M).await.unwrap();
    let row = sweet_row(
        &body(&rw, "24h", "SWEET").await,
        "no",
        "sp-idle",
        "all",
        "all",
    );
    assert_eq!(row["n_exec"].as_i64(), Some(1), "counted once");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn the_sweet_suffix_leaves_other_family_keys_unchanged() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 2 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+ab"))
        .await
        .unwrap();
    write(
        &owner,
        vec![ep("e1", r, "no_basket", T0 + M, Some(T0 + 2 * M))],
    )
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap();
    a.tick(T0 + H).await.unwrap();
    let edge = body(&rw, "7d", "EDGE").await["premise_key"]
        .as_str()
        .unwrap()
        .to_string();
    for card in ["G1", "G2", "G3", "G4", "EDGE"] {
        let k = body(&rw, "7d", card).await["premise_key"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!k.contains(";sweet="), "{card}: {k}");
    }
    let sweet = body(&rw, "7d", "SWEET").await["premise_key"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(sweet, format!("{edge}{}", cards::sweet::key_suffix()));
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn thirty_events_with_the_same_edge_make_the_no_row_a_candidate() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 2 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+ab"))
        .await
        .unwrap();
    owner
        .write_batch(&Batch {
            paper_accounts: vec![p0_account(r)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let mut eps = Vec::new();
    for i in 0..30 {
        let id = format!("e{i}");
        eps.push(ep(
            &id,
            r,
            "no_basket",
            T0 + i * M,
            Some(T0 + i * M + 1_000),
        )); // event ev-e{i}: 30 clusters
        owner
            .write_batch(&no_attempt(
                r,
                &id,
                T0 + i * M + 500,
                dec!(0.10),
                10 * i + 1,
            ))
            .await
            .unwrap();
    }
    write(&owner, eps).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap()
        .with_hurdle(Some(dec!(0.10)));
    a.tick(T0 + 2 * H).await.unwrap();
    let b = body(&rw, "24h", "SWEET").await;
    let row = sweet_row(&b, "no", "sp-idle", "all", "all");
    assert_eq!(
        (
            row["label"].as_str(),
            row["n_exec"].as_i64(),
            row["clusters"].as_i64()
        ),
        (Some("candidate"), Some(30), Some(30))
    );
    assert!(b["series"]["sweet"]["m"].as_i64().unwrap() >= 1);
    assert_eq!(b["series"]["sweet"]["hurdle"].as_str(), Some("0.1"));
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_restart_seeds_last_tick_ms_so_the_late_rule_still_reaches_the_episodes_hour() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r = run(&owner, T0 - 2 * H).await;
    owner
        .insert_run_premises(&premises(r, "1+ab"))
        .await
        .unwrap();
    let opened = T0 + H - 10_000;
    let mut e = ep("e1", r, "no_basket", opened, Some(opened + 5_000));
    e.days_to_end = Some(dec!(3));
    write(&owner, vec![e]).await;
    owner
        .write_batch(&Batch {
            paper_accounts: vec![p0_account(r)],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .write_batch(&no_attempt(r, "e1", T0 + H + 30_000, dec!(0.5), 1))
        .await
        .unwrap();
    let rw1 = rw(&db).await;
    let mut a1 = Analyzer::new(rw1.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap();
    a1.tick(T0 + H + 45_000).await.unwrap(); // 15 s after exec: in flight, not yet counted
    let row = sweet_row(
        &body(&rw1, "24h", "SWEET").await,
        "no",
        "sp-idle",
        "all",
        "all",
    );
    assert_eq!(row["n_attempts"].as_i64(), Some(0), "still in flight");
    rw1.close().await;
    // Push the episode-changed cursor far into the future so a2's own 120 s relookback
    // (LOOKBACK_MS, unrelated to SWEET) can never re-surface "e1" on its own: whatever re-dirties
    // T0 from here has to be the SWEET late rule, not that cursor.
    owner
        .card_meta_put(
            "cursor",
            &serde_json::json!({ "updated_ms": 9_000_000_000_000_i64 }).to_string(),
        )
        .await
        .unwrap();
    // "restart": a fresh Analyzer on a fresh connection, no in-memory state left — only what
    // card_meta and card_cells hold. The next tick is far enough past T0 (the episode's hour)
    // that anchoring the late window on *this* process's own now_ms (the bug) would miss it, but
    // one seeded from the last real tick's `last_tick_ms` (T0 + H + 45_000, read back from
    // card_meta) still reaches it.
    let rw2 = rw(&db).await;
    let mut a2 = Analyzer::new(rw2.clone(), Duration::ZERO, "t", scenarios())
        .await
        .unwrap();
    a2.tick(T0 + 2 * H + 60_000).await.unwrap();
    let row = sweet_row(
        &body(&rw2, "24h", "SWEET").await,
        "no",
        "sp-idle",
        "all",
        "all",
    );
    assert_eq!(
        (
            row["n_attempts"].as_i64(),
            row["n_exec"].as_i64(),
            row["captured"].as_str()
        ),
        (Some(1), Some(1), Some("0.5")),
        "the late rule reached T0 across the restart"
    );
    rw2.close().await;
    db.drop(owner).await;
}
