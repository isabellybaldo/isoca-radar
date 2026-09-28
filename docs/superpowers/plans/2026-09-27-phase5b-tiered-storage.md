# isoca-radar Phase 5b: tiered storage (compactor, Parquet archive, delete guard) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep the scanner's data on the VPS: about 24 h of raw capture (tier 1), and every closed raw hour reduced by a new `compact` service into daily Parquet tables (`tokens`, `tob`, `depth`, `l2`, `gaps` + `manifest.json`) kept for months under a 55 GB budget (tier 2), with a scanner delete guard so raw is never deleted before it is reduced (except at the hard cap, recorded as lost), a dashboard heart, and `compact verify`.

**Architecture:** A new pure crate `crates/tape` becomes the one home of the raw capture format: capture lines (`Line`, `Ctl`), Polymarket frame parsing (moved out of `pm-api`, which re-exports it), the capture file layout, and `tape::feed::FeedBooks`, the scanner engine's book rules without detection or trading, pinned to the engine line by line by two agreement tests (books, and the `tob` rows built on them). A new binary `bins/compact` (0.25 CPU, 256 MB, role `compact_rw`, no network client) reads each closed raw hour through `tape`, turns it into rows (top-of-book changes, 5-minute depth, l2 around net-positive arbitrage episodes, gaps), writes hourly Parquet parts atomically, checkpoints its books, records `compact_hours` in Postgres, merges whole UTC days and enforces the budget. The scanner deletes a raw hour by age only once it is `done`/`lost`, deletes at the 15 GB cap whatever the status (marking it `lost`), writes a universe record only when it changed (and names only records the capture thread acknowledged), and falls back to today's rule when Postgres is unreachable or the compactor has been silent for 6 h (the rollback rule).

**Tech Stack:** Rust 1.98.1 (edition 2024), tokio, sqlx 0.8 (inside `store` only), rust_decimal 1.41+, serde_json, zstd 0.13, sha2, clap 4, `parquet` + `arrow` (Apache Arrow Rust, one exact pinned version, compact only); axum 0.8 (api); React 19, Vite 8, TypeScript 5.9, Vitest 4 + Testing Library; Postgres 17.

**Spec:** `docs/superpowers/specs/2026-09-27-phase5b-tiered-storage-design.md` (parent `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §7, §9.3, §10, §19; builds on Phase 5a `2026-09-26-phase5a-cards-design.md` and 5d `2026-09-27-phase5d-sweet-spots-design.md`). The spec is the authority; the rulings below settle its §11 open items and are recorded again in **Rulings** at the end.

This is ONE plan with 21 sequential tasks:
- Tasks 1–4: `crates/tape` (the shared capture format and frame parsing; the universe view and `server_top`; `FeedBooks`; the agreement test against the engine).
- Task 5: store (migration 0008, `compact_rw`, the compact reads/writes).
- Tasks 6–7: the scanner (the delete guard, 24 h / 15 GB and the rollback rule; universe records only on change, acknowledged).
- Tasks 8–18: `bins/compact` (skeleton and pins; config and rows; Parquet I/O; the reducer; the `tob` agreement against the engine; l2 windows; hour files; checkpoints; the daily merge and budget; `verify`; the service).
- Task 19: api and web (health fields, the heart, the Archive box).
- Task 20: Docker, compose, `.env.example`, roles script.
- Task 21: the deploy runbook.

Tasks 1–18 and 20 never touch `web/`; Task 19 touches `web/` and `bins/api` only.

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere. `compact` refuses to start if an exchange/wallet credential variable is set (the same denylist as `bins/scanner/src/guard.rs`, kept in step by `bins/scanner/tests/arch.rs`).
- **`compact` has no network client:** the arch test bans `reqwest`, `pm-api`, `tokio-tungstenite` and `sqlx` from `compact`; it reaches Postgres only through `store`. `crates/tape` stays pure: no `tokio`, `reqwest`, `tokio-tungstenite`, `sqlx`, `pm-api`, `store` (files and parsing only).
- **Only `store` uses sqlx** (`deny.toml`: `{ name = "sqlx", wrappers = ["store"] }`).
- **New crates (owner-approved 2026-09-27): `parquet` and `arrow` only**, pinned to one exact version (`=X.Y.Z`, found with `cargo search` in Task 8; `60.0.0` at planning time) in `[workspace.dependencies]`, used only by `bins/compact`: `deny.toml` gets `{ name = "parquet", wrappers = ["compact"] }` and `{ name = "arrow", wrappers = ["compact"] }`, and the arch test fails if any other workspace crate depends on them. No other new crates.io crate; new Rust dependencies come from `[workspace.dependencies]` only. **No new web dependency.**
- **Roles after migration 0008 (spec §9):** `compact_rw` (created NOLOGIN, activated by the runbook with `COMPACT_RW_DB_PASSWORD`): SELECT on `episodes`, `runs`; SELECT, INSERT, UPDATE on `compact_hours`, `compact_status` (SELECT is needed by `INSERT … ON CONFLICT DO UPDATE` and by the lost-hour read; ruling R7); statement timeout 10 s. `scanner_rw`: SELECT, INSERT on `compact_hours`, SELECT on `compact_status` (the rollback rule). `analyze_ro` (the api): SELECT on `compact_status`, `compact_hours`. `compact` refuses to start if its role can write any table other than `compact_hours`, `compact_status`. `analyze_rw`'s write set is unchanged.
- **Premise areas are untouched.** Do not edit any file listed in `crates/arb-core/src/premises.rs` `PREMISES` (`crates/arb-core/src/{episode,fill,detect,fee}.rs`, `crates/sim/src/*.rs`); `crates/arb-core/tests/premises_lock.rs` and `bins/scanner/tests/premises.rs` must stay green without a version bump. None of the new config values (`config/compact.toml`, `retention_hours`, `max_capture_gb`) is a premise input (spec §7); Task 6 runs both checks.
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (tests may unwrap: `clippy.toml`; integration test files start with `#![allow(clippy::unwrap_used, clippy::expect_used)]`). Prices and sizes in the archive are **micro-units** (`i64`, value × 10⁶, exact because Polymarket values have at most 6 decimals: `arb_core::money::MAX_DP`).
- **Time** is integer ms since epoch, days and hours in UTC: `tape::time::hour_of(ms) = ms.div_euclid(3_600_000) * 3_600_000`. `tob.ts_ms` is the book's server timestamp, `recv_ms` our receive time; rows are ordered by `recv_ms` inside an hour (ruling R4).
- **Configuration (spec §7), exact defaults:** `config/compact.toml`: `raw_dir = "/data/capture"`, `archive_dir = "/archive"`, `depth_every_secs = 300`, `depth_levels = 10`, `l2_before_secs = 300`, `l2_after_secs = 300`, `l2_max_window_secs = 1800`, `archive_budget_gb = 55`, `lag_alarm_hours = 3`. `config/scanner.toml`: `retention_hours = 24`, `max_capture_gb = 15`. Parquet: zstd level 3, row groups of **262,144 rows**, write batches of 65,536 rows (ruling R2).
- **Exact strings (spec):**
  - gap reasons: `down`, `raw_hour_lost`, `no_book_yet`, `scanner_stopped`;
  - hour log: `compact: <H> done in <ms> ms, tob=<n> depth=<n> l2=<n> gaps=<n>, <bytes> B` (`<H>` = `YYYY-MM-DDTHH`, `tape::time::hour_label`);
  - scanner log: `capture: hour <H> deleted before compaction (disk cap)`;
  - hearts: `compactor behind: raw at risk` (`compact_lag_hours > 3`) and `compactor status older than 2 h` (`compact_age_ms > 7_200_000`); one heart at most between the two.
- **Web look:** the existing theme; the new Archive box has a (?) whose copy lives in `web/src/help.ts`; no horizontal page scroll at 360 px; byte sizes use integer math.
- **No fresh `npm install` in `web/`** (npm 10.9.2 crashes): reuse the existing `web/node_modules`.
- **Debug builds only** on the dev machine (no `--release`, no `cargo install`). Docker builds happen only in Task 21's runbook, run by the owner (the release benchmark runs there, inside the image, with `--cpus 0.25 --memory 256m`).
- **Disk budgets:** 30 GB on the PC (`sh scripts/disk-budget.sh`), 75 GB for this project on the VPS (spec §1). Run `cargo clean` if `target/` passes 8 GB (it is ~15 GB at plan time: Task 1 Step 0).
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A` or `.`); never stage `PROJECT.md` or `README.md` (untracked, the owner's). Commit messages have **no** `Co-Authored-By` or any other attribution trailer.
- **Shell prefix for every command:** `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store, api, compact service) use the Postgres already listening on **127.0.0.1:15432** (not 55432). Prefix their commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" &&`. A test that prints `SKIPPED: TEST_DATABASE_URL not set` does **not** count as passing. Before the first such run in a task, check the port: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` must print `pg-up`; if it doesn't, stop and ask the owner.
- **Secrets and the machine:** never print `.env` (no `cat`, no `grep` that shows values, no `echo` of a password); never run `docker compose config` except as `docker compose --env-file .env.example … config -q`; never restart Docker or WSL.
- **Line endings:** `*.sql` and `config/*.toml` are LF (`.gitattributes`); the Docker build refuses CRLF in `crates/store/migrations` and `config` (5d lesson). New files in those folders are covered by the existing patterns.

## Review Focus

1. **A truncated raw hour** (the scanner was killed mid-write, then restarted and appended a new zstd frame after a broken one): compaction must not stall on it forever. The lines before the break become rows, the rest of the hour becomes a `raw_hour_lost` gap, every book is untrusted until the feed sends a new snapshot, and the hour is still recorded `done`. Pinned by `a_truncated_hour_keeps_its_rows_and_gaps_the_rest` (Task 14).
2. **An hour deleted at the hard cap around compaction:** the loss must stay visible whichever side acts first. `mark_hours_lost` never turns a `done` hour into `lost`; the scanner inserts its mark the moment it deletes; a compactor that saw the file and finds it gone records `lost` itself, and waits 5 minutes before calling a never-seen, unmarked hour `scanner_stopped`; passing a `lost` hour writes one `raw_hour_lost` gap and never carries a stale book across it. Pinned by `mark_hours_lost_never_downgrades_a_done_hour` (Task 5), `housekeeping_marks_hours_lost_at_the_cap` (Task 6), `a_lost_hour_becomes_a_raw_hour_lost_gap_and_untrusts_every_book` (Task 11), `a_file_deleted_before_its_lost_mark_is_compacted_as_lost` and `a_missing_hour_nobody_marked_is_scanner_stopped_and_done` (Task 18).
3. **Frames captured out of receive order across shards:** the capture file is in engine order, not `recv_ms` order. Rows must come out sorted by `recv_ms` inside the reorder window, a row later than the window is kept (appended and counted in `late_rows`), never dropped. Pinned by `reorder_sorts_within_the_window_and_counts_late_rows` (Task 14).
4. **A universe refresh that drops a group's connection, and an unchanged refresh naming an old record:** a reset book must produce no rows until the feed sends a new snapshot; a record capture never wrote must never be named; and retention must keep the record still in force for the oldest remaining hour even when it is older than that hour. Pinned by `a_group_losing_its_connection_stops_rows_until_a_new_snapshot` (Task 11), `a_record_capture_never_wrote_is_written_again_once_capture_resumes` and `retention_keeps_the_universe_record_in_force_at_the_oldest_hour` (Task 7).
5. **Midnight UTC:** the token dictionary restarts at 00:00, an l2 window spanning midnight restarts with snapshot rows in the new day so `verify` passes on both days, and the day merges only when all 24 hours are `done` or `lost`. Pinned by `a_window_across_midnight_restarts_with_snapshots_in_the_new_day` (Task 13) and `a_day_missing_an_hour_is_not_merged` (Task 16).

## File Structure

```
Cargo.toml                                        MODIFY: members += crates/tape, bins/compact; workspace deps parquet, arrow (exact)
deny.toml                                         MODIFY: parquet/arrow wrappers = ["compact"]
crates/tape/                                      NEW: pure crate
  src/lib.rs, src/time.rs                         civil/hour/day helpers (moved from scanner capture.rs)
  src/line.rs                                     Line, Ctl, book_ctl, ctl_book (moved from scanner capture.rs)
  src/frame.rs                                    MarketEvent, parse_frame, FrameError (moved from pm-api ws.rs)
  src/files.rs                                    capture layout, listing, line reading, zstd JSON helpers
  src/universe_view.rs                            RecordView (read-only projection of UniverseRecord), canary_key
  src/feed.rs                                     FeedBooks: the engine's book rules; FeedState (checkpoints)
  tests/{frame,files,server_top,feed}.rs
crates/pm-api/src/{ws,error}.rs, Cargo.toml       MODIFY: re-export tape's MarketEvent, wrap parse_frame
bins/scanner/src/capture.rs                       MODIFY: use tape; housekeeping via retention::housekeep; UniverseDedup + deliver (acks); universe mtime rule
bins/scanner/src/replay.rs                        MODIFY: apply_line + ReplayState (per-line driver), tape line reading
bins/scanner/src/retention.rs                     NEW: retention plan, housekeep, compactor_alive, GuardView, guard_task
bins/scanner/src/metrics.rs                       MODIFY: universe_acked_seq
bins/scanner/src/{lib,run,shards,engine}.rs       MODIFY: wiring; canary_key from tape; engine uses tape::feed::server_top
bins/scanner/Cargo.toml                           MODIFY: + tape; dev-dependency compact (Task 12)
bins/scanner/tests/agreement/mod.rs               NEW: the shared fixture hour and engine/FeedBooks driver
bins/scanner/tests/tape_agreement.rs              NEW: FeedBooks == Engine after every line
bins/scanner/tests/tob_agreement.rs               NEW: every compactor tob row == the engine's top at its line
bins/scanner/tests/record_view.rs                 NEW: tape's RecordView reads the scanner's record
bins/scanner/tests/retention.rs                   NEW
bins/scanner/tests/universe_dedup.rs              NEW
bins/scanner/tests/replay.rs                      MODIFY: the universe retention test (mtime rule)
bins/scanner/tests/arch.rs                        MODIFY: tape pure, compact no network, parquet/arrow compact-only, guard parity
config/scanner.toml                               MODIFY: retention_hours 24, max_capture_gb 15
config/compact.toml                               NEW
crates/store/migrations/0008_compact.sql          NEW
crates/store/src/compact_io.rs, src/lib.rs        NEW/MODIFY
crates/store/tests/compact_io.rs                  NEW;  crates/store/tests/migrate.rs MODIFY (table list)
bins/compact/                                     NEW: Cargo.toml, src/{lib,main,config,guard,rows,pq,reduce,windows,hour,state,archive,verify,run,bench}.rs
bins/compact/tests/                               NEW: fixture/mod.rs, common/mod.rs, config.rs, rows.rs, pq.rs, reduce.rs, l2.rs, hour.rs, checkpoint.rs, archive.rs, verify.rs, service.rs, bench.rs
bins/api/src/health.rs, bins/api/tests/health.rs  MODIFY
web/src/api/types.ts, web/src/test/fixtures.ts    MODIFY
web/src/components/Hearts.tsx, Hearts.test.tsx    MODIFY
web/src/pages/Health.tsx, Health.test.tsx         MODIFY
web/src/help.ts                                   MODIFY: archive entry
web/src/lib/bytes.ts, bytes.test.ts               NEW: fmtGb
docker/rust.Dockerfile, compose.yml, .env.example, docker/postgres/init/01-roles.sh   MODIFY
docs/superpowers/runbooks/phase5b-storage-vps.md  NEW
```

---

### Task 1: `crates/tape`: the capture format and frame parsing in one pure crate

Spec §2 requires the compactor to reuse the scanner's line parsing, frame parsing and `Book`, and bans `pm-api` from `compact`. So the pure pieces move into a new crate that both use (ruling R1): `Line`/`Ctl`/`book_ctl`/`ctl_book` and the time/path helpers leave `bins/scanner/src/capture.rs`, `MarketEvent`/`parse_frame` leave `crates/pm-api/src/ws.rs`. Both old modules re-export them, so every existing import keeps compiling. Replay gains a per-line driver (`apply_line`) that Task 4's agreement test uses.

**Files:**
- Create: `crates/tape/Cargo.toml`, `crates/tape/src/{lib,time,line,frame,files}.rs`, `crates/tape/tests/{frame,files}.rs`
- Modify: `Cargo.toml` (members), `crates/pm-api/Cargo.toml`, `crates/pm-api/src/ws.rs`, `crates/pm-api/src/error.rs`, `bins/scanner/Cargo.toml`, `bins/scanner/src/capture.rs`, `bins/scanner/src/replay.rs`, `bins/scanner/tests/arch.rs`

**Interfaces:**
- Produces (crate `tape`):
  - `tape::time`: `HOUR_MS`, `DAY_MS`, `hour_of(ms) -> i64`, `day_of(ms) -> i64`, `civil(ms) -> (i64, u32, u32, u32)`, `days_from_civil(y: i64, m: i64, d: i64) -> i64`, `day_name(ms) -> String` (`YYYY-MM-DD`), `hour_label(ms) -> String` (`YYYY-MM-DDTHH`), `parse_day(&str) -> Option<i64>` (00:00Z ms, strict).
  - `tape::line`: `Line { recv_ts_us, shard, raw: Option<String>, ctl: Option<Ctl> }`, `Ctl` (unchanged variants), `book_ctl(&str, &Book) -> Ctl`, `ctl_book(&[(String,String)], &[(String,String)], i64) -> anyhow::Result<Book>`.
  - `tape::frame`: `MarketEvent` (unchanged), `FrameError { Json(serde_json::Error), Field { field: &'static str, detail: String } }`, `parse_frame(&str) -> Result<Vec<MarketEvent>, FrameError>`.
  - `tape::files`: `day_dir(dir, ms) -> PathBuf`, `hour_path`, `universe_path`, `universe_file_name`, `universe_taken_ms(&str) -> Option<i64>`, `hour_start_ms(day: &str, file: &str) -> Option<i64>`, `HourFile { start_ms, path, bytes }`, `UniverseFile { path, taken_ms, bytes, mtime_ms: Option<i64> }`, `Listing { hours, universes }`, `listing(dir) -> Listing`, `for_each_line(path, FnMut(Line) -> anyhow::Result<()>) -> anyhow::Result<()>`, `read_zst_json<T>(path) -> anyhow::Result<T>`, `write_zst_json<T>(path, &T) -> anyhow::Result<()>`, `write_lines(path, &[Line]) -> anyhow::Result<()>`.
  - `scanner::replay`: `ReplayState { frames, parse_errors, last_ms, shut_down }` (Default), `apply_line(&mut Engine, dir: &Path, &Line, &mut ReplayState, &mut impl FnMut(Out)) -> anyhow::Result<()>`.
  - `pm_api::ws::{MarketEvent, parse_frame}` and `scanner::capture::{Line, Ctl, book_ctl, ctl_book, civil, hour_path, universe_path, universe_file_name}` keep their paths (re-exports).

- [ ] **Step 0: Free the disk first**

`target/` is about 15 GB (over the 8 GB rule). Clean it once before the first build of this plan, then check the budget:
```bash
cargo clean && sh scripts/disk-budget.sh
```
Expected: the budget script passes (under 30 GB).

- [ ] **Step 1: Write the failing tests**

`crates/tape/tests/files.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use tape::files::*;
use tape::line::{Ctl, Line};
use tape::time::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tape-test-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const T0: i64 = 1_790_121_600_000; // 2026-09-23 00:00Z

#[test]
fn civil_days_and_labels_match_known_values() {
    assert_eq!(civil(0), (1970, 1, 1, 0));
    assert_eq!(civil(T0), (2026, 9, 23, 0));
    assert_eq!(civil(1_790_200_799_999), (2026, 9, 23, 21));
    assert_eq!(civil(951_782_400_000), (2000, 2, 29, 0));
    assert_eq!(day_name(T0 + 5 * HOUR_MS), "2026-09-23");
    assert_eq!(hour_label(T0 + 13 * HOUR_MS + 59_999), "2026-09-23T13");
    assert_eq!(parse_day("2026-09-23"), Some(T0));
    assert_eq!(parse_day("2026-02-30"), None, "not a real date");
    assert_eq!(parse_day("_state"), None);
    assert_eq!(hour_of(T0 + 3_599_999), T0);
    assert_eq!(day_of(T0 + DAY_MS - 1), T0);
    assert_eq!(days_from_civil(2026, 9, 23) * DAY_MS, T0);
}

#[test]
fn hour_and_universe_paths_round_trip() {
    let p = hour_path(Path::new("/data/capture"), 1_790_200_799_999);
    assert!(p.ends_with("2026-09-23/21.jsonl.zst"), "{}", p.display());
    assert_eq!(hour_start_ms("2026-09-23", "21.jsonl.zst"), Some(T0 + 21 * HOUR_MS));
    assert_eq!(hour_start_ms("2026-09-23", "universe-1-2.json.zst"), None);
    let name = universe_file_name(T0, 7);
    assert_eq!(name, format!("universe-{T0}-7.json.zst"));
    assert_eq!(universe_taken_ms(&name), Some(T0));
    assert!(universe_path(Path::new("/c"), T0, &name).ends_with(format!("2026-09-23/{name}")));
}

#[test]
fn listing_sorts_hours_and_universes_and_lines_round_trip() {
    let dir = tmp("listing");
    let lines = vec![
        Line { recv_ts_us: (T0 + 1_000) * 1000, shard: 2, raw: Some("PONG".into()), ctl: None },
        Line { recv_ts_us: (T0 + 2_000) * 1000, shard: 0, raw: None, ctl: Some(Ctl::Down) },
    ];
    write_lines(&hour_path(&dir, T0 + HOUR_MS), &lines).unwrap();
    write_lines(&hour_path(&dir, T0), &lines[..1]).unwrap();
    write_zst_json(&universe_path(&dir, T0, &universe_file_name(T0, 1)), &serde_json::json!({"a": 1})).unwrap();
    let l = listing(&dir);
    assert_eq!(l.hours.iter().map(|h| h.start_ms).collect::<Vec<_>>(), vec![T0, T0 + HOUR_MS]);
    assert!(l.hours.iter().all(|h| h.bytes > 0));
    assert_eq!(l.universes.len(), 1);
    assert_eq!(l.universes[0].taken_ms, T0);
    assert!(l.universes[0].mtime_ms.is_some());
    let mut back = Vec::new();
    for_each_line(&l.hours[1].path, |x| {
        back.push(x);
        Ok(())
    })
    .unwrap();
    assert_eq!(back, lines);
    let v: serde_json::Value = read_zst_json(&l.universes[0].path).unwrap();
    assert_eq!(v["a"], 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_bad_capture_line_is_an_error_naming_the_file() {
    let dir = tmp("bad-line");
    let p = hour_path(&dir, T0);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, zstd::encode_all(&b"{not json}\n"[..], 3).unwrap()).unwrap();
    let e = for_each_line(&p, |_| Ok(())).unwrap_err();
    assert!(format!("{e:#}").contains("bad capture line in"), "{e:#}");
    std::fs::remove_dir_all(&dir).ok();
}
```
`crates/tape/tests/frame.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Side;
use tape::frame::{FrameError, MarketEvent, parse_frame};

#[test]
fn a_book_and_a_price_change_parse_like_the_scanner_always_did() {
    let evs = parse_frame(r#"[{"event_type":"book","asset_id":"1","timestamp":"5","hash":"h","bids":[{"price":"0.4","size":"10"}],"asks":[]}]"#).unwrap();
    assert!(matches!(&evs[0], MarketEvent::Snapshot { token, server_ts_ms: 5, bids, .. } if token == "1" && bids.len() == 1));
    let evs = parse_frame(r#"{"event_type":"price_change","timestamp":"6","price_changes":[{"asset_id":"1","side":"SELL","price":"0.5","size":"0","best_bid":"0.4","best_ask":"1"}]}"#).unwrap();
    assert!(matches!(&evs[0], MarketEvent::Delta { side: Side::Ask, server_ts_ms: 6, .. }));
    assert!(parse_frame("PONG").unwrap().is_empty());
}

#[test]
fn a_bad_item_in_an_array_names_its_asset_id() {
    let e = parse_frame(r#"[{"event_type":"book","asset_id":"bad-one","timestamp":"1","bids":[{"price":"2","size":"1"}],"asks":[]}]"#).unwrap_err();
    assert!(matches!(e, FrameError::Field { .. }));
    assert!(e.to_string().contains("asset_id=bad-one"), "{e}");
    assert!(matches!(parse_frame("{").unwrap_err(), FrameError::Json(_)));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p tape`
Expected: FAIL (`package(s) tape not found`).

- [ ] **Step 3: Create the crate**

Add `"crates/tape"` to `members` in the root `Cargo.toml` (after `"crates/cards"`).

`crates/tape/Cargo.toml`:
```toml
[package]
name = "tape"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# The raw capture format and the scanner's book rules, shared by scanner and compact (spec 5b §2).
# Pure: files and parsing only, no network, no database, no async (bins/scanner/tests/arch.rs).
[dependencies]
arb-core = { path = "../arb-core" }
rust_decimal.workspace = true
serde.workspace = true
serde_json.workspace = true
zstd.workspace = true
anyhow.workspace = true
thiserror.workspace = true

[dev-dependencies]
rust_decimal_macros.workspace = true

[lints]
workspace = true
```
`crates/tape/src/lib.rs`:
```rust
//! tape: the raw capture as both the scanner and the compactor read it (spec 5b §2, "reuse, not a
//! second implementation"): capture and control lines, Polymarket frame parsing, the capture file
//! layout, and the scanner engine's book rules (`feed`, Tasks 2–3).
#![forbid(unsafe_code)]

pub mod files;
pub mod frame;
pub mod line;
pub mod time;
```
`crates/tape/src/time.rs` (`civil` moved verbatim from `bins/scanner/src/capture.rs`; `days_from_civil` is the inverse that `hour_start_ms` used inline):
```rust
//! UTC calendar helpers, integers only (Howard Hinnant's civil algorithms).
pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;

pub fn hour_of(ms: i64) -> i64 {
    ms.div_euclid(HOUR_MS) * HOUR_MS
}

pub fn day_of(ms: i64) -> i64 {
    ms.div_euclid(DAY_MS) * DAY_MS
}

/// UTC (year, month, day, hour) of a millisecond timestamp.
pub fn civil(ms: i64) -> (i64, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let hour = u32::try_from(secs.rem_euclid(86_400) / 3600).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, hour)
}

/// Days since 1970-01-01 of a civil date (inverse of `civil`).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` of the UTC day containing `ms`.
pub fn day_name(ms: i64) -> String {
    let (y, m, d, _) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DDTHH` of the UTC hour containing `ms` (log lines, spec 5b §5).
pub fn hour_label(ms: i64) -> String {
    let (y, m, d, h) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}T{h:02}")
}

/// 00:00Z of a strict `YYYY-MM-DD` (a real date), else None.
pub fn parse_day(name: &str) -> Option<i64> {
    let b = name.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = name.get(0..4)?.parse().ok()?;
    let m: i64 = name.get(5..7)?.parse().ok()?;
    let d: i64 = name.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let ms = days_from_civil(y, m, d) * DAY_MS;
    (day_name(ms) == name).then_some(ms)
}
```
`crates/tape/src/line.rs`: move `Line`, `Ctl`, `book_ctl` and `ctl_book` **verbatim** (doc comments included) out of `bins/scanner/src/capture.rs` (they start at `pub struct Line`, `pub enum Ctl`, `pub fn book_ctl`, `pub fn ctl_book`), with this header:
```rust
//! Capture lines (parent spec §7): frames {recv_ts_us, shard, raw} and control lines
//! {recv_ts_us, shard, ctl}. The scanner writes them; replay and the compactor read them.
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};
```
`crates/tape/src/frame.rs`: move `MarketEvent`, `text_of`, `str_field`, `ts`, `opt_ts`, `opt_str_field`, `price`, `opt_price`, `levels_of`, `parse_event`, `item_asset_id`, `with_asset_id` and `parse_frame` **verbatim** out of `crates/pm-api/src/ws.rs` (from `#[derive(Debug, Clone, PartialEq, Eq)] pub enum MarketEvent` down to the end of `parse_frame`), replacing every `ApiError` with `FrameError` and every `ApiError::field(` with `FrameError::field(`, with this header:
```rust
//! Market websocket frame parsing (parent spec §3, §7), moved from pm-api (spec 5b §2) so the
//! compactor parses frames exactly as the scanner does without depending on the network crate.
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bad field {field}: {detail}")]
    Field { field: &'static str, detail: String },
}

impl FrameError {
    pub fn field(field: &'static str, detail: impl Into<String>) -> Self {
        FrameError::Field {
            field,
            detail: detail.into(),
        }
    }
}
```
and `with_asset_id` becomes:
```rust
fn with_asset_id(e: FrameError, id: &str) -> FrameError {
    match e {
        FrameError::Field { field, detail } => FrameError::Field {
            field,
            detail: format!("asset_id={id}: {detail}"),
        },
        other => FrameError::field("asset_id", format!("asset_id={id}: {other}")),
    }
}
```
`crates/tape/src/files.rs`:
```rust
//! The capture layout (parent spec §7): <dir>/YYYY-MM-DD/HH.jsonl.zst hour files and
//! universe-<taken_ms>-<seq>.json.zst records in the day of their taken_ms.
use crate::line::Line;
use crate::time::{civil, days_from_civil};
use anyhow::Context;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn day_dir(dir: &Path, ms: i64) -> PathBuf {
    let (y, m, d, _) = civil(ms);
    dir.join(format!("{y:04}-{m:02}-{d:02}"))
}

pub fn hour_path(dir: &Path, ms: i64) -> PathBuf {
    let (_, _, _, h) = civil(ms);
    day_dir(dir, ms).join(format!("{h:02}.jsonl.zst"))
}

/// `<dir>/<day>/<file>`, where `file` is the exact name from `Ctl::Universe`; the day is derived
/// from `taken_ms` alone.
pub fn universe_path(dir: &Path, taken_ms: i64, file: &str) -> PathBuf {
    day_dir(dir, taken_ms).join(file)
}

pub fn universe_file_name(taken_ms: i64, seq: u64) -> String {
    format!("universe-{taken_ms}-{seq}.json.zst")
}

/// `<taken_ms>` of a `universe-<taken_ms>-<seq>.json.zst` file name.
pub fn universe_taken_ms(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("universe-")?.strip_suffix(".json.zst")?;
    let (taken, _seq) = rest.rsplit_once('-')?;
    taken.parse().ok()
}

/// "<day>/<HH>.jsonl.zst" back to the hour's start in ms.
pub fn hour_start_ms(day: &str, file: &str) -> Option<i64> {
    let hh: i64 = file.strip_suffix(".jsonl.zst")?.parse().ok()?;
    let mut it = day.split('-');
    let (y, m, d): (i64, i64, i64) = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    Some((days_from_civil(y, m, d) * 86_400 + hh * 3600) * 1000)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourFile {
    pub start_ms: i64,
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseFile {
    pub path: PathBuf,
    pub taken_ms: i64,
    pub bytes: u64,
    /// When the file was written (its mtime); None when the filesystem can't say.
    pub mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Sorted by (start_ms, path).
    pub hours: Vec<HourFile>,
    /// Sorted by path.
    pub universes: Vec<UniverseFile>,
}

fn mtime_ms(m: &std::fs::Metadata) -> Option<i64> {
    let d = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

/// Every hour file and universe record one level below `dir` (the day folders). A missing or
/// unreadable folder lists nothing: callers never fail on a half-deleted capture tree.
pub fn listing(dir: &Path) -> Listing {
    let mut out = Listing::default();
    let Ok(days) = std::fs::read_dir(dir) else {
        return out;
    };
    for day in days.flatten() {
        let day_name = day.file_name().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(day.path()) else {
            continue;
        };
        for f in entries.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let meta = f.metadata().ok();
            let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            if let Some(start_ms) = hour_start_ms(&day_name, &name) {
                out.hours.push(HourFile {
                    start_ms,
                    path: f.path(),
                    bytes,
                });
            } else if let Some(taken_ms) = universe_taken_ms(&name) {
                out.universes.push(UniverseFile {
                    path: f.path(),
                    taken_ms,
                    bytes,
                    mtime_ms: meta.as_ref().and_then(mtime_ms),
                });
            }
        }
    }
    out.hours
        .sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then_with(|| a.path.cmp(&b.path)));
    out.universes.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Every non-blank line of one hour file, in file order. A line that is not a capture line is an
/// error naming the file (replay's contract: a corrupt capture is never silently skipped).
pub fn for_each_line(
    path: &Path,
    mut f: impl FnMut(Line) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(zstd::Decoder::new(file)?);
    for text in reader.lines() {
        let text = text?;
        if text.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(&text)
            .with_context(|| format!("bad capture line in {}", path.display()))?;
        f(line)?;
    }
    Ok(())
}

pub fn read_zst_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut dec = zstd::Decoder::new(bytes.as_slice())
        .with_context(|| format!("decompress {}", path.display()))?;
    let mut json = Vec::new();
    dec.read_to_end(&mut json)
        .with_context(|| format!("decompress {}", path.display()))?;
    serde_json::from_slice(&json).with_context(|| format!("parse {}", path.display()))
}

/// Tests and tools: write one zstd-compressed JSON document (creates the parent folder).
pub fn write_zst_json<T: Serialize>(path: &Path, v: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, zstd::encode_all(serde_json::to_vec(v)?.as_slice(), 3)?)?;
    Ok(())
}

/// Tests and tools: write capture lines as one zstd frame (creates the parent folder).
pub fn write_lines(path: &Path, lines: &[Line]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut enc = zstd::Encoder::new(File::create(path)?, 3)?;
    for l in lines {
        serde_json::to_writer(&mut enc, l)?;
        enc.write_all(b"\n")?;
    }
    enc.finish()?;
    Ok(())
}
```

- [ ] **Step 4: Point pm-api and the scanner at the moved code**

`crates/pm-api/Cargo.toml` `[dependencies]` += `tape = { path = "../tape" }`.

`crates/pm-api/src/ws.rs`: delete the moved block (from `#[derive(Debug, Clone, PartialEq, Eq)] pub enum MarketEvent` through the end of `pub fn parse_frame`), change the imports `use arb_core::book::Side;`, `use arb_core::money::{Price, Shares};` and `use serde_json::{Value, json};` to just `use serde_json::json;`, and put this where the block was:
```rust
/// Frame parsing lives in `tape` (spec 5b §2: the compactor parses frames exactly as we do).
pub use tape::frame::MarketEvent;

/// Parse one text frame (see `tape::frame::parse_frame`); errors keep this crate's type.
pub fn parse_frame(text: &str) -> Result<Vec<MarketEvent>, ApiError> {
    tape::frame::parse_frame(text).map_err(ApiError::from)
}
```
`crates/pm-api/src/error.rs`, after the `impl From<tokio_tungstenite::tungstenite::Error> for ApiError` block:
```rust
impl From<tape::frame::FrameError> for ApiError {
    fn from(e: tape::frame::FrameError) -> Self {
        match e {
            tape::frame::FrameError::Json(j) => ApiError::Json(j),
            tape::frame::FrameError::Field { field, detail } => ApiError::Field { field, detail },
        }
    }
}
```
`bins/scanner/Cargo.toml` `[dependencies]` += `tape = { path = "../../crates/tape" }`.

`bins/scanner/src/capture.rs`: delete `Line`, `Ctl`, `civil`, `day_dir`, `hour_path`, `universe_path`, `universe_file_name`, `universe_taken_ms`, `book_ctl`, `ctl_book` and `hour_start_ms` (now in `tape`), drop the imports `use arb_core::book::Book;` and `use arb_core::money::{Price, Shares};`, and add below the remaining `use` lines:
```rust
pub use tape::files::{hour_path, universe_file_name, universe_path};
pub use tape::line::{Ctl, Line, book_ctl, ctl_book};
pub use tape::time::civil;
use tape::files::{hour_start_ms, universe_taken_ms};
```
(`expired_hours`, `over_budget_hours` and `stale_universe_files` keep calling `hour_start_ms` / `universe_taken_ms`, now tape's.)

- [ ] **Step 5: Replay reads lines through tape and exposes a per-line driver**

Replace `bins/scanner/src/replay.rs` entirely with:
```rust
//! Replay a capture directory through the engine (spec §8.3). Same engine, same inputs, same outputs.
//! Lines are read by `tape::files::for_each_line`, the reader the compactor shares (spec 5b §2).
use crate::capture::{Ctl, Line, UniverseRecord, ctl_book, universe_path};
use crate::engine::{Engine, EngineCtx, Out};
use anyhow::Context;
use pm_api::ws::parse_frame;
use rust_decimal::Decimal;
use sim::trader::PaperTrader;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct ReplayOut {
    pub outs: Vec<Out>,
    pub frames: u64,
    pub parse_errors: u64,
}

#[derive(Debug)]
pub struct ReplayStats {
    pub frames: u64,
    pub parse_errors: u64,
}

/// What a replay carries from one line to the next.
#[derive(Debug, Default)]
pub struct ReplayState {
    pub frames: u64,
    pub parse_errors: u64,
    pub last_ms: i64,
    pub shut_down: bool,
}

fn walk(dir: &Path, hours: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let p = entry?.path();
        if p.is_dir() {
            walk(&p, hours)?;
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".jsonl.zst") {
                hours.push(p);
            }
        }
    }
    Ok(())
}

/// Load and decompress the universe record `Ctl::Universe` names, at its exact path (F3: the
/// ctl line carries the file name, so replay never has to search for it by `taken_ms`).
fn load_universe(dir: &Path, taken_ms: i64, file: &str) -> anyhow::Result<UniverseRecord> {
    tape::files::read_zst_json(&universe_path(dir, taken_ms, file))
}

/// Feed one capture line to the engine exactly as replay does. The tape agreement test
/// (bins/scanner/tests/tape_agreement.rs) drives the engine line by line through this.
pub fn apply_line(
    engine: &mut Engine,
    dir: &Path,
    line: &Line,
    st: &mut ReplayState,
    sink: &mut impl FnMut(Out),
) -> anyhow::Result<()> {
    let ms = line.recv_ts_us.div_euclid(1000);
    st.last_ms = st.last_ms.max(ms);
    if let Some(raw) = &line.raw {
        st.frames += 1;
        match parse_frame(raw) {
            Ok(evs) => {
                for o in engine.on_frame(line.shard, ms, &evs) {
                    sink(o);
                }
                engine.alive(line.shard, ms);
            }
            Err(_) => st.parse_errors += 1,
        }
    }
    match &line.ctl {
        None => {}
        Some(Ctl::Down) => {
            for o in engine.disconnect(line.shard, ms) {
                sink(o);
            }
        }
        Some(Ctl::Universe { taken_ms, file }) => {
            let rec = load_universe(dir, *taken_ms, file)?;
            let conn: HashMap<String, usize> =
                rec.conn_of.iter().map(|(k, v)| (k.clone(), *v)).collect();
            for o in engine.apply_universe(Arc::new(rec.universe), &conn, ms) {
                sink(o);
            }
        }
        Some(Ctl::RestSnapshot {
            token,
            server_ts_ms,
            bids,
            asks,
        }) => {
            let b = ctl_book(bids, asks, *server_ts_ms)?;
            for o in engine.rest_snapshot(token, b) {
                sink(o);
            }
        }
        Some(Ctl::Suspect { event_id, until_ms }) => {
            for o in engine.mark_suspect(event_id, *until_ms, ms) {
                sink(o);
            }
        }
        Some(Ctl::Resolved {
            market_id,
            payout_yes,
            payout_no,
        }) => {
            let y = Decimal::from_str_exact(payout_yes)
                .with_context(|| format!("bad payout_yes {payout_yes:?}"))?;
            let n = Decimal::from_str_exact(payout_no)
                .with_context(|| format!("bad payout_no {payout_no:?}"))?;
            for o in engine.on_resolution(ms, market_id, y, n) {
                sink(o);
            }
        }
        Some(Ctl::Shutdown) => {
            for o in engine.close_all(ms) {
                sink(o);
            }
            for o in engine.paper_close(ms) {
                sink(o);
            }
            st.shut_down = true;
        }
    }
    Ok(())
}

/// Replay a capture directory, delivering every engine output to `sink` in order instead of
/// collecting them, so a long replay does not have to hold millions of `Out`s in memory. With a
/// paper trader (S3c) the trader starts FRESH (never from `paper_state`), so two replays of one
/// capture produce identical paper rows; resolutions come only from `Ctl::Resolved` lines.
pub fn replay_dir_paper_with(
    dir: &Path,
    ctx: EngineCtx,
    paper: Option<PaperTrader>,
    mut sink: impl FnMut(Out),
) -> anyhow::Result<ReplayStats> {
    let mut hours = Vec::new();
    walk(dir, &mut hours)?;
    hours.sort();
    let mut engine = Engine::new(ctx);
    if let Some(t) = paper {
        engine.set_paper(t);
    }
    let mut st = ReplayState::default();
    for h in hours {
        tape::files::for_each_line(&h, |line| {
            apply_line(&mut engine, dir, &line, &mut st, &mut sink)
        })?;
    }
    if !st.shut_down {
        // crash: close at the last captured instant
        for o in engine.close_all(st.last_ms) {
            sink(o);
        }
        for o in engine.paper_close(st.last_ms) {
            sink(o);
        }
    }
    Ok(ReplayStats {
        frames: st.frames,
        parse_errors: st.parse_errors,
    })
}

/// Replay without a paper trader (the S2 scanner outputs only).
pub fn replay_dir_with(
    dir: &Path,
    ctx: EngineCtx,
    sink: impl FnMut(Out),
) -> anyhow::Result<ReplayStats> {
    replay_dir_paper_with(dir, ctx, None, sink)
}

pub fn replay_dir(dir: &Path, ctx: EngineCtx) -> anyhow::Result<ReplayOut> {
    let mut outs = Vec::new();
    let stats = replay_dir_with(dir, ctx, |o| outs.push(o))?;
    Ok(ReplayOut {
        outs,
        frames: stats.frames,
        parse_errors: stats.parse_errors,
    })
}
```
(Behaviour is unchanged: the old loop's body is `apply_line`, the old `BufReader` + `serde_json` loop is `for_each_line`, and `load_universe` reads through `read_zst_json` with the same contexts.)

- [ ] **Step 6: The arch test keeps tape pure**

In `bins/scanner/tests/arch.rs` `crate_graph_matches_spec_9_2`, add `"tape"` to the pure list: change `for pure in ["arb-core", "sim", "cards"] {` to `for pure in ["arb-core", "sim", "cards", "tape"] {`.

- [ ] **Step 7: Run everything that moved**

Run: `cargo test -p tape && cargo test -p pm-api && cargo test -p scanner && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS (the pm-api `ws` tests, including `bad_item_in_array_names_its_asset_id`, and every scanner replay test pass unchanged; `spikes/probe` compiles through the re-exports).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/tape crates/pm-api/Cargo.toml crates/pm-api/src/ws.rs crates/pm-api/src/error.rs bins/scanner/Cargo.toml bins/scanner/src/capture.rs bins/scanner/src/replay.rs bins/scanner/tests/arch.rs
git commit -m "tape: one pure home for capture lines, frame parsing and the capture layout"
```

---
### Task 2: `tape`: the universe record view and the shared `server_top`

The compactor must rebuild books exactly as the scanner did (spec §2, §8 "agreement with the scanner"). The engine keeps its books inside `arb_core::detect::Group` (a premise file), so its book code can't be lifted out without touching premise areas. Ruling R1 (**owner signed off 2026-09-27**, see Rulings): the book *rules* are re-expressed in `tape::feed::FeedBooks` (Task 3) on the same `Book`, frame parser and line type, pinned to the engine line by line (Task 4, and Task 12 for the rows). This task lays the two shared pieces: a read-only view of the scanner's universe record, and `server_top`, the one expression the engine and `FeedBooks` must agree on byte for byte, which the engine now calls from `tape`.

**Files:**
- Create: `crates/tape/src/universe_view.rs`, `crates/tape/src/feed.rs` (`server_top` only; Task 3 fills it), `crates/tape/tests/server_top.rs`, `bins/scanner/tests/record_view.rs`
- Modify: `crates/tape/src/lib.rs`, `bins/scanner/src/engine.rs` (one expression), `bins/scanner/src/shards.rs` (`canary_key` from tape)

**Interfaces:**
- Consumes: Task 1 (`tape::files`).
- Produces:
  - `tape::universe_view`: `LegView { market_id, yes_token, no_token, status: String }`, `GroupView { event_id, neg_risk_market_id: Option<String>, legs }`, `CanaryView { market_id, event_id, yes_token, no_token }`, `UniverseView { taken_ms, groups, canary }`, `RecordView { universe, conn_of: BTreeMap<String, usize>, file }` (all `Serialize + Deserialize + Clone + PartialEq`), `load_record(dir, taken_ms, file) -> anyhow::Result<RecordView>`, `canary_key(market_id) -> String`.
  - `tape::feed::server_top(Option<Price>, Option<Price>) -> (Option<Price>, Option<Price>)`.

- [ ] **Step 1: Write the failing tests**

`crates/tape/tests/server_top.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::money::Price;
use tape::feed::server_top;

#[test]
fn the_empty_side_markers_are_not_levels() {
    let p = |s: &str| Price::parse(s).unwrap();
    assert_eq!(server_top(Some(p("0")), Some(p("1"))), (None, None));
    assert_eq!(server_top(Some(p("0.4")), Some(p("0.6"))), (Some(p("0.4")), Some(p("0.6"))));
    assert_eq!(server_top(None, None), (None, None));
}
```
`bins/scanner/tests/record_view.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The compactor reads universe records through tape's view; it must read what the scanner writes.
use rust_decimal::Decimal;
use scanner::capture::{UniverseRecord, universe_file_name};
use scanner::universe::{CanaryPair, ConnMap, GroupMeta, LegMeta, Status, Universe};
use tape::universe_view::RecordView;

const T0: i64 = 1_790_121_600_000;

fn leg(t: &str, status: Status) -> LegMeta {
    LegMeta {
        market_id: format!("m-{t}"),
        yes_token: t.into(),
        no_token: format!("n-{t}"),
        status,
        fee_rate: Decimal::ZERO,
        fee_exponent: 1,
        tick: None,
        min_size: None,
        seconds_delay: None,
        end_date: None,
    }
}

#[test]
fn the_record_view_reads_what_the_scanner_writes() {
    let u = Universe {
        taken_ms: T0,
        groups: vec![GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: Some("nr-e1".into()),
            augmented: false,
            legs: vec![leg("a1", Status::Live), leg("a3", Status::Halted)],
            tags: Some(vec!["politics".into()]),
        }],
        canary: vec![CanaryPair { market_id: "m9".into(), event_id: "e9".into(), yes_token: "cy".into(), no_token: "cn".into() }],
    };
    let rec = UniverseRecord {
        universe: u,
        conn_of: [("e1".to_string(), 0), ("canary:m9".to_string(), 1)].into_iter().collect::<ConnMap>(),
        file: universe_file_name(T0, 3),
    };
    let v: RecordView = serde_json::from_slice(&serde_json::to_vec(&rec).unwrap()).unwrap();
    assert_eq!(v.file, rec.file);
    assert_eq!(v.universe.taken_ms, T0);
    assert_eq!(v.conn_of.get("canary:m9"), Some(&1));
    assert_eq!(v.universe.groups[0].neg_risk_market_id.as_deref(), Some("nr-e1"));
    let legs: Vec<(String, String)> = v.universe.groups[0].legs.iter().map(|l| (l.yes_token.clone(), l.status.clone())).collect();
    assert_eq!(legs, vec![("a1".into(), "live".into()), ("a3".into(), "halted".into())]);
    assert_eq!(v.universe.canary[0].no_token, "cn");
    assert_eq!(scanner::shards::canary_key("m9"), "canary:m9");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p tape --test server_top; cargo test -p scanner --test record_view`
Expected: FAIL (no `tape::feed`, no `tape::universe_view`).

- [ ] **Step 3: The universe projection**

`crates/tape/src/universe_view.rs`:
```rust
//! A read-only projection of the scanner's universe record (bins/scanner/src/capture.rs
//! `UniverseRecord`): only the fields the compactor needs. serde ignores the rest, so the scanner
//! can add fields freely; bins/scanner/tests/tape_agreement.rs parses a real record with it.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegView {
    pub market_id: String,
    pub yes_token: String,
    pub no_token: String,
    /// The scanner's `Status` in snake_case: "live", "halted", "closed_no", …
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupView {
    pub event_id: String,
    #[serde(default)]
    pub neg_risk_market_id: Option<String>,
    pub legs: Vec<LegView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryView {
    pub market_id: String,
    pub event_id: String,
    pub yes_token: String,
    pub no_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniverseView {
    pub taken_ms: i64,
    pub groups: Vec<GroupView>,
    pub canary: Vec<CanaryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordView {
    pub universe: UniverseView,
    /// Unit key → connection: an event id for a group, `canary_key(market_id)` for a canary pair.
    pub conn_of: BTreeMap<String, usize>,
    pub file: String,
}

/// The shard-plan unit key of a canary pair (bins/scanner/src/shards.rs re-exports this one).
pub fn canary_key(market_id: &str) -> String {
    format!("canary:{market_id}")
}

pub fn load_record(dir: &Path, taken_ms: i64, file: &str) -> anyhow::Result<RecordView> {
    crate::files::read_zst_json(&crate::files::universe_path(dir, taken_ms, file))
}
```
In `bins/scanner/src/shards.rs` replace
```rust
pub fn canary_key(market_id: &str) -> String {
    format!("canary:{market_id}")
}
```
with `pub use tape::universe_view::canary_key;`.

- [ ] **Step 4: `server_top` in `tape::feed`**

`crates/tape/src/feed.rs` (Task 3 replaces this file with the full module, which keeps this function unchanged):
```rust
//! The scanner engine's book rules (Task 3). For now: the one expression the engine shares.
use arb_core::money::Price;
use rust_decimal::Decimal;

/// Polymarket reports an EMPTY bid side as best_bid "0" and an EMPTY ask side as best_ask "1"
/// (live smoke test 2026-09-25): no real order rests at either, so neither is a level. The
/// engine calls this too, so the two can never disagree about it.
pub fn server_top(
    best_bid: Option<Price>,
    best_ask: Option<Price>,
) -> (Option<Price>, Option<Price>) {
    (
        best_bid.filter(|x| x.get() != Decimal::ZERO),
        best_ask.filter(|x| x.get() != Decimal::ONE),
    )
}
```
`crates/tape/src/lib.rs`: add `pub mod feed;` and `pub mod universe_view;` (alphabetical with the others).

- [ ] **Step 5: The engine calls the shared `server_top`**

In `bins/scanner/src/engine.rs` `on_frame`, the `MarketEvent::Delta` arm, replace
```rust
                    let (ts, bb, ba) = (
                        *server_ts_ms,
                        best_bid.filter(|x| x.get() != Decimal::ZERO),
                        best_ask.filter(|x| x.get() != Decimal::ONE),
                    );
```
with
```rust
                    let (bb, ba) = tape::feed::server_top(*best_bid, *best_ask);
                    let ts = *server_ts_ms;
```
keeping the two comment lines above it. `engine.rs` is not a premise file (`crates/arb-core/src/premises.rs`), so `premises_lock` is unaffected.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p tape && cargo test -p scanner && cargo test -p arb-core --test premises_lock && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS (engine behaviour is unchanged: every scanner engine and replay test passes as before).

- [ ] **Step 7: Commit**

```bash
git add crates/tape/src/lib.rs crates/tape/src/feed.rs crates/tape/src/universe_view.rs crates/tape/tests/server_top.rs bins/scanner/src/engine.rs bins/scanner/src/shards.rs bins/scanner/tests/record_view.rs
git commit -m "tape: universe record view and the server_top the engine now shares"
```

---

### Task 3: `tape::feed::FeedBooks`: the engine's book rules

Ruling R1 (owner signed off 2026-09-27): `FeedBooks` re-expresses only the engine's book rules — which tokens hold a book (live group legs and canary tokens), snapshots, deltas, the deferred server-top trim (engine `pending_tops` / `resolve_pending` / `check_book`), REST snapshots, disconnects, shutdown, and what a universe change carries (books by token) or resets (a group with no connection) — on the same `Book`, parser and line type. Unit tests here pin each rule; Task 4 pins the whole to the engine line by line.

**Files:**
- Modify: `crates/tape/src/feed.rs` (the full module; `server_top` unchanged)
- Create: `crates/tape/tests/feed.rs`

**Interfaces:**
- Consumes: Task 1 (`tape::line`, `tape::frame`), Task 2 (`tape::universe_view`, `server_top`).
- Produces (`tape::feed`): `TokenMeta { market_id, event_id, outcome, neg_risk }`, `Levels { bids, asks }` (`Levels::of(&Book)`), `UniverseChange { appeared, reset, gone }`, `LineEvent { None, Frame, BadLine, Down(usize), Universe(UniverseChange), UniverseMissing(String), Shutdown }`, `Applied { touched, snapshotted, before: Vec<(String, Levels)>, event }` + `Applied::before_of(&str) -> Option<&Levels>`, and `FeedBooks` with `new()`, `apply(&mut self, &Line, &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>, &dyn Fn(&str) -> bool) -> Applied`, `book(&str) -> Option<&Book>`, `meta(&str) -> Option<&TokenMeta>`, `conn(&str) -> Option<usize>`, `visible_tokens() -> Vec<String>`, `tokens_on(usize) -> Vec<String>`, `event_tokens(&str) -> Vec<String>`, `to_state() -> FeedState`, `from_state(&FeedState) -> anyhow::Result<FeedBooks>`; `FeedState` (`Serialize + Deserialize + Default + PartialEq`).

- [ ] **Step 1: Write the failing tests**

`crates/tape/tests/feed.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::BTreeMap;
use tape::feed::*;
use tape::line::{Ctl, Line};
use tape::universe_view::*;

const T: i64 = 1_790_121_600_000;

fn rec(conn: &[(&str, usize)]) -> RecordView {
    RecordView {
        universe: UniverseView {
            taken_ms: T,
            groups: vec![GroupView {
                event_id: "e1".into(),
                neg_risk_market_id: Some("nr".into()),
                legs: ["a1", "a2"]
                    .iter()
                    .map(|t| LegView {
                        market_id: format!("m-{t}"),
                        yes_token: t.to_string(),
                        no_token: format!("n-{t}"),
                        status: "live".into(),
                    })
                    .collect(),
            }],
            canary: vec![CanaryView {
                market_id: "m9".into(),
                event_id: "e9".into(),
                yes_token: "cy".into(),
                no_token: "cn".into(),
            }],
        },
        conn_of: conn.iter().map(|(k, v)| (k.to_string(), *v)).collect::<BTreeMap<_, _>>(),
        file: "u.json.zst".into(),
    }
}

fn frame(t: i64, shard: usize, raw: String) -> Line {
    Line { recv_ts_us: t * 1000, shard, raw: Some(raw), ctl: None }
}

fn ctl(t: i64, shard: usize, c: Ctl) -> Line {
    Line { recv_ts_us: t * 1000, shard, raw: None, ctl: Some(c) }
}

fn uni(t: i64) -> Line {
    ctl(t, 0, Ctl::Universe { taken_ms: T, file: "u.json.zst".into() })
}

fn book(tok: &str, ts: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> String {
    let lv = |v: &[(&str, &str)]| {
        v.iter().map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#)).collect::<Vec<_>>().join(",")
    };
    format!(r#"{{"event_type":"book","asset_id":"{tok}","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#, lv(bids), lv(asks))
}

fn change(tok: &str, ts: i64, side: &str, price: &str, size: &str, bb: &str, ba: &str) -> String {
    format!(r#"{{"event_type":"price_change","timestamp":"{ts}","price_changes":[{{"asset_id":"{tok}","side":"{side}","price":"{price}","size":"{size}","best_bid":"{bb}","best_ask":"{ba}"}}]}}"#)
}

fn run(f: &mut FeedBooks, r: &RecordView, lines: &[Line]) -> Vec<Applied> {
    let mut load = |_: i64, _: &str| Ok(r.clone());
    lines.iter().map(|l| f.apply(l, &mut load, &|_| true)).collect()
}

fn asks(f: &FeedBooks, t: &str) -> Vec<String> {
    f.book(t).unwrap().asks_best_first().iter().map(|(p, s)| format!("{}x{}", p.get(), s.get())).collect()
}

#[test]
fn a_universe_makes_tokens_visible_with_their_metadata_and_shard() {
    let r = rec(&[("e1", 0), ("canary:m9", 1)]);
    let mut f = FeedBooks::new();
    let ap = run(&mut f, &r, &[uni(T)]);
    assert_eq!(ap[0].event, LineEvent::Universe(UniverseChange {
        appeared: vec!["a1".into(), "a2".into(), "cn".into(), "cy".into()],
        reset: vec![],
        gone: vec![],
    }));
    assert_eq!(f.conn("a1"), Some(0));
    assert_eq!(f.conn("cy"), Some(1));
    assert_eq!(f.tokens_on(1), vec!["cn".to_string(), "cy".to_string()]);
    assert_eq!(f.event_tokens("e1"), vec!["a1".to_string(), "a2".to_string()]);
    let m = f.meta("cn").unwrap();
    assert_eq!((m.outcome.as_str(), m.neg_risk, m.event_id.as_str()), ("no", false, "e9"));
    assert!(f.meta("a1").unwrap().neg_risk);
    assert!(f.book("zz").is_none());
}

#[test]
fn a_phantom_level_is_trimmed_once_its_update_is_complete() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let ap = run(&mut f, &r, &[
        uni(T),
        frame(T + 10, 0, book("a1", T + 5, &[("0.40", "10")], &[("0.45", "10")])),
        // the server says the best ask is 0.46: our 0.45 is a phantom, but only once this update is complete
        frame(T + 20, 0, change("a1", T + 15, "BUY", "0.41", "3", "0.41", "0.46")),
        // same server ts: part of the same update, the trim waits
        frame(T + 21, 0, change("a1", T + 15, "BUY", "0.41", "4", "0.41", "0.46")),
        // a later ts on the same token settles the pending check first
        frame(T + 30, 0, change("a1", T + 25, "BUY", "0.39", "1", "0.41", "0.46")),
    ]);
    assert_eq!(asks(&f, "a1"), Vec::<String>::new(), "0.45 trimmed");
    assert_eq!(ap[1].snapshotted, vec!["a1".to_string()]);
    assert!(ap[4].touched.contains(&"a1".to_string()));
    assert_eq!(ap[4].before_of("a1").unwrap().asks.len(), 1, "levels before this line's first change");
}

#[test]
fn a_down_line_drops_pending_checks_without_trimming() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let ap = run(&mut f, &r, &[
        uni(T),
        frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
        frame(T + 20, 0, change("a1", T + 15, "BUY", "0.30", "1", "0.30", "0.50")),
        ctl(T + 30, 0, Ctl::Down),
        frame(T + 5_000, 0, change("a2", T + 4_990, "BUY", "0.20", "1", "0.20", "1")),
    ]);
    assert_eq!(ap[3].event, LineEvent::Down(0));
    assert_eq!(asks(&f, "a1"), vec!["0.45x10".to_string()], "no trim after the connection died");
}

#[test]
fn a_group_without_a_connection_is_reset_and_never_trimmed() {
    let with = rec(&[("e1", 0)]);
    let without = rec(&[]);
    let mut f = FeedBooks::new();
    run(&mut f, &with, &[uni(T), frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")]))]);
    let ap = run(&mut f, &without, &[uni(T + 20)]);
    assert_eq!(ap[0].event, LineEvent::Universe(UniverseChange {
        appeared: vec![],
        reset: vec!["a1".into(), "a2".into()],
        gone: vec![],
    }));
    assert!(f.book("a1").unwrap().asks_best_first().is_empty(), "reset");
    run(&mut f, &without, &[
        frame(T + 30, 0, book("a1", T + 25, &[], &[("0.45", "10")])),
        frame(T + 40, 0, change("a1", T + 35, "BUY", "0.30", "1", "0.30", "0.50")),
        frame(T + 5_000, 0, change("a1", T + 4_990, "BUY", "0.29", "1", "0.30", "0.50")),
    ]);
    assert_eq!(asks(&f, "a1"), vec!["0.45x10".to_string()], "no check without a connection (F4)");
}

#[test]
fn an_older_rest_snapshot_is_ignored_and_a_newer_one_replaces_the_book() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let rest = |ts: i64, a: &str| Ctl::RestSnapshot {
        token: "a1".into(),
        server_ts_ms: ts,
        bids: vec![],
        asks: vec![(a.into(), "5".into())],
    };
    let ap = run(&mut f, &r, &[
        uni(T),
        frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
        ctl(T + 20, 0, rest(T + 4, "0.50")),
        ctl(T + 30, 0, rest(T + 6, "0.55")),
    ]);
    assert!(ap[2].touched.is_empty());
    assert_eq!(ap[3].snapshotted, vec!["a1".to_string()]);
    assert_eq!(asks(&f, "a1"), vec!["0.55x5".to_string()]);
}

#[test]
fn a_missing_universe_record_changes_nothing_and_says_so() {
    let mut f = FeedBooks::new();
    let mut load = |_: i64, _: &str| -> anyhow::Result<RecordView> { anyhow::bail!("gone") };
    let ap = f.apply(&uni(T), &mut load, &|_| false);
    assert!(matches!(ap.event, LineEvent::UniverseMissing(ref m) if m.contains("gone")));
    assert!(f.visible_tokens().is_empty());
}

#[test]
fn state_round_trips_and_behaves_the_same_afterwards() {
    let r = rec(&[("e1", 0), ("canary:m9", 1)]);
    let mut f = FeedBooks::new();
    run(&mut f, &r, &[
        uni(T),
        frame(T + 10, 0, book("a1", T + 5, &[("0.40", "10")], &[("0.45", "10")])),
        frame(T + 20, 0, change("a1", T + 15, "BUY", "0.41", "3", "0.41", "0.46")),
    ]);
    let mut g = FeedBooks::from_state(&f.to_state()).unwrap();
    assert_eq!(g.to_state(), f.to_state());
    let more = [frame(T + 3_000, 0, change("a2", T + 2_990, "BUY", "0.1", "1", "0.1", "1"))];
    run(&mut f, &r, &more);
    run(&mut g, &r, &more);
    assert_eq!(asks(&g, "a1"), asks(&f, "a1"));
    assert_eq!(asks(&g, "a1"), Vec::<String>::new(), "the restored pending check still trims");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p tape --test feed`
Expected: FAIL (no `FeedBooks`).

- [ ] **Step 3: The book rules**

Replace `crates/tape/src/feed.rs` with:
```rust
//! The scanner engine's book rules (bins/scanner/src/engine.rs) without detection, integrity rows
//! or the paper trader: which tokens hold a book (live group legs and canary tokens), snapshots,
//! deltas, the deferred server-top trim (engine `pending_tops` / `resolve_pending` /
//! `check_book`), REST snapshots, disconnects, shutdown, and what a universe change carries (books
//! by token) or resets (a group with no connection). bins/scanner/tests/tape_agreement.rs pins
//! these books to the engine's after every line (spec 5b §2, §8).
use crate::frame::{MarketEvent, parse_frame};
use crate::line::{Ctl, Line, ctl_book};
use crate::universe_view::{RecordView, canary_key};
use anyhow::Context;
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// Polymarket reports an EMPTY bid side as best_bid "0" and an EMPTY ask side as best_ask "1"
/// (live smoke test 2026-09-25): no real order rests at either, so neither is a level. The
/// engine calls this too, so the two can never disagree about it.
pub fn server_top(
    best_bid: Option<Price>,
    best_ask: Option<Price>,
) -> (Option<Price>, Option<Price>) {
    (
        best_bid.filter(|x| x.get() != Decimal::ZERO),
        best_ask.filter(|x| x.get() != Decimal::ONE),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMeta {
    pub market_id: String,
    pub event_id: String,
    /// "yes" or "no".
    pub outcome: String,
    pub neg_risk: bool,
}

/// A book's levels, best first on each side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Levels {
    pub bids: Vec<(Price, Shares)>,
    pub asks: Vec<(Price, Shares)>,
}

impl Levels {
    pub fn of(b: &Book) -> Levels {
        Levels {
            bids: b.bids_best_first(),
            asks: b.asks_best_first(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UniverseChange {
    /// Tokens that hold a visible book now and did not before (sorted).
    pub appeared: Vec<String>,
    /// Visible tokens whose book was emptied because their group has no connection (sorted).
    pub reset: Vec<String>,
    /// Tokens that held a visible book before and no longer do (sorted).
    pub gone: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LineEvent {
    #[default]
    None,
    Frame,
    /// A frame that does not parse, or a control line with bad levels.
    BadLine,
    Down(usize),
    Universe(UniverseChange),
    /// The universe record the line names could not be read (the error, for the log).
    UniverseMissing(String),
    Shutdown,
}

/// What one line did to the books.
#[derive(Debug, Default)]
pub struct Applied {
    /// Tokens whose book this line changed (or may have: every delta counts), first touch first.
    pub touched: Vec<String>,
    /// Tokens that received a complete book (a ws snapshot or an accepted REST snapshot).
    pub snapshotted: Vec<String>,
    /// For tokens the caller watches: their levels just before this line's first change.
    pub before: Vec<(String, Levels)>,
    pub event: LineEvent,
}

impl Applied {
    fn touch(&mut self, t: &str) {
        if !self.touched.iter().any(|x| x == t) {
            self.touched.push(t.to_string());
        }
    }

    fn has_before(&self, t: &str) -> bool {
        self.before.iter().any(|(x, _)| x == t)
    }

    pub fn before_of(&self, t: &str) -> Option<&Levels> {
        self.before.iter().find(|(x, _)| x == t).map(|(_, l)| l)
    }
}

#[derive(Debug, Clone)]
struct Tok {
    book: Book,
    /// In the engine's token index (a live group leg) or a canary token: frames apply to it.
    visible: bool,
    conn: Option<usize>,
    meta: TokenMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    conn: usize,
    ts: i64,
    bid: Option<Price>,
    ask: Option<Price>,
}

#[derive(Debug, Clone, Default)]
pub struct FeedBooks {
    /// Every group leg with a YES token, live or not (a non-live leg's book is carried, frozen).
    legs: HashMap<String, Tok>,
    canary: HashMap<String, Tok>,
    event_tokens: HashMap<String, Vec<String>>,
    pending: HashMap<String, Pending>,
    by_conn: HashMap<usize, BTreeSet<(i64, String)>>,
}

fn bump(f: &mut Option<i64>, t: i64) {
    *f = Some(f.map_or(t, |x| x.max(t)));
}

fn lv(v: Vec<(Price, Shares)>) -> Vec<(String, String)> {
    v.into_iter()
        .map(|(p, s)| (p.get().to_string(), s.get().to_string()))
        .collect()
}

impl FeedBooks {
    pub fn new() -> Self {
        Self::default()
    }

    /// The engine's `book_of`: a live group leg first, else a canary token.
    fn tok(&self, t: &str) -> Option<&Tok> {
        match self.legs.get(t) {
            Some(x) if x.visible => Some(x),
            _ => self.canary.get(t),
        }
    }

    fn tok_mut(&mut self, t: &str) -> Option<&mut Tok> {
        if self.legs.get(t).is_some_and(|x| x.visible) {
            return self.legs.get_mut(t);
        }
        self.canary.get_mut(t)
    }

    pub fn book(&self, t: &str) -> Option<&Book> {
        self.tok(t).map(|x| &x.book)
    }

    pub fn meta(&self, t: &str) -> Option<&TokenMeta> {
        self.tok(t).map(|x| &x.meta)
    }

    pub fn conn(&self, t: &str) -> Option<usize> {
        self.tok(t).and_then(|x| x.conn)
    }

    /// Every token that holds a visible book, sorted.
    pub fn visible_tokens(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .legs
            .iter()
            .filter(|(_, x)| x.visible)
            .map(|(t, _)| t.clone())
            .chain(self.canary.keys().cloned())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Visible tokens on connection `conn`, sorted.
    pub fn tokens_on(&self, conn: usize) -> Vec<String> {
        self.visible_tokens()
            .into_iter()
            .filter(|t| self.conn(t) == Some(conn))
            .collect()
    }

    /// The live YES tokens of an event, in leg order.
    pub fn event_tokens(&self, event_id: &str) -> Vec<String> {
        self.event_tokens.get(event_id).cloned().unwrap_or_default()
    }

    fn pending_insert(&mut self, t: &str, p: Pending) {
        self.pending_remove(t);
        self.by_conn
            .entry(p.conn)
            .or_default()
            .insert((p.ts, t.to_string()));
        self.pending.insert(t.to_string(), p);
    }

    fn pending_remove(&mut self, t: &str) -> Option<Pending> {
        let p = self.pending.remove(t)?;
        if let Some(s) = self.by_conn.get_mut(&p.conn) {
            s.remove(&(p.ts, t.to_string()));
        }
        Some(p)
    }

    fn rebuild_by_conn(&mut self) {
        self.by_conn.clear();
        for (t, p) in &self.pending {
            self.by_conn
                .entry(p.conn)
                .or_default()
                .insert((p.ts, t.clone()));
        }
    }

    fn before(&self, t: &str, ap: &mut Applied, watch: &dyn Fn(&str) -> bool) {
        if watch(t)
            && !ap.has_before(t)
            && let Some(b) = self.book(t)
        {
            ap.before.push((t.to_string(), Levels::of(b)));
        }
    }

    /// Apply one capture line. `load` reads the universe record a `Ctl::Universe` names; `watch`
    /// says which tokens need their levels from before this line (`Applied::before`).
    pub fn apply(
        &mut self,
        line: &Line,
        load: &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>,
        watch: &dyn Fn(&str) -> bool,
    ) -> Applied {
        let mut ap = Applied::default();
        if let Some(raw) = &line.raw {
            match parse_frame(raw) {
                Ok(evs) => {
                    ap.event = LineEvent::Frame;
                    self.on_frame(line.shard, &evs, &mut ap, watch);
                }
                Err(_) => ap.event = LineEvent::BadLine,
            }
        }
        match &line.ctl {
            None | Some(Ctl::Suspect { .. }) | Some(Ctl::Resolved { .. }) => {}
            Some(Ctl::Down) => {
                // engine.disconnect: this connection's deferred checks are dropped unresolved
                let gone: Vec<String> = self
                    .by_conn
                    .get(&line.shard)
                    .map(|s| s.iter().map(|(_, t)| t.clone()).collect())
                    .unwrap_or_default();
                for t in gone {
                    self.pending_remove(&t);
                }
                ap.event = LineEvent::Down(line.shard);
            }
            Some(Ctl::Universe { taken_ms, file }) => match load(*taken_ms, file) {
                Ok(rec) => ap.event = LineEvent::Universe(self.apply_universe(&rec)),
                Err(e) => ap.event = LineEvent::UniverseMissing(format!("{e:#}")),
            },
            Some(Ctl::RestSnapshot {
                token,
                server_ts_ms,
                bids,
                asks,
            }) => match ctl_book(bids, asks, *server_ts_ms) {
                Ok(b) => self.rest_snapshot(token, b, &mut ap, watch),
                Err(_) => ap.event = LineEvent::BadLine,
            },
            Some(Ctl::Shutdown) => {
                // engine.close_all clears every deferred check
                self.pending.clear();
                self.by_conn.clear();
                ap.event = LineEvent::Shutdown;
            }
        }
        ap
    }

    fn on_frame(
        &mut self,
        conn: usize,
        evs: &[MarketEvent],
        ap: &mut Applied,
        watch: &dyn Fn(&str) -> bool,
    ) {
        let mut frame_ts: Option<i64> = None;
        for ev in evs {
            match ev {
                MarketEvent::Snapshot {
                    token,
                    server_ts_ms,
                    bids,
                    asks,
                    ..
                } => {
                    bump(&mut frame_ts, *server_ts_ms);
                    if self.tok(token).is_some() {
                        self.before(token, ap, watch);
                        if let Some(x) = self.tok_mut(token) {
                            x.book.apply_snapshot(bids, asks, *server_ts_ms);
                        }
                        self.pending_remove(token); // a snapshot supersedes earlier deltas' tops
                        ap.touch(token);
                        if !ap.snapshotted.contains(token) {
                            ap.snapshotted.push(token.clone());
                        }
                    }
                }
                MarketEvent::Delta {
                    token,
                    server_ts_ms,
                    side,
                    price,
                    size,
                    best_bid,
                    best_ask,
                } => {
                    bump(&mut frame_ts, *server_ts_ms);
                    let (bb, ba) = server_top(*best_bid, *best_ask);
                    let ts = *server_ts_ms;
                    // a pending check from an EARLIER server ts belongs to a finished update
                    if self
                        .pending
                        .get(token.as_str())
                        .is_some_and(|p| p.ts < ts)
                    {
                        self.resolve(token, ap, watch);
                    }
                    if self.tok(token).is_some() {
                        self.before(token, ap, watch);
                        if let Some(x) = self.tok_mut(token) {
                            let _ = x.book.apply_delta(*side, *price, *size, ts);
                        }
                        ap.touch(token);
                        // same-ts deltas replace the pending entry: last wins
                        self.pending_insert(
                            token,
                            Pending {
                                conn,
                                ts,
                                bid: bb,
                                ask: ba,
                            },
                        );
                    }
                }
                MarketEvent::LastTrade { server_ts_ms, .. }
                | MarketEvent::TickSize { server_ts_ms, .. } => bump(&mut frame_ts, *server_ts_ms),
                MarketEvent::Resolved { .. }
                | MarketEvent::NewMarket { .. }
                | MarketEvent::Other { .. } => {}
            }
        }
        let Some(ts) = frame_ts else {
            return;
        };
        // checks on this connection more than 1 s older than this frame settle now, token order
        let mut due: Vec<String> = self
            .by_conn
            .get(&conn)
            .map(|s| {
                s.range(..(ts.saturating_sub(1_000), String::new()))
                    .map(|(_, t)| t.clone())
                    .collect()
            })
            .unwrap_or_default();
        due.sort_unstable();
        for t in due {
            self.resolve(&t, ap, watch);
        }
    }

    /// engine.resolve_pending + check_book: the server's final top trims levels it disproves.
    fn resolve(&mut self, t: &str, ap: &mut Applied, watch: &dyn Fn(&str) -> bool) {
        let Some(p) = self.pending_remove(t) else {
            return;
        };
        // A group with no connection this refresh is never checked (engine F4).
        if self.legs.get(t).is_some_and(|x| x.visible && x.conn.is_none()) {
            return;
        }
        let Some(book) = self.book(t) else {
            return;
        };
        let before = (watch(t) && !ap.has_before(t)).then(|| Levels::of(book));
        let changed = self
            .tok_mut(t)
            .is_some_and(|x| x.book.trim_to_top(p.bid, p.ask));
        if changed {
            if let Some(l) = before {
                ap.before.push((t.to_string(), l));
            }
            ap.touch(t);
        }
    }

    /// engine.rest_snapshot: replaces the book unless it is older than what we hold.
    fn rest_snapshot(
        &mut self,
        t: &str,
        book: Book,
        ap: &mut Applied,
        watch: &dyn Fn(&str) -> bool,
    ) {
        let Some(cur) = self.book(t) else {
            return;
        };
        if book.server_ts_ms() < cur.server_ts_ms() {
            return;
        }
        self.before(t, ap, watch);
        if let Some(x) = self.tok_mut(t) {
            x.book = book;
        }
        self.pending_remove(t);
        ap.touch(t);
        if !ap.snapshotted.iter().any(|x| x == t) {
            ap.snapshotted.push(t.to_string());
        }
    }

    /// engine.apply_universe, books only: group legs carry their book by YES token (a group with no
    /// connection gets an empty one), canary tokens carry theirs, deferred checks survive only for
    /// tokens still visible.
    fn apply_universe(&mut self, rec: &RecordView) -> UniverseChange {
        let was: BTreeSet<String> = self.visible_tokens().into_iter().collect();
        let mut old_legs = std::mem::take(&mut self.legs);
        let mut old_canary = std::mem::take(&mut self.canary);
        self.event_tokens.clear();
        let mut reset = Vec::new();
        for g in &rec.universe.groups {
            let conn = rec.conn_of.get(&g.event_id).copied();
            for leg in &g.legs {
                if leg.yes_token.is_empty() {
                    continue;
                }
                let carried = old_legs.remove(&leg.yes_token).map(|x| x.book);
                let visible = leg.status == "live";
                let book = match conn {
                    Some(_) => carried.unwrap_or_default(),
                    None => {
                        if visible {
                            reset.push(leg.yes_token.clone());
                        }
                        Book::default()
                    }
                };
                if visible {
                    self.event_tokens
                        .entry(g.event_id.clone())
                        .or_default()
                        .push(leg.yes_token.clone());
                }
                self.legs.insert(
                    leg.yes_token.clone(),
                    Tok {
                        book,
                        visible,
                        conn,
                        meta: TokenMeta {
                            market_id: leg.market_id.clone(),
                            event_id: g.event_id.clone(),
                            outcome: "yes".into(),
                            neg_risk: g.neg_risk_market_id.is_some(),
                        },
                    },
                );
            }
        }
        for c in &rec.universe.canary {
            let conn = rec.conn_of.get(&canary_key(&c.market_id)).copied();
            for (t, outcome) in [(&c.yes_token, "yes"), (&c.no_token, "no")] {
                let book = old_canary.remove(t).map(|x| x.book).unwrap_or_default();
                self.canary.insert(
                    t.clone(),
                    Tok {
                        book,
                        visible: true,
                        conn,
                        meta: TokenMeta {
                            market_id: c.market_id.clone(),
                            event_id: c.event_id.clone(),
                            outcome: outcome.into(),
                            neg_risk: false,
                        },
                    },
                );
            }
        }
        let now: BTreeSet<String> = self.visible_tokens().into_iter().collect();
        let pending = std::mem::take(&mut self.pending);
        self.pending = pending
            .into_iter()
            .filter(|(t, _)| now.contains(t))
            .collect();
        self.rebuild_by_conn();
        reset.sort();
        reset.dedup();
        UniverseChange {
            appeared: now.difference(&was).cloned().collect(),
            reset,
            gone: was.difference(&now).cloned().collect(),
        }
    }

    pub fn to_state(&self) -> FeedState {
        let tok = |(t, x): (&String, &Tok)| TokState {
            token: t.clone(),
            bids: lv(x.book.bids_best_first()),
            asks: lv(x.book.asks_best_first()),
            server_ts_ms: x.book.server_ts_ms(),
            visible: x.visible,
            conn: x.conn,
            meta: x.meta.clone(),
        };
        let mut legs: Vec<TokState> = self.legs.iter().map(tok).collect();
        legs.sort_by(|a, b| a.token.cmp(&b.token));
        let mut canary: Vec<TokState> = self.canary.iter().map(tok).collect();
        canary.sort_by(|a, b| a.token.cmp(&b.token));
        let mut pending: Vec<PendingState> = self
            .pending
            .iter()
            .map(|(t, p)| PendingState {
                token: t.clone(),
                conn: p.conn,
                ts: p.ts,
                bid: p.bid.map(|x| x.get().to_string()),
                ask: p.ask.map(|x| x.get().to_string()),
            })
            .collect();
        pending.sort_by(|a, b| a.token.cmp(&b.token));
        let mut event_tokens: Vec<(String, Vec<String>)> = self
            .event_tokens
            .iter()
            .map(|(e, v)| (e.clone(), v.clone()))
            .collect();
        event_tokens.sort();
        FeedState {
            legs,
            canary,
            pending,
            event_tokens,
        }
    }

    pub fn from_state(s: &FeedState) -> anyhow::Result<FeedBooks> {
        let tok = |x: &TokState| -> anyhow::Result<(String, Tok)> {
            Ok((
                x.token.clone(),
                Tok {
                    book: ctl_book(&x.bids, &x.asks, x.server_ts_ms)
                        .with_context(|| format!("book of {}", x.token))?,
                    visible: x.visible,
                    conn: x.conn,
                    meta: x.meta.clone(),
                },
            ))
        };
        let price = |v: &Option<String>| -> anyhow::Result<Option<Price>> {
            Ok(v.as_deref().map(Price::parse).transpose()?)
        };
        let mut f = FeedBooks {
            legs: s.legs.iter().map(tok).collect::<anyhow::Result<_>>()?,
            canary: s.canary.iter().map(tok).collect::<anyhow::Result<_>>()?,
            event_tokens: s.event_tokens.iter().cloned().collect(),
            pending: HashMap::new(),
            by_conn: HashMap::new(),
        };
        for p in &s.pending {
            f.pending.insert(
                p.token.clone(),
                Pending {
                    conn: p.conn,
                    ts: p.ts,
                    bid: price(&p.bid)?,
                    ask: price(&p.ask)?,
                },
            );
        }
        f.rebuild_by_conn();
        Ok(f)
    }
}

/// `FeedBooks` as plain data for the compactor's checkpoints (exact decimal strings, sorted).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedState {
    pub legs: Vec<TokState>,
    pub canary: Vec<TokState>,
    pub pending: Vec<PendingState>,
    pub event_tokens: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokState {
    pub token: String,
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
    pub server_ts_ms: i64,
    pub visible: bool,
    pub conn: Option<usize>,
    pub meta: TokenMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingState {
    pub token: String,
    pub conn: usize,
    pub ts: i64,
    pub bid: Option<String>,
    pub ask: Option<String>,
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p tape && cargo clippy -p tape --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the seven feed tests and the Task 1–2 tape tests.

- [ ] **Step 5: Commit**

```bash
git add crates/tape/src/feed.rs crates/tape/tests/feed.rs
git commit -m "tape: FeedBooks, the scanner engine's book rules"
```

---

### Task 4: the agreement test: `FeedBooks` equals the engine after every line

Spec §8 "agreement with the scanner". A hand-built capture hour that exercises every book rule (and the control lines that must *not* move books: `Suspect`, `Resolved`), a token moving between groups, a group and a canary pair appearing mid-hour, a group losing its connection, then 1,500 random deltas with often-wrong server tops, is replayed through the engine (`scanner::replay::apply_line`) and `FeedBooks` side by side; their books and top-of-book change streams must be equal after every line. The fixture lives in a shared test module because Task 12 replays it through the compactor too. An `#[ignore]` variant runs on a real capture directory (runbook §3).

**Files:**
- Create: `bins/scanner/tests/agreement/mod.rs`, `bins/scanner/tests/tape_agreement.rs`

**Interfaces:**
- Consumes: Task 1 (`apply_line`, `ReplayState`, `for_each_line`, `listing`), Tasks 2–3 (`FeedBooks`, `load_record`).
- Produces (test module `agreement`, used by Task 12): `T0`, `TOKENS`, `tmp(&str) -> PathBuf`, `ctx() -> EngineCtx`, `fixture(&Path)` (writes the hour and its universe records), `compare(&Path, Option<&[&str]>) -> usize`.

- [ ] **Step 1: Write the test**

`bins/scanner/tests/agreement/mod.rs`:
```rust
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8 "agreement with the scanner": a hand-built capture hour, and the driver that replays
//! it through the engine (what `scanner replay` reconstructs) and tape::feed::FeedBooks side by side.
use arb_core::book::{Book, Top};
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::{CaptureMsg, Ctl, Line, UniverseRecord, Writer, universe_file_name};
use scanner::engine::{Engine, EngineCtx};
use scanner::replay::{ReplayState, apply_line};
use scanner::universe::{CanaryPair, ConnMap, GroupMeta, LegMeta, Status, Universe};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tape::feed::FeedBooks;
use tape::universe_view::load_record;

pub const T0: i64 = 1_790_121_600_000;
pub const TOKENS: [&str; 11] = ["a1", "a2", "a3", "b1", "b2", "cy", "cn", "d1", "d2", "c8y", "c8n"];

pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tape-agree-{name}-{}-{}", std::process::id(), scanner::now_ms()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn ctx() -> EngineCtx {
    EngineCtx {
        run_id: 0,
        source: "replay".into(),
        scanner_version: "t".into(),
        config_hash: "c".into(),
        gap_ms: 30_000,
        stale_lag_ms: 500,
        suspect_window_ms: 60_000,
        l1_suspect_window_ms: 3_600_000,
        drift_settle_ms: 2_000,
        issue_throttle_ms: 5_000,
    }
}

fn leg(t: &str, status: Status) -> LegMeta {
    LegMeta {
        market_id: format!("m-{t}"),
        yes_token: t.into(),
        no_token: format!("n-{t}"),
        status,
        fee_rate: Decimal::ZERO,
        fee_exponent: 1,
        tick: Some(dec!(0.01)),
        min_size: Some(dec!(5)),
        seconds_delay: Some(0),
        end_date: None,
    }
}

fn group(e: &str, legs: Vec<LegMeta>) -> GroupMeta {
    GroupMeta { event_id: e.into(), slug: None, title: None, neg_risk_market_id: Some(format!("nr-{e}")), augmented: false, legs, tags: None }
}

fn canary(m: &str, e: &str, y: &str, n: &str) -> CanaryPair {
    CanaryPair { market_id: m.into(), event_id: e.into(), yes_token: y.into(), no_token: n.into() }
}

/// U1: e1 (a1, a2, a3 halted), e2 (b1, b2), canary m9.
fn universe1() -> Universe {
    Universe {
        taken_ms: T0,
        groups: vec![
            group("e1", vec![leg("a1", Status::Live), leg("a2", Status::Live), leg("a3", Status::Halted)]),
            group("e2", vec![leg("b1", Status::Live), leg("b2", Status::Live)]),
        ],
        canary: vec![canary("m9", "e9", "cy", "cn")],
    }
}

/// U2: e1 loses its connection and a2 (a3 goes live); a2 moves to e2; b2 halts; a new group e3
/// (d1, d2) and a new canary pair m8 appear. Groups stay sorted by event id (the engine searches them).
fn universe2() -> Universe {
    Universe {
        taken_ms: T0 + 600_000,
        groups: vec![
            group("e1", vec![leg("a1", Status::Live), leg("a3", Status::Live)]),
            group("e2", vec![leg("b1", Status::Live), leg("b2", Status::Halted), leg("a2", Status::Live)]),
            group("e3", vec![leg("d1", Status::Live), leg("d2", Status::Live)]),
        ],
        canary: vec![canary("m9", "e9", "cy", "cn"), canary("m8", "e8", "c8y", "c8n")],
    }
}

fn line(t: i64, shard: usize, raw: String) -> CaptureMsg {
    CaptureMsg::Line(Line { recv_ts_us: t * 1000, shard, raw: Some(raw), ctl: None })
}

fn ctl(t: i64, shard: usize, c: Ctl) -> CaptureMsg {
    CaptureMsg::Line(Line { recv_ts_us: t * 1000, shard, raw: None, ctl: Some(c) })
}

type Side<'a> = &'a [(&'a str, &'a str)];

fn books(items: &[(&str, i64, Side, Side)]) -> String {
    let lv = |v: Side| v.iter().map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#)).collect::<Vec<_>>().join(",");
    let one: Vec<String> = items
        .iter()
        .map(|(t, ts, b, a)| format!(r#"{{"event_type":"book","asset_id":"{t}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#, lv(b), lv(a)))
        .collect();
    format!("[{}]", one.join(","))
}

/// One price_change message: (token, BUY|SELL, price, size, best_bid, best_ask).
fn change(ts: i64, items: &[(&str, &str, &str, &str, Option<&str>, Option<&str>)]) -> String {
    let one: Vec<String> = items
        .iter()
        .map(|(t, side, p, s, bb, ba)| {
            let mut x = format!(r#"{{"asset_id":"{t}","side":"{side}","price":"{p}","size":"{s}""#);
            if let Some(b) = bb {
                x.push_str(&format!(r#","best_bid":"{b}""#));
            }
            if let Some(a) = ba {
                x.push_str(&format!(r#","best_ask":"{a}""#));
            }
            x.push('}');
            x
        })
        .collect();
    format!(r#"{{"event_type":"price_change","market":"0x","timestamp":"{ts}","price_changes":[{}]}}"#, one.join(","))
}

fn record(u: &Universe, conn: &[(&str, usize)], seq: u64) -> UniverseRecord {
    UniverseRecord {
        universe: u.clone(),
        conn_of: conn.iter().map(|(k, v)| (k.to_string(), *v)).collect::<ConnMap>(),
        file: universe_file_name(u.taken_ms, seq),
    }
}

/// Write the hour (and its two universe records) into `dir`.
pub fn fixture(dir: &Path) {
    let r1 = record(&universe1(), &[("e1", 0), ("e2", 1), ("canary:m9", 1)], 1);
    let r2 = record(&universe2(), &[("e2", 1), ("e3", 2), ("canary:m9", 1), ("canary:m8", 2)], 2);
    let mut m = vec![
        CaptureMsg::Universe(Box::new(r1.clone())),
        ctl(T0 + 1_000, 0, Ctl::Universe { taken_ms: T0, file: r1.file.clone() }),
        line(T0 + 2_000, 0, books(&[("a1", T0 + 1_990, &[("0.40", "10"), ("0.39", "5")], &[("0.45", "10")]), ("a2", T0 + 1_990, &[("0.20", "3")], &[("0.30", "7")])])),
        line(T0 + 2_100, 1, books(&[("b1", T0 + 2_090, &[("0.50", "1")], &[("0.52", "2")]), ("b2", T0 + 2_090, &[], &[("0.60", "4")]), ("cy", T0 + 2_090, &[("0.10", "9")], &[("0.12", "9")]), ("cn", T0 + 2_090, &[("0.88", "9")], &[("0.90", "9")])])),
        line(T0 + 3_000, 0, change(T0 + 2_995, &[("a1", "BUY", "0.41", "2", Some("0.41"), Some("0.45"))])),
        line(T0 + 3_100, 0, change(T0 + 3_095, &[("a1", "SELL", "0.44", "3", Some("0.41"), Some("0.46"))])),
        line(T0 + 3_101, 0, change(T0 + 3_095, &[("a1", "SELL", "0.46", "1", Some("0.41"), Some("0.44"))])),
        line(T0 + 5_000, 0, change(T0 + 4_995, &[("a2", "BUY", "0.21", "1", Some("0.21"), Some("0.30"))])),
        line(T0 + 5_100, 1, format!(r#"{{"event_type":"last_trade_price","asset_id":"zz","timestamp":"{}"}}"#, T0 + 9_000)),
        line(T0 + 5_200, 1, change(T0 + 5_195, &[("b1", "SELL", "0.51", "1", Some("0.50"), Some("0.55"))])),
        ctl(T0 + 5_300, 0, Ctl::RestSnapshot { token: "b1".into(), server_ts_ms: T0 + 100, bids: vec![], asks: vec![("0.70".into(), "1".into())] }),
        ctl(T0 + 5_400, 0, Ctl::RestSnapshot { token: "cy".into(), server_ts_ms: T0 + 5_390, bids: vec![("0.11".into(), "1".into())], asks: vec![] }),
        ctl(T0 + 6_000, 1, Ctl::Down),
        ctl(T0 + 7_000, 0, Ctl::Suspect { event_id: "e2".into(), until_ms: T0 + 67_000 }),
        ctl(T0 + 8_000, 0, Ctl::Resolved { market_id: "m-b2".into(), payout_yes: "1".into(), payout_no: "0".into() }),
        line(T0 + 9_000, 1, change(T0 + 8_995, &[("b1", "BUY", "0.49", "2", Some("0.50"), None)])),
        CaptureMsg::Universe(Box::new(r2.clone())),
        ctl(T0 + 600_000, 0, Ctl::Universe { taken_ms: T0 + 600_000, file: r2.file.clone() }),
        line(T0 + 601_000, 0, change(T0 + 600_995, &[("a1", "BUY", "0.42", "1", Some("0.42"), Some("0.47")), ("a3", "SELL", "0.80", "2", Some("0"), Some("0.80"))])),
        line(T0 + 603_000, 0, change(T0 + 602_995, &[("a1", "BUY", "0.43", "1", Some("0.43"), Some("0.48"))])),
        line(T0 + 604_000, 2, books(&[("d1", T0 + 603_990, &[("0.33", "4")], &[("0.35", "4")]), ("d2", T0 + 603_990, &[("0.60", "1")], &[]), ("c8y", T0 + 603_990, &[("0.05", "2")], &[("0.07", "2")]), ("c8n", T0 + 603_990, &[("0.93", "2")], &[("0.95", "2")])])),
        line(T0 + 605_000, 1, change(T0 + 604_995, &[("a2", "BUY", "0.22", "1", Some("0.22"), Some("0.31"))])), // a2 now in e2, book carried
        line(T0 + 606_000, 2, change(T0 + 605_995, &[("d1", "SELL", "0.34", "1", Some("0.33"), Some("0.36"))])),
    ];
    // 1,500 random deltas: an LCG picks token, side, price, size and whether the server top agrees.
    let mut x: u64 = 0x5eed;
    let mut next = |n: u64| {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (x >> 33) % n
    };
    let pool = [("a1", 0usize), ("a2", 1), ("b1", 1), ("cy", 1), ("cn", 1), ("d1", 2), ("c8y", 2)];
    let mut t = T0 + 700_000;
    let mut ts = t;
    for _ in 0..1_500 {
        t += i64::try_from(next(1_500)).unwrap();
        if next(4) != 0 {
            ts = t - 5; // else: same server ts as the previous message (a split update)
        }
        let (tok, conn) = pool[usize::try_from(next(7)).unwrap()];
        let side = if next(2) == 0 { "BUY" } else { "SELL" };
        let price = format!("0.{:02}", 1 + next(98));
        let size = next(40).to_string();
        let bb = match next(3) { 0 => None, 1 => Some("0".to_string()), _ => Some(format!("0.{:02}", 1 + next(98))) };
        let ba = match next(3) { 0 => None, 1 => Some("1".to_string()), _ => Some(format!("0.{:02}", 1 + next(98))) };
        m.push(line(t, conn, change(ts, &[(tok, side, &price, &size, bb.as_deref(), ba.as_deref())])));
    }
    m.push(ctl(t + 1, 0, Ctl::Shutdown));
    let mut w = Writer::new(dir.to_path_buf());
    for msg in &m {
        w.write(msg).unwrap();
    }
    w.finish();
}

type Lv = (Vec<(Price, Shares)>, Vec<(Price, Shares)>);

fn lv(b: Option<&Book>) -> Option<Lv> {
    b.map(|b| (b.bids_best_first(), b.asks_best_first()))
}

/// Drive the engine and FeedBooks over every hour file of `dir`, comparing `tokens` after every
/// line (None: the tokens each line touched, then every visible token at the end), and the
/// top-of-book change streams. Returns the number of lines compared.
pub fn compare(dir: &Path, tokens: Option<&[&str]>) -> usize {
    let mut engine = Engine::new(ctx());
    let mut st = ReplayState::default();
    let mut feed = FeedBooks::new();
    let mut load = |taken: i64, file: &str| load_record(dir, taken, file);
    let mut engine_tops: BTreeMap<String, Vec<(usize, Top)>> = BTreeMap::new();
    let mut feed_tops: BTreeMap<String, Vec<(usize, Top)>> = BTreeMap::new();
    let mut n = 0usize;
    for h in tape::files::listing(dir).hours {
        tape::files::for_each_line(&h.path, |l| {
            apply_line(&mut engine, dir, &l, &mut st, &mut |_| {})?;
            let ap = feed.apply(&l, &mut load, &|_| false);
            let check: Vec<String> = match tokens {
                Some(t) => t.iter().map(|s| s.to_string()).collect(),
                None => ap.touched.clone(),
            };
            for t in &check {
                assert_eq!(lv(engine.book_of(t)), lv(feed.book(t)), "line {n}: books differ for {t}");
                for (tops, top) in [
                    (&mut engine_tops, engine.book_of(t).map(|b| b.top())),
                    (&mut feed_tops, feed.book(t).map(|b| b.top())),
                ] {
                    let s = tops.entry(t.clone()).or_default();
                    if let Some(top) = top
                        && s.last().map(|(_, x)| *x) != Some(top)
                    {
                        s.push((n, top));
                    }
                }
            }
            n += 1;
            Ok(())
        })
        .unwrap();
    }
    for t in feed.visible_tokens() {
        assert_eq!(lv(engine.book_of(&t)), lv(feed.book(&t)), "end: books differ for {t}");
    }
    assert_eq!(engine_tops, feed_tops, "top-of-book change streams differ");
    n
}
```
`bins/scanner/tests/tape_agreement.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8 "agreement with the scanner": FeedBooks' books equal the engine's after every line.
mod agreement;
use agreement::*;
use std::path::Path;

#[test]
fn feed_books_equal_the_engines_after_every_line() {
    let dir = tmp("fixture");
    fixture(&dir);
    let n = compare(&dir, Some(&TOKENS));
    assert!(n > 1_500, "{n} lines compared");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore]
fn feed_books_equal_the_engines_on_a_real_capture() {
    let Ok(dir) = std::env::var("TAPE_AGREEMENT_DIR") else {
        eprintln!("SKIPPED: TAPE_AGREEMENT_DIR not set");
        return;
    };
    let n = compare(Path::new(&dir), None);
    eprintln!("tape agreement: {n} lines, books equal after every one");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p scanner --test tape_agreement && cargo clippy -p scanner --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS (more than 1,500 lines compared). A failure is a difference in `FeedBooks`, never in the engine: fix `crates/tape/src/feed.rs` to match `bins/scanner/src/engine.rs` (this plan never changes engine behaviour), and add the case that failed to `crates/tape/tests/feed.rs`.

- [ ] **Step 3: Commit**

```bash
git add bins/scanner/tests/agreement/mod.rs bins/scanner/tests/tape_agreement.rs
git commit -m "scanner: agreement test, FeedBooks equal to the engine after every line"
```

---

### Task 5: store: migration 0008 (`compact_hours`, `compact_status`, `compact_rw`) and the compact reads/writes

**Files:**
- Create: `crates/store/migrations/0008_compact.sql`, `crates/store/src/compact_io.rs`, `crates/store/tests/compact_io.rs`
- Modify: `crates/store/src/lib.rs`, `crates/store/tests/migrate.rs`

**Interfaces:**
- Produces (crate `store`):
  - `COMPACT_TABLES: [&str; 2]`, `MAX_COMPACT_EPISODES: i64 = 100_000`, `MAX_COMPACT_HOURS: i64 = 10_000`.
  - `CompactEpisode { episode_id: String, event_id: String, opened_ms: i64, closed_ms: Option<i64> }`.
  - `CompactHourRow { hour_ms: i64, status: String, source_file: Option<String>, source_bytes: Option<i64>, source_sha256: Option<String>, rows: Option<String> /* JSON text */, done_ms: i64 }`.
  - `CompactStatusRow { updated_ms: i64, archive_bytes: i64, oldest_day: Option<String> /* YYYY-MM-DD */, lag_hours: i32, last_error: Option<String> }`.
  - `Store::connect_compact(PgConnectOptions, max_conns: u32, statement_timeout_ms: u32, role: Option<&str>) -> Result<Store, StoreError>`.
  - `Store::role_writes_outside_compact(&self) -> Result<Vec<String>, StoreError>`.
  - `Store::compact_episodes(&self, from_ms: i64, to_ms: i64) -> Result<Vec<CompactEpisode>, StoreError>` (live yes/no baskets with `peak_pnl_net_usd > 0`, `opened_server_ms` in `[from, to)`, ordered by open then id; overflow of the cap is an error).
  - `Store::compact_hour_statuses(&self, from_ms: i64, to_ms: i64) -> Result<Vec<(i64, String)>, StoreError>`.
  - `Store::compact_hour_put(&self, &CompactHourRow) -> Result<(), StoreError>` (upsert: the compactor's word is final).
  - `Store::mark_hours_lost(&self, hours: &[i64], now_ms: i64) -> Result<u64, StoreError>` (`ON CONFLICT DO NOTHING`).
  - `Store::compact_status_put(&self, &CompactStatusRow) -> Result<(), StoreError>`, `Store::compact_status(&self) -> Result<Option<CompactStatusRow>, StoreError>` (the api; the scanner's rollback rule, Task 6), `Store::compact_first_done_ms(&self) -> Result<Option<i64>, StoreError>` (when the first hour was recorded; the dashboard's "never reported" heart, Task 19).

- [ ] **Step 1: Write the failing tests**

In `crates/store/tests/migrate.rs` `migrations_apply_from_empty_and_are_idempotent`, add `"compact_hours",` and `"compact_status",` to the table list (after `"card_results",`).

`crates/store/tests/compact_io.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

const H: i64 = 3_600_000;
const T0: i64 = 1_790_121_600_000;

fn ep(id: &str, run_id: i64, source: &str, kind: &str, opened: i64, closed: Option<i64>, pnl: rust_decimal::Decimal) -> EpisodeRow {
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
        .start_run(&NewRun { service: "scanner".into(), started_ms: T0, version: "t".into(), config_hash: "c".into(), mode: mode.into() })
        .await
        .unwrap()
}

async fn compact(db: &common::TestDb) -> Store {
    Store::connect_compact(db.opts.clone(), 2, 10_000, Some("compact_rw")).await.unwrap()
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
    let Some((owner, db)) = common::fresh().await else { return };
    let c = compact(&db).await;
    assert_eq!(c.role_writes_outside_compact().await.unwrap(), Vec::<String>::new(), "compact_rw writes only compact tables");
    assert!(c.compact_episodes(0, i64::MAX).await.unwrap().is_empty(), "compact_rw reads episodes");
    c.raw_exec("SELECT 1 FROM runs LIMIT 1").await.expect("compact_rw reads runs");
    c.compact_hour_put(&done(T0)).await.unwrap();
    let scanner = db.role("scanner_rw").await;
    assert_eq!(scanner.compact_hour_statuses(T0, T0 + H).await.unwrap(), vec![(T0, "done".to_string())]);
    assert_eq!(scanner.mark_hours_lost(&[T0 + H], T0).await.unwrap(), 1);
    let upd = scanner.compact_hour_put(&done(T0 + H)).await.unwrap_err();
    assert!(upd.to_string().contains("permission denied"), "scanner_rw cannot update compact_hours: {upd}");
    assert_eq!(scanner.compact_status().await.unwrap(), None, "scanner_rw reads compact_status (rollback rule)");
    let api = db.role("analyze_ro").await;
    assert_eq!(api.compact_status().await.unwrap(), None, "analyze_ro reads compact_status");
    assert_eq!(api.compact_first_done_ms().await.unwrap(), Some(T0), "analyze_ro reads compact_hours");
    let analyze = Store::connect_analyze(db.opts.clone(), 2, 10_000, Some("analyze_rw")).await.unwrap();
    assert_eq!(analyze.role_writes_outside_cards().await.unwrap(), Vec::<String>::new(), "analyze_rw unchanged");
    assert!(owner.role_writes_outside_compact().await.unwrap().contains(&"episodes".to_string()));
    for s in [c, scanner, api, analyze] {
        s.close().await;
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn mark_hours_lost_never_downgrades_a_done_hour() {
    let Some((owner, db)) = common::fresh().await else { return };
    let c = compact(&db).await;
    c.compact_hour_put(&done(T0)).await.unwrap();
    assert_eq!(owner.mark_hours_lost(&[T0, T0 + H], T0 + 3 * H).await.unwrap(), 1, "only the unknown hour is inserted");
    assert_eq!(owner.compact_first_done_ms().await.unwrap(), Some(T0 + 2 * H), "the earliest done_ms");
    assert_eq!(
        owner.compact_hour_statuses(T0, T0 + 5 * H).await.unwrap(),
        vec![(T0, "done".to_string()), (T0 + H, "lost".to_string())]
    );
    // the compactor may still finish a lost hour it had read: its word is final
    c.compact_hour_put(&done(T0 + H)).await.unwrap();
    assert_eq!(owner.compact_hour_statuses(T0 + H, T0 + 2 * H).await.unwrap(), vec![(T0 + H, "done".to_string())]);
    c.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn compact_episodes_are_live_net_positive_baskets_opened_in_range() {
    let Some((owner, db)) = common::fresh().await else { return };
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    owner
        .write_batch(&Batch {
            episodes: [
                ep("a", live, "live", "no_basket", T0 + 10, Some(T0 + 900), dec!(0.41)),
                ep("b", live, "live", "yes_basket", T0 + 20, None, dec!(0.01)),
                ep("c", live, "live", "no_basket", T0 + 30, None, dec!(0)),
                ep("d", live, "live", "yes_basket_incomplete", T0 + 40, None, dec!(3)),
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
            CompactEpisode { episode_id: "a".into(), event_id: "ev1".into(), opened_ms: T0 + 10, closed_ms: Some(T0 + 900) },
            CompactEpisode { episode_id: "b".into(), event_id: "ev1".into(), opened_ms: T0 + 20, closed_ms: None },
        ]
    );
    c.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn compact_status_is_one_row_that_round_trips() {
    let Some((owner, db)) = common::fresh().await else { return };
    let c = compact(&db).await;
    assert_eq!(owner.compact_first_done_ms().await.unwrap(), None, "no hour recorded yet");
    let s = CompactStatusRow { updated_ms: T0, archive_bytes: 12_345, oldest_day: Some("2026-09-23".into()), lag_hours: 1, last_error: None };
    c.compact_status_put(&s).await.unwrap();
    let s2 = CompactStatusRow { updated_ms: T0 + H, archive_bytes: 99, oldest_day: None, lag_hours: 4, last_error: Some("read episodes: timeout".into()) };
    c.compact_status_put(&s2).await.unwrap();
    assert_eq!(owner.compact_status().await.unwrap(), Some(s2));
    c.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run them to see them fail**

Run (Postgres prefix): `cargo test -p store --test compact_io`
Expected: FAIL to compile (no `connect_compact`, no `CompactHourRow`).

- [ ] **Step 3: Write the migration**

`crates/store/migrations/0008_compact.sql`:
```sql
-- Phase 5b (spec 2026-09-27-phase5b-tiered-storage-design.md §4.3, §5, §9): compaction progress,
-- the archive status, and the compactor's role. Runs as the owner (ct_owner).

-- 1. One row per raw hour the compactor finished ('done') or that was deleted before it could
--    ('lost': the scanner at its disk cap, or the compactor passing over a marked hour).
CREATE TABLE compact_hours (
    hour_ms        BIGINT PRIMARY KEY,
    status         TEXT   NOT NULL CHECK (status IN ('done', 'lost')),
    source_file    TEXT   NULL,
    source_bytes   BIGINT NULL,
    source_sha256  TEXT   NULL,
    rows           JSONB  NULL,
    done_ms        BIGINT NOT NULL
);

-- 2. The single archive status row the api shows on Health.
CREATE TABLE compact_status (
    id             SMALLINT PRIMARY KEY CHECK (id = 1),
    updated_ms     BIGINT   NOT NULL,
    archive_bytes  BIGINT   NOT NULL,
    oldest_day     DATE     NULL,
    lag_hours      INTEGER  NOT NULL,
    last_error     TEXT     NULL
);

-- 3. compact_rw: NOLOGIN here (a migration can't carry a secret); the runbook sets LOGIN PASSWORD
--    once and fresh volumes get it from docker/postgres/init/01-roles.sh. Roles are cluster-wide:
--    tolerate one that already exists.
DO $$ BEGIN
    CREATE ROLE compact_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
DO $$ BEGIN
    EXECUTE format('ALTER ROLE compact_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '10s');
END $$;
GRANT USAGE ON SCHEMA public TO compact_rw;
GRANT SELECT ON episodes, runs TO compact_rw;
-- SELECT too: INSERT … ON CONFLICT DO UPDATE reads the row it updates, and the compactor reads
-- which hours are lost (plan ruling R7).
GRANT SELECT, INSERT, UPDATE ON compact_hours, compact_status TO compact_rw;

-- 4. The scanner's delete guard reads statuses and inserts 'lost' at the hard cap (never UPDATE),
--    and reads compact_status to wait only for a compactor that is alive (rollback, spec §9.5).
GRANT SELECT, INSERT ON compact_hours TO scanner_rw;
GRANT SELECT ON compact_status TO scanner_rw;

-- 5. The dashboard api reads the status row and when the first hour was recorded. 0001's default
--    privileges already give analyze_ro SELECT on every new table; spelled out for the two it uses.
GRANT SELECT ON compact_status, compact_hours TO analyze_ro;
```

- [ ] **Step 4: Write the store functions**

`crates/store/src/compact_io.rs`:
```rust
//! Phase 5b: the compactor's progress (compact_hours) and status (compact_status), the episodes
//! that get l2 windows, and the scanner's delete-guard reads. JSON travels as text (`$n::jsonb` in),
//! so `store` needs no JSON dependency. Every read is parameterized and capped; a capped read that
//! overflows is an error, never a truncation.
use crate::text::{clean, clean_opt};
use crate::{Store, StoreError};
use sqlx::postgres::PgConnectOptions;

pub const COMPACT_TABLES: [&str; 2] = ["compact_hours", "compact_status"];
/// Episodes one hour's l2 windows can need: ~20 net-positive arbitrage episodes an hour in practice.
pub const MAX_COMPACT_EPISODES: i64 = 100_000;
/// compact_hours rows one read can return (14 days of hours is 336).
pub const MAX_COMPACT_HOURS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CompactEpisode {
    pub episode_id: String,
    pub event_id: String,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHourRow {
    pub hour_ms: i64,
    /// 'done' or 'lost'.
    pub status: String,
    pub source_file: Option<String>,
    pub source_bytes: Option<i64>,
    pub source_sha256: Option<String>,
    /// Row counts per table as JSON text.
    pub rows: Option<String>,
    pub done_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactStatusRow {
    pub updated_ms: i64,
    pub archive_bytes: i64,
    /// YYYY-MM-DD, the oldest day still in the archive.
    pub oldest_day: Option<String>,
    pub lag_hours: i32,
    pub last_error: Option<String>,
}

impl Store {
    /// The compactor's pool: every session carries its own statement timeout (on top of the role's
    /// 10 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_compact(
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

    /// Tables outside compact_hours/compact_status this role could INSERT, UPDATE, DELETE or
    /// TRUNCATE (compact refuses to start unless this is empty).
    pub async fn role_writes_outside_compact(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT (c.relname::text = ANY($1)) \
               AND (has_table_privilege(current_user, c.oid, 'INSERT') \
                 OR has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE')) \
             ORDER BY 1",
        )
        .bind(COMPACT_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Live net-positive YES/NO-basket episodes opened in [from_ms, to_ms): the ones that get an
    /// l2 window (spec 5b §3). Uses the `episodes_opened` index.
    pub async fn compact_episodes(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<CompactEpisode>, StoreError> {
        let rows: Vec<CompactEpisode> = sqlx::query_as(
            "SELECT episode_id, event_id, opened_server_ms AS opened_ms, closed_server_ms AS closed_ms \
             FROM episodes WHERE source = 'live' AND kind IN ('yes_basket', 'no_basket') \
               AND peak_pnl_net_usd > 0 AND opened_server_ms >= $1 AND opened_server_ms < $2 \
             ORDER BY opened_server_ms, episode_id LIMIT $3",
        )
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_COMPACT_EPISODES + 1)
        .fetch_all(self.pool())
        .await?;
        if rows.len() > usize::try_from(MAX_COMPACT_EPISODES).unwrap_or(usize::MAX) {
            return Err(StoreError::TooManyRows("compact_episodes"));
        }
        Ok(rows)
    }

    /// (hour_ms, status) for hours in [from_ms, to_ms), oldest first.
    pub async fn compact_hour_statuses(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<(i64, String)>, StoreError> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT hour_ms, status FROM compact_hours WHERE hour_ms >= $1 AND hour_ms < $2 \
             ORDER BY hour_ms LIMIT $3",
        )
        .bind(from_ms)
        .bind(to_ms)
        .bind(MAX_COMPACT_HOURS + 1)
        .fetch_all(self.pool())
        .await?;
        if rows.len() > usize::try_from(MAX_COMPACT_HOURS).unwrap_or(usize::MAX) {
            return Err(StoreError::TooManyRows("compact_hour_statuses"));
        }
        Ok(rows)
    }

    /// The compactor records an hour. Its word is final: a 'lost' row the scanner inserted while
    /// the compactor was still reading the (already open) file becomes 'done'.
    pub async fn compact_hour_put(&self, r: &CompactHourRow) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO compact_hours (hour_ms, status, source_file, source_bytes, source_sha256, rows, done_ms) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7) \
             ON CONFLICT (hour_ms) DO UPDATE SET status = EXCLUDED.status, source_file = EXCLUDED.source_file, \
               source_bytes = EXCLUDED.source_bytes, source_sha256 = EXCLUDED.source_sha256, \
               rows = EXCLUDED.rows, done_ms = EXCLUDED.done_ms",
        )
        .bind(r.hour_ms)
        .bind(clean(&r.status))
        .bind(clean_opt(&r.source_file))
        .bind(r.source_bytes)
        .bind(clean_opt(&r.source_sha256))
        .bind(r.rows.as_deref())
        .bind(r.done_ms)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// The scanner deleted these raw hours: mark the ones nobody recorded as 'lost'. A 'done' hour
    /// is never downgraded (spec 5b §4.3). Returns the rows inserted.
    pub async fn mark_hours_lost(&self, hours: &[i64], now_ms: i64) -> Result<u64, StoreError> {
        Ok(sqlx::query(
            "INSERT INTO compact_hours (hour_ms, status, done_ms) \
             SELECT h, 'lost', $2 FROM unnest($1::bigint[]) AS h \
             ON CONFLICT (hour_ms) DO NOTHING",
        )
        .bind(hours)
        .bind(now_ms)
        .execute(self.pool())
        .await?
        .rows_affected())
    }

    pub async fn compact_status_put(&self, s: &CompactStatusRow) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO compact_status (id, updated_ms, archive_bytes, oldest_day, lag_hours, last_error) \
             VALUES (1, $1, $2, $3::date, $4, $5) \
             ON CONFLICT (id) DO UPDATE SET updated_ms = EXCLUDED.updated_ms, \
               archive_bytes = EXCLUDED.archive_bytes, oldest_day = EXCLUDED.oldest_day, \
               lag_hours = EXCLUDED.lag_hours, last_error = EXCLUDED.last_error",
        )
        .bind(s.updated_ms)
        .bind(s.archive_bytes)
        .bind(s.oldest_day.as_deref())
        .bind(s.lag_hours)
        .bind(clean_opt(&s.last_error))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn compact_status(&self) -> Result<Option<CompactStatusRow>, StoreError> {
        let row: Option<(i64, i64, Option<String>, i32, Option<String>)> = sqlx::query_as(
            "SELECT updated_ms, archive_bytes, oldest_day::text, lag_hours, last_error \
             FROM compact_status WHERE id = 1",
        )
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(
            |(updated_ms, archive_bytes, oldest_day, lag_hours, last_error)| CompactStatusRow {
                updated_ms,
                archive_bytes,
                oldest_day,
                lag_hours,
                last_error,
            },
        ))
    }

    /// When the first hour was recorded (done or lost), if any.
    pub async fn compact_first_done_ms(&self) -> Result<Option<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT min(done_ms) FROM compact_hours")
                .fetch_one(self.pool())
                .await?,
        )
    }
}
```
In `crates/store/src/lib.rs`: add `mod compact_io;` (alphabetical, after `mod card_io;`) and
```rust
pub use compact_io::{
    COMPACT_TABLES, CompactEpisode, CompactHourRow, CompactStatusRow, MAX_COMPACT_EPISODES,
    MAX_COMPACT_HOURS,
};
```
If `text::clean_opt` is not yet `pub(crate)` used anywhere, it already is (`crates/store/src/text.rs`); no change there.

- [ ] **Step 5: Run the store tests**

Run (Postgres prefix): `cargo test -p store`
Expected: PASS: the four new tests, `migrations_apply_from_empty_and_are_idempotent` (0008 applied twice), and every 0006/0007 role test unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/store/migrations/0008_compact.sql crates/store/src/compact_io.rs crates/store/src/lib.rs crates/store/tests/compact_io.rs crates/store/tests/migrate.rs
git commit -m "store: migration 0008 (compact_hours, compact_status, compact_rw) and the compact reads/writes"
```

---

### Task 6: scanner: the delete guard, 24 h retention, 15 GB cap, and the rollback rule

Spec §4.3, §9.5. The capture thread (a plain thread) can't await the database, so a small tokio task (`guard_task`) keeps a view of `compact_hours` in a shared `GuardView` and inserts the `lost` marks the thread sends it. It inserts each mark **as soon as it arrives** (the 30 s tick only retries failures), so the compactor never sees a deleted hour before its mark (Review Focus 2; Task 18 closes the other side). The thread runs one pure-planned, testable housekeeping pass (`housekeep`). The guard only waits for a compactor that is alive (ruling R10, owner-approved rollback): with no `compact_status` row, or one older than 6 h, the view is cleared and retention is today's rule (age plus cap) with the new numbers; a view older than 2 minutes (the database unreachable) does the same.

**Files:**
- Create: `bins/scanner/src/retention.rs`, `bins/scanner/tests/retention.rs`
- Modify: `bins/scanner/src/lib.rs`, `bins/scanner/src/capture.rs` (`spawn`), `bins/scanner/src/run.rs` (wiring), `config/scanner.toml`

**Interfaces:**
- Consumes: Task 1 (`tape::files::{listing, hour_path, HourFile}`, `tape::time::{HOUR_MS, hour_label}`), Task 5 (`Store::compact_hour_statuses`, `Store::mark_hours_lost`, `Store::compact_status`).
- Produces (`scanner::retention`): `VIEW_FRESH_MS = 120_000`, `REFRESH_MS = 30_000`, `LOOKBACK_MS`, `COMPACTOR_STALE_MS = 6 h`, `Deletion { path, hour_ms, by_cap: bool, uncompacted: bool }`, `plan(hours: &[HourFile], universe_bytes: u64, now_ms: i64, retention_hours: i64, max_bytes: u64, keep: Option<&Path>, compacted: Option<&BTreeSet<i64>>) -> Vec<Deletion>`, `compactor_alive(updated_ms: Option<i64>, now_ms: i64) -> bool`, `housekeep(dir: &Path, now_ms: i64, cfg: &CaptureCfg, view: Option<&BTreeSet<i64>>, lost: &UnboundedSender<i64>, keep: Option<&Path>) -> u64`, `GuardView` (`Default`; `set(fetched_ms, BTreeSet<i64>)`, `clear()`, `get(now_ms) -> Option<BTreeSet<i64>>`), `guard_task(Store, Arc<GuardView>, UnboundedReceiver<i64>)`. `capture::spawn(cfg, rx, metrics, guard: Arc<GuardView>, lost: UnboundedSender<i64>)`.

- [ ] **Step 1: Write the failing tests**

`bins/scanner/tests/retention.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.3: the scanner deletes a raw hour by age only once it is compacted (done or lost),
//! deletes the oldest hour at the hard cap whatever its status (and marks it lost), and falls back
//! to age + cap when it can't read compact_hours or the compactor is silent (rollback, §9.5).
use scanner::config::CaptureCfg;
use scanner::retention::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tape::files::HourFile;

const T0: i64 = 1_790_121_600_000;
const H: i64 = 3_600_000;

fn hf(i: i64, bytes: u64) -> HourFile {
    HourFile { start_ms: T0 + i * H, path: PathBuf::from(format!("/c/{i:02}.jsonl.zst")), bytes }
}

fn hours(n: i64, bytes: u64) -> Vec<HourFile> {
    (0..n).map(|i| hf(i, bytes)).collect()
}

fn deleted(d: &[Deletion]) -> Vec<i64> {
    d.iter().map(|x| (x.hour_ms - T0) / H).collect()
}

#[test]
fn the_age_rule_deletes_only_compacted_hours() {
    let now = T0 + 30 * H; // hours 0..=5 are past the 24 h retention
    let compacted: BTreeSet<i64> = [0, 1, 2, 7].iter().map(|i| T0 + i * H).collect();
    let d = plan(&hours(30, 1), 0, now, 24, u64::MAX, None, Some(&compacted));
    assert_eq!(deleted(&d), vec![0, 1, 2], "3, 4, 5 wait for the compactor; 7 is too young");
    assert!(d.iter().all(|x| !x.by_cap && !x.uncompacted));
}

#[test]
fn the_cap_deletes_the_oldest_whatever_its_status_and_flags_the_uncompacted_ones() {
    let now = T0 + 3 * H;
    let compacted: BTreeSet<i64> = [T0].into_iter().collect();
    let d = plan(&hours(3, 10), 4, now, 24, 15, None, Some(&compacted));
    assert_eq!(deleted(&d), vec![0, 1], "34 bytes → 24 → 14 ≤ 15");
    assert!(d.iter().all(|x| x.by_cap));
    assert_eq!(d.iter().map(|x| x.uncompacted).collect::<Vec<_>>(), vec![false, true]);
}

#[test]
fn without_a_view_retention_falls_back_to_age_and_cap_and_reports_every_deletion() {
    let now = T0 + 30 * H;
    let d = plan(&hours(30, 1), 0, now, 24, u64::MAX, None, None);
    assert_eq!(deleted(&d), vec![0, 1, 2, 3, 4, 5]);
    assert!(d.iter().all(|x| x.uncompacted), "each gets a lost mark; ON CONFLICT DO NOTHING keeps done rows");
}

#[test]
fn the_hour_being_written_is_never_deleted() {
    let hs = hours(2, 10);
    let keep = hs[0].path.clone();
    let d = plan(&hs, 0, T0 + 40 * H, 24, 0, Some(Path::new(&keep)), None);
    assert_eq!(deleted(&d), vec![1]);
}

#[test]
fn the_guard_view_is_fresh_for_two_minutes_only_and_can_be_cleared() {
    let v = GuardView::default();
    assert_eq!(v.get(T0), None, "never read: fall back");
    v.set(T0, [T0].into_iter().collect());
    assert_eq!(v.get(T0 + VIEW_FRESH_MS), Some([T0].into_iter().collect()));
    assert_eq!(v.get(T0 + VIEW_FRESH_MS + 1), None, "stale: fall back");
    v.set(T0, [T0].into_iter().collect());
    v.clear();
    assert_eq!(v.get(T0), None, "cleared (compactor silent): fall back");
}

#[test]
fn the_guard_waits_only_for_a_compactor_that_reported_within_six_hours() {
    assert_eq!(COMPACTOR_STALE_MS, 6 * H);
    assert!(!compactor_alive(None, T0), "no status row: not deployed yet, today's rule");
    assert!(compactor_alive(Some(T0), T0 + COMPACTOR_STALE_MS));
    assert!(!compactor_alive(Some(T0), T0 + COMPACTOR_STALE_MS + 1), "stopped (rollback): today's rule");
}

fn capture_dir(name: &str, n: i64) -> (PathBuf, CaptureCfg) {
    let dir = std::env::temp_dir().join(format!("scanner-hk-{name}-{}-{}", std::process::id(), scanner::now_ms()));
    for i in 0..n {
        let p = tape::files::hour_path(&dir, T0 + i * H);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![0u8; 10]).unwrap();
    }
    let cfg = CaptureCfg { dir: dir.to_string_lossy().to_string(), pause_below_gb: 10, resume_above_gb: 15, retention_hours: 24, max_capture_gb: 15 };
    (dir, cfg)
}

fn marks(rx: &mut tokio::sync::mpsc::UnboundedReceiver<i64>) -> Vec<i64> {
    let mut v = Vec::new();
    while let Ok(h) = rx.try_recv() {
        v.push((h - T0) / H);
    }
    v
}

fn left(dir: &Path) -> usize {
    tape::files::listing(dir).hours.len()
}

#[test]
fn housekeeping_marks_hours_lost_at_the_cap() {
    let (dir, mut cfg) = capture_dir("cap", 3);
    cfg.max_capture_gb = 0; // everything over the cap
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let keep = tape::files::hour_path(&dir, T0 + 2 * H);
    let view: BTreeSet<i64> = [T0].into_iter().collect(); // hour 0 is done
    let n = housekeep(&dir, T0 + 3 * H, &cfg, Some(&view), &tx, Some(&keep));
    assert_eq!(n, 2);
    assert_eq!(marks(&mut rx), vec![1], "only the uncompacted hour gets a lost mark");
    assert_eq!(left(&dir), 1, "the hour being written stays");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn housekeeping_keeps_uncompacted_old_hours_while_the_compactor_is_alive() {
    let (dir, cfg) = capture_dir("wait", 3);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let view: BTreeSet<i64> = [T0 + H].into_iter().collect();
    let n = housekeep(&dir, T0 + 40 * H, &cfg, Some(&view), &tx, None);
    assert_eq!(n, 1, "only hour 1 is done");
    assert!(marks(&mut rx).is_empty());
    assert_eq!(left(&dir), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn housekeeping_without_a_view_is_todays_rule_and_marks_every_deletion() {
    let (dir, cfg) = capture_dir("fallback", 3);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let n = housekeep(&dir, T0 + 40 * H, &cfg, None, &tx, None);
    assert_eq!(n, 3);
    assert_eq!(marks(&mut rx), vec![0, 1, 2]);
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p scanner --test retention`
Expected: FAIL (no `scanner::retention`).

- [ ] **Step 3: The retention module**

`bins/scanner/src/retention.rs`:
```rust
//! Raw-hour retention with the compaction guard (spec 5b §4.3, §9.5). `plan` is pure, `housekeep`
//! applies it to the capture folder, and `guard_task` keeps the capture thread's view of
//! compact_hours fresh and records the hours deleted before they were compacted. Capture never
//! stalls on the database or on a stopped compactor: with no fresh view, retention is today's rule
//! (age plus cap), and every deletion made blind is marked lost (a no-op for hours already done:
//! ON CONFLICT DO NOTHING).
use crate::config::CaptureCfg;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::Store;
use tape::files::HourFile;
use tape::time::HOUR_MS;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// The guard's view is trusted for this long after a successful read.
pub const VIEW_FRESH_MS: i64 = 120_000;
/// How often `guard_task` re-reads compact_hours (and retries lost marks that failed).
pub const REFRESH_MS: u64 = 30_000;
/// How far back `guard_task` reads (24 h retention and a 15 GB cap keep far less on disk).
pub const LOOKBACK_MS: i64 = 14 * 24 * HOUR_MS;
/// A compactor silent this long is treated as stopped: retention returns to today's rule (§9.5).
pub const COMPACTOR_STALE_MS: i64 = 6 * HOUR_MS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub path: PathBuf,
    pub hour_ms: i64,
    /// Deleted to get under max_capture_gb (else: past retention_hours).
    pub by_cap: bool,
    /// Not known to be compacted (done or lost): gets a lost mark.
    pub uncompacted: bool,
}

/// Which hour files to delete, oldest first. Age: past `retention_hours` and compacted (any age
/// when `compacted` is None, the fallback). Cap: while the capture folder (hours + universe
/// records) is over `max_bytes`, the oldest remaining hour, whatever its status. `keep` (the file
/// being written) is never deleted.
pub fn plan(
    hours: &[HourFile],
    universe_bytes: u64,
    now_ms: i64,
    retention_hours: i64,
    max_bytes: u64,
    keep: Option<&Path>,
    compacted: Option<&BTreeSet<i64>>,
) -> Vec<Deletion> {
    let cutoff = now_ms.saturating_sub(retention_hours.saturating_mul(HOUR_MS));
    let is_done = |h: i64| compacted.is_some_and(|s| s.contains(&h));
    let mut sorted: Vec<&HourFile> = hours.iter().collect();
    sorted.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then_with(|| a.path.cmp(&b.path)));
    let mut total = sorted
        .iter()
        .map(|h| h.bytes)
        .sum::<u64>()
        .saturating_add(universe_bytes);
    let mut out: Vec<Deletion> = Vec::new();
    for h in &sorted {
        if keep == Some(h.path.as_path()) {
            continue;
        }
        let old = h.start_ms.saturating_add(HOUR_MS) <= cutoff;
        if old && (compacted.is_none() || is_done(h.start_ms)) {
            total = total.saturating_sub(h.bytes);
            out.push(Deletion {
                path: h.path.clone(),
                hour_ms: h.start_ms,
                by_cap: false,
                uncompacted: !is_done(h.start_ms),
            });
        }
    }
    for h in &sorted {
        if total <= max_bytes {
            break;
        }
        if keep == Some(h.path.as_path()) || out.iter().any(|d| d.path == h.path) {
            continue;
        }
        total = total.saturating_sub(h.bytes);
        out.push(Deletion {
            path: h.path.clone(),
            hour_ms: h.start_ms,
            by_cap: true,
            uncompacted: !is_done(h.start_ms),
        });
    }
    out
}

/// The rollback rule (spec §9.5, owner ruling): wait for compaction only while the compactor has
/// reported within `COMPACTOR_STALE_MS`. No status row (compact not deployed yet) or a stale one
/// means today's rule.
pub fn compactor_alive(updated_ms: Option<i64>, now_ms: i64) -> bool {
    updated_ms.is_some_and(|u| now_ms.saturating_sub(u) <= COMPACTOR_STALE_MS)
}

/// One housekeeping pass over the capture folder: apply `plan`, send a lost mark for every hour
/// deleted before it was known compacted (logging the ones the cap forced), then delete universe
/// records no remaining hour can name. Returns the number of files deleted.
pub fn housekeep(
    dir: &Path,
    now_ms: i64,
    cfg: &CaptureCfg,
    view: Option<&BTreeSet<i64>>,
    lost: &UnboundedSender<i64>,
    keep: Option<&Path>,
) -> u64 {
    let listing = tape::files::listing(dir);
    let universe_bytes: u64 = listing.universes.iter().map(|u| u.bytes).sum();
    let budget = cfg.max_capture_gb.saturating_mul(1_000_000_000);
    let mut n = 0u64;
    for d in plan(
        &listing.hours,
        universe_bytes,
        now_ms,
        cfg.retention_hours,
        budget,
        keep,
        view,
    ) {
        if std::fs::remove_file(&d.path).is_ok() {
            n += 1;
            if d.uncompacted {
                let _ = lost.send(d.hour_ms);
                if d.by_cap {
                    eprintln!(
                        "capture: hour {} deleted before compaction (disk cap)",
                        tape::time::hour_label(d.hour_ms)
                    );
                }
            }
        }
    }
    for p in crate::capture::stale_universe_files(dir) {
        if std::fs::remove_file(p).is_ok() {
            n += 1;
        }
    }
    n
}

/// The capture thread's view of compact_hours (done or lost hours) and when it was read.
#[derive(Debug, Default)]
pub struct GuardView {
    inner: Mutex<Option<(i64, BTreeSet<i64>)>>,
}

impl GuardView {
    pub fn set(&self, fetched_ms: i64, hours: BTreeSet<i64>) {
        if let Ok(mut g) = self.inner.lock() {
            *g = Some((fetched_ms, hours));
        }
    }

    /// Forget the view: retention falls back to today's rule until the next `set`.
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            *g = None;
        }
    }

    /// The compacted hours, or None (fall back) when never read, cleared, or read over 2 min ago.
    pub fn get(&self, now_ms: i64) -> Option<BTreeSet<i64>> {
        let g = self.inner.lock().ok()?;
        let (t, s) = g.as_ref()?;
        (now_ms.abs_diff(*t) <= VIEW_FRESH_MS.unsigned_abs()).then(|| s.clone())
    }
}

async fn flush_lost(store: &Store, pending: &mut BTreeSet<i64>) {
    if pending.is_empty() {
        return;
    }
    let hours: Vec<i64> = pending.iter().copied().collect();
    if store.mark_hours_lost(&hours, crate::now_ms()).await.is_ok() {
        pending.clear();
    }
}

/// Records lost hours the moment the capture thread reports them (so the compactor can't pass an
/// hour before its mark, spec §4.3 / D5), and every 30 s retries failed marks and refreshes the
/// view: set while the compactor is alive, cleared (today's rule) while it is silent. Ends when the
/// capture thread drops its sender.
pub async fn guard_task(store: Store, view: Arc<GuardView>, mut lost: UnboundedReceiver<i64>) {
    let mut every = tokio::time::interval(Duration::from_millis(REFRESH_MS));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pending: BTreeSet<i64> = BTreeSet::new();
    let (mut failing, mut silent) = (false, false);
    loop {
        tokio::select! {
            _ = every.tick() => {}
            h = lost.recv() => match h {
                Some(h) => {
                    pending.insert(h);
                    while let Ok(more) = lost.try_recv() {
                        pending.insert(more);
                    }
                    flush_lost(&store, &mut pending).await;
                    continue;
                }
                None => return,
            },
        }
        flush_lost(&store, &mut pending).await;
        let now = crate::now_ms();
        let status = store.compact_status().await;
        let hours = store
            .compact_hour_statuses(now.saturating_sub(LOOKBACK_MS), now.saturating_add(HOUR_MS))
            .await;
        match (status, hours) {
            (Ok(status), Ok(rows)) => {
                if failing {
                    eprintln!("capture: compact_hours readable again");
                    failing = false;
                }
                if compactor_alive(status.map(|s| s.updated_ms), now) {
                    view.set(now, rows.into_iter().map(|(h, _)| h).collect());
                    if silent {
                        eprintln!("capture: compactor reporting; raw hours wait for compaction");
                        silent = false;
                    }
                } else {
                    view.clear();
                    if !silent {
                        eprintln!("capture: compactor silent (no compact_status within 6 h); retention falls back to age + cap");
                        silent = true;
                    }
                }
            }
            (Err(e), _) | (_, Err(e)) => {
                if !failing {
                    eprintln!("capture: compact_hours unreadable ({e}); retention falls back to age + cap");
                    failing = true;
                }
            }
        }
    }
}
```
`bins/scanner/src/lib.rs`: add `pub mod retention;` (alphabetical, after `pub mod replay;`).

- [ ] **Step 4: The capture thread uses it**

`bins/scanner/src/capture.rs` `spawn`: change the signature to
```rust
pub fn spawn(
    cfg: CaptureCfg,
    mut rx: tokio::sync::mpsc::Receiver<CaptureMsg>,
    metrics: Arc<Metrics>,
    guard: Arc<crate::retention::GuardView>,
    lost: tokio::sync::mpsc::UnboundedSender<i64>,
) -> std::thread::JoinHandle<()> {
```
and replace the three deletion loops (the `for p in expired_hours(…)` loop, the `let budget = …; for p in over_budget_hours(…)` loop and the `for p in stale_universe_files(&dir)` loop) with:
```rust
                let view = guard.get(now);
                let n = crate::retention::housekeep(
                    &dir,
                    now,
                    &cfg,
                    view.as_ref(),
                    &lost,
                    writer.current_path(),
                );
                metrics.capture_deletes.fetch_add(n, Ordering::Relaxed);
```
(`expired_hours` and `over_budget_hours` stay as public helpers, still covered by `bins/scanner/tests/replay.rs`.) Update the module doc's last sentence to: `Capture never blocks the scanner: it pauses on low disk (hysteresis), deletes hour files past the retention window once compacted (retention.rs), always deletes the oldest at the byte cap, and deletes universe records no remaining hour can reference.`

- [ ] **Step 5: Wire the guard in `run_live`**

`bins/scanner/src/run.rs` `run_live`, replace `let capture_thread = capture::spawn(cfg.capture.clone(), capture_rx, metrics.clone());` with:
```rust
    // Spec 5b §4.3: raw hours wait for the compactor (compact_hours), except at the byte cap.
    let guard_view = Arc::new(crate::retention::GuardView::default());
    let (lost_tx, lost_rx) = mpsc::unbounded_channel::<i64>();
    let guard_task = tokio::spawn(crate::retention::guard_task(
        store.clone(),
        guard_view.clone(),
        lost_rx,
    ));
    let capture_thread = capture::spawn(
        cfg.capture.clone(),
        capture_rx,
        metrics.clone(),
        guard_view,
        lost_tx,
    );
```
and in the shutdown sequence add `guard_task.abort();` on the line after `health_task.abort();`.

- [ ] **Step 6: The new numbers**

`config/scanner.toml` `[capture]`:
```toml
retention_hours = 24            # spec 5b §4.3: kept ~a day, deleted by age only once compacted (compact_hours)
max_capture_gb = 15             # hard cap: deletes the oldest hour whatever its status and marks it lost (spec 5b §4.3)
```

- [ ] **Step 7: Run the tests and the premise checks**

Run: `cargo test -p scanner && cargo test -p arb-core --test premises_lock && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS, including the nine retention tests, `bins/scanner/tests/premises.rs` (the `[capture]` section is not a premise section, spec §7) and `premises_lock` with no version bump.

- [ ] **Step 8: Commit**

```bash
git add bins/scanner/src/retention.rs bins/scanner/src/lib.rs bins/scanner/src/capture.rs bins/scanner/src/run.rs bins/scanner/tests/retention.rs config/scanner.toml
git commit -m "scanner: raw hours wait for a live compactor (delete guard), lost marks at once, 24 h / 15 GB"
```

---

### Task 7: scanner: universe records only when changed (and acknowledged); retention keeps the record in force

Spec §4.5. A refresh whose content (groups, canary pairs, connection map; not `taken_ms`, which changes every time) hashes the same as the last record written writes no new record; its `Ctl::Universe` line names the last written record (its `taken_ms` and file), so replay loads the same content. Retention then must keep the record in force at the oldest remaining hour even when it is older than that hour: ruling R8 uses the record files' write time (mtime) — every record written at or after the oldest hour's start, plus the newest one written before it, is kept.

A record handed to the capture thread may still never reach disk (capture paused on low disk, a write error). Naming it from later refreshes would break replay for hours, so the capture thread **acknowledges** every record it wrote (its sequence number in `Metrics::universe_acked_seq`), and the dedup reuses a record only when it was acknowledged, capture is not paused, and no capture message was dropped since it was written; otherwise the refresh writes a new record (ruling R16).

**Files:**
- Create: `bins/scanner/tests/universe_dedup.rs`
- Modify: `bins/scanner/src/capture.rs` (`UniverseDedup`, `UniversePlan`, `CaptureState`, `universe_hash`, `deliver`, `stale_universe_files`, the `spawn` write path), `bins/scanner/src/metrics.rs` (`universe_acked_seq`), `bins/scanner/src/run.rs` (the `apply` closure), `bins/scanner/tests/replay.rs` (the old universe-retention test)

**Interfaces:**
- Consumes: Task 6 (`housekeep` calls `stale_universe_files`).
- Produces (`scanner::capture`): `UniversePlan { Write { hash: String }, Reuse { taken_ms: i64, file: String } }`, `CaptureState { acked_seq: u64, paused: bool, drops: u64 }` (`CaptureState::of(&Metrics)`), `UniverseDedup` (`Default`; `plan(&self, &Universe, &ConnMap, CaptureState) -> UniversePlan`, `written(&mut self, hash: String, taken_ms: i64, file: String, seq: u64, drops: u64)`), `universe_hash(&Universe, &ConnMap) -> anyhow::Result<String>`, `deliver(&mut Writer, &CaptureMsg, &Metrics) -> bool`; `stale_universe_files(dir) -> Vec<PathBuf>` with the mtime rule; `Metrics::universe_acked_seq: AtomicU64`.

- [ ] **Step 1: Write the failing tests**

`bins/scanner/tests/universe_dedup.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.5: a universe record is written only when its content changed; an unchanged refresh
//! still captures a control line, naming the last record, so replay is unchanged; retention never
//! deletes the record still in force for the oldest remaining hour.
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::*;
use scanner::engine::EngineCtx;
use scanner::metrics::Metrics;
use scanner::replay::replay_dir;
use scanner::universe::{ConnMap, GroupMeta, LegMeta, Status, Universe};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, UNIX_EPOCH};

const T0: i64 = 1_790_121_600_000;
const H: i64 = 3_600_000;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("scanner-dedup-{name}-{}-{}", std::process::id(), scanner::now_ms()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ctx() -> EngineCtx {
    EngineCtx {
        run_id: 0,
        source: "replay".into(),
        scanner_version: "t".into(),
        config_hash: "c".into(),
        gap_ms: 30_000,
        stale_lag_ms: 500,
        suspect_window_ms: 60_000,
        l1_suspect_window_ms: 3_600_000,
        drift_settle_ms: 2_000,
        issue_throttle_ms: 5_000,
    }
}

fn universe(taken_ms: i64, status: Status) -> Universe {
    let legs = (0..3)
        .map(|i| LegMeta {
            market_id: format!("m{i}"),
            yes_token: format!("y{i}"),
            no_token: format!("n{i}"),
            status: if i == 2 { status } else { Status::Live },
            fee_rate: Decimal::ZERO,
            fee_exponent: 1,
            tick: Some(dec!(0.01)),
            min_size: Some(dec!(5)),
            seconds_delay: Some(0),
            end_date: None,
        })
        .collect();
    Universe {
        taken_ms,
        groups: vec![GroupMeta { event_id: "e1".into(), slug: None, title: None, neg_risk_market_id: None, augmented: false, legs, tags: None }],
        canary: vec![],
    }
}

fn conn(c: usize) -> ConnMap {
    [("e1".to_string(), c)].into_iter().collect()
}

/// Capture is running and has written everything up to record `acked_seq`.
fn ok(acked_seq: u64) -> CaptureState {
    CaptureState { acked_seq, paused: false, drops: 0 }
}

#[test]
fn an_unchanged_refresh_writes_no_record_but_names_the_last_one() {
    let mut d = UniverseDedup::default();
    let u1 = universe(T0, Status::Live);
    let UniversePlan::Write { hash } = d.plan(&u1, &conn(0), ok(0)) else { panic!("first refresh writes") };
    d.written(hash, T0, universe_file_name(T0, 1), 1, 0);
    let u2 = universe(T0 + 300_000, Status::Live); // same content, new taken_ms
    assert_eq!(d.plan(&u2, &conn(0), ok(1)), UniversePlan::Reuse { taken_ms: T0, file: universe_file_name(T0, 1) });
    assert!(matches!(d.plan(&universe(T0 + 600_000, Status::Halted), &conn(0), ok(1)), UniversePlan::Write { .. }), "content changed");
    assert!(matches!(d.plan(&u2, &conn(1), ok(1)), UniversePlan::Write { .. }), "connection map changed");
    assert_ne!(universe_hash(&u1, &conn(0)).unwrap(), universe_hash(&u1, &conn(1)).unwrap());
    assert_eq!(universe_hash(&u1, &conn(0)).unwrap(), universe_hash(&u2, &conn(0)).unwrap());
}

#[test]
fn a_record_capture_never_wrote_is_written_again_once_capture_resumes() {
    let mut d = UniverseDedup::default();
    let (u1, u2, u3) = (universe(T0, Status::Live), universe(T0 + 300_000, Status::Live), universe(T0 + 600_000, Status::Live));
    let UniversePlan::Write { hash } = d.plan(&u1, &conn(0), ok(0)) else { panic!("first refresh writes") };
    d.written(hash, T0, universe_file_name(T0, 7), 7, 0);
    // capture paused on low disk: record 7 was dropped and never acknowledged
    let paused = CaptureState { acked_seq: 0, paused: true, drops: 1 };
    assert!(matches!(d.plan(&u2, &conn(0), paused), UniversePlan::Write { .. }), "never name a record while paused");
    // resumed, but record 7 never reached disk: write a new one
    let resumed = CaptureState { acked_seq: 0, paused: false, drops: 1 };
    let UniversePlan::Write { hash } = d.plan(&u2, &conn(0), resumed) else { panic!("unacknowledged: write again") };
    d.written(hash, T0 + 300_000, universe_file_name(T0 + 300_000, 8), 8, 1);
    // record 8 acknowledged: the next unchanged refresh names it
    let acked = CaptureState { acked_seq: 8, paused: false, drops: 1 };
    assert_eq!(d.plan(&u3, &conn(0), acked), UniversePlan::Reuse { taken_ms: T0 + 300_000, file: universe_file_name(T0 + 300_000, 8) });
    // a message dropped since record 8 was written: write again (the drop may have been its line)
    let dropped = CaptureState { acked_seq: 8, paused: false, drops: 2 };
    assert!(matches!(d.plan(&u3, &conn(0), dropped), UniversePlan::Write { .. }));
}

#[test]
fn deliver_acknowledges_only_records_that_reach_disk() {
    let dir = tmp("deliver");
    let m = Metrics::default();
    let mut w = Writer::new(dir.clone());
    let file = universe_file_name(T0, 42);
    let rec = CaptureMsg::Universe(Box::new(UniverseRecord { universe: universe(T0, Status::Live), conn_of: conn(0), file: file.clone() }));
    m.capture_paused.store(true, Ordering::SeqCst);
    assert!(!deliver(&mut w, &rec, &m));
    assert_eq!((m.universe_acked_seq.load(Ordering::SeqCst), m.capture_drops.load(Ordering::SeqCst)), (0, 1));
    assert!(!universe_path(&dir, T0, &file).exists());
    m.capture_paused.store(false, Ordering::SeqCst);
    assert!(deliver(&mut w, &rec, &m));
    assert_eq!(m.universe_acked_seq.load(Ordering::SeqCst), 42);
    assert!(universe_path(&dir, T0, &file).exists());
    assert_eq!(CaptureState::of(&m), CaptureState { acked_seq: 42, paused: false, drops: 1 });
    std::fs::remove_dir_all(&dir).ok();
}

fn book_frame(ts: i64, ask: &str) -> String {
    let items: Vec<String> = (0..3)
        .map(|i| format!(r#"{{"event_type":"book","asset_id":"y{i}","market":"0x","timestamp":"{ts}","hash":"h","bids":[],"asks":[{{"price":"{ask}","size":"10"}}]}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

fn write(dir: &Path, reuse: bool) {
    let u1 = universe(T0, Status::Live);
    let f1 = universe_file_name(T0, 1);
    let mut w = Writer::new(dir.to_path_buf());
    let mut msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord { universe: u1.clone(), conn_of: conn(0), file: f1.clone() })),
        CaptureMsg::Line(Line { recv_ts_us: (T0 + 1_000) * 1000, shard: 0, raw: None, ctl: Some(Ctl::Universe { taken_ms: T0, file: f1.clone() }) }),
        CaptureMsg::Line(Line { recv_ts_us: (T0 + 2_000) * 1000, shard: 0, raw: Some(book_frame(T0 + 1_990, "0.30")), ctl: None }),
    ];
    let t2 = T0 + 300_000;
    let second = if reuse {
        Ctl::Universe { taken_ms: T0, file: f1 }
    } else {
        let f2 = universe_file_name(t2, 2);
        msgs.push(CaptureMsg::Universe(Box::new(UniverseRecord { universe: universe(t2, Status::Live), conn_of: conn(0), file: f2.clone() })));
        Ctl::Universe { taken_ms: t2, file: f2 }
    };
    msgs.push(CaptureMsg::Line(Line { recv_ts_us: (t2 + 1_000) * 1000, shard: 0, raw: None, ctl: Some(second) }));
    msgs.push(CaptureMsg::Line(Line { recv_ts_us: (t2 + 2_000) * 1000, shard: 0, raw: Some(book_frame(t2 + 1_990, "0.31")), ctl: None }));
    msgs.push(CaptureMsg::Line(Line { recv_ts_us: (t2 + 3_000) * 1000, shard: 0, raw: None, ctl: Some(Ctl::Shutdown) }));
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

#[test]
fn replay_of_a_reused_record_equals_replay_of_a_fresh_identical_one() {
    let (a, b) = (tmp("reuse"), tmp("fresh"));
    write(&a, true);
    write(&b, false);
    let (ra, rb) = (replay_dir(&a, ctx()).unwrap(), replay_dir(&b, ctx()).unwrap());
    assert_eq!(format!("{:?}", ra.outs), format!("{:?}", rb.outs));
    assert!(!ra.outs.is_empty());
    std::fs::remove_dir_all(&a).ok();
    std::fs::remove_dir_all(&b).ok();
}

fn touch(p: &Path, ms: i64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap())).unwrap();
}

#[test]
fn retention_keeps_the_universe_record_in_force_at_the_oldest_hour() {
    let dir = tmp("keep");
    let t = T0 + 10 * H;
    let rec = |taken: i64, seq: u64, mtime: i64| {
        let p = universe_path(&dir, taken, &universe_file_name(taken, seq));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        touch(&p, mtime);
        p
    };
    let older = rec(t - 3 * H, 1, t - 3 * H);
    let in_force = rec(t - 2 * H, 2, t - 2 * H); // written before the oldest hour, still named by it
    let fresh = rec(t + 60_000, 3, t + 61_000);
    assert!(stale_universe_files(&dir).is_empty(), "no hour files: nothing to compare against");
    let hour = hour_path(&dir, t);
    std::fs::create_dir_all(hour.parent().unwrap()).unwrap();
    std::fs::write(&hour, b"x").unwrap();
    assert_eq!(stale_universe_files(&dir), vec![older]);
    assert!(in_force.exists() && fresh.exists());
    std::fs::remove_dir_all(&dir).ok();
}
```
In `bins/scanner/tests/replay.rs`, delete the test `stale_universe_files_are_deleted_once_older_than_every_remaining_hour_file` (its rule is replaced; `retention_keeps_the_universe_record_in_force_at_the_oldest_hour` covers the new one) and remove `stale_universe_files` from that file's `use scanner::capture::{…}` list if it is then unused.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p scanner --test universe_dedup`
Expected: FAIL (no `UniverseDedup`); after Step 3 alone the retention test still fails until Step 4.

- [ ] **Step 3: The dedup**

`bins/scanner/src/capture.rs`, after `CaptureMsg` (add `use sha2::{Digest, Sha256};` to the imports):
```rust
/// What to capture for a universe refresh (spec 5b §4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniversePlan {
    /// The content changed: write a new record (then call `UniverseDedup::written`).
    Write { hash: String },
    /// Same content as the last record written: capture only a control line naming it.
    Reuse { taken_ms: i64, file: String },
}

/// sha256 of the content a replay uses: the groups, the canary pairs and the connection map.
/// `taken_ms` is left out on purpose: it changes every refresh, the content often does not.
pub fn universe_hash(u: &Universe, conn_of: &ConnMap) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(&(&u.groups, &u.canary, conn_of))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// What the dedup needs to know about the capture thread (spec 5b §4.5, ruling R16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureState {
    /// Seq of the newest universe record the capture thread wrote to disk.
    pub acked_seq: u64,
    pub paused: bool,
    /// Capture messages dropped so far.
    pub drops: u64,
}

impl CaptureState {
    pub fn of(m: &Metrics) -> CaptureState {
        CaptureState {
            acked_seq: m.universe_acked_seq.load(Ordering::Acquire),
            paused: m.capture_paused.load(Ordering::Relaxed),
            drops: m.capture_drops.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct LastRecord {
    hash: String,
    taken_ms: i64,
    file: String,
    seq: u64,
    drops: u64,
}

/// The last universe record handed to the capture thread.
#[derive(Debug, Clone, Default)]
pub struct UniverseDedup {
    last: Option<LastRecord>,
}

impl UniverseDedup {
    /// Reuse the last record only when its content is the same AND capture really wrote it
    /// (acknowledged), is not paused, and dropped nothing since; otherwise write a new record.
    pub fn plan(&self, u: &Universe, conn_of: &ConnMap, st: CaptureState) -> UniversePlan {
        match universe_hash(u, conn_of) {
            Ok(h) => match &self.last {
                Some(l)
                    if l.hash == h && st.acked_seq >= l.seq && !st.paused && st.drops == l.drops =>
                {
                    UniversePlan::Reuse {
                        taken_ms: l.taken_ms,
                        file: l.file.clone(),
                    }
                }
                _ => UniversePlan::Write { hash: h },
            },
            // Can't hash it: always write (an empty hash is never remembered).
            Err(_) => UniversePlan::Write {
                hash: String::new(),
            },
        }
    }

    /// Call once record `seq` was handed to the capture thread (`drops`: the drop count then).
    pub fn written(&mut self, hash: String, taken_ms: i64, file: String, seq: u64, drops: u64) {
        self.last = (!hash.is_empty()).then_some(LastRecord {
            hash,
            taken_ms,
            file,
            seq,
            drops,
        });
    }
}

/// `<seq>` of a `universe-<taken_ms>-<seq>.json.zst` file name.
fn universe_seq(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("universe-")?.strip_suffix(".json.zst")?;
    rest.rsplit_once('-')?.1.parse().ok()
}

/// Write one capture message unless capture is paused. A universe record that reaches disk is
/// acknowledged (`metrics.universe_acked_seq`), so the dedup never names a record that was
/// dropped. Returns whether it was written.
pub fn deliver(writer: &mut Writer, msg: &CaptureMsg, metrics: &Metrics) -> bool {
    if metrics.capture_paused.load(Ordering::Relaxed) {
        metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    match writer.write(msg) {
        Ok(()) => {
            if let CaptureMsg::Universe(rec) = msg
                && let Some(seq) = universe_seq(&rec.file)
            {
                metrics.universe_acked_seq.fetch_max(seq, Ordering::Release);
            }
            true
        }
        Err(e) => {
            metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
            eprintln!("capture: write failed: {e}");
            false
        }
    }
}
```
In `spawn`, replace the write path at the end of the loop (the `if metrics.capture_paused.load(…) { …; continue; }` block and the `if let Err(e) = writer.write(&msg) { … }` block) with `deliver(&mut writer, &msg, &metrics);`.

`bins/scanner/src/metrics.rs`, in `Metrics` after `capture_paused`:
```rust
    /// Seq of the newest universe record the capture thread wrote to disk (spec 5b §4.5): the
    /// universe dedup names only acknowledged records.
    pub universe_acked_seq: AtomicU64,
```
In `bins/scanner/src/run.rs` `run_live`, replace the whole `let mut apply = |engine: &mut Engine, u: Arc<Universe>, conn_of: &HashMap<String, usize>, io: &Io| { … };` closure with:
```rust
    // Spec 5b §4.5: a new record only when the content changed; an unchanged refresh still
    // captures its control line, naming the last record capture acknowledged, so replay sees the
    // same inputs (ruling R16).
    let mut dedup = capture::UniverseDedup::default();
    let mut apply =
        |engine: &mut Engine, u: Arc<Universe>, conn_of: &HashMap<String, usize>, io: &Io| {
            let conn_map: ConnMap = conn_of.iter().map(|(k, v)| (k.clone(), *v)).collect();
            let st = capture::CaptureState::of(&io.metrics);
            let ctl = match dedup.plan(&u, &conn_map, st) {
                capture::UniversePlan::Reuse { taken_ms, file } => Ctl::Universe { taken_ms, file },
                capture::UniversePlan::Write { hash } => {
                    universe_seq += 1;
                    let file = capture::universe_file_name(u.taken_ms, universe_seq);
                    let rec = UniverseRecord {
                        universe: (*u).clone(),
                        conn_of: conn_map,
                        file: file.clone(),
                    };
                    if io
                        .capture
                        .try_send(CaptureMsg::Universe(Box::new(rec)))
                        .is_err()
                    {
                        io.metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
                    } else {
                        dedup.written(hash, u.taken_ms, file.clone(), universe_seq, st.drops);
                    }
                    Ctl::Universe {
                        taken_ms: u.taken_ms,
                        file,
                    }
                }
            };
            let t = now_ms();
            io.ctl(0, ctl, t);
            engine.apply_universe(u, conn_of, t)
        };
```

- [ ] **Step 4: The retention rule**

Replace `stale_universe_files` in `bins/scanner/src/capture.rs` with:
```rust
/// Universe records no remaining hour can name (spec 5b §4.5). A control line names the record in
/// force when it was captured: one written during or after the oldest remaining hour, or the newest
/// one written before it (an unchanged refresh keeps naming that older record). So every record
/// written (mtime) at or after the oldest hour's start is kept, plus the newest one before it; the
/// rest are returned. A record whose write time can't be read is kept. No hour files: nothing.
pub fn stale_universe_files(dir: &Path) -> Vec<PathBuf> {
    let l = tape::files::listing(dir);
    let Some(oldest) = l.hours.iter().map(|h| h.start_ms).min() else {
        return Vec::new();
    };
    let mut before: Vec<(i64, PathBuf)> = l
        .universes
        .into_iter()
        .filter_map(|u| u.mtime_ms.filter(|m| *m < oldest).map(|m| (m, u.path)))
        .collect();
    before.sort();
    before.pop(); // the newest record written before the oldest hour is still in force
    let mut out: Vec<PathBuf> = before.into_iter().map(|(_, p)| p).collect();
    out.sort();
    out
}
```
(`universe_taken_ms` may now be unused in `capture.rs` except by `over_budget_hours`; keep the import only if the compiler uses it.)

- [ ] **Step 5: Run the scanner tests**

Run: `cargo test -p scanner && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the five dedup tests and every replay test.

- [ ] **Step 6: Commit**

```bash
git add bins/scanner/src/capture.rs bins/scanner/src/metrics.rs bins/scanner/src/run.rs bins/scanner/tests/universe_dedup.rs bins/scanner/tests/replay.rs
git commit -m "scanner: universe records only when changed and acknowledged; retention keeps the record in force"
```

---

### Task 8: `bins/compact` skeleton: pinned `parquet`/`arrow`, deny rules, guard, arch rules

**Files:**
- Create: `bins/compact/Cargo.toml`, `bins/compact/src/{lib,guard}.rs`
- Modify: `Cargo.toml` (members, `[workspace.dependencies]`), `deny.toml`, `bins/scanner/tests/arch.rs`

**Interfaces:**
- Produces (crate `compact`): `compact::version() -> &'static str`, `compact::now_ms() -> i64`, `compact::refuse_other_writes(&[String]) -> anyhow::Result<()>`, `compact::guard::{forbidden, check_env}` (the shared denylist).

- [ ] **Step 1: Pin the two approved crates**

```bash
cargo search parquet --limit 1 && cargo search arrow --limit 1
```
`60.0.0` was the current version of both when this plan was written (2026-09-27); re-check with the command above and pin exactly what it prints. Apache Arrow Rust releases `arrow` and `parquet` in lockstep, so both must print the same version: **if they differ, stop and ask the owner.** In the root `Cargo.toml` `[workspace.dependencies]` add (with the version the command printed in place of `60.0.0` if it moved):
```toml
# Phase 5b, owner-approved 2026-09-27: the compactor's Parquet archive. compact only (deny.toml wrappers, arch test).
parquet = { version = "=60.0.0", default-features = false, features = ["arrow", "zstd"] }
arrow = { version = "=60.0.0", default-features = false }
```
and add `"bins/compact"` to `members` (after `"bins/analyze"`). In `deny.toml` `[bans] deny`, after the `sqlx` line:
```toml
  { name = "parquet", wrappers = ["compact"] },
  { name = "arrow", wrappers = ["compact"] },
```

- [ ] **Step 2: Write the failing arch rules**

In `bins/scanner/tests/arch.rs` `crate_graph_matches_spec_9_2`, after the `analyze` loop:
```rust
    for bad in ["reqwest", "tokio-tungstenite", "sqlx", "pm-api"] {
        assert!(
            !d["compact"].contains(bad),
            "compact reads Postgres only via store and has no network client ({bad})"
        );
    }
    for (name, ds) in &d {
        for c in ["parquet", "arrow"] {
            assert!(
                name == "compact" || !ds.contains(c),
                "{c} is compact-only (owner approval 2026-09-27), but {name} depends on it"
            );
        }
    }
```
and in `scanner_and_api_guard_denylists_are_identical`, after the `analyze` comparison:
```rust
    let compact_src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../compact/src/guard.rs"
    ))
    .expect("compact guard.rs");
    assert_eq!(scanner_contains, denylist(&compact_src, "CONTAINS"), "CONTAINS drifted in compact");
    assert_eq!(scanner_prefixes, denylist(&compact_src, "PREFIXES"), "PREFIXES drifted in compact");
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p scanner --test arch`
Expected: FAIL (`compact` is not a workspace package yet: indexing `d["compact"]` panics, and `compact/src/guard.rs` can't be read).

- [ ] **Step 4: The crate and its guard**

`bins/compact/Cargo.toml`:
```toml
[package]
name = "compact"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# No network client (spec 5b §2): Postgres only through store; the arch test enforces it.
# parquet and arrow are compact-only (owner-approved 2026-09-27; deny.toml wrappers).
[dependencies]
tape = { path = "../../crates/tape" }
arb-core = { path = "../../crates/arb-core" }
store = { path = "../../crates/store" }
tokio = { workspace = true, features = ["signal"] }
serde.workspace = true
serde_json.workspace = true
rust_decimal.workspace = true
toml.workspace = true
clap.workspace = true
anyhow.workspace = true
zstd.workspace = true
sha2.workspace = true
parquet.workspace = true
arrow.workspace = true

[dev-dependencies]
rust_decimal_macros.workspace = true

[lints]
workspace = true
```
`bins/compact/src/lib.rs`:
```rust
//! compact: every closed raw capture hour → the Parquet archive (spec 5b). No network client
//! (spec §2), paper only (parent §16); Postgres only through store as compact_rw, which writes
//! only compact_hours and compact_status.
#![forbid(unsafe_code)]

pub mod guard;

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

/// Refuse to run with a role that can write anything but compact_hours and compact_status.
pub fn refuse_other_writes(bad: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(
        bad.is_empty(),
        "refusing to start: the database role can write {} (connect as compact_rw, which writes only \
         compact_hours and compact_status)",
        bad.join(", ")
    );
    Ok(())
}
```
`bins/compact/src/guard.rs`: create it as an exact copy of `bins/analyze/src/guard.rs` (same `CONTAINS` and `PREFIXES` lists, same `forbidden`, `check_env` and test), changing only the first doc line's last sentence to `Same denylist as bins/scanner/src/guard.rs, bins/api/src/guard.rs and bins/analyze/src/guard.rs (bins/scanner/tests/arch.rs keeps the four in step).`:
```bash
cp bins/analyze/src/guard.rs bins/compact/src/guard.rs
```
then edit that doc line.

- [ ] **Step 5: Check the direct dependents and run the tests**

```bash
cargo tree -i parquet --edges normal --depth 1 && cargo tree -i arrow --edges normal --depth 1
```
Expect `compact` as the only dependent of each. If `parquet` itself depends on `arrow` (not just the `arrow-*` sub-crates), make that ban `{ name = "arrow", wrappers = ["compact", "parquet"] }`; CI runs `cargo deny check bans` (not installed on the PC).

Run: `cargo test -p scanner --test arch && cargo test -p compact && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && sh scripts/disk-budget.sh`
Expected: PASS; the budget script passes (parquet adds build artifacts; run `cargo clean` first if `target/` is over 8 GB).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock deny.toml bins/compact/Cargo.toml bins/compact/src/lib.rs bins/compact/src/guard.rs bins/scanner/tests/arch.rs
git commit -m "compact: skeleton with pinned parquet/arrow (compact-only), guard and arch rules"
```

---

### Task 9: `compact`: config and rows

**Files:**
- Create: `bins/compact/src/config.rs`, `bins/compact/src/rows.rs`, `config/compact.toml`, `bins/compact/tests/config.rs`, `bins/compact/tests/rows.rs`
- Modify: `bins/compact/src/lib.rs`

**Interfaces:**
- Consumes: Task 8 (the crate).
- Produces:
  - `compact::config::Config { raw_dir, archive_dir, depth_every_secs: i64, depth_levels: usize, l2_before_secs: i64, l2_after_secs: i64, l2_max_window_secs: i64, archive_budget_gb: u64, lag_alarm_hours: i64 }` (`Serialize + Deserialize + Clone + PartialEq`), `Config::parse(&str) -> anyhow::Result<(Config, String /* sha256 hex */)>`, `Config::load(&Path)`, `Config::budget_bytes() -> u64`, `sha256_hex(&[u8]) -> String`.
  - `compact::rows`: `MICRO`, `SIDE_BID = 0`, `SIDE_ASK = 1`, `reason::{DOWN, RAW_HOUR_LOST, NO_BOOK_YET, SCANNER_STOPPED}`, `micros(Decimal) -> anyhow::Result<i64>`, `TokenRow`, `TobRow`, `DepthRow`, `L2Row`, `GapRow` (fields as spec §3), `trait Sink { tob, depth, l2, gap }` (each `-> anyhow::Result<()>`), `VecSink { tob, depth, l2, gaps }`.

- [ ] **Step 1: Write the failing tests**

`bins/compact/tests/config.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::config::Config;

const REPO: &str = include_str!("../../../config/compact.toml");

#[test]
fn the_repo_config_has_the_spec_defaults() {
    let (c, sha) = Config::parse(REPO).unwrap();
    assert_eq!((c.raw_dir.as_str(), c.archive_dir.as_str()), ("/data/capture", "/archive"));
    assert_eq!((c.depth_every_secs, c.depth_levels), (300, 10));
    assert_eq!((c.l2_before_secs, c.l2_after_secs, c.l2_max_window_secs), (300, 300, 1800));
    assert_eq!((c.archive_budget_gb, c.lag_alarm_hours), (55, 3));
    assert_eq!(c.budget_bytes(), 55_000_000_000);
    assert_eq!(sha.len(), 64);
    assert!(!REPO.contains('\r'), "config/compact.toml must be LF (.gitattributes)");
}

#[test]
fn bad_values_are_refused_at_startup() {
    for (from, to) in [
        ("depth_every_secs = 300", "depth_every_secs = 7"),
        ("depth_every_secs = 300", "depth_every_secs = 420"),
        ("depth_levels = 10", "depth_levels = 0"),
        ("l2_before_secs = 300", "l2_before_secs = 3601"),
        ("l2_max_window_secs = 1800", "l2_max_window_secs = 30"),
        ("archive_budget_gb = 55", "archive_budget_gb = 71"),
        ("lag_alarm_hours = 3", "lag_alarm_hours = 0"),
    ] {
        assert!(REPO.contains(from), "{from}");
        assert!(Config::parse(&REPO.replace(from, to)).is_err(), "{to} must be refused");
    }
    assert!(Config::parse(&format!("{REPO}\nsurprise = 1\n")).is_err(), "unknown keys refused");
    let (_, a) = Config::parse(REPO).unwrap();
    let (_, b) = Config::parse(&REPO.replace("lag_alarm_hours = 3", "lag_alarm_hours = 4")).unwrap();
    assert_ne!(a, b, "the manifest's config sha changes with the file");
}
```
`bins/compact/tests/rows.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::rows::*;
use rust_decimal_macros::dec;

#[test]
fn micro_units_are_exact_or_refused() {
    assert_eq!(micros(dec!(0.123456)).unwrap(), 123_456);
    assert_eq!(micros(dec!(100)).unwrap(), 100_000_000);
    assert_eq!(micros(dec!(0)).unwrap(), 0);
    assert!(micros(dec!(0.1234567)).is_err());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test config --test rows`
Expected: FAIL (no `compact::config`, no `compact::rows`).

- [ ] **Step 3: The config and the rows**

`config/compact.toml` (LF, covered by `config/*.toml text eol=lf`):
```toml
# compact: raw capture hours → the Parquet archive (spec 2026-09-27-phase5b-tiered-storage-design.md §3, §7).
# Integers only. None of these is a premise input. Its sha256 goes into every day's manifest.json.
raw_dir = "/data/capture"       # the scanner's capture folder, mounted read-only
archive_dir = "/archive"        # YYYY-MM-DD/{tokens,tob,depth,l2,gaps}.parquet + manifest.json, _state/
depth_every_secs = 300          # D3: a depth snapshot every 5 min (must divide 3600)
depth_levels = 10               # D3: top 10 levels per side, only tokens whose book changed
l2_before_secs = 300            # D2: l2 from 5 min before a net-positive arbitrage episode opens
l2_after_secs = 300             # … to 5 min after it closes
l2_max_window_secs = 1800       # … capped at 30 min (flagged as truncated in the manifest)
archive_budget_gb = 55          # §4.4: 55 GB archive + ~15 GB raw + ~0.6 GB database < 75 GB
lag_alarm_hours = 3             # §5: the compactor logs (and the dashboard loses a heart) above this
```
`bins/compact/src/config.rs`:
```rust
//! config/compact.toml (spec 5b §7). Integers only; validated at startup; its sha256 goes into
//! every day's manifest.
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub raw_dir: String,
    pub archive_dir: String,
    pub depth_every_secs: i64,
    pub depth_levels: usize,
    pub l2_before_secs: i64,
    pub l2_after_secs: i64,
    pub l2_max_window_secs: i64,
    pub archive_budget_gb: u64,
    pub lag_alarm_hours: i64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Config {
    /// Parse and validate; returns the config and the sha256 of its text.
    pub fn parse(text: &str) -> anyhow::Result<(Config, String)> {
        let c: Config = toml::from_str(text).context("compact.toml")?;
        c.validate()?;
        Ok((c, sha256_hex(text.as_bytes())))
    }

    pub fn load(path: &Path) -> anyhow::Result<(Config, String)> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        Self::parse(&text)
    }

    pub fn budget_bytes(&self) -> u64 {
        self.archive_budget_gb.saturating_mul(1_000_000_000)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.raw_dir.is_empty() && !self.archive_dir.is_empty(),
            "raw_dir and archive_dir must be set"
        );
        anyhow::ensure!(
            (60..=3_600).contains(&self.depth_every_secs) && 3_600 % self.depth_every_secs == 0,
            "depth_every_secs must be 60..=3600 and divide 3600 (snapshots line up with hours)"
        );
        anyhow::ensure!(
            (1..=50).contains(&self.depth_levels),
            "depth_levels must be 1..=50"
        );
        anyhow::ensure!(
            (0..=3_600).contains(&self.l2_before_secs) && (0..=3_600).contains(&self.l2_after_secs),
            "l2_before_secs and l2_after_secs must be 0..=3600 (an hour is compacted once the next one closes)"
        );
        anyhow::ensure!(
            (60..=7_200).contains(&self.l2_max_window_secs),
            "l2_max_window_secs must be 60..=7200"
        );
        anyhow::ensure!(
            (1..=70).contains(&self.archive_budget_gb),
            "archive_budget_gb must be 1..=70 (the project's 75 GB ceiling, spec §4.4)"
        );
        anyhow::ensure!(
            (1..=48).contains(&self.lag_alarm_hours),
            "lag_alarm_hours must be 1..=48"
        );
        Ok(())
    }
}
```
`bins/compact/src/rows.rs`:
```rust
//! Archive rows (spec 5b §3). Integers only: prices and sizes in micro-units (value × 10⁶).
use anyhow::Context;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

pub const MICRO: i64 = 1_000_000;
pub const SIDE_BID: u8 = 0;
pub const SIDE_ASK: u8 = 1;

/// `gaps.reason` values (spec §3).
pub mod reason {
    pub const DOWN: &str = "down";
    pub const RAW_HOUR_LOST: &str = "raw_hour_lost";
    pub const NO_BOOK_YET: &str = "no_book_yet";
    pub const SCANNER_STOPPED: &str = "scanner_stopped";
}

/// value × 10⁶, exactly: an error when `d` has more than 6 decimals or does not fit.
pub fn micros(d: Decimal) -> anyhow::Result<i64> {
    let m = d
        .checked_mul(Decimal::from(MICRO))
        .with_context(|| format!("{d} × 10⁶ overflows"))?;
    anyhow::ensure!(m.fract().is_zero(), "{d} has more than 6 decimal places");
    m.to_i64()
        .with_context(|| format!("{d} × 10⁶ does not fit an i64"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRow {
    pub token_idx: u32,
    pub token_id: String,
    pub market_id: String,
    pub event_id: String,
    /// "yes" or "no".
    pub outcome: String,
    pub neg_risk: bool,
}

/// A change of best bid or best ask (price or size); a side is null when it is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TobRow {
    pub ts_ms: i64,
    pub recv_ms: i64,
    pub token_idx: u32,
    pub bid: Option<i64>,
    pub bid_sz: Option<i64>,
    pub ask: Option<i64>,
    pub ask_sz: Option<i64>,
}

/// One level of a periodic snapshot (level 0 = best).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthRow {
    pub ts_ms: i64,
    pub token_idx: u32,
    pub side: u8,
    pub level: u16,
    pub price: i64,
    pub size: i64,
}

/// An exact level change (or, with `snapshot`, a level of a full book) inside an episode window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Row {
    pub ts_ms: i64,
    pub recv_ms: i64,
    pub token_idx: u32,
    pub side: u8,
    pub price: i64,
    pub new_size: i64,
    pub snapshot: bool,
    pub episode_id: String,
}

/// A stretch with missing or untrustworthy data. `shard` null: the whole feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapRow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub shard: Option<i32>,
    pub reason: String,
}

/// Where the reducer puts rows (Parquet parts in production, vectors in tests).
pub trait Sink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()>;
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()>;
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()>;
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VecSink {
    pub tob: Vec<TobRow>,
    pub depth: Vec<DepthRow>,
    pub l2: Vec<L2Row>,
    pub gaps: Vec<GapRow>,
}

impl Sink for VecSink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()> {
        self.tob.push(r);
        Ok(())
    }
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()> {
        self.depth.push(r);
        Ok(())
    }
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()> {
        self.l2.push(r);
        Ok(())
    }
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()> {
        self.gaps.push(r);
        Ok(())
    }
}
```
`bins/compact/src/lib.rs`: add `pub mod config;` and `pub mod rows;` (alphabetical with `guard`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add bins/compact/src/config.rs bins/compact/src/rows.rs bins/compact/src/lib.rs config/compact.toml bins/compact/tests/config.rs bins/compact/tests/rows.rs
git commit -m "compact: config (validated, sha256) and archive rows in micro-units"
```

---

### Task 10: `compact`: Parquet I/O

**Files:**
- Create: `bins/compact/src/pq.rs`, `bins/compact/tests/pq.rs`
- Modify: `bins/compact/src/lib.rs`

**Interfaces:**
- Consumes: Task 9 (`rows`).
- Produces (`compact::pq`): `ROW_GROUP_ROWS = 262_144`, `BATCH_ROWS = 65_536`, `ZSTD_LEVEL = 3`, `props() -> anyhow::Result<WriterProperties>`, `trait Table { const NAME; fn schema() -> SchemaRef; fn batch(&[Self]) -> anyhow::Result<RecordBatch>; fn rows(&RecordBatch) -> anyhow::Result<Vec<Self>> }` for all five row types, `tmp_of(&Path) -> PathBuf`, `write_atomic(&Path, &[u8]) -> std::io::Result<()>`, `PartWriter<R: Table>` (`create(PathBuf)`, `push(R)`, `close() -> anyhow::Result<Staged>`), `Staged { tmp, fin, rows }` + `Staged::publish()`, `num_rows(&Path) -> anyhow::Result<u64>`, `num_row_groups(&Path) -> anyhow::Result<usize>`, `for_each_batch(&Path, FnMut(&RecordBatch) -> anyhow::Result<()>)`, `read_all::<R>(&Path) -> anyhow::Result<Vec<R>>`.

- [ ] **Step 1: Write the failing tests**

`bins/compact/tests/pq.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::pq::*;
use compact::rows::*;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("compact-pq-{name}-{}-{}", std::process::id(), compact::now_ms()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write<R: Table>(p: PathBuf, rows: &[R]) -> Staged
where
    R: Clone,
{
    let mut w = PartWriter::<R>::create(p).unwrap();
    for r in rows {
        w.push(r.clone()).unwrap();
    }
    w.close().unwrap()
}

#[test]
fn every_table_round_trips_with_nulls_and_is_invisible_until_published() {
    let d = tmp("rt");
    let tob = vec![
        TobRow { ts_ms: 1, recv_ms: 2, token_idx: 0, bid: Some(400_000), bid_sz: Some(1), ask: None, ask_sz: None },
        TobRow { ts_ms: 3, recv_ms: 4, token_idx: 7, bid: None, bid_sz: None, ask: Some(420_000), ask_sz: Some(9) },
    ];
    let s = write(d.join("00.tob.parquet"), &tob);
    assert!(s.tmp.exists() && !s.fin.exists(), "written under a temporary name");
    s.publish().unwrap();
    assert!(!s.tmp.exists() && s.fin.exists());
    assert_eq!(read_all::<TobRow>(&s.fin).unwrap(), tob);
    let depth = vec![DepthRow { ts_ms: 5, token_idx: 1, side: SIDE_ASK, level: 9, price: 1, size: 2 }];
    let l2 = vec![L2Row { ts_ms: 6, recv_ms: 7, token_idx: 2, side: SIDE_BID, price: 3, new_size: 0, snapshot: true, episode_id: "live-1-01".into() }];
    let gaps = vec![
        GapRow { start_ms: 1, end_ms: 2, shard: None, reason: reason::RAW_HOUR_LOST.into() },
        GapRow { start_ms: 3, end_ms: 4, shard: Some(12), reason: reason::DOWN.into() },
    ];
    let tokens = vec![TokenRow { token_idx: 0, token_id: "123".into(), market_id: "m".into(), event_id: "e".into(), outcome: "yes".into(), neg_risk: true }];
    for (s, n) in [
        (write(d.join("depth.parquet"), &depth), 1),
        (write(d.join("l2.parquet"), &l2), 1),
        (write(d.join("gaps.parquet"), &gaps), 2),
        (write(d.join("tokens.parquet"), &tokens), 1),
    ] {
        assert_eq!(s.rows, n);
        s.publish().unwrap();
        assert_eq!(num_rows(&s.fin).unwrap(), n);
    }
    assert_eq!(read_all::<DepthRow>(&d.join("depth.parquet")).unwrap(), depth);
    assert_eq!(read_all::<L2Row>(&d.join("l2.parquet")).unwrap(), l2);
    assert_eq!(read_all::<GapRow>(&d.join("gaps.parquet")).unwrap(), gaps);
    assert_eq!(read_all::<TokenRow>(&d.join("tokens.parquet")).unwrap(), tokens);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn row_groups_hold_262144_rows_so_memory_stays_bounded() {
    let d = tmp("rg");
    let rows: Vec<TobRow> = (0..600_000)
        .map(|i| TobRow { ts_ms: i, recv_ms: i, token_idx: u32::try_from(i % 4_000).unwrap(), bid: Some(i), bid_sz: Some(1), ask: None, ask_sz: None })
        .collect();
    let s = write(d.join("tob.parquet"), &rows);
    s.publish().unwrap();
    assert_eq!(ROW_GROUP_ROWS, 262_144);
    assert_eq!(num_row_groups(&s.fin).unwrap(), 3, "600,000 rows = 262,144 + 262,144 + 75,712");
    assert_eq!(num_rows(&s.fin).unwrap(), 600_000);
    std::fs::remove_dir_all(&d).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test pq`
Expected: FAIL (no `compact::pq`).

- [ ] **Step 3: Parquet I/O**

Ruling R2 (spec §11): `ArrowWriter` keeps the row group being built in memory, encoded and compressed, until it reaches the row-group size, plus one buffered write batch. Row groups of 262,144 rows (a tob row is 7 columns of at most 8 bytes: ≤ 15 MB before encoding, a few MB after zstd) and batches of 65,536 rows keep one open part well under 50 MB, and an hour writes four parts at once. The benchmark in Task 21 (runbook §3) measures peak RSS on a real hour against the 256 MB cap.

`bins/compact/src/pq.rs`:
```rust
//! The archive's Parquet files (spec 5b §3): schemas, a writer that works under a temporary name
//! until `publish` renames it, and readers. zstd level 3; dictionary encoding (parquet's default)
//! for every column, which is what `token_idx` needs. Row groups of 262,144 rows bound memory
//! (plan ruling R2).
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use arrow::array::{
    Array, ArrayRef, BooleanArray, Int32Array, Int64Array, StringArray, UInt8Array, UInt16Array,
    UInt32Array,
};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const ROW_GROUP_ROWS: usize = 262_144;
pub const BATCH_ROWS: usize = 65_536;
pub const ZSTD_LEVEL: i32 = 3;

pub fn props() -> anyhow::Result<WriterProperties> {
    Ok(WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL)?))
        .set_max_row_group_size(ROW_GROUP_ROWS)
        .build())
}

/// `<path>.tmp`: where a file is written before it is published.
pub fn tmp_of(fin: &Path) -> PathBuf {
    let mut s = fin.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Write `bytes` under a temporary name, then rename over `path` (never a half-written file).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_of(path);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn f(name: &str, t: DataType, nullable: bool) -> Field {
    Field::new(name, t, nullable)
}

fn col<'a, T: Array + 'static>(b: &'a RecordBatch, i: usize) -> anyhow::Result<&'a T> {
    anyhow::ensure!(i < b.num_columns(), "column {i} missing");
    b.column(i)
        .as_any()
        .downcast_ref::<T>()
        .with_context(|| format!("column {i} has an unexpected type"))
}

fn opt(a: &Int64Array, i: usize) -> Option<i64> {
    if a.is_null(i) { None } else { Some(a.value(i)) }
}

fn arc<A: Array + 'static>(a: A) -> ArrayRef {
    Arc::new(a)
}

pub trait Table: Sized {
    const NAME: &'static str;
    fn schema() -> SchemaRef;
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch>;
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>>;
}

impl Table for TobRow {
    const NAME: &'static str = "tob";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("recv_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("bid", DataType::Int64, true),
            f("bid_sz", DataType::Int64, true),
            f("ask", DataType::Int64, true),
            f("ask_sz", DataType::Int64, true),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.recv_ms).collect::<Vec<_>>())),
                arc(UInt32Array::from(rows.iter().map(|r| r.token_idx).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.bid).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.bid_sz).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.ask).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.ask_sz).collect::<Vec<_>>())),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, recv, idx) = (col::<Int64Array>(b, 0)?, col::<Int64Array>(b, 1)?, col::<UInt32Array>(b, 2)?);
        let (bid, bsz, ask, asz) = (col::<Int64Array>(b, 3)?, col::<Int64Array>(b, 4)?, col::<Int64Array>(b, 5)?, col::<Int64Array>(b, 6)?);
        Ok((0..b.num_rows())
            .map(|i| TobRow {
                ts_ms: ts.value(i),
                recv_ms: recv.value(i),
                token_idx: idx.value(i),
                bid: opt(bid, i),
                bid_sz: opt(bsz, i),
                ask: opt(ask, i),
                ask_sz: opt(asz, i),
            })
            .collect())
    }
}

impl Table for DepthRow {
    const NAME: &'static str = "depth";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("side", DataType::UInt8, false),
            f("level", DataType::UInt16, false),
            f("price", DataType::Int64, false),
            f("size", DataType::Int64, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>())),
                arc(UInt32Array::from(rows.iter().map(|r| r.token_idx).collect::<Vec<_>>())),
                arc(UInt8Array::from(rows.iter().map(|r| r.side).collect::<Vec<_>>())),
                arc(UInt16Array::from(rows.iter().map(|r| r.level).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.price).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.size).collect::<Vec<_>>())),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, idx, side, level) = (col::<Int64Array>(b, 0)?, col::<UInt32Array>(b, 1)?, col::<UInt8Array>(b, 2)?, col::<UInt16Array>(b, 3)?);
        let (price, size) = (col::<Int64Array>(b, 4)?, col::<Int64Array>(b, 5)?);
        Ok((0..b.num_rows())
            .map(|i| DepthRow {
                ts_ms: ts.value(i),
                token_idx: idx.value(i),
                side: side.value(i),
                level: level.value(i),
                price: price.value(i),
                size: size.value(i),
            })
            .collect())
    }
}

impl Table for L2Row {
    const NAME: &'static str = "l2";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("ts_ms", DataType::Int64, false),
            f("recv_ms", DataType::Int64, false),
            f("token_idx", DataType::UInt32, false),
            f("side", DataType::UInt8, false),
            f("price", DataType::Int64, false),
            f("new_size", DataType::Int64, false),
            f("snapshot", DataType::Boolean, false),
            f("episode_id", DataType::Utf8, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.recv_ms).collect::<Vec<_>>())),
                arc(UInt32Array::from(rows.iter().map(|r| r.token_idx).collect::<Vec<_>>())),
                arc(UInt8Array::from(rows.iter().map(|r| r.side).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.price).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.new_size).collect::<Vec<_>>())),
                arc(BooleanArray::from(rows.iter().map(|r| r.snapshot).collect::<Vec<_>>())),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.episode_id.as_str()))),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (ts, recv, idx, side) = (col::<Int64Array>(b, 0)?, col::<Int64Array>(b, 1)?, col::<UInt32Array>(b, 2)?, col::<UInt8Array>(b, 3)?);
        let (price, size, snap, ep) = (col::<Int64Array>(b, 4)?, col::<Int64Array>(b, 5)?, col::<BooleanArray>(b, 6)?, col::<StringArray>(b, 7)?);
        Ok((0..b.num_rows())
            .map(|i| L2Row {
                ts_ms: ts.value(i),
                recv_ms: recv.value(i),
                token_idx: idx.value(i),
                side: side.value(i),
                price: price.value(i),
                new_size: size.value(i),
                snapshot: snap.value(i),
                episode_id: ep.value(i).to_string(),
            })
            .collect())
    }
}

impl Table for GapRow {
    const NAME: &'static str = "gaps";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("start_ms", DataType::Int64, false),
            f("end_ms", DataType::Int64, false),
            f("shard", DataType::Int32, true),
            f("reason", DataType::Utf8, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(Int64Array::from(rows.iter().map(|r| r.start_ms).collect::<Vec<_>>())),
                arc(Int64Array::from(rows.iter().map(|r| r.end_ms).collect::<Vec<_>>())),
                arc(Int32Array::from(rows.iter().map(|r| r.shard).collect::<Vec<_>>())),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.reason.as_str()))),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (s, e, shard, why) = (col::<Int64Array>(b, 0)?, col::<Int64Array>(b, 1)?, col::<Int32Array>(b, 2)?, col::<StringArray>(b, 3)?);
        Ok((0..b.num_rows())
            .map(|i| GapRow {
                start_ms: s.value(i),
                end_ms: e.value(i),
                shard: if shard.is_null(i) { None } else { Some(shard.value(i)) },
                reason: why.value(i).to_string(),
            })
            .collect())
    }
}

impl Table for TokenRow {
    const NAME: &'static str = "tokens";
    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            f("token_idx", DataType::UInt32, false),
            f("token_id", DataType::Utf8, false),
            f("market_id", DataType::Utf8, false),
            f("event_id", DataType::Utf8, false),
            f("outcome", DataType::Utf8, false),
            f("neg_risk", DataType::Boolean, false),
        ]))
    }
    fn batch(rows: &[Self]) -> anyhow::Result<RecordBatch> {
        Ok(RecordBatch::try_new(
            Self::schema(),
            vec![
                arc(UInt32Array::from(rows.iter().map(|r| r.token_idx).collect::<Vec<_>>())),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.token_id.as_str()))),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.market_id.as_str()))),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.event_id.as_str()))),
                arc(StringArray::from_iter_values(rows.iter().map(|r| r.outcome.as_str()))),
                arc(BooleanArray::from(rows.iter().map(|r| r.neg_risk).collect::<Vec<_>>())),
            ],
        )?)
    }
    fn rows(b: &RecordBatch) -> anyhow::Result<Vec<Self>> {
        let (idx, tok, mkt) = (col::<UInt32Array>(b, 0)?, col::<StringArray>(b, 1)?, col::<StringArray>(b, 2)?);
        let (ev, out, nr) = (col::<StringArray>(b, 3)?, col::<StringArray>(b, 4)?, col::<BooleanArray>(b, 5)?);
        Ok((0..b.num_rows())
            .map(|i| TokenRow {
                token_idx: idx.value(i),
                token_id: tok.value(i).to_string(),
                market_id: mkt.value(i).to_string(),
                event_id: ev.value(i).to_string(),
                outcome: out.value(i).to_string(),
                neg_risk: nr.value(i),
            })
            .collect())
    }
}

/// A file written under `tmp` and not yet visible under `fin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub tmp: PathBuf,
    pub fin: PathBuf,
    pub rows: u64,
}

impl Staged {
    pub fn publish(&self) -> std::io::Result<()> {
        std::fs::rename(&self.tmp, &self.fin)
    }
}

/// One Parquet file of one table, written in batches of `BATCH_ROWS` rows.
pub struct PartWriter<R: Table> {
    w: ArrowWriter<File>,
    buf: Vec<R>,
    rows: u64,
    tmp: PathBuf,
    fin: PathBuf,
}

impl<R: Table> PartWriter<R> {
    pub fn create(fin: PathBuf) -> anyhow::Result<Self> {
        let tmp = tmp_of(&fin);
        let file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let w = ArrowWriter::try_new(file, R::schema(), Some(props()?))?;
        Ok(Self {
            w,
            buf: Vec::new(),
            rows: 0,
            tmp,
            fin,
        })
    }

    pub fn push(&mut self, r: R) -> anyhow::Result<()> {
        self.buf.push(r);
        self.rows += 1;
        if self.buf.len() >= BATCH_ROWS {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let b = R::batch(&self.buf)?;
        self.w.write(&b)?;
        self.buf.clear();
        Ok(())
    }

    /// Finish the file under its temporary name. Nothing is visible until `Staged::publish`.
    pub fn close(mut self) -> anyhow::Result<Staged> {
        self.flush()?;
        let _meta = self.w.close()?;
        Ok(Staged {
            tmp: self.tmp,
            fin: self.fin,
            rows: self.rows,
        })
    }
}

fn open(path: &Path) -> anyhow::Result<ParquetRecordBatchReaderBuilder<File>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(ParquetRecordBatchReaderBuilder::try_new(file)?)
}

pub fn num_rows(path: &Path) -> anyhow::Result<u64> {
    Ok(u64::try_from(open(path)?.metadata().file_metadata().num_rows())?)
}

pub fn num_row_groups(path: &Path) -> anyhow::Result<usize> {
    Ok(open(path)?.metadata().num_row_groups())
}

/// Stream a file batch by batch (at most `BATCH_ROWS` rows in memory).
pub fn for_each_batch(
    path: &Path,
    mut f: impl FnMut(&RecordBatch) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let reader = open(path)?.with_batch_size(BATCH_ROWS).build()?;
    for b in reader {
        f(&b?)?;
    }
    Ok(())
}

/// Every row of a (small) file: tests, tokens, gaps.
pub fn read_all<R: Table>(path: &Path) -> anyhow::Result<Vec<R>> {
    let mut out = Vec::new();
    for_each_batch(path, |b| {
        out.extend(R::rows(b)?);
        Ok(())
    })?;
    Ok(out)
}
```
If the pinned version marks `set_max_row_group_size` deprecated (clippy `-D warnings` then fails), use the replacement its deprecation note names with the same 262,144-row limit, and say so in the commit message.
`bins/compact/src/lib.rs`: add `pub mod pq;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: both pq tests.

- [ ] **Step 5: Commit**

```bash
git add bins/compact/src/pq.rs bins/compact/src/lib.rs bins/compact/tests/pq.rs
git commit -m "compact: Parquet I/O (atomic parts, 262,144-row row groups, readers)"
```

---

### Task 11: `compact`: the reducer (trust, top of book, depth, gaps)

**Files:**
- Create: `bins/compact/src/reduce.rs`, `bins/compact/src/windows.rs` (the `Window` type only; Task 13 adds its math), `bins/compact/tests/fixture/mod.rs`, `bins/compact/tests/reduce.rs`
- Modify: `bins/compact/src/lib.rs`, `bins/compact/src/config.rs` (`Config::reduce()`)

**Interfaces:**
- Consumes: Task 3 (`FeedBooks`, `Applied`, `LineEvent`, `FeedState`), Task 9 (`rows`, `Config`).
- Produces:
  - `compact::windows::Window { episode_id: String, event_id: String, opened_ms: i64, start_ms: i64, end_ms: i64, truncated: bool }`.
  - `compact::reduce::ReduceCfg { depth_every_ms: i64, depth_levels: usize }`, `Config::reduce() -> ReduceCfg`.
  - `compact::reduce::{HourStats { lines, bad_lines, ts_clamped, universe_missing }, ReducerState, Reducer}`; `Reducer::new(ReduceCfg)`, `Reducer::from_state(ReduceCfg, ReducerState) -> anyhow::Result<Reducer>`, `state(&self) -> ReducerState`, `begin_hour(&mut self, hour_ms: i64, windows: Vec<Window>)`, `line(&mut self, &Line, &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>, &mut dyn Sink) -> anyhow::Result<()>`, `count_bad_line(&mut self)`, `missing_hour(&mut self, reason: &str, &mut dyn Sink) -> anyhow::Result<()>`, `cut(&mut self, reason: &str, &mut dyn Sink) -> anyhow::Result<()>`, `end_hour(&mut self, &mut dyn Sink) -> anyhow::Result<HourStats>`, `tokens(&self) -> &[TokenRow]`, `clock_ms(&self) -> i64`.
  - Test helpers in `bins/compact/tests/fixture/mod.rs` (used by Tasks 11–18): `T0`, `H`, `tmp`, `rec`, `uni`, `frame`, `down`, `shutdown`, `book`, `change`, `trade`, `m`, `rcfg`, `cfg`, `run`, `write_hour`, `write_record`.

Rules this task implements (spec §3, §4.1; rulings R4, R5):
- A book is **trusted** only after a full snapshot (ws `book` or an accepted REST snapshot). It becomes untrusted when its shard goes `Down`, when it first appears or is reset by a universe change, when a universe record is missing, at a `Shutdown`, and across a missing, lost or unreadable stretch. Untrusted books make no rows.
- `tob`: after each line, for each trusted token the line touched, a row when (bid, bid size, ask, ask size) differs from its last row (kept across gaps and days). `ts_ms` = the book's server ts, raised to the token's previous row's if it would go back (counted in `ts_clamped`); `recv_ms` = the line's.
- `depth`: when the receive clock crosses a multiple of `depth_every_secs`, before the line applies, one snapshot at that boundary of the top `depth_levels` levels per side of every trusted token that changed since its last snapshot.
- `gaps`: `shard = null` for whole-feed stretches (cold start `no_book_yet` until the first line; `scanner_stopped` from a `Shutdown` to the next line; a whole missing hour `scanner_stopped` or `raw_hour_lost`; `raw_hour_lost` from an unreadable point to the hour's end); `shard = n` while tokens of shard n wait for a snapshot (`down` after a disconnect, else `no_book_yet`), closed when the last one is trusted again. Every row is clipped to the hour being built; open gaps continue in the next hour.
- The token dictionary restarts every UTC day; `token_idx` is assigned at a token's first row that day.

- [ ] **Step 1: The shared test fixture**

`bins/compact/tests/fixture/mod.rs`:
```rust
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
//! Shared by the compact tests: hand-built universes, capture lines, frames and configs.
use compact::config::Config;
use compact::reduce::{ReduceCfg, Reducer};
use compact::rows::{VecSink, micros};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tape::line::{Ctl, Line};
use tape::universe_view::{GroupView, LegView, RecordView, UniverseView};

pub const T0: i64 = 1_790_121_600_000; // 2026-09-23 00:00Z
pub const H: i64 = 3_600_000;

pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("compact-{name}-{}-{}", std::process::id(), compact::now_ms()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A universe record named `file`: groups (event, live YES tokens; every group is NegRisk).
pub fn rec(file: &str, taken_ms: i64, groups: &[(&str, &[&str])], conn: &[(&str, usize)]) -> RecordView {
    RecordView {
        universe: UniverseView {
            taken_ms,
            groups: groups
                .iter()
                .map(|(e, toks)| GroupView {
                    event_id: e.to_string(),
                    neg_risk_market_id: Some(format!("nr-{e}")),
                    legs: toks
                        .iter()
                        .map(|t| LegView { market_id: format!("m-{t}"), yes_token: t.to_string(), no_token: format!("n-{t}"), status: "live".into() })
                        .collect(),
                })
                .collect(),
            canary: vec![],
        },
        conn_of: conn.iter().map(|(k, v)| (k.to_string(), *v)).collect::<BTreeMap<_, _>>(),
        file: file.into(),
    }
}

pub fn uni(t: i64, r: &RecordView) -> Line {
    Line { recv_ts_us: t * 1000, shard: 0, raw: None, ctl: Some(Ctl::Universe { taken_ms: r.universe.taken_ms, file: r.file.clone() }) }
}

pub fn frame(t: i64, shard: usize, raw: String) -> Line {
    Line { recv_ts_us: t * 1000, shard, raw: Some(raw), ctl: None }
}

pub fn down(t: i64, shard: usize) -> Line {
    Line { recv_ts_us: t * 1000, shard, raw: None, ctl: Some(Ctl::Down) }
}

pub fn shutdown(t: i64) -> Line {
    Line { recv_ts_us: t * 1000, shard: 0, raw: None, ctl: Some(Ctl::Shutdown) }
}

pub fn book(tok: &str, ts: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> String {
    let lv = |v: &[(&str, &str)]| v.iter().map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#)).collect::<Vec<_>>().join(",");
    format!(r#"{{"event_type":"book","asset_id":"{tok}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#, lv(bids), lv(asks))
}

/// One price_change item. `bb`/`ba`: the server's best bid/ask after it ("0"/"1" = empty side).
pub fn change(tok: &str, ts: i64, side: &str, price: &str, size: &str, bb: &str, ba: &str) -> String {
    format!(r#"{{"event_type":"price_change","market":"0x","timestamp":"{ts}","price_changes":[{{"asset_id":"{tok}","side":"{side}","price":"{price}","size":"{size}","best_bid":"{bb}","best_ask":"{ba}"}}]}}"#)
}

pub fn trade(tok: &str, ts: i64) -> String {
    format!(r#"{{"event_type":"last_trade_price","asset_id":"{tok}","timestamp":"{ts}"}}"#)
}

/// Micro-units of a decimal string, for expected rows.
pub fn m(x: &str) -> i64 {
    micros(Decimal::from_str_exact(x).unwrap()).unwrap()
}

/// Reducer settings for the tests: depth every 5 min, 2 levels.
pub fn rcfg() -> ReduceCfg {
    ReduceCfg { depth_every_ms: 300_000, depth_levels: 2 }
}

pub fn cfg(raw: &Path, archive: &Path) -> Config {
    Config {
        raw_dir: raw.to_string_lossy().to_string(),
        archive_dir: archive.to_string_lossy().to_string(),
        depth_every_secs: 300,
        depth_levels: 2,
        l2_before_secs: 300,
        l2_after_secs: 300,
        l2_max_window_secs: 1_800,
        archive_budget_gb: 55,
        lag_alarm_hours: 3,
    }
}

/// Feed lines to a reducer; universe lines resolve against `recs` by file name.
pub fn run(red: &mut Reducer, lines: &[Line], recs: &[RecordView], sink: &mut VecSink) {
    let mut load = |_: i64, file: &str| -> anyhow::Result<RecordView> {
        recs.iter().find(|r| r.file == file).cloned().ok_or_else(|| anyhow::anyhow!("no record {file}"))
    };
    for l in lines {
        red.line(l, &mut load, sink).unwrap();
    }
}

pub fn write_record(raw: &Path, r: &RecordView) {
    tape::files::write_zst_json(&tape::files::universe_path(raw, r.universe.taken_ms, &r.file), r).unwrap();
}

pub fn write_hour(raw: &Path, hour_ms: i64, lines: &[Line]) {
    tape::files::write_lines(&tape::files::hour_path(raw, hour_ms), lines).unwrap();
}
```

- [ ] **Step 2: Write the failing tests**

`bins/compact/tests/reduce.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::reduce::Reducer;
use compact::rows::*;
use fixture::*;

fn tob(ts: i64, recv: i64, idx: u32, bid: Option<(&str, &str)>, ask: Option<(&str, &str)>) -> TobRow {
    TobRow {
        ts_ms: ts,
        recv_ms: recv,
        token_idx: idx,
        bid: bid.map(|b| m(b.0)),
        bid_sz: bid.map(|b| m(b.1)),
        ask: ask.map(|a| m(a.0)),
        ask_sz: ask.map(|a| m(a.1)),
    }
}

fn gap(s: i64, e: i64, shard: Option<i32>, why: &str) -> GapRow {
    GapRow { start_ms: s, end_ms: e, shard, reason: why.into() }
}

fn depth(ts: i64, idx: u32, side: u8, level: u16, p: &str, s: &str) -> DepthRow {
    DepthRow { ts_ms: ts, token_idx: idx, side, level, price: m(p), size: m(s) }
}

#[test]
fn the_golden_hour_produces_exactly_the_expected_rows() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "100"), ("0.39", "50")], &[("0.42", "30")])),
        frame(T0 + 3_000, 0, book("y2", T0 + 2_995, &[("0.55", "10")], &[])),
        frame(T0 + 4_000, 0, change("y1", T0 + 3_995, "SELL", "0.42", "0", "0.40", "1")),
        frame(T0 + 5_000, 0, change("y1", T0 + 4_995, "BUY", "0.40", "100", "0.40", "1")), // same top: no row
        frame(T0 + 301_000, 0, change("y2", T0 + 300_995, "BUY", "0.56", "5", "0.56", "1")), // crosses the 5-min boundary first
        down(T0 + 400_000, 0),
        frame(T0 + 401_000, 0, change("y1", T0 + 400_995, "BUY", "0.41", "7", "0.41", "1")), // untrusted: no row
        frame(T0 + 402_000, 0, book("y1", T0 + 401_995, &[("0.41", "7")], &[("0.43", "9")])),
    ], &[r.clone()], &mut s);
    let stats = red.end_hour(&mut s).unwrap();
    assert_eq!(s.tob, vec![
        tob(T0 + 1_990, T0 + 2_000, 0, Some(("0.40", "100")), Some(("0.42", "30"))),
        tob(T0 + 2_995, T0 + 3_000, 1, Some(("0.55", "10")), None),
        tob(T0 + 3_995, T0 + 4_000, 0, Some(("0.40", "100")), None),
        tob(T0 + 300_995, T0 + 301_000, 1, Some(("0.56", "5")), None),
        tob(T0 + 401_995, T0 + 402_000, 0, Some(("0.41", "7")), Some(("0.43", "9"))),
    ]);
    assert_eq!(s.depth, vec![
        depth(T0 + 300_000, 0, SIDE_BID, 0, "0.40", "100"),
        depth(T0 + 300_000, 0, SIDE_BID, 1, "0.39", "50"),
        depth(T0 + 300_000, 1, SIDE_BID, 0, "0.55", "10"),
    ]);
    assert_eq!(s.gaps, vec![
        gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
        gap(T0 + 1_000, T0 + 3_000, Some(0), reason::NO_BOOK_YET),
        gap(T0 + 400_000, T0 + H, Some(0), reason::DOWN),
    ]);
    assert!(s.l2.is_empty());
    let toks = red.tokens();
    assert_eq!(toks.len(), 2);
    assert_eq!((toks[0].token_id.as_str(), toks[0].market_id.as_str(), toks[0].event_id.as_str(), toks[0].outcome.as_str(), toks[0].neg_risk), ("y1", "m-y1", "e1", "yes", true));
    assert_eq!((stats.lines, stats.bad_lines, stats.ts_clamped), (9, 0, 0));
}

#[test]
fn a_lost_hour_becomes_a_raw_hour_lost_gap_and_untrusts_every_book() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
        frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "1")], &[])),
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    let mut s1 = VecSink::default();
    red.begin_hour(T0 + H, vec![]);
    red.missing_hour(reason::RAW_HOUR_LOST, &mut s1).unwrap();
    red.end_hour(&mut s1).unwrap();
    assert_eq!(s1.gaps, vec![gap(T0 + H, T0 + 2 * H, None, reason::RAW_HOUR_LOST)]);
    let mut s2 = VecSink::default();
    red.begin_hour(T0 + 2 * H, vec![]);
    run(&mut red, &[
        frame(T0 + 2 * H + 500, 0, change("y1", T0 + 2 * H + 495, "BUY", "0.41", "1", "0.41", "1")), // stale book: no row
        frame(T0 + 2 * H + 1_000, 0, book("y1", T0 + 2 * H + 995, &[("0.42", "1")], &[])),
    ], &[r.clone()], &mut s2);
    red.end_hour(&mut s2).unwrap();
    assert_eq!(s2.tob.len(), 1);
    assert_eq!(s2.tob[0].bid, Some(m("0.42")));
    assert_eq!(s2.gaps, vec![
        gap(T0 + 2 * H, T0 + 2 * H + 500, None, reason::RAW_HOUR_LOST),
        gap(T0 + 2 * H + 500, T0 + 3 * H, Some(0), reason::NO_BOOK_YET),
    ]);
}

#[test]
fn shutdown_is_scanner_stopped_until_the_next_line_then_books_wait_for_snapshots() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
        shutdown(T0 + 10_000),
        uni(T0 + 20_000, &r),
        frame(T0 + 21_000, 0, change("y1", T0 + 20_995, "BUY", "0.41", "1", "0.41", "1")),
        frame(T0 + 22_000, 0, book("y1", T0 + 21_995, &[("0.45", "1")], &[])),
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    assert_eq!(s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(), vec![Some(m("0.40")), Some(m("0.45"))]);
    assert_eq!(s.gaps, vec![
        gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
        gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
        gap(T0 + 10_001, T0 + 20_000, None, reason::SCANNER_STOPPED),
        gap(T0 + 20_000, T0 + 22_000, Some(0), reason::NO_BOOK_YET),
    ]);
}

#[test]
fn a_group_losing_its_connection_stops_rows_until_a_new_snapshot() {
    let with = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let without = rec("u2", T0 + 60_000, &[("e1", &["y1"])], &[]);
    let back = rec("u3", T0 + 120_000, &[("e1", &["y1"])], &[("e1", 3)]);
    let recs = [with.clone(), without.clone(), back.clone()];
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &with),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
        uni(T0 + 60_000, &without),
        frame(T0 + 61_000, 0, change("y1", T0 + 60_995, "BUY", "0.41", "1", "0.41", "1")), // reset book: no row
        uni(T0 + 120_000, &back),
        frame(T0 + 121_000, 3, change("y1", T0 + 120_995, "BUY", "0.42", "1", "0.42", "1")), // still untrusted
        frame(T0 + 122_000, 3, book("y1", T0 + 121_995, &[("0.43", "1")], &[])),
    ], &recs, &mut s);
    red.end_hour(&mut s).unwrap();
    assert_eq!(s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(), vec![Some(m("0.40")), Some(m("0.43"))]);
    assert!(s.gaps.contains(&gap(T0 + 120_000, T0 + 122_000, Some(3), reason::NO_BOOK_YET)), "{:?}", s.gaps);
}

#[test]
fn depth_covers_only_tokens_changed_since_their_last_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0), ("zz", 1)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
        frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "1")], &[])),
        frame(T0 + 300_500, 0, change("y1", T0 + 300_495, "BUY", "0.41", "1", "0.41", "1")),
        frame(T0 + 600_500, 0, change("y1", T0 + 600_495, "BUY", "0.42", "1", "0.42", "1")),
        frame(T0 + 900_500, 1, trade("other", T0 + 900_495)),   // y1 changed at 600 500: snapshot at 900 000
        frame(T0 + 1_200_500, 1, trade("other", T0 + 1_200_495)), // nothing changed since: no snapshot
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    let mut at: Vec<(i64, u32)> = s.depth.iter().map(|d| (d.ts_ms - T0, d.token_idx)).collect();
    at.dedup(); // one entry per (snapshot, token); a token has up to depth_levels rows per side
    assert_eq!(at, vec![(300_000, 0), (300_000, 1), (600_000, 0), (900_000, 0)]);
}

#[test]
fn tob_ts_is_clamped_non_decreasing_per_token() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 6_000, 0, book("y1", T0 + 5_000, &[("0.40", "1")], &[])),
        frame(T0 + 7_000, 0, change("y1", T0 + 4_000, "BUY", "0.41", "1", "0.41", "1")),
    ], &[r.clone()], &mut s);
    let st = red.end_hour(&mut s).unwrap();
    assert_eq!(s.tob.iter().map(|t| t.ts_ms).collect::<Vec<_>>(), vec![T0 + 5_000, T0 + 5_000]);
    assert_eq!(st.ts_clamped, 1);
}

#[test]
fn the_token_dictionary_restarts_each_utc_day() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0 + 23 * H, vec![]);
    run(&mut red, &[
        uni(T0 + 23 * H + 1_000, &r),
        frame(T0 + 23 * H + 2_000, 0, book("y2", T0 + 23 * H + 1_990, &[("0.20", "1")], &[])),
        frame(T0 + 23 * H + 3_000, 0, book("y1", T0 + 23 * H + 2_990, &[("0.40", "1")], &[])),
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    assert_eq!(red.tokens().iter().map(|t| t.token_id.as_str()).collect::<Vec<_>>(), vec!["y2", "y1"]);
    let mut s2 = VecSink::default();
    red.begin_hour(T0 + 24 * H, vec![]);
    assert!(red.tokens().is_empty(), "a new day, a new dictionary");
    run(&mut red, &[frame(T0 + 24 * H + 1_000, 0, change("y1", T0 + 24 * H + 995, "BUY", "0.41", "1", "0.41", "1"))], &[r.clone()], &mut s2);
    red.end_hour(&mut s2).unwrap();
    assert_eq!(s2.tob[0].token_idx, 0);
    assert_eq!(red.tokens()[0].token_id, "y1");
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p compact --test reduce`
Expected: FAIL (no `compact::reduce`).

- [ ] **Step 4: The window type and the reducer**

`bins/compact/src/windows.rs`:
```rust
//! l2 windows around net-positive arbitrage episodes (spec 5b §3). Task 13 adds the math.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub episode_id: String,
    pub event_id: String,
    /// The episode's open (server ms): windows are ordered by it, then by id.
    pub opened_ms: i64,
    pub start_ms: i64,
    /// Exclusive.
    pub end_ms: i64,
    /// Capped at l2_max_window_secs (or still open when read).
    pub truncated: bool,
}
```
In `bins/compact/src/config.rs` add to `impl Config`:
```rust
    pub fn reduce(&self) -> crate::reduce::ReduceCfg {
        crate::reduce::ReduceCfg {
            depth_every_ms: self.depth_every_secs.saturating_mul(1_000),
            depth_levels: self.depth_levels,
        }
    }
```
`bins/compact/src/lib.rs`: add `pub mod reduce;` and `pub mod windows;`.

`bins/compact/src/reduce.rs`:
```rust
//! One raw hour → archive rows (spec 5b §3, §4.1). Pure: no files, no database. Lines go through
//! tape::feed::FeedBooks, the scanner's own book rules (spec §2). On top of it this keeps which
//! books are trusted (untrusted from a disconnect, a restart, a missing or lost stretch, or a
//! token's first appearance, until the feed sends it a full snapshot: no row is ever made from a
//! stale book), the day's token dictionary, top-of-book changes, periodic depth, and the gaps.
use crate::rows::{DepthRow, GapRow, SIDE_ASK, SIDE_BID, Sink, TobRow, TokenRow, micros, reason};
use crate::windows::Window;
use anyhow::Context;
use arb_core::book::Top;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tape::feed::{Applied, FeedBooks, FeedState, LineEvent};
use tape::line::Line;
use tape::time::{HOUR_MS, day_of};
use tape::universe_view::RecordView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReduceCfg {
    pub depth_every_ms: i64,
    pub depth_levels: usize,
}

/// A top of book in micro-units: ((bid, bid size), (ask, ask size)).
pub type TopMicros = (Option<(i64, i64)>, Option<(i64, i64)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastTop {
    pub top: TopMicros,
    pub ts_ms: i64,
}

/// A shard's tokens waiting for a snapshot since `since_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenGap {
    pub reason: String,
    pub since_ms: i64,
    pub waiting: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourStats {
    pub lines: u64,
    pub bad_lines: u64,
    pub ts_clamped: u64,
    pub universe_missing: u64,
}

/// What a checkpoint carries from one hour to the next (spec §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReducerState {
    pub feed: FeedState,
    pub started: bool,
    pub trusted: BTreeSet<String>,
    pub dict_day_ms: i64,
    pub dict: Vec<TokenRow>,
    pub last_top: BTreeMap<String, LastTop>,
    pub changed: BTreeSet<String>,
    pub next_depth_ms: Option<i64>,
    pub clock_ms: i64,
    pub shard_gaps: BTreeMap<usize, OpenGap>,
    pub global_gap: Option<(String, i64)>,
}

pub struct Reducer {
    cfg: ReduceCfg,
    feed: FeedBooks,
    started: bool,
    trusted: BTreeSet<String>,
    dict_day_ms: i64,
    dict: Vec<TokenRow>,
    idx: HashMap<String, u32>,
    last_top: BTreeMap<String, LastTop>,
    changed: BTreeSet<String>,
    next_depth_ms: Option<i64>,
    clock_ms: i64,
    shard_gaps: BTreeMap<usize, OpenGap>,
    global_gap: Option<(String, i64)>,
    // per hour, never checkpointed
    hour_ms: i64,
    windows: Vec<Window>,
    stats: HourStats,
}

fn top_micros(t: Top) -> anyhow::Result<TopMicros> {
    let pair = |x: Option<(Price, Shares)>| -> anyhow::Result<Option<(i64, i64)>> {
        x.map(|(p, s)| Ok((micros(p.get())?, micros(s.get())?)))
            .transpose()
    };
    Ok((pair(t.best_bid)?, pair(t.best_ask)?))
}

/// Emit a gap clipped to the hour being built: every hour's parts stand on their own.
fn gap(
    hour_ms: i64,
    start: i64,
    end: i64,
    shard: Option<usize>,
    why: &str,
    sink: &mut dyn Sink,
) -> anyhow::Result<()> {
    let (s, e) = (start.max(hour_ms), end.min(hour_ms + HOUR_MS));
    if e > s {
        sink.gap(GapRow {
            start_ms: s,
            end_ms: e,
            shard: shard.map(|c| i32::try_from(c).unwrap_or(i32::MAX)),
            reason: why.to_string(),
        })?;
    }
    Ok(())
}

impl Reducer {
    pub fn new(cfg: ReduceCfg) -> Reducer {
        Reducer {
            cfg,
            feed: FeedBooks::new(),
            started: false,
            trusted: BTreeSet::new(),
            dict_day_ms: i64::MIN,
            dict: Vec::new(),
            idx: HashMap::new(),
            last_top: BTreeMap::new(),
            changed: BTreeSet::new(),
            next_depth_ms: None,
            clock_ms: i64::MIN,
            shard_gaps: BTreeMap::new(),
            global_gap: None,
            hour_ms: 0,
            windows: Vec::new(),
            stats: HourStats::default(),
        }
    }

    pub fn from_state(cfg: ReduceCfg, s: ReducerState) -> anyhow::Result<Reducer> {
        let mut r = Reducer::new(cfg);
        r.feed = FeedBooks::from_state(&s.feed).context("checkpoint books")?;
        r.started = s.started;
        r.trusted = s.trusted;
        r.dict_day_ms = s.dict_day_ms;
        r.idx = s
            .dict
            .iter()
            .map(|t| (t.token_id.clone(), t.token_idx))
            .collect();
        r.dict = s.dict;
        r.last_top = s.last_top;
        r.changed = s.changed;
        r.next_depth_ms = s.next_depth_ms;
        r.clock_ms = s.clock_ms;
        r.shard_gaps = s.shard_gaps;
        r.global_gap = s.global_gap;
        Ok(r)
    }

    pub fn state(&self) -> ReducerState {
        ReducerState {
            feed: self.feed.to_state(),
            started: self.started,
            trusted: self.trusted.clone(),
            dict_day_ms: self.dict_day_ms,
            dict: self.dict.clone(),
            last_top: self.last_top.clone(),
            changed: self.changed.clone(),
            next_depth_ms: self.next_depth_ms,
            clock_ms: self.clock_ms,
            shard_gaps: self.shard_gaps.clone(),
            global_gap: self.global_gap.clone(),
        }
    }

    /// The day's token dictionary so far (index = token_idx).
    pub fn tokens(&self) -> &[TokenRow] {
        &self.dict
    }

    /// The latest receive time seen (ms).
    pub fn clock_ms(&self) -> i64 {
        self.clock_ms
    }

    pub fn count_bad_line(&mut self) {
        self.stats.bad_lines += 1;
    }

    pub fn begin_hour(&mut self, hour_ms: i64, mut windows: Vec<Window>) {
        self.hour_ms = hour_ms;
        let day = day_of(hour_ms);
        if day != self.dict_day_ms {
            self.dict.clear();
            self.idx.clear();
            self.dict_day_ms = day;
        }
        windows.sort_by(|a, b| {
            a.opened_ms
                .cmp(&b.opened_ms)
                .then_with(|| a.episode_id.cmp(&b.episode_id))
        });
        self.windows = windows;
        self.stats = HourStats::default();
        if !self.started {
            // cold start: nothing is known until the first line (and books until snapshots)
            self.started = true;
            self.global_gap = Some((reason::NO_BOOK_YET.to_string(), hour_ms));
        }
    }

    /// One capture line of the hour, in file order.
    pub fn line(
        &mut self,
        line: &Line,
        load: &mut dyn FnMut(i64, &str) -> anyhow::Result<RecordView>,
        sink: &mut dyn Sink,
    ) -> anyhow::Result<()> {
        let r = line.recv_ts_us.div_euclid(1000);
        self.stats.lines += 1;
        self.clock_ms = self.clock_ms.max(r);
        if let Some((why, since)) = self.global_gap.take() {
            gap(self.hour_ms, since, r, None, &why, sink)?;
            self.wait_for_untrusted(r);
        }
        self.depth_due(sink)?;
        let ap = self.feed.apply(line, load, &|_| false);
        self.on_event(&ap, r, sink)?;
        self.rows(&ap, r, sink)
    }

    fn rows(&mut self, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        for t in &ap.snapshotted {
            self.retrust(t, r, sink)?;
        }
        for t in &ap.touched {
            if self.trusted.contains(t) {
                self.changed.insert(t.clone());
                self.tob(t, r, sink)?;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        match &ap.event {
            LineEvent::Down(conn) => {
                for t in self.feed.tokens_on(*conn) {
                    self.trusted.remove(&t);
                    self.wait(*conn, &t, reason::DOWN, r);
                }
            }
            LineEvent::Universe(ch) => {
                for t in &ch.gone {
                    self.forget(t, r, sink)?;
                }
                for t in ch.appeared.iter().chain(ch.reset.iter()) {
                    self.trusted.remove(t);
                }
                self.wait_for_untrusted(r);
            }
            LineEvent::UniverseMissing(_) => {
                self.stats.universe_missing += 1;
                self.trusted.clear();
                self.wait_for_untrusted(r);
            }
            LineEvent::Shutdown => {
                self.close_shard_gaps(r, sink)?;
                self.trusted.clear();
                if self.global_gap.is_none() {
                    self.global_gap = Some((reason::SCANNER_STOPPED.to_string(), r.saturating_add(1)));
                }
            }
            LineEvent::BadLine => self.stats.bad_lines += 1,
            LineEvent::None | LineEvent::Frame => {}
        }
        Ok(())
    }

    fn wait(&mut self, conn: usize, t: &str, why: &str, r: i64) {
        self.shard_gaps
            .entry(conn)
            .or_insert_with(|| OpenGap {
                reason: why.to_string(),
                since_ms: r,
                waiting: BTreeSet::new(),
            })
            .waiting
            .insert(t.to_string());
    }

    /// Every visible, untrusted token with a connection waits for its snapshot.
    fn wait_for_untrusted(&mut self, r: i64) {
        for t in self.feed.visible_tokens() {
            if self.trusted.contains(&t) {
                continue;
            }
            if let Some(c) = self.feed.conn(&t) {
                self.wait(c, &t, reason::NO_BOOK_YET, r);
            }
        }
    }

    fn stop_waiting(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let mut closed = Vec::new();
        for (c, g) in self.shard_gaps.iter_mut() {
            if g.waiting.remove(t) && g.waiting.is_empty() {
                closed.push(*c);
            }
        }
        for c in closed {
            if let Some(g) = self.shard_gaps.remove(&c) {
                gap(self.hour_ms, g.since_ms, r, Some(c), &g.reason, sink)?;
            }
        }
        Ok(())
    }

    fn retrust(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if self.trusted.insert(t.to_string()) {
            self.changed.insert(t.to_string());
            self.stop_waiting(t, r, sink)?;
        }
        Ok(())
    }

    fn forget(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        self.trusted.remove(t);
        self.changed.remove(t);
        self.stop_waiting(t, r, sink)
    }

    fn close_shard_gaps(&mut self, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        for (c, g) in std::mem::take(&mut self.shard_gaps) {
            gap(self.hour_ms, g.since_ms, at, Some(c), &g.reason, sink)?;
        }
        Ok(())
    }

    fn index(&mut self, t: &str) -> anyhow::Result<u32> {
        if let Some(&i) = self.idx.get(t) {
            return Ok(i);
        }
        let meta = self
            .feed
            .meta(t)
            .cloned()
            .with_context(|| format!("token {t} has no metadata"))?;
        let i = u32::try_from(self.dict.len()).context("token dictionary overflow")?;
        self.dict.push(TokenRow {
            token_idx: i,
            token_id: t.to_string(),
            market_id: meta.market_id,
            event_id: meta.event_id,
            outcome: meta.outcome,
            neg_risk: meta.neg_risk,
        });
        self.idx.insert(t.to_string(), i);
        Ok(i)
    }

    fn tob(&mut self, t: &str, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let top = top_micros(book.top())?;
        let ts = book.server_ts_ms();
        let prev = self.last_top.get(t).copied();
        if prev.is_some_and(|p| p.top == top) {
            return Ok(());
        }
        let ts_ms = match prev {
            Some(p) if ts < p.ts_ms => {
                self.stats.ts_clamped += 1;
                p.ts_ms
            }
            _ => ts,
        };
        let token_idx = self.index(t)?;
        sink.tob(TobRow {
            ts_ms,
            recv_ms: r,
            token_idx,
            bid: top.0.map(|x| x.0),
            bid_sz: top.0.map(|x| x.1),
            ask: top.1.map(|x| x.0),
            ask_sz: top.1.map(|x| x.1),
        })?;
        self.last_top.insert(t.to_string(), LastTop { top, ts_ms });
        Ok(())
    }

    /// At each depth boundary the receive clock crosses: the books as of that boundary.
    fn depth_due(&mut self, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let every = self.cfg.depth_every_ms;
        let Some(next) = self.next_depth_ms else {
            self.next_depth_ms = Some((self.clock_ms.div_euclid(every) + 1) * every);
            return Ok(());
        };
        if self.clock_ms < next {
            return Ok(());
        }
        let at = self.clock_ms.div_euclid(every) * every;
        let toks: Vec<String> = self
            .changed
            .iter()
            .filter(|t| self.trusted.contains(*t))
            .cloned()
            .collect();
        for t in toks {
            self.depth_rows(&t, at, sink)?;
        }
        self.changed.clear();
        self.next_depth_ms = Some(at + every);
        Ok(())
    }

    fn depth_rows(&mut self, t: &str, at: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let n = self.cfg.depth_levels;
        let sides = [
            (SIDE_BID, book.bids_best_first().into_iter().take(n).collect::<Vec<_>>()),
            (SIDE_ASK, book.asks_best_first().into_iter().take(n).collect::<Vec<_>>()),
        ];
        let token_idx = self.index(t)?;
        for (side, levels) in sides {
            for (level, (p, s)) in levels.into_iter().enumerate() {
                sink.depth(DepthRow {
                    ts_ms: at,
                    token_idx,
                    side,
                    level: u16::try_from(level)?,
                    price: micros(p.get())?,
                    size: micros(s.get())?,
                })?;
            }
        }
        Ok(())
    }

    /// The whole hour has no raw file: `raw_hour_lost` (recorded lost) or `scanner_stopped`.
    pub fn missing_hour(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let h = self.hour_ms;
        self.close_shard_gaps(h, sink)?;
        self.trusted.clear();
        match &self.global_gap {
            Some((w, _)) if w == why => {}
            _ => self.global_gap = Some((why.to_string(), h)),
        }
        self.clock_ms = self.clock_ms.max(h + HOUR_MS - 1);
        self.next_depth_ms = None;
        Ok(())
    }

    /// The raw file can't be read past the last line: the rest of the hour is `why`.
    pub fn cut(&mut self, why: &str, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let at = self.clock_ms.saturating_add(1).max(self.hour_ms);
        self.close_shard_gaps(at, sink)?;
        self.trusted.clear();
        if self.global_gap.is_none() {
            self.global_gap = Some((why.to_string(), at));
        }
        self.next_depth_ms = None;
        Ok(())
    }

    /// Flush every open gap up to the hour's end (they stay open) and return the hour's counts.
    pub fn end_hour(&mut self, sink: &mut dyn Sink) -> anyhow::Result<HourStats> {
        let end = self.hour_ms + HOUR_MS;
        if let Some((why, since)) = self.global_gap.clone() {
            gap(self.hour_ms, since, end, None, &why, sink)?;
        }
        let open: Vec<(usize, OpenGap)> = self
            .shard_gaps
            .iter()
            .map(|(c, g)| (*c, g.clone()))
            .collect();
        for (c, g) in open {
            gap(self.hour_ms, g.since_ms, end, Some(c), &g.reason, sink)?;
        }
        Ok(std::mem::take(&mut self.stats))
    }
}
```
(`windows` is stored but unused until Task 13; if clippy flags it as never read, add `#[allow(dead_code)]` on that one field with the comment `// used by l2 (Task 13)` and remove it in Task 13.)

- [ ] **Step 5: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the seven reducer tests.

- [ ] **Step 6: Commit**

```bash
git add bins/compact/src/lib.rs bins/compact/src/config.rs bins/compact/src/reduce.rs bins/compact/src/windows.rs bins/compact/tests/fixture/mod.rs bins/compact/tests/reduce.rs
git commit -m "compact: reducer (trusted books, top of book, depth, gaps)"
```

---

### Task 12: the `tob` agreement: every row the compactor emits equals the engine's top at that line

Spec §8 "the `tob` stream equals the top of book that `scanner replay` reconstructs at every change". Task 4 proves the books equal; this proves the rows built on them: the Task 4 fixture hour is replayed through the engine and the compactor's `Reducer` side by side, and every `tob` row the reducer emits for a line must equal (price and size, both sides) the engine's top of that token after that very line. The `#[ignore]` variant runs on a real capture directory (runbook §3). The scanner gets `compact` as a **dev**-dependency only (the arch test and `deny.toml` look at normal dependencies; `compact` does not depend on the scanner, so there is no cycle).

**Files:**
- Create: `bins/scanner/tests/tob_agreement.rs`
- Modify: `bins/scanner/Cargo.toml` (`[dev-dependencies]` += `compact = { path = "../compact" }`)

**Interfaces:**
- Consumes: Task 4 (the `agreement` test module: `fixture`, `tmp`, `ctx`), Task 11 (`Reducer`, `ReduceCfg`), Task 9 (`VecSink`, `micros`).

- [ ] **Step 1: Write the test**

`bins/scanner/tests/tob_agreement.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8: every tob row the compactor emits equals the scanner engine's top of book for that
//! token after that very line (the Task 4 fixture, and #[ignore] a real capture: TAPE_AGREEMENT_DIR).
mod agreement;
use agreement::*;
use arb_core::money::{Price, Shares};
use compact::reduce::{ReduceCfg, Reducer};
use compact::rows::{VecSink, micros};
use scanner::engine::Engine;
use scanner::replay::{ReplayState, apply_line};
use std::path::Path;
use tape::universe_view::load_record;

type Side = Option<(i64, i64)>;

fn side(x: Option<(Price, Shares)>) -> Side {
    x.map(|(p, s)| (micros(p.get()).unwrap(), micros(s.get()).unwrap()))
}

/// Returns the number of tob rows checked.
fn check(dir: &Path) -> usize {
    let mut engine = Engine::new(ctx());
    let mut st = ReplayState::default();
    let mut red = Reducer::new(ReduceCfg { depth_every_ms: 300_000, depth_levels: 10 });
    let mut load = |taken: i64, file: &str| load_record(dir, taken, file);
    let mut rows = 0usize;
    for h in tape::files::listing(dir).hours {
        red.begin_hour(h.start_ms, vec![]);
        tape::files::for_each_line(&h.path, |l| {
            apply_line(&mut engine, dir, &l, &mut st, &mut |_| {})?;
            let mut sink = VecSink::default();
            red.line(&l, &mut load, &mut sink)?;
            for row in &sink.tob {
                let tok = red.tokens()[usize::try_from(row.token_idx)?].token_id.clone();
                let top = engine
                    .book_of(&tok)
                    .map(|b| b.top())
                    .unwrap_or_else(|| panic!("the engine holds no book for {tok}"));
                assert_eq!(
                    (row.bid.zip(row.bid_sz), row.ask.zip(row.ask_sz)),
                    (side(top.best_bid), side(top.best_ask)),
                    "tob row for {tok} at recv_ms {} differs from the engine's top",
                    row.recv_ms
                );
            }
            rows += sink.tob.len();
            Ok(())
        })
        .unwrap();
        red.end_hour(&mut VecSink::default()).unwrap();
    }
    rows
}

#[test]
fn every_tob_row_equals_the_engines_top_at_that_line() {
    let dir = tmp("tob");
    fixture(&dir);
    let n = check(&dir);
    assert!(n > 100, "{n} tob rows checked");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore]
fn every_tob_row_equals_the_engines_top_on_a_real_capture() {
    let Ok(dir) = std::env::var("TAPE_AGREEMENT_DIR") else {
        eprintln!("SKIPPED: TAPE_AGREEMENT_DIR not set");
        return;
    };
    eprintln!("tob agreement: {} rows equal the engine's top", check(Path::new(&dir)));
}
```
and in `bins/scanner/Cargo.toml` `[dev-dependencies]` add `compact = { path = "../compact" }`.

- [ ] **Step 2: Run it**

Run: `cargo test -p scanner --test tob_agreement && cargo test -p scanner --test arch && cargo clippy -p scanner --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS (well over 100 rows checked; the arch test still passes: a dev-dependency is not a normal one). A difference is a reducer bug (Task 11): which books are trusted, or how the top is read; fix it there.

- [ ] **Step 3: Commit**

```bash
git add bins/scanner/tests/tob_agreement.rs bins/scanner/Cargo.toml Cargo.lock
git commit -m "scanner: tob agreement, every compactor tob row equals the engine's top at its line"
```

---

### Task 13: `compact`: l2 windows around net-positive arbitrage episodes

Spec §3: an episode's window runs from `l2_before_secs` before it opens to `l2_after_secs` after it closes, capped at `l2_max_window_secs` from its start (flagged truncated; a still-open episode is capped the same way, spec §4.2). Inside a window, each token of the episode's event starts with `snapshot = true` rows (one per level) and then gets its exact level changes; overlapping windows merge so a row belongs to its first episode (by open time, then id), and a token whose owner changes gets a fresh snapshot under the new owner. Ruling R5: every hour restarts each active window with snapshots (so each hour's part, and each day across midnight, stands alone), and an empty book is snapshotted by its first non-empty state.

**Files:**
- Modify: `bins/compact/src/windows.rs` (window math), `bins/compact/src/reduce.rs` (owners, l2 rows)
- Create: `bins/compact/tests/l2.rs`

**Interfaces:**
- Consumes: Task 5 (`store::CompactEpisode`), Task 11 (`Reducer`, `Window`).
- Produces: `compact::windows::{window_of(&CompactEpisode, before_ms: i64, after_ms: i64, max_ms: i64) -> Window, windows_for_hour(&[CompactEpisode], hour_ms: i64, &Config) -> Vec<Window>, query_range(hour_ms: i64, &Config) -> (i64, i64)}`; the reducer now emits `L2Row`s.

- [ ] **Step 1: Write the failing tests**

`bins/compact/tests/l2.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::reduce::Reducer;
use compact::rows::*;
use compact::windows::*;
use fixture::*;
use std::path::Path;
use store::CompactEpisode;

fn ep(id: &str, opened: i64, closed: Option<i64>) -> CompactEpisode {
    CompactEpisode { episode_id: id.into(), event_id: "e1".into(), opened_ms: opened, closed_ms: closed }
}

fn win(id: &str, start: i64, end: i64) -> Window {
    Window { episode_id: id.into(), event_id: "e1".into(), opened_ms: start + 300_000, start_ms: start, end_ms: end, truncated: false }
}

fn l2(ts: i64, recv: i64, idx: u32, side: u8, p: &str, s: &str, snapshot: bool, id: &str) -> L2Row {
    L2Row { ts_ms: ts, recv_ms: recv, token_idx: idx, side, price: m(p), new_size: m(s), snapshot, episode_id: id.into() }
}

#[test]
fn window_math_follows_the_spec() {
    let (b, a, max) = (300_000, 300_000, 1_800_000);
    let w = window_of(&ep("x", T0 + 1_000_000, Some(T0 + 1_060_000)), b, a, max);
    assert_eq!((w.start_ms, w.end_ms, w.truncated), (T0 + 700_000, T0 + 1_360_000, false));
    let long = window_of(&ep("y", T0 + 1_000_000, Some(T0 + 3_000_000)), b, a, max);
    assert_eq!((long.end_ms, long.truncated), (T0 + 700_000 + max, true));
    let open = window_of(&ep("z", T0 + 1_000_000, None), b, a, max);
    assert_eq!((open.end_ms, open.truncated), (T0 + 700_000 + max, true));
    let c = cfg(Path::new("/r"), Path::new("/a"));
    assert_eq!(query_range(T0 + H, &c), (T0 + H - 1_800_000 + 300_000, T0 + 2 * H + 300_000));
    let eps = [
        ep("late", T0 + H + 400_000, None),   // window starts after this hour
        ep("b", T0 + 10_000, Some(T0 + 20_000)),
        ep("a", T0 + 10_000, Some(T0 + 20_000)),
        ep("before", T0 - 3_000_000, Some(T0 - 2_900_000)), // ended before this hour
    ];
    let ws = windows_for_hour(&eps, T0, &c);
    assert_eq!(ws.iter().map(|w| w.episode_id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
}

#[test]
fn an_l2_window_starts_with_snapshot_rows_then_exact_changes() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 200_000)]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "10")])),
        frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "3")], &[])),
        frame(T0 + 50_000, 0, change("y1", T0 + 49_995, "BUY", "0.39", "1", "0.40", "0.45")), // before the window
        frame(T0 + 100_500, 0, change("y1", T0 + 100_495, "BUY", "0.41", "2", "0.41", "0.46")), // snapshots, then this change
        frame(T0 + 150_000, 0, change("y1", T0 + 149_995, "BUY", "0.38", "4", "0.41", "0.46")), // settles 0.46: 0.45 trimmed
        frame(T0 + 200_500, 0, change("y1", T0 + 200_495, "BUY", "0.37", "1", "0.41", "0.46")), // after the window
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    let (t1, t2) = (T0 + 100_500, T0 + 150_000);
    assert_eq!(s.l2, vec![
        l2(T0 + 49_995, t1, 0, SIDE_BID, "0.40", "10", true, "ep"),
        l2(T0 + 49_995, t1, 0, SIDE_BID, "0.39", "1", true, "ep"),
        l2(T0 + 49_995, t1, 0, SIDE_ASK, "0.45", "10", true, "ep"),
        l2(T0 + 2_090, t1, 1, SIDE_BID, "0.20", "3", true, "ep"),
        l2(T0 + 100_495, t1, 0, SIDE_BID, "0.41", "2", false, "ep"),
        l2(T0 + 149_995, t2, 0, SIDE_BID, "0.38", "4", false, "ep"),
        l2(T0 + 149_995, t2, 0, SIDE_ASK, "0.45", "0", false, "ep"),
    ]);
}

#[test]
fn overlapping_windows_merge_and_rows_belong_to_the_first_episode() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("b", T0 + 150_000, T0 + 300_000), win("a", T0 + 100_000, T0 + 200_000)]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "10")], &[])),
        frame(T0 + 160_000, 0, change("y1", T0 + 159_995, "BUY", "0.41", "1", "0.41", "1")),
        frame(T0 + 210_000, 0, change("y1", T0 + 209_995, "BUY", "0.42", "1", "0.42", "1")),
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    let ids: Vec<(&str, bool)> = s.l2.iter().map(|x| (x.episode_id.as_str(), x.snapshot)).collect();
    assert_eq!(ids, vec![
        ("a", true),             // a starts first: its snapshot at 160 000
        ("a", false),            // 0.41 belongs to a (b overlaps, a opened first)
        ("b", true), ("b", true), // a ended at 200 000: b takes over with a fresh snapshot (0.41, 0.40)
        ("b", false),            // 0.42
    ]);
}

#[test]
fn an_untrusted_or_empty_book_in_a_window_starts_with_its_first_real_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(&mut red, &[
        uni(T0 + 1_000, &r),
        frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "10")], &[])),
        frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[], &[])), // empty book
        down(T0 + 50_000, 0),                                    // y1, y2 untrusted
        frame(T0 + 100_500, 0, book("y2", T0 + 100_495, &[], &[])), // trusted, still empty: nothing
        frame(T0 + 101_000, 0, change("y2", T0 + 100_995, "BUY", "0.20", "3", "0.20", "1")), // first level: a snapshot
        frame(T0 + 102_000, 0, change("y1", T0 + 101_995, "BUY", "0.41", "1", "0.41", "1")), // untrusted: nothing
        frame(T0 + 103_000, 0, book("y1", T0 + 102_995, &[("0.42", "1")], &[])),            // its snapshot
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    let rows: Vec<(u32, bool, i64)> = s.l2.iter().map(|x| (x.token_idx, x.snapshot, x.price)).collect();
    let (y1, y2) = (0, 1);
    assert_eq!(rows, vec![(y2, true, m("0.20")), (y1, true, m("0.42"))]);
}

#[test]
fn a_window_across_midnight_restarts_with_snapshots_in_the_new_day() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let d = T0 + 23 * H;
    let w = Window { episode_id: "ep".into(), event_id: "e1".into(), opened_ms: d + 3_300_000, start_ms: d + 3_000_000, end_ms: d + H + 600_000, truncated: false };
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(d, vec![w.clone()]);
    run(&mut red, &[
        uni(d + 1_000, &r),
        frame(d + 2_000, 0, book("y1", d + 1_990, &[("0.40", "10")], &[])),
        frame(d + 3_100_000, 0, change("y1", d + 3_099_995, "BUY", "0.41", "1", "0.41", "1")),
    ], &[r.clone()], &mut s);
    red.end_hour(&mut s).unwrap();
    assert_eq!(s.l2.iter().map(|x| x.snapshot).collect::<Vec<_>>(), vec![true, false]);
    let mut s2 = VecSink::default();
    red.begin_hour(d + H, vec![w]);
    run(&mut red, &[frame(d + H + 5_000, 0, change("y1", d + H + 4_995, "BUY", "0.42", "1", "0.42", "1"))], &[r.clone()], &mut s2);
    red.end_hour(&mut s2).unwrap();
    let rows: Vec<(u32, bool, i64)> = s2.l2.iter().map(|x| (x.token_idx, x.snapshot, x.price)).collect();
    assert_eq!(rows, vec![(0, true, m("0.41")), (0, true, m("0.40")), (0, false, m("0.42"))], "a new day: new dictionary, fresh snapshot first");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test l2`
Expected: FAIL (no `window_of`; then, after Step 3, l2 rows are empty).

- [ ] **Step 3: The window math**

Append to `bins/compact/src/windows.rs`:
```rust
use crate::config::Config;
use store::CompactEpisode;
use tape::time::HOUR_MS;

/// [open − before, close + after), capped at `max_ms` from its start. A still-open episode is
/// capped the same way (spec §4.2: an hour never waits for an episode to close).
pub fn window_of(ep: &CompactEpisode, before_ms: i64, after_ms: i64, max_ms: i64) -> Window {
    let start_ms = ep.opened_ms.saturating_sub(before_ms);
    let cap = start_ms.saturating_add(max_ms);
    let natural = ep.closed_ms.map(|c| c.saturating_add(after_ms));
    Window {
        episode_id: ep.episode_id.clone(),
        event_id: ep.event_id.clone(),
        opened_ms: ep.opened_ms,
        start_ms,
        end_ms: natural.map_or(cap, |e| e.min(cap)),
        truncated: natural.is_none_or(|e| e > cap),
    }
}

/// The opens [from, to) whose window can touch [hour, hour + 1 h).
pub fn query_range(hour_ms: i64, cfg: &Config) -> (i64, i64) {
    let before = cfg.l2_before_secs.saturating_mul(1_000);
    let max = cfg.l2_max_window_secs.saturating_mul(1_000);
    (hour_ms - max + before, hour_ms + HOUR_MS + before)
}

/// The windows that intersect the hour, ordered by open then id (the merge order).
pub fn windows_for_hour(eps: &[CompactEpisode], hour_ms: i64, cfg: &Config) -> Vec<Window> {
    let mut out: Vec<Window> = eps
        .iter()
        .map(|e| {
            window_of(
                e,
                cfg.l2_before_secs.saturating_mul(1_000),
                cfg.l2_after_secs.saturating_mul(1_000),
                cfg.l2_max_window_secs.saturating_mul(1_000),
            )
        })
        .filter(|w| w.start_ms < hour_ms + HOUR_MS && w.end_ms > hour_ms)
        .collect();
    out.sort_by(|a, b| {
        a.opened_ms
            .cmp(&b.opened_ms)
            .then_with(|| a.episode_id.cmp(&b.episode_id))
    });
    out
}
```
(Move the new `use` lines to the top of the file with the existing one.)

- [ ] **Step 4: l2 in the reducer**

In `bins/compact/src/reduce.rs`:
1. Add `use crate::rows::L2Row;` to the `rows` import, `use tape::feed::Levels;` to the `tape::feed` import.
2. Add three fields to `Reducer` (after `windows`, removing any `#[allow(dead_code)]` from Task 11):
```rust
    /// token → index in `windows` of the episode that owns its l2 rows now.
    owner: HashMap<String, usize>,
    /// Owned tokens whose owner has not had a non-empty snapshot yet.
    needs_snap: BTreeSet<String>,
    /// The next window start or end: owners are recomputed when the clock reaches it.
    next_owner_ms: i64,
```
initialized in `new()` as `owner: HashMap::new(), needs_snap: BTreeSet::new(), next_owner_ms: i64::MIN,`.
3. In `begin_hour`, after `self.windows = windows;`:
```rust
        self.owner.clear();
        self.needs_snap.clear();
        self.next_owner_ms = i64::MIN; // every hour restarts its windows with snapshots (ruling R5)
```
4. In `line`, replace `let ap = self.feed.apply(line, load, &|_| false);` with:
```rust
        self.owners_due(r, sink)?;
        let owner = &self.owner;
        let ap = self.feed.apply(line, load, &|t| owner.contains_key(t));
```
5. In `rows`, after `self.tob(t, r, sink)?;`, add `self.l2(t, ap, r, sink)?;`.
6. In `on_event`'s `LineEvent::Universe` arm, add `self.next_owner_ms = i64::MIN;` (an event's tokens may have changed).
7. In `forget`, add `self.owner.remove(t);` and `self.needs_snap.remove(t);`.
8. Add these methods to `impl Reducer`:
```rust
    /// Recompute which episode owns each token when a window starts or ends: the first window
    /// (by open, then id) covering the clock whose event lists the token. A token that gets a new
    /// owner starts with a snapshot under it.
    fn owners_due(&mut self, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        if self.clock_ms < self.next_owner_ms {
            return Ok(());
        }
        let now = self.clock_ms;
        let mut owner: HashMap<String, usize> = HashMap::new();
        let mut next = i64::MAX;
        for (i, w) in self.windows.iter().enumerate() {
            if w.start_ms > now {
                next = next.min(w.start_ms);
                continue;
            }
            if w.end_ms <= now {
                continue;
            }
            next = next.min(w.end_ms);
            for t in self.feed.event_tokens(&w.event_id) {
                owner.entry(t).or_insert(i);
            }
        }
        self.next_owner_ms = next;
        let mut handover: Vec<(String, usize)> = owner
            .iter()
            .filter(|(t, w)| self.owner.get(*t) != Some(*w))
            .map(|(t, w)| (t.clone(), *w))
            .collect();
        handover.sort();
        self.needs_snap.retain(|t| owner.contains_key(t));
        self.owner = owner;
        for (t, w) in handover {
            if self.trusted.contains(&t) {
                self.l2_snapshot(&t, w, r, sink)?;
            } else {
                self.needs_snap.insert(t);
            }
        }
        Ok(())
    }

    fn l2(&mut self, t: &str, ap: &Applied, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(&w) = self.owner.get(t) else {
            return Ok(());
        };
        if ap.snapshotted.iter().any(|x| x == t) || self.needs_snap.contains(t) {
            return self.l2_snapshot(t, w, r, sink);
        }
        let Some(before) = ap.before_of(t) else {
            return Ok(());
        };
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let (now, ts) = (Levels::of(book), book.server_ts_ms());
        let changes = diff(before, &now);
        let token_idx = self.index(t)?;
        let episode_id = self.windows[w].episode_id.clone();
        for (side, p, s) in changes {
            sink.l2(L2Row {
                ts_ms: ts,
                recv_ms: r,
                token_idx,
                side,
                price: micros(p.get())?,
                new_size: s.map(|x| micros(x.get())).transpose()?.unwrap_or(0),
                snapshot: false,
                episode_id: episode_id.clone(),
            })?;
        }
        Ok(())
    }

    /// One `snapshot = true` row per level under window `w`; an empty book waits for its first level.
    fn l2_snapshot(&mut self, t: &str, w: usize, r: i64, sink: &mut dyn Sink) -> anyhow::Result<()> {
        let Some(book) = self.feed.book(t) else {
            return Ok(());
        };
        let (lv, ts) = (Levels::of(book), book.server_ts_ms());
        if lv.bids.is_empty() && lv.asks.is_empty() {
            self.needs_snap.insert(t.to_string());
            return Ok(());
        }
        self.needs_snap.remove(t);
        let token_idx = self.index(t)?;
        let episode_id = self.windows[w].episode_id.clone();
        for (side, levels) in [(SIDE_BID, lv.bids), (SIDE_ASK, lv.asks)] {
            for (p, s) in levels {
                sink.l2(L2Row {
                    ts_ms: ts,
                    recv_ms: r,
                    token_idx,
                    side,
                    price: micros(p.get())?,
                    new_size: micros(s.get())?,
                    snapshot: true,
                    episode_id: episode_id.clone(),
                })?;
            }
        }
        Ok(())
    }
```
9. Add this free function (after `gap`):
```rust
/// Levels whose size differs between two states of one book, bids then asks, prices ascending.
/// `None` = the level is gone (written as new_size 0).
fn diff(before: &Levels, now: &Levels) -> Vec<(u8, Price, Option<Shares>)> {
    let mut out = Vec::new();
    for (side, b, n) in [
        (SIDE_BID, &before.bids, &now.bids),
        (SIDE_ASK, &before.asks, &now.asks),
    ] {
        let bm: BTreeMap<Price, Shares> = b.iter().copied().collect();
        let nm: BTreeMap<Price, Shares> = n.iter().copied().collect();
        let prices: BTreeSet<Price> = bm.keys().chain(nm.keys()).copied().collect();
        for p in prices {
            let (x, y) = (bm.get(&p), nm.get(&p));
            if x != y {
                out.push((side, p, y.copied()));
            }
        }
    }
    out
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the five l2 tests and every reducer test from Task 11 (they have no windows, so no l2 rows).

- [ ] **Step 6: Commit**

```bash
git add bins/compact/src/windows.rs bins/compact/src/reduce.rs bins/compact/tests/l2.rs
git commit -m "compact: l2 windows around net-positive arbitrage episodes (snapshots, exact changes, merge)"
```

---

### Task 14: `compact`: one hour to Parquet parts, atomically

Spec §4.1, §4.3, §11. Each hour writes `HH.tokens`, `HH.tob`, `HH.depth`, `HH.l2`, `HH.gaps` parts under temporary names, renames them only when all five are complete, and renames the sidecar `HH.hour.json` (the hour's manifest entry) last. Ruling R4: `tob` and `l2` rows are written in `recv_ms` order through a 10 s reorder buffer (the capture file is in engine order, and shards interleave); a row later than the buffer is appended where it arrives and counted in `late_rows`, never dropped. A raw file that can't be read to the end keeps the rows before the break and turns the rest of the hour into a `raw_hour_lost` gap (Review Focus 1).

**Files:**
- Create: `bins/compact/src/hour.rs`, `bins/compact/src/archive.rs` (paths only; Task 16 adds the merge), `bins/compact/tests/hour.rs`
- Modify: `bins/compact/src/lib.rs`, `bins/compact/tests/fixture/mod.rs` (hour helpers)

**Interfaces:**
- Consumes: Tasks 9–13.
- Produces:
  - `compact::archive`: `MANIFEST = "manifest.json"`, `day_dir(&Path, ms) -> PathBuf`, `hour_number(ms) -> u32`, `part_path(day: &Path, hh: u32, table: &str) -> PathBuf` (`HH.<table>.parquet`), `sidecar_path(day, hh) -> PathBuf` (`HH.hour.json`), `daily_path(day, table) -> PathBuf` (`<table>.parquet`).
  - `compact::hour`: `REORDER_MS = 10_000`, `Reorder<R>` (`Default`; `push(recv_ms, R, &mut dyn FnMut(R) -> anyhow::Result<()>)`, `drain(…)`, `late() -> u64`), `RowCounts { tokens, tob, depth, l2, gaps }` (+ `total()`), `HourReport { hour_ms, status, source_file, source_bytes, source_sha256, rows: RowCounts, bytes: RowCounts, truncated_windows, stats: HourStats, late_rows, read_error }`, `Source { File(PathBuf), Lost, Stopped }`, `HourJob { hour_ms, raw_dir: PathBuf, archive_dir: PathBuf, source: Source, windows: Vec<Window>, stop_before_publish: bool }`, `process_hour(&mut Reducer, &HourJob) -> anyhow::Result<Option<HourReport>>` (None only when `stop_before_publish`).
  - Test helpers appended to `bins/compact/tests/fixture/mod.rs` (used by Tasks 14–15): `three_hours(raw) -> (RecordView, Vec<Vec<Line>>)`, `hour_windows(i) -> Vec<Window>`, `hour_job(raw, archive, i) -> HourJob`, `day_files(archive) -> Vec<(PathBuf, Vec<u8>)>`.

- [ ] **Step 1: Write the failing tests**

Append to `bins/compact/tests/fixture/mod.rs`:
```rust
/// Three raw hours that depend on each other (books, pending checks, a disconnect, a window).
pub fn three_hours(raw: &Path) -> (RecordView, Vec<Vec<Line>>) {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(raw, &r);
    let h: Vec<Vec<Line>> = vec![
        vec![
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "5")])),
            frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "3")], &[("0.25", "1")])),
            frame(T0 + 400_000, 0, change("y1", T0 + 399_995, "BUY", "0.41", "1", "0.41", "0.46")),
        ],
        vec![
            frame(T0 + H + 1_000, 0, change("y1", T0 + H + 995, "SELL", "0.47", "2", "0.41", "0.46")),
            down(T0 + H + 2_000, 0),
            frame(T0 + H + 3_000, 0, book("y1", T0 + H + 2_995, &[("0.42", "1")], &[("0.44", "2")])),
            frame(T0 + H + 600_000, 0, change("y1", T0 + H + 599_995, "BUY", "0.43", "1", "0.43", "0.44")),
            frame(T0 + H + 700_000, 0, book("y2", T0 + H + 699_995, &[("0.21", "3")], &[])),
        ],
        vec![
            frame(T0 + 2 * H + 1_000, 0, change("y2", T0 + 2 * H + 995, "BUY", "0.22", "1", "0.22", "1")),
            frame(T0 + 2 * H + 2_000, 0, change("y1", T0 + 2 * H + 1_995, "SELL", "0.44", "0", "0.43", "1")),
        ],
    ];
    for (i, lines) in h.iter().enumerate() {
        write_hour(raw, T0 + i64::try_from(i).unwrap() * H, lines);
    }
    (r, h)
}

pub fn hour_windows(i: i64) -> Vec<compact::windows::Window> {
    if i == 1 {
        vec![compact::windows::Window { episode_id: "ep".into(), event_id: "e1".into(), opened_ms: T0 + H + 500_000, start_ms: T0 + H + 200_000, end_ms: T0 + H + 800_000, truncated: true }]
    } else {
        vec![]
    }
}

pub fn hour_job(raw: &Path, archive: &Path, i: i64) -> compact::hour::HourJob {
    let hour_ms = T0 + i * H;
    compact::hour::HourJob {
        hour_ms,
        raw_dir: raw.to_path_buf(),
        archive_dir: archive.to_path_buf(),
        source: compact::hour::Source::File(tape::files::hour_path(raw, hour_ms)),
        windows: hour_windows(i),
        stop_before_publish: false,
    }
}

/// Every file of day T0's archive folder, sorted by name, with its bytes.
pub fn day_files(archive: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let day = compact::archive::day_dir(archive, T0);
    let mut v: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(&day)
        .unwrap()
        .flatten()
        .map(|e| (PathBuf::from(e.file_name()), std::fs::read(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}
```
`bins/compact/tests/hour.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::{day_dir, part_path, sidecar_path};
use compact::hour::*;
use compact::pq::read_all;
use compact::reduce::Reducer;
use compact::rows::*;
use fixture::*;

#[test]
fn an_hour_writes_five_parts_and_a_sidecar_that_match_the_reducer() {
    let (raw, arch) = (tmp("hour-raw"), tmp("hour-arch"));
    let (r, h) = three_hours(&raw);
    let mut red = Reducer::new(rcfg());
    let report = process_hour(&mut red, &hour_job(&raw, &arch, 0)).unwrap().unwrap();
    let mut want = VecSink::default();
    let mut red2 = Reducer::new(rcfg());
    red2.begin_hour(T0, vec![]);
    run(&mut red2, &h[0], &[r], &mut want);
    red2.end_hour(&mut want).unwrap();
    let day = day_dir(&arch, T0);
    assert_eq!(read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap(), want.tob);
    assert_eq!(read_all::<DepthRow>(&part_path(&day, 0, "depth")).unwrap(), want.depth);
    assert_eq!(read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap(), want.gaps);
    assert_eq!(read_all::<TokenRow>(&part_path(&day, 0, "tokens")).unwrap(), red2.tokens().to_vec());
    assert!(read_all::<L2Row>(&part_path(&day, 0, "l2")).unwrap().is_empty());
    let side: HourReport = serde_json::from_slice(&std::fs::read(sidecar_path(&day, 0)).unwrap()).unwrap();
    assert_eq!(side, report);
    let bytes = std::fs::read(tape::files::hour_path(&raw, T0)).unwrap();
    assert_eq!(report.source_bytes, Some(u64::try_from(bytes.len()).unwrap()));
    assert_eq!(report.source_sha256.as_deref(), Some(compact::config::sha256_hex(&bytes).as_str()));
    assert_eq!(report.source_file.as_deref(), Some("2026-09-23/00.jsonl.zst"));
    assert_eq!((report.status.as_str(), report.rows.tob, report.rows.tokens), ("done", u64::try_from(want.tob.len()).unwrap(), 2));
    assert!(std::fs::read_dir(&day).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_crash_before_the_rename_leaves_no_final_file_and_the_rerun_matches() {
    let raw = tmp("crash-raw");
    three_hours(&raw);
    let (a, b) = (tmp("crash-a"), tmp("crash-b"));
    let mut j = hour_job(&raw, &a, 0);
    j.stop_before_publish = true;
    assert!(process_hour(&mut Reducer::new(rcfg()), &j).unwrap().is_none());
    let day = day_dir(&a, T0);
    assert!(!part_path(&day, 0, "tob").exists() && !sidecar_path(&day, 0).exists(), "nothing published");
    assert!(compact::pq::tmp_of(&part_path(&day, 0, "tob")).exists(), "only temporary files");
    process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &a, 0)).unwrap().unwrap();
    process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &b, 0)).unwrap().unwrap();
    assert_eq!(day_files(&a), day_files(&b), "the re-run equals a clean run");
    for d in [&raw, &a, &b] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn a_truncated_hour_keeps_its_rows_and_gaps_the_rest() {
    let (raw, arch) = (tmp("trunc-raw"), tmp("trunc-arch"));
    let (r, h) = three_hours(&raw);
    // the scanner restarted and appended a second zstd frame, then was killed mid-write
    let p = tape::files::hour_path(&raw, T0);
    let mut bytes = std::fs::read(&p).unwrap();
    let mut second = Vec::new();
    for l in [uni(T0 + 500_000, &r), frame(T0 + 501_000, 0, book("y1", T0 + 500_995, &[("0.30", "1")], &[]))] {
        second.extend(serde_json::to_vec(&l).unwrap());
        second.push(b'\n');
    }
    let z = zstd::encode_all(second.as_slice(), 3).unwrap();
    bytes.extend_from_slice(&z[..z.len() - 6]);
    std::fs::write(&p, &bytes).unwrap();
    let report = process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0)).unwrap().unwrap();
    assert!(report.read_error.is_some(), "{report:?}");
    assert_eq!(report.status, "done", "the hour is recorded; compaction does not stall on it");
    let day = day_dir(&arch, T0);
    let tob = read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap();
    let last_recv = h[0].last().unwrap().recv_ts_us / 1000;
    assert!(tob.iter().all(|t| t.recv_ms <= last_recv) && tob.len() >= 3, "rows before the break are kept: {tob:?}");
    let gaps = read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap();
    assert!(gaps.contains(&GapRow { start_ms: last_recv + 1, end_ms: T0 + H, shard: None, reason: reason::RAW_HOUR_LOST.into() }), "{gaps:?}");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn reorder_sorts_within_the_window_and_counts_late_rows() {
    let mut r: Reorder<char> = Reorder::default();
    let mut out = Vec::new();
    for (recv, v) in [(100, 'a'), (90, 'b'), (95, 'c'), (20_000, 'd'), (50, 'e'), (20_001, 'f')] {
        r.push(recv, v, &mut |x| {
            out.push(x);
            Ok(())
        })
        .unwrap();
    }
    r.drain(&mut |x| {
        out.push(x);
        Ok(())
    })
    .unwrap();
    assert_eq!(out, vec!['b', 'c', 'a', 'e', 'd', 'f'], "b, c, a sorted; e arrived after 100 was written: kept, late");
    assert_eq!(r.late(), 1);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test hour`
Expected: FAIL (no `compact::hour`).

- [ ] **Step 3: Archive paths**

`bins/compact/src/archive.rs`:
```rust
//! The archive layout (spec 5b §3, §4.4): <archive>/YYYY-MM-DD/ holds hourly parts
//! HH.<table>.parquet and HH.hour.json until the day is complete, then the daily
//! <table>.parquet files and manifest.json. <archive>/_state/ holds the checkpoints.
use std::path::{Path, PathBuf};
use tape::time::{civil, day_name};

pub const MANIFEST: &str = "manifest.json";

pub fn day_dir(archive: &Path, ms: i64) -> PathBuf {
    archive.join(day_name(ms))
}

pub fn hour_number(ms: i64) -> u32 {
    civil(ms).3
}

pub fn part_path(day: &Path, hh: u32, table: &str) -> PathBuf {
    day.join(format!("{hh:02}.{table}.parquet"))
}

pub fn sidecar_path(day: &Path, hh: u32) -> PathBuf {
    day.join(format!("{hh:02}.hour.json"))
}

pub fn daily_path(day: &Path, table: &str) -> PathBuf {
    day.join(format!("{table}.parquet"))
}
```

- [ ] **Step 4: The hour writer**

`bins/compact/src/hour.rs`:
```rust
//! One raw hour → its Parquet parts and sidecar, atomically (spec 5b §4.1, §4.3): the five parts
//! are written under temporary names and renamed only when all are complete; the sidecar
//! HH.hour.json (this hour's manifest entry) is renamed last. Re-running an hour overwrites them.
use crate::archive::{day_dir, hour_number, part_path, sidecar_path};
use crate::pq::{PartWriter, Staged, Table, write_atomic};
use crate::reduce::{HourStats, Reducer};
use crate::rows::{DepthRow, GapRow, L2Row, Sink, TobRow, TokenRow, reason};
use crate::windows::Window;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use tape::line::Line;
use tape::universe_view::load_record;

/// Rows leave in recv_ms order within this window (plan ruling R4).
pub const REORDER_MS: i64 = 10_000;

/// Sorts rows by recv_ms (ties in arrival order) within `REORDER_MS` of the newest one seen. A row
/// older than one already written is written at once and counted as late, never dropped.
pub struct Reorder<R> {
    buf: BTreeMap<(i64, u64), R>,
    seq: u64,
    high: i64,
    flushed: i64,
    late: u64,
}

impl<R> Default for Reorder<R> {
    fn default() -> Self {
        Reorder {
            buf: BTreeMap::new(),
            seq: 0,
            high: i64::MIN,
            flushed: i64::MIN,
            late: 0,
        }
    }
}

impl<R> Reorder<R> {
    pub fn push(
        &mut self,
        recv_ms: i64,
        row: R,
        out: &mut dyn FnMut(R) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if recv_ms < self.flushed {
            self.late += 1;
            return out(row);
        }
        self.buf.insert((recv_ms, self.seq), row);
        self.seq += 1;
        self.high = self.high.max(recv_ms);
        let cut = self.high.saturating_sub(REORDER_MS);
        while let Some(e) = self.buf.first_entry() {
            if e.key().0 >= cut {
                break;
            }
            let ((k, _), v) = e.remove_entry();
            self.flushed = k;
            out(v)?;
        }
        Ok(())
    }

    pub fn drain(&mut self, out: &mut dyn FnMut(R) -> anyhow::Result<()>) -> anyhow::Result<()> {
        while let Some(((k, _), v)) = self.buf.pop_first() {
            self.flushed = k;
            out(v)?;
        }
        Ok(())
    }

    pub fn late(&self) -> u64 {
        self.late
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowCounts {
    pub tokens: u64,
    pub tob: u64,
    pub depth: u64,
    pub l2: u64,
    pub gaps: u64,
}

impl RowCounts {
    pub fn total(&self) -> u64 {
        self.tokens + self.tob + self.depth + self.l2 + self.gaps
    }
}

/// One hour's manifest entry (spec §3 manifest.json) and its compact_hours row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourReport {
    pub hour_ms: i64,
    /// "done" or "lost".
    pub status: String,
    /// Relative to raw_dir: YYYY-MM-DD/HH.jsonl.zst; None when there was no file.
    pub source_file: Option<String>,
    pub source_bytes: Option<u64>,
    pub source_sha256: Option<String>,
    pub rows: RowCounts,
    /// Bytes of each part.
    pub bytes: RowCounts,
    pub truncated_windows: Vec<String>,
    pub stats: HourStats,
    pub late_rows: u64,
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    /// No file, and compact_hours says lost (the scanner deleted it at its cap).
    Lost,
    /// No file and no record: the scanner was not running.
    Stopped,
}

#[derive(Debug, Clone)]
pub struct HourJob {
    pub hour_ms: i64,
    pub raw_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub source: Source,
    pub windows: Vec<Window>,
    /// Tests: stop after writing every part, before any rename (a crash at the worst moment).
    pub stop_before_publish: bool,
}

/// The five parts of one hour, tob and l2 through the reorder buffers.
struct PartSink {
    tob: PartWriter<TobRow>,
    depth: PartWriter<DepthRow>,
    l2: PartWriter<L2Row>,
    gaps: PartWriter<GapRow>,
    tob_order: Reorder<TobRow>,
    l2_order: Reorder<L2Row>,
    tokens_fin: PathBuf,
}

impl PartSink {
    fn create(day: &Path, hh: u32) -> anyhow::Result<Self> {
        Ok(PartSink {
            tob: PartWriter::create(part_path(day, hh, TobRow::NAME))?,
            depth: PartWriter::create(part_path(day, hh, DepthRow::NAME))?,
            l2: PartWriter::create(part_path(day, hh, L2Row::NAME))?,
            gaps: PartWriter::create(part_path(day, hh, GapRow::NAME))?,
            tob_order: Reorder::default(),
            l2_order: Reorder::default(),
            tokens_fin: part_path(day, hh, TokenRow::NAME),
        })
    }

    /// Drain the buffers, write the tokens part, close everything under temporary names.
    fn close(mut self, tokens: &[TokenRow]) -> anyhow::Result<(Vec<Staged>, u64)> {
        let w = &mut self.tob;
        self.tob_order.drain(&mut |x| w.push(x))?;
        let w = &mut self.l2;
        self.l2_order.drain(&mut |x| w.push(x))?;
        let late = self.tob_order.late() + self.l2_order.late();
        let mut t = PartWriter::<TokenRow>::create(self.tokens_fin.clone())?;
        for row in tokens {
            t.push(row.clone())?;
        }
        Ok((
            vec![
                t.close()?,
                self.tob.close()?,
                self.depth.close()?,
                self.l2.close()?,
                self.gaps.close()?,
            ],
            late,
        ))
    }
}

impl Sink for PartSink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()> {
        let w = &mut self.tob;
        self.tob_order.push(r.recv_ms, r, &mut |x| w.push(x))
    }
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()> {
        self.depth.push(r)
    }
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()> {
        let w = &mut self.l2;
        self.l2_order.push(r.recv_ms, r, &mut |x| w.push(x))
    }
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()> {
        self.gaps.push(r)
    }
}

/// Hashes everything read through it (the raw file's sha256 in one pass).
struct Hashing<R> {
    inner: R,
    h: Sha256,
    n: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.h.update(&buf[..k]);
        self.n += u64::try_from(k).unwrap_or(0);
        Ok(k)
    }
}

struct SourceInfo {
    file: String,
    bytes: u64,
    sha256: String,
    read_error: Option<String>,
}

fn read_hour(
    red: &mut Reducer,
    raw_dir: &Path,
    path: &Path,
    sink: &mut PartSink,
) -> anyhow::Result<SourceInfo> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hr = Hashing {
        inner: f,
        h: Sha256::new(),
        n: 0,
    };
    let mut read_error = None;
    {
        let mut load = |taken: i64, file: &str| load_record(raw_dir, taken, file);
        match zstd::Decoder::new(&mut hr) {
            Err(e) => read_error = Some(format!("zstd: {e}")),
            Ok(dec) => {
                for text in BufReader::new(dec).lines() {
                    let text = match text {
                        Ok(t) => t,
                        Err(e) => {
                            read_error = Some(format!("read: {e}"));
                            break;
                        }
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Line>(&text) {
                        Ok(line) => red.line(&line, &mut load, sink)?,
                        Err(_) => red.count_bad_line(),
                    }
                }
            }
        }
    }
    if read_error.is_some() {
        red.cut(reason::RAW_HOUR_LOST, sink)?;
    }
    std::io::copy(&mut hr, &mut std::io::sink())?; // hash the bytes past a break too
    let file = path
        .strip_prefix(raw_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    let bytes = hr.n;
    Ok(SourceInfo {
        file,
        bytes,
        sha256: hr.h.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        read_error,
    })
}

/// Compact one hour into the archive. `Ok(None)` only when `job.stop_before_publish`.
pub fn process_hour(red: &mut Reducer, job: &HourJob) -> anyhow::Result<Option<HourReport>> {
    let day = day_dir(&job.archive_dir, job.hour_ms);
    std::fs::create_dir_all(&day).with_context(|| format!("create {}", day.display()))?;
    let hh = hour_number(job.hour_ms);
    let mut sink = PartSink::create(&day, hh)?;
    red.begin_hour(job.hour_ms, job.windows.clone());
    let (status, src) = match &job.source {
        Source::File(p) => ("done", Some(read_hour(red, &job.raw_dir, p, &mut sink)?)),
        Source::Lost => {
            red.missing_hour(reason::RAW_HOUR_LOST, &mut sink)?;
            ("lost", None)
        }
        Source::Stopped => {
            red.missing_hour(reason::SCANNER_STOPPED, &mut sink)?;
            ("done", None)
        }
    };
    let stats = red.end_hour(&mut sink)?;
    let (staged, late_rows) = sink.close(red.tokens())?;
    if job.stop_before_publish {
        return Ok(None);
    }
    for s in &staged {
        s.publish()
            .with_context(|| format!("publish {}", s.fin.display()))?;
    }
    let count = |t: &str| {
        staged
            .iter()
            .find(|s| s.fin == part_path(&day, hh, t))
            .map_or(0, |s| s.rows)
    };
    let size = |t: &str| {
        std::fs::metadata(part_path(&day, hh, t))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    let per = |f: &dyn Fn(&str) -> u64| RowCounts {
        tokens: f(TokenRow::NAME),
        tob: f(TobRow::NAME),
        depth: f(DepthRow::NAME),
        l2: f(L2Row::NAME),
        gaps: f(GapRow::NAME),
    };
    let mut truncated: Vec<String> = job
        .windows
        .iter()
        .filter(|w| w.truncated)
        .map(|w| w.episode_id.clone())
        .collect();
    truncated.sort();
    truncated.dedup();
    let report = HourReport {
        hour_ms: job.hour_ms,
        status: status.to_string(),
        source_file: src.as_ref().map(|s| s.file.clone()),
        source_bytes: src.as_ref().map(|s| s.bytes),
        source_sha256: src.as_ref().map(|s| s.sha256.clone()),
        rows: per(&count),
        bytes: per(&size),
        truncated_windows: truncated,
        stats,
        late_rows,
        read_error: src.and_then(|s| s.read_error),
    };
    write_atomic(&sidecar_path(&day, hh), &serde_json::to_vec_pretty(&report)?)?;
    Ok(Some(report))
}
```

`bins/compact/src/lib.rs`: add `pub mod archive;` and `pub mod hour;`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the four hour tests and every earlier compact test.

- [ ] **Step 6: Commit**

```bash
git add bins/compact/src/lib.rs bins/compact/src/archive.rs bins/compact/src/hour.rs bins/compact/tests/hour.rs bins/compact/tests/fixture/mod.rs
git commit -m "compact: one hour to Parquet parts atomically, recv-ordered"
```

---

### Task 15: `compact`: checkpoints and resume

Spec §4.1, §8 (resume). After each hour the reducer's whole state goes to `_state/checkpoint-<hour_ms>.bin.zst` (zstd-compressed JSON, ruling R15; two kept); a restart resumes from the newest readable one, and three hours processed with a restart after hour 1 must equal three processed in one go, byte for byte.

**Files:**
- Create: `bins/compact/src/state.rs`, `bins/compact/tests/checkpoint.rs`
- Modify: `bins/compact/src/lib.rs`

**Interfaces:**
- Consumes: Task 14 (`HourReport`, `process_hour`, the fixture helpers), Task 11 (`ReducerState`, `Reducer::from_state`).
- Produces (`compact::state`): `Checkpoint { hour_ms, report: HourReport, reducer: ReducerState }`, `save(archive: &Path, &Checkpoint) -> anyhow::Result<()>`, `list(archive) -> Vec<(i64, PathBuf)>`, `load_newest(archive) -> anyhow::Result<Option<Checkpoint>>`.

- [ ] **Step 1: Write the failing tests**

`bins/compact/tests/checkpoint.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::{day_dir, part_path};
use compact::hour::process_hour;
use compact::pq::read_all;
use compact::reduce::Reducer;
use compact::rows::L2Row;
use compact::state;
use fixture::*;

#[test]
fn resume_from_a_checkpoint_after_hour_1_matches_a_straight_run_byte_for_byte() {
    let raw = tmp("resume-raw");
    three_hours(&raw);
    let (a, b) = (tmp("resume-a"), tmp("resume-b"));
    let mut straight = Reducer::new(rcfg());
    for i in 0..3 {
        process_hour(&mut straight, &hour_job(&raw, &a, i)).unwrap().unwrap();
    }
    let mut first = Reducer::new(rcfg());
    let rep = process_hour(&mut first, &hour_job(&raw, &b, 0)).unwrap().unwrap();
    state::save(&b, &state::Checkpoint { hour_ms: T0, report: rep, reducer: first.state() }).unwrap();
    drop(first);
    let cp = state::load_newest(&b).unwrap().unwrap();
    assert_eq!(cp.hour_ms, T0);
    let mut resumed = Reducer::from_state(rcfg(), cp.reducer).unwrap();
    for i in 1..3 {
        process_hour(&mut resumed, &hour_job(&raw, &b, i)).unwrap().unwrap();
    }
    let (fa, fb) = (day_files(&a), day_files(&b));
    assert_eq!(fa.len(), 18, "3 hours × (5 parts + sidecar)");
    assert_eq!(fa, fb, "every part and sidecar identical, byte for byte");
    let l2 = read_all::<L2Row>(&part_path(&day_dir(&a, T0), 1, "l2")).unwrap();
    assert!(!l2.is_empty() && l2[0].snapshot, "hour 1 has the window, starting with snapshots");
    for d in [&raw, &a, &b] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn checkpoints_keep_the_newest_two_and_skip_a_corrupt_one() {
    let (raw, arch) = (tmp("cp-raw"), tmp("cp-arch"));
    three_hours(&raw);
    let mut red = Reducer::new(rcfg());
    for i in 0..3 {
        let rep = process_hour(&mut red, &hour_job(&raw, &arch, i)).unwrap().unwrap();
        state::save(&arch, &state::Checkpoint { hour_ms: T0 + i * H, report: rep, reducer: red.state() }).unwrap();
    }
    let l = state::list(&arch);
    assert_eq!(l.iter().map(|(h, _)| *h).collect::<Vec<_>>(), vec![T0 + H, T0 + 2 * H]);
    std::fs::write(&l[1].1, b"not zstd").unwrap();
    assert_eq!(state::load_newest(&arch).unwrap().unwrap().hour_ms, T0 + H, "a corrupt newest falls back to the older one");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test checkpoint`
Expected: FAIL (no `compact::state`).

- [ ] **Step 3: Checkpoints**

`bins/compact/src/state.rs`:
```rust
//! Checkpoints (spec 5b §4.1): after each hour, the reducer's whole state (books, pending checks,
//! trust, the day's dictionary, open gaps) goes to _state/checkpoint-<hour_ms>.bin.zst
//! (zstd-compressed JSON) via a temporary name and a rename. The newest two are kept; a
//! checkpoint that can't be read falls back to the older one.
use crate::hour::HourReport;
use crate::pq::write_atomic;
use crate::reduce::ReducerState;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STATE_DIR: &str = "_state";
const KEEP: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The last hour processed.
    pub hour_ms: i64,
    /// Its report: re-recorded in compact_hours at startup if the database write was missed.
    pub report: HourReport,
    pub reducer: ReducerState,
}

fn path(archive: &Path, hour_ms: i64) -> PathBuf {
    archive
        .join(STATE_DIR)
        .join(format!("checkpoint-{hour_ms}.bin.zst"))
}

/// Checkpoints on disk, oldest first.
pub fn list(archive: &Path) -> Vec<(i64, PathBuf)> {
    let mut out: Vec<(i64, PathBuf)> = std::fs::read_dir(archive.join(STATE_DIR))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let h = name
                        .strip_prefix("checkpoint-")?
                        .strip_suffix(".bin.zst")?
                        .parse()
                        .ok()?;
                    Some((h, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub fn save(archive: &Path, cp: &Checkpoint) -> anyhow::Result<()> {
    std::fs::create_dir_all(archive.join(STATE_DIR))?;
    let json = serde_json::to_vec(cp)?;
    let z = zstd::encode_all(json.as_slice(), 3)?;
    let p = path(archive, cp.hour_ms);
    write_atomic(&p, &z).with_context(|| format!("write {}", p.display()))?;
    let all = list(archive);
    for (_, old) in all.iter().take(all.len().saturating_sub(KEEP)) {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

fn read(p: &Path) -> anyhow::Result<Checkpoint> {
    let z = std::fs::read(p)?;
    Ok(serde_json::from_slice(&zstd::decode_all(z.as_slice())?)?)
}

pub fn load_newest(archive: &Path) -> anyhow::Result<Option<Checkpoint>> {
    for (_, p) in list(archive).into_iter().rev() {
        match read(&p) {
            Ok(cp) => return Ok(Some(cp)),
            Err(e) => eprintln!(
                "compact: checkpoint {} unreadable ({e:#}); trying the older one",
                p.display()
            ),
        }
    }
    Ok(None)
}
```
`bins/compact/src/lib.rs`: add `pub mod state;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: both checkpoint tests. If `resume…byte_for_byte` differs only in Parquet footer metadata (it carries no timestamp in the pinned version, but a future version might), compare `read_all` rows of each part plus the sidecars instead, and say so in the commit message (spec §8 allows "row for row").

- [ ] **Step 5: Commit**

```bash
git add bins/compact/src/lib.rs bins/compact/src/state.rs bins/compact/tests/checkpoint.rs
git commit -m "compact: checkpoints (two kept, corrupt falls back) and byte-for-byte resume"
```

---

### Task 16: `compact`: the daily merge, the manifest and the 55 GB budget

Spec §4.4: when all 24 hours of a UTC day have their sidecar (each hour is `done` or `lost`, the compactor writes one for every hour, file or not), the parts are merged into the daily files, `manifest.json` is written, the row counts are checked against the sidecars, and only then the hourly parts are deleted. After each merge, whole merged days are deleted oldest first while the archive is over its budget. A crash anywhere in a merge is finished by `recover` at the next start (the day is merged again from its parts, or its leftover parts are removed).

**Files:**
- Modify: `bins/compact/src/archive.rs`, `bins/compact/tests/fixture/mod.rs` (`build_day`)
- Create: `bins/compact/tests/archive.rs`

**Interfaces:**
- Consumes: Task 14 (`process_hour`, `HourReport`, `RowCounts`, `archive` paths).
- Produces (`compact::archive`): `Manifest { compact_version, day, config: Config, config_sha256, hours: Vec<HourReport>, truncated_windows, totals: RowCounts, bytes: RowCounts, ts_clamped: u64, late_rows: u64 }`, `days(archive) -> Vec<(String, PathBuf)>` (oldest first), `merged(day: &Path) -> bool`, `complete(day: &Path) -> bool`, `merge_day(day: &Path, name: &str, &Config, cfg_sha: &str) -> anyhow::Result<Manifest>`, `merge_ready(archive, &Config, cfg_sha) -> anyhow::Result<Vec<Manifest>>`, `recover(archive, &Config, cfg_sha) -> anyhow::Result<Vec<Manifest>>`, `archive_bytes(archive) -> u64`, `oldest_day(archive) -> Option<String>`, `enforce_budget(archive, budget_bytes: u64) -> anyhow::Result<Vec<(String, u64)>>`. Fixture: `build_day(raw: &Path, archive: &Path, day_ms: i64, lost: &[u32]) -> Vec<HourReport>` (hour 0 from a real raw hour with an l2 window, the rest `Stopped`, `lost` hours `Lost`; no merge).

- [ ] **Step 1: Write the failing tests**

Append to `bins/compact/tests/fixture/mod.rs`:
```rust
/// A whole day in the archive, unmerged: hour 0 from a real raw hour (two tokens, an l2 window),
/// the hours in `lost` as Lost, every other hour Stopped.
pub fn build_day(raw: &Path, archive: &Path, day_ms: i64, lost: &[u32]) -> Vec<compact::hour::HourReport> {
    use compact::hour::{HourJob, Source, process_hour};
    let r = rec(&format!("u-{day_ms}"), day_ms, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(raw, &r);
    write_hour(raw, day_ms, &[
        uni(day_ms + 1_000, &r),
        frame(day_ms + 2_000, 0, book("y1", day_ms + 1_990, &[("0.40", "10")], &[("0.45", "5")])),
        frame(day_ms + 2_100, 0, book("y2", day_ms + 2_090, &[("0.20", "3")], &[])),
        frame(day_ms + 200_000, 0, change("y1", day_ms + 199_995, "BUY", "0.41", "1", "0.41", "0.45")),
        frame(day_ms + 400_000, 0, change("y1", day_ms + 399_995, "BUY", "0.42", "1", "0.42", "0.45")),
        frame(day_ms + 500_000, 0, change("y2", day_ms + 499_995, "BUY", "0.21", "1", "0.21", "1")),
    ]);
    let win = compact::windows::Window {
        episode_id: format!("live-1-{day_ms}"),
        event_id: "e1".into(),
        opened_ms: day_ms + 450_000,
        start_ms: day_ms + 150_000,
        end_ms: day_ms + 1_950_000,
        truncated: true,
    };
    let mut red = Reducer::new(rcfg());
    (0..24u32)
        .map(|hh| {
            let hour_ms = day_ms + i64::from(hh) * H;
            let source = if hh == 0 {
                Source::File(tape::files::hour_path(raw, hour_ms))
            } else if lost.contains(&hh) {
                Source::Lost
            } else {
                Source::Stopped
            };
            let job = HourJob {
                hour_ms,
                raw_dir: raw.to_path_buf(),
                archive_dir: archive.to_path_buf(),
                source,
                windows: if hh == 0 { vec![win.clone()] } else { vec![] },
                stop_before_publish: false,
            };
            process_hour(&mut red, &job).unwrap().unwrap()
        })
        .collect()
}
```
`bins/compact/tests/archive.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::*;
use compact::pq::{num_rows, read_all};
use compact::rows::*;
use fixture::*;
use std::path::Path;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    v.sort();
    v
}

#[test]
fn a_complete_day_merges_into_daily_files_with_a_manifest_and_drops_the_parts() {
    let (raw, arch) = (tmp("merge-raw"), tmp("merge-arch"));
    let reports = build_day(&raw, &arch, T0, &[5]);
    let c = cfg(&raw, &arch);
    let ms = merge_ready(&arch, &c, "cfgsha").unwrap();
    assert_eq!(ms.len(), 1);
    let m = &ms[0];
    let day = day_dir(&arch, T0);
    assert_eq!(names(&day), vec!["depth.parquet", "gaps.parquet", "l2.parquet", MANIFEST, "tob.parquet", "tokens.parquet"]);
    assert_eq!(m.day, "2026-09-23");
    assert_eq!(m.config_sha256, "cfgsha");
    assert_eq!(m.hours, reports);
    assert_eq!(m.hours[5].status, "lost");
    assert_eq!(m.truncated_windows, vec![format!("live-1-{T0}")]);
    assert_eq!(m.ts_clamped, reports.iter().map(|r| r.stats.ts_clamped).sum::<u64>(), "R4: clamps are counted per day");
    assert_eq!(m.late_rows, reports.iter().map(|r| r.late_rows).sum::<u64>());
    assert_eq!(m.totals.tob, reports.iter().map(|r| r.rows.tob).sum::<u64>());
    assert_eq!(num_rows(&daily_path(&day, "tob")).unwrap(), m.totals.tob);
    assert_eq!(num_rows(&daily_path(&day, "tokens")).unwrap(), 2);
    let gaps = read_all::<GapRow>(&daily_path(&day, "gaps")).unwrap();
    assert!(gaps.contains(&GapRow { start_ms: T0 + 5 * H, end_ms: T0 + 6 * H, shard: None, reason: reason::RAW_HOUR_LOST.into() }));
    let written: Manifest = serde_json::from_slice(&std::fs::read(day.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(&written, m);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_day_missing_an_hour_is_not_merged() {
    let (raw, arch) = (tmp("partial-raw"), tmp("partial-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    std::fs::remove_file(day.join("23.hour.json")).unwrap();
    assert!(!complete(&day));
    assert!(merge_ready(&arch, &cfg(&raw, &arch), "s").unwrap().is_empty());
    assert!(!merged(&day));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_merge_interrupted_at_any_point_is_finished_by_recover() {
    let (raw, arch) = (tmp("recover-raw"), tmp("recover-arch"));
    build_day(&raw, &arch, T0, &[]);
    let day = day_dir(&arch, T0);
    let copy = tmp("recover-copy");
    for n in names(&day) {
        std::fs::copy(day.join(&n), copy.join(&n)).unwrap();
    }
    let c = cfg(&raw, &arch);
    let first = merge_ready(&arch, &c, "s").unwrap().remove(0);
    // crash after the daily files were renamed, before the manifest: parts come back, no manifest
    std::fs::remove_file(day.join(MANIFEST)).unwrap();
    for n in names(&copy) {
        std::fs::copy(copy.join(&n), day.join(&n)).unwrap();
    }
    let again = recover(&arch, &c, "s").unwrap();
    assert_eq!(again, vec![first.clone()], "merged again from its parts, same manifest");
    // crash after the manifest, before the parts were deleted: recover only cleans up
    std::fs::copy(copy.join("07.tob.parquet"), day.join("07.tob.parquet")).unwrap();
    std::fs::write(day.join("tob.parquet.tmp"), b"half").unwrap();
    assert!(recover(&arch, &c, "s").unwrap().is_empty());
    assert_eq!(names(&day), vec!["depth.parquet", "gaps.parquet", "l2.parquet", MANIFEST, "tob.parquet", "tokens.parquet"]);
    for d in [&raw, &arch, &copy] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn over_budget_the_oldest_merged_day_goes_first_and_oldest_day_moves() {
    let arch = tmp("budget");
    let mk = |day: &str, merged: bool| {
        let d = arch.join(day);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("tob.parquet"), vec![0u8; 1_000]).unwrap();
        if merged {
            std::fs::write(d.join(MANIFEST), b"{}").unwrap();
        }
    };
    mk("2026-09-20", true);
    mk("2026-09-21", true);
    mk("2026-09-22", true);
    mk("2026-09-23", false); // today, still hourly: never deleted by the budget
    std::fs::create_dir_all(arch.join("_state")).unwrap();
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-20"));
    let gone = enforce_budget(&arch, 2_100).unwrap();
    assert_eq!(gone.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(), vec!["2026-09-20", "2026-09-21"]);
    assert_eq!(oldest_day(&arch).as_deref(), Some("2026-09-22"));
    assert!(archive_bytes(&arch) <= 2_100);
    let none_left = enforce_budget(&arch, 0).unwrap();
    assert_eq!(none_left.len(), 1, "only merged days go");
    assert!(arch.join("2026-09-23").exists());
    std::fs::remove_dir_all(&arch).ok();
}
```
(In the budget test each `manifest.json` is an empty JSON object: `enforce_budget` only checks that the file exists.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test archive`
Expected: FAIL (no `merge_ready`).

- [ ] **Step 3: The merge, recovery and budget**

Append to `bins/compact/src/archive.rs` (and merge the `use` lines into the top of the file):
```rust
use crate::config::Config;
use crate::hour::{HourReport, RowCounts};
use crate::pq::{Staged, Table, for_each_batch, num_rows, props, tmp_of, write_atomic};
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use serde::{Deserialize, Serialize};
use std::fs::File;
use tape::time::parse_day;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub compact_version: String,
    pub day: String,
    pub config: Config,
    pub config_sha256: String,
    /// All 24 hours, in order.
    pub hours: Vec<HourReport>,
    pub truncated_windows: Vec<String>,
    pub totals: RowCounts,
    pub bytes: RowCounts,
    /// tob rows whose server ts_ms was raised to the token's previous one (ruling R4), all hours.
    pub ts_clamped: u64,
    /// tob/l2 rows written after the reorder window (ruling R4), all hours.
    pub late_rows: u64,
}

/// Day folders (YYYY-MM-DD names only; `_state` and strays are ignored), oldest first.
pub fn days(archive: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(archive)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
                .filter(|(n, _)| parse_day(n).is_some())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn merged(day: &Path) -> bool {
    day.join(MANIFEST).exists()
}

/// All 24 hours published (each has its sidecar).
pub fn complete(day: &Path) -> bool {
    (0..24u32).all(|hh| sidecar_path(day, hh).exists())
}

/// `HH.<anything>` (hourly parts, sidecars and their temporary files).
fn hourly(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 3 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[2] == b'.'
}

fn remove_leftovers(day: &Path) -> anyhow::Result<()> {
    for e in std::fs::read_dir(day)?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if hourly(&n) || n.ends_with(".tmp") {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

fn merge_table<R: Table>(day: &Path) -> anyhow::Result<Staged> {
    let fin = daily_path(day, R::NAME);
    let tmp = tmp_of(&fin);
    let file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut w = ArrowWriter::try_new(file, R::schema(), Some(props()?))?;
    let mut rows = 0u64;
    for hh in 0..24u32 {
        for_each_batch(&part_path(day, hh, R::NAME), |b| {
            let b = RecordBatch::try_new(R::schema(), b.columns().to_vec())?;
            rows += u64::try_from(b.num_rows())?;
            w.write(&b)?;
            Ok(())
        })?;
    }
    let _meta = w.close()?;
    Ok(Staged { tmp, fin, rows })
}

/// Merge a complete day (spec §4.4): daily files under temporary names, row counts checked
/// against the sidecars, then published, the manifest written last, the hourly parts deleted.
pub fn merge_day(day: &Path, name: &str, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Manifest> {
    let mut hours: Vec<HourReport> = Vec::with_capacity(24);
    for hh in 0..24u32 {
        let p = sidecar_path(day, hh);
        let bytes = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
        hours.push(serde_json::from_slice(&bytes).with_context(|| format!("parse {}", p.display()))?);
    }
    let sum = |f: fn(&RowCounts) -> u64| hours.iter().map(|h| f(&h.rows)).sum::<u64>();
    let tokens_fin = daily_path(day, TokenRow::NAME);
    let tokens_tmp = tmp_of(&tokens_fin);
    // the last hour's part holds the whole day's dictionary
    std::fs::copy(part_path(day, 23, TokenRow::NAME), &tokens_tmp)?;
    let staged = vec![
        Staged { rows: num_rows(&tokens_tmp)?, tmp: tokens_tmp, fin: tokens_fin },
        merge_table::<TobRow>(day)?,
        merge_table::<DepthRow>(day)?,
        merge_table::<L2Row>(day)?,
        merge_table::<GapRow>(day)?,
    ];
    let want = [
        hours.last().map_or(0, |h| h.rows.tokens),
        sum(|r| r.tob),
        sum(|r| r.depth),
        sum(|r| r.l2),
        sum(|r| r.gaps),
    ];
    for (s, w) in staged.iter().zip(want) {
        let got = num_rows(&s.tmp)?;
        if got != w || s.rows != w {
            for s in &staged {
                let _ = std::fs::remove_file(&s.tmp);
            }
            anyhow::bail!("merge {name}: {} has {got} rows, the hours say {w}", s.fin.display());
        }
    }
    for s in &staged {
        s.publish()?;
    }
    let size = |t: &str| std::fs::metadata(daily_path(day, t)).map(|m| m.len()).unwrap_or(0);
    let mut truncated: Vec<String> = hours.iter().flat_map(|h| h.truncated_windows.clone()).collect();
    truncated.sort();
    truncated.dedup();
    let m = Manifest {
        compact_version: crate::version().to_string(),
        day: name.to_string(),
        config: cfg.clone(),
        config_sha256: cfg_sha.to_string(),
        totals: RowCounts { tokens: want[0], tob: want[1], depth: want[2], l2: want[3], gaps: want[4] },
        bytes: RowCounts {
            tokens: size(TokenRow::NAME),
            tob: size(TobRow::NAME),
            depth: size(DepthRow::NAME),
            l2: size(L2Row::NAME),
            gaps: size(GapRow::NAME),
        },
        ts_clamped: hours.iter().map(|h| h.stats.ts_clamped).sum(),
        late_rows: hours.iter().map(|h| h.late_rows).sum(),
        hours,
        truncated_windows: truncated,
    };
    write_atomic(&day.join(MANIFEST), &serde_json::to_vec_pretty(&m)?)?;
    remove_leftovers(day)?;
    Ok(m)
}

/// Merge every complete, unmerged day.
pub fn merge_ready(archive: &Path, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Vec<Manifest>> {
    let mut out = Vec::new();
    for (name, dir) in days(archive) {
        if !merged(&dir) && complete(&dir) {
            out.push(merge_day(&dir, &name, cfg, cfg_sha)?);
        }
    }
    Ok(out)
}

/// At startup: remove what an interrupted merge left in merged days, then merge what is ready.
pub fn recover(archive: &Path, cfg: &Config, cfg_sha: &str) -> anyhow::Result<Vec<Manifest>> {
    for (_, dir) in days(archive) {
        if merged(&dir) {
            remove_leftovers(&dir)?;
        }
    }
    merge_ready(archive, cfg, cfg_sha)
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Every byte under the archive, checkpoints included.
pub fn archive_bytes(archive: &Path) -> u64 {
    dir_bytes(archive)
}

pub fn oldest_day(archive: &Path) -> Option<String> {
    days(archive).into_iter().next().map(|(n, _)| n)
}

/// While over `budget`, delete the oldest merged day (spec §4.4). Returns (day, bytes) deleted.
pub fn enforce_budget(archive: &Path, budget: u64) -> anyhow::Result<Vec<(String, u64)>> {
    let mut gone = Vec::new();
    while archive_bytes(archive) > budget {
        let Some((name, dir)) = days(archive).into_iter().find(|(_, d)| merged(d)) else {
            break;
        };
        let bytes = dir_bytes(&dir);
        std::fs::remove_dir_all(&dir).with_context(|| format!("delete {}", dir.display()))?;
        gone.push((name, bytes));
    }
    Ok(gone)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the four archive tests.

- [ ] **Step 5: Commit**

```bash
git add bins/compact/src/archive.rs bins/compact/tests/fixture/mod.rs bins/compact/tests/archive.rs
git commit -m "compact: daily merge with manifest and row-count check, crash recovery, 55 GB budget"
```

---

### Task 17: `compact verify <YYYY-MM-DD>`

Spec §6. Reads one merged day and reports every problem; the binary exits non-zero on any. Checks: each table's rows equal the manifest's totals (and the totals equal the sum of its hours); `tokens.token_idx` equals its row number; each `tob` row really is a change (never two identical tops in a row for a token); `ts_ms` never goes back per token (`tob`, `depth`); every `(episode_id, token_idx)` in `l2` starts with a `snapshot` row; no `tob`, `depth` or `l2` row falls inside a whole-feed gap (`shard` null). Ruling R6: shard gaps (`down`, `no_book_yet` with a shard) can't be checked against rows that carry no shard; the reducer tests pin that untrusted books make no rows.

**Files:**
- Create: `bins/compact/src/verify.rs`, `bins/compact/tests/verify.rs`
- Modify: `bins/compact/src/lib.rs`

**Interfaces:**
- Consumes: Task 16 (`Manifest`, `daily_path`, `build_day`, `merge_ready`).
- Produces: `compact::verify::{MAX_LISTED = 50, verify_day(archive: &Path, day: &str) -> anyhow::Result<Vec<String>>}` (empty = the day is good).

- [ ] **Step 1: Write the failing tests**

`bins/compact/tests/verify.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::*;
use compact::pq::{PartWriter, Table, read_all};
use compact::rows::*;
use compact::verify::verify_day;
use fixture::*;
use std::path::{Path, PathBuf};

fn good_day(name: &str) -> (PathBuf, PathBuf) {
    let (raw, arch) = (tmp(&format!("{name}-raw")), tmp(&format!("{name}-arch")));
    build_day(&raw, &arch, T0, &[3]);
    merge_ready(&arch, &cfg(&raw, &arch), "s").unwrap();
    (raw, arch)
}

/// Rewrite one daily table (tob, l2 or gaps) through `f`, keeping the manifest's totals in step
/// (all of this fixture's tob and l2 rows are in hour 0), so only `f`'s change is wrong.
fn rewrite<R: Table + Clone>(arch: &Path, f: impl FnOnce(&mut Vec<R>)) {
    let day = day_dir(arch, T0);
    let p = daily_path(&day, R::NAME);
    let mut rows: Vec<R> = read_all(&p).unwrap();
    f(&mut rows);
    let mut w = PartWriter::<R>::create(p).unwrap();
    for r in &rows {
        w.push(r.clone()).unwrap();
    }
    w.close().unwrap().publish().unwrap();
    let mp = day.join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    let n = u64::try_from(rows.len()).unwrap();
    let (total, hour0) = match R::NAME {
        "tob" => (&mut m.totals.tob, &mut m.hours[0].rows.tob),
        "l2" => (&mut m.totals.l2, &mut m.hours[0].rows.l2),
        _ => (&mut m.totals.gaps, &mut m.hours[0].rows.gaps),
    };
    *hour0 = *hour0 + n - *total; // every rewrite here adds rows or keeps the count
    *total = n;
    std::fs::write(&mp, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
}

fn fails(arch: &Path, needle: &str) {
    let p = verify_day(arch, "2026-09-23").unwrap();
    assert!(p.iter().any(|x| x.contains(needle)), "expected a problem containing {needle:?}, got {p:?}");
}

#[test]
fn verify_passes_on_a_good_day() {
    let (raw, arch) = good_day("vgood");
    assert_eq!(verify_day(&arch, "2026-09-23").unwrap(), Vec::<String>::new());
    assert!(verify_day(&arch, "2026-9-23").is_err(), "a bad day name is an error, not a pass");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_fails_on_each_kind_of_corruption() {
    // a manifest that disagrees with a file
    let (raw, arch) = good_day("vcount");
    let mp = day_dir(&arch, T0).join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    m.totals.depth += 1;
    std::fs::write(&mp, serde_json::to_vec(&m).unwrap()).unwrap();
    fails(&arch, "depth:");
    // a tob row that is not a change
    let (raw2, arch2) = good_day("vdup");
    rewrite::<TobRow>(&arch2, |rows| {
        let r = rows[0];
        rows.insert(1, TobRow { recv_ms: r.recv_ms, ..r });
    });
    fails(&arch2, "repeats its top");
    // a ts_ms that goes back
    let (raw3, arch3) = good_day("vts");
    rewrite::<TobRow>(&arch3, |rows| {
        let i = (1..rows.len()).find(|&i| rows[i].token_idx == rows[0].token_idx).unwrap();
        rows[i].ts_ms = rows[0].ts_ms - 1;
    });
    fails(&arch3, "ts_ms goes back");
    // an l2 window that does not start with a snapshot
    let (raw4, arch4) = good_day("vl2");
    rewrite::<L2Row>(&arch4, |rows| rows[0].snapshot = false);
    fails(&arch4, "does not start with a snapshot");
    // a row inside a whole-feed gap
    let (raw5, arch5) = good_day("vgap");
    let t = read_all::<TobRow>(&daily_path(&day_dir(&arch5, T0), "tob")).unwrap()[0].recv_ms;
    rewrite::<GapRow>(&arch5, |rows| rows.push(GapRow { start_ms: t, end_ms: t + 1, shard: None, reason: reason::SCANNER_STOPPED.into() }));
    fails(&arch5, "inside a gap");
    for d in [raw, arch, raw2, arch2, raw3, arch3, raw4, arch4, raw5, arch5] {
        std::fs::remove_dir_all(d).ok();
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p compact --test verify`
Expected: FAIL (no `compact::verify`).

- [ ] **Step 3: The checks**

`bins/compact/src/verify.rs`:
```rust
//! `compact verify <YYYY-MM-DD>` (spec 5b §6): read one merged day and report every problem
//! (the binary exits non-zero on any). Whole-feed gaps (shard null) are checked against the rows;
//! shard gaps can't be, because rows carry no shard (plan ruling R6).
use crate::archive::{MANIFEST, Manifest, daily_path, day_dir};
use crate::pq::{Table, for_each_batch, num_rows, read_all};
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tape::time::parse_day;

pub const MAX_LISTED: usize = 50;

#[derive(Default)]
struct Problems {
    list: Vec<String>,
    total: usize,
}

impl Problems {
    fn add(&mut self, s: String) {
        self.total += 1;
        if self.list.len() < MAX_LISTED {
            self.list.push(s);
        }
    }

    fn done(mut self) -> Vec<String> {
        if self.total > self.list.len() {
            let more = self.total - self.list.len();
            self.list.push(format!("… and {more} more"));
        }
        self.list
    }
}

type TopKey = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);

pub fn verify_day(archive: &Path, day: &str) -> anyhow::Result<Vec<String>> {
    let day_ms = parse_day(day).with_context(|| format!("{day}: expected YYYY-MM-DD"))?;
    let dir = day_dir(archive, day_ms);
    let mp = dir.join(MANIFEST);
    let m: Manifest = serde_json::from_slice(
        &std::fs::read(&mp).with_context(|| format!("read {}", mp.display()))?,
    )
    .with_context(|| format!("parse {}", mp.display()))?;
    let mut p = Problems::default();

    let sum = |f: fn(&crate::hour::RowCounts) -> u64| m.hours.iter().map(|h| f(&h.rows)).sum::<u64>();
    for (name, want, hours) in [
        (TokenRow::NAME, m.totals.tokens, m.hours.last().map_or(0, |h| h.rows.tokens)),
        (TobRow::NAME, m.totals.tob, sum(|r| r.tob)),
        (DepthRow::NAME, m.totals.depth, sum(|r| r.depth)),
        (L2Row::NAME, m.totals.l2, sum(|r| r.l2)),
        (GapRow::NAME, m.totals.gaps, sum(|r| r.gaps)),
    ] {
        let got = num_rows(&daily_path(&dir, name))?;
        if got != want {
            p.add(format!("{name}: {got} rows, the manifest says {want}"));
        }
        if hours != want {
            p.add(format!("{name}: the manifest's total {want} is not the sum of its hours {hours}"));
        }
    }

    let tokens: Vec<TokenRow> = read_all(&daily_path(&dir, TokenRow::NAME))?;
    for (i, t) in tokens.iter().enumerate() {
        if usize::try_from(t.token_idx).ok() != Some(i) {
            p.add(format!("tokens: row {i} has token_idx {}", t.token_idx));
        }
    }
    let n_tokens = tokens.len();
    let known = |idx: u32| usize::try_from(idx).is_ok_and(|i| i < n_tokens);

    let mut global: Vec<(i64, i64)> = read_all::<GapRow>(&daily_path(&dir, GapRow::NAME))?
        .into_iter()
        .filter(|g| g.shard.is_none())
        .map(|g| (g.start_ms, g.end_ms))
        .collect();
    global.sort();
    let in_gap = |t: i64| global.iter().any(|(s, e)| *s <= t && t < *e);

    let mut last: HashMap<u32, (TopKey, i64)> = HashMap::new();
    for_each_batch(&daily_path(&dir, TobRow::NAME), |b| {
        for r in TobRow::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("tob: token_idx {} is not in tokens", r.token_idx));
            }
            let key = (r.bid, r.bid_sz, r.ask, r.ask_sz);
            if let Some((k, ts)) = last.get(&r.token_idx) {
                if *k == key {
                    p.add(format!("tob: token_idx {} repeats its top at recv_ms {}", r.token_idx, r.recv_ms));
                }
                if r.ts_ms < *ts {
                    p.add(format!("tob: token_idx {} ts_ms goes back at recv_ms {}", r.token_idx, r.recv_ms));
                }
            }
            if in_gap(r.recv_ms) {
                p.add(format!("tob: a row inside a gap at recv_ms {}", r.recv_ms));
            }
            last.insert(r.token_idx, (key, r.ts_ms));
        }
        Ok(())
    })?;

    let mut depth_ts: HashMap<u32, i64> = HashMap::new();
    for_each_batch(&daily_path(&dir, DepthRow::NAME), |b| {
        for r in DepthRow::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("depth: token_idx {} is not in tokens", r.token_idx));
            }
            if depth_ts.get(&r.token_idx).is_some_and(|t| r.ts_ms < *t) {
                p.add(format!("depth: token_idx {} ts_ms goes back at {}", r.token_idx, r.ts_ms));
            }
            if in_gap(r.ts_ms) {
                p.add(format!("depth: a row inside a gap at ts_ms {}", r.ts_ms));
            }
            depth_ts.insert(r.token_idx, r.ts_ms);
        }
        Ok(())
    })?;

    let mut seen: HashSet<(String, u32)> = HashSet::new();
    for_each_batch(&daily_path(&dir, L2Row::NAME), |b| {
        for r in L2Row::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("l2: token_idx {} is not in tokens", r.token_idx));
            }
            if seen.insert((r.episode_id.clone(), r.token_idx)) && !r.snapshot {
                p.add(format!("l2: episode {} token_idx {} does not start with a snapshot", r.episode_id, r.token_idx));
            }
            if in_gap(r.recv_ms) {
                p.add(format!("l2: a row inside a gap at recv_ms {}", r.recv_ms));
            }
        }
        Ok(())
    })?;
    Ok(p.done())
}
```
`bins/compact/src/lib.rs`: add `pub mod verify;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p compact && cargo clippy -p compact --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: both verify tests.

- [ ] **Step 5: Commit**

```bash
git add bins/compact/src/lib.rs bins/compact/src/verify.rs bins/compact/tests/verify.rs
git commit -m "compact: verify a day (counts, real changes, time order, l2 snapshots, gaps)"
```

---

### Task 18: `compact`: the service, the binary and the benchmark

Spec §4.2–§4.4, §5, §8 (benchmark). Hour H is ready once the scanner has started an hour file at or after H + 2 h (so H + 1 is closed and every episode whose l2 window can reach into H has its row). Hours go strictly in order. The order of writes per hour is: parts and sidecar (Task 14), checkpoint (Task 15), then the `compact_hours` row; a missed database write is retried before the next hour and re-done from the checkpoint at startup. A database or file error sets `last_error`, and the next poll retries from the last checkpoint. `compact_status` is written every poll (60 s): an age over 2 h on the dashboard means the service is gone.

A missing raw hour (ruling R9, Review Focus 2): if a file for it was ever listed, only the scanner can have deleted it, so it is `lost` at once, whether or not the scanner's mark has arrived (a later mark is `ON CONFLICT DO NOTHING` against the compactor's `lost` row: nothing hidden). If no file was ever listed, it is `lost` when `compact_hours` says so, and otherwise `scanner_stopped` only once it has been ready for 5 minutes (`MISSING_GRACE_MS`), giving a just-deleted hour's mark time to land; until then the service waits.

**Files:**
- Create: `bins/compact/src/run.rs`, `bins/compact/src/bench.rs`, `bins/compact/src/main.rs`, `bins/compact/tests/common/mod.rs` (copy), `bins/compact/tests/service.rs`, `bins/compact/tests/bench.rs`
- Modify: `bins/compact/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 5, 8–17.
- Produces:
  - `compact::run`: `POLL_SECS = 60`, `STATEMENT_TIMEOUT_MS = 10_000`, `MISSING_GRACE_MS = 300_000`, `ready(next_hour: i64, hours: &[HourFile]) -> bool`, `row_of(&HourReport, now_ms) -> anyhow::Result<CompactHourRow>`, `Service::new(Config, cfg_sha: String)`, `Service::step(&mut self, &Store, now_ms) -> anyhow::Result<bool>` (true = one hour compacted), `Service::status(&mut self, now_ms) -> CompactStatusRow`, `Service::last_error: Option<String>` (pub field), `Service::next_hour() -> Option<i64>`.
  - `compact::bench::{bench(&Config, hour_file: &Path, out: &Path) -> anyhow::Result<String>, peak_rss_kb() -> Option<u64>}`.
  - The binary: `compact [--config PATH] run | verify <YYYY-MM-DD> [--archive DIR] | bench <HOUR_FILE> [--out DIR]` (default config `/etc/cute-trading/compact.toml`).

- [ ] **Step 1: Write the failing tests**

```bash
cp bins/analyze/tests/common/mod.rs bins/compact/tests/common/mod.rs
```
(The same throwaway-database helper analyze's tests use: `fresh()`, `TestDb { opts, … }`, `role()`, `drop()`.)

`bins/compact/tests/service.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
mod fixture;
use compact::archive::{day_dir, part_path, sidecar_path};
use compact::pq::read_all;
use compact::rows::*;
use compact::run::{Service, ready};
use fixture::*;
use rust_decimal_macros::dec;
use std::path::{Path, PathBuf};
use store::*;
use tape::files::HourFile;

/// Raw hours `present` (hour indexes from T0): hour 0 has a universe and two books, later ones one
/// change each, so each file exists and depends on the books carried over.
fn raw_hours(raw: &Path, present: &[i64]) {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(raw, &r);
    for &i in present {
        let t = T0 + i * H;
        let lines = if i == 0 {
            vec![
                uni(t + 1_000, &r),
                frame(t + 2_000, 0, book("y1", t + 1_990, &[("0.40", "10")], &[("0.45", "5")])),
                frame(t + 2_100, 0, book("y2", t + 2_090, &[("0.20", "3")], &[])),
                frame(t + 700_000, 0, change("y1", t + 699_995, "BUY", "0.41", "1", "0.41", "0.45")),
            ]
        } else {
            vec![frame(t + 1_000, 0, change("y2", t + 995, "BUY", &format!("0.{:02}", 20 + i), "1", &format!("0.{:02}", 20 + i), "1"))]
        };
        write_hour(raw, t, &lines);
    }
}

async fn compact_store(db: &common::TestDb) -> Store {
    Store::connect_compact(db.opts.clone(), 2, 10_000, Some("compact_rw")).await.unwrap()
}

fn service(raw: &Path, arch: &Path) -> Service {
    Service::new(cfg(raw, arch), "cfgsha".into())
}

fn hf(i: i64) -> HourFile {
    HourFile { start_ms: T0 + i * H, path: PathBuf::from(format!("{i}")), bytes: 1 }
}

#[test]
fn an_hour_is_ready_once_the_next_one_has_closed() {
    assert!(!ready(T0, &[hf(0), hf(1)]), "hour 1 is still being written");
    assert!(ready(T0, &[hf(0), hf(1), hf(2)]));
    assert!(ready(T0, &[hf(0), hf(5)]), "the scanner skipped hours: 1 and later are closed");
}

#[tokio::test]
async fn the_service_compacts_ready_hours_in_order_and_records_them() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (raw, arch) = (tmp("svc-raw"), tmp("svc-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    let now = T0 + 4 * H;
    assert!(svc.step(&c, now).await.unwrap(), "hour 0");
    assert!(svc.step(&c, now).await.unwrap(), "hour 1");
    assert!(!svc.step(&c, now).await.unwrap(), "hour 2 waits for hour 4");
    assert_eq!(svc.next_hour(), Some(T0 + 2 * H));
    assert_eq!(owner.compact_hour_statuses(T0, T0 + 10 * H).await.unwrap(), vec![(T0, "done".to_string()), (T0 + H, "done".to_string())]);
    let day = day_dir(&arch, T0);
    assert!(sidecar_path(&day, 0).exists() && sidecar_path(&day, 1).exists() && !sidecar_path(&day, 2).exists());
    assert_eq!(compact::state::list(&arch).len(), 2);
    let st = svc.status(now);
    assert_eq!((st.lag_hours, st.oldest_day.as_deref(), st.last_error.as_deref()), (2, Some("2026-09-23"), None));
    assert!(st.archive_bytes > 0);
    c.compact_status_put(&st).await.unwrap();
    assert_eq!(owner.compact_status().await.unwrap(), Some(st));
    // a restarted service resumes after its checkpoint, never redoing hour 0 or 1
    raw_hours(&raw, &[4]);
    let mut again = service(&raw, &arch);
    assert!(again.step(&c, now + H).await.unwrap());
    assert_eq!(again.next_hour(), Some(T0 + 3 * H));
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_missing_hour_marked_lost_is_compacted_as_lost() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (raw, arch) = (tmp("lost-raw"), tmp("lost-arch"));
    raw_hours(&raw, &[0, 2, 3]);
    owner.mark_hours_lost(&[T0 + H], T0 + 2 * H).await.unwrap();
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap());
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap());
    assert_eq!(owner.compact_hour_statuses(T0 + H, T0 + 2 * H).await.unwrap(), vec![(T0 + H, "lost".to_string())]);
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(gaps, vec![GapRow { start_ms: T0 + H, end_ms: T0 + 2 * H, shard: None, reason: reason::RAW_HOUR_LOST.into() }]);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_missing_hour_nobody_marked_is_scanner_stopped_and_done() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (raw, arch) = (tmp("stop-raw"), tmp("stop-arch"));
    raw_hours(&raw, &[0, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 0");
    assert!(!svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 1 never listed, no mark: wait for a mark to land");
    assert!(!svc.step(&c, T0 + 4 * H + 299_999).await.unwrap(), "still inside the 5-minute grace");
    assert!(svc.step(&c, T0 + 4 * H + 300_000).await.unwrap(), "then: the scanner was not running");
    assert_eq!(owner.compact_hour_statuses(T0 + H, T0 + 2 * H).await.unwrap(), vec![(T0 + H, "done".to_string())]);
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(gaps[0].reason, reason::SCANNER_STOPPED);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn a_file_deleted_before_its_lost_mark_is_compacted_as_lost() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (raw, arch) = (tmp("race-raw"), tmp("race-arch"));
    raw_hours(&raw, &[0, 1, 2, 3]);
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 0; hours 1–3 listed");
    // the scanner deletes hour 1 at its cap; its lost mark has not reached the database yet
    std::fs::remove_file(tape::files::hour_path(&raw, T0 + H)).unwrap();
    assert!(svc.step(&c, T0 + 4 * H).await.unwrap(), "hour 1 at once: it was listed, so it was deleted");
    assert_eq!(owner.compact_hour_statuses(T0 + H, T0 + 2 * H).await.unwrap(), vec![(T0 + H, "lost".to_string())]);
    // the mark arrives afterwards: nothing changes, and the loss stays visible
    assert_eq!(owner.mark_hours_lost(&[T0 + H], T0 + 4 * H).await.unwrap(), 0);
    assert_eq!(owner.compact_hour_statuses(T0 + H, T0 + 2 * H).await.unwrap(), vec![(T0 + H, "lost".to_string())]);
    let gaps = read_all::<GapRow>(&part_path(&day_dir(&arch, T0), 1, "gaps")).unwrap();
    assert_eq!(gaps[0].reason, reason::RAW_HOUR_LOST);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn l2_windows_come_from_live_net_positive_episodes() {
    let Some((owner, db)) = common::fresh().await else { return };
    let (raw, arch) = (tmp("l2-raw"), tmp("l2-arch"));
    raw_hours(&raw, &[0, 1, 2]);
    let run_id = owner
        .start_run(&NewRun { service: "scanner".into(), started_ms: T0, version: "t".into(), config_hash: "c".into(), mode: "live".into() })
        .await
        .unwrap();
    let e = EpisodeRow {
        episode_id: "live-1-00000001".into(),
        run_id,
        source: "live".into(),
        kind: "no_basket".into(),
        realization: "converter".into(),
        event_id: "e1".into(),
        event_slug: None,
        event_title: None,
        neg_risk_market_id: None,
        pair_id: None,
        category: None,
        n_outcomes: 2,
        n_legs: 2,
        legs_dropped: 0,
        augmented: false,
        incomplete_set: false,
        uncovered_legs: 0,
        opened_server_ms: T0 + 600_000,
        closed_server_ms: Some(T0 + 650_000),
        duration_ms: Some(50_000),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        detect_lag_ms: None,
        n_updates: 1,
        peak_server_ms: T0 + 600_000,
        peak_edge_top: dec!(0.0049),
        peak_edge_top_gross: dec!(0.0137),
        peak_max_size: dec!(83),
        peak_cost_usd: dec!(99.59),
        peak_payout_usd: dec!(100),
        peak_fees_usd: dec!(0.96),
        peak_pnl_net_usd: dec!(0.41),
        peak_pnl_gross_usd: dec!(1.37),
        peak_profit_per_dollar: None,
        pnl_net_budget100_usd: None,
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
    };
    let losing = EpisodeRow { episode_id: "live-1-00000002".into(), peak_pnl_net_usd: dec!(-0.1), ..e.clone() };
    owner
        .write_batch(&Batch { episodes: vec![EpisodeWrite { row: e, legs: vec![] }, EpisodeWrite { row: losing, legs: vec![] }], ..Batch::default() })
        .await
        .unwrap();
    let c = compact_store(&db).await;
    let mut svc = service(&raw, &arch);
    assert!(svc.step(&c, T0 + 3 * H).await.unwrap());
    let l2 = read_all::<L2Row>(&part_path(&day_dir(&arch, T0), 0, "l2")).unwrap();
    assert!(!l2.is_empty(), "the window [300 000, 950 000) covers the 700 000 change");
    assert!(l2.iter().all(|r| r.episode_id == "live-1-00000001"), "only the net-positive episode");
    assert!(l2[0].snapshot);
    c.close().await;
    db.drop(owner).await;
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[tokio::test]
async fn the_service_refuses_a_role_that_writes_other_tables() {
    let Some((owner, db)) = common::fresh().await else { return };
    assert!(compact::refuse_other_writes(&owner.role_writes_outside_compact().await.unwrap()).is_err());
    let c = compact_store(&db).await;
    assert!(compact::refuse_other_writes(&c.role_writes_outside_compact().await.unwrap()).is_ok());
    c.close().await;
    db.drop(owner).await;
}
```
`bins/compact/tests/bench.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use fixture::*;
use std::path::Path;

#[test]
fn bench_reports_time_rows_and_bytes_for_one_hour() {
    let (raw, out) = (tmp("bench-raw"), tmp("bench-out"));
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    write_record(&raw, &r);
    write_hour(&raw, T0 + 13 * H, &[uni(T0 + 13 * H + 1_000, &r), frame(T0 + 13 * H + 2_000, 0, book("y1", T0 + 13 * H + 1_990, &[("0.40", "1")], &[]))]);
    let (c, _) = compact::config::Config::parse(include_str!("../../../config/compact.toml")).unwrap();
    let line = compact::bench::bench(&c, &tape::files::hour_path(&raw, T0 + 13 * H), &out).unwrap();
    assert!(line.starts_with("bench: 2026-09-23/13.jsonl.zst "), "{line}");
    assert!(line.contains(" tob=1 ") && line.contains("per_day_bytes=") && line.contains("peak_rss_kb="), "{line}");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&out).ok();
}

/// Spec §8 benchmark, by hand: `COMPACT_BENCH_HOUR=<dir>/YYYY-MM-DD/HH.jsonl.zst` (the universe
/// records the hour names in the same tree). Debug build: a correctness run on real data, not the
/// timing (the timing is the release run in the runbook, §1).
#[test]
#[ignore]
fn bench_one_real_hour() {
    let Ok(p) = std::env::var("COMPACT_BENCH_HOUR") else {
        eprintln!("SKIPPED: COMPACT_BENCH_HOUR not set");
        return;
    };
    let (c, _) = compact::config::Config::parse(include_str!("../../../config/compact.toml")).unwrap();
    let out = tmp("bench-real");
    eprintln!("{}", compact::bench::bench(&c, Path::new(&p), &out).unwrap());
    std::fs::remove_dir_all(&out).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run (Postgres prefix): `cargo test -p compact --test service --test bench`
Expected: FAIL (no `compact::run`, no `compact::bench`).

- [ ] **Step 3: The service**

`bins/compact/src/run.rs`:
```rust
//! The service (spec 5b §4): compact every ready raw hour in order, record it in compact_hours,
//! merge whole days, keep the archive under its budget, and report compact_status. A database or
//! disk hiccup never stops it: the error is reported and the next poll resumes from the last
//! checkpoint.
use crate::archive;
use crate::config::Config;
use crate::hour::{HourJob, HourReport, Source, process_hour};
use crate::reduce::Reducer;
use crate::state::{self, Checkpoint};
use crate::windows::{query_range, windows_for_hour};
use anyhow::Context;
use std::collections::BTreeSet;
use std::path::PathBuf;
use store::{CompactHourRow, CompactStatusRow, Store};
use tape::files::HourFile;
use tape::time::{HOUR_MS, hour_label};

pub const POLL_SECS: u64 = 60;
pub const STATEMENT_TIMEOUT_MS: u32 = 10_000;
/// A ready hour with no file ever listed and no lost mark waits this long before it is taken as
/// `scanner_stopped` (a just-deleted hour's mark needs time to land; ruling R9).
pub const MISSING_GRACE_MS: i64 = 300_000;

/// Hour H is ready once the scanner has started an hour ≥ H + 2 h: H + 1 is closed, so every
/// l2 window that reaches into H has its episode row (spec §4.2).
pub fn ready(next_hour: i64, hours: &[HourFile]) -> bool {
    hours.iter().any(|h| h.start_ms >= next_hour + 2 * HOUR_MS)
}

pub fn row_of(r: &HourReport, now_ms: i64) -> anyhow::Result<CompactHourRow> {
    Ok(CompactHourRow {
        hour_ms: r.hour_ms,
        status: r.status.clone(),
        source_file: r.source_file.clone(),
        source_bytes: r.source_bytes.map(i64::try_from).transpose()?,
        source_sha256: r.source_sha256.clone(),
        rows: Some(serde_json::to_string(&r.rows)?),
        done_ms: now_ms,
    })
}

pub struct Service {
    cfg: Config,
    cfg_sha: String,
    raw: PathBuf,
    archive: PathBuf,
    red: Option<Reducer>,
    next_hour: Option<i64>,
    pending_row: Option<CompactHourRow>,
    pub last_error: Option<String>,
    lag_alarm: bool,
    /// Hour files seen in any listing (≥ next_hour): a seen hour that is gone was deleted.
    seen: BTreeSet<i64>,
    /// (hour, when it was first seen ready with no file and no mark).
    missing_since: Option<(i64, i64)>,
}

impl Service {
    pub fn new(cfg: Config, cfg_sha: String) -> Service {
        Service {
            raw: PathBuf::from(&cfg.raw_dir),
            archive: PathBuf::from(&cfg.archive_dir),
            cfg,
            cfg_sha,
            red: None,
            next_hour: None,
            pending_row: None,
            last_error: None,
            lag_alarm: false,
            seen: BTreeSet::new(),
            missing_since: None,
        }
    }

    pub fn next_hour(&self) -> Option<i64> {
        self.next_hour
    }

    /// Load the newest checkpoint (or start cold) and finish any merge a crash interrupted.
    fn load(&mut self, now_ms: i64) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.archive)
            .with_context(|| format!("create {}", self.archive.display()))?;
        match state::load_newest(&self.archive)? {
            Some(cp) => {
                self.next_hour = Some(cp.hour_ms + HOUR_MS);
                self.pending_row = Some(row_of(&cp.report, now_ms)?);
                self.red = Some(Reducer::from_state(self.cfg.reduce(), cp.reducer)?);
            }
            None => {
                self.next_hour = None;
                self.red = Some(Reducer::new(self.cfg.reduce()));
            }
        }
        for m in archive::recover(&self.archive, &self.cfg, &self.cfg_sha)? {
            eprintln!("compact: merged {} (recovered), {} B", m.day, m.bytes.total());
        }
        Ok(())
    }

    /// Compact the next hour if it is ready. Ok(true) = one hour done; Ok(false) = nothing ready.
    pub async fn step(&mut self, store: &Store, now_ms: i64) -> anyhow::Result<bool> {
        if self.red.is_none() {
            self.load(now_ms)?;
        }
        if let Some(row) = self.pending_row.clone() {
            store.compact_hour_put(&row).await.context("record the last hour")?;
            self.pending_row = None;
        }
        let listing = tape::files::listing(&self.raw);
        let Some(next) = self
            .next_hour
            .or_else(|| listing.hours.first().map(|h| h.start_ms))
        else {
            return Ok(false);
        };
        if !ready(next, &listing.hours) {
            return Ok(false);
        }
        let (from, to) = query_range(next, &self.cfg);
        let eps = store.compact_episodes(from, to).await.context("read episodes")?;
        let windows = windows_for_hour(&eps, next, &self.cfg);
        for h in &listing.hours {
            if h.start_ms >= next {
                self.seen.insert(h.start_ms);
            }
        }
        self.seen.retain(|h| *h >= next);
        let source = match listing.hours.iter().find(|h| h.start_ms == next) {
            Some(h) => Source::File(h.path.clone()),
            // listed before, gone now: only the scanner deletes raw hours (its mark may be in flight)
            None if self.seen.contains(&next) => Source::Lost,
            None => {
                let st = store
                    .compact_hour_statuses(next, next + HOUR_MS)
                    .await
                    .context("read compact_hours")?;
                if st.iter().any(|(_, s)| s == "lost") {
                    Source::Lost
                } else {
                    let since = match self.missing_since {
                        Some((h, t)) if h == next => t,
                        _ => {
                            self.missing_since = Some((next, now_ms));
                            now_ms
                        }
                    };
                    if now_ms.saturating_sub(since) < MISSING_GRACE_MS {
                        return Ok(false);
                    }
                    Source::Stopped
                }
            }
        };
        let job = HourJob {
            hour_ms: next,
            raw_dir: self.raw.clone(),
            archive_dir: self.archive.clone(),
            source,
            windows,
            stop_before_publish: false,
        };
        let mut red = self.red.take().context("reducer not loaded")?;
        let t0 = std::time::Instant::now();
        let (red, res) = tokio::task::spawn_blocking(move || {
            let r = process_hour(&mut red, &job);
            (red, r)
        })
        .await
        .context("hour task")?;
        // On an error `red` is dropped: the next step reloads the last checkpoint and redoes the hour.
        let report = res?.context("hour stopped before publish")?;
        state::save(
            &self.archive,
            &Checkpoint {
                hour_ms: next,
                report: report.clone(),
                reducer: red.state(),
            },
        )?;
        self.red = Some(red);
        self.next_hour = Some(next + HOUR_MS);
        let row = row_of(&report, now_ms)?;
        self.pending_row = Some(row.clone());
        store.compact_hour_put(&row).await.context("record the hour")?;
        self.pending_row = None;
        eprintln!(
            "compact: {} done in {} ms, tob={} depth={} l2={} gaps={}, {} B",
            hour_label(next),
            t0.elapsed().as_millis(),
            report.rows.tob,
            report.rows.depth,
            report.rows.l2,
            report.rows.gaps,
            report.bytes.total()
        );
        if let Some(e) = &report.read_error {
            eprintln!("compact: {} read error ({e}); the rest of the hour is a raw_hour_lost gap", hour_label(next));
        }
        let (arch, cfg, sha) = (self.archive.clone(), self.cfg.clone(), self.cfg_sha.clone());
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            for m in archive::merge_ready(&arch, &cfg, &sha)? {
                eprintln!(
                    "compact: merged {}, tob={} depth={} l2={} gaps={}, {} B",
                    m.day, m.totals.tob, m.totals.depth, m.totals.l2, m.totals.gaps, m.bytes.total()
                );
            }
            for (day, bytes) in archive::enforce_budget(&arch, cfg.budget_bytes())? {
                eprintln!("compact: budget: deleted {day} ({bytes} B) to stay under {} GB", cfg.archive_budget_gb);
            }
            Ok(())
        })
        .await
        .context("merge task")??;
        Ok(true)
    }

    /// The compact_status row (spec §5). Lag = whole hours between the end of the last compacted
    /// hour and now (1–2 in steady state).
    pub fn status(&mut self, now_ms: i64) -> CompactStatusRow {
        let lag = self
            .next_hour
            .map_or(0, |n| now_ms.saturating_sub(n).div_euclid(HOUR_MS).max(0));
        if lag > self.cfg.lag_alarm_hours && !self.lag_alarm {
            eprintln!(
                "compact: {lag} h behind (lag_alarm_hours {}): raw hours are at risk",
                self.cfg.lag_alarm_hours
            );
        }
        self.lag_alarm = lag > self.cfg.lag_alarm_hours;
        CompactStatusRow {
            updated_ms: now_ms,
            archive_bytes: i64::try_from(archive::archive_bytes(&self.archive)).unwrap_or(i64::MAX),
            oldest_day: archive::oldest_day(&self.archive),
            lag_hours: i32::try_from(lag).unwrap_or(i32::MAX),
            last_error: self.last_error.clone(),
        }
    }
}
```
`bins/compact/src/bench.rs`:
```rust
//! `compact bench <HOUR_FILE>` (spec 5b §8): compact one raw hour with no database (no l2 windows)
//! and print the time, rows, bytes per table, a per-day projection and peak memory. The deploy
//! runbook runs it in the release image with --cpus 0.25 --memory 256m.
use crate::config::Config;
use crate::hour::{HourJob, Source, process_hour};
use crate::reduce::Reducer;
use anyhow::Context;
use std::path::Path;

/// Peak resident memory of this process (Linux VmHWM), in kB.
pub fn peak_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

pub fn bench(cfg: &Config, hour_file: &Path, out: &Path) -> anyhow::Result<String> {
    let name = hour_file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let day_dir = hour_file.parent().context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    let day = day_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let raw = day_dir.parent().context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    let hour_ms = tape::files::hour_start_ms(&day, &name).context("expected <raw>/YYYY-MM-DD/HH.jsonl.zst")?;
    std::fs::create_dir_all(out)?;
    let job = HourJob {
        hour_ms,
        raw_dir: raw.to_path_buf(),
        archive_dir: out.to_path_buf(),
        source: Source::File(hour_file.to_path_buf()),
        windows: Vec::new(),
        stop_before_publish: false,
    };
    let t0 = std::time::Instant::now();
    let r = process_hour(&mut Reducer::new(cfg.reduce()), &job)?.context("not published")?;
    let ms = t0.elapsed().as_millis();
    let per_day = (r.bytes.tokens + r.bytes.tob + r.bytes.depth + r.bytes.gaps).saturating_mul(24);
    Ok(format!(
        "bench: {day}/{name} {ms} ms, lines={} bad_lines={} universe_missing={} late_rows={}, tob={} depth={} gaps={} tokens={}, bytes tob={} depth={} gaps={} tokens={}, per_day_bytes={per_day} (l2 not measured: no episodes), peak_rss_kb={}",
        r.stats.lines,
        r.stats.bad_lines,
        r.stats.universe_missing,
        r.late_rows,
        r.rows.tob,
        r.rows.depth,
        r.rows.gaps,
        r.rows.tokens,
        r.bytes.tob,
        r.bytes.depth,
        r.bytes.gaps,
        r.bytes.tokens,
        peak_rss_kb().map_or_else(|| "n/a".to_string(), |k| k.to_string())
    ))
}
```
`bins/compact/src/main.rs`:
```rust
use clap::{Parser, Subcommand};
use compact::config::Config;
use compact::run::{POLL_SECS, STATEMENT_TIMEOUT_MS, Service};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::{PgConnectOptions, Store};

#[derive(Parser)]
#[command(name = "compact", about = "raw capture hours → the Parquet archive (paper trading only)")]
struct Cli {
    #[arg(long, global = true, default_value = "/etc/cute-trading/compact.toml")]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the service (compact_rw via DATABASE_URL).
    Run,
    /// Check one archived day (YYYY-MM-DD); exits non-zero on any problem.
    Verify {
        day: String,
        /// The archive folder (default: archive_dir from the config).
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Compact one raw hour file with no database; print time, rows, bytes and peak memory.
    Bench {
        hour_file: PathBuf,
        #[arg(long, default_value = "/tmp/compact-bench")]
        out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("compact: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    compact::guard::check_env()?;
    let cli = Cli::parse();
    let (cfg, sha) = Config::load(&cli.config)?;
    match cli.cmd {
        Cmd::Run => run(cfg, sha).await,
        Cmd::Verify { day, archive } => {
            let dir = archive.unwrap_or_else(|| PathBuf::from(&cfg.archive_dir));
            let problems = compact::verify::verify_day(&dir, &day)?;
            if problems.is_empty() {
                println!("verify {day}: ok");
                return Ok(());
            }
            for p in &problems {
                println!("{p}");
            }
            anyhow::bail!("verify {day}: {} problem(s)", problems.len())
        }
        Cmd::Bench { hour_file, out } => {
            println!("{}", compact::bench::bench(&cfg, &hour_file, &out)?);
            Ok(())
        }
    }
}

/// Connect and check the role. `Err`: a refusal (fatal). `Ok(Err)`: unreachable, retried next poll.
async fn connect(opts: &PgConnectOptions) -> anyhow::Result<anyhow::Result<Store>> {
    let s = match Store::connect_compact(opts.clone(), 2, STATEMENT_TIMEOUT_MS, None).await {
        Ok(s) => s,
        Err(e) => return Ok(Err(e.into())),
    };
    let bad = match s.role_writes_outside_compact().await {
        Ok(b) => b,
        Err(e) => {
            s.close().await;
            return Ok(Err(e.into()));
        }
    };
    if let Err(refusal) = compact::refuse_other_writes(&bad) {
        s.close().await;
        return Err(refusal);
    }
    Ok(Ok(s))
}

async fn run(cfg: Config, sha: String) -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the compact_rw URL)"))?;
    let opts = Store::options(&url)?;
    eprintln!(
        "compact: every {POLL_SECS} s, {} → {} (writes compact_hours/compact_status only, paper only)",
        cfg.raw_dir, cfg.archive_dir
    );
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(tokio::sync::Notify::new());
    {
        let (stop, wake) = (stop.clone(), wake.clone());
        tokio::spawn(async move {
            shutdown().await;
            stop.store(true, Ordering::Relaxed);
            wake.notify_one();
        });
    }
    let mut svc = Service::new(cfg, sha);
    let mut every = tokio::time::interval(Duration::from_secs(POLL_SECS));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut store: Option<Store> = None;
    loop {
        tokio::select! {
            _ = wake.notified() => {}
            _ = every.tick() => {}
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if store.is_none() {
            match connect(&opts).await? {
                Ok(s) => store = Some(s),
                Err(e) => {
                    eprintln!("compact: database unreachable, retrying next poll: {e:#}");
                    continue;
                }
            }
        }
        let Some(s) = store.as_ref() else {
            continue;
        };
        // An hour in progress is never interrupted here; a SIGKILL mid-hour is safe (atomic writes).
        while !stop.load(Ordering::Relaxed) {
            match svc.step(s, compact::now_ms()).await {
                Ok(true) => svc.last_error = None,
                Ok(false) => {
                    svc.last_error = None;
                    break;
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    eprintln!("compact: {msg}");
                    svc.last_error = Some(msg);
                    break;
                }
            }
        }
        let st = svc.status(compact::now_ms());
        if let Err(e) = s.compact_status_put(&st).await {
            eprintln!("compact: status not written: {e:#}");
        }
    }
    if let Some(s) = store {
        s.close().await;
    }
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
`bins/compact/src/lib.rs`: add `pub mod bench;` and `pub mod run;`.

(`Ok(true)` clearing `last_error` keeps an error on the dashboard until the service has really recovered.)

- [ ] **Step 4: Run the tests**

Run (Postgres prefix): `cargo test -p compact && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: PASS: the seven service tests (none `SKIPPED`), the bench test, and every earlier compact test; `bench_one_real_hour` is ignored.

- [ ] **Step 5: The whole workspace**

Run (Postgres prefix): `cargo test --workspace && sh scripts/disk-budget.sh`
Expected: PASS; the budget passes (`cargo clean` first if `target/` is over 8 GB).

- [ ] **Step 6: Commit**

```bash
git add bins/compact/src/lib.rs bins/compact/src/run.rs bins/compact/src/bench.rs bins/compact/src/main.rs bins/compact/tests/common/mod.rs bins/compact/tests/service.rs bins/compact/tests/bench.rs
git commit -m "compact: the service (ready hours in order, compact_hours, merge, budget, status), verify and bench commands"
```

---

### Task 19: api and web: the archive on Health, one heart for the compactor

Spec §5. `/api/health/now` gains `archive_bytes`, `archive_oldest_day`, `compact_lag_hours`, `compact_last_error` and `compact_age_ms` (now − `compact_status.updated_ms`), all null until the compactor first reports. Radar loses one heart when `compact_lag_hours > 3` (`compactor behind: raw at risk`) or, if not, when the status is more than 2 h old (`compactor status older than 2 h`), or when no status row exists 2 h after the first `compact_hours` row (`compactor has never reported`, e.g. the scanner is marking hours lost with no compactor running). The Health page gets an **ARCHIVE** box.

**Files:**
- Modify: `bins/api/src/health.rs`, `bins/api/tests/health.rs`, `web/src/api/types.ts`, `web/src/test/fixtures.ts`, `web/src/components/Hearts.tsx`, `web/src/components/Hearts.test.tsx`, `web/src/pages/Health.tsx`, `web/src/pages/Health.test.tsx`, `web/src/help.ts`
- Create: `web/src/lib/bytes.ts`, `web/src/lib/bytes.test.ts`

**Interfaces:**
- Consumes: Task 5 (`Store::compact_status`, `Store::compact_first_done_ms`, `CompactStatusRow`).
- Produces: `HealthNowDto` (and the web `HealthNow`) fields `archive_bytes: i64?`, `archive_oldest_day: String?`, `compact_lag_hours: i32?`, `compact_last_error: String?`, `compact_age_ms: i64?`, `compact_started_ms: i64?` (the first `compact_hours.done_ms`); web `fmtGb(bytes: number): string`, `NO_ARCHIVE`.

- [ ] **Step 1: Write the failing api test**

Append to `bins/api/tests/health.rs`:
```rust
#[tokio::test]
async fn health_now_carries_the_archive_status_once_the_compactor_reports() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    let t = n - n % 60_000;
    owner
        .write_batch(&Batch { health: vec![common::health(live, t, 0)], ..Batch::default() })
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (_, v) = common::get(&app, "/api/health/now").await;
    for k in ["archive_bytes", "archive_oldest_day", "compact_lag_hours", "compact_last_error", "compact_age_ms", "compact_started_ms"] {
        assert!(v["health"][k].is_null(), "{k} before any compact_status: {v}");
    }
    owner.mark_hours_lost(&[t - 7_200_000], n - 10_800_000).await.unwrap();
    owner
        .compact_status_put(&CompactStatusRow {
            updated_ms: n - 600_000,
            archive_bytes: 12_345_678_901,
            oldest_day: Some("2026-09-28".into()),
            lag_hours: 4,
            last_error: Some("read episodes: timeout".into()),
        })
        .await
        .unwrap();
    let (status, v) = common::get(&app, "/api/health/now").await;
    assert_eq!(status, StatusCode::OK);
    let h = &v["health"];
    assert_eq!(h["archive_bytes"], 12_345_678_901_i64);
    assert_eq!(h["archive_oldest_day"], "2026-09-28");
    assert_eq!(h["compact_lag_hours"], 4);
    assert_eq!(h["compact_last_error"], "read episodes: timeout");
    let age = h["compact_age_ms"].as_i64().unwrap();
    assert!((600_000..700_000).contains(&age), "{age}");
    assert_eq!(h["compact_started_ms"], n - 10_800_000);
    db.drop(owner).await;
}
```
Run (Postgres prefix): `cargo test -p api --test health health_now_carries_the_archive`
Expected: FAIL (the fields are missing).

- [ ] **Step 2: The api fields**

`bins/api/src/health.rs`: import `store::CompactStatusRow` (`use store::{CompactStatusRow, HealthNow, HealthRow};`), add to `HealthNowDto` after `dropped_groups`:
```rust
    /// Phase 5b archive (compact_status): all null until the compactor first reports.
    pub archive_bytes: Option<i64>,
    pub archive_oldest_day: Option<String>,
    pub compact_lag_hours: Option<i32>,
    pub compact_last_error: Option<String>,
    /// now − compact_status.updated_ms (server clock).
    pub compact_age_ms: Option<i64>,
    /// When the first hour was recorded in compact_hours (done or lost), if any.
    pub compact_started_ms: Option<i64>,
```
change `fn new(h: HealthNow, now: i64) -> Self` to `fn new(h: HealthNow, now: i64, c: Option<CompactStatusRow>, started: Option<i64>) -> Self` and add to its struct literal:
```rust
            archive_bytes: c.as_ref().map(|c| c.archive_bytes),
            archive_oldest_day: c.as_ref().and_then(|c| c.oldest_day.clone()),
            compact_lag_hours: c.as_ref().map(|c| c.lag_hours),
            compact_last_error: c.as_ref().and_then(|c| c.last_error.clone()),
            compact_age_ms: c.as_ref().map(|c| now.saturating_sub(c.updated_ms).max(0)),
            compact_started_ms: started,
```
and in `now`:
```rust
pub async fn now(State(st): State<AppState>) -> Result<Json<HealthEnvelope>, ApiError> {
    let now = now_ms();
    let h = st.store.health_now(now).await?;
    let c = st.store.compact_status().await?;
    let started = st.store.compact_first_done_ms().await?;
    Ok(Json(HealthEnvelope {
        health: h.map(|h| HealthNowDto::new(h, now, c, started)),
    }))
}
```
Run (Postgres prefix): `cargo test -p api`
Expected: PASS.

- [ ] **Step 3: Write the failing web tests**

`web/src/lib/bytes.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { fmtGb } from './bytes';

describe('fmtGb', () => {
  it('shows GB (10^9 bytes, like the budgets) with one decimal, rounded down, no float formatting', () => {
    expect(fmtGb(12_345_678_901)).toBe('12.3 GB');
    expect(fmtGb(55_000_000_000)).toBe('55.0 GB');
    expect(fmtGb(99_999_999)).toBe('0.0 GB');
    expect(fmtGb(0)).toBe('0.0 GB');
  });
});
```
Append to the `describe('Hearts', …)` block in `web/src/components/Hearts.test.tsx` (and add `archive_bytes: 12_400_000_000, archive_oldest_day: '2026-09-28', compact_lag_hours: 1, compact_last_error: null, compact_age_ms: 600_000, compact_started_ms: -36_000_000,` to its `ok` literal):
```ts
  it('costs one heart when the compactor is behind or silent, never two', () => {
    expect(troubles({ ...ok, compact_lag_hours: 4 })).toEqual(['compactor behind: raw at risk']);
    expect(troubles({ ...ok, compact_age_ms: 7_200_001 })).toEqual(['compactor status older than 2 h']);
    expect(troubles({ ...ok, compact_lag_hours: 9, compact_age_ms: 9_000_000 })).toEqual(['compactor behind: raw at risk']);
    expect(troubles({ ...ok, compact_lag_hours: 3, compact_age_ms: 7_200_000 })).toEqual([]);
    expect(troubles({ ...ok, compact_lag_hours: null, compact_age_ms: null, compact_started_ms: null })).toEqual([]);
    // no status row 2 h after the first compact_hours row (ok.now_ms is 0)
    expect(troubles({ ...ok, compact_age_ms: null, compact_started_ms: -7_200_001 })).toEqual(['compactor has never reported']);
    expect(troubles({ ...ok, compact_age_ms: null, compact_started_ms: -7_200_000 })).toEqual([]);
  });
```
Append to the `describe('Health', …)` block in `web/src/pages/Health.test.tsx` (and import `NO_ARCHIVE` from `./Health`):
```tsx
  it('shows the archive size, oldest day, compactor lag and last error', async () => {
    mockApi({
      '/api/health/now': {
        health: healthFixture({ archive_bytes: 12_345_678_901, archive_oldest_day: '2026-09-28', compact_lag_hours: 4, compact_last_error: 'read episodes: timeout', compact_age_ms: 120_000 }),
      },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': integrity,
    });
    renderAt('/health');
    expect(await screen.findByText('12.3 GB')).toBeInTheDocument();
    expect(screen.getByText('2026-09-28')).toBeInTheDocument();
    expect(screen.getByText('4 h')).toBeInTheDocument();
    expect(screen.getByText('BEHIND')).toBeInTheDocument();
    expect(screen.getByText('read episodes: timeout')).toBeInTheDocument();
  });

  it('says so when the compactor has not reported yet', async () => {
    mockApi({
      '/api/health/now': { health: healthFixture({ archive_bytes: null, archive_oldest_day: null, compact_lag_hours: null, compact_last_error: null, compact_age_ms: null, compact_started_ms: null }) },
      '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
      '/api/integrity/summary': integrity,
    });
    renderAt('/health');
    expect(await screen.findByText(NO_ARCHIVE)).toBeInTheDocument();
  });
```
Run: `cd web && npx vitest run src/lib/bytes.test.ts src/components/Hearts.test.tsx src/pages/Health.test.tsx; cd ..`
Expected: FAIL (no `bytes.ts`, no new fields, no ARCHIVE box).

- [ ] **Step 4: The web**

`web/src/api/types.ts`, in `HealthNow` after `dropped_groups: number;`:
```ts
  /** Phase 5b archive (compact_status); null until the compactor first reports. */
  archive_bytes: number | null;
  archive_oldest_day: string | null;
  compact_lag_hours: number | null;
  compact_last_error: string | null;
  /** now − the compactor's last report, server clock. */
  compact_age_ms: number | null;
  /** When the first hour was recorded in compact_hours (done or lost). */
  compact_started_ms: number | null;
```
`web/src/test/fixtures.ts` `healthFixture`: add `archive_bytes: 12_400_000_000, archive_oldest_day: '2026-09-28', compact_lag_hours: 1, compact_last_error: null, compact_age_ms: 600_000, compact_started_ms: 1_789_964_000_000,` before `...o`.

`web/src/lib/bytes.ts`:
```ts
/** Bytes as GB (10^9 B, like the budgets) with one decimal, rounded down; integer math only. */
export function fmtGb(bytes: number): string {
  const tenths = Math.floor(bytes / 100_000_000);
  return `${Math.floor(tenths / 10)}.${tenths % 10} GB`;
}
```
`web/src/components/Hearts.tsx` `troubles`, after the `capture paused` line:
```ts
  // spec 5b §5: one heart for the compactor: behind (3 mirrors lag_alarm_hours), silent for 2 h, or
  // never reported although hours are being recorded
  if (h.compact_lag_hours !== null && h.compact_lag_hours > 3) t.push('compactor behind: raw at risk');
  else if (h.compact_age_ms !== null && h.compact_age_ms > 7_200_000) t.push('compactor status older than 2 h');
  else if (h.compact_age_ms === null && h.compact_started_ms !== null && h.now_ms - h.compact_started_ms > 7_200_000) t.push('compactor has never reported');
```
`web/src/help.ts`, after the `capture` entry:
```ts
  archive: {
    title: 'What is the archive?',
    text: 'Raw capture is kept for about a day. Every closed hour is reduced to compact tables (top of book, depth every 5 minutes, full detail around arbitrage episodes, and the stretches where data is missing) and kept for months inside a 55 GB budget, oldest days deleted first. Lag is how many hours the compactor is behind; above 3, raw hours may be deleted before they are reduced, and a heart is lost.',
  },
```
`web/src/pages/Health.tsx`: add `import { fmtGb } from '../lib/bytes';`, export the empty-state copy and the box component:
```tsx
export const NO_ARCHIVE = 'No archive status yet. The compactor has not reported.';

function ArchiveStatus({ h }: { h: HealthNow }) {
  if (h.archive_bytes === null || h.compact_age_ms === null) return <EmptyState text={NO_ARCHIVE} />;
  const behind = h.compact_lag_hours !== null && h.compact_lag_hours > 3;
  return (
    <dl className="kv">
      <dt>archive</dt>
      <dd>{fmtGb(h.archive_bytes)}</dd>
      <dt>oldest day</dt>
      <dd>{h.archive_oldest_day ?? '—'}</dd>
      <dt>compactor lag</dt>
      <dd>
        <span>{h.compact_lag_hours === null ? '—' : `${h.compact_lag_hours} h`}</span>
        {behind ? <> <Tag tone="flag">BEHIND</Tag></> : null}
      </dd>
      <dt>last report</dt>
      <dd>{`${fmtDuration(h.compact_age_ms)} ago`}</dd>
      <dt>last error</dt>
      <dd>{h.compact_last_error ?? '—'}</dd>
    </dl>
  );
}
```
and in `Health()`, right after the `CAPTURE` box:
```tsx
      <Box title="ARCHIVE" help="archive">
        <Loadable state={now} what="archive status">
          {(d) => (d.health === null ? <EmptyState text={NO_HEALTH_ROWS} /> : <ArchiveStatus h={d.health} />)}
        </Loadable>
      </Box>
```

- [ ] **Step 5: Run the web suite**

Run: `cd web && npx vitest run && npm run build && npm run lint; cd ..`
Expected: PASS (every existing test too: the fixtures report a healthy compactor, so Radar still shows ten hearts); the build and lint succeed. At 360 px the ARCHIVE box is a `kv` list like CAPTURE, so it does not scroll sideways.

- [ ] **Step 6: Commit**

```bash
git add bins/api/src/health.rs bins/api/tests/health.rs web/src/api/types.ts web/src/test/fixtures.ts web/src/components/Hearts.tsx web/src/components/Hearts.test.tsx web/src/pages/Health.tsx web/src/pages/Health.test.tsx web/src/help.ts web/src/lib/bytes.ts web/src/lib/bytes.test.ts
git commit -m "api, web: the archive on Health and one heart for the compactor"
```

---

### Task 20: Docker, compose, `.env.example`, roles script

Spec §2, §9. The `compact` service in the same Rust image: `core` profile, 0.25 CPU, 256 MB, read-only root filesystem, the `data` volume read-only at `/data`, the new `archive` volume read-write at `/archive`, only the `internal` network.

**Files:**
- Modify: `docker/rust.Dockerfile`, `compose.yml`, `.env.example`, `docker/postgres/init/01-roles.sh`

- [ ] **Step 1: The image**

`docker/rust.Dockerfile`:
- the header comment's second sentence becomes `One image, four binaries: scanner (entrypoint), api (compose.web.yml), analyze and compact (compose.yml), each selected by its entrypoint.`;
- the cook line: `RUN cargo chef cook --release --recipe-path recipe.json --package scanner --package api --package analyze --package compact`;
- the build line: `RUN cargo build --release --package scanner --package api --package analyze --package compact && mkdir -p /data /archive && chown 65532:65532 /data /archive`;
- after `COPY --from=builder /src/target/release/analyze /usr/local/bin/analyze` add `COPY --from=builder /src/target/release/compact /usr/local/bin/compact`;
- after `COPY --from=builder --chown=65532:65532 /data /data` add `COPY --from=builder --chown=65532:65532 /archive /archive` (a new named volume copies this ownership, so the non-root compactor can write it);
- after `COPY config/policies.toml /etc/cute-trading/policies.toml` add `COPY config/compact.toml /etc/cute-trading/compact.toml`.

The CRLF guard (`grep -rlI … crates/store/migrations config`) already covers `0008_compact.sql` and `compact.toml`.

- [ ] **Step 2: Compose**

`compose.yml`: in `postgres.environment` add `COMPACT_RW_DB_PASSWORD: ${COMPACT_RW_DB_PASSWORD:-}`; after the `analyze` service add:
```yaml
  # Phase 5b tiered storage (spec 2026-09-27-phase5b-tiered-storage-design.md §2): every closed raw
  # hour → the Parquet archive. No network client in the binary (arch test); Postgres only through
  # store as compact_rw (writes compact_hours/compact_status only). Reads the raw capture read-only.
  compact:
    image: cute-trading/rust:dev
    profiles: ["core"]
    entrypoint: ["/usr/local/bin/compact"]
    command: ["run"]
    environment:
      DATABASE_URL: postgres://compact_rw:${COMPACT_RW_DB_PASSWORD:?set COMPACT_RW_DB_PASSWORD in .env}@postgres:5432/cute_trading
    depends_on:
      postgres:
        condition: service_healthy
      migrate:
        condition: service_completed_successfully
    restart: on-failure:5
    stop_grace_period: 30s          # an hour in progress may be killed: its writes are atomic
    read_only: true
    tmpfs: [/tmp]
    security_opt: ["no-new-privileges:true"]
    cap_drop: ["ALL"]
    cpus: "0.25"
    mem_limit: 256m
    volumes:
      - data:/data:ro
      - archive:/archive
    logging: *logging
    networks: [internal]
```
and under `volumes:` add `archive: {}`.

- [ ] **Step 3: The key and fresh volumes**

`.env.example`, after the `ANALYZE_RW_DB_PASSWORD` line:
```
# Phase 5b compactor's role (compact_rw). Migration 0008 creates it NOLOGIN; the runbook sets this password once.
COMPACT_RW_DB_PASSWORD=change-me-compact-rw
```
`docker/postgres/init/01-roles.sh`: in the header comment add `compact_rw (Phase 5b) likewise from COMPACT_RW_DB_PASSWORD (migration 0008 otherwise).`, and at the end:
```sh
if [ -n "${COMPACT_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
       -v compact_rw_pw="$COMPACT_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE compact_rw LOGIN PASSWORD :'compact_rw_pw';
ALTER ROLE compact_rw SET statement_timeout = '10s';
SQL
fi
```

- [ ] **Step 4: Check the files**

```bash
docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
sh -n docker/postgres/init/01-roles.sh && echo roles-sh-ok
grep -c 'compact' docker/rust.Dockerfile
grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
Expected: `config-ok`, `roles-sh-ok`, `5` (the header, cook, build, binary and config lines), `lf-ok`. (No image is built here: Docker builds happen in Task 21's runbook, by the owner.)

- [ ] **Step 5: Commit**

```bash
git add docker/rust.Dockerfile compose.yml .env.example docker/postgres/init/01-roles.sh
git commit -m "docker: compact in the rust image; compose service (0.25 CPU, 256 MB, data ro, archive volume)"
```

---

### Task 21: the deploy runbook `docs/superpowers/runbooks/phase5b-storage-vps.md`

No code. The owner runs it. The owner's runbook format is strict: a flat numbered list; each step is ONE copy-pasteable bash block, then `→ expected:` (the exact output shape), then at most one `if not:` line; **STOP** after every step whose failure must halt; prose at most one line per step; fully self-contained (no "as in 5a §n": every command is written out). It carries the 5d lessons (LF line endings, migrate alone first, the migration-checksum test) and goes after the current 5d redeploy and the other agent's frontend changes (spec §9).

**Files:**
- Create: `docs/superpowers/runbooks/phase5b-storage-vps.md`

- [ ] **Step 1: Write the runbook**

Create `docs/superpowers/runbooks/phase5b-storage-vps.md` with exactly this content (fill only the `<…>` values when running it):

````markdown
# Phase 5b on the VPS: tiered storage (compact service, Parquet archive, delete guard)

Rules: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; PC steps in Git Bash, VPS steps as root over SSH.

## On the PC

1. Go to the repo on the branch being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && git status --short --untracked-files=no && git log -1 --format='%h %s'
```
→ expected: no status lines, then one line `<sha> <subject>` of the commit to deploy.
if not: commit or stash the listed changes, then run this step again.

2. Line endings: migrations and configs baked into the image must be LF.
```bash
git ls-files --eol crates/store/migrations config | grep -v 'w/lf' ; grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
→ expected: only `lf-ok`.
if not: `git checkout -- <each listed file>` (.gitattributes pins LF) and run this step again.
**STOP**

3. Migration checksums: the applied migrations' bytes must match what production recorded.
```bash
cargo test -p store --test migration_checksums 2>&1 | tail -3
```
→ expected: `test result: ok. 2 passed; 0 failed`.
if not: a CRLF or edited migration (fix as in step 2); if the test file is missing, merge `main` (PR #4) into this branch first.
**STOP**

4. The new `.env` key on the PC (compose refuses to parse without it; the value is never printed).
```bash
(grep -q '^COMPACT_RW_DB_PASSWORD=' .env || printf 'COMPACT_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && grep -c -E '^(ANALYZE_RW_DB_PASSWORD|SCANNER_HOST|COMPACT_RW_DB_PASSWORD)=' .env
```
→ expected: `3`.
if not: add the missing key the same way (`openssl rand -hex 24`), never with an editor that echoes it.

5. Disk budget on the PC before building.
```bash
powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: the script's pass line (under 30 GB).
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

6. Build both images (Rust: scanner, api, analyze, compact; web).
```bash
export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web
```
→ expected: the short sha, then both builds end without `ERROR` (a `CRLF found` failure means step 2 was skipped).
if not: read the first `ERROR` line; fix and run this step again.
**STOP**

7. The compactor and its config are in the image.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/compact cute-trading/rust:dev --help | head -1
```
→ expected: `raw capture hours → the Parquet archive (paper trading only)`.
if not: step 6 built from the wrong tree; rebuild.

8. (Benchmark, on the VPS) Copy one closed hour from yesterday and the universe records its control lines name (`D` yesterday's date, `H=13`).
```bash
D=<YYYY-MM-DD>; H=13; C=/var/lib/docker/volumes/cute-trading_data/_data/capture; command -v zstdcat >/dev/null || apt-get install -y zstd; mkdir -p ~/bench/$D && cp $C/$D/$H.jsonl.zst ~/bench/$D/ && for f in $(zstdcat $C/$D/$H.jsonl.zst | grep '"kind":"universe"' | grep -o 'universe-[0-9]*-[0-9]*\.json\.zst' | sort -u); do t=${f#universe-}; t=${t%%-*}; d=$(date -u -d @$((t / 1000)) +%F); mkdir -p ~/bench/$d && cp $C/$d/$f ~/bench/$d/; done; tar -cf ~/bench.tar -C ~ bench && rm -rf ~/bench && du -h ~/bench.tar
```
→ expected: one line `<size>  /root/bench.tar` (a few hundred MB).
if not: skip steps 8–11; the spec §3 defaults stay (write "no benchmark" in Results).

9. (Benchmark, on the PC) Fetch and unpack it, then delete it on the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123:/root/bench.tar "$HOME/bench.tar" && rm -rf "$HOME/bench" && tar -xf "$HOME/bench.tar" -C "$HOME" && rm "$HOME/bench.tar" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 rm /root/bench.tar && ls "$HOME/bench"
```
→ expected: one or two `YYYY-MM-DD` folder names.
if not: check the scp line's key path (quoted, forward slashes).

10. (Benchmark, on the PC) The release compactor on that hour under the service's limits.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --cpus 0.25 --memory 256m --tmpfs /tmp -v "$HOME/bench:/bench:ro" --entrypoint /usr/local/bin/compact cute-trading/rust:dev bench /bench/<YYYY-MM-DD>/13.jsonl.zst
```
→ expected: one line `bench: <day>/13.jsonl.zst <ms> ms, lines=… tob=… depth=… gaps=… tokens=…, bytes tob=… depth=… gaps=… tokens=…, per_day_bytes=<n> …, peak_rss_kb=<n>` with `<ms>` < 600000 and `peak_rss_kb` < 200000; write the line in Results.
if not: over time or memory: **STOP**, do not deploy, report the line; `per_day_bytes` + 50 MB over 300 MB with depth over 50 MB a day: set `depth_every_secs = 600` in `config/compact.toml`, commit, and redo steps 6–10.

11. (Benchmark, on the PC) Both agreement tests on the real hour (the compactor's books and tob rows equal the scanner engine's).
```bash
TAPE_AGREEMENT_DIR="$HOME/bench" cargo test -p scanner --test tape_agreement --test tob_agreement -- --ignored --nocapture 2>&1 | grep -E 'tape agreement|tob agreement|test result'
```
→ expected: `tape agreement: <n> lines, books equal after every one`, `tob agreement: <n> rows equal the engine's top`, and two `test result: ok. 1 passed`.
if not: **STOP**, do not deploy; the failing line and token go to the plan owner.

12. Bundle the images, compose files, `.env.example`, roles script and disk-budget script.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-5b/docker/postgres/init" "$HOME/ct-5b/scripts" && cd "$HOME/ct-5b" && docker save cute-trading/rust:dev cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/ && cp "$R/scripts/disk-budget.sh" scripts/ && tar -cf "$HOME/ct-5b.tar" image.tar.gz compose.yml compose.web.yml .env.example docker scripts && cd "$HOME" && rm -rf "$HOME/ct-5b" && ls -l "$HOME/ct-5b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-5b.tar` (about 1 GB).
if not: free disk (step 5) and run this step again.

13. Ship the bundle to the VPS.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-5b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-5b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS

14. Disk headroom: with the project at its 75 GB ceiling the disk must keep ≥ 20 GB free (capture pauses below 10 GB free).
```bash
cd /home/isoca/projects/cute-trading && df -BG --output=size,used,avail /var/lib/docker | tail -1 && du -s --block-size=1G /var/lib/docker/volumes/cute-trading_* | awk '{s += $1} END {print "project_gb", s}'
```
→ expected: `<size>G <used>G <avail>G` then `project_gb <p>`; with `other = used − p`, `size − other − 75 ≥ 20`.
if not: set `archive_budget_gb = size − other − 36` (rounded down, ≥ 1) in `config/compact.toml` on the PC, commit, redo steps 6–13.
**STOP**

15. Keep the running images under a rollback tag before loading the new ones, and back up `.env`.
```bash
docker tag "$(docker inspect -f '{{.Image}}' "$(docker compose --profile core ps -q scanner)")" cute-trading/rust:pre5b && docker tag "$(docker inspect -f '{{.Image}}' "$(docker compose -f compose.yml -f compose.web.yml --profile web ps -q web)")" cute-trading/web:pre5b && cp compose.yml /root/compose.yml.pre5b && cp compose.web.yml /root/compose.web.yml.pre5b && cp .env .env.bak-5b && chmod 600 .env.bak-5b && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}'
```
→ expected: `rust:pre5b` on the same ID as `rust:dev`, `web:pre5b` on the same ID as `web:dev`.
if not: a service is not running; tag from `docker image ls` by the ID it last ran, then continue.

16. Back up the database.
```bash
docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-5b.dump && ls -lh /root/ct-pre-5b.dump
```
→ expected: one `ls` line, a few hundred MB.
if not: free space and run this step again; do not migrate without it.
**STOP**

17. Load the bundle.
```bash
tar -xf /root/ct-5b.tar && rm /root/ct-5b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}'
```
→ expected: `Loaded image: cute-trading/rust:dev` and `…/web:dev`; `:dev` now on new IDs, `:pre5b` on the old ones.
if not: the tar was truncated; re-ship (step 13).

18. Compose parses with the new service and keys.
```bash
docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c -E '^  compact:$|COMPACT_RW_DB_PASSWORD' compose.yml
```
→ expected: `config-ok` then `3`.
if not: the bundle's compose files are old; re-bundle (step 12).
**STOP**

19. The new `.env` key on the VPS (never printed).
```bash
grep -q '^COMPACT_RW_DB_PASSWORD=' .env || printf 'COMPACT_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env; grep -c '^COMPACT_RW_DB_PASSWORD=' .env
```
→ expected: `1`.
if not: remove duplicate lines by hand without printing values (`sed -i` on the key name), then run again.

20. Disk budget of the project before migrating.
```bash
LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: the script's pass line.
if not: free space (old images: `docker image prune -f`) and run again.

21. Stop the scanner and migrate ALONE.
```bash
docker compose --profile core stop --timeout 60 scanner && docker compose --profile core run --rm migrate; echo "migrate exit=$?"
```
→ expected: `migrate exit=0`.
if not: `docker compose --profile core logs --tail 50 migrate`; do not start anything; a checksum error means a CRLF image: redo steps 2–17.
**STOP**

22. Migration 8 is applied and the grants are right.
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('compact_rw','compact_hours','UPDATE'), has_table_privilege('compact_rw','episodes','INSERT'), has_table_privilege('scanner_rw','compact_hours','INSERT'), has_table_privilege('scanner_rw','compact_hours','UPDATE'), has_table_privilege('analyze_ro','compact_status','SELECT'), has_table_privilege('scanner_rw','compact_status','SELECT')"
```
→ expected: `8` then `t|f|t|f|t|t`.
if not: do not continue; roll back (step 37).
**STOP**

23. Activate `compact_rw` with the `.env` password (never printed).
```bash
PW="$(grep -E '^COMPACT_RW_DB_PASSWORD=' .env | cut -d= -f2-)"; case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad"; PW= ;; *) echo pw-read ;; esac; [ -n "$PW" ] && printf '\\set pw %s\nSET log_min_error_statement = panic;\nALTER ROLE compact_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set; unset PW; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'compact_rw'"
```
→ expected: `pw-read`, `role-set`, `t`.
if not: `pw-bad` means the `.env` line isn't an `openssl rand -hex 24` value: replace it that way (step 19) and run again.
**STOP**

24. Start the scanner (new retention, delete guard, universe dedup) and note the deploy time.
```bash
date +%s%3N > /root/5b_deploy_ms && docker compose --profile core up -d --no-build --no-deps scanner && sleep 60 && docker compose --profile core logs --since 70s scanner 2>&1 | grep -E 'capture:|error|panicked' | head
```
→ expected: `capture: compactor silent (no compact_status within 6 h); retention falls back to age + cap` and no `error`/`panicked` lines.
if not: roll back the scanner (step 37).

25. Start the compactor.
```bash
docker compose --profile core up -d --no-build --no-deps compact && sleep 90 && docker compose --profile core logs --since 100s compact 2>&1 | head -5
```
→ expected: `compact: every 60 s, /data/capture → /archive (writes compact_hours/compact_status only, paper only)`, then `compact: <YYYY-MM-DDTHH> done in <ms> ms, tob=… depth=… l2=… gaps=…, <n> B` lines as it catches up.
if not: `refusing to start` = step 23 failed; `database unreachable` = the password; fix, then `docker compose --profile core restart compact`.

26. Restart api and web (health fields, Archive box).
```bash
docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && docker compose -f compose.yml -f compose.web.yml --profile web ps api web --format '{{.Service}} {{.Status}}'
```
→ expected: `api Up …` and `web Up …`.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api web`.

27. The scanner now waits for the compactor.
```bash
sleep 60; docker compose --profile core logs --since 3m scanner 2>&1 | grep 'capture: compactor' | tail -1
```
→ expected: `capture: compactor reporting; raw hours wait for compaction`.
if not: check `compact_status` (step 29); the scanner keeps today's rule meanwhile, nothing is lost.

28. First compaction check (run about 2 h after step 25): hours become `done` 1–2 h after they close.
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(hour_ms/1000) AT TIME ZONE 'UTC','MM-DD HH24'), status, rows FROM compact_hours ORDER BY hour_ms DESC LIMIT 3"
```
→ expected: three `MM-DD HH|done|{"tob": …}` lines, the newest 1–2 hours before now (UTC).
if not: `docker compose --profile core logs --tail 30 compact`; do not leave it running with errors: roll back compact (step 36).
**STOP**

29. Status row, lag and errors.
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(updated_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), archive_bytes, oldest_day, lag_hours, last_error FROM compact_status"
```
→ expected: `HH:MI|<bytes>|<YYYY-MM-DD>|1|` or `…|2|` (empty last_error), updated within the last minute.
if not: the last_error text says what failed; lag above 3 costs a heart on Radar.

30. Container limits hold while it works.
```bash
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' | grep compact
```
→ expected: `cute-trading-compact-1 <≤ 25%> <under 256MiB> / 256MiB`; write it in Results.
if not: memory near the limit: lower `depth_levels` or the row groups per the plan; report.

31. No hour lost at the cap since the deploy.
```bash
docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM compact_hours WHERE status = 'lost' AND done_ms > $(cat /root/5b_deploy_ms)"; docker compose --profile core logs scanner 2>&1 | grep -c 'deleted before compaction'
```
→ expected: `0` then `0`.
if not: the compactor is too slow for the cap: report the lag and `docker stats` (step 30).

32. Raw capture holds about a day (run 24 h after step 24).
```bash
du -sh /var/lib/docker/volumes/cute-trading_data/_data/capture && ls /var/lib/docker/volumes/cute-trading_data/_data/capture/$(date -u +%F) | grep -c universe-
```
→ expected: at most about 10G (never over 15G), then a count far below 12 per hour elapsed today.
if not: over 15G means the cap isn't enforced: report the scanner log's `capture:` lines.

33. Verify the first full archived day (run after `compact: merged <day>` appears in the compact log).
```bash
DAY=<YYYY-MM-DD>; docker compose --profile core run --rm --no-deps compact verify $DAY; echo "verify exit=$?"; python3 -c "import json,sys; m=json.load(open(sys.argv[1])); print('tob', m['totals']['tob'], 'ts_clamped', m['ts_clamped'], 'late_rows', m['late_rows'], 'bytes', m['bytes'])" /var/lib/docker/volumes/cute-trading_archive/_data/$DAY/manifest.json
```
→ expected: `verify <day>: ok`, `verify exit=0`, then one line with `ts_clamped` and `late_rows` small next to `tob`, and the bytes per table; write them in Results.
if not: **STOP** compaction (step 36) and report every listed problem.

34. (On the PC, after step 22 succeeded) Pin 0008's production checksum in the checksum test.
```bash
ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc \"SELECT encode(checksum, 'hex') FROM _sqlx_migrations WHERE version = 8\""
```
→ expected: one 96-character hex line; add `(8, "<that hex>"),` after the version-7 entry of `APPLIED` in `crates/store/tests/migration_checksums.rs`, run `cargo test -p store --test migration_checksums` (→ `2 passed`), commit `store: pin migration 0008's checksum`.
if not: the test file is not on the branch yet: add the line once PR #4 is merged.

35. Query the archive on the PC with DuckDB (copy a day first; prices and sizes are micro-units).
```bash
DAY=<YYYY-MM-DD>; mkdir -p "$HOME/archive" && scp -r -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123:/var/lib/docker/volumes/cute-trading_archive/_data/$DAY "$HOME/archive/" && cd "$HOME" && duckdb <<'SQL'
-- 1. one token's top of book (every copied day)
WITH tok AS (SELECT regexp_extract(filename, '(\d{4}-\d{2}-\d{2})', 1) AS day, token_idx, token_id FROM read_parquet('archive/*/tokens.parquet', filename = true)),
     tob AS (SELECT regexp_extract(filename, '(\d{4}-\d{2}-\d{2})', 1) AS day, * FROM read_parquet('archive/*/tob.parquet', filename = true))
SELECT to_timestamp(tob.ts_ms / 1000) AS t, tob.bid / 1e6 AS bid, tob.bid_sz / 1e6 AS bid_sz, tob.ask / 1e6 AS ask, tob.ask_sz / 1e6 AS ask_sz
FROM tob JOIN tok USING (day, token_idx) WHERE tok.token_id = '<TOKEN_ID>' ORDER BY tob.day, tob.recv_ms LIMIT 20;
-- 2. the l2 book at an episode's peak (<PEAK_MS> = episodes.peak_server_ms)
WITH l AS (SELECT * FROM read_parquet('archive/<YYYY-MM-DD>/l2.parquet', file_row_number = true) WHERE episode_id = '<EPISODE_ID>' AND ts_ms <= <PEAK_MS>),
     s AS (SELECT token_idx, max(recv_ms) AS snap_recv FROM l WHERE snapshot GROUP BY token_idx),
     b AS (SELECT l.token_idx, min(l.file_row_number) AS from_row FROM l JOIN s USING (token_idx) WHERE l.snapshot AND l.recv_ms = s.snap_recv GROUP BY l.token_idx),
     book AS (SELECT l.token_idx, l.side, l.price, arg_max(l.new_size, l.file_row_number) AS size FROM l JOIN b USING (token_idx) WHERE l.file_row_number >= b.from_row GROUP BY ALL)
SELECT t.token_id, CASE book.side WHEN 0 THEN 'bid' ELSE 'ask' END AS side, book.price / 1e6 AS price, book.size / 1e6 AS size
FROM book JOIN read_parquet('archive/<YYYY-MM-DD>/tokens.parquet') t USING (token_idx) WHERE book.size > 0 ORDER BY t.token_id, side, price DESC;
-- 3. the day's gaps ('all' = the whole feed)
SELECT to_timestamp(start_ms / 1000) AS start, (end_ms - start_ms) / 1000 AS secs, coalesce(CAST(shard AS VARCHAR), 'all') AS shard, reason
FROM read_parquet('archive/<YYYY-MM-DD>/gaps.parquet') ORDER BY start_ms;
SQL
```
→ expected: three result tables (20 top-of-book rows, the book's levels, the gaps with their reasons).
if not: `duckdb` missing: install the DuckDB CLI on the PC (a standalone binary, not a crate).

## Rollback (only if a step above says so)

36. Stop the compactor (the scanner returns to today's rule by itself once `compact_status` is 6 h old).
```bash
docker compose --profile core stop compact && docker compose exec -T postgres psql -U ct_owner -d cute_trading -c "ALTER ROLE compact_rw NOLOGIN"
```
→ expected: `Stopped`, then `ALTER ROLE`; within 6 h the scanner logs `capture: compactor silent …; retention falls back to age + cap`.
if not: `docker compose --profile core ps compact` must show it exited; run the stop again.

37. Full rollback: the previous scanner, api and web images (0008 stays: additive, unused by the old code).
```bash
docker compose --profile core stop compact scanner && docker tag cute-trading/rust:dev cute-trading/rust:5b && docker tag cute-trading/web:dev cute-trading/web:5b && docker tag cute-trading/rust:pre5b cute-trading/rust:dev && docker tag cute-trading/web:pre5b cute-trading/web:dev && docker compose --profile core up -d --no-build --no-deps scanner && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api web && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT version FROM runs WHERE service = 'scanner' ORDER BY run_id DESC LIMIT 1"
```
→ expected: the newest scanner run on the pre-5b version (sha).
if not: the database is unusable (should not happen, 0008 is additive): ask the owner before `pg_restore` of `/root/ct-pre-5b.dump`.

## Results

- `GIT_SHA`: · parquet/arrow version: · checksum test (step 3):
- Benchmark line (step 10): · agreement counts (step 11): · decision:
- Disk headroom (step 14) `size` / `other` / budget change:
- migrate exit / version / grants (steps 21–22): · role (step 23):
- First `done` hour and its lag (step 28): · status (step 29): · docker stats (step 30):
- Lost since deploy (step 31): · raw size at +24 h (step 32):
- First merged day, `verify`, bytes per table, `ts_clamped`, `late_rows` (step 33):
- 0008 checksum pinned (step 34): · date:
````

- [ ] **Step 2: Check the format**

```bash
f=docs/superpowers/runbooks/phase5b-storage-vps.md; grep -c '^→ expected:' $f; grep -c '^```bash' $f; grep -n -i -E '5a runbook|5d runbook|as in 5a|as in 5d' $f || echo self-contained
```
Expected: `37`, `37`, `self-contained` (one bash block and one expected line per step, no references to other runbooks).

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/runbooks/phase5b-storage-vps.md
git commit -m "runbook: Phase 5b on the VPS (migrate 0008 alone, compact_rw, compact service, verify, DuckDB)"
```

---

## Rulings (spec §11 open items and ambiguities)

- **R1, reuse (spec §11 "Reuse"). Owner signed off 2026-09-27.** A new pure crate `crates/tape` holds everything both binaries read the raw capture with: `Line`/`Ctl`/`book_ctl`/`ctl_book` and the time/path helpers (moved from `bins/scanner/src/capture.rs`, re-exported there), `MarketEvent`/`parse_frame` (moved from `crates/pm-api/src/ws.rs`, re-exported and wrapped there, because the arch test bans `pm-api` from `compact`), the capture layout and line reader (replay now reads through it and exposes `apply_line`), a read-only projection of the universe record, and `FeedBooks`. The engine's book-keeping can't be lifted out as code: its books live inside `arb_core::detect::Group` (a premise file) and are interleaved with detection, integrity rows and the paper trader. So `FeedBooks` re-expresses only the book rules on the same `Book`, the same parser and the same line type; the one expression both must share (`server_top`) is called by both; `bins/scanner/tests/tape_agreement.rs` asserts identical books after every line of a capture that exercises every rule (including `Suspect`/`Resolved` lines, a token moving between groups, a group and a canary pair appearing mid-hour), and `bins/scanner/tests/tob_agreement.rs` asserts every `tob` row the compactor emits equals the engine's top at that line; both also run `#[ignore]` on a real hour in the runbook §3. `scanner::run::replay_cmd` itself is not shared: it writes episodes and paper rows, which the compactor never needs.
- **R2, Parquet row groups (spec §11 "Parquet writer").** 262,144-row row groups, 65,536-row write batches, zstd 3, dictionary encoding on by default. An open part holds one encoded row group plus one batch; five parts per hour stay well inside 256 MB. Pinned by `row_groups_hold_262144_rows_so_memory_stays_bounded`; measured on a real hour by the runbook §3 bench (`peak_rss_kb` under 200,000 or no deploy).
- **R3, the benchmark (spec §11 "Benchmark").** A `compact bench <hour file>` subcommand plus an `#[ignore]` test. The timing that decides is the release build inside the image with `--cpus 0.25 --memory 256m` (runbook §3), because the PC runs debug builds only; the debug `#[ignore]` runs check correctness on the real hour (including engine agreement). The owner copies the hour; without it the spec §3 defaults stay. The decision table is in runbook §3 and §11.
- **R4, timestamps (spec §11 "Timestamps").** Confirmed: the capture file is in engine processing order, and frames from different shards interleave, so `recv_ms` is not monotone in the file. `tob` and `l2` rows are written in `recv_ms` order through a 10 s reorder buffer (ties keep file order); a row older than one already written is appended and counted (`late_rows`), never dropped. Both timestamps are kept: `ts_ms` is the book's server timestamp, raised to the token's previous row's `ts_ms` if it would go back (counted as `ts_clamped`), so `verify`'s "ts_ms non-decreasing per token" holds without inventing data. Both counts are totalled per day in `manifest.json` (`ts_clamped`, `late_rows`) and read in runbook §11.
- **R5, l2 snapshots.** Each hour restarts every active window with snapshot rows (so each hourly part, and each day across midnight, stands alone for `verify` and for a reader), as does a hand-over between overlapping windows. A token whose book is untrusted or empty when its window starts gets its snapshot at its first trusted, non-empty state.
- **R6, gaps and `verify`.** `shard = null` gaps are whole-feed stretches (cold start before the first line, `scanner_stopped` from a `Shutdown` to the next line or a missing hour with no record, `raw_hour_lost` for a lost hour or from an unreadable point to the hour's end) and are checked against every row. `shard = n` gaps (`down`, `no_book_yet`) cover tokens of one shard waiting for a snapshot; rows carry no shard, so `verify` can't check them; the reducer tests pin that untrusted books make no rows. Every gap row is clipped to its hour.
- **R7, `compact_rw` also gets SELECT on `compact_hours`/`compact_status`** (spec §9 lists INSERT and UPDATE): `INSERT … ON CONFLICT DO UPDATE` needs it, and the compactor reads which hours are lost.
- **R8, which universe records to keep (spec §4.5).** "The newest universe file referenced by any remaining hour" can't be found without decompressing hours every 30 s. Every control line names the record in force when it was captured, which is one written at or after the oldest remaining hour's start, or the newest one written before it; so retention keeps exactly those, by file write time (mtime). `taken_ms` can't be used: a warm start writes a record whose `taken_ms` is older than records written before it.
- **R9, a missing raw hour.** A file that was listed and is gone was deleted by the scanner: `lost` at once (`raw_hour_lost`), whether or not its mark has landed (a later mark is a no-op against the compactor's row, so the loss stays visible). Never listed: `lost` if `compact_hours` says so; otherwise, after 5 minutes of being ready (`MISSING_GRACE_MS`), `scanner_stopped` with status `done` and no source file. The scanner inserts every lost mark the moment it deletes (its 30 s tick only retries). An unreadable file (truncated zstd) keeps the rows before the break, gaps the rest as `raw_hour_lost`, and is `done` with `read_error` in its sidecar, so compaction never stalls on one bad file.
- **R10, rollback (owner-approved).** The scanner waits for compaction only while the compactor is alive: `compact_status` exists and is at most 6 h old (`COMPACTOR_STALE_MS`). No status row (compact not deployed yet) or an older one clears the guard's view, and retention is today's rule (age plus cap) with the new numbers; the same happens while the database is unreachable (view older than 2 minutes). So "stop compact" is the rollback, and before compact's first report (deploy order: scanner first) the scanner behaves as today.
- **R11, `compact_status` is written every poll (60 s)**, not only after an hour or a merge, so "older than 2 h" on the dashboard means the service is gone, not merely idle between hours.
- **R12, what a `tob` row is.** A change of the best bid or best ask price *or size* (the table stores both), including a token's first trusted state of the day; the last top is kept across gaps and days, so a book resynchronized to the same top makes no row.
- **R13, readiness.** Hour H is compacted once any raw hour file at or after H + 2 h exists (H + 1 closed), which also moves on after the scanner skipped hours.
- **R14, the `tokens` table** lists the tokens that got at least one row that day, with the metadata of the universe in force when they first did; `token_idx` restarts every UTC day.
- **R15, checkpoints** keep the spec's name `checkpoint-<hour>.bin.zst`; the content is zstd-compressed JSON (the repo's existing serde_json, no new crate).
- **R16, universe records are named only once written.** The capture thread acknowledges every universe record it wrote (`Metrics::universe_acked_seq`); a refresh reuses the last record only if it was acknowledged, capture is not paused, and no capture message was dropped since; otherwise it writes a new one. A record lost on its way to disk (capture paused, a write error) is therefore never named by later lines, and replay can't break on it.
- **R17, one more heart case.** Besides "behind" and "older than 2 h", the compactor heart is lost when no `compact_status` row exists 2 h after the first `compact_hours` row (hours are being recorded, e.g. lost at the cap, with no compactor reporting).

## Results

Filled in by the implementer:
- Task 8: pinned `parquet` / `arrow` version (60.0.0 at planning time): ______ (and any `set_max_row_group_size` replacement used: ______)
- Task 15: resume test compared byte for byte / row for row: ______
- Task 21 (runbook) results live in `docs/superpowers/runbooks/phase5b-storage-vps.md` Results.
