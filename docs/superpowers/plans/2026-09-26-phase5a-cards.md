# isoca-radar Phase 5a: live paper cards (Gebele), Edge capture, readability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A live Papers page (Gebele cards G1–G4 with PAPER / THEIR METHOD / OUR METHOD columns, a status and a hash), an Edge capture box on Paper with a Radar teaser, premise versions that never let different premises mix, and a readability and responsive pass over every page, all on the existing read-only dashboard.

**Architecture:** The scanner records each run's premises (`run_premises`) and times its own CLOB requests into the health row. A new pure crate `crates/cards` holds the statistics, the hourly merge-exact cells, the incremental windows and the card bodies. A new binary `bins/analyze` (0.25 CPU, 128 MB, role `analyze_rw`, no network client) reads changed episodes every 15 minutes through `store`, rebuilds the dirty hourly cells, updates its windows by adding new and subtracting old cells, and writes `card_results`. The api serves `card_results` / `card_meta` as stored; the web renders them.

**Tech Stack:** Rust 1.98.1 (edition 2024), tokio, sqlx 0.8 (inside `store` only), rust_decimal 1.43 (`serde-with-str`), sha2, serde_json, proptest; axum 0.8 (api); React 19, React Router 7, Vite 8, TypeScript 5.9, Vitest 4 + Testing Library; Postgres 17.

**Spec:** `docs/superpowers/specs/2026-09-26-phase5a-cards-design.md` (parent: `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §4, §5.5, §11, §12, §14, §19; Phase 4: `docs/superpowers/specs/2026-09-25-phase4-dashboard-design.md`). The mockups `mockup-papers.html` and `mockup-fonts-fantasy.html` are with the owner, not in the repo: Task 12 builds the Papers page from the spec §8.2 description of the approved mockup, and the body font is **C, IBM Plex Mono**.

This is ONE plan with 14 sequential tasks: premises (Task 1), store (Tasks 2–3), scanner (Task 4), the pure `cards` crate (Tasks 5–8), `analyze` (Task 9), api (Task 10), web (Tasks 11–13), deploy (Task 14). Tasks 1–10 never touch `web/`; Tasks 11–13 never touch Rust.

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere. `analyze` refuses to start if an exchange/wallet credential variable is set (the same denylist as `bins/scanner/src/guard.rs`, kept in step by `bins/scanner/tests/arch.rs`).
- **Money and prices are decimal strings** in JSON (cells, bodies, api), never JSON numbers. `null` (not measured) and `"0"` (measured zero) stay distinct. Trailing zeros are dropped; `-0` is `"0"`.
- **The api stays read-only:** it connects as `analyze_ro` on read-only sessions, refuses to start if its role can INSERT, and gains only `GET /api/cards?win=` and `GET /api/cards/meta`, each one small query on `card_results` / `card_meta`.
- **`analyze` has no network client:** the arch test bans `reqwest`, `pm-api`, `tokio-tungstenite` and `sqlx` from `analyze`; it reaches Postgres only through `store` (parent §9.3 rule unchanged). `crates/cards` stays pure (no tokio, sqlx, reqwest, tokio-tungstenite, pm-api, store).
- **Only `store` uses sqlx** (`deny.toml`: `{ name = "sqlx", wrappers = ["store"] }`).
- **`analyze_rw` (spec §2):** SELECT on `episodes`, `runs`, `run_premises`, `health`, `paper_marks`, `paper_accounts`; INSERT, UPDATE, DELETE on `card_cells`, `card_results`, `card_meta` only; statement timeout 10 s. `analyze` refuses to start if its role can write any other table.
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (tests may unwrap: `clippy.toml`). Statistics use `i64` counts, integer ms, `rust_decimal::Decimal`, and an `i128` fixed point (1.0 = 10²⁸) for Kaplan–Meier. `crates/cards` gets the same no-`f32`/`f64` source scan as `arb-core`.
- **Time** is integer ms since epoch; hours are UTC: `hour_of(ms) = ms.div_euclid(3_600_000) * 3_600_000`.
- **Different premises never mix (spec §4):** a card counts only runs whose `run_premises` match its premise key; runs without `run_premises` are never counted. `premises.lock` hashes each area's files with `\r` stripped.
- **analyze budget:** `cpus: 0.25`, `mem_limit: 128m`; one tick over a synthetic 30-day history (~250k episodes, ~350/hour) takes **< 200 ms** in a debug build on the PC, with catch-up measured separately. `ANALYZE_EVERY_SECS` defaults to 900, range 60..=3600. Catch-up builds 24 hours per batch and sleeps 2 s between batches.
- **Thin thresholds (spec §4):** fewer than 30 uncensored durations for a median; fewer than 10 episodes on a side for a ratio.
- **Exact copy strings (spec):** `The difference is physics and other people.` · `Edge on the table today: +$X · captured at 145 ms: Y%.` · `coming later: needs the on-chain dataset and somewhere to put it that is not your laptop` · `upper bound: assumes first-in-line execution; CLOB queue ignored` · `measured: ⟨host⟩` · the paper's `does not support a reliable distributional comparison`.
- **Web look:** IBM Plex Mono for body text and tables (15–16 px, line-height ≥ 1.5); Press Start 2P for headings and tags; VT323 only for big numbers. Fonts load from Google Fonts only, so the CSP in `compose.web.yml` does not change. No horizontal page scroll at 360 px on any page; wide tables scroll inside their own box; charts scale to their container. Every section of every page has a (?) button: click, Enter or Space toggles it, Escape closes it, it carries `aria-expanded` and `aria-controls`, and its copy lives in `web/src/help.ts`.
- **No float formatting of money** in the web (the existing `money.test.ts` spy pattern). Numbers parsed from decimal strings are used for chart geometry only.
- **No fresh `npm install` in `web/`** (npm 10.9.2 crashes): reuse the existing `web/node_modules`. **No new web dependency** (that would need owner approval). No new crates.io crate in Rust: new dependencies come from `[workspace.dependencies]` (`sha2` joins `arb-core`; `crates/cards` and `bins/analyze` use workspace crates only).
- **Debug builds only** on the dev machine (no `--release`, no `cargo install`). Docker builds happen only in Task 14's runbook, run by the owner.
- **Disk budgets:** 30 GB on the PC (`sh scripts/disk-budget.sh`), 50 GB for the VPS project. Run `cargo clean` if `target/` passes 8 GB.
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A` or `.`); never stage `PROJECT.md` or `README.md` (untracked, the owner's). Commit messages have **no** `Co-Authored-By` or any other attribution trailer.
- **Shell prefix for every command:** `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store, api, analyze) use the Postgres already listening on **127.0.0.1:15432** (not 55432). Prefix their commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" &&`. A test that prints `SKIPPED: TEST_DATABASE_URL not set` does **not** count as passing. Before the first such run in a task, check the port: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` must print `pg-up`; if it doesn't, stop and ask the owner.
- **Secrets and the machine:** never print `.env` (no `cat`, no `grep` that shows values, no `echo` of a password); never run `docker compose config` except as `docker compose --env-file .env.example … config -q`; never restart Docker or WSL.

## Review Focus

1. **A late close of an old episode:** an episode opened 30 hours ago (outside analyze's in-memory 48-hour cache, inside 7d/30d/all) closes now. Its opening hour and every hour it spanned must be rebuilt and swapped in by subtraction, so the windows count it once, as a finished duration, and the hours it no longer spans stop observing it. Pinned by `a_late_close_outside_the_recent_48h_rebuilds_the_old_hour_without_double_counting` (Task 9).
2. **A premise bump mid-window:** a run started under a new `episode_def` version must produce a new premise key whose windows contain only new-key runs (status `warming`, never a blend), while the old key's cells stay untouched in `card_cells`. Pinned by `a_premise_bump_mid_window_never_mixes_keys` (Task 9).
3. **An episode spanning several hours:** an episode opened hours before an hour starts must still be observed in that hour (G3) and still hold capital in that hour's overlap (Edge capture), and the window maximum must equal a sweep over the whole window. Pinned by `an_episode_spanning_three_hours_is_observed_in_each_hour` (Task 6) and `capital_overlap_counts_an_episode_opened_hours_earlier` (Task 8).
4. **Thin and empty samples:** a NO side with 0 or 4 episodes, a window with nothing on the table, a side with no uncensored durations: the number becomes `null` with a stated reason and a `thin` status, never a division panic or a fake `"0"`. Pinned by `thin_and_empty_samples_never_divide_by_zero` (Task 7) and `capture_rate_is_null_when_nothing_was_on_the_table` (Task 8).
5. **The tick stays fast as "all" grows:** a tick must not re-read or re-merge every cell of the key; the same tick over 30 and over 90 days of history must both stay under 200 ms in a debug build. Pinned by `tick_stays_under_200ms_as_all_grows_from_30_to_90_days` (Task 9).

## File Structure

```
.gitattributes                                   MODIFY: premises.lock is LF everywhere
Cargo.toml                                       MODIFY: members += crates/cards, bins/analyze
crates/arb-core/Cargo.toml                       MODIFY: + sha2
crates/arb-core/src/premises.rs                  NEW: PREMISES, effective versions, host band, premise keys, lock checks
crates/arb-core/src/lib.rs                       MODIFY: pub mod premises
crates/arb-core/premises.lock                    NEW (written by the refresh command)
crates/arb-core/tests/premises.rs                NEW: pure premise tests
crates/arb-core/tests/premises_lock.rs           NEW: lock enforcement + refresh command
crates/store/migrations/0006_cards.sql           NEW: updated_ms, run_premises, health RTT, card tables, analyze_rw, backfill
crates/store/src/batch.rs                        MODIFY: episode upsert sets updated_ms; health RTT columns
crates/store/src/rows.rs                         MODIFY: HealthRow RTT fields, RunPremiseRow
crates/store/src/runs.rs                         MODIFY: insert_run_premises, run_premises_for
crates/store/src/read.rs                         MODIFY: HEALTH_COLS += RTT columns
crates/store/src/error.rs                        MODIFY: StoreError::TooManyRows
crates/store/src/card_io.rs                      NEW: analyze reads/writes, the api's card reads
crates/store/src/lib.rs                          MODIFY: mod card_io, re-exports, two admin helpers
crates/store/Cargo.toml                          MODIFY: dev-dep arb-core
crates/store/tests/{roles,migrate,batch,read}.rs MODIFY;  crates/store/tests/card_io.rs NEW
docker/postgres/init/01-roles.sh                 MODIFY: analyze_rw on fresh volumes
crates/pm-api/src/{http,clob,lib}.rs             MODIFY: RttSink, timed CLOB sends
crates/pm-api/Cargo.toml                         MODIFY: dev tokio net + io-util
bins/scanner/src/premises.rs                     NEW: SCANNER_HOST, run rows, HostBand
bins/scanner/src/{lib,metrics,run}.rs            MODIFY
bins/scanner/tests/premises.rs                   NEW: backfill literals = current premises
bins/scanner/tests/arch.rs                       MODIFY: cards pure, analyze no network, guard parity
bins/api/tests/common/mod.rs                     MODIFY: HealthRow literal
crates/cards/                                    NEW: stats, cells, window, family, body, gebele, edge, papers, tests, bench
bins/analyze/                                    NEW: config, guard, scenarios, tick, main, tests
bins/api/src/{cards,lib,params}.rs               NEW/MODIFY;  bins/api/tests/cards.rs NEW
web/index.html, web/src/theme.css                MODIFY: IBM Plex Mono, responsive
web/src/help.ts, web/src/components/Help.tsx     NEW
web/src/components/{Box,CardView,CardCharts,DecayChart,EdgeCapture}.tsx   MODIFY/NEW
web/src/lib/cards.ts, web/src/pages/Papers.tsx   NEW
web/src/pages/{Radar,Episodes,EpisodeDetail,Paper,Health}.tsx, web/src/App.tsx   MODIFY
web/src/api/{types,client}.ts, web/src/test/fixtures.ts                        MODIFY
docker/rust.Dockerfile, compose.yml, .env.example                             MODIFY
docs/superpowers/runbooks/phase5a-cards-vps.md                                 NEW
```

---

### Task 1: arb-core: premise versions, `premises.lock` and its enforcement test

**Files:**
- Create: `crates/arb-core/src/premises.rs`, `crates/arb-core/tests/premises.rs`, `crates/arb-core/tests/premises_lock.rs`, `crates/arb-core/premises.lock` (written by the refresh command in Step 6)
- Modify: `crates/arb-core/src/lib.rs` (add `pub mod premises;`), `crates/arb-core/Cargo.toml` (`[dependencies]` += `sha2.workspace = true`), `.gitattributes`

**Interfaces:**
- Consumes: nothing new (`sha2` from `[workspace.dependencies]`).
- Produces (all in `arb_core::premises`):
  - `pub const PREMISES: &[(&str, &str, &[&str])]` — (area, code version, source files relative to the workspace root), sorted by area: `censoring`, `depth`, `detector_no`, `detector_yes`, `episode_def`, `fees`, `host`, `sim`.
  - `pub fn code_version(area: &str) -> Option<&'static str>`
  - `pub fn sha8(text: &str) -> String`
  - `pub fn effective_version(code: &str, config_section: &str) -> String` → `"{code}+{sha8(section)}"`
  - `pub fn episode_def_section(stale_lag_ms: i64, suspect_window_ms: i64, l1_suspect_window_ms: i64, drift_settle_ms: i64) -> String`, `pub fn censoring_section(gap_ms: i64) -> String`, `pub fn sim_section(policies_sha256: &str) -> String`
  - `pub fn host_band(measured_ms: Option<i64>) -> &'static str` (`unmeasured`, `lt25`, `25-100`, `100-250`, `gt250`), `pub fn valid_host_label(label: &str) -> bool`, `pub fn host_version(label: &str, measured_ms: Option<i64>) -> String` → `"{code}:{label}|{band}"`
  - `pub fn premise_key(areas: &[&str], versions: &BTreeMap<String, String>) -> Option<String>` → sorted `area=version` joined by `;`
  - `pub fn files_hash(files: &[(&str, &str)]) -> String`, `pub fn parse_lock(text: &str) -> Vec<(String, String, String)>`, `pub fn render_lock(entries: &[(String, String, String)]) -> String`, `pub fn lock_problems(current: &[(String, String, String)], lock: &[(String, String, String)]) -> Vec<String>`, `pub fn refreshed_lock(current: &[(String, String, String)], lock: &[(String, String, String)]) -> Result<String, Vec<String>>`
  - The refresh command: `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock`.

- [ ] **Step 1: Write the failing pure tests.** Create `crates/arb-core/tests/premises.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::*;
use std::collections::BTreeMap;

fn t(a: &str, v: &str, h: &str) -> (String, String, String) {
    (a.to_string(), v.to_string(), h.to_string())
}

#[test]
fn sha8_and_effective_versions_use_the_first_8_hex_of_sha256() {
    // sha256("") = e3b0c442…, sha256("abc") = ba7816bf… (FIPS 180-2 test vectors)
    assert_eq!(sha8(""), "e3b0c442");
    assert_eq!(sha8("abc"), "ba7816bf");
    assert_eq!(effective_version("1", ""), "1+e3b0c442");
    assert_eq!(censoring_section(30_000), "gap_ms=30000");
    assert_eq!(
        episode_def_section(500, 60_000, 3_600_000, 2_000),
        "stale_lag_ms=500;suspect_window_ms=60000;l1_suspect_window_ms=3600000;drift_settle_ms=2000"
    );
    assert_eq!(sim_section("ab"), "policies_sha256=ab");
}

#[test]
fn host_bands_follow_the_spec_edges() {
    assert_eq!(host_band(None), "unmeasured");
    assert_eq!(host_band(Some(0)), "lt25");
    assert_eq!(host_band(Some(24)), "lt25");
    assert_eq!(host_band(Some(25)), "25-100");
    assert_eq!(host_band(Some(99)), "25-100");
    assert_eq!(host_band(Some(100)), "100-250");
    assert_eq!(host_band(Some(250)), "100-250");
    assert_eq!(host_band(Some(251)), "gt250");
    assert_eq!(host_version("vps-kvm2", Some(120)), "1:vps-kvm2|100-250");
    assert_eq!(host_version("pc-dev", None), "1:pc-dev|unmeasured");
    assert!(valid_host_label("vps-kvm2") && valid_host_label("pc-dev") && valid_host_label("a"));
    assert!(!valid_host_label("") && !valid_host_label("VPS") && !valid_host_label("a b"));
    assert!(!valid_host_label("a|b") && !valid_host_label(&"x".repeat(33)));
}

#[test]
fn premise_keys_are_sorted_and_need_every_area() {
    let mut v = BTreeMap::new();
    v.insert("fees".to_string(), "1+aa".to_string());
    v.insert("episode_def".to_string(), "2+bb".to_string());
    v.insert("sim".to_string(), "1+cc".to_string());
    assert_eq!(
        premise_key(&["fees", "episode_def"], &v).as_deref(),
        Some("episode_def=2+bb;fees=1+aa")
    );
    assert_eq!(premise_key(&["fees", "depth"], &v), None);
}

#[test]
fn the_premises_table_is_sorted_unique_and_its_versions_are_plain_tokens() {
    let areas: Vec<&str> = PREMISES.iter().map(|p| p.0).collect();
    let mut sorted = areas.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(areas, sorted);
    for (area, version, _) in PREMISES {
        assert!(!version.is_empty(), "{area}");
        assert!(
            !version.contains(['=', ';', '+', '|', ':', ' ']),
            "{area}: version {version:?} must be a plain token"
        );
        assert_eq!(code_version(area), Some(*version));
    }
    assert_eq!(code_version("nope"), None);
}

#[test]
fn file_hashes_ignore_carriage_returns_but_not_paths() {
    let lf = files_hash(&[("a.rs", "x\ny\n")]);
    assert_eq!(files_hash(&[("a.rs", "x\r\ny\r\n")]), lf);
    assert_ne!(files_hash(&[("b.rs", "x\ny\n")]), lf);
    assert_eq!(lf.len(), 64);
    assert_eq!(files_hash(&[]).len(), 64);
}

#[test]
fn an_unbumped_change_fails_and_refresh_refuses_it() {
    let lock = vec![t("fees", "1", "aaa"), t("depth", "1", "bbb")];
    // fee.rs changed, version still 1
    let now = vec![t("fees", "1", "ccc"), t("depth", "1", "bbb")];
    let p = lock_problems(&now, &lock);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("fees: its files changed but its version is still 1"), "{p:?}");
    assert!(refreshed_lock(&now, &lock).is_err());
    // bumped: the check still asks for a refresh, and the refresh is allowed
    let bumped = vec![t("fees", "2", "ccc"), t("depth", "1", "bbb")];
    let p = lock_problems(&bumped, &lock);
    assert_eq!(p, vec!["fees: version 1 → 2; refresh the lock".to_string()]);
    let text = refreshed_lock(&bumped, &lock).unwrap();
    assert_eq!(parse_lock(&text), bumped);
    assert!(lock_problems(&bumped, &parse_lock(&text)).is_empty());
}

#[test]
fn new_and_removed_areas_ask_for_a_refresh_and_the_lock_parses_crlf() {
    let lock = parse_lock("# header\r\nfees 1 aaa\r\n\r\nold 1 zzz\r\n");
    assert_eq!(lock, vec![t("fees", "1", "aaa"), t("old", "1", "zzz")]);
    let now = vec![t("fees", "1", "aaa"), t("sim", "1", "ddd")];
    let p = lock_problems(&now, &lock);
    assert!(p.iter().any(|x| x.starts_with("sim: not in premises.lock")), "{p:?}");
    assert!(p.iter().any(|x| x.starts_with("old: in premises.lock but no longer")), "{p:?}");
    assert!(refreshed_lock(&now, &lock).is_ok());
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p arb-core --test premises`
Expected: compile error `could not find premises in arb_core` (E0432).

- [ ] **Step 3: Add the dependency and the module.** In `crates/arb-core/Cargo.toml`, under `[dependencies]`, add the line `sha2.workspace = true`. In `crates/arb-core/src/lib.rs`, add `pub mod premises;` after `pub mod money;`.

- [ ] **Step 4: Write the module.** Create `crates/arb-core/src/premises.rs`:

```rust
//! Premise versions (Phase 5a spec §4). One table names every area a card can depend on, its code
//! version, and the source files that define it. `premises.lock` holds a sha256 of each area's
//! files; `tests/premises_lock.rs` fails when a file changed but its area's version didn't. The
//! fix: bump the version here (every card that uses the area restarts on a new premise key), then
//!     PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock
//! A run records `area → effective version` in `run_premises`; a card counts only runs whose
//! versions match its premise key, so different premises never mix.
//!
//! `bins/scanner/src/engine.rs` is deliberately not locked (it changes weekly for unrelated
//! reasons); the thresholds behind the flags cards exclude enter through `episode_def_section`.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// (area, code version, source files relative to the workspace root), sorted by area. A version
/// is a plain token: no `=`, `;`, `+`, `|`, `:` or spaces.
pub const PREMISES: &[(&str, &str, &[&str])] = &[
    ("censoring", "1", &["crates/arb-core/src/episode.rs"]),
    ("depth", "1", &["crates/arb-core/src/fill.rs"]),
    ("detector_no", "1", &["crates/arb-core/src/detect.rs"]),
    ("detector_yes", "1", &["crates/arb-core/src/detect.rs"]),
    ("episode_def", "1", &["crates/arb-core/src/episode.rs"]),
    ("fees", "1", &["crates/arb-core/src/fee.rs"]),
    // The host's version is its label and latency band (host_version); no code defines it.
    ("host", "1", &[]),
    (
        "sim",
        "1",
        &[
            "crates/sim/src/account.rs",
            "crates/sim/src/config.rs",
            "crates/sim/src/consumed.rs",
            "crates/sim/src/latency.rs",
            "crates/sim/src/policy.rs",
            "crates/sim/src/trader.rs",
            "crates/sim/src/types.rs",
            "crates/sim/src/venue.rs",
        ],
    ),
];

pub fn code_version(area: &str) -> Option<&'static str> {
    PREMISES.iter().find(|p| p.0 == area).map(|p| p.1)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The first 8 hex digits of sha256(text).
pub fn sha8(text: &str) -> String {
    let d = Sha256::digest(text.as_bytes());
    hex(d.get(..4).unwrap_or_default())
}

/// `{code}+{sha8(config_section)}`: a changed config section behaves like a code change (spec §4).
pub fn effective_version(code: &str, config_section: &str) -> String {
    format!("{code}+{}", sha8(config_section))
}

/// The scanner config behind `episode_def`: the thresholds of the flags OUR METHOD excludes.
pub fn episode_def_section(
    stale_lag_ms: i64,
    suspect_window_ms: i64,
    l1_suspect_window_ms: i64,
    drift_settle_ms: i64,
) -> String {
    format!(
        "stale_lag_ms={stale_lag_ms};suspect_window_ms={suspect_window_ms};\
         l1_suspect_window_ms={l1_suspect_window_ms};drift_settle_ms={drift_settle_ms}"
    )
}

/// The scanner config behind `censoring`: the feed-gap rule (parent spec §5.5).
pub fn censoring_section(gap_ms: i64) -> String {
    format!("gap_ms={gap_ms}")
}

/// The paper trader's config: the frozen policies file, by its sha256.
pub fn sim_section(policies_sha256: &str) -> String {
    format!("policies_sha256={policies_sha256}")
}

/// The band of a measured latency (feed lag p50 + REST RTT p50 / 2, spec §6.3).
pub fn host_band(measured_ms: Option<i64>) -> &'static str {
    match measured_ms {
        None => "unmeasured",
        Some(m) if m < 25 => "lt25",
        Some(m) if m < 100 => "25-100",
        Some(m) if m <= 250 => "100-250",
        Some(_) => "gt250",
    }
}

/// `SCANNER_HOST` labels: 1 to 32 of `a-z`, `0-9` and `-`.
pub fn valid_host_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 32
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The `host` area's version: `{code}:{label}|{band}`.
pub fn host_version(label: &str, measured_ms: Option<i64>) -> String {
    let code = code_version("host").unwrap_or("0");
    format!("{code}:{label}|{}", host_band(measured_ms))
}

/// A card's premise key: `area=version` for each of its areas, sorted by area, joined by `;`.
/// None when a run lacks one of the areas (such a run never counts for the card).
pub fn premise_key(areas: &[&str], versions: &BTreeMap<String, String>) -> Option<String> {
    let mut sorted: Vec<&str> = areas.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::with_capacity(sorted.len());
    for a in sorted {
        parts.push(format!("{a}={}", versions.get(a)?));
    }
    Some(parts.join(";"))
}

/// sha256 over every file as `path NUL text-without-\r NUL`, in the listed order, so a CRLF
/// checkout on Windows and an LF checkout in CI agree.
pub fn files_hash(files: &[(&str, &str)]) -> String {
    let mut h = Sha256::new();
    for (path, text) in files {
        h.update(path.as_bytes());
        h.update([0u8]);
        let stripped: String = text.chars().filter(|c| *c != '\r').collect();
        h.update(stripped.as_bytes());
        h.update([0u8]);
    }
    hex(&h.finalize())
}

/// `(area, version, hash)` lines; `#` comments, blank lines and `\r` are ignored.
pub fn parse_lock(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((
                it.next()?.to_string(),
                it.next()?.to_string(),
                it.next()?.to_string(),
            ))
        })
        .collect()
}

pub fn render_lock(entries: &[(String, String, String)]) -> String {
    let mut out = String::from(
        "# premises.lock: area version sha256(files, \\r stripped). Never edit by hand.\n\
         # Refresh after a version bump: PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock\n",
    );
    for (a, v, h) in entries {
        let _ = writeln!(out, "{a} {v} {h}");
    }
    out
}

/// Everything wrong between the current `(area, version, hash)` triples and the lock; empty when
/// they agree. A changed hash under an unchanged version is the one a refresh can't fix.
pub fn lock_problems(
    current: &[(String, String, String)],
    lock: &[(String, String, String)],
) -> Vec<String> {
    let mut out = Vec::new();
    for (area, version, hash) in current {
        match lock.iter().find(|l| &l.0 == area) {
            None => out.push(format!("{area}: not in premises.lock; refresh the lock")),
            Some((_, lv, lh)) if lh != hash && lv == version => out.push(format!(
                "{area}: its files changed but its version is still {version}: bump it in \
                 crates/arb-core/src/premises.rs (cards restart on the new key), then refresh the lock"
            )),
            Some((_, lv, _)) if lv != version => {
                out.push(format!("{area}: version {lv} → {version}; refresh the lock"))
            }
            Some(_) => {}
        }
    }
    for (a, _, _) in lock {
        if !current.iter().any(|c| &c.0 == a) {
            out.push(format!(
                "{a}: in premises.lock but no longer in PREMISES; refresh the lock"
            ));
        }
    }
    out
}

/// The refreshed lock text, or the unbumped changes that make a refresh refuse.
pub fn refreshed_lock(
    current: &[(String, String, String)],
    lock: &[(String, String, String)],
) -> Result<String, Vec<String>> {
    let unbumped: Vec<String> = lock_problems(current, lock)
        .into_iter()
        .filter(|p| p.contains("version is still"))
        .collect();
    if unbumped.is_empty() {
        Ok(render_lock(current))
    } else {
        Err(unbumped)
    }
}
```

- [ ] **Step 5: Run the pure tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p arb-core --test premises`
Expected: `test result: ok. 7 passed; 0 failed`.

- [ ] **Step 6: Write the lock test, then generate the lock with the refresh command.** Create `crates/arb-core/tests/premises_lock.rs`:

```rust
//! Spec §4 enforcement: a premise area's files may not change while its version stays the same.
//! Refresh (after bumping a version in src/premises.rs):
//!     PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::{PREMISES, files_hash, lock_problems, parse_lock, refreshed_lock};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn lock_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("premises.lock")
}

fn current() -> Vec<(String, String, String)> {
    PREMISES
        .iter()
        .map(|(area, version, files)| {
            let texts: Vec<(String, String)> = files
                .iter()
                .map(|f| {
                    let text = std::fs::read_to_string(root().join(f))
                        .unwrap_or_else(|e| panic!("read {f}: {e}"));
                    (f.to_string(), text)
                })
                .collect();
            let refs: Vec<(&str, &str)> = texts
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str()))
                .collect();
            (area.to_string(), version.to_string(), files_hash(&refs))
        })
        .collect()
}

#[test]
fn premises_lock_matches_the_sources() {
    let lock = parse_lock(&std::fs::read_to_string(lock_path()).unwrap_or_default());
    let now = current();
    if std::env::var("PREMISES_LOCK_WRITE").is_ok() {
        match refreshed_lock(&now, &lock) {
            Ok(text) => {
                std::fs::write(lock_path(), text).unwrap();
                println!("premises.lock refreshed");
                return;
            }
            Err(p) => panic!("refusing to refresh premises.lock:\n{}", p.join("\n")),
        }
    }
    let problems = lock_problems(&now, &lock);
    assert!(
        problems.is_empty(),
        "premises.lock is out of step:\n{}\nBump the area's version in crates/arb-core/src/premises.rs when \
         the change alters what the area means, then run: \
         PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock",
        problems.join("\n")
    );
}
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p arb-core --test premises_lock`
Expected: FAIL with `premises.lock is out of step:` listing `censoring: not in premises.lock; refresh the lock` (and the other seven areas).

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock -- --nocapture`
Expected: PASS, printing `premises.lock refreshed`. `crates/arb-core/premises.lock` now has the two `#` header lines and 8 lines of the form `censoring 1 <64 hex>`; the `host` line hashes zero files (its hash is `e3b0c442…b855`, the sha256 of nothing).

- [ ] **Step 7: Pin the lock to LF and prove the enforcement end to end.** Append this line to `.gitattributes`:

```
crates/arb-core/premises.lock text eol=lf
```

Then prove the test catches an unbumped change, and undo it:

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && echo '// probe' >> crates/arb-core/src/fee.rs && (cargo test -p arb-core --test premises_lock 2>&1 | grep -c 'fees: its files changed but its version is still 1'); git checkout -- crates/arb-core/src/fee.rs && cargo test -p arb-core --test premises_lock`
Expected: first `1` (the failure names `fees`), then after the checkout `test result: ok. 1 passed`.

- [ ] **Step 8: Run all arb-core tests and lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p arb-core && cargo fmt --all && cargo clippy -p arb-core --all-targets -- -D warnings`
Expected: every arb-core test passes (including `no_floats.rs`), no clippy warnings.

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/arb-core/Cargo.toml crates/arb-core/src/lib.rs crates/arb-core/src/premises.rs crates/arb-core/tests/premises.rs crates/arb-core/tests/premises_lock.rs crates/arb-core/premises.lock .gitattributes Cargo.lock && git commit -m "arb-core: premise versions, premises.lock and the unbumped-change test"
```

---

### Task 2: store: migration 0006, `updated_ms`, `run_premises`, health RTT columns, `analyze_rw`, backfill

**Files:**
- Create: `crates/store/migrations/0006_cards.sql`
- Modify: `crates/store/src/batch.rs` (episode upsert + health insert), `crates/store/src/rows.rs` (`HealthRow`, new `RunPremiseRow`), `crates/store/src/runs.rs`, `crates/store/src/read.rs` (`HEALTH_COLS`), `crates/store/src/lib.rs` (two admin helpers), `crates/store/Cargo.toml` (dev-dep `arb-core`), `docker/postgres/init/01-roles.sh`
- Modify (the new `HealthRow` fields in existing literals): `crates/store/tests/batch.rs` (2 literals), `crates/store/tests/read.rs` (fn `health`), `bins/api/tests/common/mod.rs` (fn `health`), `bins/scanner/src/metrics.rs` (`minute_row`; Task 4 fills real values)
- Test: `crates/store/tests/roles.rs`, `crates/store/tests/migrate.rs`, `crates/store/tests/batch.rs`

**Interfaces:**
- Consumes: `arb_core::premises::sha8` (dev-dependency, to check the SQL backfill against Rust); `Store::{start_run, write_batch, raw_exec, migrate}`, `common::fresh`, `TestDb::role` (existing).
- Produces:
  - Tables `run_premises(run_id, area, version)`, `card_cells(card, premise_key, hour_ms, data jsonb, computed_ms)`, `card_results(card, win, premise_key, status, body jsonb, computed_ms)`, `card_meta(key, value jsonb)`; columns `episodes.updated_ms`, `health.rest_rtt_p50_ms`, `health.rest_rtt_p99_ms`; indexes `episodes_updated`, `episodes_closed`, `paper_marks_account_t`; role `analyze_rw`.
  - `store::RunPremiseRow { run_id: i64, area: String, version: String }`
  - `Store::insert_run_premises(&self, rows: &[RunPremiseRow]) -> Result<(), StoreError>` (INSERT … ON CONFLICT DO NOTHING)
  - `Store::run_premises_for(&self, run_id: i64) -> Result<Vec<RunPremiseRow>, StoreError>` (sorted by area)
  - `HealthRow` gains `rest_rtt_p50_ms: Option<i32>`, `rest_rtt_p99_ms: Option<i32>` (after `dropped_groups`)
  - Admin/test tooling: `Store::episode_updated_ms(&self, episode_id: &str) -> Result<Option<i64>, StoreError>`, `Store::role_db_settings(&self, role: &str) -> Result<Vec<String>, StoreError>`

- [ ] **Step 1: Write the failing tests.**

Check the test Postgres first: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` → `pg-up`.

In `crates/store/tests/migrate.rs`, add these four names to the `for t in [ … ]` list (keep it sorted): `"card_cells"`, `"card_meta"`, `"card_results"`, `"run_premises"`. Then append:

```rust
const M0006: &str = include_str!("../migrations/0006_cards.sql");

/// The backfill statement of 0006, verbatim, so a test can re-run it over seeded runs.
fn backfill_sql() -> &'static str {
    let start = M0006.find("INSERT INTO run_premises").expect("backfill start");
    let end = M0006[start..].find("ON CONFLICT DO NOTHING;").expect("backfill end")
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
    store.raw_exec(backfill_sql()).await.expect("backfill is idempotent");
    for run in [1, 6, 8, 9] {
        assert!(store.run_premises_for(run).await.unwrap().is_empty(), "run {run}");
    }
    for run in [7, 10] {
        let rows = store.run_premises_for(run).await.unwrap();
        let areas: Vec<&str> = rows.iter().map(|r| r.area.as_str()).collect();
        assert_eq!(
            areas,
            ["censoring", "depth", "detector_no", "detector_yes", "episode_def", "fees"],
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
```

Append to `crates/store/tests/batch.rs`:

```rust
#[tokio::test]
async fn updated_ms_is_set_on_insert_and_advances_on_every_upsert() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let row = episode("01J00000000000000000000UPD", run);
    let write = |r: EpisodeRow| Batch {
        episodes: vec![EpisodeWrite { row: r, legs: vec![] }],
        ..Batch::default()
    };
    store.write_batch(&write(row.clone())).await.unwrap();
    let first = store
        .episode_updated_ms(&row.episode_id)
        .await
        .unwrap()
        .expect("row exists");
    assert!(first > 1_700_000_000_000, "wall-clock ms, got {first}");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let mut closed = row.clone();
    closed.closed_server_ms = Some(9_000);
    store.write_batch(&write(closed)).await.unwrap();
    let second = store.episode_updated_ms(&row.episode_id).await.unwrap().unwrap();
    assert!(second > first, "{second} > {first}");
    assert_eq!(store.episode_updated_ms("nope").await.unwrap(), None);
    db.drop(store).await;
}

#[tokio::test]
async fn health_rtt_columns_round_trip_and_upsert() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut h = HealthRow {
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
        rest_rtt_p50_ms: None,
        rest_rtt_p99_ms: None,
    };
    let b = |h: HealthRow| Batch {
        health: vec![h],
        ..Batch::default()
    };
    store.write_batch(&b(h.clone())).await.unwrap();
    h.rest_rtt_p50_ms = Some(130);
    h.rest_rtt_p99_ms = Some(0);
    store.write_batch(&b(h.clone())).await.unwrap();
    let ro = Store::connect_read_only(db.opts.clone(), 1, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let rows = ro.health_series(0, 120_000).await.unwrap();
    assert_eq!(
        (rows[0].rest_rtt_p50_ms, rows[0].rest_rtt_p99_ms),
        (Some(130), Some(0)),
        "a measured 0 stays 0, not NULL"
    );
    ro.close().await;
    db.drop(store).await;
}
```

In the same file, add `rest_rtt_p50_ms: None,` and `rest_rtt_p99_ms: None,` after `dropped_groups: 0,` in the two existing `HealthRow { … }` literals (in `observations_integrity_and_health_land_and_health_upserts` and `duplicate_health_minute_in_one_batch_keeps_the_last`). Do the same in `crates/store/tests/read.rs` (fn `health`) and `bins/api/tests/common/mod.rs` (fn `health`).

Append to `crates/store/tests/roles.rs`:

```rust
#[tokio::test]
async fn analyze_rw_reads_the_scanner_tables_and_writes_only_card_tables() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let rw = db.role("analyze_rw").await;
    for t in ["episodes", "runs", "run_premises", "health", "paper_marks", "paper_accounts"] {
        rw.raw_exec(&format!("SELECT count(*) FROM {t}"))
            .await
            .unwrap_or_else(|e| panic!("analyze_rw reads {t}: {e}"));
    }
    rw.raw_exec(
        "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) VALUES ('g1', 'k', 0, '{}', 1); \
         UPDATE card_cells SET computed_ms = 2; \
         INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
             VALUES ('G1', '7d', 'k', 'live', '{}', 1); \
         UPDATE card_results SET computed_ms = 2; \
         INSERT INTO card_meta (key, value) VALUES ('tick', '{}'); \
         UPDATE card_meta SET value = '{\"a\":1}'; \
         DELETE FROM card_cells; DELETE FROM card_results; DELETE FROM card_meta",
    )
    .await
    .expect("analyze_rw writes the card tables");
    for sql in [
        "INSERT INTO runs (service, started_ms, version, config_hash, mode) VALUES ('x', 1, 'v', 'c', 'live')",
        "UPDATE episodes SET kind = kind",
        "DELETE FROM episodes",
        "UPDATE health SET msgs = msgs",
        "INSERT INTO run_premises (run_id, area, version) VALUES (1, 'fees', '1')",
        "UPDATE paper_marks SET equity = equity",
        "DELETE FROM paper_accounts",
        "UPDATE paper_state SET updated_ms = 1",
        "UPDATE legs SET status = status",
    ] {
        let e = rw.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    let ddl = rw.raw_exec("CREATE TABLE nope (a int)").await.unwrap_err();
    assert!(is_permission_denied(&ddl), "{ddl}");
    let settings = owner.role_db_settings("analyze_rw").await.unwrap();
    assert!(
        settings.iter().any(|s| s == "statement_timeout=10s"),
        "{settings:?}"
    );
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn analyze_ro_reads_card_results_and_meta_but_still_cannot_write() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let ro = db.role("analyze_ro").await;
    ro.raw_exec("SELECT count(*) FROM card_results; SELECT count(*) FROM card_meta")
        .await
        .expect("analyze_ro reads results");
    for sql in [
        "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) VALUES ('G1', '7d', 'k', 'live', '{}', 1)",
        "INSERT INTO card_meta (key, value) VALUES ('x', '{}')",
        "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) VALUES ('g1', 'k', 0, '{}', 1)",
        "INSERT INTO run_premises (run_id, area, version) VALUES (1, 'fees', '1')",
    ] {
        let e = ro.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    ro.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn scanner_appends_run_premises_but_never_rewrites_them() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let scanner = db.role("scanner_rw").await;
    let run = scanner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap();
    let rows = vec![
        RunPremiseRow { run_id: run, area: "fees".into(), version: "1+e3b0c442".into() },
        RunPremiseRow { run_id: run, area: "depth".into(), version: "1+e3b0c442".into() },
    ];
    scanner.insert_run_premises(&rows).await.expect("scanner appends");
    scanner
        .insert_run_premises(&rows)
        .await
        .expect("a re-sent row is a no-op, not an error");
    let back = scanner.run_premises_for(run).await.unwrap();
    assert_eq!(back.iter().map(|r| r.area.as_str()).collect::<Vec<_>>(), ["depth", "fees"]);
    for sql in ["UPDATE run_premises SET version = 'x'", "DELETE FROM run_premises"] {
        let e = scanner.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    let cards = scanner
        .raw_exec("INSERT INTO card_meta (key, value) VALUES ('x', '{}')")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&cards), "{cards}");
    scanner.close().await;
    db.drop(owner).await;
}
```

In `crates/store/Cargo.toml`, under `[dev-dependencies]`, add `arb-core = { path = "../arb-core" }`.

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p store --test migrate --test roles --test batch`
Expected: compile errors: `struct HealthRow has no field named rest_rtt_p50_ms`, `no method named episode_updated_ms`, `cannot find struct RunPremiseRow`, and `couldn't read …/0006_cards.sql` from `include_str!`.

- [ ] **Step 3: Write the migration.** Create `crates/store/migrations/0006_cards.sql`:

```sql
-- Phase 5a (spec 2026-09-26-phase5a-cards-design.md §2–§4, §11): live cards. Runs as the owner (ct_owner).

-- 1. The changed-row cursor analyze reads. The store's episode upsert sets updated_ms on every
--    insert and update. Existing rows get opened_server_ms, so analyze's first start sees them.
--    The NOTIFY trigger is paused for this one-time fill (it would fire once per existing row).
ALTER TABLE episodes ADD COLUMN updated_ms BIGINT NOT NULL DEFAULT 0;
ALTER TABLE episodes DISABLE TRIGGER episodes_notify;
UPDATE episodes SET updated_ms = opened_server_ms;
ALTER TABLE episodes ENABLE TRIGGER episodes_notify;
CREATE INDEX episodes_updated ON episodes (updated_ms);
-- Hour-overlap reads (G3, capital overlap): episodes still open, or closed after an hour starts.
CREATE INDEX episodes_closed ON episodes (closed_server_ms);
-- Edge capture: an account's previous mark before a window, by index, not by scan.
CREATE INDEX paper_marks_account_t ON paper_marks (account_id, t_ms);

-- 2. The premises each run was started under (spec §4). Append-only: the scanner inserts, never
--    updates (host arrives as its own row after the run's first 10 health minutes).
CREATE TABLE run_premises (
    run_id   BIGINT NOT NULL REFERENCES runs (run_id),
    area     TEXT   NOT NULL,
    version  TEXT   NOT NULL,
    PRIMARY KEY (run_id, area)
);
GRANT SELECT, INSERT ON run_premises TO scanner_rw;

-- 3. Per-minute percentiles of the scanner's own CLOB request durations (pm-api times requests it
--    already makes; no new traffic). No new GRANT: the scanner already writes health.
ALTER TABLE health ADD COLUMN rest_rtt_p50_ms INTEGER NULL;
ALTER TABLE health ADD COLUMN rest_rtt_p99_ms INTEGER NULL;

-- 4. Cards (spec §3, §7). JSON holds exact decimals as strings and integer counts, never floats.
--    card_cells.card is the cell family (g1, dur, g4, edge); card_results.card is the card (G1…G4, EDGE).
CREATE TABLE card_cells (
    card         TEXT   NOT NULL,
    premise_key  TEXT   NOT NULL,
    hour_ms      BIGINT NOT NULL,
    data         JSONB  NOT NULL,
    computed_ms  BIGINT NOT NULL,
    PRIMARY KEY (card, premise_key, hour_ms)
);
CREATE TABLE card_results (
    card         TEXT   NOT NULL,
    win          TEXT   NOT NULL CHECK (win IN ('24h', '7d', '30d', 'all', 'today')),
    premise_key  TEXT   NOT NULL,
    status       TEXT   NOT NULL CHECK (status IN ('live', 'warming', 'thin', 'under_review')),
    body         JSONB  NOT NULL,
    computed_ms  BIGINT NOT NULL,
    PRIMARY KEY (card, win)
);
CREATE TABLE card_meta (
    key    TEXT  PRIMARY KEY,
    value  JSONB NOT NULL
);

-- 5. analyze_rw (spec §2): reads the scanner's tables, writes only the card tables. A migration
--    can't carry a secret, so the role is NOLOGIN here; the runbook sets LOGIN PASSWORD once, and
--    fresh volumes get it from docker/postgres/init/01-roles.sh. Roles are cluster-wide: tolerate a
--    role that 01-roles.sh or another database's migration already created.
DO $$ BEGIN
    CREATE ROLE analyze_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
-- Per database, so parallel test databases never update the same pg_db_role_setting row.
DO $$ BEGIN
    EXECUTE format('ALTER ROLE analyze_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '10s');
END $$;
GRANT USAGE ON SCHEMA public TO analyze_rw;
GRANT SELECT ON episodes, runs, run_premises, health, paper_marks, paper_accounts TO analyze_rw;
GRANT SELECT, INSERT, UPDATE, DELETE ON card_cells, card_results, card_meta TO analyze_rw;
-- The dashboard api (analyze_ro) reads results only. 0001's default privileges already give it
-- SELECT on every new table; spelled out for the two it uses.
GRANT SELECT ON card_results, card_meta TO analyze_ro;

-- 6. Backfill (spec §3, §11; list approved 2026-09-26): live scanner runs from run_id 7 (soak run 3,
--    image 2c5b84d) on, which ran today's episode, censoring, detector, fee and depth logic. Runs
--    1–6 and every replay, safe, census and breaker_reset run are never backfilled. sim and host are
--    not backfilled: those runs measured no REST round trip, so Edge capture starts at the deploy.
--    version = code || '+' || sha8(section), from the literal sections below, which
--    bins/scanner/tests/premises.rs pins to config/scanner.toml and arb_core::premises.
INSERT INTO run_premises (run_id, area, version)
SELECT r.run_id, p.area, p.code || '+' || left(encode(sha256(convert_to(p.section, 'UTF8')), 'hex'), 8)
FROM runs r
CROSS JOIN (VALUES
-- backfill:begin
    ('censoring', '1', 'gap_ms=30000'),
    ('depth', '1', ''),
    ('detector_no', '1', ''),
    ('detector_yes', '1', ''),
    ('episode_def', '1', 'stale_lag_ms=500;suspect_window_ms=60000;l1_suspect_window_ms=3600000;drift_settle_ms=2000'),
    ('fees', '1', '')
-- backfill:end
) AS p (area, code, section)
WHERE r.run_id >= 7 AND r.service = 'scanner' AND r.mode = 'live'
ON CONFLICT DO NOTHING;
```

- [ ] **Step 4: The rows.** In `crates/store/src/rows.rs`, inside `pub struct HealthRow`, after `pub dropped_groups: i32,` add:

```rust
    /// p50 / p99 of the scanner's own CLOB request round trips this minute (send → response
    /// headers, successful requests only). None: no request finished this minute.
    pub rest_rtt_p50_ms: Option<i32>,
    pub rest_rtt_p99_ms: Option<i32>,
```

After the `HealthRow` struct, add:

```rust
/// One premise a run was started under (Phase 5a spec §4): `version` is `code+sha8(config)`, or
/// `code:label|band` for `host`.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RunPremiseRow {
    pub run_id: i64,
    pub area: String,
    pub version: String,
}
```

In `bins/scanner/src/metrics.rs`, inside `minute_row`'s `HealthRow { … }`, after `dropped_groups: …,` add `rest_rtt_p50_ms: None,` and `rest_rtt_p99_ms: None,` (Task 4 replaces both with measured values).

- [ ] **Step 5: The episode upsert sets `updated_ms`; health writes the RTT columns.** In `crates/store/src/batch.rs`:

After `const CHUNK: usize = 1000;` add:

```rust
/// The database clock in ms, evaluated per row: every insert and update of an episode stamps
/// `updated_ms` with it, and analyze's cursor compares only these stamps (one clock).
const NOW_MS_SQL: &str = "(extract(epoch from clock_timestamp()) * 1000)::bigint";
```

In `write_batch`, replace the `update_set` expression and the episode insert loop with:

```rust
        let update_set = EPISODE_COLS[1..]
            .iter()
            .map(|c| format!("{c} = EXCLUDED.{c}"))
            .chain(std::iter::once("updated_ms = EXCLUDED.updated_ms".to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        let episodes = dedupe_last(&b.episodes);
        for chunk in episodes.chunks(EP_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(format!(
                "INSERT INTO episodes ({}, updated_ms) ",
                EPISODE_COLS.join(", ")
            ));
            qb.push_values(chunk, |mut sep, w| {
                push_episode(&mut sep, &w.row);
                sep.push(NOW_MS_SQL);
            });
            qb.push(format!(
                " ON CONFLICT (episode_id) DO UPDATE SET {update_set}"
            ));
            qb.build().execute(&mut *tx).await?;
        }
```

In the health block, change the column list to end with `…, capture_paused, parked_shards, dropped_groups, rest_rtt_p50_ms, rest_rtt_p99_ms) `, add `.push_bind(h.rest_rtt_p50_ms).push_bind(h.rest_rtt_p99_ms)` after `.push_bind(h.dropped_groups)`, and end the `ON CONFLICT … DO UPDATE SET` string with `…, dropped_groups = EXCLUDED.dropped_groups, rest_rtt_p50_ms = EXCLUDED.rest_rtt_p50_ms, rest_rtt_p99_ms = EXCLUDED.rest_rtt_p99_ms`.

In `crates/store/src/read.rs`, change `HEALTH_COLS` to end with `…, capture_paused, parked_shards, dropped_groups, rest_rtt_p50_ms, rest_rtt_p99_ms";`.

- [ ] **Step 6: `run_premises` reads and writes.** Append to `impl Store` in `crates/store/src/runs.rs` (and change its `use crate::{NewRun, Store, StoreError};` to `use crate::{NewRun, RunPremiseRow, Store, StoreError};`):

```rust
    /// Append this run's premises (spec §4). Re-sending a row is a no-op (the scanner may retry).
    pub async fn insert_run_premises(&self, rows: &[RunPremiseRow]) -> Result<(), StoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "INSERT INTO run_premises (run_id, area, version) ",
        );
        qb.push_values(rows, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.area))
                .push_bind(clean(&r.version));
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(self.pool()).await?;
        Ok(())
    }

    pub async fn run_premises_for(&self, run_id: i64) -> Result<Vec<RunPremiseRow>, StoreError> {
        Ok(sqlx::query_as::<_, RunPremiseRow>(
            "SELECT run_id, area, version FROM run_premises WHERE run_id = $1 ORDER BY area",
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?)
    }
```

In `crates/store/src/lib.rs`, append inside `impl Store`:

```rust
    /// Admin/test tooling: one episode's `updated_ms`.
    pub async fn episode_updated_ms(&self, episode_id: &str) -> Result<Option<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT updated_ms FROM episodes WHERE episode_id = $1")
                .bind(episode_id)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    /// Admin/test tooling: `role`'s settings for THIS database (`ALTER ROLE … IN DATABASE … SET`).
    pub async fn role_db_settings(&self, role: &str) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT unnest(s.setconfig)::text FROM pg_db_role_setting s \
             JOIN pg_roles r ON r.oid = s.setrole JOIN pg_database d ON d.oid = s.setdatabase \
             WHERE r.rolname = $1 AND d.datname = current_database() ORDER BY 1",
        )
        .bind(role)
        .fetch_all(self.pool())
        .await?)
    }
```

- [ ] **Step 7: The role on fresh volumes.** Replace `docker/postgres/init/01-roles.sh` with:

```sh
#!/bin/sh
# Spec §10 roles. Table privileges are granted by the migrations (crates/store/migrations).
# NOTE: runs only on an EMPTY pgdata volume; changing a password later needs ALTER ROLE by hand.
# analyze_rw (Phase 5a) is created LOGIN here when ANALYZE_RW_DB_PASSWORD is set; otherwise
# migration 0006 creates it NOLOGIN and the runbook activates it once.
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
     -v scanner_pw="$SCANNER_DB_PASSWORD" -v analyze_pw="$ANALYZE_DB_PASSWORD" <<'SQL'
CREATE ROLE scanner_rw LOGIN PASSWORD :'scanner_pw';
CREATE ROLE analyze_ro LOGIN PASSWORD :'analyze_pw';
ALTER ROLE scanner_rw SET statement_timeout = '5s';
ALTER ROLE analyze_ro SET statement_timeout = '60s';
SQL
if [ -n "${ANALYZE_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
       -v analyze_rw_pw="$ANALYZE_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE analyze_rw LOGIN PASSWORD :'analyze_rw_pw';
ALTER ROLE analyze_rw SET statement_timeout = '10s';
SQL
fi
```

- [ ] **Step 8: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p store && cargo test -p api && cargo test -p scanner --lib`
Expected: PASS: the new `migrate.rs`, `batch.rs` and `roles.rs` tests and every existing store, api and scanner-lib test; none prints `SKIPPED`.

- [ ] **Step 9: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 10: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/store/migrations/0006_cards.sql crates/store/src/batch.rs crates/store/src/rows.rs crates/store/src/runs.rs crates/store/src/read.rs crates/store/src/lib.rs crates/store/Cargo.toml crates/store/tests/migrate.rs crates/store/tests/batch.rs crates/store/tests/read.rs crates/store/tests/roles.rs bins/api/tests/common/mod.rs bins/scanner/src/metrics.rs docker/postgres/init/01-roles.sh Cargo.lock && git commit -m "store: migration 0006 (updated_ms, run_premises, health RTT, card tables, analyze_rw, backfill from run 7)"
```

---

### Task 3: store: what `analyze` reads and writes, and the api's two card reads

**Files:**
- Create: `crates/store/src/card_io.rs`, `crates/store/tests/card_io.rs`
- Modify: `crates/store/src/lib.rs` (`mod card_io;` + re-exports), `crates/store/src/error.rs` (`TooManyRows`)

**Interfaces:**
- Consumes: Task 2's tables and `RunPremiseRow`, `Store::insert_run_premises`; `EPISODE_COLS` is not needed (explicit column lists).
- Produces (all `pub`, re-exported from `store`):
  - consts `MAX_CARD_EPISODES: i64 = 200_000`, `MAX_CHANGED_PAGE: i64 = 5_000`, `CARD_TABLES: [&str; 3] = ["card_cells", "card_results", "card_meta"]`
  - `struct PremiseRun { run_id: i64, started_ms: i64, end_ms: Option<i64>, premises: Vec<(String, String)> }`
  - `struct ChangedSpan { episode_id: String, run_id: i64, opened_ms: i64, closed_ms: Option<i64>, updated_ms: i64 }`
  - `struct CardEpisode { episode_id: String, run_id: i64, kind: String, opened_ms: i64, closed_ms: Option<i64>, duration_ms: Option<i64>, censor_left: bool, censor_right: bool, censor_gap: bool, edge_net: Decimal, edge_gross: Decimal, pnl_net: Decimal, pnl_gross: Decimal, cost: Decimal, budget100: Option<Decimal>, suspect: bool, stale: bool, prev_gap_ms: Option<i64> }`
  - `struct LatencyMinute { run_id: i64, minute_ms: i64, lag_p50_ms: Option<i32>, rest_rtt_p50_ms: Option<i32> }`
  - `struct MarkPoint { run_id: i64, account_id: String, t_ms: i64, equity: Decimal }`
  - `struct CardCellRow { card: String, premise_key: String, hour_ms: i64, data: String, computed_ms: i64 }`
  - `struct CardResultRow { card: String, win: String, premise_key: String, status: String, body: String, computed_ms: i64 }`
  - `Store::connect_analyze(opts: PgConnectOptions, max_conns: u32, statement_timeout_ms: u32, role: Option<&str>) -> Result<Store, StoreError>`
  - `Store::role_writes_outside_cards(&self) -> Result<Vec<String>, StoreError>`
  - `Store::premise_runs(&self) -> Result<Vec<PremiseRun>, StoreError>`
  - `Store::max_episode_updated_ms(&self) -> Result<Option<i64>, StoreError>`
  - `Store::changed_spans(&self, after_ms: i64, after_id: &str, limit: i64) -> Result<Vec<ChangedSpan>, StoreError>`
  - `Store::card_episodes(&self, run_ids: &[i64], from_ms: i64, to_ms: i64) -> Result<Vec<CardEpisode>, StoreError>`
  - `Store::health_latency(&self, run_ids: &[i64], from_ms: i64, to_ms: i64) -> Result<Vec<LatencyMinute>, StoreError>`
  - `Store::p0_accounts(&self) -> Result<Vec<(String, String)>, StoreError>` (account_id, scenario)
  - `Store::marks_with_prev(&self, accounts: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<MarkPoint>, StoreError>`
  - `Store::card_cells_range(&self, card: &str, premise_key: &str, from_hour: i64, to_hour: i64) -> Result<Vec<(i64, String)>, StoreError>` ([from, to))
  - `Store::card_cells_at(&self, card: &str, premise_key: &str, hours: &[i64]) -> Result<Vec<(i64, String)>, StoreError>`
  - `Store::card_cells_put(&self, rows: &[CardCellRow]) -> Result<(), StoreError>`
  - `Store::card_results_put(&self, rows: &[CardResultRow], meta: &[(String, String)]) -> Result<(), StoreError>`
  - `Store::card_meta_get(&self, key: &str) -> Result<Option<String>, StoreError>`, `Store::card_meta_put(&self, key: &str, value: &str) -> Result<(), StoreError>`
  - api: `Store::card_results_for(&self, win: &str) -> Result<Vec<(String, String)>, StoreError>` (card, body), `Store::card_meta_all(&self) -> Result<Vec<(String, String)>, StoreError>` (key, value); both return empty before 0006 (`to_regclass`).
  - `StoreError::TooManyRows(&'static str)`

- [ ] **Step 1: Write the failing tests.** Create `crates/store/tests/card_io.rs`:

```rust
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
            episodes: rows.into_iter().map(|row| EpisodeWrite { row, legs: vec![] }).collect(),
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
    let Some((owner, db)) = common::fresh().await else { return };
    let analyze = rw(&db).await;
    assert_eq!(analyze.role_writes_outside_cards().await.unwrap(), Vec::<String>::new());
    let scanner = db.role("scanner_rw").await;
    assert!(scanner.role_writes_outside_cards().await.unwrap().contains(&"episodes".to_string()));
    assert!(owner.role_writes_outside_cards().await.unwrap().contains(&"runs".to_string()));
    analyze.close().await;
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn premise_runs_are_live_scanner_runs_with_premises_and_their_ends() {
    let Some((owner, db)) = common::fresh().await else { return };
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
    let got: Vec<(i64, i64, Option<i64>)> = runs.iter().map(|r| (r.run_id, r.started_ms, r.end_ms)).collect();
    assert_eq!(
        got,
        vec![(ended, 1_000, Some(1_500)), (crashed, 3_000, Some(4_000)), (running, 4_000, None)],
        "a crashed run ends where the next scanner session starts; bare {bare} and replay {replay} never count"
    );
    assert_eq!(runs[0].premises, vec![("fees".to_string(), "1+e3b0c442".to_string())]);
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn changed_spans_page_by_updated_ms_then_id_and_skip_replays() {
    let Some((owner, db)) = common::fresh().await else { return };
    let run = run_at(&owner, "live", 1).await;
    let mut replay = ep("replay-1", run, "no_basket", 10, None);
    replay.source = "replay".into();
    write(&owner, vec![ep("a", run, "no_basket", 10, Some(20)), ep("b", run, "yes_basket", 30, None), replay]).await;
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
    assert_eq!(first.len() + rest.len(), 2, "keyset pages never repeat a row");
    let max = analyze.max_episode_updated_ms().await.unwrap().unwrap();
    assert_eq!(max, all.iter().map(|s| s.updated_ms).max().unwrap());
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn card_episodes_return_hour_overlaps_with_the_previous_gap() {
    let Some((owner, db)) = common::fresh().await else { return };
    let run = run_at(&owner, "live", 1).await;
    let other = run_at(&owner, "live", 2).await;
    let mut imp = ep("imp", run, "implication", 1_600, Some(1_700));
    imp.event_id = "ev2".into();
    write(
        &owner,
        vec![
            ep("a", run, "no_basket", 1_000, Some(2_000)),
            ep("b", run, "no_basket", 2_200, Some(2_400)), // same event and kind: gap 200 ms
            ep("c", run, "yes_basket", 5_000, None),        // still open
            ep("old", run, "no_basket", 100, Some(1_400)),  // closed before the range
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
    assert_eq!((got[2].closed_ms, got[2].kind.as_str()), (None, "yes_basket"));
    assert_eq!((got[0].pnl_net, got[0].budget100, got[0].cost), (dec!(0.41), Some(dec!(0.33)), dec!(99.59)));
    assert!(analyze.card_episodes(&[], 0, 10_000).await.unwrap().is_empty());
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn health_latency_and_marks_read_the_edge_inputs() {
    let Some((owner, db)) = common::fresh().await else { return };
    let run = run_at(&owner, "live", 1).await;
    let h = HealthRow {
        run_id: run, minute_ms: 60_000, tokens: 1, shards: 1, msgs: 1, bytes: 1, drops: 0, reconnects: 0,
        rest_p50_ms: None, rest_p99_ms: None, lag_p50_ms: Some(80), lag_p99_ms: None, clock_offset_ms: None,
        ok: None, capture_paused: false, parked_shards: 0, dropped_groups: 0,
        rest_rtt_p50_ms: Some(130), rest_rtt_p99_ms: Some(300),
    };
    let h2 = HealthRow { minute_ms: 120_000, rest_rtt_p50_ms: None, ..h.clone() };
    let acct = |id: &str, policy: &str, scenario: &str, fee: rust_decimal::Decimal| PaperAccountRow {
        run_id: run, account_id: id.into(), policy: policy.into(), scenario: scenario.into(),
        fee_bips: fee, bankroll: dec!(1000), per_trade: dec!(100), policies_sha256: "ab".repeat(32),
    };
    let mark = |id: &str, t: i64, eq: rust_decimal::Decimal| PaperMarkRow {
        run_id: run, account_id: id.into(), t_ms: t, cash: eq, equity: eq, realized: dec!(0),
        unrealized: dec!(0), fees_paid: dec!(0), locked: dec!(0), max_drawdown: dec!(0), n_positions: 0,
    };
    owner
        .write_batch(&Batch {
            health: vec![h, h2],
            paper_accounts: vec![
                acct("P0-converter/sp-idle", "P0-converter", "sp-idle", dec!(0)),
                acct("P0-converter/sp-idle/fee10", "P0-converter", "sp-idle", dec!(10)),
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
        lat.iter().map(|m| (m.minute_ms, m.lag_p50_ms, m.rest_rtt_p50_ms)).collect::<Vec<_>>(),
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
    let Some((owner, db)) = common::fresh().await else { return };
    let analyze = rw(&db).await;
    let cell = |h: i64, data: &str| CardCellRow {
        card: "g1".into(), premise_key: "k1".into(), hour_ms: h, data: data.into(), computed_ms: 1,
    };
    analyze.card_cells_put(&[cell(0, r#"{"a":1}"#), cell(H, r#"{"a":2}"#), cell(2 * H, r#"{"a":3}"#)]).await.unwrap();
    analyze.card_cells_put(&[cell(H, r#"{"a":22}"#)]).await.unwrap();
    let range = analyze.card_cells_range("g1", "k1", 0, 2 * H).await.unwrap();
    assert_eq!(range, vec![(0, r#"{"a": 1}"#.to_string()), (H, r#"{"a": 22}"#.to_string())]);
    let at = analyze.card_cells_at("g1", "k1", &[2 * H, 5 * H]).await.unwrap();
    assert_eq!(at, vec![(2 * H, r#"{"a": 3}"#.to_string())]);
    assert!(analyze.card_cells_range("g1", "other", 0, 10 * H).await.unwrap().is_empty());
    let body = r#"{"card":"G1","columns":[{"ours":"0","their":null}]}"#;
    analyze
        .card_results_put(
            &[CardResultRow {
                card: "G1".into(), win: "7d".into(), premise_key: "k1".into(), status: "thin".into(),
                body: body.into(), computed_ms: 5,
            }],
            &[("tick".to_string(), r#"{"last_tick_ms":5}"#.to_string())],
        )
        .await
        .unwrap();
    analyze.card_meta_put("cursor", r#"{"updated_ms":7}"#).await.unwrap();
    assert_eq!(analyze.card_meta_get("cursor").await.unwrap().as_deref(), Some(r#"{"updated_ms": 7}"#));
    assert_eq!(analyze.card_meta_get("nope").await.unwrap(), None);
    let api = Store::connect_read_only(db.opts.clone(), 1, 5_000, Some("analyze_ro")).await.unwrap();
    let rows = api.card_results_for("7d").await.unwrap();
    assert_eq!(rows.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&rows[0].1).unwrap();
    assert_eq!(v["columns"][0]["ours"], "0");
    assert!(v["columns"][0]["their"].is_null());
    assert!(api.card_results_for("24h").await.unwrap().is_empty());
    let meta = api.card_meta_all().await.unwrap();
    assert_eq!(meta.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["cursor", "tick"]);
    api.close().await;
    analyze.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p store --test card_io`
Expected: compile errors such as `no function or associated item named connect_analyze found for struct Store` and `cannot find struct CardCellRow`.

- [ ] **Step 3: The error variant.** In `crates/store/src/error.rs`, add to `enum StoreError`:

```rust
    /// A bounded read hit its row cap: the caller must narrow the range (never a silent truncation).
    #[error("too many rows: {0}")]
    TooManyRows(&'static str),
```

- [ ] **Step 4: Write the module.** Create `crates/store/src/card_io.rs`:

```rust
//! Phase 5a: what `analyze` reads (episodes, runs, run_premises, health, paper marks) and writes
//! (card_cells, card_results, card_meta), and the api's two card reads. JSON columns travel as
//! text (`$n::jsonb` in, `col::text` out), so `store` needs no JSON dependency. Every read is
//! parameterized and capped; a capped read that overflows is an error, never a truncation.
use crate::text::clean;
use crate::{Store, StoreError};
use rust_decimal::Decimal;
use sqlx::postgres::PgConnectOptions;
use sqlx::{Postgres, QueryBuilder};

pub const MAX_CARD_EPISODES: i64 = 200_000;
pub const MAX_CHANGED_PAGE: i64 = 5_000;
pub const CARD_TABLES: [&str; 3] = ["card_cells", "card_results", "card_meta"];
const MAX_ROWS: i64 = 200_000;
const CELL_CHUNK: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PremiseRun {
    pub run_id: i64,
    pub started_ms: i64,
    /// `ended_ms`, else the start of the next scanner session (a crashed run), else None (running).
    pub end_ms: Option<i64>,
    /// (area, version), sorted by area.
    pub premises: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ChangedSpan {
    pub episode_id: String,
    pub run_id: i64,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub updated_ms: i64,
}

/// An episode as the cards need it. `prev_gap_ms` = this open minus the latest close of another
/// live episode with the same event and kind that closed within the second before (merge-gap
/// sensitivity, parent spec §5.5).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CardEpisode {
    pub episode_id: String,
    pub run_id: i64,
    pub kind: String,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    pub edge_net: Decimal,
    pub edge_gross: Decimal,
    pub pnl_net: Decimal,
    pub pnl_gross: Decimal,
    pub cost: Decimal,
    pub budget100: Option<Decimal>,
    pub suspect: bool,
    pub stale: bool,
    pub prev_gap_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LatencyMinute {
    pub run_id: i64,
    pub minute_ms: i64,
    pub lag_p50_ms: Option<i32>,
    pub rest_rtt_p50_ms: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct MarkPoint {
    pub run_id: i64,
    pub account_id: String,
    pub t_ms: i64,
    pub equity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCellRow {
    pub card: String,
    pub premise_key: String,
    pub hour_ms: i64,
    pub data: String,
    pub computed_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardResultRow {
    pub card: String,
    pub win: String,
    pub premise_key: String,
    pub status: String,
    pub body: String,
    pub computed_ms: i64,
}

fn capped<T>(mut rows: Vec<T>, cap: i64, what: &'static str) -> Result<Vec<T>, StoreError> {
    if rows.len() > usize::try_from(cap).unwrap_or(usize::MAX) {
        rows.clear();
        return Err(StoreError::TooManyRows(what));
    }
    Ok(rows)
}

impl Store {
    /// analyze's pool: every session carries its own statement timeout (on top of the role's
    /// 10 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_analyze(
        opts: PgConnectOptions,
        max_conns: u32,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([("statement_timeout", timeout.as_str())]);
        match role {
            Some(r) => Self::connect_with_role(opts, max_conns, r).await,
            None => Self::connect_with(opts, max_conns).await,
        }
    }

    /// Tables outside the card tables this role could INSERT, UPDATE, DELETE or TRUNCATE (analyze
    /// refuses to start unless this is empty: spec §2).
    pub async fn role_writes_outside_cards(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT (c.relname::text = ANY($1)) \
               AND (has_table_privilege(current_user, c.oid, 'INSERT') \
                 OR has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE')) \
             ORDER BY 1",
        )
        .bind(CARD_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Live scanner runs that recorded premises, oldest first (runs without them never count).
    pub async fn premise_runs(&self) -> Result<Vec<PremiseRun>, StoreError> {
        let runs: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(
            "SELECT r.run_id, r.started_ms, \
                    COALESCE(r.ended_ms, (SELECT min(n.started_ms) FROM runs n \
                        WHERE n.service = 'scanner' AND n.mode IN ('live', 'safe') AND n.run_id > r.run_id)) \
             FROM runs r \
             WHERE r.service = 'scanner' AND r.mode = 'live' \
               AND EXISTS (SELECT 1 FROM run_premises p WHERE p.run_id = r.run_id) \
             ORDER BY r.run_id LIMIT 100000",
        )
        .fetch_all(self.pool())
        .await?;
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT run_id, area, version FROM run_premises ORDER BY run_id, area LIMIT 1000000",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(runs
            .into_iter()
            .map(|(run_id, started_ms, end_ms)| PremiseRun {
                run_id,
                started_ms,
                end_ms,
                premises: rows
                    .iter()
                    .filter(|r| r.0 == run_id)
                    .map(|r| (r.1.clone(), r.2.clone()))
                    .collect(),
            })
            .collect())
    }

    pub async fn max_episode_updated_ms(&self) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query_scalar("SELECT max(updated_ms) FROM episodes")
            .fetch_one(self.pool())
            .await?)
    }

    /// Live episodes changed after `(after_ms, after_id)`, keyset-paged on (updated_ms, episode_id).
    pub async fn changed_spans(
        &self,
        after_ms: i64,
        after_id: &str,
        limit: i64,
    ) -> Result<Vec<ChangedSpan>, StoreError> {
        Ok(sqlx::query_as::<_, ChangedSpan>(
            "SELECT episode_id, run_id, opened_server_ms AS opened_ms, closed_server_ms AS closed_ms, updated_ms \
             FROM episodes WHERE source = 'live' AND updated_ms >= $1 AND (updated_ms, episode_id) > ($1, $2) \
             ORDER BY updated_ms, episode_id LIMIT $3",
        )
        .bind(after_ms)
        .bind(after_id)
        .bind(limit.clamp(1, MAX_CHANGED_PAGE))
        .fetch_all(self.pool())
        .await?)
    }

    /// YES/NO-basket live episodes of `run_ids` that overlap [from_ms, to_ms): opened before
    /// `to_ms` and still open or closed at or after `from_ms`.
    pub async fn card_episodes(
        &self,
        run_ids: &[i64],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<CardEpisode>, StoreError> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, CardEpisode>(
            "SELECT e.episode_id, e.run_id, e.kind, e.opened_server_ms AS opened_ms, \
                    e.closed_server_ms AS closed_ms, e.duration_ms, e.censor_left, e.censor_right, e.censor_gap, \
                    e.peak_edge_top AS edge_net, e.peak_edge_top_gross AS edge_gross, \
                    e.peak_pnl_net_usd AS pnl_net, e.peak_pnl_gross_usd AS pnl_gross, e.peak_cost_usd AS cost, \
                    e.pnl_net_budget100_usd AS budget100, e.engine_suspect AS suspect, e.stale_transport AS stale, \
                    (SELECT e.opened_server_ms - max(p.closed_server_ms) FROM episodes p \
                      WHERE p.event_id = e.event_id AND p.kind = e.kind AND p.source = 'live' \
                        AND p.episode_id <> e.episode_id \
                        AND p.closed_server_ms BETWEEN e.opened_server_ms - 1000 AND e.opened_server_ms) AS prev_gap_ms \
             FROM episodes e \
             WHERE e.source = 'live' AND e.run_id = ANY($1) AND e.kind IN ('yes_basket', 'no_basket') \
               AND e.opened_server_ms < $3 AND (e.closed_server_ms IS NULL OR e.closed_server_ms >= $2) \
             ORDER BY e.opened_server_ms, e.episode_id LIMIT $4",
        )
        .bind(run_ids.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_CARD_EPISODES + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_CARD_EPISODES, "card_episodes")
    }

    pub async fn health_latency(
        &self,
        run_ids: &[i64],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<LatencyMinute>, StoreError> {
        let rows = sqlx::query_as::<_, LatencyMinute>(
            "SELECT run_id, minute_ms, lag_p50_ms, rest_rtt_p50_ms FROM health \
             WHERE run_id = ANY($1) AND minute_ms >= $2 AND minute_ms < $3 \
             ORDER BY minute_ms, run_id LIMIT $4",
        )
        .bind(run_ids.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_ROWS, "health_latency")
    }

    /// P0-converter accounts at the default fee: (account_id, scenario), one per scenario.
    pub async fn p0_accounts(&self) -> Result<Vec<(String, String)>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT DISTINCT account_id, scenario FROM paper_accounts \
             WHERE policy = 'P0-converter' AND fee_bips = 0 ORDER BY 1 LIMIT 1000",
        )
        .fetch_all(self.pool())
        .await?)
    }

    /// For each account: its last mark before `from_ms` (any run), then its marks in
    /// [from_ms, to_ms). Sorted by (account_id, t_ms, run_id).
    pub async fn marks_with_prev(
        &self,
        accounts: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<MarkPoint>, StoreError> {
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = sqlx::query_as::<_, MarkPoint>(
            "SELECT m.run_id, m.account_id, m.t_ms, m.equity FROM unnest($1::text[]) AS a(account_id) \
             CROSS JOIN LATERAL (SELECT p.run_id, p.account_id, p.t_ms, p.equity FROM paper_marks p \
                 WHERE p.account_id = a.account_id AND p.t_ms < $2 \
                 ORDER BY p.t_ms DESC, p.run_id DESC LIMIT 1) m",
        )
        .bind(accounts.to_vec())
        .bind(from_ms)
        .fetch_all(self.pool())
        .await?;
        let inside = sqlx::query_as::<_, MarkPoint>(
            "SELECT run_id, account_id, t_ms, equity FROM paper_marks \
             WHERE account_id = ANY($1) AND t_ms >= $2 AND t_ms < $3 LIMIT $4",
        )
        .bind(accounts.to_vec())
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        rows.extend(capped(inside, MAX_ROWS, "marks_with_prev")?);
        rows.sort_by(|a, b| (&a.account_id, a.t_ms, a.run_id).cmp(&(&b.account_id, b.t_ms, b.run_id)));
        Ok(rows)
    }

    /// Stored cells of one family and key with hour_ms in [from_hour, to_hour), oldest first.
    pub async fn card_cells_range(
        &self,
        card: &str,
        premise_key: &str,
        from_hour: i64,
        to_hour: i64,
    ) -> Result<Vec<(i64, String)>, StoreError> {
        let rows = sqlx::query_as(
            "SELECT hour_ms, data::text FROM card_cells \
             WHERE card = $1 AND premise_key = $2 AND hour_ms >= $3 AND hour_ms < $4 \
             ORDER BY hour_ms LIMIT $5",
        )
        .bind(card)
        .bind(premise_key)
        .bind(from_hour)
        .bind(to_hour)
        .bind(MAX_ROWS + 1)
        .fetch_all(self.pool())
        .await?;
        capped(rows, MAX_ROWS, "card_cells_range")
    }

    pub async fn card_cells_at(
        &self,
        card: &str,
        premise_key: &str,
        hours: &[i64],
    ) -> Result<Vec<(i64, String)>, StoreError> {
        if hours.is_empty() {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(
            "SELECT hour_ms, data::text FROM card_cells \
             WHERE card = $1 AND premise_key = $2 AND hour_ms = ANY($3) ORDER BY hour_ms",
        )
        .bind(card)
        .bind(premise_key)
        .bind(hours.to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Upsert cells in one transaction.
    pub async fn card_cells_put(&self, rows: &[CardCellRow]) -> Result<(), StoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await?;
        for chunk in rows.chunks(CELL_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO card_cells (card, premise_key, hour_ms, data, computed_ms) ",
            );
            qb.push_values(chunk, |mut s, r| {
                s.push_bind(clean(&r.card))
                    .push_bind(clean(&r.premise_key))
                    .push_bind(r.hour_ms)
                    .push_bind(r.data.clone())
                    .push_unseparated("::jsonb")
                    .push_bind(r.computed_ms);
            });
            qb.push(
                " ON CONFLICT (card, premise_key, hour_ms) DO UPDATE SET data = EXCLUDED.data, \
                 computed_ms = EXCLUDED.computed_ms",
            );
            qb.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Upsert results and meta rows in one transaction.
    pub async fn card_results_put(
        &self,
        rows: &[CardResultRow],
        meta: &[(String, String)],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await?;
        for chunk in rows.chunks(CELL_CHUNK) {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) ",
            );
            qb.push_values(chunk, |mut s, r| {
                s.push_bind(clean(&r.card))
                    .push_bind(clean(&r.win))
                    .push_bind(clean(&r.premise_key))
                    .push_bind(clean(&r.status))
                    .push_bind(r.body.clone())
                    .push_unseparated("::jsonb")
                    .push_bind(r.computed_ms);
            });
            qb.push(
                " ON CONFLICT (card, win) DO UPDATE SET premise_key = EXCLUDED.premise_key, \
                 status = EXCLUDED.status, body = EXCLUDED.body, computed_ms = EXCLUDED.computed_ms",
            );
            qb.build().execute(&mut *tx).await?;
        }
        for (key, value) in meta {
            sqlx::query(
                "INSERT INTO card_meta (key, value) VALUES ($1, $2::jsonb) \
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            )
            .bind(clean(key))
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn card_meta_put(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.card_results_put(&[], &[(key.to_string(), value.to_string())])
            .await
    }

    pub async fn card_meta_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT value::text FROM card_meta WHERE key = $1")
                .bind(key)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    async fn cards_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT to_regclass('public.card_results') IS NOT NULL \
                AND to_regclass('public.card_meta') IS NOT NULL",
        )
        .fetch_one(self.pool())
        .await?)
    }

    /// The api: (card, body) of one window, by card. Empty before migration 0006.
    pub async fn card_results_for(&self, win: &str) -> Result<Vec<(String, String)>, StoreError> {
        if !self.cards_ready().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(
            "SELECT card, body::text FROM card_results WHERE win = $1 ORDER BY card LIMIT 50",
        )
        .bind(win)
        .fetch_all(self.pool())
        .await?)
    }

    /// The api: every (key, value) of card_meta. Empty before migration 0006.
    pub async fn card_meta_all(&self) -> Result<Vec<(String, String)>, StoreError> {
        if !self.cards_ready().await? {
            return Ok(Vec::new());
        }
        Ok(
            sqlx::query_as("SELECT key, value::text FROM card_meta ORDER BY key LIMIT 100")
                .fetch_all(self.pool())
                .await?,
        )
    }
}
```

- [ ] **Step 5: Wire the module.** In `crates/store/src/lib.rs`, add `mod card_io;` after `mod batch;`, and below the existing `pub use` lines add:

```rust
pub use card_io::{
    CARD_TABLES, CardCellRow, CardEpisode, CardResultRow, ChangedSpan, LatencyMinute, MAX_CARD_EPISODES,
    MAX_CHANGED_PAGE, MarkPoint, PremiseRun,
};
```

- [ ] **Step 6: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p store`
Expected: PASS: the 6 tests in `card_io.rs` and every existing store test; none prints `SKIPPED`. (Postgres returns JSONB as normalized text, hence `{"a": 1}` with a space.)

- [ ] **Step 7: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p store --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/store/src/card_io.rs crates/store/src/lib.rs crates/store/src/error.rs crates/store/tests/card_io.rs && git commit -m "store: analyze reads and writes (premise runs, changed spans, card episodes, Edge inputs, cells, results, meta) and the api's card reads"
```

---

### Task 4: scanner: `run_premises` at run start, the `host` band, and REST round trips in the health row

**Files:**
- Create: `bins/scanner/src/premises.rs`, `bins/scanner/tests/premises.rs`
- Modify: `crates/pm-api/src/http.rs`, `crates/pm-api/src/clob.rs`, `crates/pm-api/src/lib.rs`, `crates/pm-api/Cargo.toml` (dev tokio features), `bins/scanner/src/lib.rs` (`pub mod premises;`), `bins/scanner/src/metrics.rs`, `bins/scanner/src/run.rs`

**Interfaces:**
- Consumes: `arb_core::premises::{PREMISES, effective_version, episode_def_section, censoring_section, sim_section, host_version, valid_host_label}` (Task 1); `store::{RunPremiseRow, HealthRow}`, `Store::insert_run_premises` (Task 2); `scanner::config::Config`, `scanner::paper::Policies` (existing).
- Produces:
  - `pm_api::RttSink = Arc<dyn Fn(i64) + Send + Sync>`; `ClobClient::with_rtt(self, sink: RttSink) -> Self` (every `books`/`ok` request reports its ms from send to response headers, successful responses only; no new requests).
  - `Metrics::push_rtt(&self, ms: i64)`; `minute_row` fills `rest_rtt_p50_ms` / `rest_rtt_p99_ms`.
  - `scanner::premises::{HOST_BAND_MINUTES: u32 = 10, host_label(value: Option<String>) -> anyhow::Result<String>, section(area: &str, cfg: &Config, policies: Option<&Policies>) -> Option<String>, run_rows(run_id: i64, cfg: &Config, policies: Option<&Policies>) -> Vec<RunPremiseRow>, HostBand}` with `HostBand::new(run_id: i64, label: String) -> HostBand` and `HostBand::observe(&mut self, row: &HealthRow) -> Option<RunPremiseRow>`.
  - Env `SCANNER_HOST` (e.g. `vps-kvm2`, `pc-dev`; unset → `unknown`; invalid → the scanner refuses to start).

- [ ] **Step 1: Write the failing pm-api test.** Append to `crates/pm-api/src/http.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn recorder() -> (RttSink, Arc<Mutex<Vec<i64>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sink: RttSink = Arc::new(move |ms| s2.lock().unwrap().push(ms));
        (sink, seen)
    }

    #[tokio::test]
    async fn a_successful_request_reports_its_round_trip_to_the_sink() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        });
        let (sink, seen) = recorder();
        // A plain-http test client: the production client is https-only, the timing is the same code.
        let client = reqwest::Client::builder().build().unwrap();
        let resp = send_timed(client.get(format!("http://{addr}/ok")), Some(&sink))
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let v = seen.lock().unwrap().clone();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!((20..5_000).contains(&v[0]), "{v:?}");
    }

    #[tokio::test]
    async fn a_failed_request_reports_nothing_and_no_sink_is_fine() {
        crate::init_tls();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // nothing listens: connection refused
        let (sink, seen) = recorder();
        let client = reqwest::Client::builder().build().unwrap();
        assert!(
            send_timed(client.get(format!("http://{addr}/ok")), Some(&sink))
                .await
                .is_err()
        );
        assert!(seen.lock().unwrap().is_empty());
        assert!(send_timed(client.get(format!("http://{addr}/ok")), None).await.is_err());
    }
}
```

In `crates/pm-api/Cargo.toml`, change the dev-dependency line to `tokio = { workspace = true, features = ["test-util", "net", "io-util"] }`.

- [ ] **Step 2: Run it to verify it fails.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p pm-api --lib http`
Expected: compile errors `cannot find type RttSink` and `cannot find function send_timed`.

- [ ] **Step 3: Time the requests pm-api already makes.** In `crates/pm-api/src/http.rs`, after `use std::time::Duration;` add:

```rust
use std::sync::Arc;
use std::time::Instant;

/// Where pm-api reports each CLOB request's duration in ms (send → response headers), so the
/// scanner can put p50/p99 in its health row (Phase 5a spec §3). It times requests it already
/// makes and adds none. Failed requests (timeouts, refusals) report nothing.
pub type RttSink = Arc<dyn Fn(i64) + Send + Sync>;

pub(crate) async fn send_timed(
    req: reqwest::RequestBuilder,
    sink: Option<&RttSink>,
) -> Result<reqwest::Response, reqwest::Error> {
    let t = Instant::now();
    let resp = req.send().await;
    if let (Some(s), Ok(_)) = (sink, &resp) {
        s(i64::try_from(t.elapsed().as_millis()).unwrap_or(i64::MAX));
    }
    resp
}
```

In `crates/pm-api/src/lib.rs`, after `pub use error::ApiError;` add `pub use http::RttSink;`.

In `crates/pm-api/src/clob.rs`, replace `pub struct ClobClient { … }` and its `impl` block's `new`, `books` and `ok` with:

```rust
pub struct ClobClient {
    http: reqwest::Client,
    base: String,
    rtt: Option<crate::RttSink>,
}

impl ClobClient {
    pub fn new() -> Result<Self, ApiError> {
        Ok(Self {
            http: crate::http::http_client()?,
            base: CLOB_BASE.to_string(),
            rtt: None,
        })
    }

    /// Report every request's round trip to `sink` (the scanner's health row).
    pub fn with_rtt(mut self, sink: crate::RttSink) -> Self {
        self.rtt = Some(sink);
        self
    }

    pub async fn books(&self, tokens: &[String]) -> Result<Vec<BookWire>, ApiError> {
        let mut out = Vec::new();
        for chunk in chunk_tokens(tokens) {
            let body: Vec<Value> = chunk.iter().map(|t| json!({ "token_id": t })).collect();
            let req = self
                .http
                .post(format!("{}{}", self.base, CLOB_BOOKS))
                .json(&body);
            let text = crate::http::send_timed(req, self.rtt.as_ref())
                .await?
                .error_for_status()?
                .text()
                .await?;
            let mut books: Vec<BookWire> = serde_json::from_str(&text)?;
            out.append(&mut books);
        }
        Ok(out)
    }

    pub async fn ok(&self) -> Result<bool, ApiError> {
        let req = self.http.get(format!("{}{}", self.base, CLOB_OK));
        let resp = crate::http::send_timed(req, self.rtt.as_ref()).await?;
        Ok(resp.status().is_success())
    }
}
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p pm-api --lib`
Expected: PASS, including the two new `http::tests`.

- [ ] **Step 4: Write the failing scanner tests.** In `bins/scanner/src/metrics.rs`'s `mod tests`, add:

```rust
    #[test]
    fn minute_row_reports_rest_round_trip_percentiles_and_drains_them() {
        let m = Metrics::default();
        for v in [40, 10, 30, 20, 1_000] {
            m.push_rtt(v);
        }
        let r = m.minute_row(1, 60_000);
        assert_eq!((r.rest_rtt_p50_ms, r.rest_rtt_p99_ms), (Some(30), Some(40)));
        let r2 = m.minute_row(1, 120_000);
        assert_eq!((r2.rest_rtt_p50_ms, r2.rest_rtt_p99_ms), (None, None));
    }
```

(Sorted index rule, `percentile`: p99 of 5 values is index `4 · 99 / 100 = 3` → 40.)

Create `bins/scanner/tests/premises.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::{PREMISES, code_version, effective_version};
use scanner::config::Config;
use scanner::paper::parse_policies;
use scanner::premises::{HOST_BAND_MINUTES, HostBand, host_label, run_rows, section};
use std::path::PathBuf;
use store::HealthRow;

fn repo(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
}

fn repo_config() -> Config {
    Config::load(&repo("config/scanner.toml")).unwrap().0
}

fn minute(lag: Option<i32>, rtt: Option<i32>) -> HealthRow {
    HealthRow {
        run_id: 7, minute_ms: 0, tokens: 0, shards: 0, msgs: 0, bytes: 0, drops: 0, reconnects: 0,
        rest_p50_ms: None, rest_p99_ms: None, lag_p50_ms: lag, lag_p99_ms: None, clock_offset_ms: None,
        ok: None, capture_paused: false, parked_shards: 0, dropped_groups: 0,
        rest_rtt_p50_ms: rtt, rest_rtt_p99_ms: None,
    }
}

#[test]
fn run_rows_cover_every_area_but_host_with_config_hashed_versions() {
    let cfg = repo_config();
    let text = std::fs::read_to_string(repo("config/policies.toml")).unwrap();
    let pol = parse_policies(&text).unwrap();
    let rows = run_rows(9, &cfg, Some(&pol));
    let areas: Vec<&str> = rows.iter().map(|r| r.area.as_str()).collect();
    assert_eq!(
        areas,
        ["censoring", "depth", "detector_no", "detector_yes", "episode_def", "fees", "sim"]
    );
    assert!(rows.iter().all(|r| r.run_id == 9));
    let censoring = rows.iter().find(|r| r.area == "censoring").unwrap();
    assert_eq!(censoring.version, effective_version("1", "gap_ms=30000"));
    let sim = rows.iter().find(|r| r.area == "sim").unwrap();
    assert_eq!(sim.version, effective_version("1", &format!("policies_sha256={}", pol.sha256)));
    // paper trading off: no sim row, so Edge capture never counts the run
    assert!(run_rows(9, &cfg, None).iter().all(|r| r.area != "sim"));
}

#[test]
fn the_backfill_literals_in_0006_are_todays_premises() {
    let sql = std::fs::read_to_string(repo("crates/store/migrations/0006_cards.sql")).unwrap();
    let block = sql
        .split("-- backfill:begin")
        .nth(1)
        .and_then(|s| s.split("-- backfill:end").next())
        .expect("backfill markers");
    let tuples: Vec<(String, String, String)> = block
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("('"))
        .map(|l| {
            let inner = l.trim_end_matches(',').trim_start_matches("('").trim_end_matches("')");
            let parts: Vec<&str> = inner.split("', '").collect();
            assert_eq!(parts.len(), 3, "{l}");
            (parts[0].to_string(), parts[1].to_string(), parts[2].to_string())
        })
        .collect();
    let cfg = repo_config();
    let expected: Vec<(String, String, String)> = PREMISES
        .iter()
        .filter(|p| p.0 != "sim" && p.0 != "host")
        .map(|p| {
            (
                p.0.to_string(),
                code_version(p.0).unwrap().to_string(),
                section(p.0, &cfg, None).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        tuples, expected,
        "0006's backfill must name exactly today's premises (a version bump after 0006 needs no backfill change: \
         backfilled runs simply stop matching the new key)"
    );
}

#[test]
fn host_labels_default_to_unknown_and_reject_bad_input() {
    assert_eq!(host_label(None).unwrap(), "unknown");
    assert_eq!(host_label(Some(String::new())).unwrap(), "unknown");
    assert_eq!(host_label(Some("vps-kvm2".into())).unwrap(), "vps-kvm2");
    assert!(host_label(Some("VPS KVM2".into())).is_err());
    assert!(host_label(Some("a|b".into())).is_err());
}

#[test]
fn the_host_row_comes_once_after_ten_minutes_with_the_median_band() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    // minutes 1..=9: no row yet; lag + rtt/2 = 80+65=145, one minute unmeasured
    for i in 1..HOST_BAND_MINUTES {
        let row = if i == 3 { minute(Some(80), None) } else { minute(Some(80), Some(130)) };
        assert_eq!(band.observe(&row), None, "minute {i}");
    }
    let host = band.observe(&minute(Some(10), Some(10))).unwrap();
    assert_eq!((host.run_id, host.area.as_str()), (7, "host"));
    assert_eq!(host.version, "1:vps-kvm2|100-250");
    assert_eq!(band.observe(&minute(Some(1), Some(1))), None, "written once");
    let mut quiet = HostBand::new(8, "pc-dev".into());
    let mut last = None;
    for _ in 0..HOST_BAND_MINUTES {
        last = quiet.observe(&minute(None, None));
    }
    assert_eq!(last.unwrap().version, "1:pc-dev|unmeasured");
}
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p scanner --lib metrics && cargo test -p scanner --test premises`
Expected: compile errors `no method named push_rtt` and `could not find premises in scanner`.

- [ ] **Step 5: Metrics.** In `bins/scanner/src/metrics.rs`: add the field `pub rtt_ms: Mutex<Vec<i64>>,` after `pub rest_ms: Mutex<Vec<i64>>,`; add to `impl Metrics`:

```rust
    /// One CLOB request's round trip (pm-api's RttSink).
    pub fn push_rtt(&self, v: i64) {
        self.rtt_ms.lock().unwrap_or_else(|p| p.into_inner()).push(v);
    }
```

In `minute_row`, add `let rtt = take_sorted(&self.rtt_ms);` after `let rest = take_sorted(&self.rest_ms);`, and replace the two `None` placeholders from Task 2 with `rest_rtt_p50_ms: p(&rtt, 50),` and `rest_rtt_p99_ms: p(&rtt, 99),`.

- [ ] **Step 6: The premises module.** Create `bins/scanner/src/premises.rs`:

```rust
//! Run premises (Phase 5a spec §4): what this run's episodes are computed under. Every area but
//! `host` is written at run start; `host` (SCANNER_HOST + measured latency band) is written as its
//! own row once the run's first HOST_BAND_MINUTES health minutes exist, so `run_premises` stays
//! append-only. A run that dies earlier has no `host` row and never counts for Edge capture.
use crate::config::Config;
use crate::paper::Policies;
use arb_core::premises::{self, PREMISES};
use store::{HealthRow, RunPremiseRow};

pub const HOST_BAND_MINUTES: u32 = 10;

/// `SCANNER_HOST` (e.g. `vps-kvm2`, `pc-dev`); `unknown` when unset or empty.
pub fn host_label(value: Option<String>) -> anyhow::Result<String> {
    match value.filter(|v| !v.is_empty()) {
        None => Ok("unknown".to_string()),
        Some(v) if premises::valid_host_label(&v) => Ok(v),
        Some(_) => anyhow::bail!("SCANNER_HOST must be 1 to 32 characters of a-z, 0-9 and '-'"),
    }
}

/// The config section an area's effective version hashes (spec §4). `sim` exists only with paper
/// trading on; `host` is not config-versioned (see HostBand).
pub fn section(area: &str, cfg: &Config, policies: Option<&Policies>) -> Option<String> {
    match area {
        "episode_def" => Some(premises::episode_def_section(
            cfg.engine.stale_lag_ms,
            cfg.engine.suspect_window_ms,
            cfg.engine.l1_suspect_window_ms,
            cfg.engine.drift_settle_ms,
        )),
        "censoring" => Some(premises::censoring_section(cfg.engine.gap_ms)),
        "sim" => policies.map(|p| premises::sim_section(&p.sha256)),
        "host" => None,
        _ => Some(String::new()),
    }
}

/// The rows written at run start: every area but `host`.
pub fn run_rows(run_id: i64, cfg: &Config, policies: Option<&Policies>) -> Vec<RunPremiseRow> {
    PREMISES
        .iter()
        .filter_map(|(area, code, _)| {
            section(area, cfg, policies).map(|s| RunPremiseRow {
                run_id,
                area: (*area).to_string(),
                version: premises::effective_version(code, &s),
            })
        })
        .collect()
}

/// Collects the first HOST_BAND_MINUTES health minutes' latency estimates (feed lag p50 +
/// REST RTT p50 / 2, minutes with both only) and yields the `host` row once, at the last of them.
#[derive(Debug)]
pub struct HostBand {
    run_id: i64,
    label: String,
    minutes: u32,
    samples: Vec<i64>,
    done: bool,
}

impl HostBand {
    pub fn new(run_id: i64, label: String) -> Self {
        Self {
            run_id,
            label,
            minutes: 0,
            samples: Vec::new(),
            done: false,
        }
    }

    pub fn observe(&mut self, row: &HealthRow) -> Option<RunPremiseRow> {
        if self.done {
            return None;
        }
        self.minutes += 1;
        if let (Some(lag), Some(rtt)) = (row.lag_p50_ms, row.rest_rtt_p50_ms) {
            self.samples.push(i64::from(lag) + i64::from(rtt) / 2);
        }
        if self.minutes < HOST_BAND_MINUTES {
            return None;
        }
        self.done = true;
        self.samples.sort_unstable();
        let median = match self.samples.len() {
            0 => None,
            n => self.samples.get((n - 1) / 2).copied(),
        };
        Some(RunPremiseRow {
            run_id: self.run_id,
            area: "host".to_string(),
            version: premises::host_version(&self.label, median),
        })
    }
}
```

In `bins/scanner/src/lib.rs`, add `pub mod premises;` after `pub mod paper;`.

(In the band test, nine minutes give eight samples of 145 plus one of 15: the lower median of nine is 145 → `100-250`.)

- [ ] **Step 7: Wire it into the live run.** In `bins/scanner/src/run.rs`:

1. Add `use pm_api::RttSink;` to the imports.
2. In `pub async fn run(…)`, replace the three lines from `let run_id = store.start_run(&new_run(&cfg_hash, "live")).await?;` through `let result = run_live(&cfg, &cfg_hash, policies.as_ref(), &store, run_id).await;` with:

```rust
    let host = crate::premises::host_label(std::env::var("SCANNER_HOST").ok())?;
    let run_id = store.start_run(&new_run(&cfg_hash, "live")).await?;
    // Premises first (spec §4): a run without them never counts on a card.
    let premises = crate::premises::run_rows(run_id, &cfg, policies.as_ref());
    let result = match store.insert_run_premises(&premises).await {
        Ok(()) => run_live(&cfg, &cfg_hash, policies.as_ref(), &store, run_id, host).await,
        Err(e) => Err(anyhow::Error::from(e).context("record run premises")),
    };
```

3. Change `run_live`'s signature to add `host: String,` as the last parameter.
4. Replace `let clob = Arc::new(ClobClient::new()?);` with:

```rust
    let rtt: RttSink = {
        let m = metrics.clone();
        Arc::new(move |ms| m.push_rtt(ms))
    };
    let clob = Arc::new(ClobClient::new()?.with_rtt(rtt));
```

5. In the `health_task` block, replace `let (metrics, writer_tx) = (metrics.clone(), writer_tx.clone());` with:

```rust
        let (metrics, writer_tx, store) = (metrics.clone(), writer_tx.clone(), store.clone());
        let mut band = crate::premises::HostBand::new(run_id, host);
```

and replace the block

```rust
                if writer_tx
                    .send(WriterMsg::Health(metrics.minute_row(run_id, minute)))
                    .await
                    .is_err()
                {
                    return;
                }
```

with

```rust
                let row = metrics.minute_row(run_id, minute);
                let host_row = band.observe(&row);
                if writer_tx.send(WriterMsg::Health(row)).await.is_err() {
                    return;
                }
                if let Some(h) = host_row
                    && let Err(e) = store.insert_run_premises(&[h]).await
                {
                    eprintln!("scanner: host premise not recorded (Edge capture skips this run): {e}");
                }
```

- [ ] **Step 8: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p pm-api --lib && cargo test -p scanner`
Expected: PASS: the metrics test, the 4 tests in `tests/premises.rs`, and every existing scanner test (the ignored benches stay ignored).

- [ ] **Step 9: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 10: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/pm-api/src/http.rs crates/pm-api/src/clob.rs crates/pm-api/src/lib.rs crates/pm-api/Cargo.toml bins/scanner/src/premises.rs bins/scanner/src/lib.rs bins/scanner/src/metrics.rs bins/scanner/src/run.rs bins/scanner/tests/premises.rs Cargo.lock && git commit -m "scanner: run premises at start, host band after 10 minutes (SCANNER_HOST), CLOB round trips in the health row"
```

---

### Task 5: `crates/cards`: exact statistics (Kaplan–Meier with ties and censoring, empirical S, medians, p90, bins)

**Files:**
- Create: `crates/cards/Cargo.toml`, `crates/cards/src/lib.rs`, `crates/cards/src/stats.rs`, `crates/cards/tests/stats.rs`, `crates/cards/tests/no_floats.rs`
- Modify: `Cargo.toml` (workspace `members` += `"crates/cards"`)

**Interfaces:**
- Consumes: nothing from earlier tasks at runtime; the tests cross-check against `arb_core::stats::kaplan_meier` (existing).
- Produces (`cards::`):
  - `HOUR_MS: i64 = 3_600_000`, `DAY_MS: i64 = 86_400_000`, `fn hour_of(ms: i64) -> i64`
  - `stats::{DurMap = BTreeMap<i64, (i64, i64)>, CountMap = BTreeMap<i64, i64>, FP_ONE: i128, FP_SCALE: u32 = 28}`
  - `stats::add_counts(into: &mut CountMap, other: &CountMap, sign: i64)`, `stats::add_durs(into: &mut DurMap, other: &DurMap, sign: i64)`, `stats::total(m: &CountMap) -> i64`, `stats::dur_total(m: &DurMap) -> (i64, i64)` (events, censored)
  - `stats::merge_sorted<V: Copy>(a: &BTreeMap<i64, V>, b: &BTreeMap<i64, V>, plus: fn(V, V) -> V) -> impl Iterator<Item = (i64, V)>`
  - `stats::KmSummary { at: Vec<i128>, median: Option<i64>, events: i64, censored: i64 }`; `stats::km(entries: impl Iterator<Item = (i64, (i64, i64))> + Clone, grid: &[i64]) -> KmSummary`
  - `stats::fp_to_dec(v: i128) -> Decimal`
  - `stats::emp_at(m: &CountMap, grid: &[i64]) -> Vec<Option<Decimal>>`
  - `stats::median_of(entries: impl Iterator<Item = (i64, i64)>, n: i64) -> Option<Decimal>` (sample median), `stats::nearest_rank(entries: impl Iterator<Item = (i64, i64)>, n: i64, pct: i64) -> Option<i64>`
  - `stats::bins(m: &CountMap, width: i64, top: i64) -> (i64, Vec<(i64, i64)>)` (count below 0, then `(lo, count)` per bin, the last bin open from `top`)
  - `stats::centi_bps(edge: Decimal) -> i64` (edge as a fraction → 0.01 bp units, half-even), `stats::secs(ms: Decimal) -> String` (2 dp), `stats::pct(num: i64, den: i64) -> Option<Decimal>` (1 dp), `stats::dec(d: Decimal) -> String` (normalized; `-0` → `"0"`)

- [ ] **Step 1: The crate skeleton.** In the root `Cargo.toml`, change `members` to `["crates/arb-core", "crates/pm-api", "crates/store", "crates/sim", "crates/cards", "spikes/probe", "bins/scanner", "bins/api"]`. Create `crates/cards/Cargo.toml`:

```toml
[package]
name = "cards"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# Pure: no IO, no clock, no floats (Phase 5a spec §7, §9). analyze does the IO.
[dependencies]
arb-core = { path = "../arb-core" }
rust_decimal = { workspace = true, features = ["serde-with-str"] }
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true

[dev-dependencies]
rust_decimal_macros.workspace = true
proptest.workspace = true

[lints]
workspace = true
```

Create `crates/cards/src/lib.rs`:

```rust
//! cards: the pure maths of the Phase 5a live cards (spec §5–§7). Hourly merge-exact cells, the
//! incremental windows over them, and the card bodies the api serves. No IO, no clock, no floats:
//! `bins/analyze` reads Postgres through `store` and calls into this crate.
#![forbid(unsafe_code)]

pub mod stats;

pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;

/// The UTC hour `ms` falls in.
pub fn hour_of(ms: i64) -> i64 {
    ms.div_euclid(HOUR_MS) * HOUR_MS
}
```

Create `crates/cards/tests/no_floats.rs` as an exact copy of `crates/arb-core/tests/no_floats.rs`, with its first line changed to `//! Phase 5a: cards must never use floating point (same scan as arb-core).`

- [ ] **Step 2: Write the failing tests.** Create `crates/cards/tests/stats.rs`:

```rust
//! Oracle: the 6-MP arm of the Freireich et al. (1963) leukemia remission trial (Blood 21:699),
//! remission in weeks, + = right-censored, as tabulated in Kleinbaum & Klein, *Survival Analysis:
//! A Self-Learning Text* (3rd ed., 2012), ch. 2, and in Klein & Moeschberger, *Survival Analysis*
//! (2nd ed., 2003), §1.2 / Table 4.1A. It has tied events (three at 6), a censored time tied with
//! events (6+, 10+) and heavy censoring. Published KM: 0.857, 0.807, 0.753, 0.690, 0.627, 0.538,
//! 0.448 at 6, 7, 10, 13, 16, 22, 23; median 23. Exact values: 6/7, 96/119, 64/85, 176/255, 32/51,
//! 64/119, 160/357. The placebo arm (no censoring) has median 8.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::stats::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

fn durmap(obs: &[(i64, bool)]) -> DurMap {
    let mut m = DurMap::new();
    for (t, event) in obs {
        let e = m.entry(*t).or_insert((0, 0));
        if *event { e.0 += 1 } else { e.1 += 1 }
    }
    m
}

fn six_mp() -> Vec<(i64, bool)> {
    let events = [6, 6, 6, 7, 10, 13, 16, 22, 23];
    let censored = [6, 9, 10, 11, 17, 19, 20, 25, 32, 32, 34, 35];
    events.iter().map(|t| (*t, true)).chain(censored.iter().map(|t| (*t, false))).collect()
}

fn counts(vals: &[i64]) -> CountMap {
    let mut m = CountMap::new();
    for v in vals {
        *m.entry(*v).or_insert(0) += 1;
    }
    m
}

#[test]
fn kaplan_meier_matches_the_6mp_leukemia_textbook_table() {
    let m = durmap(&six_mp());
    assert_eq!(dur_total(&m), (9, 12));
    let grid = [5, 6, 7, 10, 13, 16, 22, 23, 35];
    let s = km(m.iter().map(|(t, v)| (*t, *v)), &grid);
    let got: Vec<Decimal> = s.at.iter().map(|v| fp_to_dec(*v).round_dp(4)).collect();
    assert_eq!(
        got,
        vec![dec!(1), dec!(0.8571), dec!(0.8067), dec!(0.7529), dec!(0.6902), dec!(0.6275), dec!(0.5378), dec!(0.4482), dec!(0.4482)]
    );
    // exact to 20 places against the fractions
    assert_eq!(fp_to_dec(s.at[1]).round_dp(20), (dec!(6) / dec!(7)).round_dp(20));
    assert_eq!(fp_to_dec(s.at[7]).round_dp(20), (dec!(160) / dec!(357)).round_dp(20));
    assert_eq!(s.median, Some(23));
    assert_eq!((s.events, s.censored), (9, 12));
}

#[test]
fn km_median_without_censoring_and_not_reached() {
    let placebo = [1, 1, 2, 2, 3, 4, 4, 5, 5, 8, 8, 8, 8, 11, 11, 12, 12, 15, 17, 22, 23];
    let m = durmap(&placebo.iter().map(|t| (*t, true)).collect::<Vec<_>>());
    assert_eq!(km(m.iter().map(|(t, v)| (*t, *v)), &[]).median, Some(8));
    assert_eq!(median_of(counts(&placebo).into_iter(), 21), Some(dec!(8)));
    // most censored early: S never falls to 1/2
    let m = durmap(&[(1, true), (2, false), (3, false), (4, false)]);
    let s = km(m.iter().map(|(t, v)| (*t, *v)), &[10]);
    assert_eq!(s.median, None);
    assert_eq!(fp_to_dec(s.at[0]), dec!(0.75));
    // empty input: S = 1 everywhere, no median, no division
    let empty = DurMap::new();
    let s = km(empty.iter().map(|(t, v)| (*t, *v)), &[1, 2]);
    assert_eq!((s.at, s.median), (vec![FP_ONE, FP_ONE], None));
}

#[test]
fn empirical_survival_is_the_share_strictly_greater() {
    let m = counts(&[1, 2, 2, 3]);
    assert_eq!(emp_at(&m, &[0, 1, 2, 3]), vec![Some(dec!(1)), Some(dec!(0.75)), Some(dec!(0.25)), Some(dec!(0))]);
    assert_eq!(emp_at(&CountMap::new(), &[1]), vec![None]);
}

#[test]
fn sample_median_averages_the_middle_pair_and_nearest_rank_is_ceil() {
    assert_eq!(median_of(counts(&[1, 2, 3, 4]).into_iter(), 4), Some(dec!(2.5)));
    assert_eq!(median_of(counts(&[5, 1, 3]).into_iter(), 3), Some(dec!(3)));
    assert_eq!(median_of(CountMap::new().into_iter(), 0), None);
    let ten = counts(&(1..=10).collect::<Vec<_>>());
    assert_eq!(nearest_rank(ten.clone().into_iter(), 10, 90), Some(9));
    assert_eq!(nearest_rank(ten.clone().into_iter(), 10, 50), Some(5));
    assert_eq!(nearest_rank(ten.into_iter(), 10, 100), Some(10));
    assert_eq!(nearest_rank(counts(&[7]).into_iter(), 1, 90), Some(7));
    assert_eq!(nearest_rank(CountMap::new().into_iter(), 0, 90), None);
}

#[test]
fn bins_are_fixed_width_with_an_open_top_and_negatives_apart() {
    // centi-bps: 0.0049 → 4900 (49 bps)
    assert_eq!(centi_bps(dec!(0.0049)), 4_900);
    assert_eq!(centi_bps(dec!(0.000012345)), 12); // 0.12345 bp → 0.12 (half-even)
    assert_eq!(centi_bps(dec!(0.0000125)), 12); // 0.125 bp → 0.12 (tie to even)
    assert_eq!(centi_bps(dec!(-0.001)), -1_000);
    let m = counts(&[-5, 0, 2_499, 2_500, 4_900, 49_999, 50_000, 123_456]);
    let (below, b) = bins(&m, 2_500, 50_000);
    assert_eq!(below, 1);
    assert_eq!(b.len(), 21);
    assert_eq!(b[0], (0, 2));
    assert_eq!(b[1], (2_500, 2));
    assert_eq!(b[19], (47_500, 1));
    assert_eq!(b[20], (50_000, 2));
    assert_eq!(b.iter().map(|x| x.1).sum::<i64>() + below, 8);
}

#[test]
fn count_maps_add_and_subtract_back_to_empty() {
    let a = counts(&[1, 1, 5]);
    let mut m = CountMap::new();
    add_counts(&mut m, &a, 1);
    add_counts(&mut m, &a, 1);
    add_counts(&mut m, &a, -1);
    assert_eq!(m, a);
    add_counts(&mut m, &a, -1);
    assert!(m.is_empty(), "zero entries are removed: {m:?}");
    let d = durmap(&[(3, true), (3, false), (4, true)]);
    let mut dm = DurMap::new();
    add_durs(&mut dm, &d, 1);
    add_durs(&mut dm, &d, -1);
    assert!(dm.is_empty());
    let merged: Vec<(i64, i64)> = merge_sorted(&counts(&[1, 3, 3]), &counts(&[2, 3]), |x, y| x + y).collect();
    assert_eq!(merged, vec![(1, 1), (2, 1), (3, 3)]);
    assert_eq!(total(&counts(&[1, 3, 3])), 3);
}

#[test]
fn formatting_helpers_are_exact() {
    assert_eq!(secs(dec!(16150)), "16.15");
    assert_eq!(secs(dec!(7990)), "7.99");
    assert_eq!(secs(dec!(1000)), "1");
    assert_eq!(secs(dec!(4194.5)), "4.19");
    assert_eq!(pct(366, 624), Some(dec!(58.7)));
    assert_eq!(pct(1, 0), None);
    assert_eq!(dec(dec!(-0.000)), "0");
    assert_eq!(dec(dec!(0.4100)), "0.41");
    assert_eq!(cards::hour_of(3_600_000 * 5 + 1), 3_600_000 * 5);
    assert_eq!(cards::hour_of(-1), -3_600_000);
}

proptest! {
    #[test]
    fn km_agrees_with_arb_core_to_12_places(obs in proptest::collection::vec((0i64..500, any::<bool>()), 0..300)) {
        let m = durmap(&obs);
        let times: Vec<i64> = m.keys().copied().collect();
        let ours = km(m.iter().map(|(t, v)| (*t, *v)), &times);
        let theirs: BTreeMap<i64, Decimal> = arb_core::stats::kaplan_meier(&obs).into_iter().collect();
        let mut s_prev = Decimal::ONE;
        for (i, t) in times.iter().enumerate() {
            let expect = theirs.get(t).copied().unwrap_or(s_prev);
            prop_assert_eq!(fp_to_dec(ours.at[i]).round_dp(12), expect.round_dp(12), "t = {}", t);
            s_prev = expect;
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards`
Expected: compile errors `could not find stats in cards`-style (`unresolved import cards::stats`) because `src/stats.rs` does not exist yet.

- [ ] **Step 4: Write the module.** Create `crates/cards/src/stats.rs`:

```rust
//! Exact statistics over integer count maps (spec §7: durations as `duration_ms → (events,
//! censored)` count maps, so Kaplan–Meier is exact and hourly cells merge by addition). Survival
//! probabilities use an i128 fixed point (1.0 = 10²⁸, Decimal's largest scale): one multiply and
//! one divide per event time, fast even in a debug build, truncation ≤ 10⁻²⁸ per step.
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use std::collections::BTreeMap;

/// duration_ms → (events, censored). Counts are i64 so windows can subtract.
pub type DurMap = BTreeMap<i64, (i64, i64)>;
/// value → count.
pub type CountMap = BTreeMap<i64, i64>;

pub const FP_SCALE: u32 = 28;
pub const FP_ONE: i128 = 10_000_000_000_000_000_000_000_000_000;

pub fn add_counts(into: &mut CountMap, other: &CountMap, sign: i64) {
    for (k, v) in other {
        let e = into.entry(*k).or_insert(0);
        *e += sign * v;
        if *e == 0 {
            into.remove(k);
        }
    }
}

pub fn add_durs(into: &mut DurMap, other: &DurMap, sign: i64) {
    for (k, (ev, ce)) in other {
        let e = into.entry(*k).or_insert((0, 0));
        e.0 += sign * ev;
        e.1 += sign * ce;
        if *e == (0, 0) {
            into.remove(k);
        }
    }
}

pub fn total(m: &CountMap) -> i64 {
    m.values().sum()
}

/// (events, censored)
pub fn dur_total(m: &DurMap) -> (i64, i64) {
    m.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1))
}

/// Two sorted maps as one sorted stream, equal keys combined with `plus`. Collected into one
/// Vec so the stream is `Clone` (Kaplan–Meier walks it twice).
pub fn merge_sorted<'a, V: Copy + 'a>(
    a: &'a BTreeMap<i64, V>,
    b: &'a BTreeMap<i64, V>,
    plus: fn(V, V) -> V,
) -> impl Iterator<Item = (i64, V)> + Clone + 'a {
    let mut ia = a.iter().peekable();
    let mut ib = b.iter().peekable();
    let mut out: Vec<(i64, V)> = Vec::new();
    loop {
        let ka = ia.peek().map(|(k, _)| **k);
        let kb = ib.peek().map(|(k, _)| **k);
        match (ka, kb) {
            (None, None) => break,
            (Some(x), Some(y)) if x == y => {
                if let (Some((k, va)), Some((_, vb))) = (ia.next(), ib.next()) {
                    out.push((*k, plus(*va, *vb)));
                }
            }
            (Some(x), Some(y)) if x < y => {
                if let Some((k, v)) = ia.next() {
                    out.push((*k, *v));
                }
            }
            (Some(_), None) => {
                if let Some((k, v)) = ia.next() {
                    out.push((*k, *v));
                }
            }
            _ => {
                if let Some((k, v)) = ib.next() {
                    out.push((*k, *v));
                }
            }
        }
    }
    out.into_iter()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KmSummary {
    /// Ŝ(τ) for each grid point, fixed point (FP_ONE = 1).
    pub at: Vec<i128>,
    /// The smallest event time with Ŝ ≤ 1/2 (None: not reached).
    pub median: Option<i64>,
    pub events: i64,
    pub censored: i64,
}

/// Kaplan–Meier, Ŝ(τ) = Π_{t_j ≤ τ} (1 − d_j / n_j), over `(t, (events, censored))` in ascending
/// t (a censored time tied with events stays at risk at that time, the textbook convention).
/// `grid` must be ascending.
pub fn km<I>(entries: I, grid: &[i64]) -> KmSummary
where
    I: Iterator<Item = (i64, (i64, i64))> + Clone,
{
    let (events, censored) = entries.clone().fold((0i64, 0i64), |a, (_, v)| (a.0 + v.0, a.1 + v.1));
    let mut at_risk = events + censored;
    let mut s = FP_ONE;
    let mut at = Vec::with_capacity(grid.len());
    let mut gi = 0usize;
    let mut median = None;
    for (t, (e, c)) in entries {
        while gi < grid.len() && grid.get(gi).is_some_and(|g| *g < t) {
            at.push(s);
            gi += 1;
        }
        if e > 0 && at_risk > 0 {
            s = s
                .checked_mul(i128::from(at_risk - e))
                .map_or(0, |x| x / i128::from(at_risk));
            if median.is_none() && s.saturating_mul(2) <= FP_ONE {
                median = Some(t);
            }
        }
        at_risk -= e + c;
    }
    while gi < grid.len() {
        at.push(s);
        gi += 1;
    }
    KmSummary { at, median, events, censored }
}

pub fn fp_to_dec(v: i128) -> Decimal {
    Decimal::try_from_i128_with_scale(v, FP_SCALE).unwrap_or(Decimal::ZERO)
}

/// Gebele's empirical Ŝ(τ) = K⁻¹ Σ 𝟙{D_k > τ} over uncensored durations; None when K = 0.
pub fn emp_at(m: &CountMap, grid: &[i64]) -> Vec<Option<Decimal>> {
    let n = total(m);
    if n <= 0 {
        return vec![None; grid.len()];
    }
    grid.iter()
        .map(|tau| {
            let above: i64 = m.range(tau.saturating_add(1)..).map(|(_, c)| c).sum();
            Some(Decimal::from(above) / Decimal::from(n))
        })
        .collect()
}

/// The sample median of `n` values given as ascending (value, count): the mean of the middle
/// pair when n is even.
pub fn median_of<I: Iterator<Item = (i64, i64)>>(entries: I, n: i64) -> Option<Decimal> {
    if n <= 0 {
        return None;
    }
    let (klo, khi) = ((n - 1) / 2, n / 2);
    let mut lo = None;
    let mut seen = 0i64;
    for (v, c) in entries {
        seen += c;
        if lo.is_none() && seen > klo {
            lo = Some(v);
        }
        if seen > khi {
            return lo.map(|l| (Decimal::from(l) + Decimal::from(v)) / Decimal::from(2));
        }
    }
    None
}

/// Nearest-rank percentile: the smallest value whose cumulative count reaches ⌈pct·n/100⌉.
pub fn nearest_rank<I: Iterator<Item = (i64, i64)>>(entries: I, n: i64, pct: i64) -> Option<i64> {
    if n <= 0 {
        return None;
    }
    let rank = ((pct * n + 99) / 100).max(1);
    let mut seen = 0i64;
    for (v, c) in entries {
        seen += c;
        if seen >= rank {
            return Some(v);
        }
    }
    None
}

/// Fixed-width bins: (count below 0, [(lo, count)] for lo = 0, width, …, top − width, then the
/// open bin from `top`).
pub fn bins(m: &CountMap, width: i64, top: i64) -> (i64, Vec<(i64, i64)>) {
    let n_bins = usize::try_from(top / width).unwrap_or(0);
    let mut out: Vec<(i64, i64)> = (0..=n_bins)
        .map(|i| (i64::try_from(i).unwrap_or(0) * width, 0))
        .collect();
    let mut below = 0;
    for (v, c) in m {
        if *v < 0 {
            below += c;
            continue;
        }
        let i = usize::try_from((*v).min(top) / width).unwrap_or(0).min(n_bins);
        if let Some(slot) = out.get_mut(i) {
            slot.1 += c;
        }
    }
    (below, out)
}

/// An edge given as a fraction of $1 (0.0049) in 0.01-bp units (4900), rounded half-even.
pub fn centi_bps(edge: Decimal) -> i64 {
    (edge * Decimal::from(1_000_000))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        .to_i64()
        .unwrap_or(i64::MAX)
}

/// ms → seconds with at most 2 decimals, as a decimal string ("16.15").
pub fn secs(ms: Decimal) -> String {
    dec((ms / Decimal::from(1_000)).round_dp(2))
}

/// num / den as a percentage with 1 decimal; None when den is 0.
pub fn pct(num: i64, den: i64) -> Option<Decimal> {
    if den == 0 {
        return None;
    }
    Some((Decimal::from(num) * Decimal::from(100) / Decimal::from(den)).round_dp(1))
}

/// The api's decimal string: normalized (no trailing zeros), `-0` → `"0"`.
pub fn dec(d: Decimal) -> String {
    let n = d.normalize();
    if n.is_zero() { "0".to_string() } else { n.to_string() }
}
```

In `crates/cards/src/lib.rs`, the `pub mod stats;` line is already there.


- [ ] **Step 5: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards`
Expected: PASS: 8 tests in `stats.rs` (including the proptest) and 2 in `no_floats.rs`.

- [ ] **Step 6: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p cards --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add Cargo.toml Cargo.lock crates/cards/Cargo.toml crates/cards/src/lib.rs crates/cards/src/stats.rs crates/cards/tests/stats.rs crates/cards/tests/no_floats.rs && git commit -m "cards: exact statistics (Kaplan-Meier on the 6-MP oracle, empirical S, medians, nearest-rank p90, bins)"
```

---

### Task 6: `cards`: hourly merge-exact cells for the Gebele cards, and the incremental windows

**Files:**
- Create: `crates/cards/src/cells.rs`, `crates/cards/src/window.rs`, `crates/cards/tests/cells.rs`, `crates/cards/tests/window_property.rs`, `crates/cards/tests/merge_property.rs`
- Modify: `crates/cards/src/lib.rs` (`pub mod cells; pub mod window;`)

**Interfaces:**
- Consumes: `cards::stats::*`, `cards::{HOUR_MS, hour_of}` (Task 5).
- Produces (`cards::cells`):
  - consts `MERGE_GAPS_MS: [i64; 3] = [0, 250, 1_000]`, `LONG_MS: i64 = 3_000_000`, `EDGE_MINUTES_MS: i64 = 300_000`
  - `enum Side { Yes, No }` with `Side::from_kind(kind: &str) -> Option<Side>` (`yes_basket`, `no_basket`)
  - `struct Num(pub Decimal)` — a decimal serialized as a JSON string (`Copy`, `Default`, `Ord`)
  - `struct PerSide<T> { yes: T, no: T }` with `get(&self, Side) -> &T`, `get_mut(&mut self, Side) -> &mut T`
  - `struct Ep { id: String, run_id: i64, side: Side, opened_ms: i64, closed_ms: Option<i64>, end_ms: i64, duration_ms: Option<i64>, censor_left: bool, censor_right: bool, censor_gap: bool, edge_net: Decimal, edge_gross: Decimal, pnl_net: Decimal, pnl_gross: Decimal, cost: Decimal, budget100: Option<Decimal>, suspect: bool, stale: bool, prev_gap_ms: Option<i64> }` with `positive()`, `flagged()`, `uncensored()`
  - `struct HourInput<'a> { hour_ms: i64, episodes: &'a [Ep], latency_ms: &'a [i64], equity_deltas: &'a [(String, Decimal)] }`
  - `fn opened_in(eps: &[Ep], hour_ms: i64) -> impl Iterator<Item = &Ep>`, `fn overlap(e: &Ep, hour_ms: i64) -> Option<(i64, i64)>`
  - `trait CellData: Clone + Default + PartialEq + Debug + Serialize + DeserializeOwned { const FAMILY: &'static str; const AREAS: &'static [&'static str]; const EDGE_INPUTS: bool = false; type Max: Clone + Default + PartialEq + Debug; fn build(inp: &HourInput<'_>) -> Self; fn add(&mut self, o: &Self); fn sub(&mut self, o: &Self); fn max_part(&self) -> Self::Max; fn merge_max(into: &mut Self::Max, o: &Self::Max); fn to_json(&self) -> Result<String, serde_json::Error>; fn from_json(s: &str) -> Result<Self, serde_json::Error>; }`
  - `struct G1Cell { their, gross, ours, flagged: PerSide<i64>, merged_away: PerSide<[i64; 3]> }` (FAMILY `g1`)
  - `struct DurCell { their: PerSide<CountMap>, ours: PerSide<DurMap>, left: PerSide<i64>, gap: PerSide<CountMap>, flagged: PerSide<i64>, open: PerSide<i64>, hour_observed: i64, hour_long: i64, clipped: CountMap }` (FAMILY `dur`, for G2 and G3)
  - `struct G4Cell { their: PerSide<i64>, net: PerSide<CountMap>, gross: PerSide<CountMap> }` (FAMILY `g4`)
- Produces (`cards::window`):
  - `SPANS: [(&str, i64); 3] = [("24h", 24), ("7d", 168), ("30d", 720)]`, `RECENT_HOURS: i64 = 48`
  - `struct Windows<C: CellData> { now_hour: i64, spans: [C; 3], all: C, maxes: BTreeMap<i64, C::Max>, recent: BTreeMap<i64, C> }` with `new(now_hour)`, `span_start(&self, span_hours: i64) -> i64`, `needed_ranges(&self, to_hour: i64) -> Vec<(i64, i64)>`, `advance(&mut self, to_hour: i64, stored: &BTreeMap<i64, C>)`, `apply(&mut self, hour: i64, old: Option<&C>, new: &C)`, `max_from(&self, from_hour: i64) -> C::Max`, `today(&self, day_start: i64) -> C`

- [ ] **Step 1: Write the failing cell tests.** Create `crates/cards/tests/cells.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const M: i64 = 60_000;

/// A closed, uncensored, positive episode.
pub fn ep(id: &str, side: Side, opened: i64, end: i64) -> Ep {
    Ep {
        id: id.into(),
        run_id: 1,
        side,
        opened_ms: opened,
        closed_ms: Some(end),
        end_ms: end,
        duration_ms: Some(end - opened),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        edge_net: dec!(0.0049),
        edge_gross: dec!(0.0137),
        pnl_net: dec!(0.41),
        pnl_gross: dec!(1.37),
        cost: dec!(99.59),
        budget100: Some(dec!(0.33)),
        suspect: false,
        stale: false,
        prev_gap_ms: None,
    }
}

fn input(hour: i64, eps: &[Ep]) -> HourInput<'_> {
    HourInput { hour_ms: hour, episodes: eps, latency_ms: &[], equity_deltas: &[] }
}

#[test]
fn g1_counts_positive_episodes_opened_in_the_hour_per_side() {
    let mut neg = ep("n", Side::Yes, 10, 20);
    neg.pnl_net = dec!(-0.01);
    let mut flag = ep("f", Side::No, 30, 40);
    flag.suspect = true;
    let mut close1 = ep("c1", Side::No, 50, 60);
    close1.prev_gap_ms = Some(200);
    let mut close2 = ep("c2", Side::No, 70, 80);
    close2.prev_gap_ms = Some(0);
    let eps = vec![
        ep("y1", Side::Yes, 0, 5),
        ep("y2", Side::Yes, H - 1, H + 10),
        ep("later", Side::Yes, H, H + 5), // next hour
        ep("earlier", Side::Yes, -5, 5),  // previous hour
        neg, flag, close1, close2,
    ];
    let c = G1Cell::build(&input(0, &eps));
    assert_eq!((c.their.yes, c.their.no), (2, 3));
    assert_eq!((c.gross.yes, c.gross.no), (3, 3), "gross counts every episode with Π_gross > 0");
    assert_eq!((c.ours.yes, c.ours.no), (2, 2));
    assert_eq!((c.flagged.yes, c.flagged.no), (0, 1));
    assert_eq!(c.merged_away.no, [1, 2, 2], "≤0 ms: c2; ≤250 ms and ≤1 s: c1 and c2");
    assert_eq!(c.merged_away.yes, [0, 0, 0]);
}

#[test]
fn dur_cell_splits_uncensored_right_left_gap_flagged_and_open() {
    let mut right = ep("r", Side::Yes, 10, 1_010);
    right.censor_right = true;
    let mut left = ep("l", Side::Yes, 20, 520);
    left.censor_left = true;
    let mut gap = ep("g", Side::No, 30, 90_030);
    gap.censor_gap = true;
    let mut flag = ep("f", Side::Yes, 40, 140);
    flag.stale = true;
    let mut open = ep("o", Side::No, 50, H);
    open.closed_ms = None;
    open.duration_ms = None;
    let eps = vec![ep("a", Side::Yes, 0, 2_000), ep("b", Side::Yes, 5, 2_005), right, left, gap, flag, open];
    let c = DurCell::build(&input(0, &eps));
    assert_eq!(c.their.yes.get(&2_000), Some(&2), "a and b: uncensored, THEIR includes flagged");
    assert_eq!(c.their.yes.get(&100), Some(&1), "the flagged one is uncensored too");
    assert_eq!(c.ours.yes.get(&2_000), Some(&(2, 0)));
    assert_eq!(c.ours.yes.get(&1_000), Some(&(0, 1)), "right-censored enters KM as censored");
    assert_eq!(c.ours.yes.get(&100), None, "OUR METHOD drops flagged");
    assert_eq!((c.left.yes, c.flagged.yes), (1, 1));
    assert_eq!(c.gap.no.get(&90_000), Some(&1));
    assert_eq!(c.open.no, 1);
}

#[test]
fn an_episode_spanning_three_hours_is_observed_in_each_hour() {
    // opened 00:30, closed 03:10: touches hours 0, 1, 2 and 3
    let long = ep("long", Side::No, 30 * M, 3 * H + 10 * M);
    let short = ep("short", Side::Yes, H + 20 * M, H + 25 * M);
    let eps = vec![long, short];
    let cells: Vec<DurCell> = (0..4).map(|i| DurCell::build(&input(i * H, &eps))).collect();
    assert_eq!(cells.iter().map(|c| c.hour_observed).collect::<Vec<_>>(), vec![1, 2, 1, 1]);
    assert_eq!(
        cells.iter().map(|c| c.hour_long).collect::<Vec<_>>(),
        vec![0, 1, 1, 0],
        "long-lived = present in the first and the last 5 minutes of the hour"
    );
    assert_eq!(cells[0].clipped.get(&(30 * M)), Some(&1));
    assert_eq!(cells[1].clipped.get(&H), Some(&1));
    assert_eq!(cells[1].clipped.get(&(5 * M)), Some(&1));
    assert_eq!(cells[3].clipped.get(&(10 * M)), Some(&1));
    assert_eq!(cells[0].ours.no.get(&(2 * H + 40 * M)), Some(&(1, 0)), "its duration counts once, in its opening hour");
    assert!(cells[1..].iter().all(|c| c.ours.no.is_empty()));
}

#[test]
fn g4_bins_peak_edges_in_centi_bps_and_drops_flagged() {
    let mut big = ep("b", Side::Yes, 1, 2);
    big.edge_net = dec!(0.0512);
    big.edge_gross = dec!(0.06);
    let mut flag = ep("f", Side::Yes, 3, 4);
    flag.suspect = true;
    let eps = vec![ep("a", Side::Yes, 0, 1), big, flag];
    let c = G4Cell::build(&input(0, &eps));
    assert_eq!(c.their.yes, 3);
    assert_eq!(c.net.yes.get(&4_900), Some(&1));
    assert_eq!(c.net.yes.get(&51_200), Some(&1));
    assert_eq!(c.gross.yes.get(&13_700), Some(&1));
    assert_eq!(cards::stats::total(&c.net.yes), 2);
}

#[test]
fn cells_add_subtract_and_round_trip_through_json() {
    let eps = vec![ep("a", Side::Yes, 0, 2_000), ep("b", Side::No, 10, 20)];
    let a = DurCell::build(&input(0, &eps));
    let mut sum = DurCell::default();
    sum.add(&a);
    sum.add(&a);
    sum.sub(&a);
    assert_eq!(sum, a);
    sum.sub(&a);
    assert_eq!(sum, DurCell::default(), "subtraction removes zero entries");
    let json = a.to_json().unwrap();
    assert!(!json.contains('.'), "cells carry integer counts only: {json}");
    assert_eq!(DurCell::from_json(&json).unwrap(), a);
    let g1 = G1Cell::build(&input(0, &eps));
    assert_eq!(G1Cell::from_json(&g1.to_json().unwrap()).unwrap(), g1);
    assert_eq!(serde_json::to_string(&Num(dec!(0.410))).unwrap(), "\"0.410\"");
    assert_eq!(serde_json::from_str::<Num>("\"12345678901234567.89\"").unwrap(), Num(dec!(12345678901234567.89)));
    let _: Decimal = Num::default().0;
    assert_eq!((G1Cell::FAMILY, DurCell::FAMILY, G4Cell::FAMILY), ("g1", "dur", "g4"));
    assert!(DurCell::AREAS.contains(&"censoring") && !G1Cell::AREAS.contains(&"censoring"));
    assert!(G4Cell::AREAS.contains(&"depth"));
}
```

- [ ] **Step 2: Write the failing window property test.** Create `crates/cards/tests/window_property.rs`:

```rust
//! The incremental windows equal a direct sum of the stored cells in each window, after any mix
//! of rebuilt hours (late closes, future hours) and clock advances (spec §7, Review Focus 1 and 5).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::{CellData, G1Cell, PerSide};
use cards::window::{SPANS, Windows};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn cell(seed: i64) -> G1Cell {
    G1Cell {
        their: PerSide { yes: seed % 7, no: seed % 3 },
        gross: PerSide { yes: seed % 5, no: 1 },
        ours: PerSide { yes: seed % 4, no: seed % 2 },
        flagged: PerSide { yes: 0, no: seed % 2 },
        merged_away: PerSide { yes: [seed % 2, seed % 3, seed % 4], no: [0, 0, 1] },
    }
}

fn direct(stored: &BTreeMap<i64, G1Cell>, from: i64, to: i64) -> G1Cell {
    let mut c = G1Cell::default();
    for (_, v) in stored.range(from..=to) {
        c.add(v);
    }
    c
}

#[derive(Debug, Clone)]
enum Op {
    /// Rebuild the cell of (now + offset hours) with a new value (offset ≤ 0: past, > 0: future).
    Put(i64, i64),
    /// Advance the clock by n hours.
    Advance(i64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (-800i64..3, 0i64..1000).prop_map(|(o, s)| Op::Put(o, s)),
        1 => prop_oneof![1i64..3, 20i64..30, 700i64..800].prop_map(Op::Advance),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn windows_equal_the_direct_sum_after_any_ops(ops in proptest::collection::vec(op(), 1..60)) {
        let start = 1_000 * H;
        let mut stored: BTreeMap<i64, G1Cell> = BTreeMap::new();
        let mut w: Windows<G1Cell> = Windows::new(start);
        for o in ops {
            match o {
                Op::Put(off, seed) => {
                    let h = w.now_hour + off * H;
                    let new = cell(seed);
                    // the applied version of an hour ≤ now is the stored one (the analyze invariant)
                    let old = if h <= w.now_hour { stored.get(&h).cloned() } else { None };
                    w.apply(h, old.as_ref(), &new);
                    stored.insert(h, new);
                }
                Op::Advance(n) => {
                    let to = w.now_hour + n * H;
                    let mut need = BTreeMap::new();
                    for (from, until) in w.needed_ranges(to) {
                        for (h, c) in stored.range(from..until) {
                            need.insert(*h, c.clone());
                        }
                    }
                    w.advance(to, &need);
                }
            }
            for (i, (_, span)) in SPANS.iter().enumerate() {
                let from = w.now_hour - (span - 1) * H;
                prop_assert_eq!(&w.spans[i], &direct(&stored, from, w.now_hour), "span {}", span);
            }
            prop_assert_eq!(&w.all, &direct(&stored, i64::MIN, w.now_hour));
            let day = w.now_hour - 5 * H;
            prop_assert_eq!(&w.today(day), &direct(&stored, day, w.now_hour));
        }
    }
}

#[test]
fn a_clock_that_goes_back_changes_nothing() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    w.apply(100 * H, None, &cell(5));
    let before = w.all.clone();
    w.advance(90 * H, &BTreeMap::new());
    assert_eq!((w.now_hour, w.all.clone()), (100 * H, before));
    assert!(w.needed_ranges(90 * H).is_empty());
}
```

- [ ] **Step 3: Write the failing merge property test.** Create `crates/cards/tests/merge_property.rs`:

```rust
//! Spec §9 merge property: for random episode sets, the sum of the hourly cells equals the direct
//! computation over the same episodes (and Kaplan–Meier on the merged cells equals arb-core's
//! independent implementation on the raw durations).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::*;
use cards::stats::{self, CountMap, DurMap};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const HOURS: i64 = 6;

fn arb_ep() -> impl Strategy<Value = Ep> {
    (
        (0..HOURS * H, 0..3 * H, any::<bool>(), any::<bool>()),
        (any::<[bool; 5]>(), -2i64..8, 0i64..9_000, proptest::option::of(0i64..1_200)),
    )
        .prop_map(|((opened, dur, yes, closed), (f, pnl_c, edge, gap))| {
            let end = opened + dur;
            Ep {
                id: format!("e{opened}-{dur}"),
                run_id: 1,
                side: if yes { Side::Yes } else { Side::No },
                opened_ms: opened,
                closed_ms: closed.then_some(end),
                end_ms: if closed { end } else { HOURS * H },
                duration_ms: closed.then_some(dur),
                censor_left: f[0],
                censor_right: f[1],
                censor_gap: f[2],
                edge_net: Decimal::new(edge, 6),
                edge_gross: Decimal::new(edge + 100, 6),
                pnl_net: Decimal::new(pnl_c, 2),
                pnl_gross: Decimal::new(pnl_c + 1, 2),
                cost: dec!(80),
                budget100: Some(Decimal::new(pnl_c, 2)),
                suspect: f[3],
                stale: f[4],
                prev_gap_ms: gap,
            }
        })
}

/// What the store query returns for hour h: opened before its end, still open or closed at/after its start.
fn store_view(eps: &[Ep], h: i64) -> Vec<Ep> {
    eps.iter()
        .filter(|e| e.opened_ms < h + H && e.closed_ms.is_none_or(|c| c >= h))
        .cloned()
        .collect()
}

fn summed<C: CellData>(eps: &[Ep]) -> C {
    let mut sum = C::default();
    for i in 0..HOURS {
        let h = i * H;
        let view = store_view(eps, h);
        sum.add(&C::build(&HourInput { hour_ms: h, episodes: &view, latency_ms: &[], equity_deltas: &[] }));
    }
    sum
}

fn side_count(eps: &[Ep], s: Side, f: impl Fn(&Ep) -> bool) -> i64 {
    i64::try_from(eps.iter().filter(|e| e.side == s && f(e)).count()).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn g1_cells_sum_to_the_direct_counts(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: G1Cell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            prop_assert_eq!(*c.their.get(s), side_count(&eps, s, |e| e.pnl_net > Decimal::ZERO));
            prop_assert_eq!(*c.gross.get(s), side_count(&eps, s, |e| e.pnl_gross > Decimal::ZERO));
            let ours = |e: &Ep| e.pnl_net > Decimal::ZERO && !e.suspect && !e.stale;
            prop_assert_eq!(*c.ours.get(s), side_count(&eps, s, ours));
            for (i, g) in MERGE_GAPS_MS.iter().enumerate() {
                let m = side_count(&eps, s, |e| ours(e) && e.prev_gap_ms.is_some_and(|p| p <= *g));
                prop_assert_eq!(c.merged_away.get(s)[i], m);
            }
        }
    }

    #[test]
    fn dur_cells_sum_to_the_direct_maps_and_km_matches_arb_core(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: DurCell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            let mut their = CountMap::new();
            let mut ours = DurMap::new();
            let mut obs = Vec::new();
            for e in eps.iter().filter(|e| e.side == s && e.pnl_net > Decimal::ZERO) {
                let Some(d) = e.duration_ms else { continue };
                if !e.censor_left && !e.censor_right && !e.censor_gap {
                    *their.entry(d).or_insert(0) += 1;
                }
                if e.censor_left || e.censor_gap || e.suspect || e.stale {
                    continue;
                }
                let slot = ours.entry(d).or_insert((0, 0));
                if e.censor_right { slot.1 += 1 } else { slot.0 += 1 }
                obs.push((d, !e.censor_right));
            }
            prop_assert_eq!(c.their.get(s), &their);
            prop_assert_eq!(c.ours.get(s), &ours);
            let times: Vec<i64> = ours.keys().copied().collect();
            let k = stats::km(c.ours.get(s).iter().map(|(t, v)| (*t, *v)), &times);
            let arb: std::collections::BTreeMap<i64, Decimal> = arb_core::stats::kaplan_meier(&obs).into_iter().collect();
            let mut prev = Decimal::ONE;
            for (i, t) in times.iter().enumerate() {
                let expect = arb.get(t).copied().unwrap_or(prev);
                prop_assert_eq!(stats::fp_to_dec(k.at[i]).round_dp(12), expect.round_dp(12));
                prev = expect;
            }
        }
        // G3, hour by hour, straight from the episodes
        let (mut observed, mut long, mut clipped) = (0i64, 0i64, CountMap::new());
        for i in 0..HOURS {
            let h = i * H;
            for e in eps.iter().filter(|e| e.pnl_net > Decimal::ZERO) {
                let (s, t) = (e.opened_ms.max(h), e.end_ms.min(h + H));
                if s >= t { continue; }
                observed += 1;
                if e.opened_ms < h + 300_000 && e.end_ms > h + H - 300_000 { long += 1; }
                if e.closed_ms.is_some() && !e.censor_left && !e.censor_right && !e.censor_gap {
                    *clipped.entry(t - s).or_insert(0) += 1;
                }
            }
        }
        prop_assert_eq!((c.hour_observed, c.hour_long), (observed, long));
        prop_assert_eq!(&c.clipped, &clipped);
    }

    #[test]
    fn g4_cells_sum_to_the_direct_edge_maps(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: G4Cell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            let mut net = CountMap::new();
            for e in eps.iter().filter(|e| e.side == s && e.pnl_net > Decimal::ZERO && !e.suspect && !e.stale) {
                *net.entry(stats::centi_bps(e.edge_net)).or_insert(0) += 1;
            }
            prop_assert_eq!(c.net.get(s), &net);
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test cells --test window_property --test merge_property`
Expected: compile errors `unresolved import cards::cells` and `unresolved import cards::window`.

- [ ] **Step 5: Write the cells.** Create `crates/cards/src/cells.rs`:

```rust
//! Hourly, merge-exact cells (spec §7). One cell per family per premise key per UTC hour. Every
//! field except Edge capture's maxima is a sum, so a window is the sum of its cells, and a
//! window drops an expired or rebuilt cell by subtracting it. Counts are i64: a negative count
//! would mean a bug, never a valid state. Money is `Num` (a JSON string).
use crate::HOUR_MS;
use crate::stats::{self, CountMap, DurMap};
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Flicker-merge sensitivity (parent spec §5.5: runs separated by ≤ {0, 250 ms, 1 s}).
pub const MERGE_GAPS_MS: [i64; 3] = [0, 250, 1_000];
/// G3: long-lived means ≥ 50 minutes.
pub const LONG_MS: i64 = 3_000_000;
/// G3 THEIR METHOD: present in the first and in the last 5 minutes of the hour.
pub const EDGE_MINUTES_MS: i64 = 300_000;

const GEBELE_AREAS: &[&str] = &["detector_no", "detector_yes", "episode_def", "fees"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Yes,
    No,
}

impl Side {
    /// The scanner's `kind_str`: `yes_basket` is the YES side, `no_basket` the NO side;
    /// `yes_basket_incomplete` (not arbitrage) and `implication` belong to no side.
    pub fn from_kind(kind: &str) -> Option<Side> {
        match kind {
            "yes_basket" => Some(Side::Yes),
            "no_basket" => Some(Side::No),
            _ => None,
        }
    }
}

/// A decimal stored as a JSON string, never a JSON number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Num(#[serde(with = "rust_decimal::serde::str")] pub Decimal);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerSide<T> {
    pub yes: T,
    pub no: T,
}

impl<T> PerSide<T> {
    pub fn get(&self, s: Side) -> &T {
        match s {
            Side::Yes => &self.yes,
            Side::No => &self.no,
        }
    }

    pub fn get_mut(&mut self, s: Side) -> &mut T {
        match s {
            Side::Yes => &mut self.yes,
            Side::No => &mut self.no,
        }
    }
}

/// One episode as the cards see it (analyze maps `store::CardEpisode` into this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ep {
    pub id: String,
    pub run_id: i64,
    pub side: Side,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    /// The span's end for hour overlap: `closed_ms`, else its run's end, else now.
    pub end_ms: i64,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    /// Peak net / gross top edge, as a fraction of $1.
    pub edge_net: Decimal,
    pub edge_gross: Decimal,
    pub pnl_net: Decimal,
    pub pnl_gross: Decimal,
    pub cost: Decimal,
    pub budget100: Option<Decimal>,
    pub suspect: bool,
    pub stale: bool,
    pub prev_gap_ms: Option<i64>,
}

impl Ep {
    /// Net of taker fees, Π > 0 (Gebele's episode, and ours).
    pub fn positive(&self) -> bool {
        self.pnl_net > Decimal::ZERO
    }

    /// Excluded under OUR METHOD (and counted as excluded).
    pub fn flagged(&self) -> bool {
        self.suspect || self.stale
    }

    /// Closed with no censoring flag: its duration is known exactly.
    pub fn uncensored(&self) -> bool {
        self.closed_ms.is_some() && !self.censor_left && !self.censor_right && !self.censor_gap
    }
}

/// Everything one hour's cell is built from. `episodes` may hold more than the hour (a whole
/// batch): each builder picks what it needs with `opened_in` and `overlap`.
#[derive(Debug, Clone, Copy)]
pub struct HourInput<'a> {
    pub hour_ms: i64,
    pub episodes: &'a [Ep],
    /// Edge capture: one measured latency (ms) per health minute of the hour.
    pub latency_ms: &'a [i64],
    /// Edge capture: (scenario, equity change) per paper mark inside the hour.
    pub equity_deltas: &'a [(String, Decimal)],
}

pub fn opened_in(eps: &[Ep], hour_ms: i64) -> impl Iterator<Item = &Ep> {
    eps.iter()
        .filter(move |e| e.opened_ms >= hour_ms && e.opened_ms < hour_ms + HOUR_MS)
}

/// The part of the episode's span inside the hour, if any.
pub fn overlap(e: &Ep, hour_ms: i64) -> Option<(i64, i64)> {
    let s = e.opened_ms.max(hour_ms);
    let t = e.end_ms.min(hour_ms + HOUR_MS);
    (s < t).then_some((s, t))
}

pub trait CellData:
    Clone + Default + PartialEq + Debug + Serialize + DeserializeOwned
{
    /// `card_cells.card`.
    const FAMILY: &'static str;
    /// The premise areas the family depends on (arb_core::premises names).
    const AREAS: &'static [&'static str];
    /// Needs health latency and paper marks (Edge capture only).
    const EDGE_INPUTS: bool = false;
    /// Non-subtractable per-hour maxima, merged over a window's hours by `merge_max`.
    type Max: Clone + Default + PartialEq + Debug;

    fn build(inp: &HourInput<'_>) -> Self;
    fn add(&mut self, other: &Self);
    fn sub(&mut self, other: &Self);
    fn max_part(&self) -> Self::Max;
    fn merge_max(into: &mut Self::Max, other: &Self::Max);

    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

fn add_i64(a: &mut PerSide<i64>, b: &PerSide<i64>, sign: i64) {
    a.yes += sign * b.yes;
    a.no += sign * b.no;
}

fn add_cm(a: &mut PerSide<CountMap>, b: &PerSide<CountMap>, sign: i64) {
    stats::add_counts(&mut a.yes, &b.yes, sign);
    stats::add_counts(&mut a.no, &b.no, sign);
}

fn add_dm(a: &mut PerSide<DurMap>, b: &PerSide<DurMap>, sign: i64) {
    stats::add_durs(&mut a.yes, &b.yes, sign);
    stats::add_durs(&mut a.no, &b.no, sign);
}

fn bump(m: &mut CountMap, k: i64) {
    *m.entry(k).or_insert(0) += 1;
}

/// G1: side asymmetry (spec §5.1). Episodes opened in the hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G1Cell {
    /// Π_net > 0 (THEIR METHOD: raw episodes).
    pub their: PerSide<i64>,
    /// Π_gross > 0 (context).
    pub gross: PerSide<i64>,
    /// Π_net > 0 and not flagged (OUR METHOD).
    pub ours: PerSide<i64>,
    /// Π_net > 0 and flagged (excluded from OUR METHOD, counted).
    pub flagged: PerSide<i64>,
    /// OUR METHOD episodes that merge into their predecessor at gap ≤ 0 / 250 / 1000 ms.
    pub merged_away: PerSide<[i64; 3]>,
}

impl G1Cell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_i64(&mut self.their, &o.their, s);
        add_i64(&mut self.gross, &o.gross, s);
        add_i64(&mut self.ours, &o.ours, s);
        add_i64(&mut self.flagged, &o.flagged, s);
        for (a, b) in self.merged_away.yes.iter_mut().zip(o.merged_away.yes) {
            *a += s * b;
        }
        for (a, b) in self.merged_away.no.iter_mut().zip(o.merged_away.no) {
            *a += s * b;
        }
    }
}

impl CellData for G1Cell {
    const FAMILY: &'static str = "g1";
    const AREAS: &'static [&'static str] = GEBELE_AREAS;
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let mut c = G1Cell::default();
        for e in opened_in(inp.episodes, inp.hour_ms) {
            if e.pnl_gross > Decimal::ZERO {
                *c.gross.get_mut(e.side) += 1;
            }
            if !e.positive() {
                continue;
            }
            *c.their.get_mut(e.side) += 1;
            if e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
                continue;
            }
            *c.ours.get_mut(e.side) += 1;
            if let Some(gap) = e.prev_gap_ms {
                for (slot, g) in c.merged_away.get_mut(e.side).iter_mut().zip(MERGE_GAPS_MS) {
                    if gap <= g {
                        *slot += 1;
                    }
                }
            }
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}

/// G2 and G3: durations and survival, the long-lived share (spec §5.2, §5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurCell {
    /// THEIR METHOD: uncensored durations (flagged included), by opening hour.
    pub their: PerSide<CountMap>,
    /// OUR METHOD, Kaplan–Meier input: duration → (events, right-censored), flagged, left- and
    /// gap-censored episodes excluded.
    pub ours: PerSide<DurMap>,
    pub left: PerSide<i64>,
    /// Gap-censored recorded spans (an upper bound on the true duration).
    pub gap: PerSide<CountMap>,
    pub flagged: PerSide<i64>,
    /// Not closed yet: no duration.
    pub open: PerSide<i64>,
    /// G3 THEIR METHOD: episodes observed in this clock hour, and those present in both its first
    /// and last 5 minutes. An episode spanning several hours is observed in each.
    pub hour_observed: i64,
    pub hour_long: i64,
    /// G3 OUR METHOD: overlap of each uncensored episode with each hour it touches.
    pub clipped: CountMap,
}

impl DurCell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_cm(&mut self.their, &o.their, s);
        add_dm(&mut self.ours, &o.ours, s);
        add_i64(&mut self.left, &o.left, s);
        add_cm(&mut self.gap, &o.gap, s);
        add_i64(&mut self.flagged, &o.flagged, s);
        add_i64(&mut self.open, &o.open, s);
        self.hour_observed += s * o.hour_observed;
        self.hour_long += s * o.hour_long;
        stats::add_counts(&mut self.clipped, &o.clipped, s);
    }
}

impl CellData for DurCell {
    const FAMILY: &'static str = "dur";
    const AREAS: &'static [&'static str] =
        &["censoring", "detector_no", "detector_yes", "episode_def", "fees"];
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let h = inp.hour_ms;
        let mut c = DurCell::default();
        for e in opened_in(inp.episodes, h).filter(|e| e.positive()) {
            let Some(d) = e.duration_ms.filter(|_| e.closed_ms.is_some()) else {
                *c.open.get_mut(e.side) += 1;
                continue;
            };
            if e.uncensored() {
                bump(c.their.get_mut(e.side), d);
            }
            if e.censor_left {
                *c.left.get_mut(e.side) += 1;
                continue;
            }
            if e.censor_gap {
                bump(c.gap.get_mut(e.side), d);
                continue;
            }
            if e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
                continue;
            }
            let slot = c.ours.get_mut(e.side).entry(d).or_insert((0, 0));
            if e.censor_right {
                slot.1 += 1;
            } else {
                slot.0 += 1;
            }
        }
        for e in inp.episodes.iter().filter(|e| e.positive()) {
            let Some((s, t)) = overlap(e, h) else { continue };
            c.hour_observed += 1;
            let first = e.opened_ms < h + EDGE_MINUTES_MS && e.end_ms > h;
            let last = e.opened_ms < h + HOUR_MS && e.end_ms > h + HOUR_MS - EDGE_MINUTES_MS;
            if first && last {
                c.hour_long += 1;
            }
            if e.uncensored() {
                bump(&mut c.clipped, t - s);
            }
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}

/// G4: net edge by side (spec §5.4). Peak edges in 0.01-bp units, episodes opened in the hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G4Cell {
    /// Raw Π_net > 0 counts (THEIR METHOD, and the counts the paper's figure insets show).
    pub their: PerSide<i64>,
    /// OUR METHOD (not flagged): peak net edge → count.
    pub net: PerSide<CountMap>,
    /// The same episodes' peak gross edge (the overlay).
    pub gross: PerSide<CountMap>,
}

impl G4Cell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_i64(&mut self.their, &o.their, s);
        add_cm(&mut self.net, &o.net, s);
        add_cm(&mut self.gross, &o.gross, s);
    }
}

impl CellData for G4Cell {
    const FAMILY: &'static str = "g4";
    const AREAS: &'static [&'static str] =
        &["depth", "detector_no", "detector_yes", "episode_def", "fees"];
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let mut c = G4Cell::default();
        for e in opened_in(inp.episodes, inp.hour_ms).filter(|e| e.positive()) {
            *c.their.get_mut(e.side) += 1;
            if e.flagged() {
                continue;
            }
            bump(c.net.get_mut(e.side), stats::centi_bps(e.edge_net));
            bump(c.gross.get_mut(e.side), stats::centi_bps(e.edge_gross));
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}
```

- [ ] **Step 6: Write the windows.** Create `crates/cards/src/window.rs`:

```rust
//! Incremental windows (spec §7 step 3, kept fast as "all" grows: Review Focus 5). Instead of
//! re-reading and re-merging up to every cell of a key on each tick, each window keeps the sum of
//! the cells it covers: a rebuilt hour swaps its old cell for the new one (subtract, add), and
//! advancing the clock subtracts the hours that leave each span and adds stored cells of the
//! hours that enter. The invariant analyze keeps: the applied version of any hour ≤ now_hour is
//! exactly the one stored in card_cells.
use crate::HOUR_MS;
use crate::cells::CellData;
use std::collections::BTreeMap;

/// Rolling spans in hours, ending on (and including) the current, partial hour.
pub const SPANS: [(&str, i64); 3] = [("24h", 24), ("7d", 168), ("30d", 720)];
/// Cells kept in memory: today's window and the 24h window's expiring hour.
pub const RECENT_HOURS: i64 = 48;

#[derive(Debug, Clone)]
pub struct Windows<C: CellData> {
    /// The current hour (the last hour of every window).
    pub now_hour: i64,
    /// One sum per SPANS entry.
    pub spans: [C; 3],
    /// Every applied hour ≤ now_hour.
    pub all: C,
    /// Per-hour maxima of every applied hour (tiny: Edge capture's two numbers).
    pub maxes: BTreeMap<i64, C::Max>,
    pub recent: BTreeMap<i64, C>,
}

impl<C: CellData> Windows<C> {
    pub fn new(now_hour: i64) -> Self {
        Self {
            now_hour,
            spans: Default::default(),
            all: C::default(),
            maxes: BTreeMap::new(),
            recent: BTreeMap::new(),
        }
    }

    pub fn span_start(&self, span_hours: i64) -> i64 {
        self.now_hour - (span_hours - 1) * HOUR_MS
    }

    fn recent_floor(&self) -> i64 {
        self.now_hour - (RECENT_HOURS - 1) * HOUR_MS
    }

    /// The stored cells `advance(to_hour)` needs, as [from, to) ranges: hours leaving each span
    /// (some are in `recent` already) and hours entering the windows.
    pub fn needed_ranges(&self, to_hour: i64) -> Vec<(i64, i64)> {
        if to_hour <= self.now_hour {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (_, span) in SPANS {
            let from = self.span_start(span);
            let end = (to_hour - (span - 1) * HOUR_MS).min(self.now_hour + HOUR_MS);
            if from < end {
                out.push((from, end));
            }
        }
        out.push((self.now_hour + HOUR_MS, to_hour + HOUR_MS));
        out
    }

    /// Move the windows' end to `to_hour`. `stored` holds the stored cells of
    /// `needed_ranges(to_hour)` (a missing hour is an empty cell). A `to_hour` that isn't later
    /// is ignored: a clock that went back changes nothing.
    pub fn advance(&mut self, to_hour: i64, stored: &BTreeMap<i64, C>) {
        if to_hour <= self.now_hour {
            return;
        }
        let old_now = self.now_hour;
        for (i, (_, span)) in SPANS.iter().enumerate() {
            let from = old_now - (span - 1) * HOUR_MS;
            let end = (to_hour - (span - 1) * HOUR_MS).min(old_now + HOUR_MS);
            let mut h = from;
            while h < end {
                if let Some(c) = self.recent.get(&h).or_else(|| stored.get(&h)) {
                    self.spans[i].sub(c);
                }
                h += HOUR_MS;
            }
        }
        for (h, c) in stored.range(old_now + HOUR_MS..=to_hour) {
            for (i, (_, span)) in SPANS.iter().enumerate() {
                if *h >= to_hour - (span - 1) * HOUR_MS {
                    self.spans[i].add(c);
                }
            }
            self.all.add(c);
            self.maxes.insert(*h, c.max_part());
            self.recent.insert(*h, c.clone());
        }
        self.now_hour = to_hour;
        let floor = self.recent_floor();
        self.recent = self.recent.split_off(&floor);
    }

    /// Replace hour `hour`'s applied cell `old` (None: nothing applied yet) with `new` in every
    /// window holding it. Hours after now_hour are left to `advance` (it adds their stored cell).
    pub fn apply(&mut self, hour: i64, old: Option<&C>, new: &C) {
        if hour > self.now_hour {
            return;
        }
        for (i, (_, span)) in SPANS.iter().enumerate() {
            let start = self.span_start(*span);
            if hour >= start {
                if let Some(o) = old {
                    self.spans[i].sub(o);
                }
                self.spans[i].add(new);
            }
        }
        if let Some(o) = old {
            self.all.sub(o);
        }
        self.all.add(new);
        self.maxes.insert(hour, new.max_part());
        if hour >= self.recent_floor() {
            self.recent.insert(hour, new.clone());
        }
    }

    /// The merged maxima of the applied hours in [from_hour, now_hour].
    pub fn max_from(&self, from_hour: i64) -> C::Max {
        let mut m = C::Max::default();
        for (_, v) in self.maxes.range(from_hour..=self.now_hour) {
            C::merge_max(&mut m, v);
        }
        m
    }

    /// The sum of the applied hours in [day_start, now_hour] (Edge capture's `today`).
    pub fn today(&self, day_start: i64) -> C {
        let mut c = C::default();
        for (_, v) in self.recent.range(day_start..=self.now_hour) {
            c.add(v);
        }
        c
    }
}
```

In `crates/cards/src/lib.rs`, add `pub mod cells;` and `pub mod window;` next to `pub mod stats;`.

- [ ] **Step 7: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards`
Expected: PASS: 5 tests in `cells.rs`, 2 in `window_property.rs`, 3 in `merge_property.rs`, and Task 5's tests.

- [ ] **Step 8: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p cards --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 9: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/cards/src/lib.rs crates/cards/src/cells.rs crates/cards/src/window.rs crates/cards/tests/cells.rs crates/cards/tests/window_property.rs crates/cards/tests/merge_property.rs && git commit -m "cards: merge-exact hourly cells (G1, durations/G3, G4) and incremental windows with property tests"
```

---

### Task 7: `cards`: card bodies for G1–G4, status, reproducibility hash, and the papers list

**Files:**
- Create: `crates/cards/src/body.rs`, `crates/cards/src/gebele.rs`, `crates/cards/src/papers.rs`, `crates/cards/tests/bodies.rs`
- Modify: `crates/cards/src/lib.rs` (`pub mod body; pub mod gebele; pub mod papers;`)

**Interfaces:**
- Consumes: `cards::cells::{CellData, G1Cell, DurCell, G4Cell, Side, MERGE_GAPS_MS, LONG_MS}`, `cards::stats::*` (Tasks 5–6).
- Produces (`cards::body`):
  - `WINS: [&str; 5] = ["24h", "7d", "30d", "all", "today"]`
  - `enum Status { Live, Warming, Thin, UnderReview }` (serde `live`, `warming`, `thin`, `under_review`) with `as_str()`
  - `struct Row { label: String, unit: String, paper: Option<String>, their: Option<String>, ours: Option<String> }`; units used: `count`, `×`, `s`, `%`, `bps`, `USD`, `ms`
  - `struct Series { bars: Vec<Bar>, curves: Vec<Curve>, bins: Vec<Bin>, decay: Vec<DecayPoint>, measured: Option<Measured> }` (empty parts are omitted from JSON)
  - `struct Bar { label: String, value: i64 }`, `struct Curve { label: String, points: Vec<CurvePoint>, median_ms: Option<String> }`, `struct CurvePoint { t_ms: i64, s: String }`, `struct Bin { side: String, lo_bps: i64, hi_bps: Option<i64>, net: i64, gross: i64 }`, `struct DecayPoint { scenario: String, latency_ms: i64, net_ms: i64, captured: Option<String>, rate_pct: Option<String>, headline: bool }`, `struct Measured { latency_ms: i64, host: String }`
  - `struct CardBody { card, paper, title, win: String, status: Status, premise_key, hash: String, data_from_ms, data_to_ms, since_ms: Option<i64>, n: i64, columns: Vec<Row>, series: Series, notes: Vec<String>, thin_reasons: Vec<String>, episode_link: Option<String>, computed_ms: i64 }`
  - `struct ScenarioLat { name: String, net_ms: i64, latency_ms: i64 }`
  - `struct BodyCtx<'a> { win: &'a str, base: Status, premise_key: &'a str, analyze_version: &'a str, from_ms: Option<i64>, to_ms: Option<i64>, since_ms: Option<i64>, computed_ms: i64, scenarios: &'a [ScenarioLat] }`
  - `trait Card: CellData { fn bodies(agg: &Self, max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody>; }`
  - `fn base_status(catching_up: bool, since_ms: Option<i64>, window_start_ms: Option<i64>) -> Status`, `fn final_status(base: Status, thin: &[String]) -> Status`, `fn repro_hash(premise_key: &str, analyze_version: &str, from_ms: Option<i64>, to_ms: Option<i64>, card: &str, win: &str, params: &str) -> String`, `fn row(label: &str, unit: &str, paper: Option<&str>, their: Option<String>, ours: Option<String>) -> Row`, `fn make(…) -> CardBody` (see Step 3)
- Produces (`cards::gebele`): `PAPER_ID = "gebele-2026"`, `THIN_MEDIAN_N = 30`, `THIN_RATIO_N = 10`, `GRID_MS: [i64; 18]`, `BIN_CBPS = 2_500`, `TOP_CBPS = 50_000`, `fn g1(&G1Cell, &BodyCtx) -> CardBody`, `fn g2(&DurCell, &BodyCtx) -> CardBody`, `fn g3(&DurCell, &BodyCtx) -> CardBody`, `fn g4(&G4Cell, &BodyCtx) -> CardBody`; `impl Card for G1Cell` (→ G1), `DurCell` (→ G2, G3), `G4Cell` (→ G4).
- Produces (`cards::papers`): `fn papers() -> serde_json::Value`, `fn not_replicated() -> serde_json::Value`, `SAGUILLO_NOTE: &str`.

- [ ] **Step 1: Write the failing tests.** Create `crates/cards/tests/bodies.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::*;
use cards::cells::*;
use cards::gebele::*;
use cards::stats::{CountMap, DurMap};
use serde_json::Value;

fn ctx(base: Status) -> BodyCtx<'static> {
    BodyCtx {
        win: "7d",
        base,
        premise_key: "detector_no=1+e3b0c442;detector_yes=1+e3b0c442;episode_def=1+aa;fees=1+e3b0c442",
        analyze_version: "abc1234",
        from_ms: Some(1_000 * 3_600_000),
        to_ms: Some(1_167 * 3_600_000 + 5),
        since_ms: Some(900 * 3_600_000),
        computed_ms: 1_167 * 3_600_000 + 5,
        scenarios: &[],
    }
}

fn col<'a>(b: &'a CardBody, label: &str) -> &'a Row {
    b.columns.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("no row {label}: {:?}", b.columns))
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn g1_quotes_the_paper_and_counts_both_methods() {
    let c = G1Cell {
        their: PerSide { yes: 1_200, no: 40 },
        gross: PerSide { yes: 1_300, no: 44 },
        ours: PerSide { yes: 1_150, no: 38 },
        flagged: PerSide { yes: 50, no: 2 },
        merged_away: PerSide { yes: [0, 100, 150], no: [0, 3, 5] },
    };
    let b = g1(&c, &ctx(Status::Live));
    assert_eq!((b.card.as_str(), b.paper.as_str(), b.status), ("G1", "gebele-2026", Status::Live));
    let yes = col(&b, "YES-side episodes");
    assert_eq!((yes.paper.clone(), yes.their.clone(), yes.ours.clone()), (s("2098"), s("1200"), s("1150")));
    let ratio = col(&b, "YES : NO");
    assert_eq!((ratio.paper.clone(), ratio.their.clone(), ratio.ours.clone()), (s("58.28"), s("30"), s("30.26")));
    assert_eq!(col(&b, "YES, flicker-merged ≤ 250 ms").ours, s("1050"));
    assert_eq!(col(&b, "NO, flicker-merged ≤ 1000 ms").ours, s("33"));
    assert_eq!(col(&b, "NO, gross Π > 0 (context)").their, s("44"));
    assert_eq!(b.n, 1_240);
    assert_eq!(b.series.bars.iter().map(|x| x.value).collect::<Vec<_>>(), vec![1_150, 38]);
    assert!(b.notes.iter().any(|n| n.contains("52 flagged")), "{:?}", b.notes);
}

#[test]
fn thin_and_empty_samples_never_divide_by_zero() {
    // NO side empty, YES side small: ratios are null (not "0"), the card is thin with reasons
    let c = G1Cell { their: PerSide { yes: 4, no: 0 }, ours: PerSide { yes: 4, no: 0 }, ..G1Cell::default() };
    let b = g1(&c, &ctx(Status::Live));
    assert_eq!(b.status, Status::Thin);
    let ratio = col(&b, "YES : NO");
    assert_eq!((ratio.their.clone(), ratio.ours.clone()), (None, None));
    assert_eq!(col(&b, "NO-side episodes").ours, s("0"), "a measured zero stays \"0\"");
    assert!(b.thin_reasons.iter().any(|r| r.starts_with("NO side: n = 0 < 10")), "{:?}", b.thin_reasons);
    assert!(b.thin_reasons.iter().any(|r| r.starts_with("YES side: n = 4 < 10")));
    // every Gebele card over an empty window
    let empty_d = DurCell::default();
    for body in [g2(&empty_d, &ctx(Status::Live)), g3(&empty_d, &ctx(Status::Live)), g4(&G4Cell::default(), &ctx(Status::Live))] {
        assert_eq!(body.status, Status::Thin, "{}", body.card);
        assert!(!body.thin_reasons.is_empty());
        for r in &body.columns {
            if r.unit == "s" || r.unit == "%" || r.unit == "bps" {
                assert_eq!(r.ours, None, "{} {}", body.card, r.label);
            }
        }
    }
    // warming beats thin: the window is not covered yet, and the reasons still show
    let w = g1(&c, &ctx(Status::Warming));
    assert_eq!(w.status, Status::Warming);
    assert!(!w.thin_reasons.is_empty());
}

#[test]
fn g2_medians_and_survival_curves() {
    let mut their = CountMap::new();
    let mut ours = DurMap::new();
    for i in 1..=40 {
        their.insert(i * 1_000, 1);
        ours.insert(i * 1_000, (1, 0));
    }
    ours.insert(100_000, (0, 3)); // three right-censored at 100 s
    let c = DurCell {
        their: PerSide { yes: their, no: CountMap::new() },
        ours: PerSide { yes: ours, no: DurMap::new() },
        left: PerSide { yes: 2, no: 1 },
        ..DurCell::default()
    };
    let b = g2(&c, &ctx(Status::Live));
    let yes = col(&b, "YES median duration");
    assert_eq!((yes.paper.clone(), yes.their.clone()), (s("16.15"), s("20.5")));
    // KM: 43 at risk; S(t_k) = (43 − k)/43; first S ≤ 1/2 at k = 22 → 22 s
    assert_eq!(yes.ours, s("22"));
    assert_eq!(col(&b, "NO median duration").paper, s("7.99"));
    assert_eq!(col(&b, "NO median duration").ours, None);
    assert_eq!(col(&b, "left-censored, excluded").ours, s("3"));
    assert_eq!(col(&b, "right-censored, kept in Kaplan–Meier").ours, s("3"));
    let km_yes = b.series.curves.iter().find(|c| c.label.starts_with("YES · Kaplan")).unwrap();
    assert_eq!(km_yes.points.len(), GRID_MS.len());
    assert_eq!(km_yes.points[0], CurvePoint { t_ms: 100, s: "1".into() });
    assert!(b.notes.iter().any(|n| n.contains("does not support a reliable distributional comparison")));
    assert!(b.thin_reasons.iter().any(|r| r.starts_with("NO side")), "NO side has no durations");
}

#[test]
fn g3_share_and_the_one_hour_window_bias() {
    let mut ours = DurMap::new();
    for i in 0..30 {
        ours.insert(60_000 + i, (1, 0)); // 30 short episodes
    }
    for i in 0..10 {
        ours.insert(4_000_000 + i, (1, 0)); // 10 of 50+ minutes
    }
    let mut their = CountMap::new();
    for (k, v) in &ours {
        their.insert(*k, v.0);
    }
    let mut clipped = their.clone();
    clipped.insert(3_600_000, 5);
    let c = DurCell {
        their: PerSide { yes: their, no: CountMap::new() },
        ours: PerSide { yes: ours, no: DurMap::new() },
        hour_observed: 624,
        hour_long: 366,
        clipped,
        ..DurCell::default()
    };
    let b = g3(&c, &ctx(Status::Live));
    let share = col(&b, "long-lived share");
    assert_eq!((share.paper.clone(), share.their.clone(), share.ours.clone()), (s("58.7"), s("58.7"), s("25")));
    assert_eq!(col(&b, "episodes observed (per hour)").their, s("624"));
    assert!(col(&b, "median bias from one-hour windows").ours.is_some());
    assert_eq!(b.status, Status::Live);
}

#[test]
fn g4_median_p90_and_bins() {
    let mut net = CountMap::new();
    for i in 1..=40 {
        net.insert(i * 250, 1); // 2.5 bp … 100 bp
    }
    net.insert(60_000, 1); // 600 bp: the open top bin
    let c = G4Cell {
        their: PerSide { yes: 45, no: 0 },
        net: PerSide { yes: net.clone(), no: CountMap::new() },
        gross: PerSide { yes: net, no: CountMap::new() },
    };
    let b = g4(&c, &ctx(Status::Live));
    assert_eq!(col(&b, "YES positive episodes").paper, s("2098"));
    assert_eq!(col(&b, "YES positive episodes").ours, s("41"));
    assert_eq!(col(&b, "YES median net edge").ours, s("52.5")); // rank 21 of 41 → 21 · 2.5 bp
    assert_eq!(col(&b, "YES p90 net edge").ours, s("92.5")); // rank ⌈36.9⌉ = 37
    assert_eq!(col(&b, "NO median net edge").ours, None);
    let yes_bins: Vec<&Bin> = b.series.bins.iter().filter(|x| x.side == "YES").collect();
    assert_eq!(yes_bins.len(), 21);
    assert_eq!((yes_bins[0].lo_bps, yes_bins[0].hi_bps, yes_bins[0].net), (0, Some(25), 9));
    assert_eq!((yes_bins[20].lo_bps, yes_bins[20].hi_bps, yes_bins[20].net), (500, None, 1));
    assert!(b.notes.iter().any(|n| n.contains("figure only")));
}

#[test]
fn status_precedence_and_the_hash_inputs() {
    assert_eq!(base_status(true, Some(0), Some(10)), Status::UnderReview);
    assert_eq!(base_status(false, Some(20), Some(10)), Status::Warming);
    assert_eq!(base_status(false, None, Some(10)), Status::Warming);
    assert_eq!(base_status(false, Some(10), Some(10)), Status::Live);
    assert_eq!(base_status(false, Some(99), None), Status::Live, "all is never warming");
    assert_eq!(final_status(Status::Live, &["x".into()]), Status::Thin);
    assert_eq!(final_status(Status::UnderReview, &["x".into()]), Status::UnderReview);
    let h = repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "p");
    assert_eq!(h.len(), 64);
    assert_eq!(h, repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "p"));
    for other in [
        repro_hash("k2", "v1", Some(0), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v2", Some(0), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v1", Some(3_600_000), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "q"),
    ] {
        assert_ne!(h, other);
    }
    let b = g1(&G1Cell::default(), &ctx(Status::Live));
    // the data range enters the hash by hour
    assert_eq!(
        b.hash,
        repro_hash(ctx(Status::Live).premise_key, "abc1234", Some(1_000 * 3_600_000), Some(1_167 * 3_600_000), "G1", "7d", "merge_gaps_ms=0,250,1000;thin_ratio_n=10")
    );
}

#[test]
fn bodies_carry_decimals_as_strings_or_null_never_numbers() {
    let d = DurCell::default();
    for b in G1Cell::bodies(&G1Cell::default(), &(), &ctx(Status::Live))
        .into_iter()
        .chain(DurCell::bodies(&d, &(), &ctx(Status::Live)))
        .chain(G4Cell::bodies(&G4Cell::default(), &(), &ctx(Status::Live)))
    {
        let v = serde_json::to_value(&b).unwrap();
        for r in v["columns"].as_array().unwrap() {
            for k in ["paper", "their", "ours"] {
                assert!(r[k].is_string() || r[k].is_null(), "{} {}: {}", b.card, k, r[k]);
            }
        }
        assert_eq!(v["status"], "thin");
        let back: CardBody = serde_json::from_value(v).unwrap();
        assert_eq!(back, b);
    }
}

#[test]
fn the_papers_list_and_the_not_replicated_box() {
    let p = cards::papers::papers();
    let arr = p.as_array().unwrap();
    let find = |id: &str| arr.iter().find(|x| x["id"] == id).unwrap().clone();
    assert_eq!(find("gebele-2026")["status"], "live");
    assert_eq!(find("saguillo")["status"], "coming_later");
    assert_eq!(
        find("saguillo")["note"],
        Value::String("coming later: needs the on-chain dataset and somewhere to put it that is not your laptop".into())
    );
    assert_eq!(find("cheng")["status"], "dropped");
    let nr = cards::papers::not_replicated();
    assert_eq!(nr.as_array().unwrap().len(), 2);
    assert!(nr.to_string().contains("308,416,666"));
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test bodies`
Expected: compile errors `unresolved import cards::body`, `cards::gebele`, `cards::papers`.

- [ ] **Step 3: Write the body module.** Create `crates/cards/src/body.rs`:

```rust
//! The card payload (spec §8.1): analyze stores it in card_results.body and the api serves it as
//! stored. Numbers that can be money, rates or durations are decimal strings or null; counts are
//! integers.
use crate::cells::CellData;
use crate::hour_of;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub const WINS: [&str; 5] = ["24h", "7d", "30d", "all", "today"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Live,
    Warming,
    Thin,
    UnderReview,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Live => "live",
            Status::Warming => "warming",
            Status::Thin => "thin",
            Status::UnderReview => "under_review",
        }
    }
}

/// One line of the three-column table: PAPER | THEIR METHOD | OUR METHOD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub label: String,
    pub unit: String,
    pub paper: Option<String>,
    pub their: Option<String>,
    pub ours: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub t_ms: i64,
    pub s: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Curve {
    pub label: String,
    pub points: Vec<CurvePoint>,
    pub median_ms: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bin {
    pub side: String,
    pub lo_bps: i64,
    /// None: the open top bin.
    pub hi_bps: Option<i64>,
    pub net: i64,
    pub gross: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecayPoint {
    pub scenario: String,
    /// proc + net + match of the scenario (policies.toml).
    pub latency_ms: i64,
    pub net_ms: i64,
    pub captured: Option<String>,
    pub rate_pct: Option<String>,
    pub headline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measured {
    pub latency_ms: i64,
    pub host: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Series {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bars: Vec<Bar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curves: Vec<Curve>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<Bin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decay: Vec<DecayPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Measured>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardBody {
    pub card: String,
    pub paper: String,
    pub title: String,
    pub win: String,
    pub status: Status,
    pub premise_key: String,
    pub hash: String,
    pub data_from_ms: Option<i64>,
    pub data_to_ms: Option<i64>,
    /// When the current premise key's data starts ("since ⟨date⟩ · ⟨x⟩ days of data").
    pub since_ms: Option<i64>,
    pub n: i64,
    pub columns: Vec<Row>,
    pub series: Series,
    pub notes: Vec<String>,
    pub thin_reasons: Vec<String>,
    pub episode_link: Option<String>,
    pub computed_ms: i64,
}

/// A latency scenario of policies.toml, as the Edge capture decay chart plots it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioLat {
    pub name: String,
    pub net_ms: i64,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct BodyCtx<'a> {
    pub win: &'a str,
    /// under_review / warming / live, before the card's own thin check.
    pub base: Status,
    pub premise_key: &'a str,
    pub analyze_version: &'a str,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub since_ms: Option<i64>,
    pub computed_ms: i64,
    pub scenarios: &'a [ScenarioLat],
}

/// A cell family that renders cards from a window's merged cell.
pub trait Card: CellData {
    fn bodies(agg: &Self, max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody>;
}

/// Spec §4: under_review while catch-up runs; warming when the key's data starts inside the
/// window (or no data yet); otherwise live. `window_start_ms` None is `all` (never warming).
pub fn base_status(catching_up: bool, since_ms: Option<i64>, window_start_ms: Option<i64>) -> Status {
    if catching_up {
        return Status::UnderReview;
    }
    match (since_ms, window_start_ms) {
        (None, _) => Status::Warming,
        (Some(since), Some(start)) if since > start => Status::Warming,
        _ => Status::Live,
    }
}

/// Precedence: under_review, warming, thin, live. Thin reasons show whatever the status.
pub fn final_status(base: Status, thin: &[String]) -> Status {
    if base == Status::Live && !thin.is_empty() {
        Status::Thin
    } else {
        base
    }
}

/// sha256 of the premise key, the analyze version, the window's data range (first and last hour),
/// the card, the window and the card's parameter set (spec §4).
pub fn repro_hash(
    premise_key: &str,
    analyze_version: &str,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    card: &str,
    win: &str,
    params: &str,
) -> String {
    let hour = |v: Option<i64>| v.map(|x| hour_of(x).to_string()).unwrap_or_default();
    let text = format!(
        "{premise_key}\n{analyze_version}\n{}\n{}\n{card}\n{win}\n{params}",
        hour(from_ms),
        hour(to_ms)
    );
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(text.as_bytes()) {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub fn row(label: &str, unit: &str, paper: Option<&str>, their: Option<String>, ours: Option<String>) -> Row {
    Row {
        label: label.to_string(),
        unit: unit.to_string(),
        paper: paper.map(str::to_string),
        their,
        ours,
    }
}

/// The parts of a card that differ per card.
pub struct Parts {
    pub card: &'static str,
    pub paper: &'static str,
    pub title: &'static str,
    pub params: String,
    pub n: i64,
    pub columns: Vec<Row>,
    pub series: Series,
    pub notes: Vec<String>,
    pub thin: Vec<String>,
    pub episode_link: Option<String>,
}

pub fn make(p: Parts, ctx: &BodyCtx<'_>) -> CardBody {
    CardBody {
        card: p.card.to_string(),
        paper: p.paper.to_string(),
        title: p.title.to_string(),
        win: ctx.win.to_string(),
        status: final_status(ctx.base, &p.thin),
        premise_key: ctx.premise_key.to_string(),
        hash: repro_hash(ctx.premise_key, ctx.analyze_version, ctx.from_ms, ctx.to_ms, p.card, ctx.win, &p.params),
        data_from_ms: ctx.from_ms,
        data_to_ms: ctx.to_ms,
        since_ms: ctx.since_ms,
        n: p.n,
        columns: p.columns,
        series: p.series,
        notes: p.notes,
        thin_reasons: p.thin,
        episode_link: p.episode_link,
        computed_ms: ctx.computed_ms,
    }
}
```

(The interface line above names `make(…)`: its exact signature is `pub fn make(p: Parts, ctx: &BodyCtx<'_>) -> CardBody` with `Parts` as defined here.)

- [ ] **Step 4: Write the Gebele cards.** Create `crates/cards/src/gebele.rs`:

```rust
//! Gebele, Mutzel, Matthes, "Executable Arbitrage and Market Efficiency in Prediction Markets",
//! arXiv 2608.00666 v1 (read 2026-09-26). Cards G1–G4 (spec §5): PAPER is their number, THEIR
//! METHOD their definitions on our data, OUR METHOD our headline definitions.
use crate::body::{Bar, Bin, BodyCtx, Card, CardBody, Curve, CurvePoint, Parts, Row, Series, make, row};
use crate::cells::{DurCell, G1Cell, G4Cell, LONG_MS, MERGE_GAPS_MS, Side};
use crate::stats::{bins, dec, dur_total, emp_at, fp_to_dec, km, median_of, merge_sorted, nearest_rank, pct, secs, total};
use rust_decimal::Decimal;

pub const PAPER_ID: &str = "gebele-2026";
/// Spec §4: fewer than 30 uncensored durations for a median, fewer than 10 per side for a ratio.
pub const THIN_MEDIAN_N: i64 = 30;
pub const THIN_RATIO_N: i64 = 10;
/// Survival curve points (ms): 0.1 s to 24 h, roughly log-spaced.
pub const GRID_MS: [i64; 18] = [
    100, 250, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 30_000, 60_000, 120_000, 300_000, 600_000,
    1_800_000, 3_600_000, 7_200_000, 21_600_000, 86_400_000,
];
/// G4 bins: 25 bp wide in 0.01-bp units, the last one open from 500 bp.
pub const BIN_CBPS: i64 = 2_500;
pub const TOP_CBPS: i64 = 50_000;

const PAPER_SAMPLE: &str = "PAPER: CLOB sample, hour-stratified, 14 Apr 2026 00:00 UTC to 19 May 2026 06:00 UTC; \
     an episode is a maximal run with the fee-adjusted edge strictly > 0, taker execution, no flicker merge.";

fn count(v: i64) -> Option<String> {
    Some(v.to_string())
}

fn ratio(a: i64, b: i64) -> Option<String> {
    (b > 0).then(|| dec((Decimal::from(a) / Decimal::from(b)).round_dp(2)))
}

fn sides() -> [(&'static str, Side); 2] {
    [("YES", Side::Yes), ("NO", Side::No)]
}

pub fn g1(c: &G1Cell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    for (name, side) in sides() {
        let n = *c.ours.get(side);
        if n < THIN_RATIO_N {
            thin.push(format!("{name} side: n = {n} < {THIN_RATIO_N}, so the ratio is not reliable"));
        }
    }
    let mut columns = vec![
        row("YES-side episodes", "count", Some("2098"), count(c.their.yes), count(c.ours.yes)),
        row("NO-side episodes", "count", Some("36"), count(c.their.no), count(c.ours.no)),
        row("YES : NO", "×", Some("58.28"), ratio(c.their.yes, c.their.no), ratio(c.ours.yes, c.ours.no)),
    ];
    for (i, g) in MERGE_GAPS_MS.iter().enumerate() {
        for (name, side) in sides() {
            let merged = c.ours.get(side) - c.merged_away.get(side).get(i).copied().unwrap_or(0);
            columns.push(row(&format!("{name}, flicker-merged ≤ {g} ms"), "count", None, None, count(merged)));
        }
    }
    for (name, side) in sides() {
        columns.push(row(&format!("{name}, gross Π > 0 (context)"), "count", None, count(*c.gross.get(side)), None));
    }
    let notes = vec![
        PAPER_SAMPLE.to_string(),
        format!(
            "THEIR METHOD: our raw live episodes, counted per side. OUR METHOD drops {} flagged episodes \
             (engine_suspect or stale_transport) and adds the merge-gap sensitivity.",
            c.flagged.yes + c.flagged.no
        ),
    ];
    let series = Series {
        bars: vec![
            Bar { label: "YES".into(), value: c.ours.yes },
            Bar { label: "NO".into(), value: c.ours.no },
        ],
        ..Series::default()
    };
    make(
        Parts {
            card: "G1",
            paper: PAPER_ID,
            title: "G1 · Side asymmetry",
            params: "merge_gaps_ms=0,250,1000;thin_ratio_n=10".into(),
            n: c.their.yes + c.their.no,
            columns,
            series,
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

fn grid_params() -> String {
    let g: Vec<String> = GRID_MS.iter().map(i64::to_string).collect();
    format!("grid_ms={};thin_median_n={THIN_MEDIAN_N}", g.join(","))
}

pub fn g2(c: &DurCell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let mut notes = vec![format!(
        "{PAPER_SAMPLE} Window-spanning episodes excluded; Ŝ(τ) = K⁻¹ Σ 𝟙{{D_k > τ}} over uncensored episodes. \
         On the NO side (n = 5) the paper says this “does not support a reliable distributional comparison”."
    )];
    let mut columns: Vec<Row> = Vec::new();
    let mut curves = Vec::new();
    let mut n = 0;
    for ((name, side), (paper_med, paper_n)) in sides().into_iter().zip([("16.15", "253"), ("7.99", "5")]) {
        let their = c.their.get(side);
        let ours = c.ours.get(side);
        let n_their = total(their);
        n += n_their;
        let (ev, ce) = dur_total(ours);
        let k = km(ours.iter().map(|(t, v)| (*t, *v)), &GRID_MS);
        let emp_med = median_of(their.iter().map(|(v, c)| (*v, *c)), n_their);
        if n_their < THIN_MEDIAN_N {
            thin.push(format!("{name} side: {n_their} uncensored durations < {THIN_MEDIAN_N}; THEIR median is not reliable"));
        }
        if ev < THIN_MEDIAN_N {
            thin.push(format!("{name} side: {ev} events < {THIN_MEDIAN_N}; the Kaplan–Meier median is not reliable"));
        }
        if ev > 0 && k.median.is_none() {
            notes.push(format!("{name} side: the Kaplan–Meier median is not reached (survival never falls to 1/2)."));
        }
        columns.push(row(
            &format!("{name} median duration"),
            "s",
            Some(paper_med),
            emp_med.map(secs),
            k.median.map(|m| secs(Decimal::from(m))),
        ));
        columns.push(row(&format!("{name} durations (n)"), "count", Some(paper_n), count(n_their), count(ev + ce)));
        if ev + ce > 0 {
            curves.push(Curve {
                label: format!("{name} · Kaplan–Meier (ours)"),
                points: GRID_MS
                    .iter()
                    .zip(&k.at)
                    .map(|(t, v)| CurvePoint { t_ms: *t, s: dec(fp_to_dec(*v).round_dp(4)) })
                    .collect(),
                median_ms: k.median.map(|m| m.to_string()),
            });
        }
        if n_their > 0 {
            curves.push(Curve {
                label: format!("{name} · empirical (theirs)"),
                points: GRID_MS
                    .iter()
                    .zip(emp_at(their, &GRID_MS))
                    .map(|(t, v)| CurvePoint { t_ms: *t, s: v.map(|x| dec(x.round_dp(4))).unwrap_or_default() })
                    .collect(),
                median_ms: emp_med.map(dec),
            });
        }
    }
    let right = dur_total(&c.ours.yes).1 + dur_total(&c.ours.no).1;
    let gap_n = total(&c.gap.yes) + total(&c.gap.no);
    let gap_med = median_of(merge_sorted(&c.gap.yes, &c.gap.no, |a, b| a + b), gap_n);
    columns.push(row("right-censored, kept in Kaplan–Meier", "count", None, None, count(right)));
    columns.push(row("left-censored, excluded", "count", None, None, count(c.left.yes + c.left.no)));
    columns.push(row("gap-censored, reported apart", "count", None, None, count(gap_n)));
    columns.push(row("gap-censored median span (upper bound)", "s", None, None, gap_med.map(secs)));
    notes.push(format!(
        "OUR METHOD: Kaplan–Meier with right-censored episodes; left-censored excluded and counted; gap-censored \
         reported apart (their recorded span is an upper bound). Drops {} flagged and {} still-open episodes.",
        c.flagged.yes + c.flagged.no,
        c.open.yes + c.open.no
    ));
    make(
        Parts {
            card: "G2",
            paper: PAPER_ID,
            title: "G2 · Durations and survival",
            params: grid_params(),
            n,
            columns,
            series: Series { curves, ..Series::default() },
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

pub fn g3(c: &DurCell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let their_share = pct(c.hour_long, c.hour_observed);
    if c.hour_observed == 0 {
        thin.push("no episode was observed in any hour of the window".to_string());
    }
    let all = merge_sorted(&c.ours.yes, &c.ours.no, |a, b| (a.0 + b.0, a.1 + b.1));
    let k = km(all, &[LONG_MS - 1]);
    if k.events < THIN_MEDIAN_N {
        thin.push(format!("{} events < {THIN_MEDIAN_N}; the Kaplan–Meier share is not reliable", k.events));
    }
    let ours_share = (k.events > 0)
        .then(|| k.at.first().map(|s| (fp_to_dec(*s) * Decimal::from(100)).round_dp(1)))
        .flatten();
    let clip_n = total(&c.clipped);
    let true_n = total(&c.their.yes) + total(&c.their.no);
    let clip_med = median_of(c.clipped.iter().map(|(v, n)| (*v, *n)), clip_n);
    let true_med = median_of(merge_sorted(&c.their.yes, &c.their.no, |a, b| a + b), true_n);
    let bias = match (clip_med, true_med) {
        (Some(a), Some(b)) => Some(secs(a - b)),
        _ => None,
    };
    let columns = vec![
        row("long-lived share", "%", Some("58.7"), their_share.map(dec), ours_share.map(dec)),
        row("episodes observed (per hour)", "count", Some("624"), count(c.hour_observed), None),
        row("long-lived (first and last 5 min)", "count", Some("366"), count(c.hour_long), None),
        row("median bias from one-hour windows", "s", None, None, bias),
    ];
    let notes = vec![
        "PAPER: “Of the 624 observed episodes, 366 (58.7%)” were observed in both the first and the final five \
         minutes of their one-hour window (≥ 50 minutes). The paper does not state how its hours were chosen."
            .to_string(),
        "THEIR METHOD: our continuous record cut into clock hours; an episode spanning several hours is observed \
         in each."
            .to_string(),
        "OUR METHOD: the share of durations ≥ 50 min by Kaplan–Meier (right-censored kept), and the bias one-hour \
         windows introduce: the median of hour-clipped durations minus the median of true durations."
            .to_string(),
    ];
    make(
        Parts {
            card: "G3",
            paper: PAPER_ID,
            title: "G3 · Long-lived share",
            params: format!("long_ms={LONG_MS};edge_min_ms=300000;thin_median_n={THIN_MEDIAN_N}"),
            n: c.hour_observed,
            columns,
            series: Series::default(),
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

fn bps(centi: i64) -> String {
    dec(Decimal::new(centi, 2))
}

pub fn g4(c: &G4Cell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let mut columns = vec![
        row("YES positive episodes", "count", Some("2098"), count(c.their.yes), count(total(&c.net.yes))),
        row("NO positive episodes", "count", Some("36"), count(c.their.no), count(total(&c.net.no))),
    ];
    let mut series = Series::default();
    let mut below_all = 0;
    for (name, side) in sides() {
        let net = c.net.get(side);
        let n = total(net);
        if n < THIN_MEDIAN_N {
            thin.push(format!("{name} side: n = {n} < {THIN_MEDIAN_N}; the median and p90 are not reliable"));
        }
        let q = |p: i64| nearest_rank(net.iter().map(|(v, k)| (*v, *k)), n, p).map(bps);
        columns.push(row(&format!("{name} median net edge"), "bps", None, None, q(50)));
        columns.push(row(&format!("{name} p90 net edge"), "bps", None, None, q(90)));
        let (below, net_bins) = bins(net, BIN_CBPS, TOP_CBPS);
        let (_, gross_bins) = bins(c.gross.get(side), BIN_CBPS, TOP_CBPS);
        below_all += below;
        for ((lo, k), (_, g)) in net_bins.into_iter().zip(gross_bins) {
            series.bins.push(Bin {
                side: name.to_string(),
                lo_bps: lo / 100,
                hi_bps: (lo < TOP_CBPS).then_some((lo + BIN_CBPS) / 100),
                net: k,
                gross: g,
            });
        }
    }
    let mut notes = vec![
        "PAPER: Figure 2 shows the distributions with insets of positive and negative counts; the text states no \
         units, bins or percentiles, so only the counts are quoted (figure only)."
            .to_string(),
        "OUR METHOD: peak top-of-book net edge per side, rounded to 0.01 bp; 25 bp bins, the last open from 500 bp; \
         gross as the overlay; nearest-rank median and p90; flagged episodes dropped."
            .to_string(),
    ];
    if below_all > 0 {
        notes.push(format!("{below_all} episodes had a negative peak edge and sit below the first bin."));
    }
    make(
        Parts {
            card: "G4",
            paper: PAPER_ID,
            title: "G4 · Net edge by side",
            params: format!("bin_bps=25;top_bps=500;quantum_bp=0.01;thin_n={THIN_MEDIAN_N}"),
            n: c.their.yes + c.their.no,
            columns,
            series,
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

impl Card for G1Cell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g1(agg, ctx)]
    }
}

impl Card for DurCell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g2(agg, ctx), g3(agg, ctx)]
    }
}

impl Card for G4Cell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g4(agg, ctx)]
    }
}
```

- [ ] **Step 5: Write the papers list.** Create `crates/cards/src/papers.rs`:

```rust
//! What `GET /api/cards/meta` lists besides the tick (spec §8.1, §5.5). analyze writes these to
//! card_meta at startup, so the api stays a pass-through.
use serde_json::{Value, json};

pub const SAGUILLO_NOTE: &str =
    "coming later: needs the on-chain dataset and somewhere to put it that is not your laptop";

pub fn papers() -> Value {
    json!([
        {
            "id": "gebele-2026",
            "cite": "Gebele, Mutzel, Matthes (2026). Executable Arbitrage and Market Efficiency in Prediction Markets. arXiv 2608.00666 v1.",
            "status": "live",
            "note": null,
            "cards": ["G1", "G2", "G3", "G4"]
        },
        { "id": "saguillo", "cite": "Saguillo et al.", "status": "coming_later", "note": SAGUILLO_NOTE, "cards": [] },
        { "id": "laya", "cite": "Laya et al.", "status": "coming_later", "note": "Phase 6.", "cards": [] },
        { "id": "cheng", "cite": "Cheng et al.", "status": "dropped", "note": "dropped: the owner's call, not a data problem", "cards": [] }
    ])
}

pub fn not_replicated() -> Value {
    json!([
        {
            "item": "Converter profits",
            "why": "They come from on-chain conversions, which we don't observe."
        },
        {
            "item": "Same-condition findings of the WebSocket validation appendix (20 inconsistencies in 308,416,666 messages)",
            "why": "They need the on-chain side too. The Health page reports our own integrity rate instead."
        }
    ])
}
```

In `crates/cards/src/lib.rs`, add `pub mod body;`, `pub mod gebele;`, `pub mod papers;`.

- [ ] **Step 6: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards`
Expected: PASS: 8 tests in `bodies.rs` plus the earlier cards tests.

- [ ] **Step 7: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p cards --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/cards/src/lib.rs crates/cards/src/body.rs crates/cards/src/gebele.rs crates/cards/src/papers.rs crates/cards/tests/bodies.rs && git commit -m "cards: G1-G4 bodies (PAPER / THEIR / OUR columns), status, reproducibility hash, papers list"
```

---

### Task 8: `cards`: Edge capture (on the table, the split, captured per scenario, capture rate, capital overlap, biggest opportunity, measured marker)

**Files:**
- Create: `crates/cards/src/edge.rs`, `crates/cards/tests/edge.rs`
- Modify: `crates/cards/src/lib.rs` (`pub mod edge;`)

**Interfaces:**
- Consumes: `cards::cells::{CellData, Ep, HourInput, Num, PerSide, Side, opened_in, overlap}`, `cards::body::{BodyCtx, Card, CardBody, DecayPoint, Measured, Parts, Series, ScenarioLat, make, row}`, `cards::stats`, `cards::window::Windows` (Tasks 5–7).
- Produces (`cards::edge`):
  - consts `CAP_USD: i64 = 100`, `HEADLINE: &str = "sp-idle"`, `THIN_EDGE_N: i64 = 10`, `ON_TABLE: &str = "edge on the table"`, `UPPER_BOUND: &str = "upper bound: assumes first-in-line execution; CLOB queue ignored"`
  - `struct Biggest { usd: Num, episode_id: String }`, `struct EdgeMax { capital: Decimal, biggest: Option<(Decimal, String)> }`
  - `struct EdgeCell { table: PerSide<Num>, n: PerSide<i64>, captured: BTreeMap<String, (i64, Num)>, lat: CountMap, max_capital: Num, biggest: Option<Biggest> }` — `CellData` with `FAMILY = "edge"`, `AREAS = ["depth", "detector_no", "detector_yes", "episode_def", "fees", "host", "sim"]`, `EDGE_INPUTS = true`, `Max = EdgeMax`; `Card` → one `EDGE` body.
  - `fn edge(c: &EdgeCell, m: &EdgeMax, ctx: &BodyCtx<'_>) -> CardBody`, `fn host_from_key(key: &str) -> Option<&str>`

- [ ] **Step 1: Write the failing tests.** Create `crates/cards/tests/edge.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::body::{BodyCtx, Card, ScenarioLat, Status};
use cards::cells::*;
use cards::edge::*;
use cards::window::Windows;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const M: i64 = 60_000;

fn ep(id: &str, side: Side, opened: i64, end: i64, b100: Option<Decimal>, cost: Decimal) -> Ep {
    Ep {
        id: id.into(), run_id: 1, side, opened_ms: opened, closed_ms: Some(end), end_ms: end,
        duration_ms: Some(end - opened), censor_left: false, censor_right: false, censor_gap: false,
        edge_net: dec!(0.004), edge_gross: dec!(0.01), pnl_net: dec!(0.5), pnl_gross: dec!(1),
        cost, budget100: b100, suspect: false, stale: false, prev_gap_ms: None,
    }
}

fn build(h: i64, eps: &[Ep], lat: &[i64], deltas: &[(String, Decimal)]) -> EdgeCell {
    EdgeCell::build(&HourInput { hour_ms: h, episodes: eps, latency_ms: lat, equity_deltas: deltas })
}

fn scenarios() -> Vec<ScenarioLat> {
    [("colo-idle", 1, 52), ("sp-idle", 145, 300), ("sp-burst", 145, 450), ("sp-idle-nodelay", 145, 300)]
        .iter()
        .map(|(n, net, lat)| ScenarioLat { name: n.to_string(), net_ms: *net, latency_ms: *lat })
        .collect()
}

fn ctx<'a>(sc: &'a [ScenarioLat]) -> BodyCtx<'a> {
    BodyCtx {
        win: "24h",
        base: Status::Live,
        premise_key: "depth=1+e3b0c442;detector_no=1+e3b0c442;detector_yes=1+e3b0c442;episode_def=1+aa;fees=1+e3b0c442;host=1:vps-kvm2|100-250;sim=1+bb",
        analyze_version: "abc1234",
        from_ms: Some(0),
        to_ms: Some(H),
        since_ms: Some(0),
        computed_ms: H,
        scenarios: sc,
    }
}

#[test]
fn the_split_the_sums_and_the_biggest_opportunity() {
    let mut flagged = ep("f", Side::Yes, 5, 6, Some(dec!(9)), dec!(50));
    flagged.stale = true;
    let mut negative = ep("neg", Side::No, 7, 8, Some(dec!(-1)), dec!(50));
    negative.pnl_net = dec!(-0.01);
    let eps = vec![
        ep("n1", Side::No, 0, 10, Some(dec!(0.33)), dec!(99.59)),
        ep("n2", Side::No, 1, 11, Some(dec!(1.2)), dec!(40)),
        ep("y1", Side::Yes, 2, 12, Some(dec!(2.5)), dec!(100)),
        ep("y0", Side::Yes, 3, 13, None, dec!(10)), // no capped P&L: not on the table
        flagged,
        negative,
    ];
    let c = build(0, &eps, &[], &[]);
    assert_eq!((c.table.no.0, c.table.yes.0), (dec!(1.53), dec!(2.5)));
    assert_eq!((c.n.no, c.n.yes), (2, 1));
    let b = c.biggest.clone().unwrap();
    assert_eq!((b.usd.0, b.episode_id.as_str()), (dec!(2.5), "y1"));
    let json = c.to_json().unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["table"]["no"], "1.53", "money is a JSON string: {json}");
    assert_eq!(EdgeCell::from_json(&json).unwrap(), c);
}

#[test]
fn capital_overlap_counts_an_episode_opened_hours_earlier() {
    // A: 00:30 → 03:10, $80. B: 02:05 → 02:20, $150 capped to $100.
    // C closes at 02:30 exactly when D opens: never both at once.
    let eps = vec![
        ep("a", Side::No, 30 * M, 3 * H + 10 * M, Some(dec!(0.1)), dec!(80)),
        ep("b", Side::No, 2 * H + 5 * M, 2 * H + 20 * M, Some(dec!(0.2)), dec!(150)),
        ep("c", Side::No, 2 * H + 25 * M, 2 * H + 30 * M, Some(dec!(0.1)), dec!(15)),
        ep("d", Side::No, 2 * H + 30 * M, 2 * H + 40 * M, Some(dec!(0.1)), dec!(19)),
    ];
    let cells: Vec<EdgeCell> = (0..4).map(|i| build(i * H, &eps, &[], &[])).collect();
    let caps: Vec<Decimal> = cells.iter().map(|c| c.max_capital.0).collect();
    assert_eq!(caps, vec![dec!(80), dec!(80), dec!(180), dec!(80)]);
    assert_eq!(cells[2].max_part().capital, dec!(180));
    let mut w: Windows<EdgeCell> = Windows::new(3 * H);
    for (i, c) in cells.iter().enumerate() {
        w.apply(i64::try_from(i).unwrap() * H, None, c);
    }
    let m = w.max_from(0);
    assert_eq!(m.capital, dec!(180));
    assert_eq!(m.biggest.unwrap().1, "b");
    assert_eq!(w.max_from(3 * H).capital, dec!(80));
}

#[test]
fn capture_rate_is_null_when_nothing_was_on_the_table() {
    let sc = scenarios();
    let c = build(0, &[], &[], &[("sp-idle".to_string(), dec!(0)), ("sp-idle".to_string(), dec!(0))]);
    assert_eq!(c.captured.get("sp-idle"), Some(&(2, Num(dec!(0)))));
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    assert_eq!(b.status, Status::Thin);
    let rate = b.columns.iter().find(|r| r.label == "capture rate, sp-idle").unwrap();
    assert_eq!(rate.ours, None);
    let cap = b.columns.iter().find(|r| r.label == "captured at sp-idle (145 ms)").unwrap();
    assert_eq!(cap.ours.as_deref(), Some("0"), "marks existed: a measured zero");
    let on = b.columns.iter().find(|r| r.label == ON_TABLE).unwrap();
    assert_eq!(on.ours.as_deref(), Some("0"));
    assert!(b.thin_reasons.iter().any(|r| r.contains("nothing was on the table")));
    let colo = b.series.decay.iter().find(|p| p.scenario == "colo-idle").unwrap();
    assert_eq!((colo.captured.clone(), colo.rate_pct.clone()), (None, None), "no marks: not measured");
}

#[test]
fn the_edge_body_has_rows_decay_points_and_the_measured_marker() {
    let sc = scenarios();
    let eps: Vec<Ep> = (0..12)
        .map(|i| {
            let b100 = if i < 10 { dec!(1) } else { dec!(1.25) }; // Σ = 12.5
            ep(&format!("e{i:02}"), if i % 3 == 0 { Side::Yes } else { Side::No }, i * M, i * M + 1_000, Some(b100), dec!(50))
        })
        .collect();
    let deltas = vec![
        ("sp-idle".to_string(), dec!(1)),
        ("sp-idle".to_string(), dec!(0.25)),
        ("colo-idle".to_string(), dec!(3.1)),
    ];
    let c = build(0, &eps, &[150, 210, 200], &deltas);
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    assert_eq!((b.card.as_str(), b.paper.as_str(), b.status, b.n), ("EDGE", "edge", Status::Live, 12));
    let get = |l: &str| b.columns.iter().find(|r| r.label == l).unwrap().ours.clone();
    assert_eq!(get(ON_TABLE).as_deref(), Some("12.5"));
    assert_eq!(get("biggest single opportunity").as_deref(), Some("1.25"));
    assert_eq!(get("captured at sp-idle (145 ms)").as_deref(), Some("1.25"));
    assert_eq!(get("capture rate, sp-idle").as_deref(), Some("10"));
    assert_eq!(get("capital needed at once").as_deref(), Some("50"));
    assert_eq!(get("measured latency (estimate)").as_deref(), Some("200"));
    let head: Vec<&str> = b.series.decay.iter().filter(|p| p.headline).map(|p| p.scenario.as_str()).collect();
    assert_eq!(head, ["sp-idle"]);
    let colo = b.series.decay.iter().find(|p| p.scenario == "colo-idle").unwrap();
    assert_eq!((colo.latency_ms, colo.rate_pct.as_deref()), (52, Some("24.8")));
    let m = b.series.measured.clone().unwrap();
    assert_eq!((m.latency_ms, m.host.as_str()), (200, "vps-kvm2"));
    assert!(b.notes.iter().any(|n| n == UPPER_BOUND));
    assert_eq!(b.episode_link.as_deref(), Some("e10"), "ties go to the smaller id");
    assert_eq!(host_from_key("fees=1+a;host=1:pc-dev|lt25;sim=1+b"), Some("pc-dev"));
    assert_eq!(host_from_key("fees=1+a"), None);
}

const HOURS: i64 = 5;

fn arb_ep() -> impl Strategy<Value = Ep> {
    (0..HOURS * H, 0..2 * H, any::<bool>(), 0i64..300, 1i64..200, any::<bool>(), -3i64..5).prop_map(
        |(opened, dur, yes, b100_c, cost, flag, pnl)| {
            let mut e = ep(
                &format!("e{opened}-{dur}-{b100_c}"),
                if yes { Side::Yes } else { Side::No },
                opened,
                opened + dur,
                (b100_c > 0).then(|| Decimal::new(b100_c, 2)),
                Decimal::from(cost),
            );
            e.suspect = flag;
            e.pnl_net = Decimal::new(pnl, 1);
            e
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]
    #[test]
    fn edge_cells_sum_to_the_direct_figures_and_the_max_is_a_whole_window_sweep(
        eps in proptest::collection::vec(arb_ep(), 0..40),
        deltas in proptest::collection::vec((0..HOURS, 0i64..3, -500i64..500), 0..30),
    ) {
        let names = ["colo-idle", "sp-idle", "sp-burst"];
        let mut sum = EdgeCell::default();
        let mut maxes = EdgeMax::default();
        for i in 0..HOURS {
            let h = i * H;
            let view: Vec<Ep> = eps.iter().filter(|e| e.opened_ms < h + H && e.end_ms >= h).cloned().collect();
            let d: Vec<(String, Decimal)> = deltas.iter().filter(|x| x.0 == i).map(|x| (names[usize::try_from(x.1).unwrap()].to_string(), Decimal::new(x.2, 2))).collect();
            let c = build(h, &view, &[], &d);
            EdgeCell::merge_max(&mut maxes, &c.max_part());
            sum.add(&c);
        }
        let counted: Vec<&Ep> = eps.iter().filter(|e| e.pnl_net > Decimal::ZERO && !e.suspect && e.budget100.is_some()).collect();
        let table_no: Decimal = counted.iter().filter(|e| e.side == Side::No).map(|e| e.budget100.unwrap()).sum();
        prop_assert_eq!(sum.table.no.0, table_no);
        prop_assert_eq!(sum.n.yes + sum.n.no, i64::try_from(counted.len()).unwrap());
        for (k, name) in names.iter().enumerate() {
            let mine: Vec<Decimal> = deltas.iter().filter(|x| x.1 == i64::try_from(k).unwrap()).map(|x| Decimal::new(x.2, 2)).collect();
            let got = sum.captured.get(*name).map(|(n, v)| (*n, v.0));
            let want = (!mine.is_empty()).then(|| (i64::try_from(mine.len()).unwrap(), mine.iter().copied().sum::<Decimal>()));
            prop_assert_eq!(got, want);
        }
        // the whole-window sweep: capped costs of concurrently open counted episodes
        let mut ev: Vec<(i64, u8, Decimal)> = Vec::new();
        for e in &counted {
            let (s, t) = (e.opened_ms, e.end_ms.min(HOURS * H));
            if s < t {
                let cost = e.cost.min(Decimal::from(100));
                ev.push((s, 1, cost));
                ev.push((t, 0, -cost));
            }
        }
        ev.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let (mut run, mut max) = (Decimal::ZERO, Decimal::ZERO);
        for (_, _, d) in ev {
            run += d;
            max = max.max(run);
        }
        prop_assert_eq!(maxes.capital, max);
        let best = counted.iter().map(|e| (e.budget100.unwrap(), e.id.clone())).fold(None::<(Decimal, String)>, |acc, x| match acc {
            Some(a) if a.0 > x.0 || (a.0 == x.0 && a.1 <= x.1) => Some(a),
            _ => Some(x),
        });
        prop_assert_eq!(maxes.biggest, best);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test edge`
Expected: compile error `unresolved import cards::edge`.

- [ ] **Step 3: Write the module.** Create `crates/cards/src/edge.rs`:

```rust
//! Edge capture (spec §6): the edge that was measurably on the table versus what each latency
//! scenario's paper account captured. Opportunities are measured live; the only hypothetical is
//! execution (first in line at the best moment).
use crate::body::{BodyCtx, Card, CardBody, DecayPoint, Measured, Parts, Series, make, row};
use crate::cells::{CellData, Ep, HourInput, Num, PerSide, opened_in, overlap};
use crate::stats::{self, CountMap, dec};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CAP_USD: i64 = 100;
pub const HEADLINE: &str = "sp-idle";
pub const THIN_EDGE_N: i64 = 10;
pub const ON_TABLE: &str = "edge on the table";
pub const UPPER_BOUND: &str = "upper bound: assumes first-in-line execution; CLOB queue ignored";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Biggest {
    pub usd: Num,
    pub episode_id: String,
}

/// The two Edge figures a window can't get by adding cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdgeMax {
    pub capital: Decimal,
    pub biggest: Option<(Decimal, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeCell {
    /// Σ pnl_net_budget100_usd of counted episodes opened in the hour: NO side (cashable before
    /// settlement) and YES side (locked until resolution).
    pub table: PerSide<Num>,
    pub n: PerSide<i64>,
    /// scenario → (paper marks, Σ equity change); a scenario without marks is absent (null).
    pub captured: BTreeMap<String, (i64, Num)>,
    /// Measured latency (ms) per health minute → count.
    pub lat: CountMap,
    /// The most capital needed at once in this hour (capped costs of overlapping episodes,
    /// including ones opened earlier). Not summed by `add`: see `EdgeMax`.
    pub max_capital: Num,
    pub biggest: Option<Biggest>,
}

/// Counted on the table: net-positive, not flagged, with a capped P&L. Returns that P&L.
fn counted(e: &Ep) -> Option<Decimal> {
    if e.positive() && !e.flagged() { e.budget100 } else { None }
}

/// Larger first; ties to the smaller episode id.
fn better(a: &(Decimal, String), b: &(Decimal, String)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1 < b.1)
}

impl EdgeCell {
    fn apply(&mut self, o: &Self, s: i64) {
        if s > 0 {
            self.table.yes.0 += o.table.yes.0;
            self.table.no.0 += o.table.no.0;
        } else {
            self.table.yes.0 -= o.table.yes.0;
            self.table.no.0 -= o.table.no.0;
        }
        self.n.yes += s * o.n.yes;
        self.n.no += s * o.n.no;
        for (k, (n, v)) in &o.captured {
            let e = self.captured.entry(k.clone()).or_insert((0, Num::default()));
            e.0 += s * n;
            if s > 0 {
                e.1.0 += v.0;
            } else {
                e.1.0 -= v.0;
            }
            if e.0 == 0 {
                self.captured.remove(k);
            }
        }
        stats::add_counts(&mut self.lat, &o.lat, s);
    }
}

impl CellData for EdgeCell {
    const FAMILY: &'static str = "edge";
    const AREAS: &'static [&'static str] =
        &["depth", "detector_no", "detector_yes", "episode_def", "fees", "host", "sim"];
    const EDGE_INPUTS: bool = true;
    type Max = EdgeMax;

    fn build(inp: &HourInput<'_>) -> Self {
        let h = inp.hour_ms;
        let mut c = EdgeCell::default();
        let mut best: Option<(Decimal, String)> = None;
        for e in opened_in(inp.episodes, h) {
            let Some(b) = counted(e) else { continue };
            c.table.get_mut(e.side).0 += b;
            *c.n.get_mut(e.side) += 1;
            let cand = (b, e.id.clone());
            if best.as_ref().is_none_or(|x| better(&cand, x)) {
                best = Some(cand);
            }
        }
        let cap = Decimal::from(CAP_USD);
        let mut events: Vec<(i64, u8, Decimal)> = Vec::new();
        for e in inp.episodes {
            if counted(e).is_none() {
                continue;
            }
            if let Some((s, t)) = overlap(e, h) {
                let cost = e.cost.min(cap).max(Decimal::ZERO);
                events.push((s, 1, cost));
                events.push((t, 0, -cost)); // at equal times closes (0) sort before opens (1)
            }
        }
        events.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let (mut run, mut max) = (Decimal::ZERO, Decimal::ZERO);
        for (_, _, d) in events {
            run += d;
            if run > max {
                max = run;
            }
        }
        c.max_capital = Num(max);
        c.biggest = best.map(|(usd, episode_id)| Biggest { usd: Num(usd), episode_id });
        for v in inp.latency_ms {
            *c.lat.entry(*v).or_insert(0) += 1;
        }
        for (scenario, d) in inp.equity_deltas {
            let e = c.captured.entry(scenario.clone()).or_insert((0, Num::default()));
            e.0 += 1;
            e.1.0 += *d;
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> EdgeMax {
        EdgeMax {
            capital: self.max_capital.0,
            biggest: self.biggest.as_ref().map(|b| (b.usd.0, b.episode_id.clone())),
        }
    }

    fn merge_max(into: &mut EdgeMax, o: &EdgeMax) {
        if o.capital > into.capital {
            into.capital = o.capital;
        }
        if let Some(b) = &o.biggest {
            if into.biggest.as_ref().is_none_or(|a| better(b, a)) {
                into.biggest = Some(b.clone());
            }
        }
    }
}

/// The host label inside a premise key's `host=code:label|band`.
pub fn host_from_key(key: &str) -> Option<&str> {
    let v = key.split(';').find_map(|p| p.strip_prefix("host="))?;
    let (_, rest) = v.split_once(':')?;
    rest.split('|').next()
}

pub fn edge(c: &EdgeCell, m: &EdgeMax, ctx: &BodyCtx<'_>) -> CardBody {
    let on_table = c.table.yes.0 + c.table.no.0;
    let n = c.n.yes + c.n.no;
    let mut thin = Vec::new();
    if n < THIN_EDGE_N {
        thin.push(format!("{n} net-positive episodes < {THIN_EDGE_N}; the capture rates are not reliable"));
    }
    if on_table.is_zero() {
        thin.push("nothing was on the table, so there is no capture rate (—)".to_string());
    }
    let rate = |scenario: &str| -> (Option<String>, Option<String>) {
        match c.captured.get(scenario) {
            None => (None, None),
            Some((_, v)) => (
                Some(dec(v.0)),
                (!on_table.is_zero()).then(|| dec((v.0 * Decimal::from(100) / on_table).round_dp(1))),
            ),
        }
    };
    let head_net = ctx.scenarios.iter().find(|s| s.name == HEADLINE).map_or(145, |s| s.net_ms);
    let (head_cap, head_rate) = rate(HEADLINE);
    let lat_n = stats::total(&c.lat);
    let lat_med = stats::nearest_rank(c.lat.iter().map(|(v, k)| (*v, *k)), lat_n, 50);
    let columns = vec![
        row(ON_TABLE, "USD", None, None, Some(dec(on_table))),
        row("NO side, cashable before settlement", "USD", None, None, Some(dec(c.table.no.0))),
        row("YES side, locked until resolution", "USD", None, None, Some(dec(c.table.yes.0))),
        row(&format!("captured at {HEADLINE} ({head_net} ms)"), "USD", None, None, head_cap),
        row(&format!("capture rate, {HEADLINE}"), "%", None, None, head_rate),
        row("capital needed at once", "USD", None, None, Some(dec(m.capital))),
        row("biggest single opportunity", "USD", None, None, m.biggest.as_ref().map(|b| dec(b.0))),
        row("measured latency (estimate)", "ms", None, None, lat_med.map(|v| v.to_string())),
    ];
    let decay = ctx
        .scenarios
        .iter()
        .map(|s| {
            let (captured, rate_pct) = rate(&s.name);
            DecayPoint {
                scenario: s.name.clone(),
                latency_ms: s.latency_ms,
                net_ms: s.net_ms,
                captured,
                rate_pct,
                headline: s.name == HEADLINE,
            }
        })
        .collect();
    let measured = lat_med.map(|latency_ms| Measured {
        latency_ms,
        host: host_from_key(ctx.premise_key).unwrap_or("unknown").to_string(),
    });
    let notes = vec![
        UPPER_BOUND.to_string(),
        "P0-converter takes the NO side only; the YES side is locked until resolution and no account takes it."
            .to_string(),
        "Captured: each scenario's P0-converter paper account (default fee), its equity change over the window \
         from paper_marks. Capture rate = captured ÷ on the table."
            .to_string(),
        "Measured latency: the window's median of feed lag p50 + REST round trip p50 / 2, per health minute. \
         An estimate."
            .to_string(),
    ];
    make(
        Parts {
            card: "EDGE",
            paper: "edge",
            title: "Edge capture",
            params: format!("cap_usd={CAP_USD};headline={HEADLINE};thin_n={THIN_EDGE_N}"),
            n,
            columns,
            series: Series { decay, measured, ..Series::default() },
            notes,
            thin,
            episode_link: m.biggest.as_ref().map(|b| b.1.clone()),
        },
        ctx,
    )
}

impl Card for EdgeCell {
    fn bodies(agg: &Self, max: &EdgeMax, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![edge(agg, max, ctx)]
    }
}
```

In `crates/cards/src/lib.rs`, add `pub mod edge;`.

(In the body test, 1.25 of 12.5 on the table is 10 %; 3.1 of 12.5 is 24.8 %.)

- [ ] **Step 4: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards`
Expected: PASS: 5 tests in `edge.rs` plus the earlier cards tests.

- [ ] **Step 5: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p cards --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add crates/cards/src/lib.rs crates/cards/src/edge.rs crates/cards/tests/edge.rs && git commit -m "cards: Edge capture cells and body (split, captured per scenario, capture rate, capital overlap, biggest, measured marker)"
```

---

### Task 9: `bins/analyze`: the tick loop, catch-up, premise keys and statuses, the write-refusal check, the arch rules, and the benchmark

**Files:**
- Create: `crates/cards/src/family.rs`, `crates/cards/tests/family.rs`, `crates/cards/tests/bench_tick.rs`
- Create: `bins/analyze/Cargo.toml`, `bins/analyze/src/lib.rs`, `bins/analyze/src/config.rs`, `bins/analyze/src/guard.rs`, `bins/analyze/src/scenarios.rs`, `bins/analyze/src/tick.rs`, `bins/analyze/src/main.rs`, `bins/analyze/tests/common/mod.rs` (a verbatim copy of `crates/store/tests/common/mod.rs`), `bins/analyze/tests/tick.rs`
- Modify: `Cargo.toml` (members += `"bins/analyze"`), `crates/cards/src/lib.rs` (`pub mod family;`), `bins/scanner/tests/arch.rs`

**Interfaces:**
- Consumes: everything in `cards` (Tasks 5–8); `store::{Store, PremiseRun, ChangedSpan, CardEpisode, CardCellRow, CardResultRow, MAX_CHANGED_PAGE, RunPremiseRow}` and the Task 3 methods; `sim::config::SimConfig` (existing) for the scenario latencies.
- Produces (`cards::family`): `struct RunSpan { run_id: i64, started_ms: i64, end_ms: Option<i64> }`, `struct RunPremises { span: RunSpan, versions: BTreeMap<String, String> }`, `fn current_key(areas: &[&str], runs: &[RunPremises]) -> Option<(String, Vec<RunSpan>)>`, `struct Family<C: CellData> { key: Option<String>, runs: BTreeMap<i64, RunSpan>, catching_up: bool, windows: Windows<C> }` with `new(now_hour)`, `since_ms()`, `run_ids()`, `run_end(run_id, now_ms)`, `fn hours_between(from_ms: i64, to_ms: i64) -> Vec<i64>`, `fn contiguous(hours: &BTreeSet<i64>, max_hours: i64) -> Vec<Vec<i64>>`.
- Produces (`analyze`): `version() -> &'static str`, `now_ms() -> i64`, `ensure_writes_only_cards(&Store) -> anyhow::Result<()>`, `config::Config { database_url, every_secs: u64, statement_timeout_ms: u32, catchup_pause_ms: u64, policies_path: String }` with `from_env()`, `from_lookup(get)`; `scenarios::{parse(text) -> anyhow::Result<Vec<ScenarioLat>>, load(path) -> anyhow::Result<Vec<ScenarioLat>>}`; `tick::{Analyzer, TickReport, LOOKBACK_MS = 120_000, BATCH_HOURS = 24}` with `Analyzer::new(store: Store, pause: Duration, version: &str, scenarios: Vec<ScenarioLat>) -> anyhow::Result<Analyzer>`, `Analyzer::tick(&mut self, now_ms: i64) -> anyhow::Result<TickReport>`, `Analyzer::reset(&mut self)`, `Analyzer::write_static_meta(&self) -> anyhow::Result<()>`, `Analyzer::record_error(&self, now_ms: i64, text: &str)`.
- card_meta keys analyze writes: `tick` = `{"last_tick_ms", "analyze_version", "tick_ms"}`, `last_error` = `null` or `{"ms", "text"}`, `cursor` = `{"updated_ms"}`, `fam:g1` / `fam:dur` / `fam:g4` / `fam:edge` = `{"key", "next_hour", "done"}`, `papers`, `not_replicated`.
- Env (compose, Task 14): `DATABASE_URL` (analyze_rw), `ANALYZE_EVERY_SECS` (900, 60..=3600), `ANALYZE_STATEMENT_TIMEOUT_MS` (10000, 1000..=60000), `ANALYZE_CATCHUP_PAUSE_MS` (2000, 0..=10000), `ANALYZE_POLICIES` (`/etc/cute-trading/policies.toml`).

- [ ] **Step 1: Write the failing family tests.** Create `crates/cards/tests/family.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::G1Cell;
use cards::family::*;
use std::collections::{BTreeMap, BTreeSet};

fn run(id: i64, started: i64, end: Option<i64>, v: &[(&str, &str)]) -> RunPremises {
    RunPremises {
        span: RunSpan { run_id: id, started_ms: started, end_ms: end },
        versions: v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

const AREAS: &[&str] = &["episode_def", "fees"];

#[test]
fn the_current_key_is_the_newest_complete_run_and_only_its_twins_match() {
    let runs = vec![
        run(7, 0, Some(10), &[("episode_def", "1+a"), ("fees", "1+b")]),
        run(8, 20, Some(30), &[("episode_def", "2+a"), ("fees", "1+b")]),
        run(9, 40, Some(50), &[("episode_def", "1+a"), ("fees", "1+b")]),
        run(10, 60, None, &[("episode_def", "1+a")]), // missing fees: never counts
    ];
    let (key, matched) = current_key(AREAS, &runs).unwrap();
    assert_eq!(key, "episode_def=1+a;fees=1+b");
    assert_eq!(matched.iter().map(|r| r.run_id).collect::<Vec<_>>(), vec![7, 9], "run 8 has other premises");
    assert_eq!(current_key(&["sim"], &runs), None);
}

#[test]
fn family_spans_hours_and_groups() {
    let mut f: Family<G1Cell> = Family::new(10 * H);
    f.runs.insert(3, RunSpan { run_id: 3, started_ms: 2 * H + 5, end_ms: Some(4 * H) });
    f.runs.insert(4, RunSpan { run_id: 4, started_ms: 5 * H, end_ms: None });
    assert_eq!(f.since_ms(), Some(2 * H + 5));
    assert_eq!(f.run_ids(), vec![3, 4]);
    assert_eq!(f.run_end(3, 10 * H), 4 * H);
    assert_eq!(f.run_end(4, 10 * H), 10 * H);
    assert_eq!(f.run_end(99, 10 * H), 10 * H);
    assert_eq!(hours_between(H + 5, 3 * H), vec![H, 2 * H, 3 * H]);
    assert_eq!(hours_between(H + 5, H + 6), vec![H]);
    let set: BTreeSet<i64> = [0, H, 2 * H, 5 * H, 6 * H].into_iter().collect();
    assert_eq!(contiguous(&set, 2), vec![vec![0, H], vec![2 * H], vec![5 * H, 6 * H]]);
}
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test family`
Expected: compile error `unresolved import cards::family`.

- [ ] **Step 2: Write the family module.** Create `crates/cards/src/family.rs`:

```rust
//! Premise keys per cell family and the state analyze keeps between ticks (spec §4, §7).
use crate::cells::CellData;
use crate::window::Windows;
use crate::{HOUR_MS, hour_of};
use arb_core::premises::premise_key;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpan {
    pub run_id: i64,
    pub started_ms: i64,
    /// None: still running.
    pub end_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPremises {
    pub span: RunSpan,
    pub versions: BTreeMap<String, String>,
}

/// The family's current premise key (the key of the newest run that has every area) and the runs
/// with exactly that key, oldest first. Runs missing an area never count. None: no run qualifies.
pub fn current_key(areas: &[&str], runs: &[RunPremises]) -> Option<(String, Vec<RunSpan>)> {
    let mut newest_first: Vec<&RunPremises> = runs.iter().collect();
    newest_first.sort_by_key(|r| std::cmp::Reverse(r.span.run_id));
    let key = newest_first.iter().find_map(|r| premise_key(areas, &r.versions))?;
    let mut matched: Vec<RunSpan> = runs
        .iter()
        .filter(|r| premise_key(areas, &r.versions).as_deref() == Some(key.as_str()))
        .map(|r| r.span.clone())
        .collect();
    matched.sort_by_key(|r| r.run_id);
    Some((key, matched))
}

#[derive(Debug, Clone)]
pub struct Family<C: CellData> {
    pub key: Option<String>,
    pub runs: BTreeMap<i64, RunSpan>,
    pub catching_up: bool,
    pub windows: Windows<C>,
}

impl<C: CellData> Family<C> {
    pub fn new(now_hour: i64) -> Self {
        Self { key: None, runs: BTreeMap::new(), catching_up: false, windows: Windows::new(now_hour) }
    }

    /// When the key's data starts: its first run's start.
    pub fn since_ms(&self) -> Option<i64> {
        self.runs.values().map(|r| r.started_ms).min()
    }

    pub fn run_ids(&self) -> Vec<i64> {
        self.runs.keys().copied().collect()
    }

    /// Where an unclosed episode of `run_id` stops, for hour overlap: its run's end, else now.
    pub fn run_end(&self, run_id: i64, now_ms: i64) -> i64 {
        self.runs.get(&run_id).and_then(|r| r.end_ms).unwrap_or(now_ms).min(now_ms)
    }
}

/// Every UTC hour touched by [from_ms, to_ms], both ends included.
pub fn hours_between(from_ms: i64, to_ms: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut h = hour_of(from_ms);
    let end = hour_of(to_ms.max(from_ms));
    while h <= end {
        out.push(h);
        h += HOUR_MS;
    }
    out
}

/// Sorted hours as runs of consecutive hours, at most `max_hours` long each.
pub fn contiguous(hours: &BTreeSet<i64>, max_hours: i64) -> Vec<Vec<i64>> {
    let cap = usize::try_from(max_hours.max(1)).unwrap_or(1);
    let mut out: Vec<Vec<i64>> = Vec::new();
    for h in hours {
        match out.last_mut() {
            Some(g) if g.len() < cap && g.last().is_some_and(|l| l + HOUR_MS == *h) => g.push(*h),
            _ => out.push(vec![*h]),
        }
    }
    out
}
```

In `crates/cards/src/lib.rs`, add `pub mod family;`.

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test family`
Expected: PASS (2 tests).

- [ ] **Step 3: Write the failing analyze tests.** In the root `Cargo.toml`, add `"bins/analyze"` to `members` (after `"bins/api"`). Create `bins/analyze/Cargo.toml`:

```toml
[package]
name = "analyze"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# No network client (spec §2): Postgres only through store; the arch test enforces it.
[dependencies]
store = { path = "../../crates/store" }
cards = { path = "../../crates/cards" }
sim = { path = "../../crates/sim" }
tokio = { workspace = true, features = ["signal"] }
serde_json.workspace = true
rust_decimal.workspace = true
toml.workspace = true
anyhow.workspace = true

[dev-dependencies]
rust_decimal_macros.workspace = true

[lints]
workspace = true
```

Copy the harness: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p bins/analyze/tests/common && cp crates/store/tests/common/mod.rs bins/analyze/tests/common/mod.rs`.

Create `bins/analyze/tests/tick.rs`:

```rust
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
    analyze::scenarios::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/policies.toml")).unwrap()
}

fn premises(run_id: i64, episode_def: &str) -> Vec<RunPremiseRow> {
    [
        ("censoring", "1+c0"), ("depth", "1+e3b0c442"), ("detector_no", "1+e3b0c442"),
        ("detector_yes", "1+e3b0c442"), ("episode_def", episode_def), ("fees", "1+e3b0c442"),
        ("host", "1:vps-kvm2|100-250"), ("sim", "1+bb"),
    ]
    .iter()
    .map(|(a, v)| RunPremiseRow { run_id, area: a.to_string(), version: v.to_string() })
    .collect()
}

fn ep(id: &str, run_id: i64, kind: &str, opened: i64, closed: Option<i64>) -> EpisodeRow {
    EpisodeRow {
        episode_id: id.into(), run_id, source: "live".into(), kind: kind.into(), realization: "converter".into(),
        event_id: format!("ev-{id}"), event_slug: None, event_title: None, neg_risk_market_id: None, pair_id: None,
        category: None, n_outcomes: 3, n_legs: 3, legs_dropped: 0, augmented: false, incomplete_set: false,
        uncovered_legs: 0, opened_server_ms: opened, closed_server_ms: closed, duration_ms: closed.map(|c| c - opened),
        censor_left: false, censor_right: false, censor_gap: false, detect_lag_ms: None, n_updates: 1,
        peak_server_ms: opened, peak_edge_top: dec!(0.0049), peak_edge_top_gross: dec!(0.0137), peak_max_size: dec!(83),
        peak_cost_usd: dec!(99.59), peak_payout_usd: dec!(100), peak_fees_usd: dec!(0.96), peak_pnl_net_usd: dec!(0.41),
        peak_pnl_gross_usd: dec!(1.37), peak_profit_per_dollar: None, pnl_net_budget100_usd: Some(dec!(0.33)),
        bottleneck_leg_idx: None, bottleneck_notional_usd: None, below_min_size: None, max_seconds_delay: None,
        days_to_end: None, annualized_return: None, post_end: None, engine_suspect: false, stale_transport: false,
        open_lag_ms: None, oracle_downgraded: None, scanner_version: "t".into(), config_hash: "c".into(),
    }
}

async fn run(store: &Store, started_ms: i64) -> i64 {
    store
        .start_run(&NewRun { service: "scanner".into(), started_ms, version: "t".into(), config_hash: "c".into(), mode: "live".into() })
        .await
        .unwrap()
}

async fn write(store: &Store, rows: Vec<EpisodeRow>) {
    store
        .write_batch(&Batch { episodes: rows.into_iter().map(|row| EpisodeWrite { row, legs: vec![] }).collect(), ..Batch::default() })
        .await
        .unwrap();
}

async fn rw(db: &common::TestDb) -> Store {
    Store::connect_analyze(db.opts.clone(), 2, 10_000, Some("analyze_rw")).await.unwrap()
}

async fn body(store: &Store, win: &str, card: &str) -> Value {
    let rows = store.card_results_for(win).await.unwrap();
    let (_, b) = rows.iter().find(|(c, _)| c == card).unwrap_or_else(|| panic!("no {card} for {win}: {rows:?}"));
    serde_json::from_str(b).unwrap()
}

fn cell(b: &Value, label: &str, col: &str) -> Value {
    b["columns"].as_array().unwrap().iter().find(|r| r["label"] == label).unwrap_or_else(|| panic!("no row {label}"))[col].clone()
}

#[tokio::test]
async fn a_tick_writes_every_card_and_window_with_statuses_and_meta() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 3 * H).await;
    owner.insert_run_premises(&premises(r, "1+aa")).await.unwrap();
    let mut rows = Vec::new();
    for i in 0..12 {
        rows.push(ep(&format!("n{i:02}"), r, "no_basket", T0 - 2 * H + i * M, Some(T0 - 2 * H + i * M + 4_000)));
    }
    for i in 0..3 {
        rows.push(ep(&format!("y{i}"), r, "yes_basket", T0 - H + i * M, Some(T0 - H + i * M + 16_000)));
    }
    write(&owner, rows).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios()).await.unwrap();
    a.write_static_meta().await.unwrap();
    let rep = a.tick(T0 + 30 * M).await.unwrap();
    assert_eq!(rep.results, 4 * 4 + 5, "G1–G4 × 4 windows + EDGE × 5 windows");
    let g1 = body(&rw, "7d", "G1").await;
    assert_eq!(cell(&g1, "NO-side episodes", "ours"), "12");
    assert_eq!(cell(&g1, "YES-side episodes", "their"), "3");
    assert_eq!(cell(&g1, "YES-side episodes", "paper"), "2098");
    assert_eq!(g1["status"], "warming", "data since T0 − 3 h starts inside 7d");
    assert!(g1["premise_key"].as_str().unwrap().contains("episode_def=1+aa"));
    assert_eq!(g1["hash"].as_str().unwrap().len(), 64);
    let all = body(&rw, "all", "G1").await;
    assert_eq!(all["status"], "thin", "all is never warming; YES n = 3 < 10 is thin");
    let today: Vec<String> = rw.card_results_for("today").await.unwrap().into_iter().map(|x| x.0).collect();
    assert_eq!(today, ["EDGE"]);
    let edge = body(&rw, "24h", "EDGE").await;
    assert_eq!(cell(&edge, "edge on the table", "ours"), "4.95");
    let meta: Vec<String> = rw.card_meta_all().await.unwrap().into_iter().map(|m| m.0).collect();
    for k in ["cursor", "fam:dur", "fam:edge", "fam:g1", "fam:g4", "last_error", "not_replicated", "papers", "tick"] {
        assert!(meta.contains(&k.to_string()), "{k} in {meta:?}");
    }
    let fam: Value = serde_json::from_str(&rw.card_meta_get("fam:g1").await.unwrap().unwrap()).unwrap();
    assert_eq!((fam["done"].as_bool(), fam["next_hour"].as_i64()), (Some(true), Some(T0)));
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_late_close_outside_the_recent_48h_rebuilds_the_old_hour_without_double_counting() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 70 * H).await;
    owner.insert_run_premises(&premises(r, "1+aa")).await.unwrap();
    let opened = T0 - 60 * H; // outside the 48 h in-memory cache, inside 7d
    write(&owner, vec![ep("long", r, "no_basket", opened, None), ep("short", r, "no_basket", opened + M, Some(opened + M + 2_000))]).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios()).await.unwrap();
    a.tick(T0).await.unwrap();
    let g3 = body(&rw, "7d", "G3").await;
    assert_eq!(cell(&g3, "episodes observed (per hour)", "their"), "61", "open: observed in every hour to now");
    assert_eq!(cell(&body(&rw, "7d", "G2").await, "NO durations (n)", "ours"), "1");
    // the late close: two hours long
    write(&owner, vec![ep("long", r, "no_basket", opened, Some(opened + 2 * H))]).await;
    a.tick(T0 + 15 * M).await.unwrap();
    for win in ["7d", "30d", "all"] {
        let g3 = body(&rw, win, "G3").await;
        assert_eq!(cell(&g3, "episodes observed (per hour)", "their"), "3", "{win}: 2 hours of long + short");
        let g2 = body(&rw, win, "G2").await;
        assert_eq!(cell(&g2, "NO durations (n)", "ours"), "2", "{win}: counted once, as a finished duration");
    }
    let key = body(&rw, "7d", "G2").await["premise_key"].as_str().unwrap().to_string();
    let stored = rw.card_cells_at("dur", &key, &[opened]).await.unwrap();
    assert!(stored[0].1.contains("7200000"), "the opening hour's cell has the 2 h duration: {}", stored[0].1);
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_premise_bump_mid_window_never_mixes_keys() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r1 = run(&owner, T0 - 50 * H).await;
    owner.insert_run_premises(&premises(r1, "1+aa")).await.unwrap();
    owner.end_run(r1, T0 - 10 * H, "shutdown").await.unwrap();
    write(&owner, (0..5).map(|i| ep(&format!("old{i}"), r1, "no_basket", T0 - 20 * H + i * M, Some(T0 - 20 * H + i * M + 1_000))).collect()).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios()).await.unwrap();
    a.tick(T0 - 6 * H).await.unwrap();
    let old_key = body(&rw, "7d", "G1").await["premise_key"].as_str().unwrap().to_string();
    let old_cells = rw.card_cells_range("g1", &old_key, 0, i64::MAX).await.unwrap();
    assert!(!old_cells.is_empty());
    // a new run under a bumped episode_def
    let r2 = run(&owner, T0 - 5 * H).await;
    owner.insert_run_premises(&premises(r2, "2+aa")).await.unwrap();
    write(&owner, (0..3).map(|i| ep(&format!("new{i}"), r2, "no_basket", T0 - 4 * H + i * M, Some(T0 - 4 * H + i * M + 1_000))).collect()).await;
    a.tick(T0).await.unwrap();
    for win in ["24h", "7d", "all"] {
        let g1 = body(&rw, win, "G1").await;
        assert!(g1["premise_key"].as_str().unwrap().contains("episode_def=2+aa"), "{win}");
        assert_eq!(cell(&g1, "NO-side episodes", "ours"), "3", "{win}: only the new key's run");
        assert_eq!(g1["since_ms"].as_i64(), Some(T0 - 5 * H));
    }
    assert_eq!(body(&rw, "7d", "G1").await["status"], "warming");
    assert_eq!(rw.card_cells_range("g1", &old_key, 0, i64::MAX).await.unwrap(), old_cells, "old cells untouched");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn runs_without_premises_are_never_counted() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r1 = run(&owner, T0 - 5 * H).await;
    owner.insert_run_premises(&premises(r1, "1+aa")).await.unwrap();
    owner.end_run(r1, T0 - 3 * H, "shutdown").await.unwrap();
    let r2 = run(&owner, T0 - 2 * H).await; // a mixed-version run that wrote no premises
    write(&owner, vec![
        ep("a", r1, "no_basket", T0 - 4 * H, Some(T0 - 4 * H + 1_000)),
        ep("b", r1, "no_basket", T0 - 4 * H + M, Some(T0 - 4 * H + M + 1_000)),
        ep("c", r2, "no_basket", T0 - H, Some(T0 - H + 1_000)),
        ep("d", r2, "no_basket", T0 - H + M, Some(T0 - H + M + 1_000)),
    ])
    .await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios()).await.unwrap();
    a.tick(T0).await.unwrap();
    assert_eq!(cell(&body(&rw, "all", "G1").await, "NO-side episodes", "ours"), "2");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn catch_up_builds_history_in_24_hour_batches_and_pauses_between_them() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 60 * H).await;
    owner.insert_run_premises(&premises(r, "1+aa")).await.unwrap();
    write(&owner, (0..60).map(|i| ep(&format!("e{i:02}"), r, "yes_basket", T0 - 60 * H + i * H, Some(T0 - 60 * H + i * H + 5_000))).collect()).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::from_millis(30), "test", scenarios()).await.unwrap();
    let t = Instant::now();
    let rep = a.tick(T0).await.unwrap();
    assert_eq!(rep.catch_up_hours, 4 * 61, "61 hours (T0 − 60 h … T0) for each of the 4 families");
    // 61 hours = batches of 24 + 24 + 13: 2 pauses per family
    assert!(t.elapsed() >= Duration::from_millis(4 * 2 * 30), "{:?}", t.elapsed());
    let g1 = body(&rw, "7d", "G1").await;
    assert_eq!(cell(&g1, "YES-side episodes", "ours"), "60");
    assert_ne!(g1["status"], "under_review", "catch-up finished inside the tick");
    let rep2 = a.tick(T0 + 20 * M).await.unwrap();
    assert_eq!(rep2.catch_up_hours, 0, "a normal tick after catch-up");
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn the_startup_check_refuses_a_role_that_can_write_elsewhere() {
    let Some((owner, db)) = common::fresh().await else { return };
    assert!(analyze::ensure_writes_only_cards(&owner).await.is_err());
    let rw = rw(&db).await;
    analyze::ensure_writes_only_cards(&rw).await.unwrap();
    rw.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_failed_tick_records_the_error_and_keeps_the_last_results() {
    let Some((owner, db)) = common::fresh().await else { return };
    let r = run(&owner, T0 - 2 * H).await;
    owner.insert_run_premises(&premises(r, "1+aa")).await.unwrap();
    write(&owner, vec![ep("a", r, "no_basket", T0 - H, Some(T0 - H + 1_000))]).await;
    let rw = rw(&db).await;
    let mut a = Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios()).await.unwrap();
    a.tick(T0).await.unwrap();
    owner.raw_exec("ALTER TABLE card_results RENAME TO card_results_away").await.unwrap();
    let err = a.tick(T0 + 15 * M).await.unwrap_err();
    a.record_error(T0 + 15 * M, &format!("{err:#}")).await;
    a.reset();
    owner.raw_exec("ALTER TABLE card_results_away RENAME TO card_results").await.unwrap();
    let e: Value = serde_json::from_str(&rw.card_meta_get("last_error").await.unwrap().unwrap()).unwrap();
    assert_eq!(e["ms"].as_i64(), Some(T0 + 15 * M));
    assert!(e["text"].as_str().unwrap().contains("card_results"), "{e}");
    assert_eq!(rw.card_results_for("7d").await.unwrap().len(), 5, "the last results stay (G1–G4 and EDGE)");
    a.tick(T0 + 30 * M).await.unwrap();
    assert_eq!(rw.card_meta_get("last_error").await.unwrap().as_deref(), Some("null"));
    assert_eq!(cell(&body(&rw, "7d", "G1").await, "NO-side episodes", "ours"), "1", "reloaded from card_cells, not doubled");
    rw.close().await;
    db.drop(owner).await;
}
```

(In the first test, 15 net-positive episodes at $0.33 each put $4.95 on the table.)

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p analyze`
Expected: compile errors: `can't find library analyze` (no `src/lib.rs` yet).

- [ ] **Step 4: The small modules.** Create `bins/analyze/src/lib.rs`:

```rust
//! analyze: the live-cards job (Phase 5a spec §7). Every ANALYZE_EVERY_SECS it rebuilds the dirty
//! hourly cells of each card family through `store`, updates its windows, and writes card_results.
//! No network client (spec §2), paper only (parent §16), writes only the card tables.
#![forbid(unsafe_code)]

pub mod config;
pub mod guard;
pub mod scenarios;
pub mod tick;

use store::Store;

/// git sha baked in at image build time (the image's SCANNER_VERSION build arg), "dev" otherwise.
pub fn version() -> &'static str {
    option_env!("SCANNER_VERSION").unwrap_or("dev")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Spec §2: refuse to run with a role that can write anything but the card tables.
pub async fn ensure_writes_only_cards(store: &Store) -> anyhow::Result<()> {
    let bad = store.role_writes_outside_cards().await?;
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: the database role can write {} (connect as analyze_rw, which writes only \
         card_cells, card_results and card_meta)",
        bad.join(", ")
    );
    Ok(())
}
```

Create `bins/analyze/src/guard.rs` as a copy of `bins/api/src/guard.rs` with one change: in its header comment, replace `Same denylist as bins/scanner/src/guard.rs (keep the two in step).` with `Same denylist as bins/scanner/src/guard.rs and bins/api/src/guard.rs (bins/scanner/tests/arch.rs keeps the three in step).` Keep `CONTAINS`, `PREFIXES`, `forbidden`, `check_env` and its test byte for byte.

Create `bins/analyze/src/config.rs`:

```rust
//! Environment-only configuration. The database URL carries a password, so Debug redacts it.
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// postgres://analyze_rw:…@postgres:5432/cute_trading
    pub database_url: String,
    pub every_secs: u64,
    pub statement_timeout_ms: u32,
    pub catchup_pause_ms: u64,
    pub policies_path: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("every_secs", &self.every_secs)
            .field("statement_timeout_ms", &self.statement_timeout_ms)
            .field("catchup_pause_ms", &self.catchup_pause_ms)
            .field("policies_path", &self.policies_path)
            .finish()
    }
}

fn num<T: std::str::FromStr>(v: Option<String>, default: T, name: &str) -> anyhow::Result<T> {
    match v {
        None => Ok(default),
        Some(s) => s.parse().map_err(|_| anyhow::anyhow!("{name} must be a non-negative integer")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let database_url = get("DATABASE_URL")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the analyze_rw URL)"))?;
        let every_secs: u64 = num(get("ANALYZE_EVERY_SECS"), 900, "ANALYZE_EVERY_SECS")?;
        let statement_timeout_ms: u32 = num(get("ANALYZE_STATEMENT_TIMEOUT_MS"), 10_000, "ANALYZE_STATEMENT_TIMEOUT_MS")?;
        let catchup_pause_ms: u64 = num(get("ANALYZE_CATCHUP_PAUSE_MS"), 2_000, "ANALYZE_CATCHUP_PAUSE_MS")?;
        let policies_path = get("ANALYZE_POLICIES").unwrap_or_else(|| "/etc/cute-trading/policies.toml".to_string());
        anyhow::ensure!((60..=3_600).contains(&every_secs), "ANALYZE_EVERY_SECS must be 60..=3600 (spec §7)");
        anyhow::ensure!((1_000..=60_000).contains(&statement_timeout_ms), "ANALYZE_STATEMENT_TIMEOUT_MS must be 1000..=60000");
        anyhow::ensure!(catchup_pause_ms <= 10_000, "ANALYZE_CATCHUP_PAUSE_MS must be 0..=10000");
        Ok(Self { database_url, every_secs, statement_timeout_ms, catchup_pause_ms, policies_path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_bounds_and_a_redacted_url() {
        let url = ("DATABASE_URL", "postgres://analyze_rw:hunter2@postgres/cute_trading");
        let c = cfg(&[url]).unwrap();
        assert_eq!((c.every_secs, c.statement_timeout_ms, c.catchup_pause_ms), (900, 10_000, 2_000));
        assert_eq!(c.policies_path, "/etc/cute-trading/policies.toml");
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "59")]).is_err());
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "3601")]).is_err());
        assert_eq!(cfg(&[url, ("ANALYZE_EVERY_SECS", "60")]).unwrap().every_secs, 60);
        assert!(cfg(&[url, ("ANALYZE_EVERY_SECS", "x")]).is_err());
        assert!(cfg(&[url, ("ANALYZE_CATCHUP_PAUSE_MS", "10001")]).is_err());
        assert!(!format!("{c:?}").contains("hunter2"));
    }
}
```

Create `bins/analyze/src/scenarios.rs`:

```rust
//! The latency scenarios of policies.toml (the same frozen file the scanner's paper trader runs):
//! x = proc + net + match for Edge capture's decay chart (spec §6.2).
use anyhow::Context;
use cards::body::ScenarioLat;

pub fn parse(text: &str) -> anyhow::Result<Vec<ScenarioLat>> {
    let sim: sim::config::SimConfig = toml::from_str(text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(sim
        .scenarios
        .iter()
        .map(|s| ScenarioLat { name: s.name.clone(), net_ms: s.net_ms, latency_ms: s.proc_ms + s.net_ms + s.match_ms })
        .collect())
}

pub fn load(path: &str) -> anyhow::Result<Vec<ScenarioLat>> {
    parse(&std::fs::read_to_string(path).with_context(|| format!("read {path}"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repo_policies_give_four_scenarios() {
        let s = load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/policies.toml")).unwrap();
        let got: Vec<(&str, i64, i64)> = s.iter().map(|x| (x.name.as_str(), x.net_ms, x.latency_ms)).collect();
        assert_eq!(got, vec![("colo-idle", 1, 52), ("sp-idle", 145, 300), ("sp-burst", 145, 450), ("sp-idle-nodelay", 145, 300)]);
    }
}
```

- [ ] **Step 5: The tick.** Create `bins/analyze/src/tick.rs`:

```rust
//! One analyze tick (spec §7):
//!  1. read the live episodes whose updated_ms passed the cursor (a 120 s lookback covers commit
//!     order) and mark every hour from their opening to now dirty, for each family whose runs
//!     they belong to (a late close also clears the hours it no longer spans);
//!  2. a family whose premise key changed (or first start) loads its stored cells and catches up
//!     the rest from history, oldest first, 24 hours per batch, pausing between batches, status
//!     under_review meanwhile; a run that newly matches (a late `host` row) dirties its hours;
//!  3. advance the windows to the current hour, rebuild the dirty cells and the current partial
//!     hour, store them, swap them into the windows;
//!  4. write every card for 24h / 7d / 30d / all (+ today for Edge capture), the cursor and meta.
//!
//! Failure: the caller logs one line, records it in card_meta and calls `reset`, so the next tick
//! reloads the windows from card_cells (the stored cells are the truth). Results keep their values.
use cards::body::{BodyCtx, Card, ScenarioLat, base_status};
use cards::cells::{CellData, DurCell, Ep, G1Cell, G4Cell, HourInput, Side};
use cards::edge::EdgeCell;
use cards::family::{Family, RunPremises, RunSpan, contiguous, current_key, hours_between};
use cards::window::SPANS;
use cards::{DAY_MS, HOUR_MS, hour_of};
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use store::{CardCellRow, CardEpisode, CardResultRow, ChangedSpan, MAX_CHANGED_PAGE, Store};

/// Re-read this much before the cursor: a transaction stamped earlier may commit later.
pub const LOOKBACK_MS: i64 = 120_000;
pub const BATCH_HOURS: i64 = 24;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    pub ms: u128,
    pub dirty_hours: usize,
    pub catch_up_hours: usize,
    pub results: usize,
}

pub struct Analyzer {
    store: Store,
    pause: Duration,
    version: String,
    scenarios: Vec<ScenarioLat>,
    g1: Family<G1Cell>,
    dur: Family<DurCell>,
    g4: Family<G4Cell>,
    edge: Family<EdgeCell>,
    cursor: Option<i64>,
    last_hour: Option<i64>,
}

struct Env<'a> {
    store: &'a Store,
    now_ms: i64,
    now_hour: i64,
    last_hour: Option<i64>,
    pause: Duration,
    version: &'a str,
    scenarios: &'a [ScenarioLat],
    accounts: &'a [(String, String)],
    runs: &'a [RunPremises],
    changed: &'a [ChangedSpan],
}

impl Analyzer {
    pub async fn new(store: Store, pause: Duration, version: &str, scenarios: Vec<ScenarioLat>) -> anyhow::Result<Self> {
        let cursor = match store.card_meta_get("cursor").await? {
            Some(text) => serde_json::from_str::<serde_json::Value>(&text)?["updated_ms"].as_i64(),
            None => None,
        };
        Ok(Self {
            store,
            pause,
            version: version.to_string(),
            scenarios,
            g1: Family::new(0),
            dur: Family::new(0),
            g4: Family::new(0),
            edge: Family::new(0),
            cursor,
            last_hour: None,
        })
    }

    /// Forget the in-memory windows: the next tick reloads them from card_cells.
    pub fn reset(&mut self) {
        self.g1 = Family::new(0);
        self.dur = Family::new(0);
        self.g4 = Family::new(0);
        self.edge = Family::new(0);
        self.last_hour = None;
    }

    pub async fn write_static_meta(&self) -> anyhow::Result<()> {
        self.store
            .card_results_put(
                &[],
                &[
                    ("papers".to_string(), cards::papers::papers().to_string()),
                    ("not_replicated".to_string(), cards::papers::not_replicated().to_string()),
                ],
            )
            .await?;
        Ok(())
    }

    /// Best effort: a failed tick is recorded for the page ("last tick failed …").
    pub async fn record_error(&self, now_ms: i64, text: &str) {
        let v = json!({ "ms": now_ms, "text": text }).to_string();
        if let Err(e) = self.store.card_meta_put("last_error", &v).await {
            eprintln!("analyze: could not record the error: {e}");
        }
    }

    async fn changed(&self) -> anyhow::Result<(Vec<ChangedSpan>, Option<i64>)> {
        let Some(cursor) = self.cursor else {
            // first start: catch-up reads the whole history; start the cursor at its end
            return Ok((Vec::new(), Some(self.store.max_episode_updated_ms().await?.unwrap_or(0))));
        };
        let (mut after_ms, mut after_id) = (cursor - LOOKBACK_MS, String::new());
        let (mut out, mut max) = (Vec::new(), cursor);
        loop {
            let page = self.store.changed_spans(after_ms, &after_id, MAX_CHANGED_PAGE).await?;
            let Some(last) = page.last() else { break };
            after_ms = last.updated_ms;
            after_id = last.episode_id.clone();
            max = max.max(after_ms);
            let full = i64::try_from(page.len()).unwrap_or(0) >= MAX_CHANGED_PAGE;
            out.extend(page);
            if !full {
                break;
            }
        }
        Ok((out, Some(max)))
    }

    pub async fn tick(&mut self, now_ms: i64) -> anyhow::Result<TickReport> {
        let started = Instant::now();
        let runs: Vec<RunPremises> = self
            .store
            .premise_runs()
            .await?
            .into_iter()
            .map(|r| RunPremises {
                span: RunSpan { run_id: r.run_id, started_ms: r.started_ms, end_ms: r.end_ms },
                versions: r.premises.into_iter().collect(),
            })
            .collect();
        let (changed, cursor) = self.changed().await?;
        let accounts = self.store.p0_accounts().await?;
        let env = Env {
            store: &self.store,
            now_ms,
            now_hour: hour_of(now_ms),
            last_hour: self.last_hour,
            pause: self.pause,
            version: &self.version,
            scenarios: &self.scenarios,
            accounts: &accounts,
            runs: &runs,
            changed: &changed,
        };
        let mut results = Vec::new();
        let mut meta = Vec::new();
        let mut report = TickReport::default();
        sync(&env, &mut self.g1, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.dur, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.g4, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.edge, &mut results, &mut meta, &mut report).await?;
        report.ms = started.elapsed().as_millis();
        meta.push((
            "tick".to_string(),
            json!({ "last_tick_ms": now_ms, "analyze_version": self.version, "tick_ms": report.ms }).to_string(),
        ));
        meta.push(("last_error".to_string(), "null".to_string()));
        if let Some(c) = cursor {
            meta.push(("cursor".to_string(), json!({ "updated_ms": c }).to_string()));
        }
        self.store.card_results_put(&results, &meta).await?;
        report.results = results.len();
        self.cursor = cursor;
        self.last_hour = Some(env.now_hour);
        Ok(report)
    }
}

async fn sync<C: Card>(
    env: &Env<'_>,
    fam: &mut Family<C>,
    results: &mut Vec<CardResultRow>,
    meta: &mut Vec<(String, String)>,
    report: &mut TickReport,
) -> anyhow::Result<()> {
    let Some((key, matched)) = current_key(C::AREAS, env.runs) else {
        fam.key = None; // no run recorded these premises yet: no result row (the page says so)
        return Ok(());
    };
    let matched: BTreeMap<i64, RunSpan> = matched.into_iter().map(|r| (r.run_id, r)).collect();
    let mut dirty: BTreeSet<i64> = BTreeSet::new();
    if fam.key.as_deref() != Some(key.as_str()) {
        start_key(env, fam, &key, matched, report).await?;
    } else {
        for r in matched.values().filter(|r| !fam.runs.contains_key(&r.run_id)) {
            dirty.extend(hours_between(r.started_ms, r.end_ms.unwrap_or(env.now_ms).min(env.now_ms)));
        }
        fam.runs = matched;
    }
    let needed = fam.windows.needed_ranges(env.now_hour);
    if !needed.is_empty() {
        let stored = load_ranges::<C>(env, &key, &needed).await?;
        fam.windows.advance(env.now_hour, &stored);
    }
    for s in env.changed.iter().filter(|s| fam.runs.contains_key(&s.run_id)) {
        dirty.extend(hours_between(s.opened_ms, env.now_ms));
    }
    dirty.insert(env.now_hour);
    if let Some(last) = env.last_hour.filter(|l| *l < env.now_hour) {
        dirty.insert(last);
    }
    let since_hour = fam.since_ms().map_or(env.now_hour, hour_of);
    dirty.retain(|h| *h >= since_hour && *h <= env.now_hour);
    report.dirty_hours += dirty.len();
    for group in contiguous(&dirty, BATCH_HOURS) {
        let built = build_hours(env, fam, &group).await?;
        let olds = applied_cells(env, fam, &key, &group).await?;
        put_cells(env, &key, &built).await?;
        for (h, c) in &built {
            fam.windows.apply(*h, olds.get(h), c);
        }
    }
    results.extend(family_results(env, fam, &key)?);
    meta.push((
        format!("fam:{}", C::FAMILY),
        json!({ "key": key, "next_hour": env.now_hour, "done": true }).to_string(),
    ));
    Ok(())
}

/// A new (or first-seen) premise key: load what is stored for it, then catch up the rest.
async fn start_key<C: Card>(
    env: &Env<'_>,
    fam: &mut Family<C>,
    key: &str,
    runs: BTreeMap<i64, RunSpan>,
    report: &mut TickReport,
) -> anyhow::Result<()> {
    *fam = Family::new(env.now_hour);
    fam.key = Some(key.to_string());
    fam.runs = runs;
    let since_hour = fam.since_ms().map_or(env.now_hour, hour_of);
    let meta_key = format!("fam:{}", C::FAMILY);
    let resume = match env.store.card_meta_get(&meta_key).await? {
        Some(text) => {
            let m: serde_json::Value = serde_json::from_str(&text)?;
            if m["key"].as_str() == Some(key) {
                m["next_hour"].as_i64().unwrap_or(since_hour).clamp(since_hour, env.now_hour + HOUR_MS)
            } else {
                since_hour
            }
        }
        None => since_hour,
    };
    let mut h = since_hour;
    while h < resume {
        let end = (h + BATCH_HOURS * HOUR_MS).min(resume);
        for (hh, text) in env.store.card_cells_range(C::FAMILY, key, h, end).await? {
            fam.windows.apply(hh, None, &C::from_json(&text)?);
        }
        h = end;
    }
    if resume > env.now_hour {
        return Ok(());
    }
    fam.catching_up = true;
    env.store.card_results_put(&family_results(env, fam, key)?, &[]).await?;
    let mut h = resume;
    while h <= env.now_hour {
        let end = (h + BATCH_HOURS * HOUR_MS).min(env.now_hour + HOUR_MS);
        let hours: Vec<i64> = (0..).map(|i| h + i * HOUR_MS).take_while(|x| *x < end).collect();
        let built = build_hours(env, fam, &hours).await?;
        put_cells(env, key, &built).await?;
        for (hh, c) in &built {
            // never applied before (only hours < resume were loaded): old = None
            fam.windows.apply(*hh, None, c);
        }
        report.catch_up_hours += built.len();
        h = end;
        env.store
            .card_meta_put(&meta_key, &json!({ "key": key, "next_hour": h, "done": false }).to_string())
            .await?;
        if h <= env.now_hour && !env.pause.is_zero() {
            tokio::time::sleep(env.pause).await;
        }
    }
    fam.catching_up = false;
    Ok(())
}

async fn load_ranges<C: CellData>(env: &Env<'_>, key: &str, ranges: &[(i64, i64)]) -> anyhow::Result<BTreeMap<i64, C>> {
    let mut out = BTreeMap::new();
    for (from, to) in ranges {
        for (h, text) in env.store.card_cells_range(C::FAMILY, key, *from, *to).await? {
            out.insert(h, C::from_json(&text)?);
        }
    }
    Ok(out)
}

/// The versions of `hours` currently in the windows (= stored, for hours ≤ now_hour).
async fn applied_cells<C: CellData>(
    env: &Env<'_>,
    fam: &Family<C>,
    key: &str,
    hours: &[i64],
) -> anyhow::Result<BTreeMap<i64, C>> {
    let mut out = BTreeMap::new();
    let mut missing = Vec::new();
    for h in hours.iter().filter(|h| **h <= fam.windows.now_hour) {
        match fam.windows.recent.get(h) {
            Some(c) => {
                out.insert(*h, c.clone());
            }
            None => missing.push(*h),
        }
    }
    for (h, text) in env.store.card_cells_at(C::FAMILY, key, &missing).await? {
        out.insert(h, C::from_json(&text)?);
    }
    Ok(out)
}

async fn put_cells<C: CellData>(env: &Env<'_>, key: &str, built: &[(i64, C)]) -> anyhow::Result<()> {
    let mut rows = Vec::with_capacity(built.len());
    for (h, c) in built {
        rows.push(CardCellRow {
            card: C::FAMILY.to_string(),
            premise_key: key.to_string(),
            hour_ms: *h,
            data: c.to_json()?,
            computed_ms: env.now_ms,
        });
    }
    env.store.card_cells_put(&rows).await?;
    Ok(())
}

fn to_ep<C: CellData>(e: CardEpisode, fam: &Family<C>, now_ms: i64) -> Option<Ep> {
    let side = Side::from_kind(&e.kind)?;
    let end_ms = e.closed_ms.unwrap_or_else(|| fam.run_end(e.run_id, now_ms)).max(e.opened_ms);
    Some(Ep {
        id: e.episode_id,
        run_id: e.run_id,
        side,
        opened_ms: e.opened_ms,
        closed_ms: e.closed_ms,
        end_ms,
        duration_ms: e.duration_ms,
        censor_left: e.censor_left,
        censor_right: e.censor_right,
        censor_gap: e.censor_gap,
        edge_net: e.edge_net,
        edge_gross: e.edge_gross,
        pnl_net: e.pnl_net,
        pnl_gross: e.pnl_gross,
        cost: e.cost,
        budget100: e.budget100,
        suspect: e.suspect,
        stale: e.stale,
        prev_gap_ms: e.prev_gap_ms,
    })
}

type Latency = BTreeMap<i64, Vec<i64>>;
type Deltas = BTreeMap<i64, Vec<(String, Decimal)>>;

/// Edge capture's inputs for [from, to): measured latency per minute (lag p50 + RTT p50 / 2, both
/// present) and the equity change between consecutive marks of each P0 account, counted only when
/// both marks come from runs of this key.
async fn edge_inputs<C: CellData>(
    env: &Env<'_>,
    fam: &Family<C>,
    run_ids: &[i64],
    from: i64,
    to: i64,
) -> anyhow::Result<(Latency, Deltas)> {
    let mut lat = Latency::new();
    for m in env.store.health_latency(run_ids, from, to).await? {
        if let (Some(l), Some(r)) = (m.lag_p50_ms, m.rest_rtt_p50_ms) {
            lat.entry(hour_of(m.minute_ms)).or_default().push(i64::from(l) + i64::from(r) / 2);
        }
    }
    let scenario: BTreeMap<&str, &str> = env.accounts.iter().map(|(a, s)| (a.as_str(), s.as_str())).collect();
    let ids: Vec<String> = env.accounts.iter().map(|(a, _)| a.clone()).collect();
    let marks = env.store.marks_with_prev(&ids, from, to).await?;
    let mut deltas = Deltas::new();
    for pair in marks.windows(2) {
        let (p, m) = (&pair[0], &pair[1]);
        if p.account_id != m.account_id || m.t_ms < from {
            continue;
        }
        if !(fam.runs.contains_key(&p.run_id) && fam.runs.contains_key(&m.run_id)) {
            continue;
        }
        if let Some(s) = scenario.get(m.account_id.as_str()) {
            deltas.entry(hour_of(m.t_ms)).or_default().push((s.to_string(), m.equity - p.equity));
        }
    }
    Ok((lat, deltas))
}

async fn build_hours<C: CellData>(env: &Env<'_>, fam: &Family<C>, hours: &[i64]) -> anyhow::Result<Vec<(i64, C)>> {
    let (Some(first), Some(last)) = (hours.first(), hours.last()) else {
        return Ok(Vec::new());
    };
    let (from, to) = (*first, *last + HOUR_MS);
    let run_ids = fam.run_ids();
    let eps: Vec<Ep> = env
        .store
        .card_episodes(&run_ids, from, to)
        .await?
        .into_iter()
        .filter_map(|e| to_ep(e, fam, env.now_ms))
        .collect();
    let (lat, deltas) = if C::EDGE_INPUTS {
        edge_inputs(env, fam, &run_ids, from, to).await?
    } else {
        (Latency::new(), Deltas::new())
    };
    let (no_lat, no_deltas): (Vec<i64>, Vec<(String, Decimal)>) = (Vec::new(), Vec::new());
    Ok(hours
        .iter()
        .map(|h| {
            let inp = HourInput {
                hour_ms: *h,
                episodes: &eps,
                latency_ms: lat.get(h).map_or(&no_lat[..], |v| &v[..]),
                equity_deltas: deltas.get(h).map_or(&no_deltas[..], |v| &v[..]),
            };
            (*h, C::build(&inp))
        })
        .collect())
}

fn family_results<C: Card>(env: &Env<'_>, fam: &Family<C>, key: &str) -> anyhow::Result<Vec<CardResultRow>> {
    let since = fam.since_ms();
    let mut out = Vec::new();
    let mut emit = |win: &str, start: Option<i64>, agg: &C, max: C::Max| -> anyhow::Result<()> {
        let from = match (start, since) {
            (Some(s), Some(k)) => Some(s.max(k)),
            (None, k) => k,
            (s, None) => s,
        };
        let ctx = BodyCtx {
            win,
            base: base_status(fam.catching_up, since, start),
            premise_key: key,
            analyze_version: env.version,
            from_ms: from,
            to_ms: Some(env.now_ms),
            since_ms: since,
            computed_ms: env.now_ms,
            scenarios: env.scenarios,
        };
        for body in C::bodies(agg, &max, &ctx) {
            out.push(CardResultRow {
                card: body.card.clone(),
                win: win.to_string(),
                premise_key: key.to_string(),
                status: body.status.as_str().to_string(),
                body: serde_json::to_string(&body)?,
                computed_ms: env.now_ms,
            });
        }
        Ok(())
    };
    for (i, (name, span)) in SPANS.iter().enumerate() {
        let start = fam.windows.span_start(*span);
        emit(name, Some(start), &fam.windows.spans[i], fam.windows.max_from(start))?;
    }
    emit("all", None, &fam.windows.all, fam.windows.max_from(i64::MIN))?;
    if C::EDGE_INPUTS {
        let day = env.now_ms - env.now_ms.rem_euclid(DAY_MS);
        let today = fam.windows.today(day);
        emit("today", Some(day), &today, fam.windows.max_from(day))?;
    }
    Ok(out)
}
```

- [ ] **Step 6: The binary.** Create `bins/analyze/src/main.rs`:

```rust
use analyze::config::Config;
use analyze::tick::Analyzer;
use std::process::ExitCode;
use std::time::Duration;
use store::Store;

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("analyze: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    analyze::guard::check_env()?;
    let cfg = Config::from_env()?;
    let scenarios = analyze::scenarios::load(&cfg.policies_path)?;
    let store = Store::connect_analyze(Store::options(&cfg.database_url)?, 2, cfg.statement_timeout_ms, None).await?;
    analyze::ensure_writes_only_cards(&store).await?;
    let mut a = Analyzer::new(store.clone(), Duration::from_millis(cfg.catchup_pause_ms), analyze::version(), scenarios).await?;
    a.write_static_meta().await?;
    eprintln!("analyze: every {} s (writes card tables only, paper only)", cfg.every_secs);
    let mut every = tokio::time::interval(Duration::from_secs(cfg.every_secs));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let stop = shutdown();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = every.tick() => {}
        }
        let now = analyze::now_ms();
        match a.tick(now).await {
            Ok(r) => eprintln!(
                "analyze: tick {} ms, {} dirty hours, {} catch-up hours, {} results",
                r.ms, r.dirty_hours, r.catch_up_hours, r.results
            ),
            Err(e) => {
                eprintln!("analyze: tick failed: {e:#}");
                a.record_error(now, &format!("{e:#}")).await;
                a.reset();
            }
        }
    }
    store.close().await;
    Ok(())
}

/// SIGTERM (docker stop) or Ctrl-C. A tick in progress finishes first (its writes are idempotent).
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

- [ ] **Step 7: Run the analyze tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p analyze`
Expected: PASS: 7 tests in `tests/tick.rs`, the config, scenarios and guard unit tests; none prints `SKIPPED`.

- [ ] **Step 8: The arch rules.** In `bins/scanner/tests/arch.rs`, inside `crate_graph_matches_spec_9_2`:
  - change `for pure in ["arb-core", "sim"] {` to `for pure in ["arb-core", "sim", "cards"] {`
  - after the `api` loop, add:

```rust
    for bad in ["reqwest", "tokio-tungstenite", "sqlx", "pm-api"] {
        assert!(
            !d["analyze"].contains(bad),
            "analyze reads Postgres only via store and has no network client ({bad})"
        );
    }
    assert!(!d["cards"].contains("store"), "cards is pure: analyze does the IO");
```

Then in `scanner_and_api_guard_denylists_are_identical`, after `let api_prefixes = …;` add

```rust
    let analyze_src =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../analyze/src/guard.rs"))
            .expect("analyze guard.rs");
    assert_eq!(scanner_contains, denylist(&analyze_src, "CONTAINS"), "CONTAINS drifted in analyze");
    assert_eq!(scanner_prefixes, denylist(&analyze_src, "PREFIXES"), "PREFIXES drifted in analyze");
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p scanner --test arch`
Expected: PASS (2 tests).

- [ ] **Step 9: The benchmark (Review Focus 5).** Create `crates/cards/tests/bench_tick.rs`:

```rust
//! Spec §9 benchmark (ignored by default; run by hand, debug build): one tick's compute over a
//! synthetic history of ~350 episodes/hour must stay under 200 ms, at 30 and at 90 days of history,
//! because a tick never re-merges every cell. Catch-up is timed and printed separately. The tick
//! part mirrors analyze: advance one hour (parse the stored cells leaving the spans), rebuild the
//! two dirty hours, store them as JSON, swap them in, and render every window's bodies to JSON.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::{BodyCtx, Card, ScenarioLat, base_status};
use cards::cells::*;
use cards::edge::EdgeCell;
use cards::window::{SPANS, Windows};
use cards::{DAY_MS, HOUR_MS as H};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::time::Instant;

const PER_HOUR: i64 = 350;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: i64) -> i64 {
        i64::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// The episodes opened in hour `h` (deterministic per hour).
fn hour_eps(h: i64) -> Vec<Ep> {
    let mut r = Lcg(u64::try_from(h / H).unwrap());
    (0..PER_HOUR)
        .map(|i| {
            let opened = h + r.below(H);
            let dur = match r.below(100) {
                0..=79 => 1 + r.below(60_000),
                80..=97 => 1 + r.below(600_000),
                _ => 1 + r.below(4 * H),
            };
            let flags = r.below(100);
            Ep {
                id: format!("e{}-{i}", h / H),
                run_id: 1,
                side: if r.below(10) == 0 { Side::No } else { Side::Yes },
                opened_ms: opened,
                closed_ms: Some(opened + dur),
                end_ms: opened + dur,
                duration_ms: Some(dur),
                censor_left: flags == 0,
                censor_right: flags == 1,
                censor_gap: flags == 2,
                edge_net: Decimal::new(1 + r.below(60_000), 6),
                edge_gross: Decimal::new(60_000 + r.below(60_000), 6),
                pnl_net: Decimal::new(1 + r.below(500), 2),
                pnl_gross: Decimal::new(600, 2),
                cost: Decimal::new(1 + r.below(15_000), 2),
                budget100: Some(Decimal::new(1 + r.below(400), 2)),
                suspect: flags == 3,
                stale: flags == 4,
                prev_gap_ms: (flags < 20).then(|| r.below(1_000)),
            }
        })
        .collect()
}

/// What the store would return for hour h: episodes of the 4 hours before still overlapping, and h's own.
fn view(h: i64) -> Vec<Ep> {
    (0..=4).flat_map(|k| hour_eps(h - k * H)).filter(|e| e.end_ms >= h && e.opened_ms < h + H).collect()
}

fn deltas() -> Vec<(String, Decimal)> {
    (0..60).map(|i| ("sp-idle".to_string(), Decimal::new(i - 30, 2))).collect()
}

struct Run<C: CellData> {
    stored: BTreeMap<i64, String>,
    w: Windows<C>,
}

fn catch_up<C: CellData>(first: i64, now: i64) -> Run<C> {
    let lat: Vec<i64> = (0..60).map(|i| 150 + i).collect();
    let d = deltas();
    let mut run = Run { stored: BTreeMap::new(), w: Windows::new(now) };
    let mut h = first;
    while h <= now {
        let v = view(h);
        let c = C::build(&HourInput { hour_ms: h, episodes: &v, latency_ms: &lat, equity_deltas: &d });
        run.stored.insert(h, c.to_json().unwrap());
        run.w.apply(h, None, &c);
        h += H;
    }
    run
}

/// One tick at `to` (= now + 1 h) with `inputs` pre-read (the store's part). Returns the JSON bytes written.
fn tick<C: Card>(run: &mut Run<C>, to: i64, inputs: &[(i64, Vec<Ep>)], sc: &[ScenarioLat]) -> usize {
    let lat: Vec<i64> = (0..60).map(|i| 150 + i).collect();
    let d = deltas();
    let mut stored_needed = BTreeMap::new();
    for (from, until) in run.w.needed_ranges(to) {
        for (h, text) in run.stored.range(from..until) {
            if !run.w.recent.contains_key(h) {
                stored_needed.insert(*h, C::from_json(text).unwrap());
            }
        }
    }
    run.w.advance(to, &stored_needed);
    let mut bytes = 0;
    for (h, v) in inputs {
        let c = C::build(&HourInput { hour_ms: *h, episodes: v, latency_ms: &lat, equity_deltas: &d });
        let json = c.to_json().unwrap();
        bytes += json.len();
        run.stored.insert(*h, json);
        let old = run.w.recent.get(h).cloned();
        run.w.apply(*h, old.as_ref(), &c);
    }
    let key = "k";
    let mut emit = |win: &str, start: Option<i64>, agg: &C, max: C::Max| {
        let ctx = BodyCtx {
            win,
            base: base_status(false, Some(0), start),
            premise_key: key,
            analyze_version: "bench",
            from_ms: start,
            to_ms: Some(to),
            since_ms: Some(0),
            computed_ms: to,
            scenarios: sc,
        };
        for b in C::bodies(agg, &max, &ctx) {
            bytes += serde_json::to_string(&b).unwrap().len();
        }
    };
    for (i, (name, span)) in SPANS.iter().enumerate() {
        let s = run.w.span_start(*span);
        emit(name, Some(s), &run.w.spans[i], run.w.max_from(s));
    }
    emit("all", None, &run.w.all, run.w.max_from(i64::MIN));
    if C::EDGE_INPUTS {
        let day = to - to.rem_euclid(DAY_MS);
        let today = run.w.today(day);
        emit("today", Some(day), &today, run.w.max_from(day));
    }
    bytes
}

fn family_tick_ms<C: Card>(days: i64, sc: &[ScenarioLat]) -> u128 {
    let now = 1_000_000 * H;
    let first = now - (days * 24 - 1) * H;
    let t = Instant::now();
    let mut run: Run<C> = catch_up(first, now);
    let catch_ms = t.elapsed().as_millis();
    let to = now + H;
    let inputs: Vec<(i64, Vec<Ep>)> = vec![(now, view(now)), (to, view(to))];
    let t = Instant::now();
    let bytes = tick(&mut run, to, &inputs, sc);
    let ms = t.elapsed().as_millis();
    println!("days={days} family={} catch-up={catch_ms} ms tick={ms} ms json={bytes} B", C::FAMILY);
    ms
}

fn tick_ms(days: i64) -> u128 {
    let sc = vec![
        ScenarioLat { name: "colo-idle".into(), net_ms: 1, latency_ms: 52 },
        ScenarioLat { name: "sp-idle".into(), net_ms: 145, latency_ms: 300 },
    ];
    family_tick_ms::<G1Cell>(days, &sc)
        + family_tick_ms::<DurCell>(days, &sc)
        + family_tick_ms::<G4Cell>(days, &sc)
        + family_tick_ms::<EdgeCell>(days, &sc)
}

#[test]
#[ignore = "benchmark: run by hand (Phase 5a plan, Task 9)"]
fn tick_stays_under_200ms_as_all_grows_from_30_to_90_days() {
    let t30 = tick_ms(30);
    println!("tick total at 30 days: {t30} ms");
    assert!(t30 < 200, "one tick over 30 days took {t30} ms (budget 200 ms, spec §9)");
    let t90 = tick_ms(90);
    println!("tick total at 90 days: {t90} ms");
    assert!(t90 < 200, "one tick over 90 days took {t90} ms: the tick must not grow with 'all'");
}
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test bench_tick -- --ignored --nocapture`
Expected: PASS, printing one line per family and window length (e.g. `days=30 family=dur catch-up=… ms tick=… ms json=… B`) and `tick total at 30 days: N ms` / `… at 90 days: N ms` with both N < 200. The catch-up lines take the longest (tens of seconds in debug); that is the separately measured first start. If a total is at or over 200 ms, stop and report the per-family lines: do not raise the budget.

- [ ] **Step 10: Lint and the whole workspace.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test --workspace`
Expected: no warnings; every test passes (the ignored benches stay ignored); none prints `SKIPPED`. Then check disk: `du -sh target` — run `cargo clean` if it is over 8 GB.

- [ ] **Step 11: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add Cargo.toml Cargo.lock crates/cards/src/lib.rs crates/cards/src/family.rs crates/cards/tests/family.rs crates/cards/tests/bench_tick.rs bins/analyze/Cargo.toml bins/analyze/src/lib.rs bins/analyze/src/config.rs bins/analyze/src/guard.rs bins/analyze/src/scenarios.rs bins/analyze/src/tick.rs bins/analyze/src/main.rs bins/analyze/tests/common/mod.rs bins/analyze/tests/tick.rs bins/scanner/tests/arch.rs && git commit -m "analyze: 15-minute tick over dirty hours, catch-up in 24 h batches, premise keys and statuses, write-refusal check, arch rules, benchmark"
```

---

### Task 10: api: `GET /api/cards?win=` and `GET /api/cards/meta`

**Files:**
- Create: `bins/api/src/cards.rs`, `bins/api/tests/cards.rs`
- Modify: `bins/api/src/lib.rs` (`pub mod cards;` + two routes), `bins/api/src/params.rs` (`win`)

**Interfaces:**
- Consumes: `Store::card_results_for(win) -> Vec<(card, body)>`, `Store::card_meta_all() -> Vec<(key, value)>` (Task 3); `ApiError`, `params::{query, Params}`, `AppState` (existing); the api test harness `common::{fresh, app, get}` (existing).
- Produces:
  - `GET /api/cards?win=24h|7d|30d|all|today` → `{ "win": "7d", "cards": [CardBody, …] }` (bodies exactly as analyze stored them, ordered by card; `today` returns `EDGE` only; `win` missing → `7d`; anything else → 400 `bad_win`).
  - `GET /api/cards/meta` → `{ "tick": {last_tick_ms, analyze_version, tick_ms} | null, "last_error": {ms, text} | null, "catch_up": { "g1": {key, next_hour, done}, … }, "papers": [...], "not_replicated": [...] }` (nulls and empty lists before analyze's first tick).
  - `params::win(q: &Params) -> Result<&'static str, ApiError>`, `cards::WINS: [&str; 5]`.

- [ ] **Step 1: Write the failing tests.** Create `bins/api/tests/cards.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use serde_json::json;

async fn seed(owner: &store::Store, card: &str, win: &str, body: serde_json::Value) {
    let sql = format!(
        "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('{card}', '{win}', 'k', '{}', '{}'::jsonb, 1)",
        body["status"].as_str().unwrap_or("live"),
        body.to_string().replace('\'', "''")
    );
    owner.raw_exec(&sql).await.unwrap();
}

fn body(card: &str, win: &str, ours: Option<&str>) -> serde_json::Value {
    json!({
        "card": card, "paper": "gebele-2026", "title": format!("{card} · x"), "win": win, "status": "thin",
        "premise_key": "k", "hash": "ab".repeat(32), "data_from_ms": 1, "data_to_ms": 2, "since_ms": 1, "n": 4,
        "columns": [
            { "label": "NO-side episodes", "unit": "count", "paper": "36", "their": "0", "ours": "0" },
            { "label": "YES : NO", "unit": "×", "paper": "58.28", "their": null, "ours": ours }
        ],
        "series": {}, "notes": [], "thin_reasons": ["NO side: n = 0 < 10, so the ratio is not reliable"],
        "episode_link": null, "computed_ms": 2
    })
}

#[tokio::test]
async fn cards_returns_every_card_of_a_window_as_stored() {
    let Some((owner, db)) = common::fresh().await else { return };
    for card in ["G2", "G1", "EDGE"] {
        seed(&owner, card, "7d", body(card, "7d", Some("12345678901234567.89"))).await;
    }
    seed(&owner, "G1", "30d", body("G1", "30d", None)).await;
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards?win=7d").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["win"], "7d");
    let cards: Vec<&str> = v["cards"].as_array().unwrap().iter().map(|c| c["card"].as_str().unwrap()).collect();
    assert_eq!(cards, ["EDGE", "G1", "G2"]);
    let (_, d) = common::get(&app, "/api/cards").await;
    assert_eq!(d["win"], "7d", "win defaults to 7d");
    let (_, m) = common::get(&app, "/api/cards?win=30d").await;
    assert_eq!(m["cards"].as_array().unwrap().len(), 1);
    let (_, e) = common::get(&app, "/api/cards?win=24h").await;
    assert_eq!(e["cards"], json!([]));
    db.drop(owner).await;
}

#[tokio::test]
async fn decimals_stay_strings_and_null_stays_distinct_from_zero() {
    let Some((owner, db)) = common::fresh().await else { return };
    seed(&owner, "G1", "7d", body("G1", "7d", Some("12345678901234567.89"))).await;
    seed(&owner, "G1", "all", body("G1", "all", None)).await;
    let (app, _) = common::app(&db).await;
    let (_, v) = common::get(&app, "/api/cards?win=7d").await;
    let cols = &v["cards"][0]["columns"];
    assert_eq!(cols[0]["ours"], "0", "a measured zero is the string \"0\"");
    assert_eq!(cols[1]["ours"], "12345678901234567.89", "never rounded through a float");
    assert!(cols[1]["their"].is_null());
    let (_, a) = common::get(&app, "/api/cards?win=all").await;
    assert!(a["cards"][0]["columns"][1]["ours"].is_null(), "null (not measured) is not \"0\"");
    db.drop(owner).await;
}

#[tokio::test]
async fn today_returns_edge_capture_only() {
    let Some((owner, db)) = common::fresh().await else { return };
    seed(&owner, "EDGE", "today", body("EDGE", "today", Some("1.5"))).await;
    seed(&owner, "G1", "today", body("G1", "today", Some("2"))).await; // never written by analyze; filtered anyway
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards?win=today").await;
    assert_eq!(s, StatusCode::OK);
    let cards: Vec<&str> = v["cards"].as_array().unwrap().iter().map(|c| c["card"].as_str().unwrap()).collect();
    assert_eq!(cards, ["EDGE"]);
    db.drop(owner).await;
}

#[tokio::test]
async fn a_bad_win_is_a_json_400() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _) = common::app(&db).await;
    for q in ["1h", "7D", "all%20", "%27%20OR%201%3D1", "7d%00"] {
        let (s, v) = common::get(&app, &format!("/api/cards?win={q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
        assert_eq!(v["error"], "bad_win", "{q}");
        assert_eq!(v["detail"], "win must be 24h, 7d, 30d, all or today.");
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn meta_is_empty_before_analyze_runs_and_complete_after() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (app, _) = common::app(&db).await;
    let (s, v) = common::get(&app, "/api/cards/meta").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({ "tick": null, "last_error": null, "catch_up": {}, "papers": [], "not_replicated": [] }));
    owner
        .raw_exec(
            "INSERT INTO card_meta (key, value) VALUES \
             ('tick', '{\"last_tick_ms\": 5, \"analyze_version\": \"abc\", \"tick_ms\": 12}'), \
             ('last_error', 'null'), \
             ('cursor', '{\"updated_ms\": 9}'), \
             ('fam:g1', '{\"key\": \"k\", \"next_hour\": 3600000, \"done\": false}'), \
             ('papers', '[{\"id\": \"gebele-2026\", \"status\": \"live\"}]'), \
             ('not_replicated', '[{\"item\": \"Converter profits\"}]')",
        )
        .await
        .unwrap();
    let (_, v) = common::get(&app, "/api/cards/meta").await;
    assert_eq!(v["tick"]["analyze_version"], "abc");
    assert!(v["last_error"].is_null());
    assert_eq!(v["catch_up"]["g1"]["done"], false);
    assert_eq!(v["papers"][0]["status"], "live");
    assert_eq!(v["not_replicated"][0]["item"], "Converter profits");
    assert!(v.get("cursor").is_none(), "internal keys stay internal");
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_role_still_cannot_write_card_results() {
    let Some((owner, db)) = common::fresh().await else { return };
    let ro = store::Store::connect_read_only(db.opts.clone(), 1, 5_000, Some("analyze_ro")).await.unwrap();
    let err = ro
        .raw_exec("INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) VALUES ('G1', '7d', 'k', 'live', '{}', 1)")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("read-only transaction") || err.to_string().contains("permission denied"), "{err}");
    api::ensure_read_only(&ro).await.unwrap();
    ro.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p api --test cards`
Expected: FAIL: the new routes answer the JSON 404 (`assertion failed … left: 404, right: 200` / `left: 404, right: 400`); `the_api_role_still_cannot_write_card_results` passes already.

- [ ] **Step 3: The parameter.** Append to `bins/api/src/params.rs`:

```rust
/// `win` of /api/cards: one of cards::WINS; missing or empty is 7d. Anything else is a 400.
pub fn win(q: &Params) -> Result<&'static str, ApiError> {
    match q.get("win").map(String::as_str) {
        None | Some("") => Ok("7d"),
        Some(w) => crate::cards::WINS
            .iter()
            .find(|x| **x == w)
            .copied()
            .ok_or_else(|| bad("bad_win", "win must be 24h, 7d, 30d, all or today.")),
    }
}
```

- [ ] **Step 4: The handlers.** Create `bins/api/src/cards.rs`:

```rust
//! GET /api/cards?win= and GET /api/cards/meta (Phase 5a spec §8.1). Both read what analyze wrote:
//! one small query each on card_results / card_meta. Bodies pass through as stored (decimal
//! strings, null distinct from "0"); the api computes nothing.
use crate::AppState;
use crate::error::ApiError;
use crate::params::{self, Params};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub const WINS: [&str; 5] = ["24h", "7d", "30d", "all", "today"];

#[derive(Debug, Serialize)]
pub struct CardsDto {
    pub win: String,
    pub cards: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub struct MetaDto {
    pub tick: Value,
    pub last_error: Value,
    pub catch_up: BTreeMap<String, Value>,
    pub papers: Value,
    pub not_replicated: Value,
}

fn parse(what: &str, text: &str) -> Result<Value, ApiError> {
    serde_json::from_str(text).map_err(|e| {
        eprintln!("api: unreadable {what}: {e}");
        ApiError::Internal
    })
}

pub async fn list(
    State(st): State<AppState>,
    q: Result<Query<Params>, QueryRejection>,
) -> Result<Json<CardsDto>, ApiError> {
    let q = params::query(q)?;
    let win = params::win(&q)?;
    let mut cards = Vec::new();
    for (card, body) in st.store.card_results_for(win).await? {
        // spec §8.1: win=today returns Edge capture only
        if win == "today" && card != "EDGE" {
            continue;
        }
        cards.push(parse(&format!("card {card}"), &body)?);
    }
    Ok(Json(CardsDto { win: win.to_string(), cards }))
}

pub async fn meta(State(st): State<AppState>) -> Result<Json<MetaDto>, ApiError> {
    let mut dto = MetaDto {
        tick: Value::Null,
        last_error: Value::Null,
        catch_up: BTreeMap::new(),
        papers: Value::Array(Vec::new()),
        not_replicated: Value::Array(Vec::new()),
    };
    for (key, value) in st.store.card_meta_all().await? {
        let v = parse(&format!("meta {key}"), &value)?;
        match key.as_str() {
            "tick" => dto.tick = v,
            "last_error" => dto.last_error = v,
            "papers" => dto.papers = v,
            "not_replicated" => dto.not_replicated = v,
            k => {
                if let Some(fam) = k.strip_prefix("fam:") {
                    dto.catch_up.insert(fam.to_string(), v);
                }
            }
        }
    }
    Ok(Json(dto))
}
```

In `bins/api/src/lib.rs`, add `pub mod cards;` after `pub mod config;`, and in `router`, after `.route("/api/paper/ledger", get(paper::ledger))` add:

```rust
        .route("/api/cards", get(cards::list))
        .route("/api/cards/meta", get(cards::meta))
```

- [ ] **Step 5: Run the tests to verify they pass.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test -p api`
Expected: PASS: 6 tests in `tests/cards.rs` and every existing api test; none prints `SKIPPED`.

- [ ] **Step 6: Lint.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy -p api --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add bins/api/src/cards.rs bins/api/src/lib.rs bins/api/src/params.rs bins/api/tests/cards.rs && git commit -m "api: GET /api/cards?win= and /api/cards/meta (read-only pass-through of card_results and card_meta)"
```

---

### Task 11: web: readability pass (IBM Plex Mono, responsive at 360 px) and (?) help on every section

**Files:**
- Create: `web/src/help.ts`, `web/src/components/Help.tsx`, `web/src/components/Help.test.tsx`, `web/src/theme.test.ts`
- Modify: `web/index.html`, `web/src/theme.css`, `web/src/components/Box.tsx`, `web/src/pages/Radar.tsx`, `web/src/pages/Episodes.tsx`, `web/src/pages/EpisodeDetail.tsx`, `web/src/pages/Paper.tsx`, `web/src/pages/Health.tsx`, `web/src/pages/Radar.test.tsx`

**Interfaces:**
- Consumes: the existing web app (Phase 4).
- Produces:
  - `web/src/help.ts`: `interface HelpText { title: string; text: string }`, `const HELP` (ids below), `type HelpId = keyof typeof HELP`
  - `web/src/components/Help.tsx`: `Help({ id }: { id: HelpId })` — a `(?)` button, `aria-label="Help: <title>"`, `aria-expanded`, `aria-controls` → a `role="note"` `aria-live="polite"` popover; click / Enter / Space toggle; Escape closes and returns focus.
  - `Box` now takes a required `help: HelpId` and renders `<div class="box-head"><h2>title</h2><Help/></div>`.
  - Help ids: Radar `health`, `open`, `today`; Episodes `filters`, `episodes`; EpisodeDetail `timing`, `peak`, `flags`, `legs`, `rechecks`; Paper `scenario`, `equity`, `awaiting`, `ledger`, `paper` (+ `edge` in Task 13); Health `window`, `capture`, `perMinute`, `msgs`, `lag`, `drops`, `integrity`; Papers (Task 12) `papers`, `gebele`, `otherPapers`, `notReplicated`, `status`, `g1`, `g2`, `g3`, `g4`.
  - CSS tokens: `--body` = IBM Plex Mono, `--big` = VT323; classes `.box-head`, `.help`, `.help-btn`, `.help-pop`, `.big`, `[hidden]`.

Web commands run inside `web/` with the existing `node_modules` (never `npm install`).

- [ ] **Step 1: Write the failing tests.** Create `web/src/components/Help.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { HELP } from '../help';
import { Help } from './Help';

describe('Help (?)', () => {
  it('toggles on click, is announced to screen readers, and closes on Escape', async () => {
    render(<Help id="open" />);
    const btn = screen.getByRole('button', { name: `Help: ${HELP.open.title}` });
    expect(btn).toHaveTextContent('(?)');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
    const pop = document.getElementById(btn.getAttribute('aria-controls') ?? 'missing');
    expect(pop).not.toBeNull();
    expect(pop).not.toBeVisible();
    await userEvent.click(btn);
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    expect(pop).toBeVisible();
    expect(pop).toHaveAttribute('aria-live', 'polite');
    expect(pop).toHaveTextContent(HELP.open.text);
    await userEvent.keyboard('{Escape}');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
    expect(btn).toHaveFocus();
  });

  it('opens and closes from the keyboard with Enter and Space', async () => {
    render(<Help id="g2" />);
    const btn = screen.getByRole('button', { name: /^Help: / });
    btn.focus();
    await userEvent.keyboard('{Enter}');
    expect(btn).toHaveAttribute('aria-expanded', 'true');
    await userEvent.keyboard(' ');
    expect(btn).toHaveAttribute('aria-expanded', 'false');
  });

  it('has plain-language copy for every id, including the spec examples', () => {
    for (const h of Object.values(HELP)) {
      expect(h.title.endsWith('?')).toBe(true);
      expect(h.text.length).toBeGreaterThan(60);
    }
    expect(HELP.open.title).toBe('What is an episode?');
    expect(HELP.edge.title).toBe('Why is the NO side cashable and the YES side locked?');
    expect(HELP.g2.title).toBe('What is Kaplan–Meier?');
    expect(HELP.status.title).toBe('What does warming mean?');
  });
});
```

Create `web/src/theme.test.ts`:

```ts
// Structural checks of the readability pass (spec §8.4). jsdom has no layout engine, so the
// 360 px no-horizontal-scroll check itself is a manual step in the runbook; this pins the rules
// that make it true. Vitest runs on Node: node:fs exists at runtime (no @types/node installed).
// @ts-ignore
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const css: string = readFileSync(`${process.cwd()}/src/theme.css`, 'utf8');
const html: string = readFileSync(`${process.cwd()}/index.html`, 'utf8');

function rule(selector: string): string {
  const i = css.indexOf(`\n${selector} {`); // at a line start: `html, body {` is not the body rule
  expect(i, `no rule for ${selector}`).toBeGreaterThanOrEqual(0);
  return css.slice(i, css.indexOf('}', i));
}

describe('theme', () => {
  it('sets IBM Plex Mono body text at 15–16 px with line-height ≥ 1.5, VT323 only for big numbers', () => {
    expect(css).toMatch(/--body:\s*'IBM Plex Mono'/);
    expect(css).toMatch(/--big:\s*'VT323'/);
    const body = rule('body');
    expect(body).toMatch(/font-family:\s*var\(--body\)/);
    const size = Number(/font-size:\s*(\d+)px/.exec(body)?.[1]);
    expect(size >= 15 && size <= 16).toBe(true);
    expect(Number(/line-height:\s*([\d.]+)/.exec(body)?.[1])).toBeGreaterThanOrEqual(1.5);
    expect(rule('table')).toMatch(/font-size:\s*15px/);
    expect(css.match(/var\(--big\)/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('loads the three fonts from Google Fonts only (the CSP stays as it is)', () => {
    expect(html).toContain('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap');
    expect(html).not.toMatch(/<script(?![^>]*type="module")/);
  });

  it('never forces a page wider than 360 px', () => {
    for (const m of css.matchAll(/(?<![-\w])(min-width|width):\s*(\d+)px/g)) {
      expect(Number(m[2]), `${m[0]}`).toBeLessThanOrEqual(328);
    }
    expect(css).not.toMatch(/minmax\(\s*\d+px/);
    expect(rule('.scroll')).toMatch(/overflow-x:\s*auto/);
    expect(rule('body')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(css).toMatch(/\[hidden\]\s*\{\s*display:\s*none\s*!important/);
  });
});
```

In `web/src/pages/Radar.test.tsx`, add inside `describe('Radar', …)`:

```tsx
  it('gives every section a (?)', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(0, 0),
    });
    renderRadar();
    await screen.findByText(EMPTY_RADAR);
    const names = screen.getAllByRole('button', { name: /^Help: / }).map((b) => b.getAttribute('aria-label'));
    expect(names).toEqual(['Help: What are the hearts?', 'Help: What is an episode?', 'Help: What does today count?']);
  });
```

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npx vitest run src/components/Help.test.tsx src/theme.test.ts src/pages/Radar.test.tsx`
Expected: FAIL: `Failed to resolve import "../help"` / `"./Help"`, the theme assertions (`--body: 'VT323'`, `font-size: 20px`, `minmax(300px`), and the Radar case (no help buttons).

- [ ] **Step 3: The copy.** Create `web/src/help.ts`:

```ts
/** (?) copy for every section of every page (spec §8.3): short, plain, deadpan but clear. */
export interface HelpText {
  title: string;
  text: string;
}

export const HELP = {
  health: {
    title: 'What are the hearts?',
    text: 'Ten hearts is a healthy scanner: every shard connected, no dropped messages in the last 5 minutes, feed lag under a second, capture running. Each trouble costs hearts and is named in words next to them.',
  },
  open: {
    title: 'What is an episode?',
    text: 'One stretch of time in which buying a full basket of one event (every YES, or every NO) cost less than it pays out, after taker fees. It opens on the first order-book update where that is true and closes on the first where it is not. Open ones sit here while they last, which is usually not long.',
  },
  today: {
    title: 'What does today count?',
    text: 'Episodes opened since 00:00 UTC. Gross ignores fees, net includes them; the difference is what the fee schedule keeps. The last line compares the edge that was there with what a 145 ms trader would have caught.',
  },
  filters: {
    title: 'What do the filters do?',
    text: 'They narrow the list by kind, day and flags, and they live in the URL, so a filtered view can be bookmarked. Days are UTC. Nothing here changes the data, only what you see of it.',
  },
  episodes: {
    title: 'What is in this list?',
    text: 'Every live episode, newest first, a page at a time. NET is the best one-shot profit after fees at the peak, Q* the size that profit needs. Flags mark episodes the engine did not fully trust; they stay listed, and cards exclude them where the method says so.',
  },
  timing: {
    title: 'What do the times mean?',
    text: 'Open and close are the exchange server times of the first and last qualifying book update. Duration is close minus open. Censored means we did not see the whole episode: it was already open at a start or reconnect, a feed gap interrupted it, or the scanner stopped first.',
  },
  peak: {
    title: 'What is the peak?',
    text: 'The moment the one-shot profit was largest. Edge is per share, Q* is how many shares the book could take at that edge, and net budget $100 is the profit if you spent at most $100. It is measured, not simulated; getting it is a separate question.',
  },
  flags: {
    title: 'What do the flags mean?',
    text: 'Suspect: a nearby book looked wrong (mirror or top mismatch), so the edge may be a data artifact. Stale: the feed lagged past the threshold. Censored: part of the episode was not observed. OUR METHOD on the cards drops suspect and stale episodes and says how many.',
  },
  legs: {
    title: 'What are legs and S?',
    text: 'A leg is one outcome of the event. S is the set of legs actually bought at the peak; legs outside S had no ask worth taking. The table shows each leg’s price, depth used and fees.',
  },
  rechecks: {
    title: 'What are the re-checks?',
    text: 'One, two and five seconds after an episode opens, the scanner asks the exchange for the same books again. They show whether the edge was still there for someone slower than the websocket. Skipped means the re-check budget was full.',
  },
  scenario: {
    title: 'What is a scenario?',
    text: 'A latency assumption for the paper accounts: how long from seeing an episode to the order reaching the matching engine. colo-idle is a machine next to the exchange; sp-idle is São Paulo on a quiet day; sp-burst the same when everyone is trading.',
  },
  equity: {
    title: 'What is equity?',
    text: 'Cash plus positions marked at the current prices, for each paper account in the chosen scenario. Every account starts with the same bankroll. No real money exists anywhere in this project.',
  },
  awaiting: {
    title: 'What is awaiting resolution?',
    text: 'Positions that only pay when the event resolves: bought YES baskets, and YES tokens returned by a NO conversion. They are marked but not cashable until the event ends.',
  },
  ledger: {
    title: 'What is the ledger?',
    text: 'Every paper cash movement of the chosen account: buys, conversions, settlements and marks, in order. Cash, fees and realized profit add up exactly; nothing is rounded on the way.',
  },
  paper: {
    title: 'What is paper trading?',
    text: 'Simulated accounts that act on live episodes under stated latency and fee assumptions, with no connection to any exchange. They show what the strategies would have done, not what anyone did.',
  },
  edge: {
    title: 'Why is the NO side cashable and the YES side locked?',
    text: 'A full NO basket can be converted back to collateral right away, so its profit is cash within seconds. A full YES basket pays only when the event resolves, which can take weeks. Edge on the table is an upper bound: it assumes we were first in line at the best moment, which nobody is every time.',
  },
  window: {
    title: 'What does the window change?',
    text: 'How far back the charts and the integrity count look. Health is kept per minute; the longest window is a week.',
  },
  capture: {
    title: 'What is capture?',
    text: 'The scanner keeps a compressed copy of every raw message for replay. It pauses when the disk budget is tight and deletes the oldest hours first, so the budget is never exceeded.',
  },
  perMinute: {
    title: 'What is in the per-minute row?',
    text: 'The latest health minute: connections, messages, drops, reconnects, feed lag and REST timings. If it is old, the scanner is not reporting and everything else on the page may be stale.',
  },
  msgs: {
    title: 'What is msgs/s?',
    text: 'Websocket messages received per second, averaged per minute. Bursts come with news and market opens; that is when latency matters most.',
  },
  lag: {
    title: 'What is lag?',
    text: 'The time between the exchange stamping a message and the scanner receiving it, at the median and the 99th percentile. It is part of the measured latency on the Edge capture chart.',
  },
  drops: {
    title: 'What are drops and reconnects?',
    text: 'Drops are messages the scanner could not process in time; reconnects are websocket sessions that had to be reopened. Both censor episodes that were open at the time.',
  },
  integrity: {
    title: 'What is integrity?',
    text: 'Checks that the books we hold agree with the exchange: crossed books, mirror mismatches, drifts and gaps, per million messages. It is our own version of the paper’s validation appendix.',
  },
  papers: {
    title: 'What is this page?',
    text: 'Published results on prediction-market arbitrage, recomputed live on our own data. Each card shows the paper’s number, the paper’s method on our data, and our method, with the sample size, a status and a hash that identifies exactly what was computed.',
  },
  gebele: {
    title: 'What does this paper measure?',
    text: 'How often executable arbitrage appears on Polymarket, on which side, how long it lasts and how big it is, from order-book data over five weeks in 2026. The cards replay those measurements on what the scanner sees, every 15 minutes.',
  },
  otherPapers: {
    title: 'Why is this paper not live?',
    text: 'Some results need data we do not have yet (on-chain history) or were dropped. They are listed so the page does not pretend they were never read.',
  },
  notReplicated: {
    title: 'What is not replicated, and why?',
    text: 'Results that come from on-chain conversions or from the paper’s own message validation. We do not observe them, so we do not print a number we cannot compute.',
  },
  status: {
    title: 'What does warming mean?',
    text: 'live: the whole window is covered by data computed under the current premises. warming: the premises changed or the data starts inside the window, so it is only partly filled. thin: too few episodes for the number to mean much; it is dimmed, with the reason. under review: the premises changed and the history is being recomputed.',
  },
  g1: {
    title: 'What is side asymmetry?',
    text: 'Whether mispricing shows up more on the YES side (all YES tokens together cost under $1) or the NO side (all NO tokens cost under the number of outcomes minus one). The paper found YES far more often. The merge rows show how much flickering books inflate the count.',
  },
  g2: {
    title: 'What is Kaplan–Meier?',
    text: 'A way to estimate how long episodes last when some were still running when we stopped watching. Instead of dropping those, it counts them as lasted-at-least-this-long. The paper uses the plain share of finished episodes longer than τ, drawn next to it.',
  },
  g3: {
    title: 'What is the long-lived share?',
    text: 'The share of episodes that last 50 minutes or more. The paper measured it inside one-hour windows, which can only see episodes present at both ends of the hour; our method measures whole durations and reports how much the one-hour view shifts the median.',
  },
  g4: {
    title: 'What is net edge by side?',
    text: 'The best per-share edge of each episode after taker fees, in basis points (1 bp = $0.0001 per $1), binned by 25 bp. Gross, before fees, is drawn behind it. The paper shows this only as a figure, so only its counts are quoted.',
  },
} satisfies Record<string, HelpText>;

export type HelpId = keyof typeof HELP;
```

- [ ] **Step 4: The component and the Box.** Create `web/src/components/Help.tsx`:

```tsx
import { useEffect, useId, useRef, useState } from 'react';
import { HELP, type HelpId } from '../help';

/** The (?) button (spec §8.3): click, Enter or Space toggles; Escape closes and returns focus. */
export function Help({ id }: { id: HelpId }) {
  const [open, setOpen] = useState(false);
  const popId = useId();
  const btn = useRef<HTMLButtonElement>(null);
  const h = HELP[id];
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        btn.current?.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);
  return (
    <span className="help">
      <button
        ref={btn}
        type="button"
        className="help-btn"
        aria-label={`Help: ${h.title}`}
        aria-expanded={open}
        aria-controls={popId}
        onClick={() => setOpen((o) => !o)}
      >
        (?)
      </button>
      <span id={popId} className="help-pop" role="note" aria-live="polite" hidden={!open}>
        {open && (
          <>
            <b>{h.title}</b> {h.text}
          </>
        )}
      </span>
    </span>
  );
}
```

Replace `web/src/components/Box.tsx` with:

```tsx
import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** A page section. `help` is required: every section of every page has a (?) (spec §8.3). */
export function Box({ title, help, className, children }: { title?: ReactNode; help: HelpId; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `box ${className}` : 'box'}>
      <div className="box-head">
        {title !== undefined && <h2>{title}</h2>}
        <Help id={help} />
      </div>
      {children}
    </section>
  );
}
```

- [ ] **Step 5: Give every existing section its help id.** Make exactly these edits (TypeScript now rejects any `<Box` without `help`):
  - `web/src/pages/Radar.tsx`: `<Box><EmptyState text={NO_HEALTH} /></Box>` → `<Box help="health"><EmptyState text={NO_HEALTH} /></Box>`; add `help="open"` to the `OPEN EPISODES` Box and `help="today"` to the `TODAY (UTC)` Box; in `HealthBar`, add `<Help id="health" />` as the last child of the `<section className="box health" …>` and `import { Help } from '../components/Help';`.
  - `web/src/pages/Episodes.tsx`: `<Box title="FILTERS">` → `<Box title="FILTERS" help="filters">`; `<Box title="EPISODES">` → `<Box title="EPISODES" help="episodes">`.
  - `web/src/pages/EpisodeDetail.tsx`: `TIMING` → `help="timing"`, `AT PEAK` → `help="peak"`, `FLAGS, IN WORDS` → `help="flags"`, the `LEGS AT PEAK · S = …` Box → `help="legs"`, `RE-CHECK TIMELINE` → `help="rechecks"`.
  - `web/src/pages/Paper.tsx`: the `EQUITY · …` Box → `help="equity"`, the `LEDGER · …` Box → `help="ledger"`, `SCENARIO` → `help="scenario"`, `AWAITING RESOLUTION` → `help="awaiting"`, `PAPER` (empty state) → `help="paper"`.
  - `web/src/pages/Health.tsx`: `WINDOW` → `help="window"`, `CAPTURE` → `help="capture"`, `PER MINUTE` → `help="perMinute"`, `MSGS/S` → `help="msgs"`, `LAG (MS)` → `help="lag"`, `DROPS AND RECONNECTS` → `help="drops"`, `INTEGRITY` → `help="integrity"`.

- [ ] **Step 6: Fonts and the responsive pass.** In `web/index.html`, replace the stylesheet line with:

```html
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap" />
```

In `web/src/theme.css`:
  - in `:root`, replace `--body: 'VT323', monospace;` with `--body: 'IBM Plex Mono', ui-monospace, monospace;` and add the line `--big: 'VT323', monospace;` after it;
  - replace the `body { … }` rule with `body { font-family: var(--body); font-size: 15px; line-height: 1.55; padding: 16px; min-height: 100vh; overflow-wrap: anywhere; }`;
  - in `table { … }`, change `font-size: 20px;` to `font-size: 15px;`;
  - replace `.card .eq { font-family: var(--px); font-size: 12px; margin-bottom: 4px; }` with `.card .eq { font-family: var(--big); font-size: 32px; line-height: 1; margin-bottom: 4px; }`;
  - in `.grid2`, change `minmax(300px, 1fr)` to `minmax(min(100%, 300px), 1fr)`; in `.cards`, change `minmax(200px, 1fr)` to `minmax(min(100%, 200px), 1fr)`;
  - after the `button { … }` rule add:

```css
[hidden] { display: none !important; }
.big { font-family: var(--big); font-size: 32px; line-height: 1; }
.box-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 8px; }
.box-head h2 { flex: 1; min-width: 0; }
.help { position: relative; flex: none; }
.help-btn { font-family: var(--px); font-size: 8px; padding: 3px 5px; border: 2px solid var(--line); background: var(--chip); cursor: pointer; }
.help-pop { position: absolute; right: 0; top: 130%; width: min(320px, 80vw); z-index: 3; background: var(--bar); border: 2px solid var(--line); box-shadow: 4px 4px 0 var(--line); padding: 8px; font-size: 14px; line-height: 1.5; }
.help-pop b { display: block; font-family: var(--px); font-size: 8px; margin-bottom: 6px; line-height: 1.5; }
```

  - inside `@media (max-width: 640px) { … }`, add `.box { padding: 8px; }` and `.tag { white-space: normal; }`.

- [ ] **Step 7: Run the whole web suite and the build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test && npm run build`
Expected: every test passes (the 3 Help cases, 3 theme cases, the new Radar case and every existing test); `tsc --noEmit` reports no error (a `<Box` without `help` would fail here); `vite build` writes `dist/`.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/index.html web/src/theme.css web/src/theme.test.ts web/src/help.ts web/src/components/Help.tsx web/src/components/Help.test.tsx web/src/components/Box.tsx web/src/pages/Radar.tsx web/src/pages/Radar.test.tsx web/src/pages/Episodes.tsx web/src/pages/EpisodeDetail.tsx web/src/pages/Paper.tsx web/src/pages/Health.tsx && git commit -m "web: IBM Plex Mono body, responsive at 360 px, (?) help on every section"
```

---

### Task 12: web: the Papers page

**Files:**
- Create: `web/src/lib/cards.ts`, `web/src/lib/cards.test.ts`, `web/src/components/CardView.tsx`, `web/src/components/CardCharts.tsx`, `web/src/components/CardView.test.tsx`, `web/src/pages/Papers.tsx`, `web/src/pages/Papers.test.tsx`
- Modify: `web/src/api/types.ts`, `web/src/api/client.ts`, `web/src/test/fixtures.ts`, `web/src/App.tsx`, `web/src/App.test.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: `GET /api/cards?win=` and `GET /api/cards/meta` (Task 10; the body shape is Task 7's `CardBody`); `Box`, `Help`, `HelpId` (Task 11); `Tag`, `Loadable`, `EmptyState`, `LineChart`, `useApi`, `useNow`, `useTicker`, `formatMoney`, `formatDecimal`, `isoDay`, `DAY_MS` (existing).
- Produces:
  - types `CardStatus`, `CardRow`, `CardSeries`, `DecayPoint`, `CardBody`, `CardsResponse`, `PaperEntry`, `CardsMeta` in `web/src/api/types.ts`
  - `api.cards(win: string, signal?) → CardsResponse`, `api.cardsMeta(signal?) → CardsMeta`
  - `web/src/lib/cards.ts`: `STALE_MS`, `UNDER_REVIEW`, `ON_TABLE`, `statusLabel(s)`, `statusTone(s)`, `shortHash(h)`, `warmingText(card)`, `fmtValue(v, unit)`, `updatedText(lastTickMs, nowMs) → { text, stale }`
  - `CardView({ card, stale? })`, `CardChart({ card })` (bars, survival curves, histogram; Task 13 adds the decay chart)
  - `Papers` page at `/papers`, between PAPER and HEALTH in the nav; exports `WINS`, `NO_CARDS`, `PAPERS_SUBTITLE`
  - fixtures `cardFixture(o)`, `metaFixture(o)`

- [ ] **Step 1: Types, client and fixtures.** Append to `web/src/api/types.ts`:

```ts
export type CardStatus = 'live' | 'warming' | 'thin' | 'under_review';
export interface CardRow {
  label: string;
  /** count | × | s | % | bps | USD | ms */
  unit: string;
  paper: Dec | null;
  their: Dec | null;
  ours: Dec | null;
}
export interface DecayPoint {
  scenario: string;
  latency_ms: number;
  net_ms: number;
  captured: Dec | null;
  rate_pct: Dec | null;
  headline: boolean;
}
export interface CardSeries {
  bars?: { label: string; value: number }[];
  curves?: { label: string; points: { t_ms: number; s: Dec }[]; median_ms: Dec | null }[];
  bins?: { side: string; lo_bps: number; hi_bps: number | null; net: number; gross: number }[];
  decay?: DecayPoint[];
  measured?: { latency_ms: number; host: string };
}
export interface CardBody {
  card: string;
  paper: string;
  title: string;
  win: string;
  status: CardStatus;
  premise_key: string;
  hash: string;
  data_from_ms: number | null;
  data_to_ms: number | null;
  since_ms: number | null;
  n: number;
  columns: CardRow[];
  series: CardSeries;
  notes: string[];
  thin_reasons: string[];
  episode_link: string | null;
  computed_ms: number;
}
export interface CardsResponse {
  win: string;
  cards: CardBody[];
}
export interface PaperEntry {
  id: string;
  cite: string;
  status: 'live' | 'coming_later' | 'dropped';
  note: string | null;
  cards: string[];
}
export interface CardsMeta {
  tick: { last_tick_ms: number; analyze_version: string; tick_ms: number } | null;
  last_error: { ms: number; text: string } | null;
  catch_up: Record<string, { key: string; next_hour: number; done: boolean }>;
  papers: PaperEntry[];
  not_replicated: { item: string; why: string }[];
}
```

In `web/src/api/client.ts`, add `CardsMeta, CardsResponse,` to the `import type { … } from './types';` list, and add to the `api` object:

```ts
  cards: (win: string, signal?: AbortSignal) => getJson<CardsResponse>(`/api/cards${qs({ win })}`, signal),
  cardsMeta: (signal?: AbortSignal) => getJson<CardsMeta>('/api/cards/meta', signal),
```

Append to `web/src/test/fixtures.ts` (and add `CardBody, CardsMeta,` to its type import):

```ts
export const CARD_NOW = 1_790_000_000_000;

export function cardFixture(o: Partial<CardBody> = {}): CardBody {
  return {
    card: 'G1', paper: 'gebele-2026', title: 'G1 · Side asymmetry', win: '7d', status: 'live',
    premise_key: 'detector_no=1+e3b0c442;detector_yes=1+e3b0c442;episode_def=1+9d1c1e3a;fees=1+e3b0c442',
    hash: '3f2a9c1b7d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4',
    data_from_ms: CARD_NOW - 7 * 86_400_000, data_to_ms: CARD_NOW, since_ms: CARD_NOW - 10 * 86_400_000, n: 1240,
    columns: [
      { label: 'YES-side episodes', unit: 'count', paper: '2098', their: '1200', ours: '1150' },
      { label: 'NO-side episodes', unit: 'count', paper: '36', their: '40', ours: '38' },
      { label: 'YES : NO', unit: '×', paper: '58.28', their: '30', ours: '30.26' },
    ],
    series: { bars: [{ label: 'YES', value: 1150 }, { label: 'NO', value: 38 }] },
    notes: ['PAPER: CLOB sample, hour-stratified, 14 Apr 2026 00:00 UTC to 19 May 2026 06:00 UTC.'],
    thin_reasons: [], episode_link: null, computed_ms: CARD_NOW,
    ...o,
  };
}

export function metaFixture(o: Partial<CardsMeta> = {}): CardsMeta {
  return {
    tick: { last_tick_ms: CARD_NOW - 5 * 60_000, analyze_version: 'abc1234', tick_ms: 42 },
    last_error: null,
    catch_up: { g1: { key: 'k', next_hour: CARD_NOW, done: true } },
    papers: [
      { id: 'gebele-2026', cite: 'Gebele, Mutzel, Matthes (2026). Executable Arbitrage and Market Efficiency in Prediction Markets. arXiv 2608.00666 v1.', status: 'live', note: null, cards: ['G1', 'G2', 'G3', 'G4'] },
      { id: 'saguillo', cite: 'Saguillo et al.', status: 'coming_later', note: 'coming later: needs the on-chain dataset and somewhere to put it that is not your laptop', cards: [] },
      { id: 'cheng', cite: 'Cheng et al.', status: 'dropped', note: "dropped: the owner's call, not a data problem", cards: [] },
    ],
    not_replicated: [
      { item: 'Converter profits', why: "They come from on-chain conversions, which we don't observe." },
      { item: 'Same-condition findings of the WebSocket validation appendix (20 inconsistencies in 308,416,666 messages)', why: 'They need the on-chain side too.' },
    ],
    ...o,
  };
}
```

- [ ] **Step 2: Write the failing tests.** Create `web/src/lib/cards.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest';
import { cardFixture, CARD_NOW } from '../test/fixtures';
import { STALE_MS, fmtValue, shortHash, statusLabel, statusTone, updatedText, warmingText } from './cards';
import { isoDay } from './time';

describe('card helpers', () => {
  it('labels statuses in words and tones them', () => {
    expect(['live', 'warming', 'thin', 'under_review'].map((s) => statusLabel(s as never))).toEqual(['LIVE', 'WARMING', 'THIN', 'UNDER REVIEW']);
    expect(statusTone('live')).toBe('no');
    expect(statusTone('under_review')).toBe('flag');
    expect(statusTone('thin')).toBe('plain');
  });

  it('says since when and how many days of data', () => {
    const since = CARD_NOW - 6 * 86_400_000 - 5_000;
    expect(warmingText(cardFixture({ since_ms: since }))).toBe(`since ${isoDay(since)} · 6 days of data`);
    expect(warmingText(cardFixture({ since_ms: CARD_NOW - 86_400_000 }))).toContain('· 1 day of data');
    expect(warmingText(cardFixture({ since_ms: null }))).toBe('no run has recorded these premises yet');
  });

  it('formats values by unit with string operations only, and null as —', () => {
    const spies = [
      vi.spyOn(Number.prototype, 'toFixed'),
      vi.spyOn(Number.prototype, 'toLocaleString'),
      vi.spyOn(Intl, 'NumberFormat'),
      vi.spyOn(globalThis, 'parseFloat'),
      vi.spyOn(Math, 'round'),
    ];
    expect(fmtValue('12345678901234567.89', 'USD')).toBe('$12,345,678,901,234,567.89');
    expect(fmtValue('58.7', '%')).toBe('58.7%');
    expect(fmtValue('16.15', 's')).toBe('16.15 s');
    expect(fmtValue('52.5', 'bps')).toBe('52.5 bp');
    expect(fmtValue('200', 'ms')).toBe('200 ms');
    expect(fmtValue('30.26', '×')).toBe('30.26×');
    expect(fmtValue('1150', 'count')).toBe('1,150');
    expect(fmtValue('0', 'count')).toBe('0');
    expect(fmtValue(null, 'USD')).toBe('—');
    for (const s of spies) expect(s).not.toHaveBeenCalled();
    expect(shortHash('3f2a9c1b7d4e5f60')).toBe('3f2a9c1b7d4e');
  });

  it('says when the cards were updated and when that is too long ago', () => {
    expect(updatedText(null, CARD_NOW)).toEqual({ text: 'analyze has not run yet', stale: true });
    expect(updatedText(CARD_NOW - 5 * 60_000, CARD_NOW)).toEqual({ text: 'updated 5 min ago', stale: false });
    expect(updatedText(CARD_NOW - STALE_MS - 60_000, CARD_NOW)).toEqual({ text: 'updated 46 min ago', stale: true });
  });
});
```

Create `web/src/components/CardView.test.tsx`:

```tsx
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { UNDER_REVIEW } from '../lib/cards';
import { cardFixture, CARD_NOW } from '../test/fixtures';
import type { CardBody } from '../api/types';
import { CardView } from './CardView';

const renderCard = (card: CardBody, stale = false) =>
  render(
    <MemoryRouter>
      <CardView card={card} stale={stale} />
    </MemoryRouter>,
  );

describe('CardView', () => {
  it('renders a live card: three columns, n, the short hash with the full one on hover, a (?)', () => {
    renderCard(cardFixture());
    const card = screen.getByRole('article', { name: 'G1 · Side asymmetry' });
    expect(within(card).getByText('LIVE')).toBeInTheDocument();
    for (const h of ['PAPER', 'THEIR METHOD', 'OUR METHOD']) expect(within(card).getByText(h)).toBeInTheDocument();
    expect(within(card).getByText('2,098')).toBeInTheDocument();
    expect(within(card).getByText('30.26×')).toBeInTheDocument();
    expect(within(card).getByText('#3f2a9c1b7d4e')).toHaveAttribute('title', cardFixture().hash);
    expect(within(card).getByText(/n = 1240/)).toBeInTheDocument();
    expect(within(card).getByRole('button', { name: 'Help: What is side asymmetry?' })).toBeInTheDocument();
    expect(within(card).getByRole('img', { name: 'YES 1150, NO 38' })).toBeInTheDocument();
  });

  it('renders warming with since and days of data', () => {
    renderCard(cardFixture({ status: 'warming', since_ms: CARD_NOW - 3 * 86_400_000 }));
    expect(screen.getByText('WARMING')).toBeInTheDocument();
    expect(screen.getByText(/· 3 days of data$/)).toBeInTheDocument();
  });

  it('renders thin: numbers dimmed, the reason stated, null as — and "0" as 0', () => {
    renderCard(
      cardFixture({
        status: 'thin',
        columns: [
          { label: 'NO-side episodes', unit: 'count', paper: '36', their: '0', ours: '0' },
          { label: 'YES : NO', unit: '×', paper: '58.28', their: null, ours: null },
        ],
        thin_reasons: ['NO side: n = 0 < 10, so the ratio is not reliable'],
      }),
    );
    expect(screen.getByText('THIN')).toBeInTheDocument();
    expect(screen.getByText('thin: NO side: n = 0 < 10, so the ratio is not reliable')).toBeInTheDocument();
    expect(screen.getAllByText('—')).toHaveLength(2);
    expect(screen.getAllByText('0')).toHaveLength(2);
    expect(screen.getByText('NO-side episodes').closest('tr')).toHaveClass('dim');
  });

  it('renders under review', () => {
    renderCard(cardFixture({ status: 'under_review' }));
    expect(screen.getByText('UNDER REVIEW')).toBeInTheDocument();
    expect(screen.getByText(UNDER_REVIEW)).toBeInTheDocument();
  });

  it('hides empty method columns and links the biggest opportunity', () => {
    renderCard(
      cardFixture({
        card: 'EDGE', title: 'Edge capture',
        columns: [{ label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '12.5' }],
        series: {}, episode_link: 'live-9-00000042',
      }),
    );
    expect(screen.queryByText('PAPER')).toBeNull();
    expect(screen.getByText('VALUE')).toBeInTheDocument();
    expect(screen.getByText('$12.50')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'biggest opportunity →' })).toHaveAttribute('href', '/episodes/live-9-00000042');
  });

  it('dims every number when the results are stale', () => {
    renderCard(cardFixture(), true);
    expect(screen.getByText('YES-side episodes').closest('tr')).toHaveClass('dim');
  });
});
```

Create `web/src/pages/Papers.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { describe, expect, it } from 'vitest';
import { cardFixture, CARD_NOW, metaFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { NO_CARDS, PAPERS_SUBTITLE, Papers } from './Papers';

function Loc() {
  return <output data-testid="loc">{useLocation().search}</output>;
}

const renderAt = (url: string) =>
  render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/papers" element={<><Papers /><Loc /></>} />
      </Routes>
    </MemoryRouter>,
  );

const four = ['G1', 'G2', 'G3', 'G4'].map((c) => cardFixture({ card: c, title: `${c} · x` }));

describe('Papers', () => {
  it('asks for 7D by default and shows the Gebele cards, Saguillo later, Cheng dropped, and what is not replicated', async () => {
    const fetch = mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: four }),
      '/api/cards/meta': metaFixture(),
    });
    const real = Date.now;
    Date.now = () => CARD_NOW; // metaFixture's tick is 5 minutes before CARD_NOW
    try {
    renderAt('/papers');
    expect(await screen.findByRole('article', { name: 'G4 · x' })).toBeInTheDocument();
    expect(screen.getAllByRole('article')).toHaveLength(4);
    expect(String(fetch.mock.calls.find((c) => String(c[0]).startsWith('/api/cards?'))?.[0])).toBe('/api/cards?win=7d');
    expect(screen.getByText(PAPERS_SUBTITLE)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '7D' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('coming later: needs the on-chain dataset and somewhere to put it that is not your laptop')).toBeInTheDocument();
    expect(screen.getByText('DROPPED')).toBeInTheDocument();
    expect(screen.getByText('NOT REPLICATED')).toBeInTheDocument();
    expect(screen.getByText(/308,416,666 messages/)).toBeInTheDocument();
    expect(screen.getByText('updated 5 min ago')).not.toHaveClass('stale');
    } finally {
      Date.now = real;
    }
  });

  it('switches the window in the URL and refetches', async () => {
    const fetch = mockApi({
      '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: four }),
      '/api/cards/meta': metaFixture(),
    });
    renderAt('/papers');
    await screen.findByRole('article', { name: 'G1 · x' });
    await userEvent.click(screen.getByRole('button', { name: '30D' }));
    expect(screen.getByTestId('loc')).toHaveTextContent('?win=30d');
    expect(fetch.mock.calls.some((c) => String(c[0]) === '/api/cards?win=30d')).toBe(true);
  });

  it('dims the updated line when the last tick is older than 45 minutes and shows a failed tick', async () => {
    mockApi({
      '/api/cards': { win: '7d', cards: four },
      '/api/cards/meta': metaFixture({
        tick: { last_tick_ms: CARD_NOW - 50 * 60_000, analyze_version: 'abc', tick_ms: 1 },
        last_error: { ms: CARD_NOW, text: 'database: statement timeout' },
      }),
    });
    const real = Date.now;
    Date.now = () => CARD_NOW;
    try {
      renderAt('/papers');
      expect(await screen.findByText('updated 50 min ago')).toHaveClass('stale');
      expect(screen.getByText('last tick failed: database: statement timeout')).toBeInTheDocument();
    } finally {
      Date.now = real;
    }
  });

  it('says so when a live paper has no cards yet', async () => {
    mockApi({ '/api/cards': { win: '7d', cards: [] }, '/api/cards/meta': metaFixture() });
    renderAt('/papers?win=all');
    expect(await screen.findByText(NO_CARDS)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'ALL' })).toHaveAttribute('aria-pressed', 'true');
  });
});
```

In `web/src/App.test.tsx`, change the nav loop to `for (const name of ['RADAR', 'EPISODES', 'PAPER', 'PAPERS', 'HEALTH']) {`.

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npx vitest run src/lib/cards.test.ts src/components/CardView.test.tsx src/pages/Papers.test.tsx src/App.test.tsx`
Expected: FAIL: `Failed to resolve import "./cards"`, `"./CardView"`, `"./Papers"`; App: `Unable to find role="link" and name "PAPERS"`.

- [ ] **Step 4: The helpers.** Create `web/src/lib/cards.ts`:

```ts
import type { CardBody, CardStatus } from '../api/types';
import type { TagTone } from '../components/Tag';
import { formatDecimal, formatMoney } from './money';
import { DAY_MS, isoDay } from './time';

/** Spec §7: results older than this show "updated n min ago" in the thin (dimmed) style. */
export const STALE_MS = 45 * 60_000;
export const UNDER_REVIEW = 'The premises changed. The history is being recomputed; the numbers come back when it is done.';
/** The Edge capture row the Radar teaser quotes (cards::edge::ON_TABLE). */
export const ON_TABLE = 'edge on the table';

const LABELS: Record<CardStatus, string> = { live: 'LIVE', warming: 'WARMING', thin: 'THIN', under_review: 'UNDER REVIEW' };

export function statusLabel(s: CardStatus): string {
  return LABELS[s];
}

export function statusTone(s: CardStatus): TagTone {
  if (s === 'live') return 'no';
  if (s === 'under_review') return 'flag';
  return 'plain';
}

export function shortHash(h: string): string {
  return h.slice(0, 12);
}

/** "since 2026-09-20 · 6 days of data" (integer ms math; not money). */
export function warmingText(c: CardBody): string {
  if (c.since_ms === null) return 'no run has recorded these premises yet';
  const days = Math.floor((c.computed_ms - c.since_ms) / DAY_MS);
  return `since ${isoDay(c.since_ms)} · ${days} ${days === 1 ? 'day' : 'days'} of data`;
}

/** A card value as sent, by unit; money through formatMoney. null (not measured) is "—". */
export function fmtValue(v: string | null, unit: string): string {
  if (v === null) return '—';
  switch (unit) {
    case 'USD':
      return formatMoney(v);
    case '%':
      return `${formatDecimal(v)}%`;
    case 's':
      return `${formatDecimal(v)} s`;
    case 'bps':
      return `${formatDecimal(v)} bp`;
    case 'ms':
      return `${formatDecimal(v)} ms`;
    case '×':
      return `${formatDecimal(v)}×`;
    default:
      return formatDecimal(v);
  }
}

export function updatedText(lastTickMs: number | null, nowMs: number): { text: string; stale: boolean } {
  if (lastTickMs === null) return { text: 'analyze has not run yet', stale: true };
  const age = Math.max(0, nowMs - lastTickMs);
  return { text: `updated ${Math.floor(age / 60_000)} min ago`, stale: age > STALE_MS };
}
```

- [ ] **Step 5: The card and its charts.** Create `web/src/components/CardCharts.tsx`:

```tsx
import type { CardBody, CardSeries } from '../api/types';
import { LineChart, type Series } from './LineChart';

/** Two pixel bars (G1). Counts, not money. */
export function SideBars({ bars }: { bars: NonNullable<CardSeries['bars']> }) {
  const max = Math.max(1, ...bars.map((b) => b.value));
  const W = 100;
  return (
    <svg className="bars" viewBox={`0 0 ${W} ${bars.length * 12}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={bars.map((b) => `${b.label} ${b.value}`).join(', ')}>
      {bars.map((b, i) => (
        <rect key={b.label} className={`bar b${i}`} x={0} y={i * 12 + 1} width={b.value <= 0 ? 0 : Math.max(1, Math.round((b.value / max) * W))} height={10} />
      ))}
    </svg>
  );
}

const TONES = ['p0', 'p1', 'p2', 'p3'];

/** Survival step curves on the τ grid (G2): x is the grid index (0.1 s … 24 h, log-like), y is Ŝ. */
export function Survival({ curves }: { curves: NonNullable<CardSeries['curves']> }) {
  const n = Math.max(1, ...curves.map((c) => c.points.length));
  const series: Series[] = curves.map((c, i) => ({
    label: c.label,
    tone: TONES[i % TONES.length],
    // geometry only: survival probabilities are never shown as text here
    points: c.points.filter((p) => p.s !== '').map((p, j) => ({ x: j, y: Number(p.s) })),
  }));
  return <LineChart label="survival by duration, both sides, both methods" xMin={0} xMax={n - 1} series={series} yLabels={{ bottom: '0', top: '1' }} />;
}

/** 25 bp bins per side (G4): net filled, gross behind it. */
export function Histogram({ bins }: { bins: NonNullable<CardSeries['bins']> }) {
  const sides = [...new Set(bins.map((b) => b.side))];
  return (
    <div className="hist">
      {sides.map((side) => {
        const bs = bins.filter((b) => b.side === side);
        const max = Math.max(1, ...bs.map((b) => Math.max(b.net, b.gross)));
        const h = (v: number) => (v <= 0 ? 0 : Math.max(1, Math.round((v / max) * 24)));
        return (
          <figure key={side}>
            <svg className="histo" viewBox={`0 0 ${bs.length * 4} 24`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={`${side} net edge, 25 bp bins`}>
              {bs.map((b, i) => (
                <g key={b.lo_bps}>
                  <rect className="gross" x={i * 4} y={24 - h(b.gross)} width={4} height={h(b.gross)} />
                  <rect className="net" x={i * 4 + 1} y={24 - h(b.net)} width={2} height={h(b.net)} />
                </g>
              ))}
            </svg>
            <figcaption className="dim">{side} · 0 to 500+ bp</figcaption>
          </figure>
        );
      })}
    </div>
  );
}

export function CardChart({ card }: { card: CardBody }) {
  const s = card.series;
  if (s.bars && s.bars.length > 0) return <SideBars bars={s.bars} />;
  if (s.curves && s.curves.length > 0) return <Survival curves={s.curves} />;
  if (s.bins && s.bins.length > 0) return <Histogram bins={s.bins} />;
  return null;
}
```

Create `web/src/components/CardView.tsx`:

```tsx
import { Link } from 'react-router';
import type { CardBody, CardRow } from '../api/types';
import type { HelpId } from '../help';
import { UNDER_REVIEW, fmtValue, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { CardChart } from './CardCharts';
import { Help } from './Help';
import { Tag } from './Tag';

// EDGE sits inside the Edge capture box, whose own (?) explains the edge: its card (?) explains the status.
const CARD_HELP: Record<string, HelpId> = { G1: 'g1', G2: 'g2', G3: 'g3', G4: 'g4', EDGE: 'status' };
type Col = 'paper' | 'their' | 'ours';
const COLS: { key: Col; label: string }[] = [
  { key: 'paper', label: 'PAPER' },
  { key: 'their', label: 'THEIR METHOD' },
  { key: 'ours', label: 'OUR METHOD' },
];

/** One card (spec §5, §8.2): status in words, the three columns, n, notes and the short hash. */
export function CardView({ card, stale = false }: { card: CardBody; stale?: boolean }) {
  const dim = card.status === 'thin' || stale;
  const used = COLS.filter((c) => card.columns.some((r: CardRow) => r[c.key] !== null));
  const cols = used.length > 0 ? used : COLS.slice(2);
  return (
    <article className="cardv" aria-label={card.title}>
      <div className="box-head">
        <h3>{card.title}</h3>
        <Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>
        <Help id={CARD_HELP[card.card] ?? 'status'} />
      </div>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <CardChart card={card} />
      <div className="scroll">
        <table>
          <thead>
            <tr>
              <th scope="col" aria-label="measure" />
              {cols.map((c) => (
                <th key={c.key} scope="col">
                  {cols.length === 1 ? 'VALUE' : c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {card.columns.map((r) => (
              <tr key={r.label} className={dim ? 'dim' : undefined}>
                <td className="rowlabel">{r.label}</td>
                {cols.map((c) => (
                  <td key={c.key}>{fmtValue(r[c.key], r.unit)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="meta">
        n = {card.n} · <span title={card.hash}>#{shortHash(card.hash)}</span>
      </p>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      {card.thin_reasons.map((t) => (
        <p key={t} className="dim">
          thin: {t}
        </p>
      ))}
      {card.notes.map((n) => (
        <p key={n} className="note">
          {n}
        </p>
      ))}
    </article>
  );
}
```

- [ ] **Step 6: The page and the nav.** Create `web/src/pages/Papers.tsx`:

```tsx
import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { CardsResponse, PaperEntry } from '../api/types';
import { type ApiState, useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { CardView } from '../components/CardView';
import { EmptyState } from '../components/EmptyState';
import { Loadable } from '../components/Loadable';
import { Tag } from '../components/Tag';
import { updatedText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';

export const WINS = [
  ['24h', '24H'],
  ['7d', '7D'],
  ['30d', '30D'],
  ['all', 'ALL'],
] as const;
type Win = (typeof WINS)[number][0];
export const NO_CARDS = 'No cards yet. analyze has not finished a tick under these premises.';
export const PAPERS_SUBTITLE = 'Their number, their method on our data, our method. Same market, different weeks.';

function isWin(v: string | null): v is Win {
  return WINS.some(([w]) => w === v);
}

function PaperSection({ paper, cards, stale }: { paper: PaperEntry; cards: ApiState<CardsResponse>; stale: boolean }) {
  const tag =
    paper.status === 'live' ? <Tag tone="no">LIVE</Tag> : paper.status === 'coming_later' ? <Tag>COMING LATER</Tag> : <Tag tone="flag">DROPPED</Tag>;
  return (
    <Box title={paper.cite} help={paper.status === 'live' ? 'gebele' : 'otherPapers'}>
      {tag}
      {paper.status !== 'live' ? (
        <p className="quip">{paper.note ?? ''}</p>
      ) : (
        <Loadable state={cards} what="the cards">
          {(d) => {
            const mine = d.cards.filter((c) => c.paper === paper.id);
            return mine.length === 0 ? (
              <EmptyState text={NO_CARDS} />
            ) : (
              <div className="cardgrid">
                {mine.map((c) => (
                  <CardView key={c.card} card={c} stale={stale} />
                ))}
              </div>
            );
          }}
        </Loadable>
      )}
    </Box>
  );
}

/** Spec §8.2: a window switch (7D by default, in the URL), one section per paper, Not replicated. */
export function Papers() {
  const [sp, setSp] = useSearchParams();
  const asked = sp.get('win');
  const win: Win = isWin(asked) ? asked : '7d';
  const slow = useTicker(60_000);
  const cards = useApi((s) => api.cards(win, s), [win], [slow]);
  const meta = useApi((s) => api.cardsMeta(s), [], [slow]);
  const now = useNow(30_000);
  const upd = updatedText(meta.data?.tick?.last_tick_ms ?? null, now);
  return (
    <>
      <Box title="PAPERS" help="papers">
        <p className="quip">{PAPERS_SUBTITLE}</p>
        <div className="seg" role="group" aria-label="window">
          {WINS.map(([w, label]) => (
            <button key={w} type="button" className={w === win ? 'on' : undefined} aria-pressed={w === win} onClick={() => setSp(new URLSearchParams({ win: w }))}>
              {label}
            </button>
          ))}
        </div>
        <p className={upd.stale ? 'dim stale' : 'dim'}>{upd.text}</p>
        {meta.data?.last_error && <p className="error">last tick failed: {meta.data.last_error.text}</p>}
      </Box>
      <Loadable state={meta} what="the papers">
        {(m) => (
          <>
            {m.papers.map((p) => (
              <PaperSection key={p.id} paper={p} cards={cards} stale={upd.stale} />
            ))}
            <Box title="NOT REPLICATED" help="notReplicated">
              <ul className="plain">
                {m.not_replicated.map((n) => (
                  <li key={n.item}>
                    <b>{n.item}.</b> {n.why}
                  </li>
                ))}
              </ul>
            </Box>
          </>
        )}
      </Loadable>
    </>
  );
}
```

In `web/src/App.tsx`: add `import { Papers } from './pages/Papers';`, insert `{ to: '/papers', label: 'PAPERS', end: false },` into `NAV` between the PAPER and HEALTH entries, and add `<Route path="/papers" element={<Papers />} />` after the `/paper` route.

Append to `web/src/theme.css` (before the `@media (prefers-reduced-motion …)` line):

```css
.cardgrid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 8px; }
.cardv { border: 2px solid var(--line); background: var(--chip); padding: 8px; min-width: 0; display: grid; gap: 6px; align-content: start; }
.cardv h3 { font-family: var(--px); font-size: 9px; line-height: 1.5; flex: 1; min-width: 0; }
.cardv .rowlabel { color: var(--dim); }
.cardv .meta, .cardv .note { font-size: 13px; color: var(--dim); }
svg.bars { display: block; width: 100%; height: 40px; }
.bar.b0 { fill: var(--teal); }
.bar.b1 { fill: var(--salmon); }
.hist { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 140px), 1fr)); gap: 8px; }
svg.histo { display: block; width: 100%; height: 60px; background: var(--bg); }
.histo .gross { fill: var(--line2); }
.histo .net { fill: var(--accent); }
.stale { color: var(--salmon); }
ul.plain { list-style: none; display: grid; gap: 6px; }
@media (max-width: 760px) { .cardgrid { grid-template-columns: minmax(0, 1fr); } }
```

- [ ] **Step 7: Run the whole web suite and the build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test && npm run build`
Expected: every test passes (4 in `cards.test.ts`, 6 in `CardView.test.tsx`, 4 in `Papers.test.tsx`, the App shell with PAPERS, and all earlier tests, including `theme.test.ts` with the new rules); the build succeeds.

- [ ] **Step 8: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/src/api/types.ts web/src/api/client.ts web/src/test/fixtures.ts web/src/lib/cards.ts web/src/lib/cards.test.ts web/src/components/CardView.tsx web/src/components/CardCharts.tsx web/src/components/CardView.test.tsx web/src/pages/Papers.tsx web/src/pages/Papers.test.tsx web/src/App.tsx web/src/App.test.tsx web/src/theme.css && git commit -m "web: Papers page (window switch, Gebele cards with status and hash, coming later, not replicated)"
```

---

### Task 13: web: the Edge capture box on Paper, and the Radar teaser

**Files:**
- Create: `web/src/components/DecayChart.tsx`, `web/src/components/EdgeCapture.tsx`, `web/src/components/EdgeCapture.test.tsx`
- Modify: `web/src/components/CardCharts.tsx` (decay branch), `web/src/lib/cards.ts` (`teaserLine`), `web/src/lib/cards.test.ts`, `web/src/test/fixtures.ts` (`edgeFixture`), `web/src/pages/Paper.tsx`, `web/src/pages/Paper.test.tsx`, `web/src/pages/Radar.tsx`, `web/src/pages/Radar.test.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: `api.cards`, `CardBody`, `DecayPoint`, `CardView`, `ON_TABLE` (Task 12); `Box`, `Help` (Task 11); the EDGE body of Task 8 (rows `edge on the table`, `captured at sp-idle (145 ms)`, `capture rate, sp-idle`, …; `series.decay`, `series.measured`).
- Produces: `DecayChart({ points, measured })`; `EdgeCapture()` with `EDGE_SUBTITLE = 'The difference is physics and other people.'`, `EDGE_EMPTY`; `teaserLine(card: CardBody) -> string`; fixture `edgeFixture(o)`.

- [ ] **Step 1: The fixture.** Append to `web/src/test/fixtures.ts`:

```ts
export function edgeFixture(o: Partial<CardBody> = {}): CardBody {
  return cardFixture({
    card: 'EDGE', paper: 'edge', title: 'Edge capture', win: 'today',
    columns: [
      { label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '12.5' },
      { label: 'NO side, cashable before settlement', unit: 'USD', paper: null, their: null, ours: '9.25' },
      { label: 'YES side, locked until resolution', unit: 'USD', paper: null, their: null, ours: '3.25' },
      { label: 'captured at sp-idle (145 ms)', unit: 'USD', paper: null, their: null, ours: '0.4' },
      { label: 'capture rate, sp-idle', unit: '%', paper: null, their: null, ours: '3.2' },
      { label: 'capital needed at once', unit: 'USD', paper: null, their: null, ours: '180' },
      { label: 'biggest single opportunity', unit: 'USD', paper: null, their: null, ours: '1.25' },
      { label: 'measured latency (estimate)', unit: 'ms', paper: null, their: null, ours: '200' },
    ],
    series: {
      decay: [
        { scenario: 'colo-idle', latency_ms: 52, net_ms: 1, captured: '3.1', rate_pct: '24.8', headline: false },
        { scenario: 'sp-idle', latency_ms: 300, net_ms: 145, captured: '0.4', rate_pct: '3.2', headline: true },
        { scenario: 'sp-burst', latency_ms: 450, net_ms: 145, captured: null, rate_pct: null, headline: false },
      ],
      measured: { latency_ms: 200, host: 'vps-kvm2' },
    },
    notes: ['upper bound: assumes first-in-line execution; CLOB queue ignored'],
    episode_link: 'live-9-00000042',
    ...o,
  });
}
```

- [ ] **Step 2: Write the failing tests.** Append to `web/src/lib/cards.test.ts` (and add `edgeFixture` to its fixtures import, `teaserLine` to its `./cards` import):

```ts
describe('teaserLine', () => {
  it('says the Radar line in the spec voice', () => {
    expect(teaserLine(edgeFixture())).toBe('Edge on the table today: +$12.50 · captured at 145 ms: 3.2%.');
  });

  it('shows — for the rate when nothing was on the table, and $0.00 unsigned', () => {
    const zero = edgeFixture({
      columns: [{ label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '0' }],
      series: { decay: [{ scenario: 'sp-idle', latency_ms: 300, net_ms: 145, captured: '0', rate_pct: null, headline: true }] },
    });
    expect(teaserLine(zero)).toBe('Edge on the table today: $0.00 · captured at 145 ms: —.');
  });
});
```

Create `web/src/components/EdgeCapture.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { edgeFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { EDGE_EMPTY, EDGE_SUBTITLE, EdgeCapture } from './EdgeCapture';

const renderBox = () =>
  render(
    <MemoryRouter>
      <EdgeCapture />
    </MemoryRouter>,
  );

describe('EdgeCapture', () => {
  it('shows the decay chart, the measured marker, the split, the capital and the subtitle for 24H', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [edgeFixture({ win: '24h' })] }) });
    renderBox();
    expect(await screen.findByRole('img', { name: 'capture rate by latency' })).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=24h');
    expect(screen.getByText(EDGE_SUBTITLE)).toBeInTheDocument();
    expect(EDGE_SUBTITLE).toBe('The difference is physics and other people.');
    expect(screen.getByText(/^measured: vps-kvm2/)).toHaveTextContent('measured: vps-kvm2 ≈ 200 ms (an estimate)');
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-burst · 450 ms → —')).toBeInTheDocument();
    expect(screen.getByText('$9.25')).toBeInTheDocument();
    expect(screen.getByText('$180.00')).toBeInTheDocument();
    expect(screen.getByText('upper bound: assumes first-in-line execution; CLOB queue ignored')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Help: Why is the NO side cashable and the YES side locked?' })).toBeInTheDocument();
  });

  it('switches windows and says so when there is nothing yet', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [] }) });
    renderBox();
    expect(await screen.findByText(EDGE_EMPTY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: '7D' }));
    expect(fetch.mock.calls.some((c) => String(c[0]) === '/api/cards?win=7d')).toBe(true);
    expect(screen.getByRole('button', { name: '7D' })).toHaveAttribute('aria-pressed', 'true');
  });
});
```

Append to `web/src/pages/Radar.test.tsx` (and add `edgeFixture` to its fixtures import):

```tsx
  it('adds the Edge capture teaser to the today box when analyze has a today result', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture() },
      '/api/episodes/open': { now_ms: 0, items: [] },
      '/api/today': todayFixture(3, 1),
      '/api/cards': { win: 'today', cards: [edgeFixture()] },
    });
    renderRadar();
    expect(await screen.findByText('Edge on the table today: +$12.50 · captured at 145 ms: 3.2%.')).toBeInTheDocument();
  });
```

Append inside `describe('Paper', …)` in `web/src/pages/Paper.test.tsx` (and add `edgeFixture` to its fixtures import):

```tsx
  it('puts the Edge capture box at the top of the page', async () => {
    mockApi({
      '/api/paper/accounts': { items: [] },
      '/api/paper/positions': { items: [] },
      '/api/cards': { win: '24h', cards: [edgeFixture({ win: '24h' })] },
    });
    renderAt('/paper');
    await screen.findByText(PAPER_EMPTY);
    const headings = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
    expect(headings[0]).toBe('EDGE CAPTURE');
    expect(await screen.findByRole('img', { name: 'capture rate by latency' })).toBeInTheDocument();
  });
```

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npx vitest run src/lib/cards.test.ts src/components/EdgeCapture.test.tsx src/pages/Radar.test.tsx src/pages/Paper.test.tsx`
Expected: FAIL: `teaserLine` is not exported, `Failed to resolve import "./EdgeCapture"`, and the Radar/Paper cases find no teaser / no EDGE CAPTURE heading.

- [ ] **Step 3: The teaser line.** Append to `web/src/lib/cards.ts`:

```ts
/** Spec §6.2: "Edge on the table today: +$X · captured at 145 ms: Y%." (string operations only). */
export function teaserLine(card: CardBody): string {
  const onTable = card.columns.find((r) => r.label === ON_TABLE)?.ours ?? null;
  const head = card.series.decay?.find((p) => p.headline);
  const rate = head?.rate_pct ?? null;
  return `Edge on the table today: ${formatMoney(onTable, { signed: true })} · captured at ${head ? head.net_ms : 145} ms: ${rate === null ? '—' : `${rate}%`}.`;
}
```

- [ ] **Step 4: The chart and the box.** Create `web/src/components/DecayChart.tsx`:

```tsx
import type { DecayPoint } from '../api/types';

/**
 * Edge decay (spec §6.2): latency on x, capture rate on y, one point per scenario, and the
 * measured marker. Rates are parsed for geometry only; every number shown as text is the string sent.
 */
export function DecayChart({ points, measured }: { points: DecayPoint[]; measured: { latency_ms: number; host: string } | null }) {
  const W = 240;
  const H = 80;
  const xMax = Math.max(1, ...points.map((p) => p.latency_ms), measured?.latency_ms ?? 0) * 1.1;
  const rates = points.map((p) => (p.rate_pct === null ? null : Number(p.rate_pct)));
  const yMax = Math.max(100, ...rates.filter((r): r is number => r !== null));
  const sx = (x: number) => 2 + Math.round((x / xMax) * (W - 4));
  const sy = (y: number) => H - 2 - Math.round((Math.max(0, y) / yMax) * (H - 4));
  return (
    <figure className="chart-wrap">
      <svg className="chart decay" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label="capture rate by latency">
        {measured && <rect className="marker" x={sx(measured.latency_ms)} y={0} width={1} height={H} />}
        {points.map((p, i) => {
          const r = rates[i];
          return r === null || r === undefined ? null : (
            <rect key={p.scenario} className={p.headline ? 'pt head' : 'pt'} x={sx(p.latency_ms) - 2} y={sy(r) - 2} width={5} height={5} />
          );
        })}
      </svg>
      <div className="chart-axis">
        <span>0 ms</span>
        <span>latency →</span>
      </div>
      <ul className="plain legend">
        {points.map((p) => (
          <li key={p.scenario}>{`${p.scenario} · ${p.latency_ms} ms → ${p.rate_pct === null ? '—' : `${p.rate_pct}%`}`}</li>
        ))}
      </ul>
      {measured && <p className="dim">{`measured: ${measured.host} ≈ ${measured.latency_ms} ms (an estimate)`}</p>}
    </figure>
  );
}
```

In `web/src/components/CardCharts.tsx`, add `import { DecayChart } from './DecayChart';` and, in `CardChart`, before `return null;`:

```tsx
  if (s.decay && s.decay.length > 0) return <DecayChart points={s.decay} measured={s.measured ?? null} />;
```

Create `web/src/components/EdgeCapture.tsx`:

```tsx
import { useState } from 'react';
import { api } from '../api/client';
import { useApi } from '../api/useApi';
import { useTicker } from '../lib/hooks';
import { Box } from './Box';
import { CardView } from './CardView';
import { EmptyState } from './EmptyState';
import { Loadable } from './Loadable';

export const EDGE_SUBTITLE = 'The difference is physics and other people.';
export const EDGE_EMPTY = 'No Edge capture yet. analyze has not finished a tick under these premises.';
const WINS = [
  ['24h', '24H'],
  ['7d', '7D'],
  ['30d', '30D'],
  ['all', 'ALL'],
] as const;
type Win = (typeof WINS)[number][0];

/** Spec §6.2: the Edge capture box at the top of the Paper page. 24H by default (the page's equity chart is 24 h). */
export function EdgeCapture() {
  const [win, setWin] = useState<Win>('24h');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  return (
    <Box title="EDGE CAPTURE" help="edge">
      <p className="quip">{EDGE_SUBTITLE}</p>
      <div className="seg" role="group" aria-label="edge capture window">
        {WINS.map(([w, label]) => (
          <button key={w} type="button" className={w === win ? 'on' : undefined} aria-pressed={w === win} onClick={() => setWin(w)}>
            {label}
          </button>
        ))}
      </div>
      <Loadable state={st} what="edge capture">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'EDGE');
          return c ? <CardView card={c} /> : <EmptyState text={EDGE_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}
```

- [ ] **Step 5: Place them.** In `web/src/pages/Paper.tsx`, add `import { EdgeCapture } from '../components/EdgeCapture';` and change `Paper`'s return to:

```tsx
  return (
    <>
      <EdgeCapture />
      <Loadable state={accounts} what="paper accounts">
        {(d) =>
          d.items.length === 0 ? (
            <Box title="PAPER" help="paper">
              <EmptyState text={PAPER_EMPTY} />
            </Box>
          ) : (
            <PaperBody accounts={d.items} positions={positions} />
          )
        }
      </Loadable>
    </>
  );
```

In `web/src/pages/Radar.tsx`, add `import { teaserLine } from '../lib/cards';`; after the `const today = useApi(…)` line add

```tsx
  const edgeToday = useApi((s) => api.cards('today', s), [], [slow]);
  const edge = edgeToday.data?.cards.find((c) => c.card === 'EDGE');
```

and inside the TODAY box's render function, after `<p className="quip">{todayQuip(d.gross, d.net)}</p>`, add `{edge && <p className="quip teaser">{teaserLine(edge)}</p>}` (the teaser stays hidden until analyze has a `today` result; it is a teaser, not a section).

Append to `web/src/theme.css` (before the `@media (prefers-reduced-motion …)` line):

```css
svg.decay { height: 120px; }
.decay .pt { fill: var(--teal); }
.decay .pt.head { fill: var(--salmon); }
.legend.plain li { font-size: 13px; }
.teaser { color: var(--teal); }
```

- [ ] **Step 6: Run the whole web suite and the build.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm test && npm run build`
Expected: every test passes (the teaser, the EdgeCapture cases, the Paper and Radar additions, and all earlier tests); the build succeeds.

- [ ] **Step 7: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add web/src/components/DecayChart.tsx web/src/components/EdgeCapture.tsx web/src/components/EdgeCapture.test.tsx web/src/components/CardCharts.tsx web/src/lib/cards.ts web/src/lib/cards.test.ts web/src/test/fixtures.ts web/src/pages/Paper.tsx web/src/pages/Paper.test.tsx web/src/pages/Radar.tsx web/src/pages/Radar.test.tsx web/src/theme.css && git commit -m "web: Edge capture box on Paper (decay chart, measured marker, split, capital) and the Radar teaser"
```

---

### Task 14: deploy: the `analyze` service, the image, `.env.example`, CI, and the runbook

**Files:**
- Modify: `compose.yml`, `docker/rust.Dockerfile`, `.env.example`
- Create: `docs/superpowers/runbooks/phase5a-cards-vps.md`
- Check (no change expected): `.github/workflows/ci.yml`
- Test: `docker compose --env-file .env.example … config -q` (local, no build, no daemon needed), the whole workspace and web suites, and the runbook's post-deploy checks (owner, on the VPS)

**Interfaces:**
- Consumes: the `analyze` binary (Task 9: env `DATABASE_URL`, `ANALYZE_EVERY_SECS`, `ANALYZE_STATEMENT_TIMEOUT_MS`, `ANALYZE_CATCHUP_PAUSE_MS`, `ANALYZE_POLICIES`; exits non-zero if its role can write outside the card tables or a wallet/exchange variable is set); `SCANNER_HOST` (Task 4); role `analyze_rw` (Task 2: NOLOGIN from migration 0006, LOGIN from `01-roles.sh` on fresh volumes).
- Produces: compose service `analyze` (profile `core`); image `cute-trading/rust:dev` with `/usr/local/bin/analyze`; `.env` keys `SCANNER_HOST`, `ANALYZE_RW_DB_PASSWORD`; the Phase 5a runbook.

- [ ] **Step 1: The compose service.** In `compose.yml`:
  - in `postgres.environment`, add `ANALYZE_RW_DB_PASSWORD: ${ANALYZE_RW_DB_PASSWORD:-}` (only read by `01-roles.sh` on an empty volume; empty is allowed);
  - in `scanner.environment`, add `SCANNER_HOST: ${SCANNER_HOST:-unknown}`;
  - after the `scanner` service, add:

```yaml
  # Phase 5a live cards (spec 2026-09-26-phase5a-cards-design.md §2, §7): no network egress, writes
  # only card_cells/card_results/card_meta as analyze_rw, 0.25 CPU and 128 MB like the api.
  analyze:
    image: cute-trading/rust:dev
    profiles: ["core"]
    entrypoint: ["/usr/local/bin/analyze"]
    environment:
      DATABASE_URL: postgres://analyze_rw:${ANALYZE_RW_DB_PASSWORD:?set ANALYZE_RW_DB_PASSWORD in .env}@postgres:5432/cute_trading
      ANALYZE_EVERY_SECS: "900"
    depends_on:
      postgres:
        condition: service_healthy
      migrate:
        condition: service_completed_successfully
    restart: on-failure:5
    stop_grace_period: 10s
    read_only: true
    security_opt: ["no-new-privileges:true"]
    cap_drop: ["ALL"]
    cpus: "0.25"
    mem_limit: 128m
    logging: *logging
    networks: [internal]
```

(`internal` only: analyze has no egress network, on top of having no network client.)

- [ ] **Step 2: The image builds and ships `analyze`.** In `docker/rust.Dockerfile`:
  - change the second line to `# Build context: repository root.  docker compose build migrate   (spec §8.1.1). One image, three binaries: scanner (entrypoint), api (compose.web.yml) and analyze (compose.yml), each selected by its entrypoint.`
  - change `RUN cargo chef cook --release --recipe-path recipe.json --package scanner --package api` to `RUN cargo chef cook --release --recipe-path recipe.json --package scanner --package api --package analyze`
  - change `RUN cargo build --release --package scanner --package api && mkdir -p /data && chown 65532:65532 /data` to `RUN cargo build --release --package scanner --package api --package analyze && mkdir -p /data && chown 65532:65532 /data`
  - after `COPY --from=builder /src/target/release/api /usr/local/bin/api` add `COPY --from=builder /src/target/release/analyze /usr/local/bin/analyze`

(Release builds happen only inside Docker, as before; `analyze` reads `/etc/cute-trading/policies.toml`, which the image already copies. `SCANNER_VERSION=${GIT_SHA}` also stamps analyze's version.)

- [ ] **Step 3: `.env.example` (placeholders only).** Append:

```
# Phase 5a live cards. See docs/superpowers/runbooks/phase5a-cards-vps.md.
# The scanner's host label: a premise of Edge capture (vps-kvm2 on the VPS, pc-dev on the PC).
SCANNER_HOST=pc-dev
# analyze's role (analyze_rw). Migration 0006 creates it NOLOGIN; the runbook sets this password once.
ANALYZE_RW_DB_PASSWORD=change-me-analyze-rw
```

- [ ] **Step 4: The PC `.env` gets the two keys without printing anything.** Compose interpolates every `:?` in the file, so the PC `.env` needs `ANALYZE_RW_DB_PASSWORD` before any `docker compose` command works there again:

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && (grep -q '^ANALYZE_RW_DB_PASSWORD=' .env || printf 'ANALYZE_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && (grep -q '^SCANNER_HOST=' .env || echo 'SCANNER_HOST=pc-dev' >> .env) && grep -c -E '^(ANALYZE_RW_DB_PASSWORD|SCANNER_HOST)=' .env`
Expected: `2` (a count, never a value). `.env` is git-ignored; nothing to commit here.

- [ ] **Step 5: Validate the compose files (placeholders only).**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && docker compose --env-file .env.example -f compose.yml --profile core config --services`
Expected: `config-ok`, then the service list including `analyze`, `migrate`, `postgres`, `scanner`.

- [ ] **Step 6: CI covers everything already.** Check, without editing: `grep -n -E "cargo (clippy|test) --workspace|npm test|config -q" .github/workflows/ci.yml` must show `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm test` and the compose `config -q` job. `bins/analyze`, `crates/cards` and `crates/arb-core/tests/premises_lock.rs` are workspace members' tests, so the rust job runs them (with `STORE_TESTS_REQUIRED=1`, the analyze Postgres tests can't silently skip); the compose job reads `.env.example`, which now has the new keys. The benchmark stays `#[ignore]` (wall-clock on shared runners is noise); Step 9 and the runbook record its numbers.

- [ ] **Step 7: Write the runbook.** Create `docs/superpowers/runbooks/phase5a-cards-vps.md`:

````markdown
# Phase 5a on the VPS: live cards, Edge capture, readability (one bundle after the soak)

What ships (spec §10): migration 0006; the Rust image with the scanner (SCANNER_HOST, REST round
trips, run premises), analyze and api; the web image (Papers page, Edge capture box, IBM Plex Mono,
(?) help); the compose changes (the `analyze` service in `core`, `SCANNER_HOST`). The image is built
from the branch head, so it also carries the capture housekeeping fix `cc51942`.

Rules: never print `.env`; never run `docker compose config` except with `--env-file .env.example`;
never restart Docker. Disk budget: 50 GB for the VPS project (`sh scripts/disk-budget.sh`); a year of
cells is a few hundred MB.

## 0. Before: approve the backfill list (on the VPS)

Migration 0006 backfills `run_premises` for live scanner runs from `run_id` 7 (soak run 3, image
`2c5b84d`) on. List exactly what it will take:
```bash
cd /home/isoca/projects/cute-trading
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT run_id, started_ms, version, mode, exit_reason FROM runs WHERE run_id >= 7 AND service = 'scanner' AND mode = 'live' ORDER BY run_id"
```
The owner approves this list (write it in Results). Runs 1–6, replay, safe, census and
breaker_reset runs are never backfilled; `sim` and `host` are not backfilled, so Edge capture
starts at this deploy.

## 1. VPS `.env` keys (on the VPS; values never printed)

```bash
cd /home/isoca/projects/cute-trading
cp .env .env.bak
grep -q '^SCANNER_HOST=' .env || echo 'SCANNER_HOST=vps-kvm2' >> .env
grep -q '^ANALYZE_RW_DB_PASSWORD=' .env || printf 'ANALYZE_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env
chmod 600 .env
grep -c -E '^(SCANNER_HOST|ANALYZE_RW_DB_PASSWORD)=' .env
```
Expect `2`.

## 2. Build, bundle and ship (on the PC, Git Bash)

Run the benchmark first and write its two totals in Results (debug build; spec §9):
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p cards --test bench_tick -- --ignored --nocapture 2>&1 | grep -E 'tick total|test result'
```
Both totals must be under 200 ms. Then build and bundle:
```bash
sh scripts/disk-budget.sh
docker compose build --build-arg GIT_SHA="$(git rev-parse --short HEAD)" migrate    # cute-trading/rust:dev (scanner + api + analyze)
docker compose -f compose.yml -f compose.web.yml --profile web build web             # cute-trading/web:dev
mkdir -p "$HOME/ct-5a" && cd "$HOME/ct-5a"
docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz
cp /c/Users/Isabelly/PycharmProjects/cute-trading/compose.yml /c/Users/Isabelly/PycharmProjects/cute-trading/compose.web.yml /c/Users/Isabelly/PycharmProjects/cute-trading/.env.example .
mkdir -p docker/postgres/init && cp /c/Users/Isabelly/PycharmProjects/cute-trading/docker/postgres/init/01-roles.sh docker/postgres/init/
tar -cf "$HOME/ct-5a.tar" image.tar.gz compose.yml compose.web.yml .env.example docker && cd "$HOME" && rm -rf "$HOME/ct-5a"
docker builder prune -f --keep-storage 5GB
sh /c/Users/Isabelly/PycharmProjects/cute-trading/scripts/disk-budget.sh
```
Ship (PowerShell, the key has a passphrase):
```powershell
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" $HOME\ct-5a.tar root@31.97.23.123:/root/
```

## 3. Load and migrate (on the VPS)

```bash
cd /home/isoca/projects/cute-trading
tar -xf /root/ct-5a.tar && rm /root/ct-5a.tar
gunzip -c image.tar.gz | docker load && rm image.tar.gz
docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
docker compose --profile core run --rm migrate
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version FROM _sqlx_migrations ORDER BY version DESC LIMIT 1"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(DISTINCT run_id) FROM run_premises"
```
Expect `config-ok`, the migrate container exiting 0, `6`, and the number of runs approved in §0.

## 4. Activate analyze_rw once (on the VPS; the password is read into a variable, never printed)

Migration 0006 created `analyze_rw` NOLOGIN (a migration can't carry a secret). Give it the
password from `.env`:
```bash
cd /home/isoca/projects/cute-trading
PW="$(grep -E '^ANALYZE_RW_DB_PASSWORD=' .env | cut -d= -f2-)"
test -n "$PW" && echo pw-read
printf '%s\n' "$PW" | docker compose exec -T postgres sh -c 'read -r PW && echo "ALTER ROLE analyze_rw LOGIN PASSWORD :'"'"'pw'"'"';" | psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading -v pw="$PW"'
unset PW
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'analyze_rw'"
```
Expect `pw-read`, then `t`. (The password travels on stdin into the container and becomes a psql
variable there; `:'pw'` quotes it as a literal.)

## 5. Start (on the VPS)

The scanner is recreated on the new image (a new run: premises at start, `host` after 10 minutes);
analyze starts and catches up; the dashboard containers pick up the new images.
```bash
cd /home/isoca/projects/cute-trading
docker compose --profile core up -d --no-build scanner analyze
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web
docker compose -f compose.yml -f compose.web.yml ps
docker compose logs --tail 20 analyze
```
Expect `analyze: every 900 s (writes card tables only, paper only)` and, within a minute, one
`analyze: tick … ms, … dirty hours, … catch-up hours, 21 results` line.

## 6. Post-deploy checks

On the VPS:
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT card, win, status FROM card_results ORDER BY 1, 2"
docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "SELECT key, value FROM card_meta WHERE key IN ('tick', 'last_error')"
# after the scanner's first 10 minutes:
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version FROM run_premises WHERE area = 'host' ORDER BY run_id DESC LIMIT 1"
# after the first catch-up (the analyze log shows 0 catch-up hours on a later tick):
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' | grep -E 'analyze|scanner'
```
Pass: G1–G4 for `24h`, `7d`, `30d`, `all` and EDGE for those plus `today` (EDGE rows appear once
the new scanner run has its `host` row); `last_error` is `null`; the host row reads
`1:vps-kvm2|<band>`; analyze at or under 25 % CPU (0.25 of one CPU) and under 128 MiB, the
scanner's CPU as before.

In the browser (signed in), at `https://radar.isoca.space`:
- Papers: G1–G4 with a status tag, three columns, n and a short hash (full on hover); Saguillo
  "coming later: …"; the Not replicated box; "updated n min ago".
- Paper: the Edge capture box at the top with the decay chart and "measured: vps-kvm2 ≈ … ms".
- Radar: "Edge on the table today: +$X · captured at 145 ms: Y%." in the today box (once today has data).
- Every section has a (?) that opens and closes by mouse and keyboard (Tab to it, Enter, Escape).
- 360 px: DevTools → device toolbar → 360 × 740. On each page (Radar, Episodes, one episode detail,
  Paper, Papers, Health) run `document.documentElement.scrollWidth` in the console: it must print
  `360` (no horizontal page scroll); wide tables scroll inside their own box.
- Console: no CSP violations (the fonts come from fonts.googleapis.com / fonts.gstatic.com).

## 7. Phase 5a exit (spec §1)

On the VPS, the Papers page shows G1–G4 and the Edge capture box shows live figures for 24h / 7d /
30d / all; every card carries a status and a hash; analyze stays inside its CPU cap (the §2
benchmark numbers and the §6 `docker stats`). Record it below. After the first 7 days, check the
thin thresholds (30 / 10) against the real 7-day volumes (spec §11) and note them.

## Rollback

```bash
docker compose --profile core stop analyze
```
The cards keep their last results (the page shows the age). Migration 0006 is additive: the api,
web and scanner keep working with it. To also roll the scanner back, re-tag the previous image and
`docker compose --profile core up -d --no-build scanner`.

## Results

(date, image sha, approved backfill run list, benchmark totals at 30 / 90 days, migrate output,
first tick line, `docker stats` for analyze, 360 px check per page, exit check)
````

- [ ] **Step 8: Run everything once more.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" && cargo test --workspace && cd web && npm test && npm run build`
Expected: no formatting diff, no clippy warning, every Rust test passes (none `SKIPPED`), every web test passes, the web build succeeds.

- [ ] **Step 9: Record the benchmark and check the disk.**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cards --test bench_tick -- --ignored --nocapture 2>&1 | grep -E 'tick total|test result' && du -sh target && sh scripts/disk-budget.sh`
Expected: `tick total at 30 days: N ms` and `… at 90 days: N ms` with both N < 200, `test result: ok. 1 passed`; run `cargo clean` if `target/` is over 8 GB. Put the two totals in the runbook's Results when the owner deploys.

- [ ] **Step 10: Commit.**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git add compose.yml docker/rust.Dockerfile .env.example docs/superpowers/runbooks/phase5a-cards-vps.md && git commit -m "deploy: analyze service (core, 0.25 CPU, 128 MB, read-only), image ships analyze, SCANNER_HOST, Phase 5a runbook"
```

- [ ] **Step 11 (owner, not the implementer): deploy.** Follow `docs/superpowers/runbooks/phase5a-cards-vps.md` §0–§7 and fill in its Results.

---

## Rulings recorded in this plan (owner: check)

1. **14 tasks, not 13.** The store work is split in two (Task 2: schema, roles, backfill; Task 3: analyze's reads and writes), because a reviewer can reject the role/backfill design while approving the queries. The pure `cards` crate is split into statistics (5), Gebele cells and windows (6), bodies (7) and Edge capture (8) so each has its own oracle. Premises (Task 1) come first because the migration's backfill literals and the scanner both name them.
2. **`updated_ms` is the database clock** (`clock_timestamp()` per row, in the store's upsert), not the scanner process clock: analyze's cursor then compares stamps from one clock. analyze re-reads 120 s before its cursor (commit order), and a changed episode dirties every hour from its opening to now, so a late close also clears the hours it no longer spans.
3. **Two extra indexes in 0006:** `episodes_closed (closed_server_ms)` for hour-overlap reads and `paper_marks_account_t (account_id, t_ms)` for "the last mark before the window". The one-time `UPDATE episodes SET updated_ms = opened_server_ms` runs with the NOTIFY trigger disabled.
4. **`host` is its own INSERT after the run's 10th health minute** (no UPDATE grant: `run_premises` stays append-only). Its version is `{code}:{SCANNER_HOST}|{band}`, bands `lt25` / `25-100` / `100-250` / `gt250` / `unmeasured`, from the lower median of `lag_p50 + rest_rtt_p50 / 2` (integer ms, halves floored) over those minutes that have both. A run that dies earlier never counts for Edge capture; analyze dirties a run's hours when it newly matches a key. `SCANNER_HOST` unset → `unknown`; an invalid label stops the scanner at start.
5. **REST round trips are the CLOB client's only** (`books`, `ok`; send → response headers; successful responses only). Gamma calls are not timed: they are a different host and not the execution venue. The existing `rest_p50_ms` / `rest_p99_ms` (the `/ok` pings) stay as they are.
6. **Backfill = the Gebele areas only** (`censoring`, `depth`, `detector_no`, `detector_yes`, `episode_def`, `fees`) for `run_id >= 7 AND service = 'scanner' AND mode = 'live'`. `sim` and `host` are not backfilled (those runs measured no REST round trip), so Edge capture starts at the deploy. Versions are computed in SQL from literal config sections (`sha256()` in Postgres), pinned to `config/scanner.toml` and `arb_core::premises` by `the_backfill_literals_in_0006_are_todays_premises`.
7. **Premise files and config sections:** `episode_def` and `censoring` lock `arb-core/src/episode.rs`; both detectors lock `detect.rs`; `fees` `fee.rs`; `depth` `fill.rs`; `sim` the sim crate's logic files; `host` none. `bins/scanner/src/engine.rs` is not locked (it changes for unrelated reasons weekly); the thresholds behind the flags enter through `episode_def`'s config section (`stale_lag_ms`, `suspect_window_ms`, `l1_suspect_window_ms`, `drift_settle_ms`); `censoring` hashes `gap_ms`; `sim` hashes the policies file's sha256. Refresh command: `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock` (it refuses to write an unbumped change).
8. **Cells are per family, not per card:** `card_cells.card` ∈ `g1`, `dur` (G2 and G3 share areas and data), `g4`, `edge`; `card_results.card` ∈ `G1`…`G4`, `EDGE`.
9. **Windows are incremental in memory** (spec §7 describes a direct merge of 24/168/720/all cells per tick; that cannot meet < 200 ms in debug at 250k episodes, because it re-parses every cell): each window keeps a sum, a rebuilt hour swaps old for new, advancing the clock subtracts leaving hours and adds entering ones; Edge's two maxima are kept per hour. The windows are rebuilt from `card_cells` at start and after any failed tick. Property tests pin incremental = direct sum, and hourly cells = direct computation.
10. **Kaplan–Meier uses an i128 fixed point** (1.0 = 10²⁸) instead of Decimal (speed in debug builds); it matches arb-core's Decimal KM to 12 places and the 6-MP table to 20.
11. **Unclosed episodes** span to their run's end (`ended_ms`, else the next scanner session's start for a crashed run) or now; they count in G1, G4 and Edge capture and as `open` (excluded) in G2/G3.
12. **What counts:** YES/NO baskets only; `Π_net > 0` for every Gebele and Edge figure (gross counts are context); OUR METHOD and Edge capture drop `engine_suspect`/`stale_transport` (counts stated); Edge needs `pnl_net_budget100_usd`; capped cost = min(peak cost, $100).
13. **G1 merge sensitivity** uses the previous close of another live episode of the same event and kind within 1 s, merged when gap ≤ 0 / 250 / 1000 ms (parent §5.5 says "≤"). **G1 thin** looks at OUR METHOD's per-side counts.
14. **G2** reports gap-censored episodes' median recorded span as an upper bound. **G3** THEIR METHOD counts both sides, flagged included; its bias uses uncensored episodes only (true durations). **G4**'s PAPER column quotes the positive counts 2,098 / 36 (the same population as G1; the spec gives no other numbers for Figure 2) marked "figure only"; THEIR METHOD is our raw positive counts; edges are rounded to 0.01 bp; bins of 25 bp to 500 bp, then open; nearest-rank median and p90; G4 thin below 30 per side.
15. **Edge capture:** captured = the sum of equity changes between consecutive marks of each scenario's `P0-converter` default-fee account, counted only when both marks come from runs of the current key; no marks → `null`; rate = captured ÷ total on the table (spec wording, although P0 trades the NO side only: a note says so), 1 decimal; decay x = proc + net + match from `policies.toml`; the teaser quotes `sp-idle`'s `net_ms` (145); measured latency is the nearest-rank median per window; Edge thin below 10 episodes.
16. **Status precedence** under_review > warming > thin > live; `all` is never warming; `today` starts at UTC midnight. A family with no run carrying its premises writes no result row (the page says "no cards yet").
17. **Hash inputs:** premise key, analyze version, the window's first and last hour, card, window, and a parameter string per card.
18. **analyze_rw's statement timeout** is set per database (`ALTER ROLE … IN DATABASE … SET`), because parallel test databases would otherwise update the same role-setting row; `01-roles.sh` also sets it role-wide on fresh volumes; the analyze pool sets it per session too.
19. **API:** `win` missing → `7d`; `today` is filtered to `EDGE` in the api as well; bodies pass through as stored; `/api/cards/meta` returns `tick`, `last_error`, `catch_up` (from the `fam:*` keys), `papers`, `not_replicated`, and hides internal keys (`cursor`).
20. **Web:** the Papers window is in the URL (7D default); the Edge capture box keeps its window in local state (24H default, like the page's equity chart); the Radar teaser stays hidden until a `today` result exists; `Box` requires a `help` id, so TypeScript enforces "every section"; the EDGE card's own (?) explains the status (the box's (?) explains the edge); the 360 px rule is pinned by a structural CSS test plus the runbook's manual `scrollWidth` check (jsdom has no layout).
21. **The benchmark is pure and `#[ignore]`d in CI** (wall-clock on shared runners is noise): it runs by hand in Tasks 9 and 14 and before the deploy, measuring the tick's compute (DB time excluded) at 30 and 90 days.
22. **analyze** gets its own copy of the env-guard denylist (kept identical by the arch test), no egress network in compose, and the env names `ANALYZE_EVERY_SECS`, `ANALYZE_STATEMENT_TIMEOUT_MS`, `ANALYZE_CATCHUP_PAUSE_MS`, `ANALYZE_POLICIES`.
23. **The papers list** also names Laya ("Phase 6", coming later) and Cheng (dropped); citations for Saguillo, Laya and Cheng are "et al." placeholders of the names only, since the spec gives no full references.
24. **The runbook is a new file** (`phase5a-cards-vps.md`), not an addition to the Phase 4 runbook, and it uses `docker compose --env-file .env.example … config -q` only. The PC `.env` gets `ANALYZE_RW_DB_PASSWORD` and `SCANNER_HOST=pc-dev` appended in Task 14 (never printed), because compose interpolates `:?` everywhere.

## Self-review

**1. Spec coverage** (`2026-09-26-phase5a-cards-design.md`):
- §1 scope and exit: card framework (Tasks 5–9), G1–G4 (Tasks 6–7), Edge capture (Task 8), measured marker and host premise (Tasks 4, 8), (?) help (Task 11), readability and responsive pass (Task 11); exit checks in the runbook §6–§7 (Task 14). Out of scope untouched (5b storage, Saguillo/Cheng/Laya cards).
- §2 architecture: `bins/analyze` in the Rust image as a `core` service with 0.25 CPU / 128 MB, read-only, non-root, no-new-privileges, cap_drop ALL, `restart: on-failure:5` (Tasks 9, 14); arch test bans reqwest, pm-api, tokio-tungstenite, sqlx (Task 9); api read-only with SELECT on `card_results`/`card_meta` (Tasks 2, 10); `analyze_rw` grants, 10 s timeout, startup refusal (Tasks 2, 3, 9).
- §3 data: `updated_ms` + index + upsert + fill (Task 2); `run_premises` (Tasks 2, 4); health RTT columns (Tasks 2, 4); `card_cells`, `card_results`, `card_meta` (Task 2); backfill of the approved list (Task 2, pinned in Task 4); disk budgets (Global Constraints, runbook).
- §4 premises: `PREMISES` table, effective versions with config sections, `premises.lock` with `\r` stripped and the refresh command (Task 1); runs record premises, `host` with band (Task 4); per-card keys, only matching runs count (Tasks 1, 9); statuses live/warming/thin/under_review (Tasks 7, 9); reproducibility hash (Task 7).
- §5 Gebele: episodes counted (live, yes/no baskets, flags excluded under OUR METHOD with counts) (Tasks 3, 6); G1 with merge sensitivity and gross context; G2 empirical Ŝ and KM with left/gap handling; G3 redefined, both methods; G4 bins, median, p90, figure-only counts (Tasks 6–7); Not replicated box (Tasks 7, 12).
- §6 Edge capture: on the table with the split and the upper-bound label, captured per scenario from `paper_marks`, capture rate with "—" on zero, capital overlap exact across hours, biggest opportunity linked, areas incl. `sim`/`host` (Task 8); Paper box with decay chart, measured marker, split, capital, subtitle (Task 13); Radar teaser (Task 13); measured latency and host premise (Tasks 4, 8); scenarios unchanged.
- §7 analyze job: tick every 15 min (60..=3600), dirty hours via the `updated_ms` cursor, partial hour, window merge (incrementally, ruling 9), today window, meta tick time; merge-exact cell maths with the property test; catch-up 24 h per batch with 2 s pauses and `under_review`; late closes; statement timeouts; one-line failures recorded in `card_meta`, results kept, stale page styling (Tasks 6, 9, 12).
- §8 API and web: both endpoints, 400 on a bad `win`, today = Edge only, one small query each (Task 10); Papers page with window switch, sections, two-column grid, one column on phones, Not replicated (Task 12); Edge box and teaser (Task 13); (?) behaviour and `help.ts` (Task 11); fonts via Google Fonts with no CSP change, no horizontal scroll at 360 px, wide tables scroll in their box, charts scale (Tasks 11–13).
- §9 testing: KM on a textbook dataset with ties and censoring (6-MP), empirical Ŝ, medians, bins, p90, long-lived share (Tasks 5, 7); Edge sums, split, capture rates incl. zero denominator, overlap across hours (Task 8); merge property (Tasks 6, 8); premise lock failure, new key never mixing, runs without premises never counted (Tasks 1, 9); roles, `updated_ms` advancing, 0006 idempotent (Task 2); analyze tick, late closes, catch-up batches and pauses (Task 9); benchmark (Task 9); API shape, 400, strings, null vs "0" (Task 10); web statuses, Edge/teaser copy, (?) by keyboard and announced, no float money, existing tests green (Tasks 11–13); CI covers analyze and the lock test (Task 14).
- §10 deploy: one bundle with migration, images, compose changes and `cc51942`; runbook: `.env` keys, the one-time `ALTER ROLE … LOGIN PASSWORD` reading the password into a variable, `01-roles.sh` for fresh volumes, backfill approval, `docker stats` after catch-up (Task 14).
- §11 open items: backfill list as approved (Task 2); KM oracle cited (Task 5); thin thresholds re-checked after 7 days (runbook §7).

**2. Placeholder scan:** no TBD/TODO/"similar to Task N". Three steps point at existing files instead of repeating them, each with the exact command or edit: `bins/analyze/tests/common/mod.rs` is `cp` of the store harness; `bins/analyze/src/guard.rs` is a copy of the api guard with one named comment change (the arch test proves the lists identical); `premises.lock` is written by the refresh command (its hashes can't be known before the files exist). The runbook's Results section is filled at deploy time by design.

**3. Type consistency:** `RunPremiseRow`, `PremiseRun`, `ChangedSpan`, `CardEpisode`, `LatencyMinute`, `MarkPoint`, `CardCellRow`, `CardResultRow` are defined in Tasks 2–3 and used with the same fields in Tasks 4 and 9. `Ep`, `HourInput`, `CellData` (`FAMILY`, `AREAS`, `EDGE_INPUTS`, `Max`, `build`, `add`, `sub`, `max_part`, `merge_max`, `to_json`, `from_json`), `Windows` (`needed_ranges`, `advance`, `apply`, `max_from`, `today`, `span_start`, `recent`), `Card::bodies`, `BodyCtx`, `ScenarioLat`, `Family`, `current_key`, `hours_between`, `contiguous` match between Tasks 6–9 and the bench. JSON field names of `CardBody` (Task 7) match `web/src/api/types.ts` (Task 12) field for field (`premise_key`, `data_from_ms`, `since_ms`, `thin_reasons`, `episode_link`, `series.bars|curves|bins|decay|measured`, `rate_pct`, `median_ms`); row labels the web quotes (`edge on the table`) come from `cards::edge::ON_TABLE`. card_meta keys (`tick`, `last_error`, `cursor`, `fam:*`, `papers`, `not_replicated`) match between Task 9 and Task 10. Help ids named in Task 11 are the ones Tasks 12–13 use.

**4. Review Focus:** each of the five lines has its named test in the owning task: 1 and 2 and 5 in Task 9 (`a_late_close_outside_the_recent_48h_rebuilds_the_old_hour_without_double_counting`, `a_premise_bump_mid_window_never_mixes_keys`, `tick_stays_under_200ms_as_all_grows_from_30_to_90_days`), 3 in Tasks 6 and 8 (`an_episode_spanning_three_hours_is_observed_in_each_hour`, `capital_overlap_counts_an_episode_opened_hours_earlier`), 4 in Tasks 7 and 8 (`thin_and_empty_samples_never_divide_by_zero`, `capture_rate_is_null_when_nothing_was_on_the_table`).
