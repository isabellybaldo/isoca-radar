# cute-trading Phase 4: the dashboard (S6 api + S7a web + deploy) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A private, polished, read-only dashboard over the scanner's Postgres data (Radar, Episodes, Paper, Health), served on the VPS at `radar.isoca.space` behind a GitHub login.

**Architecture:** A new Rust binary `bins/api` (axum) serves read-only JSON and one SSE stream. It reads Postgres only through a new read-only query module in `crates/store`, as `analyze_ro`, on a pool whose sessions are read-only with a 5 s statement timeout. A new `web/` app (React + Vite + TypeScript, "night shift" only, hand-made pixel SVG) is built to static files and served by an unprivileged nginx. Traefik routes `radar.isoca.space/` to web and `/api/*` to api, and forward-auths every request to oauth2-proxy (GitHub user `isabellybaldo` only).

**Tech Stack:** Rust 1.98.1 (edition 2024), axum 0.8, tokio, sqlx 0.8 (inside `store` only), rust_decimal; React 19, React Router 7, Vite 8, TypeScript 5.9, Vitest 4 + Testing Library + jsdom; nginx-unprivileged; oauth2-proxy v7.12; Traefik (existing on the VPS).

**Spec:** `docs/superpowers/specs/2026-09-25-phase4-dashboard-design.md` (parent: `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §9, §10, §14, §16). Mockups (the target look; night shift only): `mockup-radar.html` variant B and `mockup-detail-paper.html` (the owner has them; the tokens and structure are copied into Task 7's `theme.css` and the page tasks).

This is ONE plan with 11 sequential tasks: api (Tasks 1–6), web (Tasks 7–10), deploy + CI (Task 11). Tasks 1–6 never touch `web/`; Tasks 7–10 never touch Rust. Task 11 needs both.

## Global Constraints

- **Paper only (spec §16):** no order placement, wallets, keys, signing or exchange clients anywhere in api or web. The api refuses to start if an exchange/wallet credential variable is set (same denylist as `bins/scanner/src/guard.rs`).
- **The API can't write (spec §3):** it connects as `analyze_ro` via `DATABASE_URL`; every session is `default_transaction_read_only=on` with `statement_timeout` 5000 ms; it refuses to start if its role has INSERT on `episodes` or `health`; it exposes no write endpoint.
- **Bounds (spec §3):** page size ≤ 200; date ranges ≤ 7 days; series ≤ 7 days (hours ≤ 168). Out-of-range input is a 400, never silently clamped. Every store query also has a hard `LIMIT`.
- **Only `store` uses sqlx** (deny.toml `wrappers = ["store"]`; spec §9.3 "store owns the schema, and nothing else writes SQL"). The api calls `store` functions only.
- **Money and prices are decimal strings in JSON** (e.g. `"0.41"`), never JSON numbers. `NULL` (not measured) and `"0"` (measured zero) stay distinct (parent §10). Trailing zeros are dropped (`"0.410000"` → `"0.41"`); `-0` is `"0"`.
- **Time** is integer ms since epoch in JSON; the web shows UTC with local time on hover; durations look like `4.19 s`.
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (tests may unwrap: `clippy.toml` allows it in tests).
- **Errors** are JSON `{ "error": code, "detail": text }`: bad filter → 400; statement timeout / DB unavailable → 503 with detail exactly `The database is thinking. Try again in a moment.`; anything else → 500 with no internals leaked.
- **Paper endpoints** read S3c's `paper_*` tables (migration 0004, exact S3c names) and return empty lists, not errors, when the tables don't exist (`to_regclass`).
- **One shared Postgres `LISTEN`** for all SSE clients (spec §3). The api adds no load to the scanner: no new tables, triggers or migrations.
- **Web look:** night shift only (`--bg #1b1026`, cards `#241532`, lines teal `#93e1d8`, accent salmon `#ffa69e`, text mint `#ddfff7`), pixel borders, hard 4–6 px shadows; Press Start 2P (headings, tags) and VT323 (body, ≥ 20 px) from Google Fonts only; no UI kit, no chart library (hand SVG, `shape-rendering: crispEdges`); React Router; Vitest + Testing Library.
- **Exact copy strings (spec):** `Night mode only.` · `0 opportunities. The market is efficient, or we are slow. Probably both.` · `N gross, M net. "Fees: N−M. Us: M."` · `The naive strategy is doing great. Until 'Other' wins.` · `No paper trades yet. Nothing ventured, nothing lost.` · `The database is thinking. Try again in a moment.`
- **The fake toggle never changes the theme** (a test asserts it). **No float formatting of money** (a test asserts it): money text is produced by string operations only.
- **Tags carry text, not colour alone.** Responsive down to phone width. Every page has pixel loading, empty and error states; never a blank page.
- **CSP (Task 11):** `default-src 'self'`, fonts from `fonts.googleapis.com` / `fonts.gstatic.com` only, no inline scripts.
- **Debug builds only** on the dev machine (no `--release`, no `cargo install` of big tools). The only docker builds are in Task 11, and the owner runs them.
- **Disk budget 30 GB** (`scripts/disk-budget.sh`): one `web/node_modules` tree (no workspaces, no second install elsewhere); `cargo clean` if `target/` passes 8 GB; `docker builder prune -f --keep-storage 5GB` after any image build.
- **Node:** dev machine has Node v23.8.0 / npm 10.9.2 (checked); CI and the web image use Node 22 (Vite 8 needs `^20.19 || >=22.12`).
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A`/`.`); commit messages have **no** `Co-Authored-By` or any attribution trailer.
- **Shell prefix for every command:** `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store and api) need `TEST_DATABASE_URL`. Start Postgres once with `docker compose -f compose.yml -f compose.dev.yml up -d postgres` (no build), then prefix test commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" &&`. A test that prints `SKIPPED: TEST_DATABASE_URL not set` does **not** count as passing.
- **Precondition for Task 2:** S3c's `crates/store/migrations/0004_paper.sql` is merged on this branch (`ls crates/store/migrations/0004_paper.sql`). If it is missing, stop and report; do not copy S3c's DDL.

## Review Focus

1. **Stale "open" episodes from a crashed run:** a scanner that died without closing its episodes leaves rows with `closed_server_ms IS NULL`; the Radar must show open episodes from the newest live scanner run only, and replay rows never. Pinned by `open_episodes_come_only_from_the_newest_live_run` (Task 1).
2. **Keyset ties:** many episodes opened in the same millisecond must page with no duplicates and no gaps. Pinned by `episodes_page_keyset_has_no_duplicates_or_gaps_on_ties` (Task 1) and `list_pages_with_cursor_until_the_end` (Task 4).
3. **Hostile or silly query values** (`limit=0`, `limit=201`, `limit=99999999999999999999`, `hours=0`, `from>to`, an 8-day range, `flags=x`, a garbage cursor, an episode id with SQL characters or 10 000 characters) must give JSON 400/404, never 500 or a slow query. Pinned by `bad_filters_are_json_400s` and `hostile_ids_are_404s` (Task 4).
4. **The shared LISTEN connection dies** (Postgres restart, `pg_terminate_backend`): the bridge must reconnect with backoff and later NOTIFYs must still reach stream clients. Pinned by `the_bridge_survives_a_killed_listen_connection` (Task 6).
5. **The login session expires while a page is open:** fetches get redirected cross-origin to GitHub and fail with a `TypeError`; the page must say how to recover, not go blank. Pinned by the Vitest case `a failed fetch says how to sign in again` in `EmptyState.test.tsx` (Task 7), and by `openStream` reopening after the browser gives up (`stream.test.ts`, Task 7).

## File Structure

```
Cargo.toml                          MODIFY: members += bins/api; workspace deps axum 0.8, tower 0.5
crates/store/src/read.rs            NEW: read-only pool, role check, health/episodes/today/integrity reads
crates/store/src/read_paper.rs      NEW: paper reads (to_regclass-guarded): accounts, equity, positions, ledger
crates/store/src/lib.rs             MODIFY: mod read; mod read_paper; re-exports
crates/store/src/error.rs           MODIFY: StoreError::is_unavailable (→ 503)
crates/store/src/rows.rs            MODIFY: HealthRow derives sqlx::FromRow
crates/store/src/batch.rs           MODIFY: EPISODE_COLS is pub(crate)
crates/store/Cargo.toml             MODIFY: dev-deps sim + serde_json (pins the paper_state JSON format)
crates/store/tests/read.rs          NEW
crates/store/tests/read_paper.rs    NEW
bins/api/Cargo.toml                 NEW
bins/api/src/lib.rs                 NEW: AppState, StreamEvent, router, now_ms, ensure_read_only
bins/api/src/main.rs                NEW: env guard, config, pool, bridge, serve, graceful shutdown
bins/api/src/config.rs              NEW: env config (DATABASE_URL, API_BIND, API_MAX_CONNS, API_STATEMENT_TIMEOUT_MS)
bins/api/src/guard.rs               NEW: exchange/wallet env denylist (mirror of scanner::guard)
bins/api/src/error.rs               NEW: ApiError → JSON {error, detail}
bins/api/src/dto.rs                 NEW: Dec (decimal → JSON string), Page<T>
bins/api/src/params.rs              NEW: query-string parsing and bounds
bins/api/src/health.rs              NEW: /api/health/now, /api/health/series
bins/api/src/episodes.rs            NEW: /api/episodes/open, /api/episodes, /api/episodes/{id}, /api/today
bins/api/src/integrity.rs           NEW: /api/integrity/summary
bins/api/src/paper.rs               NEW: /api/paper/{accounts,equity,positions,ledger}
bins/api/src/stream.rs              NEW: shared LISTEN bridge + /api/stream SSE
bins/api/tests/common/mod.rs        NEW: throwaway-DB harness (copy of the store pattern) + router helpers + seeds
bins/api/tests/{health,episodes,integrity,paper,stream}.rs   NEW
bins/scanner/tests/arch.rs          MODIFY: api has no sqlx/reqwest/tungstenite/pm-api/sim
web/…                               NEW: Vite app (package.json, tsconfig, vite.config.ts, index.html, src/**)
docker/rust.Dockerfile              MODIFY: build and ship the api binary too
docker/web.Dockerfile               NEW: node build → nginx-unprivileged
docker/web/nginx.conf               NEW
compose.web.yml                     NEW: profile `web`: api, web, oauth2-proxy + Traefik labels
.env.example                        MODIFY: RADAR_DOMAIN + OAUTH2_PROXY_* keys (placeholders only)
.gitignore, .dockerignore           MODIFY: web/node_modules, web/dist
.github/workflows/ci.yml            MODIFY: web job, compose config job
docs/superpowers/runbooks/phase4-dashboard-vps.md   NEW: OAuth App, A record, bundle, deploy, smoke test
```

---

### Task 1: store: read-only pool and dashboard reads (health, episodes, today, integrity)

**Files:**
- Create: `crates/store/src/read.rs`
- Modify: `crates/store/src/lib.rs`, `crates/store/src/error.rs`, `crates/store/src/rows.rs` (HealthRow derive line), `crates/store/src/batch.rs` (`const EPISODE_COLS` line)
- Test: `crates/store/tests/read.rs`

**Interfaces:**
- Consumes: `Store::{connect_with, connect_with_role, pool, write_batch, start_run, episode_legs, observations_for}`, `EpisodeRow`, `EpisodeLegRow`, `HealthRow`, `IntegrityRow`, `Batch`, `EpisodeWrite`, `NewRun` (all existing).
- Produces (all `pub`, re-exported from `store`):
  - `Store::connect_read_only(opts: PgConnectOptions, max_conns: u32, statement_timeout_ms: u32, role: Option<&str>) -> Result<Store, StoreError>`
  - `Store::role_can_write(&self) -> Result<bool, StoreError>`
  - `StoreError::is_unavailable(&self) -> bool`
  - `struct HealthNow { row: HealthRow, drops_5m: i64 }`; `Store::health_now(&self, now_ms: i64) -> Result<Option<HealthNow>, StoreError>`
  - `Store::health_series(&self, from_ms: i64, to_ms: i64) -> Result<Vec<HealthRow>, StoreError>`
  - `Store::latest_health_minute(&self) -> Result<Option<i64>, StoreError>`
  - `struct EpisodeSummary { episode_id, kind, realization, event_title, event_slug, n_legs: i32, s_legs: i64, opened_server_ms: i64, closed_server_ms: Option<i64>, duration_ms: Option<i64>, peak_edge_top, peak_max_size, peak_pnl_net_usd, peak_pnl_gross_usd: Decimal, engine_suspect, stale_transport, censored: bool }`
  - `Store::open_episodes(&self) -> Result<Vec<EpisodeSummary>, StoreError>`
  - `struct EpisodeFilter { kind: Option<String>, from_ms: i64, to_ms: i64, net_positive, suspect, stale, censored: bool, before: Option<(i64, String)>, limit: i64 }`; `struct EpisodePage { rows: Vec<EpisodeSummary>, next: Option<(i64, String)> }`; `Store::episodes_page(&self, f: &EpisodeFilter) -> Result<EpisodePage, StoreError>`
  - `Store::episode(&self, episode_id: &str) -> Result<Option<EpisodeRow>, StoreError>`
  - `struct Today { day_start_ms: i64, gross: i64, net: i64, hourly: Vec<(i64, i64)> }` (24 entries of `(gross, net)`); `Store::today(&self, now_ms: i64) -> Result<Today, StoreError>`
  - `struct IntegritySummary { by_kind: Vec<(String, i64)>, total: i64, msgs: i64, per_million: Option<Decimal> }`; `Store::integrity_summary(&self, from_ms: i64, to_ms: i64) -> Result<IntegritySummary, StoreError>`
  - consts `MAX_PAGE: i64 = 200`, `MAX_OPEN: i64 = 200`, `MAX_SERIES_ROWS: i64 = 10_080`, `DAY_MS: i64 = 86_400_000`

- [ ] **Step 1: Write the failing tests.** Create `crates/store/tests/read.rs`:

```rust
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
    let Some((owner, db)) = common::fresh().await else { return };
    let rw_owner = Store::connect_read_only(db.opts.clone(), 1, 5_000, None).await.unwrap();
    let err = rw_owner.raw_exec("CREATE TABLE nope (a int)").await.unwrap_err();
    assert!(err.to_string().contains("read-only transaction"), "{err}");
    rw_owner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn role_can_write_is_false_only_for_analyze() {
    let Some((owner, db)) = common::fresh().await else { return };
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
    let Some((owner, db)) = common::fresh().await else { return };
    let slow = Store::connect_read_only(db.opts.clone(), 1, 100, Some("analyze_ro")).await.unwrap();
    let err = slow.raw_exec("SELECT pg_sleep(2)").await.unwrap_err();
    assert!(err.is_unavailable(), "{err}");
    assert!(!err.is_data_error());
    slow.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn health_now_is_the_newest_live_row_with_drops_over_5_minutes() {
    let Some((owner, db)) = common::fresh().await else { return };
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
    assert_eq!((h.row.run_id, h.row.minute_ms, h.drops_5m), (live, t - MIN, 3));
    assert_eq!(analyze.latest_health_minute().await.unwrap(), Some(t - MIN));
    let series = analyze.health_series(t - 3 * MIN, t + MIN).await.unwrap();
    assert_eq!(
        series.iter().map(|r| (r.run_id, r.minute_ms)).collect::<Vec<_>>(),
        vec![(live, t - 2 * MIN), (live, t - MIN), (old, t)]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn open_episodes_come_only_from_the_newest_live_run() {
    let Some((owner, db)) = common::fresh().await else { return };
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
                EpisodeWrite { row: stale, legs: vec![] },
                EpisodeWrite {
                    row: open,
                    legs: vec![leg("live-2-00000001", 0, true), leg("live-2-00000001", 1, true), leg("live-2-00000001", 2, false)],
                },
                EpisodeWrite { row: closed, legs: vec![] },
                EpisodeWrite { row: rep, legs: vec![] },
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
    let Some((owner, db)) = common::fresh().await else { return };
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
    assert_eq!(ids(analyze.episodes_page(&base).await.unwrap()), ["live-1-c", "live-1-b", "live-1-a"]);
    let f = EpisodeFilter { kind: Some("yes_basket".into()), ..base.clone() };
    assert_eq!(ids(analyze.episodes_page(&f).await.unwrap()), ["live-1-b"]);
    let f = EpisodeFilter { net_positive: true, ..base.clone() };
    assert_eq!(ids(analyze.episodes_page(&f).await.unwrap()), ["live-1-c", "live-1-a"]);
    let f = EpisodeFilter { suspect: true, ..base.clone() };
    assert_eq!(ids(analyze.episodes_page(&f).await.unwrap()), ["live-1-b"]);
    let f = EpisodeFilter { stale: true, censored: true, ..base.clone() };
    let page = analyze.episodes_page(&f).await.unwrap();
    assert!(page.rows[0].censored);
    assert_eq!(ids(page), ["live-1-c"]);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn episodes_page_keyset_has_no_duplicates_or_gaps_on_ties() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = run(&owner, "live").await;
    let rows: Vec<EpisodeRow> = (0..5).map(|i| ep(&format!("live-1-{i:08}"), live, 7_000)).collect();
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
    assert_eq!(seen, ["live-1-00000004", "live-1-00000003", "live-1-00000002", "live-1-00000001", "live-1-00000000"]);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn episode_detail_round_trips_and_a_missing_one_is_none() {
    let Some((owner, db)) = common::fresh().await else { return };
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
    let Some((owner, db)) = common::fresh().await else { return };
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
    assert_eq!((today.hourly[0], today.hourly[1], today.hourly[2]), ((1, 1), (1, 0), (0, 0)));
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn integrity_summary_counts_by_kind_and_per_million_messages() {
    let Some((owner, db)) = common::fresh().await else { return };
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
    assert_eq!(s.by_kind, vec![("top_mismatch".to_string(), 2), ("drift".to_string(), 1)]);
    assert_eq!((s.total, s.msgs), (3, 258_000));
    assert_eq!(s.per_million, Some(dec!(11.63))); // 3e6 / 258000 = 11.627…
    let empty = analyze.integrity_summary(200_000_000, 200_060_000).await.unwrap();
    assert_eq!((empty.total, empty.msgs, empty.per_million), (0, 0, None));
    analyze.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store --test read`
Expected: compile errors such as `no function or associated item named connect_read_only found for struct Store`, `cannot find struct EpisodeFilter`.

- [ ] **Step 3: Make `EPISODE_COLS` visible to the new module and give `HealthRow` a row mapping.**

In `crates/store/src/batch.rs` change the line
```rust
const EPISODE_COLS: [&str; 49] = [
```
to
```rust
pub(crate) const EPISODE_COLS: [&str; 49] = [
```
In `crates/store/src/rows.rs` change the derive line directly above `pub struct HealthRow {` from
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthRow {
```
to
```rust
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct HealthRow {
```

- [ ] **Step 4: Add `is_unavailable` to `StoreError`.** In `crates/store/src/error.rs`, inside `impl StoreError { … }` after `is_data_error`, add:

```rust
    /// True when the database is busy or unreachable, so the api answers 503 ("try again"):
    /// a statement timeout (57014), an admin shutdown/crash (57P*), a connection failure (08*),
    /// too many connections (53300), pool exhaustion, or a broken socket.
    pub fn is_unavailable(&self) -> bool {
        match self {
            StoreError::Db(sqlx::Error::Database(e)) => e
                .code()
                .map(|c| c == "57014" || c.starts_with("57P") || c.starts_with("08") || c == "53300")
                .unwrap_or(false),
            StoreError::Db(
                sqlx::Error::PoolTimedOut
                | sqlx::Error::PoolClosed
                | sqlx::Error::Io(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::WorkerCrashed,
            ) => true,
            _ => false,
        }
    }
```
and add to its `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn pool_timeouts_are_unavailable_but_missing_rows_are_not() {
        assert!(StoreError::Db(sqlx::Error::PoolTimedOut).is_unavailable());
        assert!(!StoreError::Db(sqlx::Error::RowNotFound).is_unavailable());
    }
```

- [ ] **Step 5: Write the read module.** Create `crates/store/src/read.rs`:

```rust
//! Read-only queries for the dashboard api (Phase 4 spec §4). Every query is parameterized and has
//! a hard LIMIT here, on top of the api's own validation (page ≤ 200, ranges ≤ 7 days). Dashboard
//! reads are live-only: replay runs and `source = 'replay'` episodes never show.
use crate::batch::EPISODE_COLS;
use crate::{EpisodeRow, HealthRow, Store, StoreError};
use rust_decimal::Decimal;
use sqlx::postgres::PgConnectOptions;

pub const MAX_PAGE: i64 = 200;
pub const MAX_OPEN: i64 = 200;
/// Seven days of minutes.
pub const MAX_SERIES_ROWS: i64 = 7 * 24 * 60;
pub const DAY_MS: i64 = 86_400_000;

const HEALTH_COLS: &str = "run_id, minute_ms, tokens, shards, msgs, bytes, drops, reconnects, rest_p50_ms, \
    rest_p99_ms, lag_p50_ms, lag_p99_ms, clock_offset_ms, ok, capture_paused, parked_shards, dropped_groups";

/// The newest live run that has written at least one health row (uses the health primary key).
const LATEST_HEALTH_RUN: &str = "(SELECT r.run_id FROM runs r WHERE r.mode = 'live' \
    AND EXISTS (SELECT 1 FROM health h WHERE h.run_id = r.run_id) ORDER BY r.run_id DESC LIMIT 1)";

const SUMMARY_COLS: &str = "e.episode_id, e.kind, e.realization, e.event_title, e.event_slug, e.n_legs, \
    (SELECT count(*) FROM episode_legs l WHERE l.episode_id = e.episode_id AND l.in_s) AS s_legs, \
    e.opened_server_ms, e.closed_server_ms, e.duration_ms, e.peak_edge_top, e.peak_max_size, \
    e.peak_pnl_net_usd, e.peak_pnl_gross_usd, e.engine_suspect, e.stale_transport, \
    (e.censor_left OR e.censor_right OR e.censor_gap) AS censored";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthNow {
    pub row: HealthRow,
    /// Sum of `drops` over the same run's minutes in [now − 5 min, now].
    pub drops_5m: i64,
}

/// One episode as a table row (Radar, Episodes). `s_legs` = legs in S at peak.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EpisodeSummary {
    pub episode_id: String,
    pub kind: String,
    pub realization: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub n_legs: i32,
    pub s_legs: i64,
    pub opened_server_ms: i64,
    pub closed_server_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub peak_edge_top: Decimal,
    pub peak_max_size: Decimal,
    pub peak_pnl_net_usd: Decimal,
    pub peak_pnl_gross_usd: Decimal,
    pub engine_suspect: bool,
    pub stale_transport: bool,
    pub censored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeFilter {
    /// yes_basket | yes_basket_incomplete | no_basket | implication (None = all kinds)
    pub kind: Option<String>,
    /// opened_server_ms in [from_ms, to_ms)
    pub from_ms: i64,
    pub to_ms: i64,
    pub net_positive: bool,
    pub suspect: bool,
    pub stale: bool,
    pub censored: bool,
    /// Keyset cursor: only rows strictly older than (opened_server_ms, episode_id).
    pub before: Option<(i64, String)>,
    /// Validated by the caller to 1..=MAX_PAGE; clamped here as a last guard.
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodePage {
    pub rows: Vec<EpisodeSummary>,
    /// Cursor of the next (older) page; None when this page is the last one.
    pub next: Option<(i64, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Today {
    pub day_start_ms: i64,
    pub gross: i64,
    pub net: i64,
    /// Exactly 24 entries, hour 0..=23 UTC: (gross-positive, net-positive) episodes opened then.
    pub hourly: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegritySummary {
    /// (kind, count), most frequent first.
    pub by_kind: Vec<(String, i64)>,
    pub total: i64,
    /// Messages counted in `health` over the same window.
    pub msgs: i64,
    /// total × 1,000,000 / msgs, rounded to 2 dp; None when no messages were counted.
    pub per_million: Option<Decimal>,
}

impl Store {
    /// The api's pool: every session is read-only (`default_transaction_read_only`) with its own
    /// statement timeout, on top of the role's grants. `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_read_only(
        opts: PgConnectOptions,
        max_conns: u32,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([
            ("default_transaction_read_only", "on"),
            ("statement_timeout", timeout.as_str()),
        ]);
        match role {
            Some(r) => Self::connect_with_role(opts, max_conns, r).await,
            None => Self::connect_with(opts, max_conns).await,
        }
    }

    /// True when this connection's role could INSERT into the scanner's tables (the api refuses
    /// to start then: spec §3 "The API can't write").
    pub async fn role_can_write(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT has_table_privilege(current_user, 'episodes', 'INSERT') \
                 OR has_table_privilege(current_user, 'health', 'INSERT')",
        )
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn health_now(&self, now_ms: i64) -> Result<Option<HealthNow>, StoreError> {
        let sql = format!(
            "SELECT {HEALTH_COLS} FROM health WHERE run_id = {LATEST_HEALTH_RUN} \
             ORDER BY minute_ms DESC LIMIT 1"
        );
        let Some(row) = sqlx::query_as::<_, HealthRow>(&sql)
            .fetch_optional(self.pool())
            .await?
        else {
            return Ok(None);
        };
        let drops_5m: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(drops), 0)::int8 FROM health WHERE run_id = $1 AND minute_ms >= $2",
        )
        .bind(row.run_id)
        .bind(now_ms.saturating_sub(5 * 60_000))
        .fetch_one(self.pool())
        .await?;
        Ok(Some(HealthNow { row, drops_5m }))
    }

    /// Live health minutes in [from_ms, to_ms), oldest first.
    pub async fn health_series(&self, from_ms: i64, to_ms: i64) -> Result<Vec<HealthRow>, StoreError> {
        let sql = format!(
            "SELECT {HEALTH_COLS} FROM health \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND minute_ms >= $1 AND minute_ms < $2 \
             ORDER BY minute_ms, run_id LIMIT {MAX_SERIES_ROWS}"
        );
        Ok(sqlx::query_as::<_, HealthRow>(&sql)
            .bind(from_ms)
            .bind(to_ms)
            .fetch_all(self.pool())
            .await?)
    }

    /// The newest health minute of the newest live run (the SSE bridge polls this).
    pub async fn latest_health_minute(&self) -> Result<Option<i64>, StoreError> {
        let sql = format!("SELECT max(minute_ms) FROM health WHERE run_id = {LATEST_HEALTH_RUN}");
        Ok(sqlx::query_scalar::<_, Option<i64>>(&sql)
            .fetch_one(self.pool())
            .await?)
    }

    /// Unclosed live episodes of the newest live scanner run only: a crashed run's never-closed
    /// rows are history, not opportunities.
    pub async fn open_episodes(&self) -> Result<Vec<EpisodeSummary>, StoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM episodes e \
             WHERE e.source = 'live' AND e.closed_server_ms IS NULL \
               AND e.run_id = (SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live') \
             ORDER BY e.opened_server_ms DESC, e.episode_id DESC LIMIT {MAX_OPEN}"
        );
        Ok(sqlx::query_as::<_, EpisodeSummary>(&sql)
            .fetch_all(self.pool())
            .await?)
    }

    /// Newest first, keyset-paged on (opened_server_ms, episode_id).
    pub async fn episodes_page(&self, f: &EpisodeFilter) -> Result<EpisodePage, StoreError> {
        let limit = f.limit.clamp(1, MAX_PAGE);
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM episodes e WHERE e.source = 'live' \
               AND e.opened_server_ms >= $1 AND e.opened_server_ms < $2 \
               AND ($3::text IS NULL OR e.kind = $3) \
               AND (NOT $4 OR e.peak_pnl_net_usd > 0) \
               AND (NOT $5 OR e.engine_suspect) \
               AND (NOT $6 OR e.stale_transport) \
               AND (NOT $7 OR e.censor_left OR e.censor_right OR e.censor_gap) \
               AND ($8::int8 IS NULL OR (e.opened_server_ms, e.episode_id) < ($8::int8, $9::text)) \
             ORDER BY e.opened_server_ms DESC, e.episode_id DESC LIMIT $10"
        );
        let (b_ms, b_id) = match &f.before {
            Some((ms, id)) => (Some(*ms), Some(id.clone())),
            None => (None, None),
        };
        let mut rows = sqlx::query_as::<_, EpisodeSummary>(&sql)
            .bind(f.from_ms)
            .bind(f.to_ms)
            .bind(f.kind.clone())
            .bind(f.net_positive)
            .bind(f.suspect)
            .bind(f.stale)
            .bind(f.censored)
            .bind(b_ms)
            .bind(b_id)
            .bind(limit + 1)
            .fetch_all(self.pool())
            .await?;
        let keep = usize::try_from(limit).unwrap_or(usize::MAX);
        let next = if rows.len() > keep {
            rows.truncate(keep);
            rows.last().map(|r| (r.opened_server_ms, r.episode_id.clone()))
        } else {
            None
        };
        Ok(EpisodePage { rows, next })
    }

    /// One episode (any source) by id.
    pub async fn episode(&self, episode_id: &str) -> Result<Option<EpisodeRow>, StoreError> {
        let sql = format!(
            "SELECT {} FROM episodes WHERE episode_id = $1",
            EPISODE_COLS.join(", ")
        );
        Ok(sqlx::query_as::<_, EpisodeRow>(&sql)
            .bind(episode_id)
            .fetch_optional(self.pool())
            .await?)
    }

    /// Today (UTC) in live episodes that could be arbitrage (realization ≠ not_arbitrage):
    /// gross = peak gross Π > 0, net = peak net Π > 0.
    pub async fn today(&self, now_ms: i64) -> Result<Today, StoreError> {
        let day_start_ms = now_ms - now_ms.rem_euclid(DAY_MS);
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT ((opened_server_ms - $1) / 3600000)::int8 AS hour, \
                    count(*) FILTER (WHERE peak_pnl_gross_usd > 0) AS gross, \
                    count(*) FILTER (WHERE peak_pnl_net_usd > 0) AS net \
             FROM episodes \
             WHERE source = 'live' AND realization <> 'not_arbitrage' \
               AND opened_server_ms >= $1 AND opened_server_ms < $2 \
             GROUP BY 1 ORDER BY 1",
        )
        .bind(day_start_ms)
        .bind(day_start_ms + DAY_MS)
        .fetch_all(self.pool())
        .await?;
        let mut hourly = vec![(0i64, 0i64); 24];
        for (hour, gross, net) in rows {
            if let Some(slot) = usize::try_from(hour).ok().and_then(|i| hourly.get_mut(i)) {
                *slot = (gross, net);
            }
        }
        let gross = hourly.iter().map(|h| h.0).sum();
        let net = hourly.iter().map(|h| h.1).sum();
        Ok(Today { day_start_ms, gross, net, hourly })
    }

    /// Live integrity events in [from_ms, to_ms) by kind, plus the per-million-messages rate.
    pub async fn integrity_summary(&self, from_ms: i64, to_ms: i64) -> Result<IntegritySummary, StoreError> {
        let by_kind: Vec<(String, i64)> = sqlx::query_as(
            "SELECT kind, count(*) FROM integrity_events \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live' \
                              AND started_ms < $2 AND (ended_ms IS NULL OR ended_ms >= $1)) \
               AND ts_ms >= $1 AND ts_ms < $2 \
             GROUP BY kind ORDER BY 2 DESC, 1 LIMIT 100",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?;
        let msgs: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(msgs), 0)::int8 FROM health \
             WHERE run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND minute_ms >= $1 AND minute_ms < $2",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_one(self.pool())
        .await?;
        let total: i64 = by_kind.iter().map(|k| k.1).sum();
        let per_million = if msgs > 0 {
            Decimal::from(total)
                .checked_mul(Decimal::from(1_000_000))
                .and_then(|x| x.checked_div(Decimal::from(msgs)))
                .map(|x| x.round_dp(2))
        } else {
            None
        };
        Ok(IntegritySummary { by_kind, total, msgs, per_million })
    }
}
```

Note: the test harness's `runs` rows use `started_ms: 1` and no `ended_ms`, so every seeded run overlaps every window.

- [ ] **Step 6: Wire the module.** In `crates/store/src/lib.rs`, add `mod read;` after `mod batch;` and add below the existing `pub use` lines:

```rust
pub use read::{
    DAY_MS, EpisodeFilter, EpisodePage, EpisodeSummary, HealthNow, IntegritySummary, MAX_OPEN,
    MAX_PAGE, MAX_SERIES_ROWS, Today,
};
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: PASS, including the 11 tests in `read.rs`, the new `error.rs` unit test, and every existing store test (none prints SKIPPED).

- [ ] **Step 8: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p store --all-targets -- -D warnings`
Expected: no warnings. (`cargo fmt` may reflow the test file's long `vec![…]` lines; that is fine.)

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/store/src/read.rs crates/store/src/lib.rs crates/store/src/error.rs crates/store/src/rows.rs crates/store/src/batch.rs crates/store/tests/read.rs && git commit -m "store: read-only pool and dashboard reads (health, episodes, today, integrity)"
```

---

### Task 2: store: paper reads (degrade to empty without S3c's tables)

**Files:**
- Create: `crates/store/src/read_paper.rs`
- Modify: `crates/store/src/lib.rs`, `crates/store/Cargo.toml` (dev-dependencies)
- Test: `crates/store/tests/read_paper.rs`

**Interfaces:**
- Consumes: S3c migration 0004 tables and columns exactly: `paper_accounts(run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256)`, `paper_marks(run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown, n_positions)`, `paper_ledger(run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity)`, `paper_state(account_id, state, updated_ms)` where `state` = `serde_json::to_string(&sim::account::Account)` (S3c Task 3). Task 1's `connect_read_only`.
- Produces (re-exported from `store`):
  - `Store::paper_ready(&self) -> Result<bool, StoreError>`
  - `struct PaperAccountNow { account_id, policy, scenario: String, fee_bips, bankroll: Decimal, run_id: i64, t_ms: Option<i64>, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown: Option<Decimal>, n_positions: Option<i64> }`; `Store::paper_accounts_now(&self) -> Result<Vec<PaperAccountNow>, StoreError>`
  - `struct EquityPoint { t_ms: i64, equity: Decimal }`; `Store::paper_equity(&self, account_id: &str, from_ms: i64, to_ms: i64) -> Result<Vec<EquityPoint>, StoreError>`
  - `struct PaperPosition { account_id, market_id, outcome: String, qty, basis: Decimal, returned: bool, event_title: Option<String> }`; `Store::paper_positions(&self) -> Result<Vec<PaperPosition>, StoreError>`
  - `struct PaperLedgerEntry { run_id: i64, account_id: String, seq: i64, t_ms: i64, kind: String, episode_id, market_id, outcome: Option<String>, qty: Decimal, price: Option<Decimal>, cash_delta, fees, realized_delta, cash_after: Decimal, equity: Option<Decimal> }`; `struct LedgerPage { rows: Vec<PaperLedgerEntry>, next: Option<(i64, i64)> }`; `Store::paper_ledger_page(&self, account_id: &str, before: Option<(i64, i64)>, limit: i64) -> Result<LedgerPage, StoreError>`

- [ ] **Step 1: Check the precondition.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && ls crates/store/migrations/0004_paper.sql && grep -c "CREATE TABLE paper_" crates/store/migrations/0004_paper.sql`
Expected: the path, then `8`. If the file is missing, stop and report "S3c migration 0004 not merged".

- [ ] **Step 2: Add the dev-dependencies** (they pin the `paper_state` JSON format to the real `sim::Account`). In `crates/store/Cargo.toml` under `[dev-dependencies]` add:

```toml
sim = { path = "../sim" }
serde_json.workspace = true
```

- [ ] **Step 3: Write the failing tests.** Create `crates/store/tests/read_paper.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use sim::account::{Account, Position};
use sim::config::{AccountSpec, PolicyKind};
use sim::types::{LegKey, Outcome};
use store::*;

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

async fn account(owner: &Store, run_id: i64, id: &str, policy: &str, scenario: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run_id}, '{id}', '{policy}', '{scenario}', 0, 1000, 100, 'sha')"
        ))
        .await
        .unwrap();
}

async fn mark(owner: &Store, run_id: i64, id: &str, t_ms: i64, equity: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown, n_positions) \
             VALUES ({run_id}, '{id}', {t_ms}, 900, {equity}, 0, 0, 0, 95.000000, 0.40, 2)"
        ))
        .await
        .unwrap();
}

async fn ledger(owner: &Store, run_id: i64, id: &str, seq: i64) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run_id}, '{id}', {seq}, {}, 'buy', 'live-1-00000001', 'm1', 'y1', 'no', 51.2, 0.62, -99.84, 0.21, 0, 900.16, NULL)",
            seq * 1_000
        ))
        .await
        .unwrap();
}

async fn ro(db: &common::TestDb) -> Store {
    Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap()
}

#[tokio::test]
async fn paper_reads_are_empty_when_the_tables_are_missing() {
    let Some((owner, db)) = common::fresh().await else { return };
    let analyze = ro(&db).await;
    assert!(analyze.paper_ready().await.unwrap(), "S3c migration 0004 must be merged first");
    owner
        .raw_exec(
            "DROP TABLE paper_markouts, paper_fills, paper_execs, paper_intents, paper_ledger, \
             paper_marks, paper_state, paper_accounts CASCADE",
        )
        .await
        .unwrap();
    assert!(!analyze.paper_ready().await.unwrap());
    assert!(analyze.paper_accounts_now().await.unwrap().is_empty());
    assert!(analyze.paper_equity("P0-converter/sp-idle", 0, 10).await.unwrap().is_empty());
    assert!(analyze.paper_positions().await.unwrap().is_empty());
    let page = analyze.paper_ledger_page("P0-converter/sp-idle", None, 50).await.unwrap();
    assert!(page.rows.is_empty() && page.next.is_none());
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn accounts_show_the_latest_mark_of_the_newest_live_run() {
    let Some((owner, db)) = common::fresh().await else { return };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    account(&owner, old, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    account(&owner, live, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    account(&owner, live, "P3-naive/sp-idle", "P3-naive", "sp-idle").await;
    account(&owner, replay, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    mark(&owner, old, "P0-converter/sp-idle", 1_000, "990").await;
    mark(&owner, live, "P0-converter/sp-idle", 60_000, "1001.00").await;
    mark(&owner, live, "P0-converter/sp-idle", 120_000, "1002.56").await;
    mark(&owner, replay, "P0-converter/sp-idle", 180_000, "5").await;
    let analyze = ro(&db).await;
    let accts = analyze.paper_accounts_now().await.unwrap();
    assert_eq!(accts.len(), 2);
    assert_eq!((accts[0].account_id.as_str(), accts[0].run_id, accts[0].t_ms), ("P0-converter/sp-idle", live, Some(120_000)));
    assert_eq!(accts[0].equity, Some(dec!(1002.56)));
    assert_eq!(accts[0].realized, Some(dec!(0)));
    assert_eq!(accts[0].bankroll, dec!(1000));
    assert_eq!((accts[1].account_id.as_str(), accts[1].t_ms, accts[1].equity), ("P3-naive/sp-idle", None, None));
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn equity_spans_live_runs_in_the_window_and_skips_replays() {
    let Some((owner, db)) = common::fresh().await else { return };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [old, live, replay] {
        account(&owner, r, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    }
    mark(&owner, old, "P0-converter/sp-idle", 1_000, "999").await;
    mark(&owner, live, "P0-converter/sp-idle", 2_000, "1000").await;
    mark(&owner, live, "P0-converter/sp-idle", 9_000, "1001").await; // outside [0, 5000)
    mark(&owner, replay, "P0-converter/sp-idle", 3_000, "7").await;
    let analyze = ro(&db).await;
    let pts = analyze.paper_equity("P0-converter/sp-idle", 0, 5_000).await.unwrap();
    assert_eq!(
        pts,
        vec![EquityPoint { t_ms: 1_000, equity: dec!(999) }, EquityPoint { t_ms: 2_000, equity: dec!(1000) }]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_parse_a_real_sim_account_and_ignore_odd_states() {
    let Some((owner, db)) = common::fresh().await else { return };
    owner
        .raw_exec(
            "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) \
             VALUES ('ev1', 's', 'Fed decision in October?', NULL, false, 2, 1, 1); \
             INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, first_seen_ms, last_seen_ms) \
             VALUES ('m1', 'ev1', 'y1', 'n1', 'live', 0.07, 1, 1, 1)",
        )
        .await
        .unwrap();
    let mut acct = Account::new(AccountSpec {
        policy: PolicyKind::P1Settlement,
        scenario: "sp-idle".into(),
        bankroll: dec!(1000),
        per_trade: dec!(100),
        fee_bips: dec!(0),
    });
    let held = LegKey { market_id: "m1".into(), yes_token: "y1".into(), outcome: Outcome::Yes };
    acct.positions.insert(held, Position { qty: dec!(40), basis: dec!(18.00) });
    let back = LegKey { market_id: "m9".into(), yes_token: "y9".into(), outcome: Outcome::Yes };
    acct.returned.insert(back, dec!(5));
    let json = serde_json::to_string(&acct).unwrap();
    assert!(!json.contains('\''), "the test inlines the JSON in single quotes");
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_state (account_id, state, updated_ms) VALUES \
             ('{}', '{json}', 1), ('odd', '{{\"v\":1}}', 1), ('weird', '{{\"positions\":{{\"a\":1}}}}', 1)",
            acct.id
        ))
        .await
        .unwrap();
    let analyze = ro(&db).await;
    let pos = analyze.paper_positions().await.unwrap();
    assert_eq!(
        pos,
        vec![
            PaperPosition {
                account_id: "P1-settlement/sp-idle".into(),
                market_id: "m1".into(),
                outcome: "yes".into(),
                qty: dec!(40),
                basis: dec!(18.00),
                returned: false,
                event_title: Some("Fed decision in October?".into()),
            },
            PaperPosition {
                account_id: "P1-settlement/sp-idle".into(),
                market_id: "m9".into(),
                outcome: "yes".into(),
                qty: dec!(5),
                basis: dec!(0),
                returned: true,
                event_title: None,
            },
        ]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn ledger_pages_newest_first_by_run_then_seq() {
    let Some((owner, db)) = common::fresh().await else { return };
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [old, live, replay] {
        account(&owner, r, "P0-converter/sp-idle", "P0-converter", "sp-idle").await;
    }
    ledger(&owner, old, "P0-converter/sp-idle", 1).await;
    ledger(&owner, old, "P0-converter/sp-idle", 2).await;
    ledger(&owner, live, "P0-converter/sp-idle", 3).await;
    ledger(&owner, replay, "P0-converter/sp-idle", 9).await;
    let analyze = ro(&db).await;
    let p1 = analyze.paper_ledger_page("P0-converter/sp-idle", None, 2).await.unwrap();
    assert_eq!(p1.rows.iter().map(|r| (r.run_id, r.seq)).collect::<Vec<_>>(), vec![(live, 3), (old, 2)]);
    assert_eq!(p1.rows[0].cash_delta, dec!(-99.84));
    assert_eq!(p1.rows[0].equity, None);
    assert_eq!(p1.next, Some((old, 2)));
    let p2 = analyze.paper_ledger_page("P0-converter/sp-idle", p1.next, 2).await.unwrap();
    assert_eq!(p2.rows.iter().map(|r| (r.run_id, r.seq)).collect::<Vec<_>>(), vec![(old, 1)]);
    assert_eq!(p2.next, None);
    analyze.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 4: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store --test read_paper`
Expected: compile errors `no method named paper_ready`, `cannot find struct PaperPosition`.

- [ ] **Step 5: Write the paper read module.** Create `crates/store/src/read_paper.rs`:

```rust
//! Paper-trading reads for the dashboard (Phase 4 spec §4), over S3c's migration 0004 tables.
//! Until 0004 is applied every reader returns an empty result, never an error. Live runs only:
//! replay runs write their own paper rows and never show here.
use crate::{Store, StoreError};
use rust_decimal::Decimal;

const LEDGER_MAX: i64 = 200;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperAccountNow {
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Decimal,
    pub bankroll: Decimal,
    pub run_id: i64,
    /// The latest mark; all None until the account's first mark in this run.
    pub t_ms: Option<i64>,
    pub cash: Option<Decimal>,
    pub equity: Option<Decimal>,
    pub realized: Option<Decimal>,
    pub unrealized: Option<Decimal>,
    pub fees_paid: Option<Decimal>,
    pub locked: Option<Decimal>,
    pub max_drawdown: Option<Decimal>,
    pub n_positions: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EquityPoint {
    pub t_ms: i64,
    pub equity: Decimal,
}

/// A holding awaiting resolution, from the live `paper_state` save. `returned` = YES returned by a
/// NO→collateral conversion (basis 0, spec §5.6).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperPosition {
    pub account_id: String,
    pub market_id: String,
    pub outcome: String,
    pub qty: Decimal,
    pub basis: Decimal,
    pub returned: bool,
    pub event_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperLedgerEntry {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub outcome: Option<String>,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub cash_delta: Decimal,
    pub fees: Decimal,
    pub realized_delta: Decimal,
    pub cash_after: Decimal,
    pub equity: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerPage {
    pub rows: Vec<PaperLedgerEntry>,
    /// Cursor (run_id, seq) of the next (older) page; None on the last page.
    pub next: Option<(i64, i64)>,
}

impl Store {
    /// True once S3c's migration 0004 is applied (every paper table the dashboard reads exists).
    pub async fn paper_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT to_regclass('public.paper_accounts') IS NOT NULL \
                AND to_regclass('public.paper_marks') IS NOT NULL \
                AND to_regclass('public.paper_ledger') IS NOT NULL \
                AND to_regclass('public.paper_state') IS NOT NULL",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// Every account of the newest live run that has paper accounts, with its latest mark.
    pub async fn paper_accounts_now(&self) -> Result<Vec<PaperAccountNow>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, PaperAccountNow>(
            "SELECT a.account_id, a.policy, a.scenario, a.fee_bips, a.bankroll, a.run_id, \
                    m.t_ms, m.cash, m.equity, m.realized, m.unrealized, m.fees_paid, m.locked, \
                    m.max_drawdown, m.n_positions \
             FROM paper_accounts a \
             LEFT JOIN LATERAL ( \
                 SELECT pm.t_ms, pm.cash, pm.equity, pm.realized, pm.unrealized, pm.fees_paid, \
                        pm.locked, pm.max_drawdown, pm.n_positions \
                 FROM paper_marks pm \
                 WHERE pm.run_id = a.run_id AND pm.account_id = a.account_id \
                 ORDER BY pm.t_ms DESC LIMIT 1) m ON true \
             WHERE a.run_id = (SELECT max(a2.run_id) FROM paper_accounts a2 \
                               JOIN runs r2 ON r2.run_id = a2.run_id WHERE r2.mode = 'live') \
             ORDER BY a.account_id LIMIT 100",
        )
        .fetch_all(self.pool())
        .await?)
    }

    /// Minute marks of one account across live runs (restarts restore the account), oldest first.
    pub async fn paper_equity(&self, account_id: &str, from_ms: i64, to_ms: i64) -> Result<Vec<EquityPoint>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, EquityPoint>(
            "SELECT m.t_ms, m.equity FROM paper_marks m \
             WHERE m.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND m.account_id = $1 AND m.t_ms >= $2 AND m.t_ms < $3 \
             ORDER BY m.t_ms, m.run_id LIMIT 20160",
        )
        .bind(account_id)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// Holdings in the live `paper_state` saves (sim::Account JSON: `positions` and `returned` are
    /// arrays of [LegKey, value] pairs). A state that isn't a JSON object, or has no such arrays,
    /// contributes nothing.
    pub async fn paper_positions(&self) -> Result<Vec<PaperPosition>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, PaperPosition>(
            "WITH s AS ( \
                 SELECT account_id, state::jsonb AS j FROM paper_state WHERE state IS JSON OBJECT \
             ), held AS ( \
                 SELECT s.account_id, p.value->0->>'market_id' AS market_id, \
                        p.value->0->>'outcome' AS outcome, \
                        (p.value->1->>'qty')::numeric AS qty, \
                        (p.value->1->>'basis')::numeric AS basis, false AS returned \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'positions') = 'array' THEN s.j->'positions' \
                          ELSE '[]'::jsonb END) p \
                 WHERE p.value->1->>'qty' IS NOT NULL AND p.value->1->>'basis' IS NOT NULL \
                 UNION ALL \
                 SELECT s.account_id, r.value->0->>'market_id', r.value->0->>'outcome', \
                        (r.value->>1)::numeric, 0::numeric, true \
                 FROM s CROSS JOIN LATERAL jsonb_array_elements( \
                     CASE WHEN jsonb_typeof(s.j->'returned') = 'array' THEN s.j->'returned' \
                          ELSE '[]'::jsonb END) r \
                 WHERE r.value->>1 IS NOT NULL \
             ) \
             SELECT h.account_id, h.market_id, h.outcome, h.qty, h.basis, h.returned, \
                    e.title AS event_title \
             FROM held h \
             LEFT JOIN legs l ON l.market_id = h.market_id \
             LEFT JOIN events e ON e.event_id = l.event_id \
             WHERE h.market_id IS NOT NULL AND h.outcome IS NOT NULL \
             ORDER BY h.account_id, h.returned, h.market_id, h.outcome LIMIT 1000",
        )
        .fetch_all(self.pool())
        .await?)
    }

    /// One account's ledger across live runs, newest first, keyset-paged on (run_id, seq).
    pub async fn paper_ledger_page(
        &self,
        account_id: &str,
        before: Option<(i64, i64)>,
        limit: i64,
    ) -> Result<LedgerPage, StoreError> {
        if !self.paper_ready().await? {
            return Ok(LedgerPage { rows: Vec::new(), next: None });
        }
        let limit = limit.clamp(1, LEDGER_MAX);
        let (b_run, b_seq) = match before {
            Some((r, s)) => (Some(r), Some(s)),
            None => (None, None),
        };
        let mut rows = sqlx::query_as::<_, PaperLedgerEntry>(
            "SELECT l.run_id, l.account_id, l.seq, l.t_ms, l.kind, l.episode_id, l.market_id, \
                    l.outcome, l.qty, l.price, l.cash_delta, l.fees, l.realized_delta, \
                    l.cash_after, l.equity \
             FROM paper_ledger l \
             WHERE l.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND l.account_id = $1 \
               AND ($2::int8 IS NULL OR (l.run_id, l.seq) < ($2::int8, $3::int8)) \
             ORDER BY l.run_id DESC, l.seq DESC LIMIT $4",
        )
        .bind(account_id)
        .bind(b_run)
        .bind(b_seq)
        .bind(limit + 1)
        .fetch_all(self.pool())
        .await?;
        let keep = usize::try_from(limit).unwrap_or(usize::MAX);
        let next = if rows.len() > keep {
            rows.truncate(keep);
            rows.last().map(|r| (r.run_id, r.seq))
        } else {
            None
        };
        Ok(LedgerPage { rows, next })
    }
}
```

- [ ] **Step 6: Wire the module.** In `crates/store/src/lib.rs` add `mod read_paper;` after `mod read;`, and below the Task 1 re-export add:

```rust
pub use read_paper::{EquityPoint, LedgerPage, PaperAccountNow, PaperLedgerEntry, PaperPosition};
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: PASS (5 new tests in `read_paper.rs`; nothing SKIPPED).

- [ ] **Step 8: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p store --all-targets -- -D warnings && cargo test -p scanner --test arch`
Expected: no warnings; `crate_graph_matches_spec_9_2` passes (the new `sim` edge is a dev-dependency, which the arch test ignores).

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/store/src/read_paper.rs crates/store/src/lib.rs crates/store/Cargo.toml Cargo.lock crates/store/tests/read_paper.rs && git commit -m "store: paper reads for the dashboard, empty until S3c's tables exist"
```

---
### Task 3: `bins/api` skeleton: config, guard, errors, decimal strings, health endpoints

**Files:**
- Modify: `Cargo.toml` (workspace `members` and `[workspace.dependencies]`), `bins/scanner/tests/arch.rs`
- Create: `bins/api/Cargo.toml`, `bins/api/src/lib.rs`, `bins/api/src/main.rs`, `bins/api/src/config.rs`, `bins/api/src/guard.rs`, `bins/api/src/error.rs`, `bins/api/src/dto.rs`, `bins/api/src/params.rs`, `bins/api/src/health.rs`
- Test: `bins/api/tests/common/mod.rs`, `bins/api/tests/health.rs` (plus unit tests inside `config.rs`, `guard.rs`, `dto.rs`, `params.rs`)

**Interfaces:**
- Consumes (Task 1): `Store::{connect_read_only, role_can_write, health_now, health_series}`, `HealthNow`, `HealthRow`, `StoreError::is_unavailable`.
- Produces (used by Tasks 4–6 and 11):
  - `api::AppState { store: Store, events: broadcast::Sender<StreamEvent>, stream_slots: Arc<Semaphore> }`, `AppState::new(store: Store) -> AppState`
  - `api::StreamEvent::{Episode(String), Health(i64)}`, `api::MAX_STREAMS: usize = 16`
  - `api::router(state: AppState) -> axum::Router`, `api::now_ms() -> i64`, `api::ensure_read_only(store: &Store) -> anyhow::Result<()>`
  - `api::error::ApiError::{BadRequest(&'static str, String), NotFound, Unavailable(&'static str, &'static str), Internal}` (implements `IntoResponse` and `From<StoreError>`); `api::error::BUSY_DETAIL`
  - `api::dto::Dec(pub Decimal)` (serializes as a JSON string), `api::dto::Page<T> { items: Vec<T>, next_cursor: Option<String> }`
  - `api::params::{Params, query, opt_i64, hours, limit, flag, text, range, MAX_HOURS, MAX_RANGE_MS, MAX_LIMIT, DEFAULT_LIMIT}`
  - Test helpers in `bins/api/tests/common/mod.rs`: `fresh()`, `TestDb::{role, drop}`, `app(&TestDb) -> (Router, AppState)`, `get(&Router, &str) -> (StatusCode, serde_json::Value)`, `now()`, `run(&Store, &str) -> i64`, `ep(&str, i64, i64) -> EpisodeRow`, `leg(&str, i32, bool) -> EpisodeLegRow`, `health(i64, i64, i32) -> HealthRow`
  - JSON: `GET /api/health/now` → `{"health": null | {now_ms, age_ms, run_id, minute_ms, tokens, shards, parked_shards, msgs, drops, drops_5m, reconnects, lag_p50_ms, lag_p99_ms, rest_p50_ms, rest_p99_ms, clock_offset_ms, ok, capture_paused, dropped_groups}}`; `GET /api/health/series?hours=` → `{from_ms, to_ms, points: [{minute_ms, run_id, msgs, drops, reconnects, lag_p50_ms, lag_p99_ms, shards, parked_shards, capture_paused}]}`

- [ ] **Step 1: Add the crate to the workspace.** In the root `Cargo.toml`:
  - change `members = ["crates/arb-core", "crates/pm-api", "crates/store", "crates/sim", "spikes/probe", "bins/scanner"]` to `members = ["crates/arb-core", "crates/pm-api", "crates/store", "crates/sim", "spikes/probe", "bins/scanner", "bins/api"]`;
  - append to `[workspace.dependencies]`:
```toml
axum = { version = "0.8", default-features = false, features = ["http1", "json", "query", "tokio"] }
tower = { version = "0.5", default-features = false, features = ["util"] }
```
Create `bins/api/Cargo.toml`:
```toml
[package]
name = "api"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
store = { path = "../../crates/store" }
axum.workspace = true
tokio = { workspace = true, features = ["net", "signal"] }
serde.workspace = true
serde_json.workspace = true
rust_decimal.workspace = true
anyhow.workspace = true

[dev-dependencies]
tower.workspace = true
rust_decimal_macros.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Pin the crate boundary in the architecture test.** In `bins/scanner/tests/arch.rs`, at the end of `crate_graph_matches_spec_9_2` (after the scanner loop), add:
```rust
    for bad in ["reqwest", "tokio-tungstenite", "sqlx", "pm-api", "sim", "arb-core"] {
        assert!(
            !d["api"].contains(bad),
            "api reads Postgres only via store and has no network client or trading code ({bad})"
        );
    }
```

- [ ] **Step 3: Write the test harness.** Create `bins/api/tests/common/mod.rs` (the store harness pattern, copied because a test crate can't import another crate's `tests/`):

```rust
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
    let s = Store::connect_with(admin.clone(), 1).await.expect("connect admin");
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
    let store = Store::connect_with(opts.clone(), 5).await.expect("connect test db");
    store.migrate().await.expect("migrate");
    Some((store, TestDb { admin, name, opts }))
}

impl TestDb {
    pub async fn role(&self, role: &str) -> Store {
        Store::connect_with_role(self.opts.clone(), 2, role).await.expect("role connect")
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
    let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
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
```

- [ ] **Step 4: Write the failing endpoint tests.** Create `bins/api/tests/health.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

#[tokio::test]
async fn health_now_is_null_before_any_row() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/now").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["health"].is_null(), "{v}");
    db.drop(owner).await;
}

#[tokio::test]
async fn health_now_returns_the_latest_row_with_its_age_and_drops_5m() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let t = n - n % 60_000;
    owner
        .write_batch(&Batch {
            health: vec![common::health(live, t - 60_000, 2), common::health(live, t, 1)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/now").await;
    assert_eq!(status, StatusCode::OK);
    let h = &v["health"];
    assert_eq!(h["minute_ms"], t);
    assert_eq!(h["run_id"], live);
    assert_eq!(h["drops_5m"], 3);
    assert_eq!(h["lag_p99_ms"], 1_900);
    assert_eq!(h["capture_paused"], false);
    assert!(h["age_ms"].as_i64().unwrap() >= 0);
    assert!(h["now_ms"].as_i64().unwrap() >= t);
    db.drop(owner).await;
}

#[tokio::test]
async fn health_series_returns_the_window_and_rejects_bad_hours() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let t = n - n % 60_000;
    owner
        .write_batch(&Batch {
            health: vec![common::health(live, t - 2 * 3_600_000, 0), common::health(live, t, 0)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/health/series?hours=1").await;
    assert_eq!(status, StatusCode::OK);
    let pts = v["points"].as_array().unwrap();
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0]["minute_ms"], t);
    let (_, v24) = common::get(&app, "/api/health/series").await;
    assert_eq!(v24["points"].as_array().unwrap().len(), 2);
    for bad in ["0", "169", "abc", "-1", "99999999999999999999"] {
        let (status, v) = common::get(&app, &format!("/api/health/series?hours={bad}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "hours={bad}");
        assert_eq!(v["error"], "bad_hours");
        assert!(v["detail"].is_string());
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn an_unknown_route_is_a_json_404() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "not_found");
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_role_cannot_write_and_a_writer_role_is_refused() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (_app, st) = common::app(&db).await;
    api::ensure_read_only(&st.store).await.unwrap();
    let err = st
        .store
        .raw_exec("INSERT INTO runs (service, started_ms, version, config_hash, mode) VALUES ('x', 1, 'v', 'c', 'live')")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("read-only transaction") || err.contains("permission denied"), "{err}");
    let owner_pool = Store::connect_read_only(db.opts.clone(), 1, 5_000, None).await.unwrap();
    assert!(api::ensure_read_only(&owner_pool).await.is_err());
    owner_pool.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 5: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api`
Expected: FAIL to compile: `couldn't read bins/api/src/lib.rs` / `unresolved import api`.

- [ ] **Step 6: Write the guard** (spec §16 #1; mirrors `bins/scanner/src/guard.rs`, which the api can't import without pulling in `pm-api`). Create `bins/api/src/guard.rs`:

```rust
//! Spec §16 #1: refuse to start if any exchange or wallet credential is present in the environment.
//! Database role passwords and DATABASE_URL are expected and allowed. Only NAMES are reported.
//! Same denylist as bins/scanner/src/guard.rs (keep the two in step).

const CONTAINS: [&str; 5] = ["PRIVATE_KEY", "MNEMONIC", "SEED_PHRASE", "API_SECRET", "PASSPHRASE"];
const PREFIXES: [&str; 2] = ["POLY_", "CLOB_"];

pub fn forbidden<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut bad: Vec<String> = names
        .into_iter()
        .map(|n| n.as_ref().to_string())
        .filter(|n| {
            let u = n.to_ascii_uppercase();
            CONTAINS.iter().any(|c| u.contains(c)) || PREFIXES.iter().any(|p| u.starts_with(p))
        })
        .collect();
    bad.sort();
    bad
}

pub fn check_env() -> anyhow::Result<()> {
    let bad = forbidden(std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()));
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: exchange/wallet credential variables are set: {} (paper-only project, spec §16)",
        bad.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_wallet_and_exchange_names_but_not_database_settings() {
        let names = [
            "POLY_API_KEY",
            "WALLET_PRIVATE_KEY",
            "KEY_PASSPHRASE",
            "DATABASE_URL",
            "API_BIND",
            "API_MAX_CONNS",
            "PATH",
        ];
        assert_eq!(forbidden(names), vec!["KEY_PASSPHRASE", "POLY_API_KEY", "WALLET_PRIVATE_KEY"]);
    }
}
```

- [ ] **Step 7: Write the config.** Create `bins/api/src/config.rs`:

```rust
//! Environment-only configuration (the api has no config file). The database URL carries a
//! password, so Debug redacts it.
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// postgres://analyze_ro:…@postgres:5432/cute_trading
    pub database_url: String,
    pub bind: String,
    /// Pool size; one connection is held by the shared LISTEN.
    pub max_conns: u32,
    pub statement_timeout_ms: u32,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("bind", &self.bind)
            .field("max_conns", &self.max_conns)
            .field("statement_timeout_ms", &self.statement_timeout_ms)
            .finish()
    }
}

fn parse_or(v: Option<String>, default: u32, name: &str) -> anyhow::Result<u32> {
    match v {
        None => Ok(default),
        Some(s) => s
            .parse()
            .map_err(|_| anyhow::anyhow!("{name} must be a positive integer")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let database_url = get("DATABASE_URL")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the analyze_ro URL)"))?;
        let bind = get("API_BIND").unwrap_or_else(|| "0.0.0.0:8081".to_string());
        let max_conns = parse_or(get("API_MAX_CONNS"), 4, "API_MAX_CONNS")?;
        let statement_timeout_ms =
            parse_or(get("API_STATEMENT_TIMEOUT_MS"), 5_000, "API_STATEMENT_TIMEOUT_MS")?;
        anyhow::ensure!(
            (2..=16).contains(&max_conns),
            "API_MAX_CONNS must be 2..=16 (one connection is the shared LISTEN)"
        );
        anyhow::ensure!(
            (100..=60_000).contains(&statement_timeout_ms),
            "API_STATEMENT_TIMEOUT_MS must be 100..=60000"
        );
        Ok(Self { database_url, bind, max_conns, statement_timeout_ms })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let m: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_apply_and_the_url_is_required() {
        let c = cfg(&[("DATABASE_URL", "postgres://analyze_ro:pw@postgres/cute_trading")]).unwrap();
        assert_eq!((c.bind.as_str(), c.max_conns, c.statement_timeout_ms), ("0.0.0.0:8081", 4, 5_000));
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[("DATABASE_URL", "")]).is_err());
    }

    #[test]
    fn bounds_are_enforced_and_debug_hides_the_password() {
        let url = ("DATABASE_URL", "postgres://analyze_ro:hunter2@postgres/cute_trading");
        assert!(cfg(&[url, ("API_MAX_CONNS", "1")]).is_err());
        assert!(cfg(&[url, ("API_MAX_CONNS", "x")]).is_err());
        assert!(cfg(&[url, ("API_STATEMENT_TIMEOUT_MS", "99")]).is_err());
        let c = cfg(&[url, ("API_BIND", "127.0.0.1:9000")]).unwrap();
        assert_eq!(c.bind, "127.0.0.1:9000");
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
```

- [ ] **Step 8: Write the error type.** Create `bins/api/src/error.rs`:

```rust
//! Every error is JSON `{ "error": code, "detail": text }` (spec §4). Store internals are logged
//! to stderr and never sent to the client.
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use store::StoreError;

/// Spec §4, verbatim.
pub const BUSY_DETAIL: &str = "The database is thinking. Try again in a moment.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// 400: (code, detail)
    BadRequest(&'static str, String),
    /// 404
    NotFound,
    /// 503: (code, detail)
    Unavailable(&'static str, &'static str),
    /// 500
    Internal,
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        if e.is_unavailable() {
            eprintln!("api: database unavailable: {e}");
            ApiError::Unavailable("db_unavailable", BUSY_DETAIL)
        } else {
            eprintln!("api: store error: {e}");
            ApiError::Internal
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, detail) = match self {
            ApiError::BadRequest(code, detail) => (StatusCode::BAD_REQUEST, code, detail),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Nothing here. It may have been a dream.".to_string(),
            ),
            ApiError::Unavailable(code, detail) => {
                (StatusCode::SERVICE_UNAVAILABLE, code, detail.to_string())
            }
            ApiError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Something broke on our side. It has been written down.".to_string(),
            ),
        };
        (status, Json(json!({ "error": code, "detail": detail }))).into_response()
    }
}

/// Router fallback: unknown paths are JSON 404s too.
pub async fn not_found() -> ApiError {
    ApiError::NotFound
}
```

- [ ] **Step 9: Write the decimal-string DTO helpers.** Create `bins/api/src/dto.rs`:

```rust
//! JSON shapes shared by every endpoint. Money and prices are decimal STRINGS (spec §4): a JSON
//! number would be read as a float by the browser.
use rust_decimal::Decimal;
use serde::{Serialize, Serializer};

/// A decimal that serializes as `"0.41"`. Trailing zeros are dropped (`0.410000` → `"0.41"`),
/// `-0` becomes `"0"`, and a measured zero stays `"0"`. `Option<Dec>::None` is JSON `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec(pub Decimal);

impl Serialize for Dec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.normalize().to_string())
    }
}

pub fn opt(d: Option<Decimal>) -> Option<Dec> {
    d.map(Dec)
}

/// One keyset page; `next_cursor` is None on the last page.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn decimals_are_strings_and_zero_is_not_null() {
        let j = |v: Option<Decimal>| serde_json::to_string(&opt(v)).unwrap_or_default();
        assert_eq!(j(Some(dec!(0.410000))), "\"0.41\"");
        assert_eq!(j(Some(dec!(0))), "\"0\"");
        assert_eq!(j(Some(dec!(-0.000))), "\"0\"");
        assert_eq!(j(Some(dec!(-99.84))), "\"-99.84\"");
        assert_eq!(j(Some(dec!(1000))), "\"1000\"");
        assert_eq!(j(Some(dec!(12345678901234567.89))), "\"12345678901234567.89\"");
        assert_eq!(j(None), "null");
    }
}
```

- [ ] **Step 10: Write the query-string helpers.** Create `bins/api/src/params.rs`:

```rust
//! Query-string parsing with the spec §3 bounds. Out-of-range input is a 400, never clamped.
use crate::error::ApiError;
use axum::extract::Query;
use axum::extract::rejection::QueryRejection;
use std::collections::HashMap;

pub type Params = HashMap<String, String>;

pub const MAX_HOURS: i64 = 168;
pub const MAX_RANGE_MS: i64 = 7 * 86_400_000;
pub const MAX_LIMIT: i64 = 200;
pub const DEFAULT_LIMIT: i64 = 50;

fn bad(code: &'static str, detail: &str) -> ApiError {
    ApiError::BadRequest(code, detail.to_string())
}

pub fn query(q: Result<Query<Params>, QueryRejection>) -> Result<Params, ApiError> {
    q.map(|Query(p)| p)
        .map_err(|_| bad("bad_query", "The query string could not be read."))
}

pub fn opt_i64(q: &Params, name: &str, code: &'static str) -> Result<Option<i64>, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|_| bad(code, &format!("{name} must be an integer."))),
    }
}

pub fn hours(q: &Params, default: i64) -> Result<i64, ApiError> {
    let h = opt_i64(q, "hours", "bad_hours")?.unwrap_or(default);
    if (1..=MAX_HOURS).contains(&h) {
        Ok(h)
    } else {
        Err(bad("bad_hours", "hours must be between 1 and 168. A week is plenty."))
    }
}

pub fn limit(q: &Params) -> Result<i64, ApiError> {
    let l = opt_i64(q, "limit", "bad_limit")?.unwrap_or(DEFAULT_LIMIT);
    if (1..=MAX_LIMIT).contains(&l) {
        Ok(l)
    } else {
        Err(bad("bad_limit", "limit must be between 1 and 200."))
    }
}

pub fn flag(q: &Params, name: &str) -> Result<bool, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") | Some("0") | Some("false") => Ok(false),
        Some("1") | Some("true") => Ok(true),
        Some(_) => Err(bad("bad_flag", &format!("{name} must be 1 or 0."))),
    }
}

/// A short text parameter without control characters; empty counts as absent.
pub fn text<'a>(q: &'a Params, name: &str, max_len: usize, code: &'static str) -> Result<Option<&'a str>, ApiError> {
    match q.get(name).map(String::as_str) {
        None | Some("") => Ok(None),
        Some(s) if s.len() <= max_len && !s.chars().any(char::is_control) => Ok(Some(s)),
        Some(_) => Err(bad(code, &format!("{name} is too long or has control characters."))),
    }
}

/// `from`/`to` in ms since epoch. Defaults: to = now, from = to − 7 days. Needs 0 < to − from ≤ 7 days.
pub fn range(q: &Params, now_ms: i64) -> Result<(i64, i64), ApiError> {
    let to = opt_i64(q, "to", "bad_range")?.unwrap_or(now_ms);
    let from = opt_i64(q, "from", "bad_range")?.unwrap_or(to.saturating_sub(MAX_RANGE_MS));
    match to.checked_sub(from) {
        Some(span) if span > 0 && span <= MAX_RANGE_MS => Ok((from, to)),
        _ => Err(bad("bad_range", "from must be before to, and seven days apart at most.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn bounds_are_errors_not_clamps() {
        assert_eq!(hours(&p(&[]), 24), Ok(24));
        assert_eq!(hours(&p(&[("hours", "168")]), 24), Ok(168));
        assert!(hours(&p(&[("hours", "169")]), 24).is_err());
        assert_eq!(limit(&p(&[])), Ok(50));
        assert!(limit(&p(&[("limit", "201")])).is_err());
        assert!(limit(&p(&[("limit", "0")])).is_err());
        assert_eq!(flag(&p(&[("net", "1")]), "net"), Ok(true));
        assert!(flag(&p(&[("net", "yes")]), "net").is_err());
        assert!(text(&p(&[("kind", "a\nb")]), "kind", 32, "bad_kind").is_err());
    }

    #[test]
    fn ranges_default_to_the_last_seven_days_and_reject_longer_ones() {
        assert_eq!(range(&p(&[]), 1_000_000_000), Ok((1_000_000_000 - MAX_RANGE_MS, 1_000_000_000)));
        assert_eq!(range(&p(&[("from", "10"), ("to", "20")]), 0), Ok((10, 20)));
        assert!(range(&p(&[("from", "20"), ("to", "10")]), 0).is_err());
        assert!(range(&p(&[("from", "0"), ("to", &(MAX_RANGE_MS + 1).to_string())]), 0).is_err());
        assert!(range(&p(&[("from", "-9223372036854775808"), ("to", "9223372036854775807")]), 0).is_err());
    }
}
```

- [ ] **Step 11: Write the health endpoints.** Create `bins/api/src/health.rs`:

```rust
//! GET /api/health/now and GET /api/health/series?hours= (spec §4).
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use store::{HealthNow, HealthRow};

const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Serialize)]
pub struct HealthNowDto {
    pub now_ms: i64,
    /// now − minute_ms (server clock).
    pub age_ms: i64,
    pub run_id: i64,
    pub minute_ms: i64,
    pub tokens: i32,
    pub shards: i32,
    pub parked_shards: i32,
    /// Messages in that minute.
    pub msgs: i64,
    pub drops: i32,
    pub drops_5m: i64,
    pub reconnects: i32,
    pub lag_p50_ms: Option<i32>,
    pub lag_p99_ms: Option<i32>,
    pub rest_p50_ms: Option<i32>,
    pub rest_p99_ms: Option<i32>,
    pub clock_offset_ms: Option<i32>,
    pub ok: Option<bool>,
    pub capture_paused: bool,
    pub dropped_groups: i32,
}

impl HealthNowDto {
    fn new(h: HealthNow, now: i64) -> Self {
        let r = h.row;
        Self {
            now_ms: now,
            age_ms: now.saturating_sub(r.minute_ms).max(0),
            run_id: r.run_id,
            minute_ms: r.minute_ms,
            tokens: r.tokens,
            shards: r.shards,
            parked_shards: r.parked_shards,
            msgs: r.msgs,
            drops: r.drops,
            drops_5m: h.drops_5m,
            reconnects: r.reconnects,
            lag_p50_ms: r.lag_p50_ms,
            lag_p99_ms: r.lag_p99_ms,
            rest_p50_ms: r.rest_p50_ms,
            rest_p99_ms: r.rest_p99_ms,
            clock_offset_ms: r.clock_offset_ms,
            ok: r.ok,
            capture_paused: r.capture_paused,
            dropped_groups: r.dropped_groups,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HealthEnvelope {
    pub health: Option<HealthNowDto>,
}

#[derive(Debug, Serialize)]
pub struct HealthPointDto {
    pub minute_ms: i64,
    pub run_id: i64,
    pub msgs: i64,
    pub drops: i32,
    pub reconnects: i32,
    pub lag_p50_ms: Option<i32>,
    pub lag_p99_ms: Option<i32>,
    pub shards: i32,
    pub parked_shards: i32,
    pub capture_paused: bool,
}

impl From<HealthRow> for HealthPointDto {
    fn from(r: HealthRow) -> Self {
        Self {
            minute_ms: r.minute_ms,
            run_id: r.run_id,
            msgs: r.msgs,
            drops: r.drops,
            reconnects: r.reconnects,
            lag_p50_ms: r.lag_p50_ms,
            lag_p99_ms: r.lag_p99_ms,
            shards: r.shards,
            parked_shards: r.parked_shards,
            capture_paused: r.capture_paused,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SeriesDto {
    pub from_ms: i64,
    pub to_ms: i64,
    pub points: Vec<HealthPointDto>,
}

pub async fn now(State(st): State<AppState>) -> Result<Json<HealthEnvelope>, ApiError> {
    let now = now_ms();
    let h = st.store.health_now(now).await?;
    Ok(Json(HealthEnvelope { health: h.map(|h| HealthNowDto::new(h, now)) }))
}

pub async fn series(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<SeriesDto>, ApiError> {
    let q = params::query(q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * HOUR_MS;
    let points = st
        .store
        .health_series(from_ms, to_ms)
        .await?
        .into_iter()
        .map(HealthPointDto::from)
        .collect();
    Ok(Json(SeriesDto { from_ms, to_ms, points }))
}
```

- [ ] **Step 12: Write the library root.** Create `bins/api/src/lib.rs`:

```rust
//! api: the dashboard's read-only JSON + SSE server (Phase 4 spec §4). Reads Postgres only through
//! `store`, as analyze_ro, on read-only sessions; exposes no write endpoint; paper only (spec §16).
#![forbid(unsafe_code)]

pub mod config;
pub mod dto;
pub mod error;
pub mod guard;
pub mod health;
pub mod params;

use axum::Router;
use axum::routing::get;
use std::sync::Arc;
use store::Store;
use tokio::sync::{Semaphore, broadcast};

/// Most concurrent SSE clients (one owner, a few tabs); more get a 503.
pub const MAX_STREAMS: usize = 16;

/// What the shared LISTEN bridge fans out to stream clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// The scanner's `NOTIFY episode` payload (an episode id).
    Episode(String),
    /// A new health minute appeared (its minute_ms).
    Health(i64),
}

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub events: broadcast::Sender<StreamEvent>,
    pub stream_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let (events, _) = broadcast::channel(256);
        Self { store, events, stream_slots: Arc::new(Semaphore::new(MAX_STREAMS)) }
    }
}

/// Wall-clock milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Spec §3 "The API can't write": refuse to serve with a role that can INSERT.
pub async fn ensure_read_only(store: &Store) -> anyhow::Result<()> {
    anyhow::ensure!(
        !store.role_can_write().await?,
        "refusing to start: the database role can write; connect as analyze_ro"
    );
    Ok(())
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health/now", get(health::now))
        .route("/api/health/series", get(health::series))
        .fallback(error::not_found)
        .with_state(state)
}
```

- [ ] **Step 13: Write the binary.** Create `bins/api/src/main.rs`:

```rust
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("api: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    api::guard::check_env()?;
    let cfg = api::config::Config::from_env()?;
    let opts = store::Store::options(&cfg.database_url)?;
    let store = store::Store::connect_read_only(opts, cfg.max_conns, cfg.statement_timeout_ms, None).await?;
    api::ensure_read_only(&store).await?;
    let state = api::AppState::new(store.clone());
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    eprintln!("api: listening on {} (read-only, paper only)", cfg.bind);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    store.close().await;
    Ok(())
}

/// SIGTERM (docker stop) or Ctrl-C.
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
```

- [ ] **Step 14: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api && cargo test -p scanner --test arch`
Expected: PASS: 5 tests in `health.rs`, unit tests in `config`, `guard`, `dto`, `params`; `crate_graph_matches_spec_9_2` passes with the new api assertions.

- [ ] **Step 15: Lint and supply-chain check.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && (cargo deny --version && cargo deny check bans advisories || echo "cargo-deny not installed locally: CI runs it")`
Expected: no clippy warnings; `bans ok`, `advisories ok` (axum 0.8, tower 0.5 and their hyper/matchit/serde_urlencoded deps hit no ban; sqlx still only under `store`). Do not `cargo install cargo-deny` locally (a release build; CI installs it).

- [ ] **Step 16: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add Cargo.toml Cargo.lock bins/api/Cargo.toml bins/api/src/lib.rs bins/api/src/main.rs bins/api/src/config.rs bins/api/src/guard.rs bins/api/src/error.rs bins/api/src/dto.rs bins/api/src/params.rs bins/api/src/health.rs bins/api/tests/common/mod.rs bins/api/tests/health.rs bins/scanner/tests/arch.rs && git commit -m "api: read-only axum skeleton, JSON errors, decimal strings, health endpoints"
```

---

### Task 4: api: episodes endpoints (open, keyset list, detail, today)

**Files:**
- Create: `bins/api/src/episodes.rs`
- Modify: `bins/api/src/lib.rs` (module + 4 routes)
- Test: `bins/api/tests/episodes.rs`

**Interfaces:**
- Consumes: Task 1 `Store::{open_episodes, episodes_page, episode, today, episode_legs, observations_for}`, `EpisodeSummary`, `EpisodeFilter`, `EpisodeRow`, `EpisodeLegRow`, `ObservationRow`, `Today`; Task 3 `ApiError`, `Dec`, `dto::opt`, `Page`, `params::*`, `AppState`, `now_ms`, test helpers.
- Produces (JSON; the web types in Task 7 copy these names exactly):
  - `GET /api/episodes/open` → `{now_ms, items: [EpisodeSummaryDto]}` where `EpisodeSummaryDto = {id, kind, realization, event_title, event_slug, n_legs, s_legs, opened_ms, closed_ms, duration_ms, edge, q_star, net, gross, suspect, stale, censored}` (edge/q_star/net/gross are decimal strings)
  - `GET /api/episodes?kind=&from=&to=&net=1&flags=suspect,stale,censored&cursor=&limit=` → `{items: [EpisodeSummaryDto], next_cursor}`; cursor format `"{opened_ms}_{episode_id}"`
  - `GET /api/episodes/{id}` → `{episode: EpisodeDto, legs: [LegDto], observations: [ObservationDto]}` (observations sorted by offset_ms)
  - `GET /api/today` → `{day_start_ms, gross, net, hourly: [{hour, gross, net}] × 24}`
  - `episodes::KINDS: [&str; 4]`, `episodes::encode_cursor(i64, &str) -> String`, `episodes::decode_cursor(&str) -> Option<(i64, String)>`, `episodes::valid_id(&str) -> bool`

- [ ] **Step 1: Write the failing tests.** Create `bins/api/tests/episodes.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use rust_decimal_macros::dec;
use store::*;

fn w(row: EpisodeRow) -> EpisodeWrite {
    EpisodeWrite { row, legs: vec![] }
}

async fn seed(owner: &Store, episodes: Vec<EpisodeWrite>) {
    owner.write_batch(&Batch { episodes, ..Batch::default() }).await.unwrap();
}

#[tokio::test]
async fn open_lists_open_live_episodes_with_money_as_strings() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let now = common::now();
    let mut open = common::ep("live-1-00000001", live, now - 4_200);
    open.closed_server_ms = None;
    open.duration_ms = None;
    let closed = common::ep("live-1-00000002", live, now - 9_000);
    seed(
        &owner,
        vec![
            EpisodeWrite {
                row: open,
                legs: vec![common::leg("live-1-00000001", 0, true), common::leg("live-1-00000001", 1, true)],
            },
            w(closed),
        ],
    )
    .await;
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/episodes/open").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["now_ms"].is_i64());
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let e = &items[0];
    assert_eq!(e["id"], "live-1-00000001");
    for (k, want) in [("net", "0.41"), ("gross", "1.37"), ("q_star", "83"), ("edge", "0.0049")] {
        assert!(e[k].is_string(), "{k} must be a JSON string: {e}");
        assert_eq!(e[k], want);
    }
    assert_eq!(e["s_legs"], 2);
    assert!(e["closed_ms"].is_null());
    assert_eq!(e["suspect"], false);
    db.drop(owner).await;
}

#[tokio::test]
async fn zero_and_null_stay_distinct_in_the_detail() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let mut row = common::ep("live-1-00000003", live, 1_000);
    row.peak_fees_usd = dec!(0);
    row.peak_profit_per_dollar = Some(dec!(0.000));
    row.annualized_return = None;
    row.max_seconds_delay = Some(3);
    let mut l0 = common::leg("live-1-00000003", 0, true);
    l0.best_price = None;
    l0.fees = dec!(0);
    let l1 = common::leg("live-1-00000003", 1, false);
    let obs = |offset_ms: i32, edge: Option<rust_decimal::Decimal>, skipped: bool| ObservationRow {
        episode_id: "live-1-00000003".into(),
        offset_ms,
        source: "rest".into(),
        observed_ms: (!skipped).then_some(1_000 + i64::from(offset_ms)),
        skipped,
        skip_reason: skipped.then(|| "recheck capacity".to_string()),
        edge,
        q_max: edge.map(|_| dec!(61)),
        pnl: edge.map(|_| dec!(0)),
        still_profitable: edge.map(|e| e > dec!(0)),
        fill_at_original_size: None,
    };
    owner
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite { row, legs: vec![l0, l1] }],
            observations: vec![obs(3_000, Some(dec!(-0.0008)), false), obs(1_000, Some(dec!(0.0022)), false), obs(5_000, None, true)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/episodes/live-1-00000003").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let e = &v["episode"];
    assert_eq!(e["fees"], "0");
    assert_eq!(e["profit_per_dollar"], "0");
    assert!(e["annualized_return"].is_null());
    assert_eq!(e["net"], "0.41");
    assert_eq!(e["max_seconds_delay"], 3);
    let legs = v["legs"].as_array().unwrap();
    assert_eq!(legs.len(), 2);
    assert!(legs[0]["best_price"].is_null());
    assert_eq!(legs[0]["fees"], "0");
    assert_eq!(legs[1]["in_s"], false);
    let o = v["observations"].as_array().unwrap();
    assert_eq!(o.iter().map(|x| x["offset_ms"].as_i64().unwrap()).collect::<Vec<_>>(), vec![1_000, 3_000, 5_000]);
    assert_eq!(o[0]["edge"], "0.0022");
    assert_eq!(o[0]["pnl"], "0");
    assert_eq!(o[1]["still_profitable"], false);
    assert!(o[2]["edge"].is_null());
    assert_eq!(o[2]["skipped"], true);
    db.drop(owner).await;
}

#[tokio::test]
async fn list_pages_with_cursor_until_the_end() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let t = common::now() - 1_000;
    seed(&owner, (0..5).map(|i| w(common::ep(&format!("live-1-{i:08}"), live, t))).collect()).await;
    let (app, _st) = common::app(&db).await;
    let mut uri = "/api/episodes?limit=2".to_string();
    let mut ids = Vec::new();
    for _ in 0..5 {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        ids.extend(v["items"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()));
        match v["next_cursor"].as_str() {
            Some(c) => uri = format!("/api/episodes?limit=2&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(ids, ["live-1-00000004", "live-1-00000003", "live-1-00000002", "live-1-00000001", "live-1-00000000"]);
    db.drop(owner).await;
}

#[tokio::test]
async fn list_filters_by_kind_net_and_flags() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let t = common::now() - 60_000;
    let a = common::ep("live-1-a", live, t);
    let mut b = common::ep("live-1-b", live, t + 1);
    b.kind = "yes_basket".into();
    b.peak_pnl_net_usd = dec!(-0.12);
    b.engine_suspect = true;
    let mut c = common::ep("live-1-c", live, t + 2);
    c.censor_left = true;
    seed(&owner, vec![w(a), w(b), w(c)]).await;
    let (app, _st) = common::app(&db).await;
    let ids = |v: serde_json::Value| {
        v["items"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    assert_eq!(ids(common::get(&app, "/api/episodes").await.1), ["live-1-c", "live-1-b", "live-1-a"]);
    assert_eq!(ids(common::get(&app, "/api/episodes?kind=yes_basket").await.1), ["live-1-b"]);
    assert_eq!(ids(common::get(&app, "/api/episodes?net=1").await.1), ["live-1-c", "live-1-a"]);
    assert_eq!(ids(common::get(&app, "/api/episodes?flags=suspect").await.1), ["live-1-b"]);
    assert_eq!(ids(common::get(&app, "/api/episodes?flags=censored").await.1), ["live-1-c"]);
    assert!(ids(common::get(&app, "/api/episodes?flags=suspect,censored").await.1).is_empty());
    let (_, v) = common::get(&app, "/api/episodes?kind=no_basket").await;
    assert_eq!(v["items"][0]["censored"], true);
    db.drop(owner).await;
}

#[tokio::test]
async fn bad_filters_are_json_400s() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _st) = common::app(&db).await;
    for (q, code) in [
        ("kind=nope", "bad_kind"),
        ("limit=0", "bad_limit"),
        ("limit=201", "bad_limit"),
        ("limit=99999999999999999999", "bad_limit"),
        ("from=2000&to=1000", "bad_range"),
        ("from=0&to=691200000", "bad_range"),
        ("from=abc", "bad_range"),
        ("flags=x", "bad_flags"),
        ("flags=suspect,,stale", "bad_flags"),
        ("cursor=garbage", "bad_cursor"),
        ("cursor=12_bad%20id", "bad_cursor"),
        ("net=maybe", "bad_flag"),
    ] {
        let (status, v) = common::get(&app, &format!("/api/episodes?{q}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q}: {v}");
        assert_eq!(v["error"], code, "{q}");
        assert!(v["detail"].is_string());
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn hostile_ids_are_404s() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _st) = common::app(&db).await;
    let long = "a".repeat(10_000);
    for id in ["%27%3B%20DROP%20TABLE%20episodes%3B--", long.as_str(), "nope", "..%2F..%2Fetc"] {
        let (status, v) = common::get(&app, &format!("/api/episodes/{id}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
        assert_eq!(v["error"], "not_found");
    }
    assert!(owner.episodes_for_run(1).await.unwrap().is_empty(), "episodes table still exists");
    db.drop(owner).await;
}

#[tokio::test]
async fn today_counts_via_the_api() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let day = n - n.rem_euclid(DAY_MS);
    let mut gross_only = common::ep("live-1-g", live, day + 2_000);
    gross_only.peak_pnl_net_usd = dec!(0);
    seed(&owner, vec![w(common::ep("live-1-n", live, day + 1_000)), w(gross_only)]).await;
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/today").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((v["gross"].as_i64(), v["net"].as_i64()), (Some(2), Some(1)));
    assert_eq!(v["day_start_ms"], day);
    let hourly = v["hourly"].as_array().unwrap();
    assert_eq!(hourly.len(), 24);
    assert_eq!((hourly[0]["hour"].as_i64(), hourly[0]["gross"].as_i64(), hourly[0]["net"].as_i64()), (Some(0), Some(2), Some(1)));
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api --test episodes`
Expected: FAIL: every request returns 404 `not_found` (routes missing), e.g. `assertion left == right failed: left: 404, right: 200`.

- [ ] **Step 3: Write the episodes module.** Create `bins/api/src/episodes.rs`:

```rust
//! Episodes (spec §4): open (Radar), keyset-paged list (Episodes), detail, today's counts.
use crate::dto::{Dec, Page, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use serde::Serialize;
use store::{EpisodeFilter, EpisodeLegRow, EpisodeRow, EpisodeSummary, ObservationRow, Today};

pub const KINDS: [&str; 4] = ["yes_basket", "yes_basket_incomplete", "no_basket", "implication"];

/// Episode ids are scanner-made (`live-7-00001234`): short, ASCII, no spaces.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn encode_cursor(opened_ms: i64, id: &str) -> String {
    format!("{opened_ms}_{id}")
}

pub fn decode_cursor(s: &str) -> Option<(i64, String)> {
    let (ms, id) = s.split_once('_')?;
    let ms = ms.parse::<i64>().ok()?;
    valid_id(id).then(|| (ms, id.to_string()))
}

#[derive(Debug, Serialize)]
pub struct EpisodeSummaryDto {
    pub id: String,
    pub kind: String,
    pub realization: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub n_legs: i32,
    pub s_legs: i64,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub edge: Dec,
    pub q_star: Dec,
    pub net: Dec,
    pub gross: Dec,
    pub suspect: bool,
    pub stale: bool,
    pub censored: bool,
}

impl From<EpisodeSummary> for EpisodeSummaryDto {
    fn from(e: EpisodeSummary) -> Self {
        Self {
            id: e.episode_id,
            kind: e.kind,
            realization: e.realization,
            event_title: e.event_title,
            event_slug: e.event_slug,
            n_legs: e.n_legs,
            s_legs: e.s_legs,
            opened_ms: e.opened_server_ms,
            closed_ms: e.closed_server_ms,
            duration_ms: e.duration_ms,
            edge: Dec(e.peak_edge_top),
            q_star: Dec(e.peak_max_size),
            net: Dec(e.peak_pnl_net_usd),
            gross: Dec(e.peak_pnl_gross_usd),
            suspect: e.engine_suspect,
            stale: e.stale_transport,
            censored: e.censored,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OpenDto {
    pub now_ms: i64,
    pub items: Vec<EpisodeSummaryDto>,
}

#[derive(Debug, Serialize)]
pub struct EpisodeDto {
    pub id: String,
    pub run_id: i64,
    pub source: String,
    pub kind: String,
    pub realization: String,
    pub event_id: String,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub category: Option<String>,
    pub n_outcomes: i32,
    pub n_legs: i32,
    pub legs_dropped: i32,
    pub augmented: bool,
    pub incomplete_set: bool,
    pub uncovered_legs: i32,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    pub detect_lag_ms: Option<i64>,
    pub n_updates: i32,
    pub peak_ms: i64,
    pub edge: Dec,
    pub edge_gross: Dec,
    pub q_star: Dec,
    pub cost: Dec,
    pub payout: Dec,
    pub fees: Dec,
    pub net: Dec,
    pub gross: Dec,
    pub profit_per_dollar: Option<Dec>,
    pub net_budget100: Option<Dec>,
    pub bottleneck_leg_idx: Option<i32>,
    pub below_min_size: Option<bool>,
    pub max_seconds_delay: Option<i32>,
    pub days_to_end: Option<Dec>,
    pub annualized_return: Option<Dec>,
    pub post_end: Option<bool>,
    pub suspect: bool,
    pub stale: bool,
    pub open_lag_ms: Option<i64>,
    pub oracle_downgraded: Option<bool>,
    pub scanner_version: String,
}

impl From<EpisodeRow> for EpisodeDto {
    fn from(e: EpisodeRow) -> Self {
        Self {
            id: e.episode_id,
            run_id: e.run_id,
            source: e.source,
            kind: e.kind,
            realization: e.realization,
            event_id: e.event_id,
            event_title: e.event_title,
            event_slug: e.event_slug,
            category: e.category,
            n_outcomes: e.n_outcomes,
            n_legs: e.n_legs,
            legs_dropped: e.legs_dropped,
            augmented: e.augmented,
            incomplete_set: e.incomplete_set,
            uncovered_legs: e.uncovered_legs,
            opened_ms: e.opened_server_ms,
            closed_ms: e.closed_server_ms,
            duration_ms: e.duration_ms,
            censor_left: e.censor_left,
            censor_right: e.censor_right,
            censor_gap: e.censor_gap,
            detect_lag_ms: e.detect_lag_ms,
            n_updates: e.n_updates,
            peak_ms: e.peak_server_ms,
            edge: Dec(e.peak_edge_top),
            edge_gross: Dec(e.peak_edge_top_gross),
            q_star: Dec(e.peak_max_size),
            cost: Dec(e.peak_cost_usd),
            payout: Dec(e.peak_payout_usd),
            fees: Dec(e.peak_fees_usd),
            net: Dec(e.peak_pnl_net_usd),
            gross: Dec(e.peak_pnl_gross_usd),
            profit_per_dollar: opt(e.peak_profit_per_dollar),
            net_budget100: opt(e.pnl_net_budget100_usd),
            bottleneck_leg_idx: e.bottleneck_leg_idx,
            below_min_size: e.below_min_size,
            max_seconds_delay: e.max_seconds_delay,
            days_to_end: opt(e.days_to_end),
            annualized_return: opt(e.annualized_return),
            post_end: e.post_end,
            suspect: e.engine_suspect,
            stale: e.stale_transport,
            open_lag_ms: e.open_lag_ms,
            oracle_downgraded: e.oracle_downgraded,
            scanner_version: e.scanner_version,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LegDto {
    pub leg_idx: i32,
    pub market_id: String,
    /// buy_yes | buy_no
    pub action: String,
    pub best_price: Option<Dec>,
    pub vwap: Option<Dec>,
    pub levels: i32,
    pub size_at_best: Option<Dec>,
    pub filled: Dec,
    pub notional: Dec,
    pub fees: Dec,
    pub in_s: bool,
}

impl From<EpisodeLegRow> for LegDto {
    fn from(l: EpisodeLegRow) -> Self {
        Self {
            leg_idx: l.leg_idx,
            market_id: l.market_id,
            action: l.action,
            best_price: opt(l.best_price),
            vwap: opt(l.vwap),
            levels: l.levels,
            size_at_best: opt(l.size_at_best),
            filled: Dec(l.filled),
            notional: Dec(l.notional),
            fees: Dec(l.fees),
            in_s: l.in_s,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ObservationDto {
    pub offset_ms: i32,
    /// ws | rest
    pub source: String,
    pub observed_ms: Option<i64>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub edge: Option<Dec>,
    pub q_star: Option<Dec>,
    pub pnl: Option<Dec>,
    pub still_profitable: Option<bool>,
    pub fill_at_original_size: Option<bool>,
}

impl From<ObservationRow> for ObservationDto {
    fn from(o: ObservationRow) -> Self {
        Self {
            offset_ms: o.offset_ms,
            source: o.source,
            observed_ms: o.observed_ms,
            skipped: o.skipped,
            skip_reason: o.skip_reason,
            edge: opt(o.edge),
            q_star: opt(o.q_max),
            pnl: opt(o.pnl),
            still_profitable: o.still_profitable,
            fill_at_original_size: o.fill_at_original_size,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DetailDto {
    pub episode: EpisodeDto,
    pub legs: Vec<LegDto>,
    pub observations: Vec<ObservationDto>,
}

#[derive(Debug, Serialize)]
pub struct HourDto {
    pub hour: i64,
    pub gross: i64,
    pub net: i64,
}

#[derive(Debug, Serialize)]
pub struct TodayDto {
    pub day_start_ms: i64,
    pub gross: i64,
    pub net: i64,
    pub hourly: Vec<HourDto>,
}

impl From<Today> for TodayDto {
    fn from(t: Today) -> Self {
        Self {
            day_start_ms: t.day_start_ms,
            gross: t.gross,
            net: t.net,
            hourly: (0i64..)
                .zip(t.hourly)
                .map(|(hour, (gross, net))| HourDto { hour, gross, net })
                .collect(),
        }
    }
}

fn bad(code: &'static str, detail: &str) -> ApiError {
    ApiError::BadRequest(code, detail.to_string())
}

pub async fn open(State(st): State<AppState>) -> Result<Json<OpenDto>, ApiError> {
    let items = st.store.open_episodes().await?.into_iter().map(Into::into).collect();
    Ok(Json(OpenDto { now_ms: now_ms(), items }))
}

pub async fn list(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<Page<EpisodeSummaryDto>>, ApiError> {
    let q = params::query(q)?;
    let kind = match params::text(&q, "kind", 32, "bad_kind")? {
        None => None,
        Some(k) if KINDS.contains(&k) => Some(k.to_string()),
        Some(_) => {
            return Err(bad(
                "bad_kind",
                "kind must be one of yes_basket, yes_basket_incomplete, no_basket, implication.",
            ));
        }
    };
    let (from_ms, to_ms) = params::range(&q, now_ms())?;
    let (mut suspect, mut stale, mut censored) = (false, false, false);
    if let Some(flags) = params::text(&q, "flags", 64, "bad_flags")? {
        for f in flags.split(',') {
            match f {
                "suspect" => suspect = true,
                "stale" => stale = true,
                "censored" => censored = true,
                _ => return Err(bad("bad_flags", "flags is a comma list of suspect, stale, censored.")),
            }
        }
    }
    let before = match params::text(&q, "cursor", 100, "bad_cursor")? {
        None => None,
        Some(c) => Some(decode_cursor(c).ok_or_else(|| bad("bad_cursor", "That cursor is not one of ours."))?),
    };
    let filter = EpisodeFilter {
        kind,
        from_ms,
        to_ms,
        net_positive: params::flag(&q, "net")?,
        suspect,
        stale,
        censored,
        before,
        limit: params::limit(&q)?,
    };
    let page = st.store.episodes_page(&filter).await?;
    Ok(Json(Page {
        items: page.rows.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|(ms, id)| encode_cursor(ms, &id)),
    }))
}

pub async fn detail(
    State(st): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Json<DetailDto>, ApiError> {
    let Ok(Path(id)) = id else { return Err(ApiError::NotFound) };
    if !valid_id(&id) {
        return Err(ApiError::NotFound);
    }
    let episode = st.store.episode(&id).await?.ok_or(ApiError::NotFound)?;
    let legs = st.store.episode_legs(&id).await?;
    let mut observations = st.store.observations_for(&id).await?;
    observations.sort_by_key(|o| o.offset_ms);
    Ok(Json(DetailDto {
        episode: episode.into(),
        legs: legs.into_iter().map(Into::into).collect(),
        observations: observations.into_iter().map(Into::into).collect(),
    }))
}

pub async fn today(State(st): State<AppState>) -> Result<Json<TodayDto>, ApiError> {
    Ok(Json(st.store.today(now_ms()).await?.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_junk() {
        let c = encode_cursor(1_790_000_000_000, "live-7-00001234");
        assert_eq!(decode_cursor(&c), Some((1_790_000_000_000, "live-7-00001234".to_string())));
        assert_eq!(decode_cursor("garbage"), None);
        assert_eq!(decode_cursor("x_live-1"), None);
        assert_eq!(decode_cursor("12_bad id"), None);
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("'; DROP"));
    }
}
```

- [ ] **Step 4: Route it.** In `bins/api/src/lib.rs` add `pub mod episodes;` after `pub mod dto;`, and in `router` add after the health routes:

```rust
        .route("/api/episodes/open", get(episodes::open))
        .route("/api/episodes", get(episodes::list))
        .route("/api/episodes/{id}", get(episodes::detail))
        .route("/api/today", get(episodes::today))
```

- [ ] **Step 5: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api`
Expected: PASS (7 tests in `episodes.rs`, the cursor unit test, and Task 3's tests).

- [ ] **Step 6: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p api --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add bins/api/src/episodes.rs bins/api/src/lib.rs bins/api/tests/episodes.rs && git commit -m "api: episodes endpoints (open, keyset list with bounded filters, detail, today)"
```

---

### Task 5: api: integrity summary and paper endpoints (empty until S3c's tables exist)

**Files:**
- Create: `bins/api/src/integrity.rs`, `bins/api/src/paper.rs`
- Modify: `bins/api/src/lib.rs` (modules + 5 routes)
- Test: `bins/api/tests/integrity.rs`, `bins/api/tests/paper.rs`

**Interfaces:**
- Consumes: Task 1 `Store::integrity_summary`, `IntegritySummary`; Task 2 `Store::{paper_accounts_now, paper_equity, paper_positions, paper_ledger_page}`, `PaperAccountNow`, `EquityPoint`, `PaperPosition`, `PaperLedgerEntry`, `LedgerPage`; Task 3 helpers.
- Produces (JSON):
  - `GET /api/integrity/summary?hours=` → `{from_ms, to_ms, by_kind: [{kind, count}], total, msgs, per_million}` (`per_million`: decimal string or null)
  - `GET /api/paper/accounts` → `{items: [{account_id, policy, scenario, fee_bips, bankroll, run_id, marked_ms, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown, n_positions}]}` (money fields decimal strings; mark fields null before the first mark)
  - `GET /api/paper/equity?account=&hours=` → `{account_id, from_ms, to_ms, points: [{t_ms, equity}]}`
  - `GET /api/paper/positions` → `{items: [{account_id, market_id, outcome, qty, basis, returned, event_title}]}`
  - `GET /api/paper/ledger?account=&cursor=&limit=` → `{items: [{run_id, account_id, seq, t_ms, kind, episode_id, market_id, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity}], next_cursor}`; cursor `"{run_id}_{seq}"`

- [ ] **Step 1: Write the failing tests.** Create `bins/api/tests/integrity.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

#[tokio::test]
async fn integrity_summary_counts_kinds_and_rates_per_million() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let row = |kind: &str| IntegrityRow {
        run_id: live,
        ts_ms: n - 1_000,
        kind: kind.into(),
        shard: None,
        token: None,
        event_id: None,
        detail: None,
    };
    let (app, _st) = common::app(&db).await;
    let (_, empty) = common::get(&app, "/api/integrity/summary").await;
    assert_eq!(empty["total"], 0);
    assert!(empty["per_million"].is_null());
    owner
        .write_batch(&Batch {
            integrity: vec![row("top_mismatch"), row("top_mismatch"), row("drift")],
            health: vec![common::health(live, n - n % 60_000 - 60_000, 0)],
            ..Batch::default()
        })
        .await
        .unwrap();
    let (status, v) = common::get(&app, "/api/integrity/summary?hours=24").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["by_kind"][0]["kind"], "top_mismatch");
    assert_eq!(v["by_kind"][0]["count"], 2);
    assert_eq!(v["by_kind"][1]["kind"], "drift");
    assert_eq!((v["total"].as_i64(), v["msgs"].as_i64()), (Some(3), Some(258_000)));
    assert!(v["per_million"].is_string());
    assert_eq!(v["per_million"], "11.63");
    let (status, v) = common::get(&app, "/api/integrity/summary?hours=0").await;
    assert_eq!((status, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_hours")));
    db.drop(owner).await;
}
```

Create `bins/api/tests/paper.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

const P0: &str = "P0-converter/sp-idle";
const P0Q: &str = "P0-converter%2Fsp-idle";

async fn account(owner: &Store, run_id: i64, id: &str, policy: &str, scenario: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run_id}, '{id}', '{policy}', '{scenario}', 0, 1000, 100, 'sha')"
        ))
        .await
        .unwrap();
}

async fn mark(owner: &Store, run_id: i64, id: &str, t_ms: i64, equity: &str) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, max_drawdown, n_positions) \
             VALUES ({run_id}, '{id}', {t_ms}, 900, {equity}, 2.56, 0, 0, 95.000000, 0.40, 2)"
        ))
        .await
        .unwrap();
}

async fn ledger(owner: &Store, run_id: i64, id: &str, seq: i64) {
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run_id}, '{id}', {seq}, {}, 'buy', 'live-1-00000001', 'm1', 'y1', 'no', 51.2, 0.62, -99.84, 0.21, 0, 900.16, NULL)",
            seq * 1_000
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn paper_endpoints_are_empty_lists_when_the_tables_are_missing() {
    let Some((owner, db)) = common::fresh().await else { return };
    owner
        .raw_exec(
            "DROP TABLE paper_markouts, paper_fills, paper_execs, paper_intents, paper_ledger, \
             paper_marks, paper_state, paper_accounts CASCADE",
        )
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    for uri in ["/api/paper/accounts".to_string(), "/api/paper/positions".to_string()] {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {v}");
        assert_eq!(v["items"], serde_json::json!([]));
    }
    let (status, v) = common::get(&app, &format!("/api/paper/equity?account={P0Q}")).await;
    assert_eq!((status, v["points"].clone()), (StatusCode::OK, serde_json::json!([])));
    let (status, v) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["items"], serde_json::json!([]));
    assert!(v["next_cursor"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn accounts_equity_and_ledger_carry_money_as_strings() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    account(&owner, live, P0, "P0-converter", "sp-idle").await;
    mark(&owner, live, P0, n - 120_000, "1001.00").await;
    mark(&owner, live, P0, n - 60_000, "1002.56").await;
    for seq in 1..=3 {
        ledger(&owner, live, P0, seq).await;
    }
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/accounts").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let a = &v["items"][0];
    assert_eq!(a["account_id"], P0);
    for (k, want) in [("equity", "1002.56"), ("bankroll", "1000"), ("fee_bips", "0"), ("locked", "95"), ("realized", "2.56"), ("unrealized", "0")] {
        assert!(a[k].is_string(), "{k}: {a}");
        assert_eq!(a[k], want, "{k}");
    }
    assert_eq!(a["marked_ms"], n - 60_000);
    assert_eq!(a["n_positions"], 2);
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}&hours=24")).await;
    let pts = eq["points"].as_array().unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[1]["equity"], "1002.56");
    let (_, p1) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&limit=2")).await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 2);
    assert_eq!(p1["items"][0]["seq"], 3);
    assert_eq!(p1["items"][0]["cash_delta"], "-99.84");
    assert!(p1["items"][0]["equity"].is_null());
    let cursor = p1["next_cursor"].as_str().unwrap().to_string();
    assert_eq!(cursor, format!("{live}_2"));
    let (_, p2) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&limit=2&cursor={cursor}")).await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 1);
    assert!(p2["next_cursor"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_come_from_the_live_paper_state() {
    let Some((owner, db)) = common::fresh().await else { return };
    owner
        .raw_exec(
            r#"INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('P1-settlement/sp-idle', '{"positions":[[{"market_id":"m1","yes_token":"y1","outcome":"no"},{"qty":"51.2","basis":"31.74"}]],"returned":[]}', 1)"#,
        )
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/positions").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let p = &v["items"][0];
    assert_eq!((p["market_id"].as_str(), p["outcome"].as_str()), (Some("m1"), Some("no")));
    assert_eq!((p["qty"].as_str(), p["basis"].as_str()), (Some("51.2"), Some("31.74")));
    assert_eq!(p["returned"], false);
    assert!(p["event_title"].is_null());
    db.drop(owner).await;
}

#[tokio::test]
async fn paper_parameters_are_validated() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _st) = common::app(&db).await;
    let long = "a".repeat(200);
    for (uri, code) in [
        ("/api/paper/equity".to_string(), "bad_account"),
        ("/api/paper/ledger".to_string(), "bad_account"),
        (format!("/api/paper/equity?account={long}"), "bad_account"),
        (format!("/api/paper/equity?account={P0Q}&hours=0"), "bad_hours"),
        (format!("/api/paper/ledger?account={P0Q}&cursor=bad"), "bad_cursor"),
        (format!("/api/paper/ledger?account={P0Q}&limit=500"), "bad_limit"),
    ] {
        let (status, v) = common::get(&app, &uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {v}");
        assert_eq!(v["error"], code, "{uri}");
    }
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api --test integrity --test paper`
Expected: FAIL with 404s (routes missing).

- [ ] **Step 3: Write the integrity endpoint.** Create `bins/api/src/integrity.rs`:

```rust
//! GET /api/integrity/summary?hours= (spec §4): counts by kind and events per million messages.
use crate::dto::{Dec, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct KindCountDto {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct IntegrityDto {
    pub from_ms: i64,
    pub to_ms: i64,
    pub by_kind: Vec<KindCountDto>,
    pub total: i64,
    pub msgs: i64,
    pub per_million: Option<Dec>,
}

pub async fn summary(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<IntegrityDto>, ApiError> {
    let q = params::query(q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * 3_600_000;
    let s = st.store.integrity_summary(from_ms, to_ms).await?;
    Ok(Json(IntegrityDto {
        from_ms,
        to_ms,
        by_kind: s
            .by_kind
            .into_iter()
            .map(|(kind, count)| KindCountDto { kind, count })
            .collect(),
        total: s.total,
        msgs: s.msgs,
        per_million: opt(s.per_million),
    }))
}
```

- [ ] **Step 4: Write the paper endpoints.** Create `bins/api/src/paper.rs`:

```rust
//! Paper trading (spec §4) over S3c's tables. Empty lists, never errors, until they exist.
use crate::dto::{Dec, Page, opt};
use crate::error::ApiError;
use crate::params::{self, Params};
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use store::{PaperAccountNow, PaperLedgerEntry, PaperPosition};

#[derive(Debug, Serialize)]
pub struct AccountDto {
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Dec,
    pub bankroll: Dec,
    pub run_id: i64,
    pub marked_ms: Option<i64>,
    pub cash: Option<Dec>,
    pub equity: Option<Dec>,
    pub realized: Option<Dec>,
    pub unrealized: Option<Dec>,
    pub fees_paid: Option<Dec>,
    pub locked: Option<Dec>,
    pub max_drawdown: Option<Dec>,
    pub n_positions: Option<i64>,
}

impl From<PaperAccountNow> for AccountDto {
    fn from(a: PaperAccountNow) -> Self {
        Self {
            account_id: a.account_id,
            policy: a.policy,
            scenario: a.scenario,
            fee_bips: Dec(a.fee_bips),
            bankroll: Dec(a.bankroll),
            run_id: a.run_id,
            marked_ms: a.t_ms,
            cash: opt(a.cash),
            equity: opt(a.equity),
            realized: opt(a.realized),
            unrealized: opt(a.unrealized),
            fees_paid: opt(a.fees_paid),
            locked: opt(a.locked),
            max_drawdown: opt(a.max_drawdown),
            n_positions: a.n_positions,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ItemsDto<T> {
    pub items: Vec<T>,
}

#[derive(Debug, Serialize)]
pub struct EquityPointDto {
    pub t_ms: i64,
    pub equity: Dec,
}

#[derive(Debug, Serialize)]
pub struct EquityDto {
    pub account_id: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub points: Vec<EquityPointDto>,
}

#[derive(Debug, Serialize)]
pub struct PositionDto {
    pub account_id: String,
    pub market_id: String,
    pub outcome: String,
    pub qty: Dec,
    pub basis: Dec,
    pub returned: bool,
    pub event_title: Option<String>,
}

impl From<PaperPosition> for PositionDto {
    fn from(p: PaperPosition) -> Self {
        Self {
            account_id: p.account_id,
            market_id: p.market_id,
            outcome: p.outcome,
            qty: Dec(p.qty),
            basis: Dec(p.basis),
            returned: p.returned,
            event_title: p.event_title,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LedgerDto {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub outcome: Option<String>,
    pub qty: Dec,
    pub price: Option<Dec>,
    pub cash_delta: Dec,
    pub fees: Dec,
    pub realized_delta: Dec,
    pub cash_after: Dec,
    pub equity: Option<Dec>,
}

impl From<PaperLedgerEntry> for LedgerDto {
    fn from(l: PaperLedgerEntry) -> Self {
        Self {
            run_id: l.run_id,
            account_id: l.account_id,
            seq: l.seq,
            t_ms: l.t_ms,
            kind: l.kind,
            episode_id: l.episode_id,
            market_id: l.market_id,
            outcome: l.outcome,
            qty: Dec(l.qty),
            price: opt(l.price),
            cash_delta: Dec(l.cash_delta),
            fees: Dec(l.fees),
            realized_delta: Dec(l.realized_delta),
            cash_after: Dec(l.cash_after),
            equity: opt(l.equity),
        }
    }
}

fn account(q: &Params) -> Result<String, ApiError> {
    params::text(q, "account", 128, "bad_account")?
        .map(str::to_string)
        .ok_or_else(|| ApiError::BadRequest("bad_account", "account is required.".to_string()))
}

fn decode_ledger_cursor(s: &str) -> Option<(i64, i64)> {
    let (run, seq) = s.split_once('_')?;
    Some((run.parse().ok()?, seq.parse().ok()?))
}

pub async fn accounts(State(st): State<AppState>) -> Result<Json<ItemsDto<AccountDto>>, ApiError> {
    let items = st.store.paper_accounts_now().await?.into_iter().map(Into::into).collect();
    Ok(Json(ItemsDto { items }))
}

pub async fn equity(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<EquityDto>, ApiError> {
    let q = params::query(q)?;
    let account_id = account(&q)?;
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
    let from_ms = to_ms - hours * 3_600_000;
    let points = st
        .store
        .paper_equity(&account_id, from_ms, to_ms)
        .await?
        .into_iter()
        .map(|p| EquityPointDto { t_ms: p.t_ms, equity: Dec(p.equity) })
        .collect();
    Ok(Json(EquityDto { account_id, from_ms, to_ms, points }))
}

pub async fn positions(State(st): State<AppState>) -> Result<Json<ItemsDto<PositionDto>>, ApiError> {
    let items = st.store.paper_positions().await?.into_iter().map(Into::into).collect();
    Ok(Json(ItemsDto { items }))
}

pub async fn ledger(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<Page<LedgerDto>>, ApiError> {
    let q = params::query(q)?;
    let account_id = account(&q)?;
    let before = match params::text(&q, "cursor", 64, "bad_cursor")? {
        None => None,
        Some(c) => Some(decode_ledger_cursor(c).ok_or_else(|| {
            ApiError::BadRequest("bad_cursor", "That cursor is not one of ours.".to_string())
        })?),
    };
    let limit = params::limit(&q)?;
    let page = st.store.paper_ledger_page(&account_id, before, limit).await?;
    Ok(Json(Page {
        items: page.rows.into_iter().map(Into::into).collect(),
        next_cursor: page.next.map(|(run, seq)| format!("{run}_{seq}")),
    }))
}
```

- [ ] **Step 5: Route them.** In `bins/api/src/lib.rs` add `pub mod integrity;` and `pub mod paper;` (keep the `pub mod` list alphabetical), and in `router` add:

```rust
        .route("/api/integrity/summary", get(integrity::summary))
        .route("/api/paper/accounts", get(paper::accounts))
        .route("/api/paper/equity", get(paper::equity))
        .route("/api/paper/positions", get(paper::positions))
        .route("/api/paper/ledger", get(paper::ledger))
```

- [ ] **Step 6: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api`
Expected: PASS (1 test in `integrity.rs`, 4 in `paper.rs`, and all earlier api tests).

- [ ] **Step 7: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p api --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add bins/api/src/integrity.rs bins/api/src/paper.rs bins/api/src/lib.rs bins/api/tests/integrity.rs bins/api/tests/paper.rs && git commit -m "api: integrity summary and paper endpoints (empty lists until S3c's tables exist)"
```

---

### Task 6: api: the SSE stream on one shared LISTEN

**Files:**
- Create: `bins/api/src/stream.rs`
- Modify: `bins/api/src/lib.rs` (module + route), `bins/api/src/main.rs` (spawn the bridge), `bins/api/Cargo.toml` (futures-util), `crates/store/src/read.rs` (one admin/test helper)
- Test: `bins/api/tests/stream.rs`

**Interfaces:**
- Consumes: `Store::listen(channel) -> Listener` and `Listener::recv_payload()` (existing, `crates/store/src/batch.rs`); Task 1 `Store::latest_health_minute`; Task 3 `AppState`, `StreamEvent`, `MAX_STREAMS`, `ApiError::Unavailable`.
- Produces:
  - `api::stream::spawn_bridge(store: Store, tx: broadcast::Sender<StreamEvent>, health_every: Duration) -> [JoinHandle<()>; 2]`
  - `api::stream::sse` handler for `GET /api/stream`: `text/event-stream` with `event: episode` / `data: <episode_id>`, `event: health` / `data: <minute_ms>`, and a `: hb` comment every 15 s; 503 `too_many_streams` past 16 clients
  - `Store::listen_backends(&self) -> Result<i64, StoreError>` (admin/test tooling: LISTENing backends in this database)

- [ ] **Step 1: Add the test helper to store.** In `crates/store/src/read.rs`, inside `impl Store`, after `latest_health_minute`, add:

```rust
    /// Admin/test tooling: backends in this database whose last statement was a LISTEN.
    pub async fn listen_backends(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .fetch_one(self.pool())
        .await?)
    }
```

- [ ] **Step 2: Add the dependency.** In `bins/api/Cargo.toml` under `[dependencies]` add `futures-util.workspace = true`.

- [ ] **Step 3: Write the failing tests.** Create `bins/api/tests/stream.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::*;
use tower::ServiceExt;

/// Open a stream client and collect everything it receives into a shared string.
async fn client(app: &Router) -> Arc<Mutex<String>> {
    let resp = app
        .clone()
        .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let buf = Arc::new(Mutex::new(String::new()));
    let b = buf.clone();
    tokio::spawn(async move {
        let mut body = resp.into_body().into_data_stream();
        while let Some(Ok(chunk)) = body.next().await {
            b.lock().unwrap().push_str(&String::from_utf8_lossy(&chunk));
        }
    });
    buf
}

/// NOTIFY `id` every 200 ms until every client saw it (the bridge may still be connecting).
async fn notify_until_seen(owner: &Store, id: &str, clients: &[Arc<Mutex<String>>]) {
    let want = format!("event: episode\ndata: {id}\n");
    let seen = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            owner.raw_exec(&format!("SELECT pg_notify('episode', '{id}')")).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            if clients.iter().all(|c| c.lock().unwrap().contains(&want)) {
                break;
            }
        }
    })
    .await;
    assert!(seen.is_ok(), "{id} never reached every client");
}

#[tokio::test]
async fn a_notify_episode_reaches_two_clients_over_one_listen() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(st.store.clone(), st.events.clone(), Duration::from_millis(100));
    let a = client(&app).await;
    let b = client(&app).await;
    notify_until_seen(&owner, "live-9-00000001", &[a, b]).await;
    assert_eq!(owner.listen_backends().await.unwrap(), 1, "one shared LISTEN for all clients");
    db.drop(owner).await;
}

#[tokio::test]
async fn a_new_health_minute_emits_a_health_event() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(st.store.clone(), st.events.clone(), Duration::from_millis(100));
    let c = client(&app).await;
    let live = common::run(&owner, "live").await;
    owner
        .write_batch(&Batch { health: vec![common::health(live, 120_000, 0)], ..Batch::default() })
        .await
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(10), async {
        while !c.lock().unwrap().contains("event: health\ndata: 120000\n") {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(got.is_ok(), "no health event: {}", c.lock().unwrap());
    db.drop(owner).await;
}

#[tokio::test]
async fn the_bridge_survives_a_killed_listen_connection() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, st) = common::app(&db).await;
    let _bridge = api::stream::spawn_bridge(st.store.clone(), st.events.clone(), Duration::from_millis(100));
    let c = client(&app).await;
    notify_until_seen(&owner, "live-9-00000002", &[c.clone()]).await;
    owner
        .raw_exec(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .await
        .unwrap();
    notify_until_seen(&owner, "live-9-00000003", &[c]).await;
    db.drop(owner).await;
}

#[tokio::test]
async fn clients_past_the_cap_get_a_json_503() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, st) = common::app(&db).await;
    let _all = st.stream_slots.clone().acquire_many_owned(api::MAX_STREAMS as u32).await.unwrap();
    let (status, v) = common::get(&app, "/api/stream").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"], "too_many_streams");
    db.drop(owner).await;
}
```

- [ ] **Step 4: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api --test stream`
Expected: compile error `could not find stream in api`.

- [ ] **Step 5: Write the stream module.** Create `bins/api/src/stream.rs`:

```rust
//! GET /api/stream (spec §4): Server-Sent Events for the live Radar.
//! ONE Postgres LISTEN serves every client (spec §3): a bridge task fans the scanner's
//! `NOTIFY episode` out through a broadcast channel. `health` fires when a new health minute
//! appears (one cheap indexed poll per interval; no trigger, no migration). A `: hb` comment
//! every 15 s keeps proxies from closing idle streams.
//! The LISTEN loop and the health poll are separate tasks: `Listener::recv_payload` is never
//! raced in a `select!`, so a cancelled read can't lose part of a notification.
use crate::error::ApiError;
use crate::{AppState, StreamEvent};
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use std::convert::Infallible;
use std::time::Duration;
use store::Store;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub const HEARTBEAT: Duration = Duration::from_secs(15);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Start the shared LISTEN (with reconnect backoff) and the health-minute poll.
pub fn spawn_bridge(
    store: Store,
    tx: broadcast::Sender<StreamEvent>,
    health_every: Duration,
) -> [JoinHandle<()>; 2] {
    let episodes = {
        let store = store.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut backoff = BACKOFF_START;
            loop {
                match store.listen("episode").await {
                    Ok(mut listener) => {
                        backoff = BACKOFF_START;
                        loop {
                            match listener.recv_payload().await {
                                Ok(id) => {
                                    let id: String = id.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                                    let _ = tx.send(StreamEvent::Episode(id));
                                }
                                Err(e) => {
                                    eprintln!("api: stream LISTEN lost: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("api: stream LISTEN failed: {e}"),
                }
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(BACKOFF_MAX);
            }
        })
    };
    let health = tokio::spawn(async move {
        let mut last: Option<i64> = None;
        let mut tick = tokio::time::interval(health_every);
        loop {
            tick.tick().await;
            match store.latest_health_minute().await {
                Ok(Some(m)) if last != Some(m) => {
                    last = Some(m);
                    let _ = tx.send(StreamEvent::Health(m));
                }
                Ok(_) => {}
                Err(e) => eprintln!("api: health poll failed: {e}"),
            }
        }
    });
    [episodes, health]
}

pub fn to_event(ev: &StreamEvent) -> Event {
    match ev {
        StreamEvent::Episode(id) => Event::default().event("episode").data(id),
        StreamEvent::Health(minute_ms) => Event::default().event("health").data(minute_ms.to_string()),
    }
}

pub async fn sse(
    State(st): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = st.stream_slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::Unavailable("too_many_streams", "Too many open streams. Close a tab or two.")
    })?;
    let rx = st.events.subscribe();
    // The permit lives inside the stream state, so it is released when the client disconnects.
    let events = stream::unfold((rx, permit), |(mut rx, permit)| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => return Some((Ok(to_event(&ev)), (rx, permit))),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT).text("hb")))
}
```

- [ ] **Step 6: Route it and start the bridge.** In `bins/api/src/lib.rs` add `pub mod stream;` (alphabetical) and `.route("/api/stream", get(stream::sse))` in `router`. In `bins/api/src/main.rs`, after `let state = api::AppState::new(store.clone());` add:

```rust
    let _bridge = api::stream::spawn_bridge(
        store.clone(),
        state.events.clone(),
        std::time::Duration::from_secs(15),
    );
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p api && cargo test -p store`
Expected: PASS (4 stream tests; the kill test can take a few seconds while the bridge backs off and reconnects).

- [ ] **Step 8: Lint the whole workspace (api is done).**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no diffs, no warnings.

- [ ] **Step 9: Smoke-run the binary against the dev database** (debug build; proves the env guard, the read-only refusal and a real response).

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo build -q -p api && set -a; . ./.env; set +a; (DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/cute_trading" API_BIND=127.0.0.1:8081 ./target/debug/api; echo "exit=$?")`
Expected: `api: refusing to start: the database role can write; connect as analyze_ro` and `exit=1`.
Then (curl waits for the port itself; no sleep needed): `cd /c/Users/Isabelly/PycharmProjects/cute-trading && set -a; . ./.env; set +a; (DATABASE_URL="postgres://analyze_ro:${ANALYZE_DB_PASSWORD}@127.0.0.1:55432/cute_trading" API_BIND=127.0.0.1:8081 timeout 20 ./target/debug/api &) ; curl -s --retry 15 --retry-connrefused --retry-delay 1 http://127.0.0.1:8081/api/health/now; echo; curl -s "http://127.0.0.1:8081/api/episodes?limit=0"; echo`
Expected: a JSON `{"health":…}` body, then `{"detail":"limit must be between 1 and 200.","error":"bad_limit"}`. (If the local `cute_trading` database was never migrated, or the dev volume predates `ANALYZE_DB_PASSWORD`, the first call returns a 500/connection error: that is a local-volume issue, not a code failure. Note it in the report and move on.)

- [ ] **Step 10: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add bins/api/src/stream.rs bins/api/src/lib.rs bins/api/src/main.rs bins/api/Cargo.toml Cargo.lock crates/store/src/read.rs bins/api/tests/stream.rs && git commit -m "api: SSE stream on one shared LISTEN (episode + health events, heartbeat, client cap)"
```

---
### Task 7: `web/` scaffold: Vite + React + TS, night-shift tokens, pixel components, api layer, Vitest

**Files:**
- Create: `web/package.json`, `web/tsconfig.json`, `web/vite.config.ts`, `web/index.html`, `web/src/main.tsx`, `web/src/App.tsx`, `web/src/theme.css`
- Create: `web/src/api/types.ts`, `web/src/api/client.ts`, `web/src/api/useApi.ts`, `web/src/api/stream.ts`, `web/src/api/StreamContext.tsx`
- Create: `web/src/lib/money.ts`, `web/src/lib/time.ts`, `web/src/lib/hooks.ts`, `web/src/lib/labels.ts`
- Create: `web/src/components/Box.tsx`, `Tag.tsx`, `Table.tsx`, `Hearts.tsx`, `Spark.tsx`, `LineChart.tsx`, `Money.tsx`, `Age.tsx`, `EmptyState.tsx`, `Loadable.tsx`, `FakeToggle.tsx`, `LiveDot.tsx` (all under `web/src/components/`)
- Create: `web/src/test/setup.ts`, `web/src/test/mockApi.ts`, `web/src/test/fakeEventSource.ts`
- Modify: `.gitignore`, `.dockerignore`
- Test: `web/src/lib/money.test.ts`, `web/src/lib/time.test.ts`, `web/src/components/Money.test.tsx`, `web/src/components/Hearts.test.tsx`, `web/src/components/charts.test.tsx`, `web/src/components/EmptyState.test.tsx`, `web/src/components/FakeToggle.test.tsx`, `web/src/api/stream.test.ts`, `web/src/api/client.test.ts`, `web/src/App.test.tsx`

**Interfaces:**
- Consumes: the JSON shapes from Tasks 3–6 (copied into `types.ts` with the same field names).
- Produces (used by Tasks 8–10):
  - `api` object in `client.ts`: `healthNow, healthSeries(hours), openEpisodes, episodes(q: EpisodeQuery), episode(id), today, integrity(hours), paperAccounts, paperEquity(account, hours), paperPositions, paperLedger(account, cursor)`, each `(…, signal?: AbortSignal) => Promise<T>`; `class ApiError { status; code; detail }`; `interface EpisodeQuery { kind?, from?, to?, net?, flags?, cursor?, limit? }`
  - `useApi<T>(load: (signal) => Promise<T>, deps) => { data?: T; error: unknown; loading: boolean; reload(): void }` (keeps the previous data while refetching)
  - `openStream(handlers, options?) => stop()`; `type StreamState = 'live' | 'reconnecting'`; `StreamProvider`, `useStream() => { state, episodeTick, healthTick }` (episode events coalesced to ≤ 1 tick/s)
  - `formatMoney(value: string | null | undefined, opts?: { signed?: boolean }) => string`, `formatDecimal(value) => string`, `signOf(value) => -1 | 0 | 1`, `isZero(value: string) => boolean`
  - `fmtUtcTime(ms)`, `fmtUtcDateTime(ms)`, `fmtLocal(ms)`, `fmtDuration(ms)`, `fmtAge(ms)`, `isoDay(ms)`, `parseIsoDay(s) => number | null`, `DAY_MS`
  - `useNow(intervalMs) => number`, `useTicker(intervalMs) => number`
  - `kindTag(kind, long?) => { text, tone }`
  - Components: `Box({title?, className?, children})`, `Tag({children: string, tone?: 'plain'|'no'|'yes'|'flag', title?})`, `Table<T>({columns: Column<T>[], rows, rowKey, rowClass?})` with `Column<T> = { key, label, render(row), className?(row) }`, `Hearts({health})` + `troubles(health) => string[]` + `MAX_HEARTS`, `Spark({values: number[], label})`, `LineChart({series: Series[], label, xMin, xMax, markers?, yLabels?})` + `toColumns` + `CHART_EMPTY`, `Money({value, signed?, tone?})`, `Age({sinceMs, nowMs})`, `EmptyState({text})`, `Loading({what?})`, `ErrorState({error})`, `errorText(e)`, `Loadable<T>({state, what, children: (data: T) => ReactNode})`, `FakeToggle()` + `NIGHT_ONLY`, `LiveDot({state})`
  - `App`, `Header`, `NotFound` in `App.tsx`
  - Test helpers: `mockApi(routes)`, `jsonResponse(status, body)`, `FakeEventSource`

- [ ] **Step 1: Ignore the build outputs** (before any install). Append to `.gitignore`:
```
web/node_modules/
web/dist/
```
Append to `.dockerignore`:
```
web/node_modules
web/dist
```

- [ ] **Step 2: Create the project files.** `web/package.json`:
```json
{
  "name": "cute-trading-web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "engines": {
    "node": ">=22.12"
  },
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "test": "vitest run"
  },
  "dependencies": {
    "react": "^19.2.4",
    "react-dom": "^19.2.4",
    "react-router": "^7.14.0"
  },
  "devDependencies": {
    "@testing-library/dom": "^10.4.1",
    "@testing-library/jest-dom": "^6.9.1",
    "@testing-library/react": "^16.3.0",
    "@testing-library/user-event": "^14.6.1",
    "@types/react": "^19.2.14",
    "@types/react-dom": "^19.2.3",
    "@vitejs/plugin-react": "^6.0.1",
    "jsdom": "^27.0.0",
    "typescript": "~5.9.3",
    "vite": "^8.0.7",
    "vitest": "^4.1.0"
  }
}
```
`web/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "types": ["vite/client"]
  },
  "include": ["src", "vite.config.ts"]
}
```
`web/vite.config.ts`:
```ts
/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // No inline scripts anywhere (spec §3 CSP): no modulepreload polyfill.
  build: { modulePreload: { polyfill: false }, sourcemap: false },
  server: { proxy: { '/api': 'http://127.0.0.1:8081' } },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    restoreMocks: true,
    unstubGlobals: true,
  },
});
```
`web/index.html` (no inline script or style; fonts from Google Fonts only):
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="robots" content="noindex, nofollow" />
    <title>cute-trading radar</title>
    <link rel="icon" href="data:," />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Press+Start+2P&family=VT323&display=swap" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 3: Install (creates `web/package-lock.json`; the only `node_modules` tree this plan adds).**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && node -v && npm install --no-audit --no-fund && du -sh node_modules`
Expected: `v23.8.0` (any `>=22.12` works), a clean install, `node_modules` around 150–250 MB. If npm reports a peer-dependency conflict between `vitest` and `vite@8`, set `"vitest"` to the newest version whose `peerDependencies.vite` includes `^8` (`npm view vitest@latest peerDependencies`) and rerun; do not use `--force` or `--legacy-peer-deps`.

- [ ] **Step 4: Write the test helpers.** `web/src/test/setup.ts`:
```ts
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
});
```
`web/src/test/mockApi.ts`:
```ts
import { vi } from 'vitest';

export function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

/** A route value is a JSON body, or a function of the URL returning a body or a Response. */
export type Route = unknown | ((url: URL) => unknown);

/** Stub global fetch with a path → response table. Unknown paths answer a JSON 404. */
export function mockApi(routes: Record<string, Route>) {
  const fn = vi.fn(async (input: RequestInfo | URL) => {
    const url = new URL(String(input), 'http://localhost');
    if (!(url.pathname in routes)) {
      return jsonResponse(404, { error: 'not_found', detail: 'Nothing here. It may have been a dream.' });
    }
    const route = routes[url.pathname];
    const body = typeof route === 'function' ? (route as (u: URL) => unknown)(url) : route;
    return body instanceof Response ? body : jsonResponse(200, body);
  });
  vi.stubGlobal('fetch', fn);
  return fn;
}
```
`web/src/test/fakeEventSource.ts`:
```ts
/** Stands in for the browser's EventSource (jsdom has none). readyState 2 = CLOSED. */
export class FakeEventSource {
  static instances: FakeEventSource[] = [];
  url: string;
  readyState = 0;
  onopen: ((ev: Event) => unknown) | null = null;
  onerror: ((ev: Event) => unknown) | null = null;
  private listeners = new Map<string, ((ev: MessageEvent) => void)[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, fn: (ev: MessageEvent) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }

  close() {
    this.readyState = 2;
  }

  open() {
    this.readyState = 1;
    this.onopen?.(new Event('open'));
  }

  /** closed = true: the browser gave up (e.g. an HTTP error); false: it is retrying by itself. */
  fail(closed: boolean) {
    this.readyState = closed ? 2 : 0;
    this.onerror?.(new Event('error'));
  }

  emit(type: string, data: string) {
    for (const fn of this.listeners.get(type) ?? []) fn(new MessageEvent(type, { data }));
  }
}
```

- [ ] **Step 5: Write the failing tests.**

`web/src/lib/money.test.ts`:
```ts
import { describe, expect, it, vi } from 'vitest';
import { formatDecimal, formatMoney, isZero, signOf } from './money';

describe('formatMoney', () => {
  it('never formats money through a float', () => {
    const spies = [
      vi.spyOn(Number.prototype, 'toFixed'),
      vi.spyOn(Number.prototype, 'toLocaleString'),
      vi.spyOn(Intl, 'NumberFormat'),
      vi.spyOn(globalThis, 'parseFloat'),
      vi.spyOn(Math, 'round'),
    ];
    const out = ['12345678901234567.89', '0.000001', '0.1', '-99.84', '1000', '0'].map((v) => formatMoney(v));
    expect(out).toEqual(['$12,345,678,901,234,567.89', '$0.000001', '$0.10', '−$99.84', '$1,000.00', '$0.00']);
    for (const s of spies) expect(s).not.toHaveBeenCalled();
  });

  it('signs gains, keeps zero unsigned, and shows null as not measured', () => {
    expect(formatMoney('0.41', { signed: true })).toBe('+$0.41');
    expect(formatMoney('-0.12', { signed: true })).toBe('−$0.12');
    expect(formatMoney('0', { signed: true })).toBe('$0.00');
    expect(formatMoney('-0', { signed: true })).toBe('$0.00');
    expect(formatMoney(null)).toBe('—');
    expect(formatMoney('garbage')).toBe('garbage');
  });

  it('formats other decimals as sent, with grouping', () => {
    expect(formatDecimal('83')).toBe('83');
    expect(formatDecimal('-0.0008')).toBe('−0.0008');
    expect(formatDecimal('1234567.5')).toBe('1,234,567.5');
    expect(formatDecimal(undefined)).toBe('—');
    expect([signOf('1'), signOf('-1'), signOf('0.000'), signOf(null)]).toEqual([1, -1, 0, 0]);
    expect(isZero('0.00')).toBe(true);
  });
});
```
`web/src/lib/time.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { fmtAge, fmtDuration, fmtUtcDateTime, fmtUtcTime, isoDay, parseIsoDay } from './time';

describe('time', () => {
  const t = Date.UTC(2026, 8, 25, 1, 12, 4, 210);
  it('formats durations like the spec (4.19 s)', () => {
    expect(fmtDuration(4190)).toBe('4.19 s');
    expect(fmtDuration(84)).toBe('0.08 s');
    expect(fmtDuration(125_000)).toBe('2 min 5 s');
    expect(fmtDuration(7_500_000)).toBe('2 h 5 min');
    expect(fmtDuration(-5)).toBe('0.00 s');
  });
  it('ticks ages as mm:ss.d', () => {
    expect(fmtAge(4200)).toBe('00:04.2');
    expect(fmtAge(3_725_000)).toBe('62:05.0');
    expect(fmtAge(-1)).toBe('00:00.0');
  });
  it('shows UTC and parses days', () => {
    expect(fmtUtcTime(t)).toBe('01:12:04.210 UTC');
    expect(fmtUtcDateTime(t)).toBe('2026-09-25 01:12:04 UTC');
    expect(isoDay(t)).toBe('2026-09-25');
    expect(parseIsoDay('2026-09-25')).toBe(Date.UTC(2026, 8, 25));
    expect(parseIsoDay('25/09/2026')).toBeNull();
  });
});
```
`web/src/components/Money.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Money } from './Money';

describe('Money', () => {
  it('renders the decimal string exactly', () => {
    render(
      <>
        <Money value="12345678901234567.89" />
        <Money value="0.000001" />
      </>,
    );
    expect(screen.getByText('$12,345,678,901,234,567.89')).toBeInTheDocument();
    expect(screen.getByText('$0.000001')).toBeInTheDocument();
  });

  it('tones gains and losses, and keeps null distinct from zero', () => {
    render(
      <>
        <Money value="0.41" signed tone />
        <Money value="-0.12" signed tone />
        <Money value="0" signed tone />
        <Money value={null} />
      </>,
    );
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('−$0.12')).toHaveClass('neg');
    expect(screen.getByText('$0.00')).not.toHaveClass('pos');
    expect(screen.getByText('—')).toHaveAttribute('title', 'not measured');
  });
});
```
`web/src/components/Hearts.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { HealthNow } from '../api/types';
import { Hearts, troubles } from './Hearts';

const ok: HealthNow = {
  now_ms: 0, age_ms: 30_000, run_id: 3, minute_ms: 0, tokens: 4000, shards: 40, parked_shards: 0, msgs: 258_000,
  drops: 0, drops_5m: 0, reconnects: 0, lag_p50_ms: 78, lag_p99_ms: 1900, rest_p50_ms: 80, rest_p99_ms: 300,
  clock_offset_ms: 3, ok: true, capture_paused: false, dropped_groups: 0,
};

describe('Hearts', () => {
  it('shows ten hearts when nothing is wrong', () => {
    render(<Hearts health={ok} />);
    expect(screen.getByRole('img', { name: 'health 10 of 10' })).toHaveTextContent('♥'.repeat(10));
  });

  it('loses one heart per trouble kind and names each one', () => {
    const bad = { ...ok, drops_5m: 2, parked_shards: 1, lag_p99_ms: 5001, age_ms: 180_001, capture_paused: true };
    expect(troubles(bad)).toEqual([
      'drops in the last 5 min',
      'parked shards',
      'lag p99 over 5 s',
      'health row older than 3 min',
      'capture paused',
    ]);
    render(<Hearts health={bad} />);
    expect(screen.getByRole('img', { name: /^health 5 of 10: drops/ })).toHaveTextContent('♥♥♥♥♥♡♡♡♡♡');
    expect(troubles({ ...ok, lag_p99_ms: null, age_ms: 180_000 })).toEqual([]);
  });
});
```
`web/src/components/charts.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { CHART_EMPTY, LineChart, toColumns } from './LineChart';
import { Spark } from './Spark';

describe('charts', () => {
  it('Spark draws one crisp pixel bar per value, zero as empty', () => {
    const { container } = render(<Spark values={[0, 3, 6]} label="episodes per hour" />);
    const svg = container.querySelector('svg');
    expect(svg).toHaveAttribute('shape-rendering', 'crispEdges');
    const rects = [...container.querySelectorAll('rect')];
    expect(rects.map((r) => r.getAttribute('height'))).toEqual(['0', '6', '12']);
  });

  it('LineChart draws step rects, markers and a text legend', () => {
    const { container } = render(
      <LineChart
        label="equity"
        xMin={0}
        xMax={100}
        markers={[50]}
        series={[{ label: 'P0 converter', tone: 'p0', points: [{ x: 0, y: 1 }, { x: 50, y: 2 }, { x: 100, y: 1 }] }]}
      />,
    );
    expect(container.querySelector('svg')).toHaveAttribute('shape-rendering', 'crispEdges');
    expect(container.querySelectorAll('g.line rect').length).toBeGreaterThanOrEqual(3);
    expect(container.querySelectorAll('rect.marker')).toHaveLength(1);
    expect(screen.getByText('P0 converter')).toBeInTheDocument();
  });

  it('LineChart with no points says so instead of drawing nothing', () => {
    render(<LineChart label="empty" xMin={0} xMax={1} series={[{ label: 'a', tone: 'p0', points: [] }]} />);
    expect(screen.getByText(CHART_EMPTY)).toBeInTheDocument();
  });

  it('toColumns keeps at most one point per pixel column (the last one)', () => {
    const pts = Array.from({ length: 10_080 }, (_, i) => ({ x: i, y: i }));
    const cols = toColumns(pts, 0, 10_079, 240);
    expect(cols.length).toBeLessThanOrEqual(240);
    expect(cols[cols.length - 1]).toEqual({ x: 10_079, y: 10_079 });
  });
});
```
`web/src/components/EmptyState.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { ErrorState, Loading, errorText } from './EmptyState';
import { Loadable } from './Loadable';

describe('states', () => {
  it('shows the api detail for api errors', () => {
    const e = new ApiError(503, 'db_unavailable', 'The database is thinking. Try again in a moment.');
    render(<ErrorState error={e} />);
    expect(screen.getByRole('alert')).toHaveTextContent('The database is thinking. Try again in a moment.');
  });

  it('a failed fetch says how to sign in again', () => {
    expect(errorText(new TypeError('Failed to fetch'))).toContain('reload the page to sign in again');
  });

  it('Loadable picks loading, error or content, never blank', () => {
    const { rerender } = render(<Loadable state={{ data: undefined, error: undefined, loading: true, reload: () => {} }} what="x">{() => 'ok'}</Loadable>);
    expect(screen.getByRole('status')).toHaveTextContent('Loading x');
    rerender(<Loadable state={{ data: undefined, error: new Error('boom'), loading: false, reload: () => {} }} what="x">{() => 'ok'}</Loadable>);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    rerender(<Loadable state={{ data: 1, error: undefined, loading: false, reload: () => {} }} what="x">{(n) => `got ${n}`}</Loadable>);
    expect(screen.getByText('got 1')).toBeInTheDocument();
    render(<Loading />);
  });
});
```
`web/src/components/FakeToggle.test.tsx`:
```tsx
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { FakeToggle, NIGHT_ONLY } from './FakeToggle';

function themeSnapshot() {
  const html = document.documentElement;
  return {
    attrs: html.getAttributeNames().map((n) => [n, html.getAttribute(n)]),
    htmlClass: html.className,
    htmlStyle: html.style.cssText,
    bodyClass: document.body.className,
    bodyStyle: document.body.style.cssText,
    storage: localStorage.length,
  };
}

describe('FakeToggle', () => {
  it('says "Night mode only." and never changes the theme', async () => {
    const before = themeSnapshot();
    render(<FakeToggle />);
    const button = screen.getByRole('button', { name: 'Toggle theme' });
    await userEvent.click(button);
    expect(screen.getByRole('tooltip')).toHaveTextContent(NIGHT_ONLY);
    expect(NIGHT_ONLY).toBe('Night mode only.');
    expect(button).toHaveClass('wiggle');
    await userEvent.click(button);
    expect(themeSnapshot()).toEqual(before);
  });

  it('hides the tooltip again after a moment', () => {
    vi.useFakeTimers();
    render(<FakeToggle />);
    fireEvent.click(screen.getByRole('button', { name: 'Toggle theme' }));
    expect(screen.getByRole('tooltip')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(screen.queryByRole('tooltip')).toBeNull();
    vi.useRealTimers();
  });
});
```
`web/src/api/stream.test.ts`:
```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource } from '../test/fakeEventSource';
import { openStream, type StreamState } from './stream';

const create = (u: string) => new FakeEventSource(u);
const es = (i: number) => FakeEventSource.instances[i];

describe('openStream', () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
  });

  it('goes live on open and reports episode and health events', () => {
    const states: StreamState[] = [];
    const episodes: string[] = [];
    const minutes: number[] = [];
    openStream(
      { onState: (s) => states.push(s), onEpisode: (id) => episodes.push(id), onHealth: (m) => minutes.push(m) },
      { create },
    );
    expect(es(0).url).toBe('/api/stream');
    es(0).open();
    es(0).emit('episode', 'live-1-00000001');
    es(0).emit('health', '120000');
    es(0).emit('health', 'nonsense');
    expect(states).toEqual(['reconnecting', 'live']);
    expect(episodes).toEqual(['live-1-00000001']);
    expect(minutes).toEqual([120000]);
  });

  it('reconnects with doubling backoff only after the browser gives up, and stops cleanly', () => {
    vi.useFakeTimers();
    const states: StreamState[] = [];
    const stop = openStream({ onState: (s) => states.push(s) }, { create, baseDelayMs: 1000, maxDelayMs: 4000 });
    es(0).fail(false); // the browser is retrying by itself
    vi.advanceTimersByTime(10_000);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(states.at(-1)).toBe('reconnecting');
    es(0).fail(true);
    vi.advanceTimersByTime(999);
    expect(FakeEventSource.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(2);
    es(1).fail(true);
    vi.advanceTimersByTime(1999);
    expect(FakeEventSource.instances).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(3);
    es(2).fail(true);
    vi.advanceTimersByTime(4000);
    es(3).fail(true);
    vi.advanceTimersByTime(4000); // capped at maxDelayMs
    expect(FakeEventSource.instances).toHaveLength(5);
    es(4).open();
    expect(states.at(-1)).toBe('live');
    es(4).fail(true);
    vi.advanceTimersByTime(1000); // an open resets the backoff
    expect(FakeEventSource.instances).toHaveLength(6);
    stop();
    expect(es(5).readyState).toBe(2);
    es(5).fail(true);
    vi.advanceTimersByTime(60_000);
    expect(FakeEventSource.instances).toHaveLength(6);
    vi.useRealTimers();
  });
});
```
`web/src/api/client.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { jsonResponse, mockApi } from '../test/mockApi';
import { ApiError, api } from './client';

describe('api client', () => {
  it('turns JSON errors into ApiError with the server detail', async () => {
    mockApi({
      '/api/episodes/open': () =>
        jsonResponse(503, { error: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' }),
    });
    const err = await api.openEpisodes().catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 503, code: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' });
  });

  it('sends only the filters that are set', async () => {
    const f = mockApi({ '/api/episodes': { items: [], next_cursor: null } });
    await api.episodes({ kind: 'no_basket', net: true, flags: ['suspect', 'stale'], cursor: null, limit: 50 });
    expect(String(f.mock.calls[0][0])).toBe('/api/episodes?kind=no_basket&net=1&flags=suspect%2Cstale&limit=50');
  });

  it('keeps money as strings', async () => {
    mockApi({ '/api/paper/accounts': { items: [{ equity: '1002.56' }] } });
    const r = await api.paperAccounts();
    expect(typeof r.items[0].equity).toBe('string');
  });
});
```
`web/src/App.test.tsx`:
```tsx
import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { StreamProvider } from './api/StreamContext';
import { FakeEventSource } from './test/fakeEventSource';

describe('App shell', () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
  });

  it('has the logo, the nav, a hollow live dot, the fake toggle and a deadpan 404', () => {
    render(
      <MemoryRouter initialEntries={['/nowhere']}>
        <App />
      </MemoryRouter>,
    );
    expect(screen.getByText('♥ cute-trading')).toBeInTheDocument();
    for (const name of ['RADAR', 'EPISODES', 'PAPER', 'HEALTH']) {
      expect(screen.getByRole('link', { name })).toBeInTheDocument();
    }
    expect(screen.getByText('RECONNECTING')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Toggle theme' })).toBeInTheDocument();
    expect(screen.getByText('This page does not exist. Neither does free money.')).toBeInTheDocument();
  });

  it('turns the live dot solid when the stream opens', () => {
    vi.stubGlobal('EventSource', FakeEventSource);
    render(
      <MemoryRouter initialEntries={['/nowhere']}>
        <StreamProvider>
          <App />
        </StreamProvider>
      </MemoryRouter>,
    );
    act(() => FakeEventSource.instances[0].open());
    expect(screen.getByText('LIVE')).toBeInTheDocument();
  });
});
```

- [ ] **Step 6: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test`
Expected: FAIL: `Failed to resolve import "./money"` (and the other missing modules).

- [ ] **Step 7: Write the night-shift theme.** `web/src/theme.css` (tokens copied from mockup B / `mockup-detail-paper.html`):
```css
:root {
  --mint: #ddfff7;
  --teal: #93e1d8;
  --salmon: #ffa69e;
  --rose: #aa4465;
  --page: #120a1a;
  --bg: #1b1026;
  --fg: var(--mint);
  --line: #93e1d8;
  --line2: #4b3a5c;
  --bar: #2a1838;
  --chip: #2a1838;
  --card: #241532;
  --accent: var(--salmon);
  --accentfg: #1b1026;
  --good: #93e1d8;
  --bad: var(--salmon);
  --dim: #b9a6c9;
  --tag-no: #1f5e57;
  --tag-yes: #7a3d44;
  --tag-flag: #7a3d44;
  --px: 'Press Start 2P', monospace;
  --body: 'VT323', monospace;
  color-scheme: dark;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { background: var(--page); color: var(--fg); }
body { font-family: var(--body); font-size: 20px; line-height: 1.2; padding: 16px; min-height: 100vh; }
a { color: var(--fg); }
button { font: inherit; color: inherit; }
:focus-visible { outline: 2px dashed var(--salmon); outline-offset: 2px; }
.app { border: 3px solid var(--line); box-shadow: 6px 6px 0 var(--line); background: var(--bg); max-width: 1100px; margin: 0 auto; }
.top { display: flex; align-items: center; gap: 12px; padding: 10px 14px; border-bottom: 3px solid var(--line); background: var(--bar); flex-wrap: wrap; }
.logo { font-family: var(--px); font-size: 11px; }
.nav { display: flex; gap: 6px; margin-left: auto; flex-wrap: wrap; }
.nav a { font-family: var(--px); font-size: 8px; padding: 6px 8px; border: 2px solid var(--line); text-decoration: none; background: var(--chip); }
.nav a.on { background: var(--accent); color: var(--accentfg); }
.live { font-family: var(--px); font-size: 7px; display: inline-flex; align-items: center; gap: 5px; }
.dot { width: 8px; height: 8px; background: var(--good); border: 2px solid var(--good); }
.dot.hollow { background: transparent; }
.toggle { font-family: var(--px); font-size: 8px; border: 2px solid var(--line); padding: 5px 7px; background: var(--chip); position: relative; cursor: pointer; }
.toggle .tip { position: absolute; top: 130%; right: 0; white-space: nowrap; background: var(--salmon); color: var(--accentfg); padding: 4px 6px; font-size: 7px; border: 2px solid var(--line); z-index: 2; }
.toggle.wiggle { animation: wiggle 0.3s steps(2) 3; }
@keyframes wiggle { 0% { transform: translateX(-2px); } 100% { transform: translateX(2px); } }
.body { padding: 14px; display: grid; gap: 12px; }
.box { border: 3px solid var(--line); box-shadow: 4px 4px 0 var(--line); background: var(--card); padding: 10px 12px; min-width: 0; }
.box h2 { font-family: var(--px); font-size: 9px; margin-bottom: 8px; line-height: 1.5; }
.health { display: flex; flex-wrap: wrap; gap: 14px; align-items: center; }
.hearts { color: var(--salmon); letter-spacing: 2px; font-size: 24px; }
.stat b { font-family: var(--px); font-size: 10px; }
.grid2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 12px; }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px; }
.card { border: 2px solid var(--line); padding: 8px; background: var(--chip); }
.card .p { font-family: var(--px); font-size: 8px; margin-bottom: 6px; }
.card .eq { font-family: var(--px); font-size: 12px; margin-bottom: 4px; }
.scroll { overflow-x: auto; }
table { width: 100%; border-collapse: collapse; font-size: 20px; }
th { font-family: var(--px); font-size: 7px; text-align: left; padding: 4px; border-bottom: 2px solid var(--line); white-space: nowrap; }
td { padding: 4px; border-bottom: 1px dashed var(--line2); }
tr.dim td { color: var(--dim); }
.tag { font-family: var(--px); font-size: 7px; padding: 3px 5px; border: 2px solid var(--line); background: var(--chip); white-space: nowrap; display: inline-block; margin-right: 4px; }
.tag.no { background: var(--tag-no); }
.tag.yes { background: var(--tag-yes); }
.tag.flag { background: var(--tag-flag); }
.pos { color: var(--good); }
.neg { color: var(--bad); }
.dim { color: var(--dim); }
.quip { opacity: 0.9; }
.error { color: var(--salmon); }
.blink { animation: blink 1s steps(1) infinite; }
@keyframes blink { 50% { opacity: 0; } }
.crumb { font-family: var(--px); font-size: 8px; color: var(--dim); margin-bottom: 6px; }
.crumb a { color: var(--dim); }
.title { font-family: var(--px); font-size: 12px; line-height: 1.6; }
.kv { display: grid; grid-template-columns: auto 1fr; gap: 2px 12px; }
.kv dt { color: var(--dim); }
.seg { display: flex; gap: 4px; flex-wrap: wrap; align-items: center; }
.seg button { font-family: var(--px); font-size: 7px; padding: 4px 6px; border: 2px solid var(--line); background: var(--chip); cursor: pointer; }
.seg button.on { background: var(--accent); color: var(--accentfg); }
.filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; }
.filters label { display: inline-flex; gap: 6px; align-items: center; }
.filters input[type='date'] { font: inherit; background: var(--chip); color: var(--fg); border: 2px solid var(--line); padding: 0 4px; }
.pager { display: flex; gap: 8px; margin-top: 8px; }
.pager button { font-family: var(--px); font-size: 7px; padding: 5px 8px; border: 2px solid var(--line); background: var(--chip); cursor: pointer; }
.pager button:disabled { opacity: 0.4; cursor: default; }
svg { shape-rendering: crispEdges; }
.spark { display: block; width: 100%; height: 48px; }
.spark-bar { fill: var(--accent); }
.edgebar rect { fill: var(--accent); }
.chart-wrap { display: grid; gap: 4px; }
svg.chart { display: block; width: 100%; height: 180px; background: var(--bg); border: 2px solid var(--line2); }
svg.tl { display: block; width: 100%; height: 70px; }
.chart .marker { fill: var(--line2); }
.chart-axis { display: flex; justify-content: space-between; color: var(--dim); }
.legend { display: flex; gap: 12px; flex-wrap: wrap; }
.sw { display: inline-block; width: 12px; height: 12px; margin-right: 4px; vertical-align: middle; }
.line.p0, .tl .ok { fill: var(--teal); }
.line.p1 { fill: var(--mint); }
.line.p2 { fill: var(--rose); }
.line.p3, .tl .bad { fill: var(--salmon); }
.tl .skip, .tl .axis { fill: var(--line2); }
.sw.p0 { background: var(--teal); }
.sw.p1 { background: var(--mint); }
.sw.p2 { background: var(--rose); }
.sw.p3 { background: var(--salmon); }
@media (prefers-reduced-motion: reduce) { .toggle.wiggle, .blink { animation: none; } }
@media (max-width: 640px) {
  body { padding: 8px; }
  .body { padding: 8px; }
  .nav { margin-left: 0; width: 100%; }
  .hearts { font-size: 20px; }
}
```

- [ ] **Step 8: Write the pure helpers.** `web/src/lib/money.ts`:
```ts
/**
 * Money arrives as decimal strings (spec §4) and is formatted with string operations only.
 * Never parseFloat, Number(), toFixed, toLocaleString or Intl on money: a float rounds
 * 12345678901234567.89 and turns 0.000001 into 0.00.
 */
const DEC = /^(-?)(\d+)(?:\.(\d+))?$/;

export function groupThousands(int: string): string {
  let out = '';
  for (let i = 0; i < int.length; i++) {
    if (i > 0 && (int.length - i) % 3 === 0) out += ',';
    out += int[i];
  }
  return out;
}

export function isZero(value: string): boolean {
  return /^-?0+(\.0+)?$/.test(value.trim());
}

export function signOf(value: string | null | undefined): -1 | 0 | 1 {
  if (value === null || value === undefined || isZero(value)) return 0;
  return value.trim().startsWith('-') ? -1 : 1;
}

/** "1002.56" → "$1,002.56"; "0.1" → "$0.10" (padded, never rounded); null → "—" (not measured). */
export function formatMoney(value: string | null | undefined, opts: { signed?: boolean } = {}): string {
  if (value === null || value === undefined) return '—';
  const m = DEC.exec(value.trim());
  if (!m) return value;
  const [, minus, int, frac = ''] = m;
  const body = `$${groupThousands(int)}.${frac.length >= 2 ? frac : frac.padEnd(2, '0')}`;
  if (isZero(value)) return body;
  if (minus) return `−${body}`;
  return opts.signed ? `+${body}` : body;
}

/** Any other decimal (prices, edges, Q*): as sent, grouped, with a real minus sign. */
export function formatDecimal(value: string | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const m = DEC.exec(value.trim());
  if (!m) return value;
  const [, minus, int, frac] = m;
  return `${minus && !isZero(value) ? '−' : ''}${groupThousands(int)}${frac ? `.${frac}` : ''}`;
}
```
`web/src/lib/time.ts`:
```ts
export const DAY_MS = 86_400_000;

const pad = (n: number, w = 2) => String(n).padStart(w, '0');

export function fmtUtcTime(ms: number): string {
  return `${new Date(ms).toISOString().slice(11, 23)} UTC`;
}

export function fmtUtcDateTime(ms: number): string {
  const s = new Date(ms).toISOString();
  return `${s.slice(0, 10)} ${s.slice(11, 19)} UTC`;
}

export function fmtLocal(ms: number): string {
  return new Date(ms).toLocaleString();
}

/** 4190 → "4.19 s"; 125000 → "2 min 5 s"; 7500000 → "2 h 5 min". Integer math only. */
export function fmtDuration(ms: number): string {
  const t = Math.max(0, Math.floor(ms));
  if (t < 60_000) return `${Math.floor(t / 1000)}.${pad(Math.floor((t % 1000) / 10))} s`;
  const s = Math.floor(t / 1000);
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

/** Radar age: 4200 → "00:04.2". */
export function fmtAge(ms: number): string {
  const tenths = Math.floor(Math.max(0, ms) / 100);
  return `${pad(Math.floor(tenths / 600))}:${pad(Math.floor((tenths % 600) / 10))}.${tenths % 10}`;
}

export function isoDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/** "2026-09-25" → ms at 00:00 UTC; anything else → null. */
export function parseIsoDay(s: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return null;
  const t = Date.parse(`${s}T00:00:00Z`);
  return Number.isNaN(t) ? null : t;
}
```
`web/src/lib/hooks.ts`:
```ts
import { useEffect, useState } from 'react';

/** Date.now(), refreshed every `intervalMs` (ticking ages). */
export function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

/** A counter that increments every `intervalMs` (use it in useApi deps for slow refreshes). */
export function useTicker(intervalMs: number): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setN((x) => x + 1), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return n;
}
```
`web/src/lib/labels.ts`:
```ts
import type { TagTone } from '../components/Tag';

/** Tags carry text, not colour alone (spec §5). */
export function kindTag(kind: string, long = false): { text: string; tone: TagTone } {
  switch (kind) {
    case 'no_basket':
      return { text: long ? 'NO BASKET' : 'NO', tone: 'no' };
    case 'yes_basket':
      return { text: long ? 'YES BASKET' : 'YES', tone: 'yes' };
    case 'yes_basket_incomplete':
      return { text: long ? 'YES BASKET?' : 'YES?', tone: 'yes' };
    case 'implication':
      return { text: long ? 'IMPLICATION' : 'IMPL', tone: 'plain' };
    default:
      return { text: kind.toUpperCase(), tone: 'plain' };
  }
}
```

- [ ] **Step 9: Write the api layer.** `web/src/api/types.ts` (field names exactly as the Rust DTOs serialize):
```ts
/** A decimal string from the api ("0.41"), never a JS number (spec §4). */
export type Dec = string;

export interface HealthNow {
  now_ms: number;
  age_ms: number;
  run_id: number;
  minute_ms: number;
  tokens: number;
  shards: number;
  parked_shards: number;
  msgs: number;
  drops: number;
  drops_5m: number;
  reconnects: number;
  lag_p50_ms: number | null;
  lag_p99_ms: number | null;
  rest_p50_ms: number | null;
  rest_p99_ms: number | null;
  clock_offset_ms: number | null;
  ok: boolean | null;
  capture_paused: boolean;
  dropped_groups: number;
}
export interface HealthEnvelope {
  health: HealthNow | null;
}
export interface HealthPoint {
  minute_ms: number;
  run_id: number;
  msgs: number;
  drops: number;
  reconnects: number;
  lag_p50_ms: number | null;
  lag_p99_ms: number | null;
  shards: number;
  parked_shards: number;
  capture_paused: boolean;
}
export interface HealthSeries {
  from_ms: number;
  to_ms: number;
  points: HealthPoint[];
}
export interface EpisodeSummary {
  id: string;
  kind: string;
  realization: string;
  event_title: string | null;
  event_slug: string | null;
  n_legs: number;
  s_legs: number;
  opened_ms: number;
  closed_ms: number | null;
  duration_ms: number | null;
  edge: Dec;
  q_star: Dec;
  net: Dec;
  gross: Dec;
  suspect: boolean;
  stale: boolean;
  censored: boolean;
}
export interface OpenEpisodes {
  now_ms: number;
  items: EpisodeSummary[];
}
export interface Page<T> {
  items: T[];
  next_cursor: string | null;
}
export interface Episode {
  id: string;
  run_id: number;
  source: string;
  kind: string;
  realization: string;
  event_id: string;
  event_title: string | null;
  event_slug: string | null;
  category: string | null;
  n_outcomes: number;
  n_legs: number;
  legs_dropped: number;
  augmented: boolean;
  incomplete_set: boolean;
  uncovered_legs: number;
  opened_ms: number;
  closed_ms: number | null;
  duration_ms: number | null;
  censor_left: boolean;
  censor_right: boolean;
  censor_gap: boolean;
  detect_lag_ms: number | null;
  n_updates: number;
  peak_ms: number;
  edge: Dec;
  edge_gross: Dec;
  q_star: Dec;
  cost: Dec;
  payout: Dec;
  fees: Dec;
  net: Dec;
  gross: Dec;
  profit_per_dollar: Dec | null;
  net_budget100: Dec | null;
  bottleneck_leg_idx: number | null;
  below_min_size: boolean | null;
  max_seconds_delay: number | null;
  days_to_end: Dec | null;
  annualized_return: Dec | null;
  post_end: boolean | null;
  suspect: boolean;
  stale: boolean;
  open_lag_ms: number | null;
  oracle_downgraded: boolean | null;
  scanner_version: string;
}
export interface Leg {
  leg_idx: number;
  market_id: string;
  action: string;
  best_price: Dec | null;
  vwap: Dec | null;
  levels: number;
  size_at_best: Dec | null;
  filled: Dec;
  notional: Dec;
  fees: Dec;
  in_s: boolean;
}
export interface Observation {
  offset_ms: number;
  source: string;
  observed_ms: number | null;
  skipped: boolean;
  skip_reason: string | null;
  edge: Dec | null;
  q_star: Dec | null;
  pnl: Dec | null;
  still_profitable: boolean | null;
  fill_at_original_size: boolean | null;
}
export interface EpisodeDetail {
  episode: Episode;
  legs: Leg[];
  observations: Observation[];
}
export interface Today {
  day_start_ms: number;
  gross: number;
  net: number;
  hourly: { hour: number; gross: number; net: number }[];
}
export interface Integrity {
  from_ms: number;
  to_ms: number;
  by_kind: { kind: string; count: number }[];
  total: number;
  msgs: number;
  per_million: Dec | null;
}
export interface PaperAccount {
  account_id: string;
  policy: string;
  scenario: string;
  fee_bips: Dec;
  bankroll: Dec;
  run_id: number;
  marked_ms: number | null;
  cash: Dec | null;
  equity: Dec | null;
  realized: Dec | null;
  unrealized: Dec | null;
  fees_paid: Dec | null;
  locked: Dec | null;
  max_drawdown: Dec | null;
  n_positions: number | null;
}
export interface EquitySeries {
  account_id: string;
  from_ms: number;
  to_ms: number;
  points: { t_ms: number; equity: Dec }[];
}
export interface Position {
  account_id: string;
  market_id: string;
  outcome: string;
  qty: Dec;
  basis: Dec;
  returned: boolean;
  event_title: string | null;
}
export interface LedgerEntry {
  run_id: number;
  account_id: string;
  seq: number;
  t_ms: number;
  kind: string;
  episode_id: string | null;
  market_id: string | null;
  outcome: string | null;
  qty: Dec;
  price: Dec | null;
  cash_delta: Dec;
  fees: Dec;
  realized_delta: Dec;
  cash_after: Dec;
  equity: Dec | null;
}
```
`web/src/api/client.ts`:
```ts
import type {
  EpisodeDetail, EpisodeSummary, EquitySeries, HealthEnvelope, HealthSeries, Integrity, LedgerEntry,
  OpenEpisodes, Page, PaperAccount, Position, Today,
} from './types';

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly detail: string;
  constructor(status: number, code: string, detail: string) {
    super(detail);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.detail = detail;
  }
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, { signal, credentials: 'same-origin', headers: { Accept: 'application/json' } });
  if (!res.ok) {
    let code = `http_${res.status}`;
    let detail = 'Something broke. Not your fault. Probably.';
    try {
      const body = (await res.json()) as { error?: string; detail?: string };
      if (body.error) code = body.error;
      if (body.detail) detail = body.detail;
    } catch {
      // not JSON (a proxy page): keep the generic text
    }
    throw new ApiError(res.status, code, detail);
  }
  return (await res.json()) as T;
}

function qs(params: Record<string, string | number | null | undefined>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export interface EpisodeQuery {
  kind?: string;
  from?: number;
  to?: number;
  net?: boolean;
  flags?: string[];
  cursor?: string | null;
  limit?: number;
}

export const api = {
  healthNow: (signal?: AbortSignal) => getJson<HealthEnvelope>('/api/health/now', signal),
  healthSeries: (hours: number, signal?: AbortSignal) =>
    getJson<HealthSeries>(`/api/health/series${qs({ hours })}`, signal),
  openEpisodes: (signal?: AbortSignal) => getJson<OpenEpisodes>('/api/episodes/open', signal),
  episodes: (q: EpisodeQuery, signal?: AbortSignal) =>
    getJson<Page<EpisodeSummary>>(
      `/api/episodes${qs({
        kind: q.kind,
        from: q.from,
        to: q.to,
        net: q.net ? 1 : undefined,
        flags: q.flags && q.flags.length > 0 ? q.flags.join(',') : undefined,
        cursor: q.cursor,
        limit: q.limit,
      })}`,
      signal,
    ),
  episode: (id: string, signal?: AbortSignal) =>
    getJson<EpisodeDetail>(`/api/episodes/${encodeURIComponent(id)}`, signal),
  today: (signal?: AbortSignal) => getJson<Today>('/api/today', signal),
  integrity: (hours: number, signal?: AbortSignal) =>
    getJson<Integrity>(`/api/integrity/summary${qs({ hours })}`, signal),
  paperAccounts: (signal?: AbortSignal) => getJson<{ items: PaperAccount[] }>('/api/paper/accounts', signal),
  paperEquity: (account: string, hours: number, signal?: AbortSignal) =>
    getJson<EquitySeries>(`/api/paper/equity${qs({ account, hours })}`, signal),
  paperPositions: (signal?: AbortSignal) => getJson<{ items: Position[] }>('/api/paper/positions', signal),
  paperLedger: (account: string, cursor: string | null, signal?: AbortSignal) =>
    getJson<Page<LedgerEntry>>(`/api/paper/ledger${qs({ account, cursor })}`, signal),
};
```
`web/src/api/useApi.ts`:
```ts
import { useCallback, useEffect, useState } from 'react';

export interface ApiState<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
}

/** Fetch on mount and whenever `deps` change; keeps the last data while refetching (no flashing). */
export function useApi<T>(load: (signal: AbortSignal) => Promise<T>, deps: readonly unknown[]): ApiState<T> {
  const [data, setData] = useState<T | undefined>(undefined);
  const [error, setError] = useState<unknown>(undefined);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    const ctl = new AbortController();
    setLoading(true);
    load(ctl.signal).then(
      (d) => {
        if (ctl.signal.aborted) return;
        setData(d);
        setError(undefined);
        setLoading(false);
      },
      (e: unknown) => {
        if (ctl.signal.aborted) return;
        setError(e);
        setLoading(false);
      },
    );
    return () => ctl.abort();
    // `load` is a fresh closure every render; `deps` says when it really changed.
  }, [...deps, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, loading, reload };
}
```
`web/src/api/stream.ts`:
```ts
export type StreamState = 'live' | 'reconnecting';

export interface StreamHandlers {
  onEpisode?: (id: string) => void;
  onHealth?: (minuteMs: number) => void;
  onState?: (s: StreamState) => void;
}

export interface EventSourceLike {
  readyState: number;
  onopen: ((ev: Event) => unknown) | null;
  onerror: ((ev: Event) => unknown) | null;
  addEventListener(type: string, fn: (ev: MessageEvent) => void): void;
  close(): void;
}

export interface StreamOptions {
  url?: string;
  baseDelayMs?: number;
  maxDelayMs?: number;
  create?: (url: string) => EventSourceLike;
}

const CLOSED = 2;

/**
 * One EventSource on /api/stream. The browser retries a dropped stream by itself; when it gives up
 * (readyState CLOSED, e.g. an HTTP error or an expired login) we reopen with doubling backoff.
 * Returns stop().
 */
export function openStream(h: StreamHandlers, o: StreamOptions = {}): () => void {
  const url = o.url ?? '/api/stream';
  const base = o.baseDelayMs ?? 1000;
  const max = o.maxDelayMs ?? 30_000;
  const create = o.create ?? ((u: string) => new EventSource(u) as unknown as EventSourceLike);
  let delay = base;
  let es: EventSourceLike | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;

  const connect = () => {
    if (stopped) return;
    const current = create(url);
    es = current;
    current.onopen = () => {
      delay = base;
      h.onState?.('live');
    };
    current.onerror = () => {
      h.onState?.('reconnecting');
      if (current.readyState === CLOSED && es === current) {
        current.close();
        es = null;
        timer = setTimeout(connect, delay);
        delay = Math.min(delay * 2, max);
      }
    };
    current.addEventListener('episode', (ev) => h.onEpisode?.(String(ev.data)));
    current.addEventListener('health', (ev) => {
      const m = Number(ev.data);
      if (Number.isFinite(m)) h.onHealth?.(m);
    });
  };

  h.onState?.('reconnecting');
  connect();
  return () => {
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
    es?.close();
  };
}
```
Note on the last `stop()` assertion in `stream.test.ts`: after `stop()`, `es` is instance 5 and `stopped` is true, so its `fail(true)` schedules a `connect` that returns immediately; no seventh instance appears.

`web/src/api/StreamContext.tsx`:
```tsx
import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { openStream, type StreamState } from './stream';

export interface StreamValue {
  state: StreamState;
  /** Increments at most once per EPISODE_COALESCE_MS while episode events arrive. */
  episodeTick: number;
  healthTick: number;
}

/** The scanner NOTIFYs on every episode write (open, peak, close): refetch at most once a second. */
export const EPISODE_COALESCE_MS = 1000;

const initial: StreamValue = { state: 'reconnecting', episodeTick: 0, healthTick: 0 };
const Ctx = createContext<StreamValue>(initial);

export function StreamProvider({ children }: { children: ReactNode }) {
  const [value, setValue] = useState<StreamValue>(initial);
  useEffect(() => {
    let pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const flush = () => {
      timer = undefined;
      if (pending) {
        pending = false;
        setValue((p) => ({ ...p, episodeTick: p.episodeTick + 1 }));
      }
    };
    const stop = openStream({
      onState: (state) => setValue((p) => (p.state === state ? p : { ...p, state })),
      onEpisode: () => {
        pending = true;
        if (timer === undefined) timer = setTimeout(flush, EPISODE_COALESCE_MS);
      },
      onHealth: () => setValue((p) => ({ ...p, healthTick: p.healthTick + 1 })),
    });
    return () => {
      stop();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, []);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useStream(): StreamValue {
  return useContext(Ctx);
}
```

- [ ] **Step 10: Write the components.** `web/src/components/Box.tsx`:
```tsx
import type { ReactNode } from 'react';

export function Box({ title, className, children }: { title?: ReactNode; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `box ${className}` : 'box'}>
      {title !== undefined && <h2>{title}</h2>}
      {children}
    </section>
  );
}
```
`web/src/components/Tag.tsx`:
```tsx
export type TagTone = 'plain' | 'no' | 'yes' | 'flag';

export function Tag({ children, tone = 'plain', title }: { children: string; tone?: TagTone; title?: string }) {
  return (
    <span className={`tag ${tone}`} title={title}>
      {children}
    </span>
  );
}
```
`web/src/components/Table.tsx`:
```tsx
import type { ReactNode } from 'react';

export interface Column<T> {
  key: string;
  label: string;
  render: (row: T) => ReactNode;
  className?: (row: T) => string | undefined;
}

export function Table<T>({
  columns,
  rows,
  rowKey,
  rowClass,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  rowClass?: (row: T) => string | undefined;
}) {
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} scope="col">
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={rowKey(r)} className={rowClass?.(r)}>
              {columns.map((c) => (
                <td key={c.key} className={c.className?.(r)}>
                  {c.render(r)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
```
`web/src/components/Hearts.tsx`:
```tsx
import type { HealthNow } from '../api/types';

export const MAX_HEARTS = 10;

/** One lost heart per trouble kind (spec §5 Radar). */
export function troubles(h: HealthNow): string[] {
  const t: string[] = [];
  if (h.drops_5m > 0) t.push('drops in the last 5 min');
  if (h.parked_shards > 0) t.push('parked shards');
  if (h.lag_p99_ms !== null && h.lag_p99_ms > 5000) t.push('lag p99 over 5 s');
  if (h.age_ms > 180_000) t.push('health row older than 3 min');
  if (h.capture_paused) t.push('capture paused');
  return t;
}

export function Hearts({ health }: { health: HealthNow }) {
  const t = troubles(health);
  const full = MAX_HEARTS - t.length;
  const label = `health ${full} of ${MAX_HEARTS}${t.length > 0 ? `: ${t.join(', ')}` : ''}`;
  return (
    <span className="hearts" role="img" aria-label={label} title={t.length > 0 ? t.join(', ') : 'all good'}>
      {'♥'.repeat(full)}
      {'♡'.repeat(MAX_HEARTS - full)}
    </span>
  );
}
```
`web/src/components/Spark.tsx`:
```tsx
/** Pixel bar spark (counts, never money). 4 units per bar, 12 high. */
export function Spark({ values, label }: { values: number[]; label: string }) {
  const max = Math.max(0, ...values);
  const width = Math.max(1, values.length) * 4;
  return (
    <svg className="spark" viewBox={`0 0 ${width} 12`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={label}>
      {values.map((v, i) => {
        const h = v <= 0 || max === 0 ? 0 : Math.max(1, Math.round((v / max) * 12));
        return <rect key={i} className="spark-bar" x={i * 4} y={12 - h} width={4} height={h} />;
      })}
    </svg>
  );
}
```
`web/src/components/LineChart.tsx`:
```tsx
import type { ReactElement } from 'react';
import { EmptyState } from './EmptyState';

export interface Point {
  x: number;
  y: number;
}
export interface Series {
  label: string;
  /** CSS tone class: p0 | p1 | p2 | p3 */
  tone: string;
  points: Point[];
}

export const CHART_EMPTY = 'Nothing to draw yet. The chart is patient.';
const W = 240;
const H = 60;

/** The last point per pixel column, so a week of minutes draws at most `width` steps. */
export function toColumns(points: Point[], xMin: number, xMax: number, width: number): Point[] {
  const span = Math.max(1, xMax - xMin);
  const out: Point[] = [];
  let lastCol = -1;
  for (const p of points) {
    const col = Math.floor(((p.x - xMin) / span) * (width - 1));
    if (col === lastCol && out.length > 0) out[out.length - 1] = p;
    else {
      out.push(p);
      lastCol = col;
    }
  }
  return out;
}

function stepRects(pts: [number, number][], key: string): ReactElement[] {
  const rects: ReactElement[] = [];
  for (let i = 0; i < pts.length; i++) {
    const [x, y] = pts[i];
    const next = i + 1 < pts.length ? pts[i + 1] : undefined;
    const nx = next ? next[0] : x + 1;
    rects.push(<rect key={`${key}h${i}`} x={x} y={y} width={Math.max(1, nx - x)} height={1} />);
    if (next && next[1] !== y) {
      rects.push(<rect key={`${key}v${i}`} x={nx} y={Math.min(y, next[1])} width={1} height={Math.abs(next[1] - y) + 1} />);
    }
  }
  return rects;
}

/**
 * Hand-made pixel step chart (no chart library). Values are numbers used for GEOMETRY only;
 * any money shown as text goes through `yLabels`, already formatted from the decimal strings.
 */
export function LineChart({
  series,
  label,
  xMin,
  xMax,
  markers = [],
  yLabels,
}: {
  series: Series[];
  label: string;
  xMin: number;
  xMax: number;
  markers?: number[];
  yLabels?: { top: string; bottom: string };
}) {
  const cols = series.map((s) => ({ ...s, points: toColumns(s.points, xMin, xMax, W) }));
  const all = cols.flatMap((s) => s.points);
  if (all.length === 0) return <EmptyState text={CHART_EMPTY} />;
  let yMin = Math.min(...all.map((p) => p.y));
  let yMax = Math.max(...all.map((p) => p.y));
  if (yMax === yMin) {
    yMin -= 1;
    yMax += 1;
  }
  const span = Math.max(1, xMax - xMin);
  const sx = (x: number) => Math.min(W - 1, Math.max(0, Math.round(((x - xMin) / span) * (W - 1))));
  const sy = (y: number) => 2 + Math.round((1 - (y - yMin) / (yMax - yMin)) * (H - 5));
  return (
    <figure className="chart-wrap">
      <svg className="chart" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={label}>
        {markers.map((m, i) => (
          <rect key={`m${i}`} className="marker" x={sx(m)} y={0} width={1} height={H} />
        ))}
        {cols.map((s) => (
          <g key={s.label} className={`line ${s.tone}`}>
            {stepRects(s.points.map((p) => [sx(p.x), sy(p.y)] as [number, number]), s.label)}
          </g>
        ))}
      </svg>
      {yLabels && (
        <div className="chart-axis">
          <span>low {yLabels.bottom}</span>
          <span>high {yLabels.top}</span>
        </div>
      )}
      <div className="legend">
        {series.map((s) => (
          <span key={s.label}>
            <span className={`sw ${s.tone}`} aria-hidden="true" />
            {s.label}
          </span>
        ))}
      </div>
    </figure>
  );
}
```
`web/src/components/Money.tsx`:
```tsx
import { formatMoney, signOf } from '../lib/money';

/** A money decimal string, rendered as sent (spec §6). `tone` colours gains/losses (the sign is text too). */
export function Money({ value, signed = false, tone = false }: { value: string | null | undefined; signed?: boolean; tone?: boolean }) {
  const s = signOf(value);
  const cls = tone && s !== 0 ? (s > 0 ? 'pos' : 'neg') : undefined;
  return (
    <span className={cls} title={value === null || value === undefined ? 'not measured' : undefined}>
      {formatMoney(value, { signed })}
    </span>
  );
}
```
`web/src/components/Age.tsx`:
```tsx
import { fmtAge, fmtLocal } from '../lib/time';

export function Age({ sinceMs, nowMs }: { sinceMs: number; nowMs: number }) {
  return <span title={`opened ${fmtLocal(sinceMs)}`}>{fmtAge(nowMs - sinceMs)}</span>;
}
```
`web/src/components/EmptyState.tsx`:
```tsx
import { ApiError } from '../api/client';

export function EmptyState({ text }: { text: string }) {
  return <p className="quip empty">{text}</p>;
}

export function Loading({ what = 'data' }: { what?: string }) {
  return (
    <p className="quip loading" role="status">
      <span className="blink" aria-hidden="true">
        ▮
      </span>{' '}
      Loading {what}. The pixels are warming up.
    </p>
  );
}

export function errorText(e: unknown): string {
  if (e instanceof ApiError) return e.detail;
  // A cross-origin redirect to the GitHub login (expired session) surfaces as a TypeError.
  if (e instanceof TypeError) return 'Lost the api. Your session may have expired: reload the page to sign in again.';
  return 'Something broke. Not your fault. Probably.';
}

export function ErrorState({ error }: { error: unknown }) {
  return (
    <p className="quip error" role="alert">
      {errorText(error)}
    </p>
  );
}
```
`web/src/components/Loadable.tsx`:
```tsx
import type { ReactNode } from 'react';
import type { ApiState } from '../api/useApi';
import { ErrorState, Loading } from './EmptyState';

/** Loading, error or content: never a blank box (spec §6). Stale data stays up during a refetch. */
export function Loadable<T>({ state, what, children }: { state: ApiState<T>; what: string; children: (data: T) => ReactNode }) {
  if (state.data !== undefined) return <>{children(state.data)}</>;
  if (state.error !== undefined) return <ErrorState error={state.error} />;
  return <Loading what={what} />;
}
```
`web/src/components/FakeToggle.tsx`:
```tsx
import { useEffect, useRef, useState } from 'react';

export const NIGHT_ONLY = 'Night mode only.';

/** The ☾/☀ switch that switches nothing (spec §5). It must never touch the theme. */
export function FakeToggle() {
  const [tip, setTip] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const onClick = () => {
    setTip(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setTip(false), 1800);
  };
  return (
    <button
      type="button"
      className={tip ? 'toggle wiggle' : 'toggle'}
      onClick={onClick}
      aria-label="Toggle theme"
      aria-describedby={tip ? 'night-only' : undefined}
    >
      ☾ / ☀
      {tip && (
        <span id="night-only" className="tip" role="tooltip">
          {NIGHT_ONLY}
        </span>
      )}
    </button>
  );
}
```
`web/src/components/LiveDot.tsx`:
```tsx
import type { StreamState } from '../api/stream';

/** Solid while the stream is connected, hollow while reconnecting (spec §5 header). */
export function LiveDot({ state }: { state: StreamState }) {
  const live = state === 'live';
  return (
    <span className="live" role="status" aria-live="polite">
      <span className={live ? 'dot' : 'dot hollow'} aria-hidden="true" />
      {live ? 'LIVE' : 'RECONNECTING'}
    </span>
  );
}
```

- [ ] **Step 11: Write the shell.** `web/src/App.tsx`:
```tsx
import { NavLink, Route, Routes } from 'react-router';
import { useStream } from './api/StreamContext';
import { EmptyState } from './components/EmptyState';
import { FakeToggle } from './components/FakeToggle';
import { LiveDot } from './components/LiveDot';

const NAV = [
  { to: '/', label: 'RADAR', end: true },
  { to: '/episodes', label: 'EPISODES', end: false },
  { to: '/paper', label: 'PAPER', end: false },
  { to: '/health', label: 'HEALTH', end: false },
];

export function Header() {
  const { state } = useStream();
  return (
    <header className="top">
      <span className="logo">♥ cute-trading</span>
      <nav className="nav" aria-label="main">
        {NAV.map((n) => (
          <NavLink key={n.to} to={n.to} end={n.end} className={({ isActive }) => (isActive ? 'on' : undefined)}>
            {n.label}
          </NavLink>
        ))}
      </nav>
      <LiveDot state={state} />
      <FakeToggle />
    </header>
  );
}

export function NotFound() {
  return <EmptyState text="This page does not exist. Neither does free money." />;
}

export function App() {
  return (
    <div className="app">
      <Header />
      <main className="body">
        <Routes>
          <Route path="*" element={<NotFound />} />
        </Routes>
      </main>
    </div>
  );
}
```
`web/src/main.tsx`:
```tsx
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router';
import { App } from './App';
import { StreamProvider } from './api/StreamContext';
import './theme.css';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <BrowserRouter>
        <StreamProvider>
          <App />
        </StreamProvider>
      </BrowserRouter>
    </StrictMode>,
  );
}
```

- [ ] **Step 12: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test`
Expected: PASS: 10 test files, 0 failures.

- [ ] **Step 13: Type-check and build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm run build && grep -c "<script" dist/index.html && grep -L "<script>" dist/index.html`
Expected: `tsc` clean; Vite writes `dist/`; exactly one `<script type="module" src="/assets/…">` in `dist/index.html` and no inline `<script>` (the `grep -L` prints the file name).

- [ ] **Step 14: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add .gitignore .dockerignore web/package.json web/package-lock.json web/tsconfig.json web/vite.config.ts web/index.html web/src/main.tsx web/src/App.tsx web/src/App.test.tsx web/src/theme.css web/src/api web/src/lib web/src/components web/src/test && git commit -m "web: Vite + React + TS scaffold, night-shift tokens, pixel components, api layer, tests"
```

---

### Task 8: web: Radar

**Files:**
- Create: `web/src/pages/Radar.tsx`, `web/src/test/fixtures.ts`
- Modify: `web/src/App.tsx` (route `/`)
- Test: `web/src/pages/Radar.test.tsx`

**Interfaces:**
- Consumes (Task 7): `api.healthNow/openEpisodes/today`, `useApi`, `useStream`, `Box`, `Tag`, `Table`, `Column`, `Hearts`, `troubles`, `Spark`, `Money`, `Age`, `EmptyState`, `Loadable`, `useNow`, `useTicker`, `formatDecimal`, `kindTag`, `mockApi`, `jsonResponse`.
- Produces: `Radar` page; exported copy `EMPTY_RADAR`, `NO_HEALTH`; `todayQuip(gross, net)`; `msgsPerSecond(msgsPerMinute)`; fixtures `healthFixture(o?)`, `episodeSummaryFixture(o?)`, `todayFixture(gross, net)` (reused by Tasks 9–10).

- [ ] **Step 1: Write the fixtures.** Create `web/src/test/fixtures.ts`:
```ts
import type { EpisodeSummary, HealthNow, Today } from '../api/types';

export function healthFixture(o: Partial<HealthNow> = {}): HealthNow {
  return {
    now_ms: 1_790_000_000_000, age_ms: 30_000, run_id: 3, minute_ms: 1_789_999_940_000, tokens: 4000, shards: 40,
    parked_shards: 0, msgs: 258_000, drops: 0, drops_5m: 0, reconnects: 0, lag_p50_ms: 78, lag_p99_ms: 1900,
    rest_p50_ms: 80, rest_p99_ms: 300, clock_offset_ms: 3, ok: true, capture_paused: false, dropped_groups: 0,
    ...o,
  };
}

export function episodeSummaryFixture(o: Partial<EpisodeSummary> = {}): EpisodeSummary {
  return {
    id: 'live-1-00000001', kind: 'no_basket', realization: 'converter', event_title: 'Bitcoin price on September 26?',
    event_slug: 'bitcoin', n_legs: 7, s_legs: 5, opened_ms: 1_790_000_000_000, closed_ms: null, duration_ms: null,
    edge: '0.0049', q_star: '83', net: '0.41', gross: '1.37', suspect: false, stale: false, censored: false,
    ...o,
  };
}

export function todayFixture(gross: number, net: number): Today {
  return {
    day_start_ms: 1_789_948_800_000,
    gross,
    net,
    hourly: Array.from({ length: 24 }, (_, hour) => ({ hour, gross: hour === 1 ? gross : 0, net: hour === 1 ? net : 0 })),
  };
}
```

- [ ] **Step 2: Write the failing tests.** Create `web/src/pages/Radar.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture, healthFixture, todayFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { EMPTY_RADAR, NO_HEALTH, Radar, msgsPerSecond, todayQuip } from './Radar';

const renderRadar = () =>
  render(
    <MemoryRouter>
      <Radar />
    </MemoryRouter>,
  );

describe('Radar', () => {
  it('shows the deadpan empty state and ten hearts on a quiet, healthy day', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByText(EMPTY_RADAR)).toBeInTheDocument();
    expect(EMPTY_RADAR).toBe('0 opportunities. The market is efficient, or we are slow. Probably both.');
    expect(await screen.findByRole('img', { name: 'health 10 of 10' })).toBeInTheDocument();
    expect(screen.getByText('OPEN EPISODES · 0')).toBeInTheDocument();
    expect(screen.getByText('40/40')).toBeInTheDocument();
    expect(screen.getByText('4.3k')).toBeInTheDocument();
  });

  it('lists open episodes with a link to the detail, signed money and Q*', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [episodeSummaryFixture()] },
      '/api/today': todayFixture(1, 1),
    });
    renderRadar();
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    expect(link).toHaveAttribute('href', '/episodes/live-1-00000001');
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('+$1.37')).toBeInTheDocument();
    expect(screen.getByText('83')).toBeInTheDocument();
    expect(screen.getByText('NO')).toBeInTheDocument();
    expect(screen.getByText('OPEN EPISODES · 1')).toBeInTheDocument();
  });

  it('says today in the spec voice', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(372, 41),
    });
    renderRadar();
    expect(await screen.findByText('372 gross, 41 net. "Fees: 331. Us: 41."')).toBeInTheDocument();
    expect(todayQuip(3, 0)).toBe('3 gross, 0 net. "Fees: 3. Us: 0."');
    expect(msgsPerSecond(59)).toBe('1');
  });

  it('names health troubles in text, not colour alone', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ drops_5m: 2, capture_paused: true }) },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByRole('img', { name: /^health 8 of 10/ })).toBeInTheDocument();
    expect(screen.getByText('DROPS IN THE LAST 5 MIN')).toBeInTheDocument();
    expect(screen.getByText('CAPTURE PAUSED')).toBeInTheDocument();
  });

  it('explains a missing health row and a busy database instead of going blank', async () => {
    mockApi({
      '/api/health/now': { health: null },
      '/api/episodes/open': () =>
        jsonResponse(503, { error: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' }),
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    expect(await screen.findByText(NO_HEALTH)).toBeInTheDocument();
    expect(await screen.findByText('The database is thinking. Try again in a moment.')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test -- src/pages/Radar.test.tsx`
Expected: FAIL: `Failed to resolve import "./Radar"`.

- [ ] **Step 4: Write the page.** Create `web/src/pages/Radar.tsx`:
```tsx
import { Link } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { EpisodeSummary, HealthNow } from '../api/types';
import { useApi } from '../api/useApi';
import { Age } from '../components/Age';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Hearts, troubles } from '../components/Hearts';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Spark } from '../components/Spark';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { useNow, useTicker } from '../lib/hooks';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';

export const EMPTY_RADAR = '0 opportunities. The market is efficient, or we are slow. Probably both.';
export const NO_HEALTH = 'No health rows yet. The scanner may be asleep.';

/** Spec §5: N gross, M net. "Fees: N−M. Us: M." */
export function todayQuip(gross: number, net: number): string {
  return `${gross} gross, ${net} net. "Fees: ${gross - net}. Us: ${net}."`;
}

/** Messages per minute → per second, "4.3k" style. Counts, not money. */
export function msgsPerSecond(msgsPerMinute: number): string {
  const r = Math.round(msgsPerMinute / 60);
  return r >= 1000 ? `${Math.floor(r / 1000)}.${Math.floor((r % 1000) / 100)}k` : String(r);
}

function HealthBar({ h }: { h: HealthNow }) {
  return (
    <section className="box health" aria-label="scanner health">
      <Hearts health={h} />
      <span className="stat">
        shards <b>{`${h.shards - h.parked_shards}/${h.shards}`}</b>
      </span>
      <span className="stat">
        msgs/s <b>{msgsPerSecond(h.msgs)}</b>
      </span>
      <span className="stat">
        lag p50 <b>{h.lag_p50_ms === null ? '—' : `${h.lag_p50_ms} ms`}</b>
      </span>
      <span className="stat">
        drops <b>{h.drops_5m}</b>
      </span>
      {troubles(h).map((t) => (
        <Tag key={t} tone="flag">
          {t.toUpperCase()}
        </Tag>
      ))}
    </section>
  );
}

/** Edge bar: geometry only (the number never reaches the screen as text). */
function EdgeBar({ edge, max }: { edge: string; max: number }) {
  const v = Number(edge);
  const w = max > 0 && v > 0 ? Math.max(1, Math.round((v / max) * 60)) : 0;
  return (
    <svg className="edgebar" width="60" height="10" aria-hidden="true">
      <rect x="0" y="0" width={w} height="10" />
    </svg>
  );
}

export function Radar() {
  const { episodeTick, healthTick } = useStream();
  const slow = useTicker(30_000);
  const health = useApi((s) => api.healthNow(s), [healthTick, slow]);
  const open = useApi((s) => api.openEpisodes(s), [episodeTick, slow]);
  const today = useApi((s) => api.today(s), [episodeTick, slow]);
  const now = useNow(100);
  const rows = open.data?.items ?? [];
  const maxEdge = Math.max(0, ...rows.map((r) => Number(r.edge)));
  const columns: Column<EpisodeSummary>[] = [
    {
      key: 'kind',
      label: 'KIND',
      render: (r) => {
        const t = kindTag(r.kind);
        return <Tag tone={t.tone}>{t.text}</Tag>;
      },
    },
    {
      key: 'event',
      label: 'EVENT',
      render: (r) => <Link to={`/episodes/${encodeURIComponent(r.id)}`}>{r.event_title ?? r.id}</Link>,
    },
    { key: 's', label: 'S', render: (r) => r.s_legs },
    { key: 'net', label: 'NET', render: (r) => <Money value={r.net} signed tone /> },
    { key: 'gross', label: 'GROSS', render: (r) => <Money value={r.gross} signed /> },
    { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q_star) },
    { key: 'age', label: 'AGE', render: (r) => <Age sinceMs={r.opened_ms} nowMs={now} /> },
    { key: 'edge', label: 'EDGE', render: (r) => <EdgeBar edge={r.edge} max={maxEdge} /> },
  ];
  return (
    <>
      <Loadable state={health} what="health">
        {(d) => (d.health === null ? <Box><EmptyState text={NO_HEALTH} /></Box> : <HealthBar h={d.health} />)}
      </Loadable>
      <Box title={`OPEN EPISODES · ${rows.length}`}>
        <Loadable state={open} what="open episodes">
          {(d) =>
            d.items.length === 0 ? (
              <EmptyState text={EMPTY_RADAR} />
            ) : (
              <Table columns={columns} rows={d.items} rowKey={(r) => r.id} />
            )
          }
        </Loadable>
      </Box>
      <Box title="TODAY (UTC)">
        <Loadable state={today} what="today">
          {(d) => (
            <>
              <Spark values={d.hourly.map((h) => h.gross)} label="gross episodes per hour today" />
              <p className="quip">{todayQuip(d.gross, d.net)}</p>
            </>
          )}
        </Loadable>
      </Box>
    </>
  );
}
```

- [ ] **Step 5: Route it.** In `web/src/App.tsx` add `import { Radar } from './pages/Radar';` and, inside `<Routes>` before the `*` route, `<Route path="/" element={<Radar />} />`.

- [ ] **Step 6: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test`
Expected: PASS (all files, including 5 Radar tests; `App.test.tsx` still passes because it renders `/nowhere`).

- [ ] **Step 7: Build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm run build`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/src/pages/Radar.tsx web/src/pages/Radar.test.tsx web/src/test/fixtures.ts web/src/App.tsx && git commit -m "web: Radar (hearts, live open episodes, today in the spec voice)"
```

---
### Task 9: web: Episodes (filters in the URL, keyset pages) and EpisodeDetail

**Files:**
- Create: `web/src/pages/Episodes.tsx`, `web/src/pages/EpisodeDetail.tsx`
- Modify: `web/src/App.tsx` (2 routes), `web/src/test/fixtures.ts` (append `episodeDetailFixture`)
- Test: `web/src/pages/Episodes.test.tsx`, `web/src/pages/EpisodeDetail.test.tsx`

**Interfaces:**
- Consumes (Tasks 7–8): `api.episodes`, `api.episode`, `EpisodeQuery`, `useApi`, `Box`, `Tag`, `Table`, `Money`, `EmptyState`, `Loadable`, `kindTag`, `formatDecimal`, `signOf`, `DAY_MS`, `fmtDuration`, `fmtLocal`, `fmtUtcDateTime`, `fmtUtcTime`, `parseIsoDay`, `episodeSummaryFixture`, `mockApi`, `jsonResponse`.
- Produces: `Episodes` page + `queryFromParams(sp: URLSearchParams) => EpisodeQuery` + `NO_EPISODES`; `EpisodeDetail` page + `FLAG_WORDS`, `flagSentences(e: Episode) => string[]`, `censorText(e: Episode) => string`, `recheckLabel(offsetMs: number, maxSecondsDelay: number | null) => string`, `actionSide(action: string) => string`; fixture `episodeDetailFixture(o?)`.
- URL contract (Episodes): `?kind=&from=YYYY-MM-DD&to=YYYY-MM-DD&net=1&flags=suspect,stale,censored&cursor=` (days are UTC; `to` is inclusive; changing any filter drops `cursor`).

- [ ] **Step 1: Add the detail fixture.** Append to `web/src/test/fixtures.ts`:
```ts
import type { EpisodeDetail } from '../api/types';

export function episodeDetailFixture(o: Partial<EpisodeDetail['episode']> = {}): EpisodeDetail {
  const opened = Date.UTC(2026, 8, 25, 1, 12, 4, 210);
  return {
    episode: {
      id: 'live-1-00000001', run_id: 1, source: 'live', kind: 'no_basket', realization: 'converter', event_id: 'ev1',
      event_title: 'Bitcoin price on September 26?', event_slug: 'bitcoin', category: null, n_outcomes: 7, n_legs: 7,
      legs_dropped: 0, augmented: false, incomplete_set: false, uncovered_legs: 0, opened_ms: opened,
      closed_ms: opened + 4190, duration_ms: 4190, censor_left: false, censor_right: false, censor_gap: false,
      detect_lag_ms: 84, n_updates: 3, peak_ms: opened + 1000, edge: '0.0049', edge_gross: '0.0137', q_star: '83',
      cost: '99.59', payout: '100', fees: '0', net: '0.41', gross: '1.37', profit_per_dollar: '0.0041',
      net_budget100: '0.33', bottleneck_leg_idx: 0, below_min_size: false, max_seconds_delay: 3, days_to_end: null,
      annualized_return: null, post_end: false, suspect: true, stale: false, open_lag_ms: 12, oracle_downgraded: null,
      scanner_version: 'sha',
      ...o,
    },
    legs: [
      { leg_idx: 0, market_id: 'm0', action: 'buy_no', best_price: '0.62', vwap: '0.6231', levels: 2, size_at_best: '140', filled: '83', notional: '51.7', fees: '0.21', in_s: true },
      { leg_idx: 1, market_id: 'm1', action: 'buy_no', best_price: null, vwap: null, levels: 0, size_at_best: null, filled: '0', notional: '0', fees: '0.07', in_s: false },
    ],
    observations: [
      { offset_ms: 1000, source: 'rest', observed_ms: opened + 1000, skipped: false, skip_reason: null, edge: '0.0022', q_star: '61', pnl: '0.2', still_profitable: true, fill_at_original_size: false },
      { offset_ms: 3000, source: 'rest', observed_ms: opened + 3000, skipped: false, skip_reason: null, edge: '-0.0008', q_star: '0', pnl: '0', still_profitable: false, fill_at_original_size: false },
      { offset_ms: 5000, source: 'rest', observed_ms: null, skipped: true, skip_reason: 'recheck capacity', edge: null, q_star: null, pnl: null, still_profitable: null, fill_at_original_size: null },
    ],
  };
}
```
(Move the new `import type` line up to join the existing type import at the top of the file so the file has one import statement.)

- [ ] **Step 2: Write the failing tests.** `web/src/pages/Episodes.test.tsx`:
```tsx
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { Episodes, NO_EPISODES } from './Episodes';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/episodes"
          element={
            <>
              <Episodes />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

describe('Episodes', () => {
  it('reads its filters from the URL and sends them to the api', async () => {
    const f = mockApi({
      '/api/episodes': { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190, suspect: true })], next_cursor: null },
    });
    renderAt('/episodes?kind=no_basket&net=1&flags=suspect&from=2026-09-20&to=2026-09-25');
    expect(await screen.findByRole('link', { name: 'Bitcoin price on September 26?' })).toHaveAttribute('href', '/episodes/live-1-00000001');
    const url = String(f.mock.calls[0][0]);
    for (const part of ['kind=no_basket', 'net=1', 'flags=suspect', `from=${Date.UTC(2026, 8, 20)}`, `to=${Date.UTC(2026, 8, 26)}`, 'limit=50']) {
      expect(url).toContain(part);
    }
    expect(screen.getByText('4.19 s')).toBeInTheDocument();
    expect(screen.getByText('SUSPECT')).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'net-positive only' })).toBeChecked();
  });

  it('changing a filter updates the URL and drops the cursor', async () => {
    const f = mockApi({ '/api/episodes': { items: [], next_cursor: null } });
    renderAt('/episodes?cursor=1_live-1-x');
    expect(await screen.findByText(NO_EPISODES)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'YES' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?kind=yes_basket');
    await waitFor(() => expect(String(f.mock.calls.at(-1)?.[0])).toContain('kind=yes_basket'));
    expect(String(f.mock.calls.at(-1)?.[0])).not.toContain('cursor');
  });

  it('pages older with the cursor and back to the newest', async () => {
    mockApi({
      '/api/episodes': (url: URL) =>
        url.searchParams.get('cursor')
          ? { items: [episodeSummaryFixture({ id: 'live-1-00000000', event_title: 'Older one' })], next_cursor: null }
          : { items: [episodeSummaryFixture()], next_cursor: '1790000000000_live-1-00000001' },
    });
    renderAt('/episodes');
    await screen.findByText('Bitcoin price on September 26?');
    expect(screen.getByRole('button', { name: 'NEWEST' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'OLDER →' }));
    expect(await screen.findByText('Older one')).toBeInTheDocument();
    expect(screen.getByTestId('loc')).toHaveTextContent('cursor=1790000000000_live-1-00000001');
    expect(screen.getByRole('button', { name: 'OLDER →' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'NEWEST' }));
    expect(await screen.findByText('Bitcoin price on September 26?')).toBeInTheDocument();
  });

  it('shows the api detail for a range that is too long', async () => {
    mockApi({
      '/api/episodes': () => jsonResponse(400, { error: 'bad_range', detail: 'from must be before to, and seven days apart at most.' }),
    });
    renderAt('/episodes?from=2026-09-01&to=2026-09-25');
    expect(await screen.findByText('from must be before to, and seven days apart at most.')).toBeInTheDocument();
  });
});
```
`web/src/pages/EpisodeDetail.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router';
import { describe, expect, it } from 'vitest';
import { episodeDetailFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { EpisodeDetail, censorText, recheckLabel } from './EpisodeDetail';

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/episodes/:id" element={<EpisodeDetail />} />
      </Routes>
    </MemoryRouter>,
  );

describe('EpisodeDetail', () => {
  it('shows timing, the peak, flags in words, legs with S dimming, and the re-check timeline', async () => {
    mockApi({ '/api/episodes/live-1-00000001': episodeDetailFixture() });
    const { container } = renderAt('/episodes/live-1-00000001');
    expect(await screen.findByText('Bitcoin price on September 26?')).toBeInTheDocument();
    expect(screen.getByText('NO BASKET')).toBeInTheDocument();
    expect(screen.getByText('SUSPECT')).toBeInTheDocument();
    expect(screen.getByText(/^SUSPECT: one leg's book disagreed with the server's top/)).toBeInTheDocument();
    expect(screen.getByText('01:12:04.210 UTC')).toBeInTheDocument();
    expect(screen.getByText('4.19 s')).toBeInTheDocument();
    expect(screen.getByText('84 ms')).toBeInTheDocument();
    expect(screen.getByText('none')).toBeInTheDocument();
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('$0.00')).toBeInTheDocument();
    expect(screen.getByText('83 sets')).toBeInTheDocument();
    expect(screen.getByText('+$0.33')).toBeInTheDocument();
    expect(screen.getByText('LEGS AT PEAK · S = 1 of 7')).toBeInTheDocument();
    expect(screen.getByText('#1 m1').closest('tr')).toHaveClass('dim');
    expect(screen.getByText('#0 m0').closest('tr')).not.toHaveClass('dim');
    for (const t of ['peak', '+1 s', '+Δd (3 s)', '+5 s', 'skipped: recheck capacity', '+0.0022', '−0.0008']) {
      expect(screen.getByText(t)).toBeInTheDocument();
    }
    expect(container.querySelectorAll('svg.tl rect.bad')).toHaveLength(1);
    expect(container.querySelectorAll('svg.tl rect.skip')).toHaveLength(1);
    expect(screen.getByText('Gone in 4.19 s. Like most good ideas.')).toBeInTheDocument();
  });

  it('a missing episode is a deadpan 404, not a blank page', async () => {
    mockApi({});
    renderAt('/episodes/nope');
    expect(await screen.findByText('Nothing here. It may have been a dream.')).toBeInTheDocument();
  });

  it('labels re-checks and censoring in words', () => {
    expect(recheckLabel(1000, 0)).toBe('+1 s');
    expect(recheckLabel(3000, 3)).toBe('+Δd (3 s)');
    expect(recheckLabel(1500, null)).toBe('+1.50 s');
    const e = episodeDetailFixture({ censor_left: true, censor_gap: true }).episode;
    expect(censorText(e)).toBe('left, gap');
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test -- src/pages/Episodes.test.tsx src/pages/EpisodeDetail.test.tsx`
Expected: FAIL: `Failed to resolve import "./Episodes"` / `"./EpisodeDetail"`.

- [ ] **Step 4: Write the Episodes page.** Create `web/src/pages/Episodes.tsx`:
```tsx
import { Link, useSearchParams } from 'react-router';
import { api, type EpisodeQuery } from '../api/client';
import type { EpisodeSummary } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { kindTag } from '../lib/labels';
import { formatDecimal } from '../lib/money';
import { DAY_MS, fmtDuration, fmtLocal, fmtUtcDateTime, parseIsoDay } from '../lib/time';

export const NO_EPISODES = 'No episodes match. The filters are working; the market is not.';
const KINDS: [string, string][] = [
  ['', 'ALL'],
  ['no_basket', 'NO'],
  ['yes_basket', 'YES'],
  ['yes_basket_incomplete', 'YES?'],
  ['implication', 'IMPL'],
];
const FLAGS = ['suspect', 'stale', 'censored'];

/** URL → api query. Days are UTC; the URL's `to` day is inclusive, the api's `to` is exclusive. */
export function queryFromParams(sp: URLSearchParams): EpisodeQuery {
  const from = parseIsoDay(sp.get('from') ?? '');
  const to = parseIsoDay(sp.get('to') ?? '');
  return {
    kind: sp.get('kind') || undefined,
    from: from ?? undefined,
    to: to === null ? undefined : to + DAY_MS,
    net: sp.get('net') === '1',
    flags: (sp.get('flags') ?? '').split(',').filter((f) => f !== ''),
    cursor: sp.get('cursor'),
    limit: 50,
  };
}

function FlagTags({ e }: { e: EpisodeSummary }) {
  return (
    <>
      {e.suspect && <Tag tone="flag">SUSPECT</Tag>}
      {e.stale && <Tag tone="flag">STALE</Tag>}
      {e.censored && <Tag tone="flag">CENSORED</Tag>}
    </>
  );
}

const columns: Column<EpisodeSummary>[] = [
  { key: 'opened', label: 'OPENED', render: (r) => <span title={fmtLocal(r.opened_ms)}>{fmtUtcDateTime(r.opened_ms)}</span> },
  {
    key: 'kind',
    label: 'KIND',
    render: (r) => {
      const t = kindTag(r.kind);
      return <Tag tone={t.tone}>{t.text}</Tag>;
    },
  },
  { key: 'event', label: 'EVENT', render: (r) => <Link to={`/episodes/${encodeURIComponent(r.id)}`}>{r.event_title ?? r.id}</Link> },
  { key: 's', label: 'S', render: (r) => `${r.s_legs}/${r.n_legs}` },
  { key: 'net', label: 'NET', render: (r) => <Money value={r.net} signed tone /> },
  { key: 'gross', label: 'GROSS', render: (r) => <Money value={r.gross} signed /> },
  { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q_star) },
  { key: 'dur', label: 'DURATION', render: (r) => (r.duration_ms === null ? 'open' : fmtDuration(r.duration_ms)) },
  { key: 'flags', label: 'FLAGS', render: (r) => <FlagTags e={r} /> },
];

export function Episodes() {
  const [sp, setSp] = useSearchParams();
  const key = sp.toString();
  const q = queryFromParams(sp);
  const page = useApi((s) => api.episodes(queryFromParams(sp), s), [key]);
  const update = (name: string, value: string | null) => {
    const next = new URLSearchParams(sp);
    if (value) next.set(name, value);
    else next.delete(name);
    if (name !== 'cursor') next.delete('cursor');
    setSp(next);
  };
  const toggleFlag = (f: string) => {
    const on = new Set(q.flags);
    if (on.has(f)) on.delete(f);
    else on.add(f);
    update('flags', FLAGS.filter((x) => on.has(x)).join(',') || null);
  };
  const kind = q.kind ?? '';
  return (
    <>
      <Box title="FILTERS">
        <div className="filters">
          <div className="seg" role="group" aria-label="kind">
            {KINDS.map(([value, label]) => (
              <button key={label} type="button" className={kind === value ? 'on' : undefined} aria-pressed={kind === value} onClick={() => update('kind', value || null)}>
                {label}
              </button>
            ))}
          </div>
          <label>
            from <input type="date" value={sp.get('from') ?? ''} onChange={(e) => update('from', e.target.value || null)} />
          </label>
          <label>
            to <input type="date" value={sp.get('to') ?? ''} onChange={(e) => update('to', e.target.value || null)} />
          </label>
          <label>
            <input type="checkbox" checked={q.net ?? false} onChange={(e) => update('net', e.target.checked ? '1' : null)} />
            net-positive only
          </label>
          {FLAGS.map((f) => (
            <label key={f}>
              <input type="checkbox" checked={q.flags?.includes(f) ?? false} onChange={() => toggleFlag(f)} />
              {f}
            </label>
          ))}
        </div>
        <p className="dim">UTC days. Seven days at a time.</p>
      </Box>
      <Box title="EPISODES">
        <Loadable state={page} what="episodes">
          {(d) =>
            d.items.length === 0 ? (
              <EmptyState text={NO_EPISODES} />
            ) : (
              <>
                <Table columns={columns} rows={d.items} rowKey={(r) => r.id} />
                <div className="pager">
                  <button type="button" disabled={!q.cursor} onClick={() => update('cursor', null)}>
                    NEWEST
                  </button>
                  <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && update('cursor', d.next_cursor)}>
                    OLDER →
                  </button>
                </div>
              </>
            )
          }
        </Loadable>
      </Box>
    </>
  );
}
```

- [ ] **Step 5: Write the detail page.** Create `web/src/pages/EpisodeDetail.tsx`:
```tsx
import { Link, useParams } from 'react-router';
import { api } from '../api/client';
import type { Episode, EpisodeDetail as Detail, Leg, Observation } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { Tag } from '../components/Tag';
import { kindTag } from '../lib/labels';
import { formatDecimal, signOf } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcTime } from '../lib/time';

export const FLAG_WORDS = {
  suspect:
    "SUSPECT: one leg's book disagreed with the server's top while this was open. The stream repaired it; the numbers may still be optimistic.",
  stale: "STALE: at least one leg's feed went quiet while this was open, so its book may have been out of date.",
  censor_left: 'CENSORED (LEFT): it was already open when we started watching, so the real start is earlier.',
  censor_right: 'CENSORED (RIGHT): we stopped watching before it closed, so the real end is later.',
  censor_gap: 'CENSORED (GAP): the feed had a gap while it was open; the duration spans the gap.',
  incomplete: 'INCOMPLETE SET: the listed outcomes may not cover every result. Not an arbitrage if "Other" wins.',
  below_min: 'BELOW MIN SIZE: the size at the best price is under the venue minimum order.',
};

export function flagSentences(e: Episode): string[] {
  const out: string[] = [];
  if (e.suspect) out.push(FLAG_WORDS.suspect);
  if (e.stale) out.push(FLAG_WORDS.stale);
  if (e.censor_left) out.push(FLAG_WORDS.censor_left);
  if (e.censor_right) out.push(FLAG_WORDS.censor_right);
  if (e.censor_gap) out.push(FLAG_WORDS.censor_gap);
  if (e.incomplete_set) out.push(FLAG_WORDS.incomplete);
  if (e.below_min_size) out.push(FLAG_WORDS.below_min);
  return out;
}

export function censorText(e: Episode): string {
  const parts: string[] = [];
  if (e.censor_left) parts.push('left');
  if (e.censor_right) parts.push('right');
  if (e.censor_gap) parts.push('gap');
  return parts.length > 0 ? parts.join(', ') : 'none';
}

/** 1000 → "+1 s"; the offset equal to secondsDelay (Δd, spec §8.2) → "+Δd (3 s)". */
export function recheckLabel(offsetMs: number, maxSecondsDelay: number | null): string {
  const s = offsetMs % 1000 === 0 ? `${offsetMs / 1000} s` : fmtDuration(offsetMs);
  if (maxSecondsDelay !== null && maxSecondsDelay > 0 && offsetMs === maxSecondsDelay * 1000) return `+Δd (${s})`;
  return `+${s}`;
}

export function actionSide(action: string): string {
  if (action === 'buy_yes') return 'YES';
  if (action === 'buy_no') return 'NO';
  return action.toUpperCase();
}

function signedDec(v: string | null): string {
  if (v === null) return '—';
  return signOf(v) > 0 ? `+${formatDecimal(v)}` : formatDecimal(v);
}

function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 6)}…${id.slice(-4)}` : id;
}

const legColumns: Column<Leg>[] = [
  { key: 'leg', label: 'LEG', render: (l) => <span title={l.market_id}>{`#${l.leg_idx} ${shortId(l.market_id)}`}</span> },
  { key: 'buy', label: 'BUY', render: (l) => actionSide(l.action) },
  { key: 'best', label: 'BEST', render: (l) => formatDecimal(l.best_price) },
  { key: 'vwap', label: 'VWAP', render: (l) => formatDecimal(l.vwap) },
  { key: 'levels', label: 'LEVELS', render: (l) => l.levels },
  { key: 'size', label: 'SIZE@BEST', render: (l) => formatDecimal(l.size_at_best) },
  { key: 'fees', label: 'FEES', render: (l) => <Money value={l.fees} /> },
  { key: 's', label: 'IN S', render: (l) => (l.in_s ? '✓' : '·') },
];

interface TimelineRow {
  key: string;
  when: string;
  source: string;
  profitable: string;
  tone: 'pos' | 'neg' | undefined;
  edge: string | null;
  q: string | null;
}

const timelineColumns: Column<TimelineRow>[] = [
  { key: 'when', label: 'WHEN', render: (r) => r.when },
  { key: 'source', label: 'SOURCE', render: (r) => r.source },
  { key: 'ok', label: 'STILL PROFITABLE', render: (r) => r.profitable, className: (r) => r.tone },
  { key: 'edge', label: 'EDGE', render: (r) => signedDec(r.edge) },
  { key: 'q', label: 'Q*', render: (r) => formatDecimal(r.q) },
];

function timelineRows(e: Episode, obs: Observation[]): TimelineRow[] {
  // Open-time values aren't stored; the first row is the peak the episode reached.
  const peak: TimelineRow = { key: 'peak', when: 'peak', source: '—', profitable: 'yes', tone: 'pos', edge: e.edge, q: e.q_star };
  return [
    peak,
    ...obs.map((o, i): TimelineRow => ({
      key: `o${i}`,
      when: recheckLabel(o.offset_ms, e.max_seconds_delay),
      source: o.source,
      profitable: o.skipped
        ? `skipped: ${o.skip_reason ?? 'no reason given'}`
        : o.still_profitable === null
          ? '—'
          : o.still_profitable
            ? 'yes'
            : 'no',
      tone: o.skipped || o.still_profitable === null ? undefined : o.still_profitable ? 'pos' : 'neg',
      edge: o.edge,
      q: o.q_star,
    })),
  ];
}

function Timeline({ obs }: { obs: Observation[] }) {
  const max = Math.max(1, ...obs.map((o) => o.offset_ms));
  return (
    <svg className="tl" viewBox="0 0 100 14" preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label="re-check timeline">
      <rect className="axis" x={0} y={6} width={100} height={1} />
      <rect className="ok" x={1} y={3} width={2} height={7} />
      {obs.map((o, i) => {
        const x = Math.min(97, 4 + Math.round((o.offset_ms / max) * 93));
        const cls = o.skipped || o.still_profitable === null ? 'skip' : o.still_profitable ? 'ok' : 'bad';
        return <rect key={i} className={cls} x={x} y={3} width={2} height={7} />;
      })}
    </svg>
  );
}

function DetailView({ d }: { d: Detail }) {
  const e = d.episode;
  const k = kindTag(e.kind, true);
  const words = flagSentences(e);
  const inS = d.legs.filter((l) => l.in_s).length;
  return (
    <>
      <p className="title">
        {e.event_title ?? e.event_id} <Tag tone={k.tone}>{k.text}</Tag>
        {e.suspect && <Tag tone="flag">SUSPECT</Tag>}
        {e.stale && <Tag tone="flag">STALE</Tag>}
        {censorText(e) !== 'none' && <Tag tone="flag">CENSORED</Tag>}
        {e.source === 'replay' && <Tag>REPLAY</Tag>}
      </p>
      <div className="grid2">
        <Box title="TIMING">
          <dl className="kv">
            <dt>opened</dt>
            <dd title={fmtLocal(e.opened_ms)}>{fmtUtcTime(e.opened_ms)}</dd>
            <dt>closed</dt>
            <dd title={e.closed_ms === null ? undefined : fmtLocal(e.closed_ms)}>{e.closed_ms === null ? 'still open' : fmtUtcTime(e.closed_ms)}</dd>
            <dt>duration</dt>
            <dd>{e.duration_ms === null ? '—' : fmtDuration(e.duration_ms)}</dd>
            <dt>censoring</dt>
            <dd>{censorText(e)}</dd>
            <dt>detect lag</dt>
            <dd>{e.detect_lag_ms === null ? '—' : `${e.detect_lag_ms} ms`}</dd>
          </dl>
        </Box>
        <Box title="AT PEAK">
          <dl className="kv">
            <dt>net Π</dt>
            <dd><Money value={e.net} signed tone /></dd>
            <dt>gross Π</dt>
            <dd><Money value={e.gross} signed /></dd>
            <dt>fees</dt>
            <dd><Money value={e.fees} /></dd>
            <dt>Q*</dt>
            <dd>{`${formatDecimal(e.q_star)} sets`}</dd>
            <dt>$100 cap</dt>
            <dd><Money value={e.net_budget100} signed tone /></dd>
          </dl>
        </Box>
        <Box title="FLAGS, IN WORDS">
          {words.length === 0 ? <p className="quip">No flags. Suspiciously clean.</p> : words.map((w) => <p key={w} className="quip">{w}</p>)}
        </Box>
      </div>
      <Box title={`LEGS AT PEAK · S = ${inS} of ${e.n_legs}`}>
        <Table columns={legColumns} rows={d.legs} rowKey={(l) => String(l.leg_idx)} rowClass={(l) => (l.in_s ? undefined : 'dim')} />
      </Box>
      <Box title="RE-CHECK TIMELINE">
        <Timeline obs={d.observations} />
        <Table columns={timelineColumns} rows={timelineRows(e, d.observations)} rowKey={(r) => r.key} />
        {e.closed_ms !== null && e.duration_ms !== null && e.duration_ms < 10_000 && (
          <p className="quip">{`Gone in ${fmtDuration(e.duration_ms)}. Like most good ideas.`}</p>
        )}
      </Box>
    </>
  );
}

export function EpisodeDetail() {
  const { id = '' } = useParams();
  const d = useApi((s) => api.episode(id, s), [id]);
  return (
    <>
      <p className="crumb">
        <Link to="/episodes">EPISODES</Link> › {id}
      </p>
      <Loadable state={d} what="the episode">
        {(x) => <DetailView d={x} />}
      </Loadable>
    </>
  );
}
```

- [ ] **Step 6: Route them.** In `web/src/App.tsx` add
```tsx
import { EpisodeDetail } from './pages/EpisodeDetail';
import { Episodes } from './pages/Episodes';
```
and inside `<Routes>`, after the `/` route:
```tsx
          <Route path="/episodes" element={<Episodes />} />
          <Route path="/episodes/:id" element={<EpisodeDetail />} />
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test`
Expected: PASS (4 Episodes tests, 3 EpisodeDetail tests, all earlier files).

- [ ] **Step 8: Build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm run build`
Expected: clean.

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/src/pages/Episodes.tsx web/src/pages/Episodes.test.tsx web/src/pages/EpisodeDetail.tsx web/src/pages/EpisodeDetail.test.tsx web/src/test/fixtures.ts web/src/App.tsx && git commit -m "web: Episodes (URL filters, keyset pages) and EpisodeDetail (timing, peak, flags in words, legs, re-checks)"
```

---

### Task 10: web: Paper and Health

**Files:**
- Create: `web/src/pages/Paper.tsx`, `web/src/pages/Health.tsx`
- Modify: `web/src/App.tsx` (2 routes), `web/src/test/fixtures.ts` (append paper and health fixtures)
- Test: `web/src/pages/Paper.test.tsx`, `web/src/pages/Health.test.tsx`

**Interfaces:**
- Consumes (Tasks 7–9): `api.paperAccounts/paperEquity/paperPositions/paperLedger/healthNow/healthSeries/integrity`, `useApi`, `useStream`, `Box`, `Tag`, `Table`, `LineChart`, `Money`, `EmptyState`, `Loadable`, `formatMoney`, `formatDecimal`, `isZero`, `fmtUtcDateTime`, `fmtDuration`, `healthFixture`, `mockApi`.
- Produces: `Paper` + `PAPER_QUIP`, `PAPER_EMPTY`, `NOTHING_AWAITING`, `policyLabel(a)`, `policyCardTitle(policy)`, `extremes(values: string[])`; `Health` + `NO_HEALTH_ROWS`, `NO_INTEGRITY`; fixtures `accountFixture(o?)`, `positionFixture(o?)`, `ledgerFixture(o?)`, `healthPointFixture(minute_ms, reconnects)`.
- URL contract: Paper `?scenario=&account=&lcursor=`; Health `?hours=6|24|168`.

- [ ] **Step 1: Add the fixtures.** Append to `web/src/test/fixtures.ts` (and merge the type imports into the single import line at the top: `EpisodeDetail, EpisodeSummary, HealthNow, HealthPoint, LedgerEntry, PaperAccount, Position, Today`):
```ts
export function accountFixture(o: Partial<PaperAccount> = {}): PaperAccount {
  return {
    account_id: 'P0-converter/sp-idle', policy: 'P0-converter', scenario: 'sp-idle', fee_bips: '0', bankroll: '1000',
    run_id: 3, marked_ms: 1_790_000_000_000, cash: '900', equity: '1002.56', realized: '2.56', unrealized: '0',
    fees_paid: '0.21', locked: '0', max_drawdown: '0.4', n_positions: 0,
    ...o,
  };
}

export function positionFixture(o: Partial<Position> = {}): Position {
  return {
    account_id: 'P3-naive/sp-idle', market_id: 'm1', outcome: 'yes', qty: '40', basis: '18', returned: false,
    event_title: 'Fed decision in October?',
    ...o,
  };
}

export function ledgerFixture(o: Partial<LedgerEntry> = {}): LedgerEntry {
  return {
    run_id: 3, account_id: 'P0-converter/sp-idle', seq: 1, t_ms: Date.UTC(2026, 8, 25, 1, 12, 4), kind: 'buy',
    episode_id: 'live-1-00000001', market_id: 'm1', outcome: 'no', qty: '51.2', price: '0.62', cash_delta: '-99.84',
    fees: '0.21', realized_delta: '0', cash_after: '900.16', equity: null,
    ...o,
  };
}

export function healthPointFixture(minute_ms: number, reconnects: number): HealthPoint {
  return {
    minute_ms, run_id: 3, msgs: 258_000, drops: 0, reconnects, lag_p50_ms: 78, lag_p99_ms: 1900, shards: 40,
    parked_shards: 0, capture_paused: false,
  };
}
```

- [ ] **Step 2: Write the failing tests.** `web/src/pages/Paper.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { accountFixture, ledgerFixture, positionFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { PAPER_EMPTY, PAPER_QUIP, Paper, extremes, policyLabel } from './Paper';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/paper"
          element={
            <>
              <Paper />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

const accounts = [
  accountFixture(),
  accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive', equity: '713.4', realized: '-286.6', n_positions: 1 }),
  accountFixture({ account_id: 'P0-converter/colo-idle', scenario: 'colo-idle', equity: '1000' }),
];

describe('Paper', () => {
  it('says nothing ventured when there are no paper accounts (S3c not deployed)', async () => {
    mockApi({ '/api/paper/accounts': { items: [] }, '/api/paper/positions': { items: [] } });
    renderAt('/paper');
    expect(await screen.findByText(PAPER_EMPTY)).toBeInTheDocument();
    expect(PAPER_EMPTY).toBe('No paper trades yet. Nothing ventured, nothing lost.');
  });

  it('switches scenario and shows one line per policy, cards, positions and the ledger', async () => {
    const f = mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [positionFixture()] },
      '/api/paper/equity': (url: URL) => ({
        account_id: url.searchParams.get('account'),
        from_ms: 0,
        to_ms: 100,
        points: [
          { t_ms: 0, equity: '1000' },
          { t_ms: 50, equity: url.searchParams.get('account')?.startsWith('P3') ? '713.4' : '1002.56' },
        ],
      }),
      '/api/paper/ledger': { items: [ledgerFixture()], next_cursor: null },
    });
    renderAt('/paper');
    expect(await screen.findByRole('button', { name: 'COLO-IDLE' })).toHaveClass('on');
    await userEvent.click(screen.getByRole('button', { name: 'SP-IDLE' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('scenario=sp-idle');
    expect(await screen.findByText('P3 naive')).toBeInTheDocument();
    expect(screen.getByText('P0 converter')).toBeInTheDocument();
    expect(screen.getByText(PAPER_QUIP)).toBeInTheDocument();
    expect(PAPER_QUIP).toBe("The naive strategy is doing great. Until 'Other' wins.");
    expect(screen.getByText('$1,002.56')).toBeInTheDocument();
    expect(screen.getByText('−$286.60')).toHaveClass('neg');
    expect(screen.getByText('Fed decision in October?')).toBeInTheDocument();
    expect(await screen.findByText('−$99.84')).toBeInTheDocument();
    const urls = f.mock.calls.map((c) => String(c[0]));
    expect(urls.some((u) => u.includes('/api/paper/equity?account=P3-naive%2Fsp-idle&hours=24'))).toBe(true);
    expect(urls.some((u) => u.includes('/api/paper/ledger?account=P0-converter%2Fsp-idle'))).toBe(true);
  });

  it('labels policies and finds extremes without printing floats', () => {
    expect(policyLabel({ policy: 'P0-converter', fee_bips: '0' })).toBe('P0 converter');
    expect(policyLabel({ policy: 'P0-converter', fee_bips: '10' })).toBe('P0 converter (fee 10 bp)');
    expect(extremes(['1000', '713.4', '1002.56'])).toEqual({ min: '713.4', max: '1002.56' });
    expect(extremes([])).toBeNull();
  });
});
```
`web/src/pages/Health.test.tsx`:
```tsx
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { healthFixture, healthPointFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Health, NO_HEALTH_ROWS } from './Health';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route
          path="/health"
          element={
            <>
              <Health />
              <Loc />
            </>
          }
        />
      </Routes>
    </MemoryRouter>,
  );

const integrity = {
  from_ms: 0, to_ms: 1, by_kind: [{ kind: 'top_mismatch', count: 2 }, { kind: 'drift', count: 1 }], total: 3, msgs: 258_000, per_million: '11.63',
};

describe('Health', () => {
  it('shows capture status, per-minute charts with reconnect markers, and integrity per million', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ capture_paused: true }) },
      '/api/health/series': { from_ms: 0, to_ms: 180_000, points: [healthPointFixture(0, 0), healthPointFixture(60_000, 2), healthPointFixture(120_000, 0)] },
      '/api/integrity/summary': integrity,
    });
    const { container } = renderAt('/health');
    expect(await screen.findByText('3 events in 258000 messages: 11.63 per million.')).toBeInTheDocument();
    expect(await screen.findByText('PAUSED')).toBeInTheDocument();
    expect(screen.getByText('top_mismatch')).toBeInTheDocument();
    await waitFor(() => expect(container.querySelectorAll('rect.marker')).toHaveLength(1));
  });

  it('keeps the window in the URL', async () => {
    const f = mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': { ...integrity, by_kind: [], total: 0, msgs: 0, per_million: null },
    });
    renderAt('/health?hours=168');
    expect(await screen.findByText(NO_HEALTH_ROWS)).toBeInTheDocument();
    expect(f.mock.calls.map((c) => String(c[0]))).toContain('/api/health/series?hours=168');
    await userEvent.click(screen.getByRole('button', { name: '6 H' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?hours=6');
    await waitFor(() => expect(f.mock.calls.map((c) => String(c[0]))).toContain('/api/health/series?hours=6'));
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test -- src/pages/Paper.test.tsx src/pages/Health.test.tsx`
Expected: FAIL: `Failed to resolve import "./Paper"` / `"./Health"`.

- [ ] **Step 4: Write the Paper page.** Create `web/src/pages/Paper.tsx`:
```tsx
import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { LedgerEntry, PaperAccount, Position } from '../api/types';
import { useApi, type ApiState } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { LineChart, type Series } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { formatDecimal, formatMoney, isZero } from '../lib/money';
import { fmtUtcDateTime } from '../lib/time';

export const PAPER_QUIP = "The naive strategy is doing great. Until 'Other' wins.";
export const PAPER_EMPTY = 'No paper trades yet. Nothing ventured, nothing lost.';
export const NOTHING_AWAITING = 'Nothing awaiting resolution. Patience is free.';
const TONES: Record<string, string> = { 'P0-converter': 'p0', 'P1-settlement': 'p1', 'P2-combo': 'p2', 'P3-naive': 'p3' };

/** "P0-converter" → "P0 converter" (+ the fee sensitivity when set). */
export function policyLabel(a: Pick<PaperAccount, 'policy' | 'fee_bips'>): string {
  const base = a.policy.replace('-', ' ');
  return isZero(a.fee_bips) ? base : `${base} (fee ${a.fee_bips} bp)`;
}

/** "P0-converter" → "P0 · CONVERTER". */
export function policyCardTitle(policy: string): string {
  const [p, name = ''] = policy.split('-');
  return `${p} · ${name.toUpperCase()}`;
}

/** Min and max of decimal strings: compared as numbers, returned exactly as sent (for labels). */
export function extremes(values: string[]): { min: string; max: string } | null {
  if (values.length === 0) return null;
  let min = values[0];
  let max = values[0];
  for (const v of values) {
    if (Number(v) < Number(min)) min = v;
    if (Number(v) > Number(max)) max = v;
  }
  return { min, max };
}

const ledgerColumns: Column<LedgerEntry>[] = [
  { key: 't', label: 'TIME', render: (l) => fmtUtcDateTime(l.t_ms) },
  { key: 'kind', label: 'KIND', render: (l) => l.kind },
  { key: 'qty', label: 'QTY', render: (l) => formatDecimal(l.qty) },
  { key: 'cash', label: 'CASH', render: (l) => <Money value={l.cash_delta} signed tone /> },
  { key: 'fees', label: 'FEES', render: (l) => <Money value={l.fees} /> },
  { key: 'realized', label: 'REALIZED', render: (l) => <Money value={l.realized_delta} signed tone /> },
];

const positionColumns: Column<Position>[] = [
  { key: 'acct', label: 'ACCT', render: (p) => p.account_id.split('-')[0] },
  { key: 'market', label: 'MARKET', render: (p) => <span title={p.market_id}>{p.event_title ?? p.market_id}</span> },
  { key: 'side', label: 'SIDE', render: (p) => `${p.outcome.toUpperCase()}${p.returned ? ' (returned)' : ''}` },
  { key: 'qty', label: 'QTY', render: (p) => formatDecimal(p.qty) },
  { key: 'basis', label: 'BASIS', render: (p) => <Money value={p.basis} /> },
];

function EquityChart({ accounts }: { accounts: PaperAccount[] }) {
  const ids = accounts.map((a) => a.account_id);
  const eq = useApi((s) => Promise.all(ids.map((id) => api.paperEquity(id, 24, s))), [ids.join('|')]);
  return (
    <Box title={`EQUITY · ${formatMoney(accounts[0]?.bankroll ?? null)} EACH · LAST 24 H`}>
      <Loadable state={eq} what="equity">
        {(all) => {
          const series: Series[] = all.map((e, i) => ({
            label: policyLabel(accounts[i]),
            tone: TONES[accounts[i].policy] ?? 'p0',
            // geometry only: the numbers never reach the screen as text
            points: e.points.map((p) => ({ x: p.t_ms, y: Number(p.equity) })),
          }));
          const ext = extremes(all.flatMap((e) => e.points.map((p) => p.equity)));
          return (
            <>
              <LineChart
                label="equity per account"
                xMin={Math.min(...all.map((e) => e.from_ms))}
                xMax={Math.max(...all.map((e) => e.to_ms))}
                series={series}
                yLabels={ext ? { bottom: formatMoney(ext.min), top: formatMoney(ext.max) } : undefined}
              />
              <p className="quip">{PAPER_QUIP}</p>
            </>
          );
        }}
      </Loadable>
    </Box>
  );
}

function Cards({ accounts }: { accounts: PaperAccount[] }) {
  return (
    <div className="cards">
      {accounts.map((a) => (
        <div key={a.account_id} className="card">
          <p className="p">
            {policyCardTitle(a.policy)}
            {!isZero(a.fee_bips) && ` · FEE ${a.fee_bips} BP`}
          </p>
          <p className="eq">
            <Money value={a.equity ?? a.bankroll} />
          </p>
          {a.marked_ms === null ? (
            <p className="dim">No marks yet. Nothing ventured, nothing lost.</p>
          ) : (
            <p>
              realized <Money value={a.realized} signed tone /> · unrealized <Money value={a.unrealized} signed tone /> · locked{' '}
              <Money value={a.locked} /> · max dd <Money value={a.max_drawdown} /> · {a.n_positions ?? 0} open
            </p>
          )}
        </div>
      ))}
    </div>
  );
}

function Ledger({ accounts, account, cursor, onAccount, onCursor }: {
  accounts: PaperAccount[];
  account: string;
  cursor: string | null;
  onAccount: (id: string) => void;
  onCursor: (c: string | null) => void;
}) {
  const page = useApi((s) => api.paperLedger(account, cursor, s), [account, cursor]);
  const current = accounts.find((a) => a.account_id === account);
  return (
    <Box title={`LEDGER · ${current ? policyCardTitle(current.policy) : account}`}>
      <div className="seg" role="group" aria-label="ledger account">
        {accounts.map((a) => (
          <button key={a.account_id} type="button" className={a.account_id === account ? 'on' : undefined} aria-pressed={a.account_id === account} onClick={() => onAccount(a.account_id)}>
            {policyCardTitle(a.policy)}
          </button>
        ))}
      </div>
      <Loadable state={page} what="the ledger">
        {(d) =>
          d.items.length === 0 ? (
            <EmptyState text="Empty ledger. Nothing ventured, nothing lost." />
          ) : (
            <>
              <Table columns={ledgerColumns} rows={d.items} rowKey={(l) => `${l.run_id}_${l.seq}`} />
              <div className="pager">
                <button type="button" disabled={!cursor} onClick={() => onCursor(null)}>
                  NEWEST
                </button>
                <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && onCursor(d.next_cursor)}>
                  OLDER →
                </button>
              </div>
            </>
          )
        }
      </Loadable>
    </Box>
  );
}

function PaperBody({ accounts, positions }: { accounts: PaperAccount[]; positions: ApiState<{ items: Position[] }> }) {
  const [sp, setSp] = useSearchParams();
  const scenarios = [...new Set(accounts.map((a) => a.scenario))].sort();
  const wanted = sp.get('scenario');
  const scenario = wanted !== null && scenarios.includes(wanted) ? wanted : scenarios[0];
  const inScenario = accounts.filter((a) => a.scenario === scenario);
  const ids = inScenario.map((a) => a.account_id);
  const wantedAccount = sp.get('account');
  const account = wantedAccount !== null && ids.includes(wantedAccount) ? wantedAccount : ids[0];
  const set = (changes: Record<string, string | null>) => {
    const next = new URLSearchParams(sp);
    for (const [k, v] of Object.entries(changes)) {
      if (v === null) next.delete(k);
      else next.set(k, v);
    }
    setSp(next);
  };
  return (
    <>
      <Box title="SCENARIO">
        <div className="seg" role="group" aria-label="latency scenario">
          {scenarios.map((s) => (
            <button key={s} type="button" className={s === scenario ? 'on' : undefined} aria-pressed={s === scenario} onClick={() => set({ scenario: s, account: null, lcursor: null })}>
              {s.toUpperCase()}
            </button>
          ))}
        </div>
      </Box>
      <EquityChart accounts={inScenario} />
      <Cards accounts={inScenario} />
      <div className="grid2">
        <Box title="AWAITING RESOLUTION">
          <Loadable state={positions} what="positions">
            {(d) => {
              const rows = d.items.filter((p) => ids.includes(p.account_id));
              return rows.length === 0 ? (
                <EmptyState text={NOTHING_AWAITING} />
              ) : (
                <Table columns={positionColumns} rows={rows} rowKey={(p) => `${p.account_id}|${p.market_id}|${p.outcome}|${p.returned}`} />
              );
            }}
          </Loadable>
        </Box>
        {account !== undefined && (
          <Ledger
            accounts={inScenario}
            account={account}
            cursor={sp.get('lcursor')}
            onAccount={(id) => set({ account: id, lcursor: null })}
            onCursor={(c) => set({ lcursor: c })}
          />
        )}
      </div>
    </>
  );
}

export function Paper() {
  const accounts = useApi((s) => api.paperAccounts(s), []);
  const positions = useApi((s) => api.paperPositions(s), []);
  return (
    <Loadable state={accounts} what="paper accounts">
      {(d) =>
        d.items.length === 0 ? (
          <Box title="PAPER">
            <EmptyState text={PAPER_EMPTY} />
          </Box>
        ) : (
          <PaperBody accounts={d.items} positions={positions} />
        )
      }
    </Loadable>
  );
}
```

- [ ] **Step 5: Write the Health page.** Create `web/src/pages/Health.tsx`:
```tsx
import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import { useStream } from '../api/StreamContext';
import type { HealthNow, HealthSeries, Integrity } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { LineChart } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Table } from '../components/Table';
import { Tag } from '../components/Tag';
import { formatDecimal } from '../lib/money';
import { fmtDuration, fmtLocal, fmtUtcDateTime } from '../lib/time';

export const NO_HEALTH_ROWS = 'No health rows in this window. The scanner may be asleep.';
export const NO_INTEGRITY = 'No integrity events in this window. Either the books agree, or nobody checked.';
const WINDOWS = [6, 24, 168];

function CaptureStatus({ h }: { h: HealthNow }) {
  return (
    <dl className="kv">
      <dt>capture</dt>
      <dd>{h.capture_paused ? <Tag tone="flag">PAUSED</Tag> : 'recording'}</dd>
      <dt>shards up</dt>
      <dd>{`${h.shards - h.parked_shards}/${h.shards}`}</dd>
      <dt>tokens</dt>
      <dd>{h.tokens}</dd>
      <dt>/ok</dt>
      <dd>{h.ok === null ? '—' : h.ok ? '✓ ok' : '✗ not ok'}</dd>
      <dt>clock offset</dt>
      <dd>{h.clock_offset_ms === null ? '—' : `${h.clock_offset_ms} ms`}</dd>
      <dt>last minute</dt>
      <dd title={fmtLocal(h.minute_ms)}>{`${fmtUtcDateTime(h.minute_ms)} (${fmtDuration(h.age_ms)} ago)`}</dd>
    </dl>
  );
}

function SeriesCharts({ s }: { s: HealthSeries }) {
  if (s.points.length === 0) {
    return (
      <Box title="PER MINUTE">
        <EmptyState text={NO_HEALTH_ROWS} />
      </Box>
    );
  }
  const lag = (k: 'lag_p50_ms' | 'lag_p99_ms') =>
    s.points.flatMap((p) => (p[k] === null ? [] : [{ x: p.minute_ms, y: p[k] as number }]));
  return (
    <div className="grid2">
      <Box title="MSGS/S">
        <LineChart label="messages per second" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'msgs/s', tone: 'p0', points: s.points.map((p) => ({ x: p.minute_ms, y: p.msgs / 60 })) }]} />
      </Box>
      <Box title="LAG (MS)">
        <LineChart label="feed lag p50 and p99" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'p50', tone: 'p0', points: lag('lag_p50_ms') }, { label: 'p99', tone: 'p3', points: lag('lag_p99_ms') }]} />
      </Box>
      <Box title="DROPS AND RECONNECTS">
        <LineChart
          label="drops and reconnects per minute"
          xMin={s.from_ms}
          xMax={s.to_ms}
          markers={s.points.filter((p) => p.reconnects > 0).map((p) => p.minute_ms)}
          series={[
            { label: 'drops', tone: 'p3', points: s.points.map((p) => ({ x: p.minute_ms, y: p.drops })) },
            { label: 'reconnects', tone: 'p1', points: s.points.map((p) => ({ x: p.minute_ms, y: p.reconnects })) },
          ]}
        />
        <p className="dim">Grey bars mark minutes with reconnects.</p>
      </Box>
    </div>
  );
}

function IntegrityView({ i }: { i: Integrity }) {
  if (i.total === 0) return <EmptyState text={NO_INTEGRITY} />;
  return (
    <>
      <Table
        columns={[
          { key: 'kind', label: 'KIND', render: (r: { kind: string; count: number }) => r.kind },
          { key: 'count', label: 'COUNT', render: (r: { kind: string; count: number }) => r.count },
        ]}
        rows={i.by_kind}
        rowKey={(r) => r.kind}
      />
      <p className="quip">{`${i.total} events in ${i.msgs} messages: ${i.per_million === null ? '—' : formatDecimal(i.per_million)} per million.`}</p>
    </>
  );
}

export function Health() {
  const [sp, setSp] = useSearchParams();
  const asked = Number(sp.get('hours'));
  const hours = WINDOWS.includes(asked) ? asked : 24;
  const { healthTick } = useStream();
  const now = useApi((s) => api.healthNow(s), [healthTick]);
  const series = useApi((s) => api.healthSeries(hours, s), [hours, healthTick]);
  const integrity = useApi((s) => api.integrity(hours, s), [hours, healthTick]);
  return (
    <>
      <Box title="WINDOW">
        <div className="seg" role="group" aria-label="window">
          {WINDOWS.map((w) => (
            <button key={w} type="button" className={w === hours ? 'on' : undefined} aria-pressed={w === hours} onClick={() => setSp(new URLSearchParams({ hours: String(w) }))}>
              {w === 168 ? '7 D' : `${w} H`}
            </button>
          ))}
        </div>
      </Box>
      <Box title="CAPTURE">
        <Loadable state={now} what="capture status">
          {(d) => (d.health === null ? <EmptyState text={NO_HEALTH_ROWS} /> : <CaptureStatus h={d.health} />)}
        </Loadable>
      </Box>
      <Loadable state={series} what="the health series">
        {(d) => <SeriesCharts s={d} />}
      </Loadable>
      <Box title="INTEGRITY">
        <Loadable state={integrity} what="integrity">
          {(d) => <IntegrityView i={d} />}
        </Loadable>
      </Box>
    </>
  );
}
```

- [ ] **Step 6: Route them.** In `web/src/App.tsx` add `import { Health } from './pages/Health';` and `import { Paper } from './pages/Paper';`, and inside `<Routes>` after the episode routes:
```tsx
          <Route path="/paper" element={<Paper />} />
          <Route path="/health" element={<Health />} />
```

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test`
Expected: PASS (3 Paper tests, 2 Health tests, every earlier file).

- [ ] **Step 8: Build, then eyeball it against the mockups** (dev server with a mocked-free backend: the api from Task 6 on 127.0.0.1:8081 if it can run; otherwise the pages show their error states, which is also worth a look).

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm run build && du -sh dist`
Expected: clean build; `dist` well under 1 MB. Optional manual check: `npm run dev`, open `http://localhost:5173/` at 1100 px and at 375 px wide; compare with `mockup-radar.html` (variant B) and `mockup-detail-paper.html`: plum background, teal pixel borders with hard shadows, salmon active nav, Press Start 2P headings, VT323 body, no horizontal page scroll (tables scroll inside their box).

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/src/pages/Paper.tsx web/src/pages/Paper.test.tsx web/src/pages/Health.tsx web/src/pages/Health.test.tsx web/src/test/fixtures.ts web/src/App.tsx && git commit -m "web: Paper (scenario switcher, equity step chart, cards, positions, ledger) and Health"
```

---
### Task 11: deploy: images, `compose.web.yml` (Traefik + oauth2-proxy), `.env.example`, CI, runbook

**Files:**
- Modify: `docker/rust.Dockerfile`, `.env.example`, `.github/workflows/ci.yml`
- Create: `docker/web.Dockerfile`, `docker/web/nginx.conf`, `compose.web.yml`, `docs/superpowers/runbooks/phase4-dashboard-vps.md`
- Test: `docker compose … config -q` (local, no build, no daemon needed); CI jobs; the runbook's post-deploy smoke test (owner, on the VPS)

**Interfaces:**
- Consumes: the `api` binary (Tasks 3–6: env `DATABASE_URL`, `API_BIND`; listens on 8081; exits non-zero if the role can write or a wallet/exchange variable is set); `web/` (Tasks 7–10: `npm ci && npm run build` → `web/dist`).
- Produces: images `cute-trading/rust:dev` (now with `/usr/local/bin/api`) and `cute-trading/web:dev`; compose profile `web` with services `api`, `web`, `oauth2-proxy`; Traefik routers `ct-web`, `ct-api`, `ct-oauth`, `ct-http`; middlewares `ct-auth`, `ct-sec`, `ct-csp`, `ct-rl`, `ct-redirect`; `.env` keys `RADAR_DOMAIN`, `OAUTH2_PROXY_CLIENT_ID`, `OAUTH2_PROXY_CLIENT_SECRET`, `OAUTH2_PROXY_COOKIE_SECRET`.

- [ ] **Step 1: Ship the api in the existing Rust image.** In `docker/rust.Dockerfile`:
  - change the second line to `# Build context: repository root.  docker compose build migrate   (spec §8.1.1). One image, two binaries: scanner (entrypoint) and api (compose.web.yml overrides the entrypoint).`
  - change `RUN cargo chef cook --release --recipe-path recipe.json --package scanner` to `RUN cargo chef cook --release --recipe-path recipe.json --package scanner --package api`
  - change `RUN cargo build --release --package scanner && mkdir -p /data && chown 65532:65532 /data` to `RUN cargo build --release --package scanner --package api && mkdir -p /data && chown 65532:65532 /data`
  - after `COPY --from=builder /src/target/release/scanner /usr/local/bin/scanner` add `COPY --from=builder /src/target/release/api /usr/local/bin/api`

  (Release builds inside Docker are the existing image pattern; the dev machine stays on debug builds. If the pinned cargo-chef rejects a repeated `--package`, drop both `--package` flags from the `cook` line only: it then cooks the whole workspace.)

- [ ] **Step 2: Write the web image.** `docker/web.Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose -f compose.yml -f compose.web.yml --profile web build web
FROM node:22-alpine AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# Unprivileged nginx (uid 101, port 8080): runs with a read-only root filesystem plus a /tmp tmpfs.
FROM nginxinc/nginx-unprivileged:1.29-alpine
COPY docker/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /web/dist /usr/share/nginx/html
USER 101
EXPOSE 8080
```
`docker/web/nginx.conf`:
```nginx
# Static SPA only. Security headers (HSTS, CSP, frame, nosniff, referrer) are set by Traefik
# middlewares in compose.web.yml, so every router gets the same ones. /api/* never reaches here.
server {
    listen 8080;
    server_name _;
    server_tokens off;
    root /usr/share/nginx/html;
    index index.html;

    location /assets/ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        try_files $uri =404;
    }

    location / {
        add_header Cache-Control "no-cache";
        try_files $uri $uri/ /index.html;
    }
}
```

- [ ] **Step 3: Write the compose override.** Create `compose.web.yml`:
```yaml
# Phase 4 dashboard (spec 2026-09-25-phase4-dashboard-design.md §2–§3). VPS only; needs Traefik's
# external network `traefik_public`. Kept out of compose.yml so local `docker compose up` never needs it.
#   docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web oauth2-proxy
# Every router except oauth2-proxy's own /oauth2/* pages goes through forward-auth (spec §3).
# No secrets here: RADAR_DOMAIN and OAUTH2_PROXY_* come from the VPS-only .env.

x-logging: &logging
  driver: json-file
  options:
    max-size: "10m"
    max-file: "5"

services:
  api:
    image: cute-trading/rust:dev
    profiles: ["web"]
    entrypoint: ["/usr/local/bin/api"]
    environment:
      DATABASE_URL: postgres://analyze_ro:${ANALYZE_DB_PASSWORD:?set ANALYZE_DB_PASSWORD in .env}@postgres:5432/cute_trading
      API_BIND: 0.0.0.0:8081
    depends_on:
      postgres:
        condition: service_healthy
    restart: unless-stopped
    stop_grace_period: 5s          # open SSE streams keep graceful shutdown waiting
    read_only: true
    cpus: "0.25"
    mem_limit: 128m
    logging: *logging
    networks: [internal, traefik_public]
    labels:
      - traefik.enable=true
      - traefik.docker.network=traefik_public
      - "traefik.http.routers.ct-api.rule=Host(`${RADAR_DOMAIN:?set RADAR_DOMAIN in .env}`) && PathPrefix(`/api/`)"
      - traefik.http.routers.ct-api.entrypoints=websecure
      - traefik.http.routers.ct-api.tls.certresolver=letsencrypt
      - traefik.http.routers.ct-api.middlewares=ct-auth@docker,ct-sec@docker,ct-csp@docker,ct-rl@docker
      - traefik.http.services.ct-api.loadbalancer.server.port=8081

  web:
    image: cute-trading/web:dev
    build:
      context: .
      dockerfile: docker/web.Dockerfile
    profiles: ["web"]
    restart: unless-stopped
    read_only: true
    tmpfs: [/tmp]
    cpus: "0.1"
    mem_limit: 32m
    logging: *logging
    networks: [traefik_public]
    labels:
      - traefik.enable=true
      - traefik.docker.network=traefik_public
      - "traefik.http.routers.ct-web.rule=Host(`${RADAR_DOMAIN}`)"
      - traefik.http.routers.ct-web.entrypoints=websecure
      - traefik.http.routers.ct-web.tls.certresolver=letsencrypt
      - traefik.http.routers.ct-web.middlewares=ct-auth@docker,ct-sec@docker,ct-csp@docker,ct-rl@docker
      - traefik.http.services.ct-web.loadbalancer.server.port=8080

  oauth2-proxy:
    image: quay.io/oauth2-proxy/oauth2-proxy:v7.12.0
    profiles: ["web"]
    restart: unless-stopped
    read_only: true
    cpus: "0.1"
    mem_limit: 64m
    logging: *logging
    environment:
      OAUTH2_PROXY_PROVIDER: github
      OAUTH2_PROXY_CLIENT_ID: ${OAUTH2_PROXY_CLIENT_ID:?set OAUTH2_PROXY_CLIENT_ID in .env}
      OAUTH2_PROXY_CLIENT_SECRET: ${OAUTH2_PROXY_CLIENT_SECRET:?set OAUTH2_PROXY_CLIENT_SECRET in .env}
      OAUTH2_PROXY_COOKIE_SECRET: ${OAUTH2_PROXY_COOKIE_SECRET:?set OAUTH2_PROXY_COOKIE_SECRET in .env}
      OAUTH2_PROXY_GITHUB_USERS: isabellybaldo
      OAUTH2_PROXY_EMAIL_DOMAINS: "*"
      OAUTH2_PROXY_HTTP_ADDRESS: 0.0.0.0:4180
      OAUTH2_PROXY_REVERSE_PROXY: "true"
      OAUTH2_PROXY_UPSTREAMS: static://202
      OAUTH2_PROXY_REDIRECT_URL: https://${RADAR_DOMAIN}/oauth2/callback
      OAUTH2_PROXY_WHITELIST_DOMAINS: ${RADAR_DOMAIN}
      OAUTH2_PROXY_COOKIE_NAME: _ct_radar
      OAUTH2_PROXY_COOKIE_DOMAINS: ${RADAR_DOMAIN}
      OAUTH2_PROXY_COOKIE_SECURE: "true"
      OAUTH2_PROXY_COOKIE_HTTPONLY: "true"
      OAUTH2_PROXY_COOKIE_SAMESITE: lax
      OAUTH2_PROXY_COOKIE_EXPIRE: 168h
      OAUTH2_PROXY_SKIP_PROVIDER_BUTTON: "true"
      OAUTH2_PROXY_SILENCE_PING_LOGGING: "true"
    networks:
      traefik_public:
        aliases: [ct-oauth2-proxy]
    labels:
      - traefik.enable=true
      - traefik.docker.network=traefik_public
      # oauth2-proxy's own pages: the only route that answers without a session
      - "traefik.http.routers.ct-oauth.rule=Host(`${RADAR_DOMAIN}`) && PathPrefix(`/oauth2/`)"
      - traefik.http.routers.ct-oauth.entrypoints=websecure
      - traefik.http.routers.ct-oauth.tls.certresolver=letsencrypt
      - traefik.http.routers.ct-oauth.middlewares=ct-sec@docker,ct-rl@docker
      - traefik.http.routers.ct-oauth.service=ct-oauth
      - traefik.http.services.ct-oauth.loadbalancer.server.port=4180
      # http → https
      - "traefik.http.routers.ct-http.rule=Host(`${RADAR_DOMAIN}`)"
      - traefik.http.routers.ct-http.entrypoints=web
      - traefik.http.routers.ct-http.middlewares=ct-redirect@docker
      - traefik.http.routers.ct-http.service=ct-oauth
      - traefik.http.middlewares.ct-redirect.redirectscheme.scheme=https
      - traefik.http.middlewares.ct-redirect.redirectscheme.permanent=true
      # forward-auth: 202 with a valid session, a redirect to GitHub without one (static://202 mode)
      - traefik.http.middlewares.ct-auth.forwardauth.address=http://ct-oauth2-proxy:4180/
      - traefik.http.middlewares.ct-auth.forwardauth.trustForwardHeader=true
      # hardening (spec §3)
      - traefik.http.middlewares.ct-sec.headers.stsSeconds=31536000
      - traefik.http.middlewares.ct-sec.headers.frameDeny=true
      - traefik.http.middlewares.ct-sec.headers.contentTypeNosniff=true
      - traefik.http.middlewares.ct-sec.headers.referrerPolicy=same-origin
      - "traefik.http.middlewares.ct-csp.headers.contentSecurityPolicy=default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
      - traefik.http.middlewares.ct-rl.ratelimit.average=30
      - traefik.http.middlewares.ct-rl.ratelimit.burst=60

networks:
  traefik_public:
    external: true
```

- [ ] **Step 4: Add the new `.env` keys (placeholders only).** Replace `.env.example` with:
```
# Copy to .env (git-ignored) and replace every value. Database role passwords and the dashboard's
# login settings only: exchange/wallet credentials must never exist anywhere in this project (spec §16).
POSTGRES_PASSWORD=change-me-owner
SCANNER_DB_PASSWORD=change-me-scanner
ANALYZE_DB_PASSWORD=change-me-analyze

# Phase 4 dashboard (compose.web.yml, VPS only). See docs/superpowers/runbooks/phase4-dashboard-vps.md.
RADAR_DOMAIN=radar.isoca.space
# From the GitHub OAuth App (callback https://radar.isoca.space/oauth2/callback). VPS .env only.
OAUTH2_PROXY_CLIENT_ID=change-me-github-oauth-client-id
OAUTH2_PROXY_CLIENT_SECRET=change-me-github-oauth-client-secret
# 32 random bytes, URL-safe base64:  openssl rand -base64 32 | tr -- '+/' '-_'
OAUTH2_PROXY_COOKIE_SECRET=change-me-32-byte-urlsafe-base64
```

- [ ] **Step 5: Check the compose files render** (no build; `config` needs no daemon).

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo web-config-ok && docker compose --env-file .env.example config -q && echo base-config-ok && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web config | grep -cE "traefik.http.(routers|middlewares).ct-"`
Expected: `web-config-ok`, `base-config-ok` (local dev is untouched), and a count of `28` router/middleware label lines. Then check that nothing publishes Postgres or the api: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config | grep -n "published" ; echo "exit=$?"` → no output, `exit=1`.

- [ ] **Step 6: CI: build and test the web, check compose.** In `.github/workflows/ci.yml`, append under `jobs:` (after the `rust` job; the `rust` job already covers `bins/api` through `--workspace`, and its `TEST_DATABASE_URL`/`STORE_TESTS_REQUIRED` env makes the api's Postgres tests mandatory):
```yaml
  web:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: web/package-lock.json
      - run: npm ci
      - run: npm test
      - run: npm run build
  compose:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q
```

- [ ] **Step 7: Write the runbook.** Create `docs/superpowers/runbooks/phase4-dashboard-vps.md`:
````markdown
# Phase 4 dashboard on the VPS (radar.isoca.space)

A private dashboard over the scanner's Postgres, behind a GitHub login (only `isabellybaldo`).
Read-only (api as `analyze_ro`), paper only, no GitHub secrets. Three containers join the existing
compose project: `api`, `web`, `oauth2-proxy` (profile `web`, file `compose.web.yml`).
Disk budget: 30 GB (`sh scripts/disk-budget.sh`). The dashboard adds about 0.1 GB of images.

## 0. Once: DNS

Hostinger hPanel → Domains → isoca.space → DNS records → add **A**, name `radar`, points to
`31.97.23.123`, TTL 300. Before deploying, check from the PC (PowerShell):
```powershell
Resolve-DnsName radar.isoca.space -Server 8.8.8.8
```
Expect `31.97.23.123`. ACME (HTTP-01) needs this before the first start.

## 1. Once: the GitHub OAuth App

github.com → Settings → Developer settings → OAuth Apps → **New OAuth App**:
- Application name: `cute-trading radar`
- Homepage URL: `https://radar.isoca.space`
- Authorization callback URL: `https://radar.isoca.space/oauth2/callback`

Register, copy the **Client ID**, then **Generate a new client secret** and copy it (GitHub shows it once).
Both go only into the VPS `.env` (step 2): never into the repo, never into GitHub secrets.

## 2. Once: VPS `.env` keys and Traefik entrypoints (on the VPS)

```bash
cd /home/isoca/projects/cute-trading
cp .env .env.bak
openssl rand -base64 32 | tr -- '+/' '-_'        # the cookie secret
nano .env
```
Append (values from step 1 and the line above):
```
RADAR_DOMAIN=radar.isoca.space
OAUTH2_PROXY_CLIENT_ID=...
OAUTH2_PROXY_CLIENT_SECRET=...
OAUTH2_PROXY_COOKIE_SECRET=...
```
```bash
chmod 600 .env
docker network ls | grep traefik_public
docker inspect "$(docker ps --format '{{.Names}}' | grep -i traefik | head -1)" --format '{{json .Args}}' | tr ',' '\n' | grep -i entrypoints
```
`compose.web.yml` assumes the entrypoints are named `web` (:80) and `websecure` (:443). If Traefik
uses other names, edit the `entrypoints=` labels before step 4. If Traefik already redirects
http→https globally, the `ct-http` router is harmless.

## 3. Build, pin and ship (on the PC)

Pin the three base images once (then commit the pins: "deploy: pin web image digests"):
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
for i in node:22-alpine nginxinc/nginx-unprivileged:1.29-alpine quay.io/oauth2-proxy/oauth2-proxy:v7.12.0; do
  docker pull -q "$i" && docker inspect --format '{{index .RepoDigests 0}}' "$i"
done
```
Put each `name@sha256:…` in place of its tag: the two `FROM` lines of `docker/web.Dockerfile` and the
`oauth2-proxy` `image:` in `compose.web.yml` (keep the tag too: `name:tag@sha256:…`).

Build and bundle (Git Bash):
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading
sh scripts/disk-budget.sh
docker compose build --build-arg GIT_SHA="$(git rev-parse --short HEAD)" migrate    # cute-trading/rust:dev (scanner + api)
docker compose -f compose.yml -f compose.web.yml --profile web build web             # cute-trading/web:dev
mkdir -p "$HOME/ct-web" && cd "$HOME/ct-web"
docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz
cp /c/Users/Isabelly/PycharmProjects/cute-trading/compose.yml /c/Users/Isabelly/PycharmProjects/cute-trading/compose.web.yml .
tar -cf "$HOME/ct-web.tar" image.tar.gz compose.yml compose.web.yml && rm -rf "$HOME/ct-web"
docker builder prune -f --keep-storage 5GB
sh /c/Users/Isabelly/PycharmProjects/cute-trading/scripts/disk-budget.sh
```
Ship (PowerShell, the key has a passphrase):
```powershell
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" $HOME\ct-web.tar root@31.97.23.123:/root/
```

## 4. Start the dashboard (on the VPS)

This starts only the three dashboard containers; the running scanner keeps its current container
(a new `cute-trading/rust:dev` is only picked up by the scanner at its next recreate).
```bash
cd /home/isoca/projects/cute-trading
tar -xf /root/ct-web.tar && rm /root/ct-web.tar
gunzip -c image.tar.gz | docker load && rm image.tar.gz
docker compose -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
docker compose -f compose.yml -f compose.web.yml --profile web pull oauth2-proxy
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web oauth2-proxy
docker compose -f compose.yml -f compose.web.yml ps
docker compose -f compose.yml -f compose.web.yml logs --tail 20 api web oauth2-proxy
```
Expect `api: listening on 0.0.0.0:8081 (read-only, paper only)`. If the cert doesn't issue (Traefik serves
"TRAEFIK DEFAULT CERT"), wait for DNS, then `docker compose -f compose.yml -f compose.web.yml restart web`.

## 5. Post-deploy smoke test

From the PC (Git Bash), without a session:
```bash
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/health/now
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://radar.isoca.space/api/stream
curl -sSI http://radar.isoca.space/ | head -1
curl -sSI https://radar.isoca.space/oauth2/sign_in | grep -iE 'strict-transport|x-frame-options|x-content-type-options|referrer-policy'
```
Pass: the first three are `302` to `https://github.com/login/oauth/authorize?…` (or to `/oauth2/…`, which
then goes to GitHub); never `200`. The fourth is `301`/`308`. The last shows all four headers.

With a session: open `https://radar.isoca.space/` in the browser and sign in with GitHub (authorize the
app once). Then:
- `https://radar.isoca.space/api/health/now` shows JSON; `https://radar.isoca.space/api/episodes/open`
  shows money as strings (`"net":"0.41"`).
- DevTools → Console: no CSP violations; Network: `/api/stream` is `200 text/event-stream` and the header
  dot says LIVE.
- DevTools → Application → Cookies: `_ct_radar` is `Secure`, `HttpOnly`, `SameSite=Lax`.
- A private window signed in to a different GitHub account gets a 403 from oauth2-proxy.
On the VPS:
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}'
docker compose -f compose.yml -f compose.web.yml port postgres 5432; echo "exit=$?"
docker compose -f compose.yml -f compose.web.yml exec postgres psql -U ct_owner -d cute_trading -tAc "select count(*) from pg_stat_activity where usename = 'analyze_ro'"
```
Pass: api under 128 MiB, web under 32 MiB, oauth2-proxy under 64 MiB, the scanner's CPU as before; no
published Postgres port; `analyze_ro` holds at most 4 connections (one of them the shared LISTEN).

## 6. Phase 4 exit (after a week of capture)

Behind the login: Radar live; Episodes filtered day by day back 7 days, paging to the end of each day;
one detail per kind; Paper (empty state until S3c is live, then curves per scenario); Health with 7 D.
Record it below.

## Rollback

```bash
docker compose -f compose.yml -f compose.web.yml --profile web stop api web oauth2-proxy
```
The scanner and Postgres are untouched. To also close the door: revoke the OAuth App's client secret on GitHub.

## Results

(date, image sha, smoke-test outputs, exit check)
````

- [ ] **Step 8: Run the full local suite once more** (nothing in this task changes Rust or web code, but the Dockerfile and ignore files moved).

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test --workspace && cd web && npm test && npm run build`
Expected: everything passes; no SKIPPED store/api tests.

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add docker/rust.Dockerfile docker/web.Dockerfile docker/web/nginx.conf compose.web.yml .env.example .github/workflows/ci.yml docs/superpowers/runbooks/phase4-dashboard-vps.md && git commit -m "deploy: dashboard images, compose.web.yml (Traefik forward-auth to oauth2-proxy, CSP, rate limit, limits), CI web job, VPS runbook"
```

- [ ] **Step 10 (owner, not the implementer): deploy.** Follow `docs/superpowers/runbooks/phase4-dashboard-vps.md` §0–§5 (DNS, OAuth App, `.env`, pin + build + ship, start, smoke test), commit the digest pins, and fill in its Results. Then run the disk-budget clean-up: `cargo clean` if `target/` is over 8 GB and `docker builder prune -f --keep-storage 5GB`.

---

## Rulings recorded in this plan (owner: check)

1. **`compose.web.yml`, not `compose.yml`:** the `web` profile lives in an override file in the same compose project, so local `docker compose up` never needs Traefik's external network. Start it with `-f compose.yml -f compose.web.yml --profile web … --no-deps api web oauth2-proxy` so a new Rust image never restarts a running scanner.
2. **`health` SSE events are polled,** not triggered: one indexed `max(minute_ms)` query every 15 s on the api side. No new trigger or migration (S3c owns 0004; the api adds no schema).
3. **Open episodes** = unclosed `source='live'` episodes of the newest live `scanner` run only. Dashboard reads are live-only everywhere (replay runs never show); the detail page shows any id and tags replays.
4. **"Today"** counts live episodes whose realization isn't `not_arbitrage` (so `yes_basket_incomplete` is excluded): gross = peak gross Π > 0, net = peak net Π > 0, UTC day.
5. **Paper:** accounts = the newest live run that has paper accounts, with each account's latest mark; equity and ledger span all live runs by `account_id` (restarts restore accounts); positions come from the live `paper_state` JSON (the serialized `sim::Account`; a store test pins that format through a `sim` dev-dependency). There is no MARK column in the positions table (the api has no books).
6. **Legs** show `#idx` + a short market id (the schema has no leg question text). The re-check table's first row is **peak**, not "open" (open-time values aren't stored).
7. **The api's own guards:** sessions are `default_transaction_read_only` with a 5 s statement timeout (the role's 60 s still applies to other analyze users); it refuses to start if its role can INSERT; SSE is capped at 16 clients (503 beyond); errors other than the spec's 503 text use my deadpan copy ("Nothing here. It may have been a dream.", "Something broke on our side. It has been written down.").
8. **Chart geometry** converts decimal strings with `Number()` for pixel positions only; every money value shown as text (including axis labels) is string-formatted. The money test spies on `toFixed`, `toLocaleString`, `Intl.NumberFormat`, `parseFloat` and `Math.round` around `formatMoney`.
9. **Login:** oauth2-proxy v7.12.0 in forward-auth `static://202` mode with `skip_provider_button` (an unauthenticated request, including `/api/*`, gets a redirect to GitHub, as the spec's smoke test expects). Traefik entrypoint names (`web`/`websecure`) and the three base-image digests are verified/pinned on the day of the deploy (runbook §2–§3). The Rust image carries both binaries.
10. **Versions:** Node 22 in CI and the image (dev machine has 23.8.0); React 19.2, React Router 7.14, Vite 8, Vitest ^4.1 (bump to the first version whose peer range includes Vite 8 if npm complains). Task 2 requires S3c's migration 0004 to be merged first.

## Self-review

**1. Spec coverage** (`2026-09-25-phase4-dashboard-design.md`):
- §1 purpose/exit: runbook §6 (a week browsable behind the login).
- §2 architecture: `bins/api` (Tasks 3–6) via a `store` read module (Tasks 1–2); `web/` (Tasks 7–10); three containers under profile `web`, Traefik, `radar.isoca.space`, bundle flow without GitHub secrets (Task 11).
- §3 security: oauth2-proxy GitHub login limited to `isabellybaldo`, `Secure`/`HttpOnly`/`SameSite=Lax` cookie, secrets only in the VPS `.env` (Task 11); forward-auth on every router except `/oauth2/*` (Task 11); `analyze_ro` + read-only sessions + refusal to start as a writer + no write endpoints + parameterized, bounded queries (Tasks 1, 3–5); networks (`api` on internal + traefik_public, `web`/`oauth2-proxy` on traefik_public only, Postgres unpublished); HSTS, CSP, frame deny, nosniff, referrer, per-IP rate limit; non-root read-only containers; CPU/memory limits 0.25/128, 0.1/32, 0.1/64; one shared LISTEN (Task 6); paper-only env guard (Task 3).
- §4 API: all 12 endpoints (Tasks 3–6); decimal strings and 0 ≠ null (Task 3 `Dec`, tests in Tasks 4–5); ms timestamps; empty paper lists without the tables (Tasks 2, 5); JSON errors with 400/503/500 (Task 3).
- §5 web: night-shift tokens, fonts, hand SVG with crispEdges, fake toggle + test (Task 7); the `api/`, `components/`, `pages/`, `theme.css` structure and React Router (Tasks 7–10); Radar hearts/open/today/empty copy (Task 8); Episodes filters in the URL, keyset paging, detail sections (Task 9); Paper scenario switcher, step chart, legend, cards, positions, ledger, copy and empty state; Health charts with markers, integrity per million, capture status (Task 10); header with logo, nav, live dot, toggle (Task 7); responsive CSS and text tags (Task 7).
- §6 data correctness: money as sent + the no-float test (Task 7); UTC with local on hover, `4.19 s` durations (Tasks 7, 9); Radar refetches on `episode` events, no client merging (Task 8), coalesced to ≤ 1/s; loading/empty/error states on every page (`Loadable`).
- §7 testing: api tests against a throwaway Postgres per endpoint, strings, 0 ≠ null, limits, 400s, empty paper, role can't write, SSE NOTIFY (Tasks 3–6); Vitest component and page tests with a mock api (Tasks 7–10); `docker compose config` and the smoke test (Task 11); CI covers `bins/api` and adds the web job (Task 11).
- §8 out of scope: nothing from S4/S5/S7b, no public demo, no Tailscale, no GHCR.

**2. Placeholder scan:** no TBD/TODO/"similar to"; every code step has the code; the only values left for the day of the deploy are the OAuth credentials, the cookie secret and the image digests, which must not be in the repo or can't be known in advance, and the runbook gives the exact commands that produce them.

**3. Type consistency:** store names (`EpisodeSummary`, `EpisodeFilter`, `EpisodePage`, `HealthNow`, `Today`, `IntegritySummary`, `PaperAccountNow`, `EquityPoint`, `PaperPosition`, `PaperLedgerEntry`, `LedgerPage`) match between Tasks 1–2 and their consumers in Tasks 3–6. JSON field names in the Rust DTOs (Tasks 3–5) match `web/src/api/types.ts` (Task 7) field for field (`opened_ms`, `q_star`, `net_budget100`, `marked_ms`, `per_million`, `drops_5m`, …). `ApiError::Unavailable` takes two `&'static str` everywhere (Tasks 3, 6). Cursor formats: episodes `"{opened_ms}_{id}"`, ledger `"{run_id}_{seq}"`, the same on both sides.

**4. Review Focus:** each of the five lines has a named test in its owning task (Tasks 1, 4, 6, 7).
