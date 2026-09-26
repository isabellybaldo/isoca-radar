//! One throwaway database per test inside the Postgres named by TEST_DATABASE_URL (an owner URL),
//! as in crates/store/tests/common. Without it, tests print SKIPPED and pass, unless
//! STORE_TESTS_REQUIRED is set (CI). Plus router helpers and row builders for seeding.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use api::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use rust_decimal_macros::dec;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use store::{EpisodeLegRow, EpisodeRow, HealthRow, NewRun, PgConnectOptions, Store};
use tower::ServiceExt;

static N: AtomicU32 = AtomicU32::new(0);

pub struct TestDb {
    admin: PgConnectOptions,
    pub name: String,
    pub opts: PgConnectOptions,
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

async fn admin_exec(admin: &PgConnectOptions, sql: &str) {
    let s = Store::connect_with(admin.clone(), 1)
        .await
        .expect("connect admin");
    s.raw_exec(sql).await.expect(sql);
    s.close().await;
}

pub async fn fresh() -> Option<(Store, TestDb)> {
    let url = match std::env::var("TEST_DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            assert!(
                std::env::var("STORE_TESTS_REQUIRED").is_err(),
                "STORE_TESTS_REQUIRED is set but TEST_DATABASE_URL is not"
            );
            eprintln!("SKIPPED: TEST_DATABASE_URL not set");
            return None;
        }
    };
    let admin = Store::options(&url).expect("TEST_DATABASE_URL");
    for role in ["scanner_rw", "analyze_ro"] {
        admin_exec(
            &admin,
            &format!(
                "DO $$ BEGIN CREATE ROLE {role}; \
                 EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$"
            ),
        )
        .await;
    }
    let name = format!(
        "ct_test_{}_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
        nanos()
    );
    admin_exec(&admin, &format!("CREATE DATABASE {name}")).await;
    let opts = admin.clone().database(&name);
    let store = Store::connect_with(opts.clone(), 5)
        .await
        .expect("connect test db");
    store.migrate().await.expect("migrate");
    Some((store, TestDb { admin, name, opts }))
}

impl TestDb {
    pub async fn role(&self, role: &str) -> Store {
        Store::connect_with_role(self.opts.clone(), 2, role)
            .await
            .expect("role connect")
    }

    pub async fn drop(self, store: Store) {
        store.close().await;
        admin_exec(
            &self.admin,
            &format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
    }
}

/// The api exactly as deployed: analyze_ro, read-only sessions, 5 s statement timeout.
pub async fn app(db: &TestDb) -> (Router, AppState) {
    let ro = Store::connect_read_only(db.opts.clone(), 4, 5_000, Some("analyze_ro"))
        .await
        .expect("read-only pool");
    let st = AppState::new(ro);
    (api::router(st.clone()), st)
}

pub async fn get(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

pub fn now() -> i64 {
    api::now_ms()
}

pub async fn run(store: &Store, mode: &str) -> i64 {
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

pub fn ep(id: &str, run_id: i64, opened: i64) -> EpisodeRow {
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

pub fn leg(id: &str, idx: i32, in_s: bool) -> EpisodeLegRow {
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

pub fn health(run_id: i64, minute_ms: i64, drops: i32) -> HealthRow {
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
