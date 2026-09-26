#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

#[tokio::test]
async fn migrations_apply_from_empty_and_are_idempotent() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    // fresh() already migrated once; a second run must be a no-op.
    store.migrate().await.expect("second migrate");
    let tables: Vec<String> = sqlx_names(&store).await;
    for t in [
        "card_cells",
        "card_meta",
        "card_results",
        "episode_legs",
        "episodes",
        "events",
        "health",
        "integrity_events",
        "legs",
        "observations",
        "paper_accounts",
        "paper_execs",
        "paper_fills",
        "paper_intents",
        "paper_ledger",
        "paper_markouts",
        "paper_marks",
        "paper_state",
        "run_premises",
        "runs",
        "universe_snapshots",
    ] {
        assert!(
            tables.iter().any(|x| x == t),
            "missing table {t}: {tables:?}"
        );
    }
    db.drop(store).await;
}

async fn sqlx_names(store: &store::Store) -> Vec<String> {
    store.table_names().await.expect("table names")
}

const M0006: &str = include_str!("../migrations/0006_cards.sql");

/// The backfill statement of 0006, verbatim, so a test can re-run it over seeded runs.
fn backfill_sql() -> &'static str {
    let start = M0006
        .find("INSERT INTO run_premises")
        .expect("backfill start");
    let end = M0006[start..]
        .find("ON CONFLICT DO NOTHING;")
        .expect("backfill end")
        + start
        + "ON CONFLICT DO NOTHING;".len();
    &M0006[start..end]
}

async fn run_with(store: &store::Store, service: &str, mode: &str) -> i64 {
    store
        .start_run(&store::NewRun {
            service: service.into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .expect("run")
}

#[tokio::test]
async fn the_backfill_takes_live_scanner_runs_from_7_only_and_hashes_like_rust() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    // run ids 1..=10 in a fresh database
    for _ in 1..=6 {
        run_with(&store, "scanner", "live").await;
    }
    assert_eq!(run_with(&store, "scanner", "live").await, 7);
    assert_eq!(run_with(&store, "scanner", "replay").await, 8);
    assert_eq!(run_with(&store, "scanner", "safe").await, 9);
    assert_eq!(run_with(&store, "scanner", "live").await, 10);
    store.raw_exec(backfill_sql()).await.expect("backfill");
    store
        .raw_exec(backfill_sql())
        .await
        .expect("backfill is idempotent");
    for run in [1, 6, 8, 9] {
        assert!(
            store.run_premises_for(run).await.unwrap().is_empty(),
            "run {run}"
        );
    }
    for run in [7, 10] {
        let rows = store.run_premises_for(run).await.unwrap();
        let areas: Vec<&str> = rows.iter().map(|r| r.area.as_str()).collect();
        assert_eq!(
            areas,
            [
                "censoring",
                "depth",
                "detector_no",
                "detector_yes",
                "episode_def",
                "fees"
            ],
            "no sim/host backfill: those runs measured no REST round trip"
        );
        let censoring = &rows[0];
        assert_eq!(
            censoring.version,
            format!("1+{}", arb_core::premises::sha8("gap_ms=30000"))
        );
        assert_eq!(
            rows[1].version,
            format!("1+{}", arb_core::premises::sha8(""))
        );
    }
    db.drop(store).await;
}

#[tokio::test]
async fn episodes_and_health_have_the_new_columns() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    store
        .raw_exec("SELECT updated_ms FROM episodes LIMIT 0")
        .await
        .expect("episodes.updated_ms");
    store
        .raw_exec("SELECT rest_rtt_p50_ms, rest_rtt_p99_ms FROM health LIMIT 0")
        .await
        .expect("health RTT columns");
    db.drop(store).await;
}
