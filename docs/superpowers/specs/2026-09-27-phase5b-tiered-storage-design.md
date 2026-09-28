# isoca-radar Phase 5b: tiered storage on the VPS (raw for a day, reduced history for months)

**Date:** 2026-09-27. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§7 raw capture, §9.3 replay, §10 storage, §19). **Builds on:** Phase 5a (`2026-09-26-phase5a-cards-design.md`) and 5d (`2026-09-27-phase5d-sweet-spots-design.md`). **Status:** approved in conversation, section by section (2026-09-27): approach 1 of 3, the architecture, the tables, reliability, and reading/testing/deploy.

## 1. Purpose, scope and success

**Keep the data.** Today the VPS keeps raw capture for at most 48 h under a **10 GB hard cap** (`config/scanner.toml:46-47`). Measured on 2026-09-27, the feed runs **7.1M messages an hour, 4.4 GB an hour uncompressed**, so the cap really holds a bit under a day and everything older is deleted. There's no history for backtests. 5b keeps it, **on the VPS, inside a 75 GB ceiling for this project**, with no external storage until R2 (after 2026-10-05, with 5c).

**Approach (1 of 3, chosen):**
- **Tier 1, raw:** about **24 h** of raw capture, as today but with a larger cap.
- **Tier 2, reduced:** a new **compactor** turns every closed raw hour into compact **Parquet** tables that are kept for months.
- **Tier 3, cold:** R2 comes later and isn't built here.

**Rejected:**
- **Postgres tables for the reduced data:** about 50–100M rows a day would grow the operational database the scanner writes to, and run out of budget within weeks.
- **Raw only, shipped to R2:** nothing survives beyond a day until 05/10.

**In scope:**
- A `compact` binary and service.
- The Parquet tables `tokens`, `tob`, `depth`, `l2` and `gaps`, each day with a manifest.
- Progress tracking in Postgres.
- Scanner changes: retention, the delete guard, and universe snapshots written only when changed.
- The archive budget.
- Health fields and a heart.
- `compact verify`.
- A runbook with a "how to query" section.

**Out of scope:** R2 and any external storage; 5c and the Saguillo dataset (the lab reads it from Hugging Face directly; parent §10); a query service or UI for the archive; strategy backtests; changes to how the scanner detects or trades.

**Exit:**
- On the VPS, every raw hour is compacted within about 2 h of closing.
- The raw capture folder holds about a day.
- The archive grows about 250–300 MB a day (the §9 benchmark sets the final figure).
- `compact verify` passes on the first full day.
- The dashboard shows the archive size, its oldest day, and the compactor's lag.
- The project stays under 75 GB.

## 2. Architecture

```
scanner ──writes──► data volume: capture/YYYY-MM-DD/HH.jsonl.zst + universe-*.json.zst
                         │  (raw, ~24 h; the scanner deletes an hour only once it is compacted)
                         ▼
compact (new) ──reads raw (read-only mount)──► archive volume: YYYY-MM-DD/{tokens,tob,depth,l2,gaps}.parquet
     │                                                        + manifest.json, _state/ checkpoints
     └── Postgres (compact_rw): episodes (read) → which windows get l2; compact_hours, compact_status (write)
scanner ──reads compact_hours──► may delete raw hour H only when H is done (or marks it lost at the hard cap)
api ──reads compact_status──► /api/health/now gains archive fields → web Health + one heart
```

**The `compact` service:**
- **Binary:** a new binary, `bins/compact`, in the same Rust image as scanner, api and analyze.
- **Compose:** `core` profile, `cpus: "0.25"`, `mem_limit: 256m`, read-only root filesystem, the `data` volume mounted **read-only** at `/data`, and the new `archive` volume read-write at `/archive`.
- **No network client** (the arch test bans `reqwest`, `pm-api` and `tokio-tungstenite` from `compact`, as it does for `analyze`). It reaches Postgres only through `store`.

**Reuse, not a second implementation.** Book reconstruction reuses the scanner's replay path:
- the same line parsing (`capture::Line`, `Ctl`);
- the same Polymarket frame parsing (`pm_api::ws`);
- the same `arb_core::book::Book`.

If replay code needs to move into a library so both binaries can use it, that move is part of this phase. The archive therefore can't disagree with what the scanner saw (§8 proves it).

**New crates, owner-approved 2026-09-27:** `parquet` and `arrow` (Apache Arrow's Rust crates), used only by `compact` and pinned in `[workspace.dependencies]`. They're added to `deny.toml` bans as "compact only" if the existing wrapper mechanism allows it.

## 3. The archive: tables and files

**Layout:** `archive/YYYY-MM-DD/`, days in UTC.
- **During a day:** hourly parts `HH.<table>.parquet`.
- **After the daily merge (§4.4):** `tokens.parquet`, `tob.parquet`, `depth.parquet`, `l2.parquet`, `gaps.parquet` and `manifest.json`.

**Encoding rules:**
- **Integers only:** prices and sizes are stored as micro-units (`i64`, value × 10⁶), never as floats, in line with the repo's no-floats rule.
- **Time:** `ts_ms` is `i64` milliseconds, taken from the frame's server timestamp. `recv_ms` is kept where noted.
- **Compression:** zstd, with dictionary encoding for `token_idx`.

| Table | One row = | Columns |
|---|---|---|
| `tokens` | one token seen that day | `token_idx u32, token_id str, market_id str, event_id str, outcome str ("yes"/"no"), neg_risk bool` |
| `tob` | a change of best bid or best ask | `ts_ms i64, recv_ms i64, token_idx u32, bid i64?, bid_sz i64?, ask i64?, ask_sz i64?` (null when that side is empty) |
| `depth` | one level of a periodic snapshot | `ts_ms i64, token_idx u32, side u8 (0 bid, 1 ask), level u16 (0 = best), price i64, size i64` |
| `l2` | an exact level change inside an episode window | `ts_ms i64, recv_ms i64, token_idx u32, side u8, price i64, new_size i64, snapshot bool, episode_id str` |
| `gaps` | a stretch with missing or untrustworthy data | `start_ms i64, end_ms i64, shard i32?, reason str ("down", "raw_hour_lost", "no_book_yet", "scanner_stopped")` |

**Sizing and frequencies (config, `config/compact.toml`):**
- **`depth`:** the top **10** levels per side, every **300 s**, only for tokens whose book changed since their last snapshot.
- **`l2` windows:**
  - They cover every **net-positive arbitrage** episode (`kind IN ('yes_basket','no_basket')`, `peak_pnl_net_usd > 0`, live runs), from **300 s before it opens to 300 s after it closes**, capped at **1800 s** in total and flagged as truncated in the manifest.
  - Each window starts with a `snapshot = true` row per level of each token in the episode's group, followed by the exact changes.
  - Windows that overlap are merged, so a row belongs to its first episode.
- **Target:** about 250–300 MB a day in total (estimate: `tob` about 200 MB, `depth` about 50 MB, `l2` about 20–50 MB). The §9 benchmark on a real hour sets the final defaults before deploy.

**`manifest.json` per day:**
- `compact_version`;
- the config values used;
- for each source hour: file name, byte size, sha256, status (`done`/`lost`) and row counts per table;
- the truncated windows;
- totals.

A day can be verified, or rebuilt while its raw hours still exist.

## 4. Processing and reliability

### 4.1 Order and carried state
- **Order:** hours are processed strictly in order, and the compactor keeps every token's `Book` in memory (about 4k tokens, estimated 50–100 MB, inside 256 MB).
- **Checkpoints:** after each hour, all books, the token dictionary, the day's `token_idx` map, and each token's `l2` state (its latest `l2` `ts_ms`, so `ts_ms` never goes back across hours) are written to `archive/_state/checkpoint-<hour>.bin.zst`, via a temporary name and an atomic rename. Two checkpoints are kept. Open `l2` windows are not carried: each hour re-reads its windows from the episodes and restarts every window's streams with snapshot rows (ruling R5), which gives the same rows.
- **Restart:** the compactor resumes from the newest checkpoint.
- **Discontinuity:** a missing hour, a `Down` control line, or a stretch where the scanner was stopped (no hour file) marks the affected tokens' books **unknown**. `tob`, `depth` and `l2` rows for them resume only after the next full book snapshot in the feed, and the stretch is written to `gaps`. No row is ever invented from a stale book.

### 4.2 When an hour is ready
- **The rule:** hour H is processed once hour H+1's file is closed, because `l2` windows extend past H's end, which gives a lag of about 1–2 h.
- **Episodes:** the compactor reads the episodes relevant to H's windows from Postgres at that point. An episode still open at H+1's close has its window capped by the 1800 s rule, so it never waits forever.

### 4.3 Progress, atomic writes, and the scanner's delete guard
- **Migration 0008** adds `compact_hours(hour_ms bigint primary key, status text check in ('done','lost'), source_file text, source_bytes bigint, source_sha256 text, rows jsonb, done_ms bigint)` and a single-row `compact_status(id smallint primary key check (id = 1), updated_ms bigint, archive_bytes bigint, oldest_day date, lag_hours integer, last_error text)`.
- **Atomic writes:** each hour's parts are written under temporary names and renamed when complete. Only then does the `compact_hours` row become `done`. Re-running an hour overwrites its parts, so re-runs are idempotent.
- **The scanner's retention changes:**
  - It deletes a raw hour file older than `retention_hours` (**24**) only if `compact_hours` has it as `done`, or as `lost`.
  - When the capture folder exceeds `max_capture_gb` (**15**), the scanner may delete the oldest hour **whatever its status**, and then inserts it as `lost` (`ON CONFLICT DO NOTHING`) and logs `capture: hour <H> deleted before compaction (disk cap)`.
  - The compactor writes a `gaps` row with reason `raw_hour_lost` for every `lost` hour it passes over.
  - If the database is unreachable, the scanner falls back to today's rule (age plus cap) and logs it: capture must never stall on the database.

### 4.4 Daily merge and the budget
- **Daily merge:** when all 24 hours of a UTC day are `done` or `lost`, the compactor merges the hourly parts into the daily files, writes `manifest.json`, checks the row counts, then deletes the hourly parts.
- **Budget:** after each merge, if `archive_bytes` exceeds **55 GB** (config), whole days are deleted oldest first until it's under, each deletion logged, and `oldest_day` is updated. 55 GB plus about 15 GB of raw capture plus the database (about 0.6 GB) stays inside the 75 GB ceiling.

### 4.5 Universe snapshots written only when they change (scanner)
- **The change:** the scanner writes a universe record (`universe-<taken_ms>-<seq>.json.zst`, 9.3 MB each) only when its content hash differs from the last written one. Today it writes one at every refresh (about 2.7 GB a day).
- **Refreshes that change nothing** still emit the `Ctl::Universe` capture line, pointing at the last written file, so replay is unchanged.
- **Retention** keeps the newest universe file referenced by any remaining hour, even if that file is older than the oldest hour. Today it deletes universe files older than the oldest hour, which would drop a still-current file.
- **Tests:** an unchanged refresh writes no file but a correct control line, and retention never deletes a referenced file.

## 5. Visibility

**Health:** `compact` updates `compact_status` after each hour and each merge. `/api/health/now` adds `archive_bytes`, `archive_oldest_day`, `compact_lag_hours` and `compact_last_error`. The Health page shows them.

**Radar:** loses one heart when `compact_lag_hours > 3` ("compactor behind: raw at risk") or when `compact_status` is more than 2 h old.

**Logs:** one line per hour: `compact: <H> done in <ms> ms, tob=<n> depth=<n> l2=<n> gaps=<n>, <bytes> B`. One per merge and per budget deletion.

## 6. Reading the archive (no new service)

- **`compact verify <YYYY-MM-DD>`** reads a day, and exits non-zero on any mismatch. It checks:
  - the manifest's row counts;
  - that each `tob` row really is a change (never two identical tops in a row for a token);
  - that `ts_ms` is non-decreasing per token;
  - that every `l2` window starts with snapshots;
  - that no data rows fall inside a `gaps` stretch.
- **Analysis:** copy day folders to the PC and query them with the **DuckDB CLI**, a standalone tool rather than a crate. The runbook includes 3 example queries:
  - one token's top of book over a week;
  - the `l2` book at an episode's peak;
  - a day's gaps.
- **Later (5c and R2):** the same files move to R2, DuckDB reads them there, and backtests build on them.

## 7. Configuration

**`config/compact.toml`** (LF, pinned by `.gitattributes`):
- `raw_dir`, `archive_dir`;
- `depth_every_secs = 300`, `depth_levels = 10`;
- `l2_before_secs = 300`, `l2_after_secs = 300`, `l2_max_window_secs = 1800`;
- `archive_budget_gb = 55`;
- `lag_alarm_hours = 3`.

It's validated at startup. Its sha256 goes into the manifest.

**`config/scanner.toml`:** `retention_hours = 24`, `max_capture_gb = 15`.

None of these values are premise inputs. That must be checked (premise sections and `premises.lock` untouched), and the plan includes the check.

## 8. Testing

- **Golden hour:** a small, hand-built raw hour (a snapshot, level changes, a `Down` line, a second snapshot) produces exactly the expected `tob`, `depth`, `l2` and `gaps` rows.
- **Agreement with the scanner:** for a recorded capture hour (a fixture), the `tob` stream equals the top of book that `scanner replay` reconstructs at every change, token by token.
- **Resume:** 3 hours processed in one go equal 3 hours processed with a restart from the checkpoint after hour 1, byte for byte for the Parquet data pages (or row for row, if Parquet metadata carries a timestamp).
- **Crash safety:** a write interrupted before the rename leaves no `done` row and no final file, and the re-run is correct.
- **Delete guard:** the scanner doesn't delete a non-`done` hour under the age rule, deletes it and marks it `lost` at the hard cap, and falls back safely when Postgres is down.
- **Budget:** over budget, the oldest day goes first and `oldest_day` updates.
- **Universe written only on change:** see §4.5.
- **`compact verify`:** passes on a good day and fails on each kind of corruption.
- **Arch:** `compact` has no network client and reaches Postgres only through `store`.
- **Benchmark (`#[ignore]`, run by hand):** one real raw hour, copied once from the VPS by the owner. It measures the time, peak RSS, and bytes per table, extrapolated to a day. It must fit 0.25 CPU (an hour processed in well under an hour of CPU time, target < 10 min in a release build) and 256 MB. The plan sets the final `depth_*` and `l2_*` defaults from these numbers.

## 9. Deploy (runbook `docs/superpowers/runbooks/phase5b-storage-vps.md`)

This goes after the current 5d redeploy and the other agent's frontend changes.

1. **Migration 0008:** `compact_hours`, `compact_status`, and a `NOLOGIN` role `compact_rw`. Grants:
   - `compact_rw`: SELECT on `episodes` and `runs`; INSERT and UPDATE on `compact_hours` and `compact_status`;
   - `scanner_rw`: SELECT and INSERT on `compact_hours`;
   - `analyze_ro` (the api): SELECT on `compact_status`.
2. **Activate the role,** following 5a §11: a new `.env` key `COMPACT_RW_DB_PASSWORD`, then `ALTER ROLE compact_rw LOGIN PASSWORD …`.
3. **Compose:** the `compact` service and the `archive` volume.
4. **Order:** migrate (run alone, check the version), then the scanner (new retention and guard), then compact. Checks:
   - the first hour becomes `done` within about 2 h;
   - `compact_status` fills;
   - the raw folder shrinks to about a day over 24 h;
   - the Health fields show;
   - `compact verify` passes on the first full day.
5. **Rollback:** stop `compact`. The scanner's guard then falls back to age plus cap, because hours are never `done`, which is today's behaviour with the new numbers. The archive volume and the tables can stay.

## 10. Decisions

- **D1:** reduced data lives in Parquet files, not Postgres (size, and keeping the load off the operational database).
- **D2:** `l2` windows only around net-positive arbitrage episodes, not incomplete sets. Owner-approved: "looks right".
- **D3:** `depth` every 5 min, top 10 levels, changed tokens only. Owner-approved, subject to the benchmark.
- **D4:** hourly compaction with a 1–2 h lag, not nightly, so raw is never deleted before it's reduced.
- **D5:** the scanner may still delete an uncompacted hour at the hard cap. Capture never stalls; the loss is recorded.
- **D6:** no archive UI or query service now. DuckDB on the PC reads it.
- **D7:** `parquet` and `arrow` approved by the owner, 2026-09-27.

## 11. Open items for the plan

- **Reuse:** find exactly which replay pieces (`scanner::run::replay_cmd`, capture line reading, book application) can be shared, and whether that means moving them into a library crate.
- **Parquet writer:** check the memory behaviour of the `parquet`/`arrow` writer with row-group sizes that fit 256 MB, and pick the row-group size.
- **Benchmark:** the owner copies one raw hour from the VPS for the benchmark (a runbook step). Without it, the defaults stay as in §3.
- **Timestamps:** confirm `ts_ms` (server) versus `recv_ms` for `tob` ordering when frames arrive out of order across shards. Order by `recv_ms` inside an hour, and keep both.
