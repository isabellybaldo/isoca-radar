# S3c · paper trader wired into the scanner (live and replay) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run the pure `sim::PaperTrader` (S3b) inside the scanner, both live and in `scanner replay`. It writes every intent, fill, ledger entry, markout and mark to new `paper_*` tables. Markets settle from Gamma's payout vector. Live accounts survive restarts, and two replays of one capture produce identical ledgers (spec §19 Phase 3 exit: "Deterministic replay; equity identity holds").

**Architecture:**
- The engine owns an optional `PaperTrader` and lends it its own YES books through a zero-copy `Books` impl, built from split field borrows. It follows the S3b call contract: `on_frame` before a frame's events (so before any delta, deferred top-check or phantom-level trim of that frame), `on_level` after each applied delta **and for each level S3a's `Book::trim_to_top` removes** (a trim is a removal the consumed-liquidity ledger must see), `on_snapshot` after each applied ws snapshot and each accepted REST snapshot, `on_open` at each episode Open (wherever it happens: a frame, a deferred check's re-evaluation, a REST snapshot). A resolution and the final close are driven to their instant with `on_frame(t)` first (S3b: `close()` must follow `on_frame(t)`), then `on_resolution(t, …, books)` / `close(t, books)`.
- Paper rows leave the engine as `Out::Paper`. Live, they go through the existing batching writer. In replay they go through `replay_dir_with`'s streaming sink (aba9ce5) into bounded chunks that a writer task commits as they come, so a long replay never holds its outputs in memory. Both paths use one mapping function, `paper::push_sim`.
- The per-frame cost stays O(the trader's queued work + accounts), never O(universe) (soak run 2 lesson). The engine's pending-top indexes (S3a-2) are untouched: no new code reads or writes `pending_tops`.
- Resolutions reach the engine only as capture control lines (`Ctl::Resolved`), so replay settles exactly as live did. A live resolver task asks Gamma's read-only `/markets` route about the markets the trader holds.

**Tech Stack:** Rust 1.98.1 (pinned), tokio 1.x, `sim` (S3b), `store` (sqlx 0.8, Postgres 17), `pm-api` (reqwest via its own client), `arb-core` (`stats::{parse_rfc3339_ms, days_to_end, annualized}` from S3b Task 1). No new external crates.

**Spec:** `docs/superpowers/specs/2026-09-22-arb-scanner-design.md`: §5.5 (lock-up figures), §5.6 (accounting, payout vector), §8.2/§8.3 (paper trader beside the engine, same core for live and replay), §10 (storage, roles, zero is a measurement), §11 (policies, pre-registration hash, replay), §15 (sim/replay golden tests, role tests), §16 (route table), §18 Q19 (non-binary payouts), §19 (Phase 3 exit). Earlier plans that land **before** this one:
- `docs/superpowers/plans/2026-09-25-s3a-scanner-hardening.md` (**landed**: commits `4b68be2..088c43d` on `refactor`): migration 0003 (explicit grants; `unit_oversize` kind), store `build.rs` (`rerun-if-changed=migrations`), `SET LOCAL statement_timeout = '60s'` in the universe transactions, `mark_suspect(event_id, until_ms, now_ms) -> Vec<Out>`, deferred top-checks (`pending_tops`, `resolve_pending(token, recv_ms, out)`, `check_book(conn, token, ts, recv_ms, server, out)`), the phantom-level trim (`Book::trim_to_top`) and per-(kind, token) integrity throttle (`EngineCtx.issue_throttle_ms`), held-closed episodes and `suspect(g, until, since, out)`, `close_all`'s flush of suppressed throttle counts, sticky shards, REST classes (`Limiter::new(per_sec, reserved)`, `run_rechecks(.., max_in_flight, ..)`), the `VecDeque` writer, and the single shutdown deadline (`SHUTDOWN_BUDGET`, `by_deadline`, `wait_thread`).
- `docs/superpowers/plans/2026-09-25-s3b-sim.md`: the `sim` crate and `config/policies.toml` (**landed**: `bbf0a3e..027485f`). Where its landed code differs from that plan's text, **the code in `crates/sim/src/*.rs` wins** and this plan uses it:
  - `PaperTrader::on_resolution(t_ms, market_id, payout_yes, payout_no, books: &impl Books)` takes the books (its `AccountRow` values the account's OTHER open positions at the bid).
  - `PaperTrader::close(t_ms, books)` must be preceded by `on_frame(t_ms, books)`: `close` only disposes of what is still outstanding (completes conversions at `t_ms`, drops intents/unwinds/markouts, marks).
  - One global due-work timeline ordered by `(due_ms, Rank, seq)`: pending < unwind < conversion < markout < mark at the same ms.
  - `decide` sizes by bisection plus `settle()` (venue-rounding fixpoint); `decide_for` narrows the open context to the account's own consumed liquidity (`filtered_ctx`) and has the `SkipReason::Consumed` short-circuit (fixed below, carry-over).
  - `PaperTrader::restore(cfg, state)` already starts an account fresh when its saved spec differs, and releases reservations.
  - Unwinds that cannot trade (below `min_order_shares`, or no book) still emit a `FillRow` with side `"sell"`, `filled` 0 and `vwap` None.
  - `FillRow.levels_used: u32`, `LedgerEntry.seq: u64`, `FillRow.side: &'static str`; `Books` is `fn yes_book(&self, yes_token: &str) -> Option<&Book>`.
- S3a-2 (**landed**: `22050be..2c5b84d`): `Engine::evaluate` runs a detector only when its exact top-of-book gate (`yes_basket_may_signal` / `no_basket_may_signal`) is true or an episode is open for its key (`set_threshold_gate`, `detector_calls`); `pending_tops` has two secondary indexes kept in step by `pending_insert` / `pending_remove` (plus `pending_by_conn`, `pending_by_group`); `run_live` logs a per-minute `FrameTiming` line (`engine: frames=… parse_us … on_frame_us …`). Any engine code that inserts or removes a pending top-check must go through those helpers. This plan adds none.
- `aba9ce5`: replay streams its outputs: `replay_dir_with(dir, ctx, sink: impl FnMut(Out)) -> anyhow::Result<ReplayStats>`, `replay_dir` is a collecting wrapper (tests), and `replay_cmd` passes a sink.
- VPS soak run 2 (`8a16757`): replay equality passed; the drops came from per-frame engine CPU (an O(pending) scan). The trader hooks in this plan are costed in ruling 23.

Every "current code" anchor below was checked against the tree at `8a16757` (S3a, S3b, S3a-2, aba9ce5 landed). Refreshed 2026-09-25 (second refresh): rulings 19–25.

## Carry-over from the S3b final review (do this FIRST, before Task 1)

The S3b final re-review left one open finding in `crates/sim/src/trader.rs` `decide_for` (commit 027485f). Because S3c persists skip reasons into `paper_intents`, it must be fixed before those rows are written.
- **The problem:** today `decide_for` checks `takes_kind`, builds `filtered_ctx`, and returns `Skip(Consumed)` whenever the filtered edge (net, or gross for P3) is ≤ 0 or the filtered full-set depth is 0, BEFORE `decide` runs. That includes signals whose RAW edge is already ≤ 0: gross-only episodes the detector emits because `gross.pnl_gross > 0`. It also hides the policy gates: `PairNotApproved` for P2, and `TooLong` / `AnnualizedTooLow` / `NoLockupFigures` for P1.
- **Controller ruling:** first run `decide(cfg, spec, free_cash, ctx)` on the UNFILTERED ctx. If that returns `NotMine` or `Skip(_)`, return it unchanged. Only when the raw decision is `Take` do you apply the consumed filter, returning `Skip(Consumed)` if the filtered edge ≤ 0 or its depth is 0, and otherwise the filtered `decide`.

**Files:**
- Modify: `crates/sim/src/trader.rs` (`decide_for`, the `policy` import, the `row_from` doc comment)
- Test: `crates/sim/tests/trader.rs`

- [ ] **Step 1: Write the failing tests.** Append to `crates/sim/tests/trader.rs` (its imports already include `Group`, `detect_no_basket`, `FeeSchedule`, `Outcome`, `PaperTrader`, `PolicyKind`, `SimOut`; `books_of`, `fills`, `cfg` and `NO3` are defined at the top of the file):
```rust
/// S3b final review carry-over: consumption can only explain a skip the raw feed would have
/// taken. A gross-positive, net-negative signal on a FRESH account (nothing consumed) is
/// `not_net_positive`, never `consumed`.
#[test]
fn a_fresh_account_skips_a_net_negative_signal_as_not_net_positive() {
    let g = no_group(common::fee("0.25", 1)); // Σφ ≈ 0.169 > gross edge 0.05
    let sig = detect_no_basket(&g).unwrap();
    assert!(sig.gross.pnl_gross > Decimal::ZERO && sig.quote.pnl <= Decimal::ZERO);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    let outs = t.on_open(&ctx("e1", 1000, &g, sig, &NO3));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.as_deref() == Some("not_net_positive")),
        "{outs:?}"
    );
}

/// The policy gate speaks first: an unapproved P2 pair on a book this account already consumed
/// is `pair_not_approved`, not `consumed`.
#[test]
fn an_unapproved_pair_on_a_consumed_book_is_pair_not_approved() {
    let a = common::live(Some("0.60"), None, FeeSchedule::ZERO); // NO_A ask 0.40 × 100
    let b = common::live(None, Some("0.55"), FeeSchedule::ZERO); // YES_B ask 0.55 × 100
    let sig = arb_core::detect::detect_implication(&a, &b).unwrap();
    let g = Group {
        legs: vec![a, b],
        augmented: false,
    };
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P2Combo, "1000")])).unwrap();
    // An approved pair trades the whole level on both legs (100 × 0.95 = 95 ≤ per_trade 100)...
    let mut approved = ctx("e1", 1000, &g, sig.clone(), &[Outcome::No, Outcome::Yes]);
    approved.pair_approved = true;
    let outs = t.on_open(&approved);
    assert!(matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.is_none()), "{outs:?}");
    assert_eq!(
        fills(&t.on_frame(1100, &books)),
        vec![("m0".into(), d("100")), ("m1".into(), d("100"))]
    );
    // ...so both levels are now fully consumed by this account. The next, unapproved episode on
    // the same book is skipped by the policy, whatever consumption says.
    let outs = t.on_open(&ctx("e2", 1200, &g, sig, &[Outcome::No, Outcome::Yes]));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.as_deref() == Some("pair_not_approved")),
        "{outs:?}"
    );
}
```

- [ ] **Step 2: Run them to verify they fail.**
Run: `cargo test -p sim --test trader -- a_fresh_account_skips an_unapproved_pair`
Expected: both FAIL: the skip reason is `consumed` (a fresh account's filtered edge equals its raw edge, ≤ 0; a fully consumed book's filtered depth is 0).

- [ ] **Step 3: Fix `decide_for`.** In `crates/sim/src/trader.rs`, drop `takes_kind` from the `crate::policy::{…}` import (`decide` already returns `NotMine`), and replace `decide_for` (doc comment included) with:
```rust
    /// `decide` on the raw feed first, then, only for a raw `Take`, narrowed to what account
    /// `a`'s own consumed liquidity leaves available (spec §5.6, controller rulings): a real bot
    /// can't refill an order it already took unless the feed shows genuine turnover. Consumption
    /// can only explain a skip the raw feed would have taken, so every policy verdict
    /// (`NotMine`, `NotNetPositive`, P1's lock-up gates, `PairNotApproved`, `NoCash`,
    /// `BelowMinSize`, `BadInput`) keeps its own reason (S3b final review carry-over).
    fn decide_for(&self, a: usize, ctx: &OpenCtx) -> Decision {
        let acct = &self.accounts[a];
        let free = acct.free_cash();
        let raw = decide(&self.cfg, &acct.spec, free, ctx);
        if !matches!(raw, Decision::Take(_)) {
            return raw;
        }
        let Some(fctx) = self.filtered_ctx(a, ctx) else {
            // Unreachable after a raw Take: `decide` rejects the same malformed shapes
            // (mismatched lengths, a used leg missing from `ctx.legs`) as BadInput.
            return Decision::Skip(SkipReason::BadInput);
        };
        let naive = acct.spec.policy == PolicyKind::P3Naive;
        let quote = if naive {
            &fctx.signal.gross
        } else {
            &fctx.signal.quote
        };
        let edge = if naive { quote.pnl_gross } else { quote.pnl };
        let depth = quote
            .legs
            .iter()
            .map(|l| l.filled)
            .min()
            .unwrap_or(Decimal::ZERO);
        if edge <= Decimal::ZERO || depth <= Decimal::ZERO {
            return Decision::Skip(SkipReason::Consumed);
        }
        decide(&self.cfg, &acct.spec, free, &fctx)
    }
```
In the same file, replace the stale doc comment on `row_from` (`/// Builds an AccountRow … shared by account_row (valued at the bid) and on_resolution (valued at 0; no books there).`) with:
```rust
    /// Builds an `AccountRow` from an account and an already-computed valuation. `account_row`
    /// is its only caller: marks, resolutions and the close all value open positions at the bid.
```

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p sim`
Expected: PASS, including the two new tests and the unchanged `a_second_episode_cannot_refill_consumed_liquidity` (its e3 is still `consumed`: the raw feed takes it, the filtered book has no depth) and `consumed_liquidity_is_not_shared_across_accounts`.

- [ ] **Step 5: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p sim --all-targets -- -D warnings`
Expected: clean (no unused `takes_kind`).
```bash
git add crates/sim/src/trader.rs crates/sim/tests/trader.rs
git commit -m "sim: policy verdicts before the consumed-liquidity skip (S3b final review carry-over)"
```

## Global Constraints

- PAPER TRADING ONLY. No order placement, no wallet, no keys, no signing code. `sim` produces ledger rows and nothing else (spec §16 #5). The scanner reaches the network only through `pm-api`, and Postgres only through `store` (`deny.toml` wrappers).
- `pm-api` exposes only the routes in `routes::ALLOWED`. `tests/routes.rs` asserts that table exactly, and no URL literal may appear outside `routes.rs` (spec §16 #3).
- **The engine and `sim` are pure:** no clock and no IO. Time arrives on the inputs. Any new engine input that live feeds must also be fed by `replay.rs` from the capture's `ctl` lines, stamped with the same instant (spec §9.3).
- Exact decimals only. No `f32`/`f64`. The workspace clippy config denies `float_arithmetic`, `unwrap_used` and `expect_used`. Test files (and `#[cfg(test)]` modules) start with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- Money, sizes and prices in the new tables use **unconstrained `NUMERIC`** (the house deviation from §10's `NUMERIC(20,6)`, S2b plan: a 6 dp price × a 6 dp size has 12 dp, and a scaled column would round silently).
- **Zero is a measurement** (§10): `NULL` = not measured, `0` = measured zero. `Option<Decimal>` stays `Option` all the way to Postgres.
- After migration 0003, `scanner_rw` has **no** default INSERT/UPDATE (nor sequence USAGE): every new table needs an explicit `GRANT` in its own migration. Default SELECT (0001) still applies to both roles, but 0004 grants it explicitly too. `analyze_ro` only ever reads. The paper migration is **0004**, after 0003.
- `paper_state` is **live only**. Replay never reads it and never writes it: each replay starts every account fresh.
- The whole graceful stop, including the paper close and the state save, fits S3a's `SHUTDOWN_BUDGET` (20 s) inside compose's 30 s `stop_grace_period`.
- **Engine per-frame cost** (soak run 2): nothing added to `Engine::on_frame` may be O(universe) or O(pending top-checks). The trader's hooks cost O(its queued work + accounts) per frame. Any code that adds or removes a pending top-check uses `pending_insert` / `pending_remove` (this plan adds none).
- **Replay memory stays bounded** (aba9ce5): replay outputs go through `replay_dir_with`'s sink. No code path collects a whole replay's outputs (or rows) into one `Vec`, except the small test fixtures through `replay_dir`.
- Disk budget: the project stays under 30 GB. Debug `cargo test`/`clippy` only: no `--release`, no `docker build` in this plan.
- Commit messages are plain, with **no** `Co-Authored-By` or any other attribution trailer. Stage only the files you touch.
- Bash prefix for every command: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`
- Store tests need Postgres. Start it once with `docker compose -f compose.yml -f compose.dev.yml up -d postgres`, then prefix store test commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" &&`. Output containing `SKIPPED` does **not** count as passing.

## Review Focus

1. **A market that is closed but still awaiting its UMA outcome.** Its `outcomePrices` still show trading prices (e.g. `["0.62","0.38"]`, which sum to 1). It must never settle a position at those prices. Pinned in Task 2 (`a_closed_market_awaiting_resolution_is_not_a_payout`).
2. **Gamma ignoring the `id` filter** (the keyset lesson: a renamed parameter fails silently) and returning unrelated closed markets. These must not produce `Ctl::Resolved` lines, settlements or `non_binary_payout` rows for markets we never asked about. Pinned in Task 2 (`only_the_requested_markets_come_back`).
3. **A restart after `policies.toml` changed an account** (same id, different `per_trade`). The saved account must not continue under the new spec. It is a different pre-registered experiment, so it starts fresh, with a log line. A corrupt saved state is an error, never a silent reset. Pinned in Task 3 (`a_saved_account_whose_spec_changed_starts_fresh`).
4. **The same thing arriving twice.** A resolution can come from both the ws hint and the periodic poll. The writer can retry a batch whose commit actually succeeded. Neither may double a settlement or a row. Pinned in Task 4 (`a_repeated_resolution_settles_once`) and Task 1 (`paper_rows_round_trip_exactly_and_a_rewrite_changes_nothing`).
5. **A Postgres outage long enough to hit `max_pending_rows`.** It must never evict paper rows: a gap in `paper_ledger` breaks the equity identity. Pinned in Task 6 (`paper_rows_are_never_evicted_by_the_cap`).
6. **A phantom level (S3a soak) that an account already consumed.** When S3a's stream repair trims it, the account's consumed amount at that price must clear, as for any removal; otherwise every later episode on that leg fills short forever. Pinned in Task 4 (`a_trimmed_phantom_level_clears_consumed_liquidity`).
7. **An episode opened by a REST snapshot.** `rest_snapshot` evaluates with `recv_ms = server ts` (often minutes old), so an execution clock taken from it would fill "in the past". Pinned in Task 4 (`an_episode_opened_by_a_rest_snapshot_executes_after_the_latest_frame`).
8. **A resolution or a shutdown that arrives before any later frame**, while an execution is already due. The due execution must run at its own time first (S3b: `close` follows `on_frame(t)`); otherwise a stop silently drops a trade whose t_x already passed, or a settlement runs before the buy it should include. Pinned in Task 4 (`close_first_runs_the_work_due_before_the_close_instant`, `a_resolution_first_runs_the_work_due_before_it`).
9. **A busy feed with the trader attached.** Attaching the trader must not change one scanner output (episodes, integrity, rechecks, resnapshots) and must stay cheap per frame. Pinned in Task 4 (`the_paper_trader_never_changes_the_scanners_own_outputs` on the S3a-2 synthetic high-rate script, and the ignored bench `engine_us_per_frame_with_and_without_the_paper_trader`).

---

## File Structure

```
crates/store/migrations/0004_paper.sql       NEW: paper_* tables, explicit grants, integrity kind non_binary_payout
crates/store/src/rows.rs                     MODIFY: Paper*Row types; Batch paper fields + rows/take_paper/extend_paper/into_singles
crates/store/src/paper.rs                    NEW: write_paper (idempotent inserts, state upsert), readers, load_paper_state
crates/store/src/batch.rs                    MODIFY: write_batch calls write_paper; Table gets the paper tables
crates/store/src/lib.rs                      MODIFY: mod paper
crates/store/tests/paper.rs                  NEW: round trip, idempotency, state upsert, FK data error, batch helpers
crates/store/tests/roles.rs                  MODIFY: scanner appends/upserts paper rows; analyze_ro can't write them
crates/store/tests/migrate.rs                MODIFY: the paper tables exist
crates/pm-api/src/routes.rs                  MODIFY: GAMMA_MARKETS + ALLOWED entry
crates/pm-api/src/gamma.rs                   MODIFY: umaResolutionStatus, Resolution, payout_vector, parse_resolutions, GammaClient::resolutions
crates/pm-api/tests/routes.rs                MODIFY: the new table
crates/pm-api/tests/resolutions.rs           NEW: fixture-based payout parsing
crates/pm-api/tests/live.rs                  MODIFY: ignored live smoke of the new route
docs/superpowers/specs/2026-09-22-arb-scanner-design.md  MODIFY: §16 route list
crates/sim/src/trader.rs                     MODIFY: decide_for (carry-over); held_markets(); on_level/on_snapshot early-out
crates/sim/src/consumed.rs                   MODIFY: on_snapshot skips the book copy when the token has nothing consumed
crates/sim/tests/trader.rs                   MODIFY: carry-over skip-reason tests
crates/sim/tests/lifecycle.rs                MODIFY: held_markets test
bins/scanner/Cargo.toml                      MODIFY: depend on sim
bins/scanner/src/config.rs                   MODIFY: [paper] section
config/scanner.toml                          MODIFY: [paper] section
docker/rust.Dockerfile                       MODIFY: copy policies.toml into the image
bins/scanner/src/paper.rs                    NEW: policies + hash, SimOut → rows, state rows + restore, resolver loop
bins/scanner/src/lib.rs                      MODIFY: pub mod paper
bins/scanner/src/engine.rs                   MODIFY: PaperTrader field, EngineBooks, call contract (incl. S3a trims), OpenCtx, lock-up fields, on_resolution
bins/scanner/tests/engine_paper.rs           NEW: engine ↔ trader wiring
bins/scanner/tests/bench_engine.rs           MODIFY: ignored bench, engine µs/frame with and without the trader
bins/scanner/src/capture.rs                  MODIFY: Ctl::Resolved
bins/scanner/src/replay.rs                   MODIFY: replay_dir_paper_with (replay_dir_with wraps it); Resolved; paper close
bins/scanner/src/run.rs                      MODIFY: replay_cmd streams paper rows in bounded chunks (Task 5); live wiring (Task 7)
bins/scanner/src/main.rs                     MODIFY: policies + run hash (Task 5 replay, Task 7 run)
bins/scanner/tests/replay.rs                 MODIFY: paper replay golden test
bins/scanner/src/writer.rs                   MODIFY: WriterMsg::Paper, Pending paper rows (never evicted), quarantine via into_singles
docs/superpowers/runbooks/s3-paper.md        NEW: Phase 3 exit runbook
```

---

### Task 1: store: migration 0004 (`paper_*` tables), rows, idempotent writes, state load

**Files:**
- Create: `crates/store/migrations/0004_paper.sql`, `crates/store/src/paper.rs`, `crates/store/tests/paper.rs`
- Modify: `crates/store/src/rows.rs`, `crates/store/src/batch.rs`, `crates/store/src/lib.rs`, `crates/store/tests/roles.rs`, `crates/store/tests/migrate.rs`
- Modify (compile fix only): `bins/scanner/src/writer.rs` (`Pending::take`)

**Interfaces:**
- Consumes: S3a's `crates/store/build.rs` (`cargo:rerun-if-changed=migrations`, so `sqlx::migrate!` embeds the new 0004 file without a clean build) and 0003's revoked default INSERT/UPDATE for `scanner_rw` (a table created after 0003 is not writable by the scanner until granted; see `roles.rs::a_table_created_after_0003_is_not_writable_by_scanner_but_its_own_tables_are`). The current `Batch` (rows.rs) has exactly `episodes, observations, integrity, health` and `is_empty()`; `Table` (batch.rs) has `Observations, IntegrityEvents, Health, EpisodeLegs`.
- Produces:
  - `store::{PaperAccountRow, PaperIntentRow, PaperExecRow, PaperFillRow, PaperLedgerRow, PaperMarkoutRow, PaperMarkRow, PaperStateRow}`, with the fields shown in Step 3.
  - `Batch` gains `paper_accounts, paper_intents, paper_execs, paper_fills, paper_ledger, paper_markouts, paper_marks, paper_state` (all `Vec`).
  - `Batch::rows(&self) -> usize`, `Batch::paper_rows(&self) -> usize`, `Batch::take_paper(&mut self) -> Batch`, `Batch::extend_paper(&mut self, other: Batch)` and `Batch::into_singles(self) -> Vec<Batch>`. `is_empty` now means `rows() == 0`.
  - `Store::write_batch` writes the paper rows in the same transaction: accounts first, then `ON CONFLICT DO NOTHING` on each natural key. `paper_state` is upserted, and the newer `updated_ms` wins.
  - `Store::paper_ledger_for_run(run_id) -> Vec<PaperLedgerRow>` (ordered by account, seq), `Store::paper_marks_for_run(run_id) -> Vec<PaperMarkRow>` (by account, t_ms) and `Store::load_paper_state() -> Vec<PaperStateRow>` (by account).
  - `Table::{PaperAccounts, PaperIntents, PaperExecs, PaperFills, PaperLedger, PaperMarkouts, PaperMarks, PaperState}`.
  - Integrity kind `'non_binary_payout'`.

- [ ] **Step 1: Write the failing tests.**

Create `crates/store/tests/paper.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
use store::*;

async fn new_run(store: &Store) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c+p".into(),
            mode: "replay".into(),
        })
        .await
        .unwrap()
}

const A: &str = "P0-converter/sp-idle";

fn ledger(run: i64) -> Vec<PaperLedgerRow> {
    vec![
        PaperLedgerRow {
            run_id: run,
            account_id: A.into(),
            seq: 1,
            t_ms: 1_300,
            kind: "buy".into(),
            episode_id: Some("replay-1-00000001".into()),
            market_id: Some("m0".into()),
            yes_token: Some("y0".into()),
            outcome: Some("no".into()),
            qty: dec!(51.2),
            price: Some(dec!(0.6)),
            cash_delta: dec!(-30.72),
            fees: dec!(0),
            realized_delta: dec!(0),
            cash_after: dec!(969.28),
            equity: None,
        },
        PaperLedgerRow {
            run_id: run,
            account_id: A.into(),
            seq: 2,
            t_ms: 60_000,
            kind: "mark".into(),
            episode_id: None,
            market_id: None,
            yes_token: None,
            outcome: None,
            qty: dec!(0),
            price: None,
            cash_delta: dec!(0),
            fees: dec!(0),
            realized_delta: dec!(0),
            cash_after: dec!(969.28),
            equity: Some(dec!(1000.000000000001)),
        },
    ]
}

fn paper_batch(run: i64) -> Batch {
    Batch {
        paper_accounts: vec![PaperAccountRow {
            run_id: run,
            account_id: A.into(),
            policy: "P0-converter".into(),
            scenario: "sp-idle".into(),
            fee_bips: dec!(0),
            bankroll: dec!(1000),
            per_trade: dec!(100),
            policies_sha256: "ab".repeat(32),
        }],
        paper_intents: vec![PaperIntentRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            decided_ms: 1_000,
            exec_ms: Some(1_300),
            skip_reason: None,
            q_target: Some(dec!(51.2)),
            detected_q: Some(dec!(100)),
            detected_pnl: Some(dec!(5)),
            reserve: Some(dec!(99.841)),
        }],
        paper_execs: vec![PaperExecRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            exec_ms: 1_300,
            q_eff: dec!(51.2),
            leg_risk_qty: dec!(0),
            pnl_at_tx: None,
            hit: false,
        }],
        paper_fills: vec![PaperFillRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            market_id: "m0".into(),
            yes_token: "y0".into(),
            outcome: "no".into(),
            side: "buy".into(),
            t_ms: 1_300,
            limit_price: Some(dec!(0.6)),
            requested: dec!(51.2),
            filled: dec!(51.2),
            vwap: Some(dec!(0.6)),
            fees: dec!(0),
            levels_used: 1,
        }],
        paper_ledger: ledger(run),
        paper_markouts: vec![PaperMarkoutRow {
            run_id: run,
            account_id: A.into(),
            episode_id: "replay-1-00000001".into(),
            market_id: "m0".into(),
            yes_token: "y0".into(),
            outcome: "no".into(),
            fill_vwap: dec!(0.6),
            offset_ms: 1_000,
            mid: Some(dec!(0.48)),
            markout: Some(dec!(-0.12)),
        }],
        paper_marks: vec![PaperMarkRow {
            run_id: run,
            account_id: A.into(),
            t_ms: 60_000,
            cash: dec!(969.28),
            equity: dec!(1000),
            realized: dec!(0),
            unrealized: dec!(0),
            fees_paid: dec!(0),
            locked: dec!(30.72),
            max_drawdown: dec!(0),
            n_positions: 1,
        }],
        ..Batch::default()
    }
}

async fn counts(s: &Store) -> Vec<i64> {
    let mut v = Vec::new();
    for t in [
        Table::PaperAccounts,
        Table::PaperIntents,
        Table::PaperExecs,
        Table::PaperFills,
        Table::PaperLedger,
        Table::PaperMarkouts,
        Table::PaperMarks,
    ] {
        v.push(s.count(t).await.unwrap());
    }
    v
}

#[tokio::test]
async fn paper_rows_round_trip_exactly_and_a_rewrite_changes_nothing() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let b = paper_batch(run);
    store.write_batch(&b).await.unwrap();
    assert_eq!(counts(&store).await, vec![1, 1, 1, 1, 2, 1, 1]);
    let back = store.paper_ledger_for_run(run).await.unwrap();
    assert_eq!(back, ledger(run), "exact, zeros stay zeros and NULLs stay NULL");
    assert_eq!(back[1].equity, Some(dec!(1000.000000000001)), "12 dp survive");
    assert_eq!(store.paper_marks_for_run(run).await.unwrap(), b.paper_marks);
    // the writer re-sends a batch whose commit succeeded but whose ack was lost
    store.write_batch(&b).await.unwrap();
    assert_eq!(counts(&store).await, vec![1, 1, 1, 1, 2, 1, 1], "idempotent");
    db.drop(store).await;
}

#[tokio::test]
async fn paper_state_keeps_the_newest_save_per_account() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let st = |id: &str, body: &str, ms: i64| PaperStateRow {
        account_id: id.into(),
        state: body.into(),
        updated_ms: ms,
    };
    store
        .write_batch(&Batch {
            paper_state: vec![st("a", "{\"v\":1}", 1), st("a", "{\"v\":2}", 2), st("b", "{}", 2)],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        store.load_paper_state().await.unwrap(),
        vec![st("a", "{\"v\":2}", 2), st("b", "{}", 2)]
    );
    // an OLDER save (a restored batch from a failed write) never overwrites a newer one
    store
        .write_batch(&Batch {
            paper_state: vec![st("a", "{\"v\":0}", 0), st("b", "{\"v\":3}", 3)],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        store.load_paper_state().await.unwrap(),
        vec![st("a", "{\"v\":2}", 2), st("b", "{\"v\":3}", 3)]
    );
    db.drop(store).await;
}

#[tokio::test]
async fn a_paper_row_without_its_account_is_a_data_error() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    let mut b = paper_batch(run);
    b.paper_accounts.clear();
    let e = store.write_batch(&b).await.unwrap_err();
    assert!(e.is_data_error(), "FK violation → quarantine path, not a retry loop: {e}");
    db.drop(store).await;
}

#[tokio::test]
async fn integrity_accepts_non_binary_payout() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    let run = new_run(&store).await;
    store
        .write_batch(&Batch {
            integrity: vec![IntegrityRow {
                run_id: run,
                ts_ms: 1,
                kind: "non_binary_payout".into(),
                shard: None,
                token: None,
                event_id: Some("e1".into()),
                detail: Some("market m0: payout yes 0.5 no 0.5".into()),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(store.count(Table::IntegrityEvents).await.unwrap(), 1);
    db.drop(store).await;
}

#[test]
fn into_singles_puts_accounts_before_the_rows_that_reference_them() {
    let b = paper_batch(1);
    let n = b.rows();
    assert_eq!((n, b.paper_rows()), (8, 8));
    let singles = b.into_singles();
    assert_eq!(singles.len(), n);
    assert!(singles.iter().all(|s| s.rows() == 1));
    assert_eq!(singles[0].paper_accounts.len(), 1, "accounts first");
}

#[test]
fn take_and_extend_move_only_paper_rows() {
    let mut b = paper_batch(1);
    b.integrity.push(IntegrityRow {
        run_id: 1,
        ts_ms: 1,
        kind: "drop".into(),
        shard: None,
        token: None,
        event_id: None,
        detail: None,
    });
    let p = b.take_paper();
    assert_eq!((b.rows(), b.paper_rows(), p.rows()), (1, 0, 8));
    let mut c = Batch::default();
    c.extend_paper(p.clone());
    c.extend_paper(p);
    assert_eq!(c.paper_ledger.len(), 4);
    assert!(!c.is_empty());
    assert!(Batch::default().is_empty());
}
```

Append to `crates/store/tests/roles.rs`:
```rust
#[tokio::test]
async fn scanner_appends_paper_rows_and_upserts_state_but_analyze_only_reads() {
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
        .expect("run");
    let acct = PaperAccountRow {
        run_id: run,
        account_id: "P0-converter/sp-idle".into(),
        policy: "P0-converter".into(),
        scenario: "sp-idle".into(),
        fee_bips: 0.into(),
        bankroll: 1000.into(),
        per_trade: 100.into(),
        policies_sha256: "ab".repeat(32),
    };
    let mark = PaperLedgerRow {
        run_id: run,
        account_id: acct.account_id.clone(),
        seq: 1,
        t_ms: 60_000,
        kind: "mark".into(),
        episode_id: None,
        market_id: None,
        yes_token: None,
        outcome: None,
        qty: 0.into(),
        price: None,
        cash_delta: 0.into(),
        fees: 0.into(),
        realized_delta: 0.into(),
        cash_after: 1000.into(),
        equity: Some(1000.into()),
    };
    let state = |ms: i64| PaperStateRow {
        account_id: "P0-converter/sp-idle".into(),
        state: format!("{{\"saved\":{ms}}}"),
        updated_ms: ms,
    };
    scanner
        .write_batch(&Batch {
            paper_accounts: vec![acct],
            paper_ledger: vec![mark],
            paper_state: vec![state(1)],
            ..Batch::default()
        })
        .await
        .expect("scanner appends paper rows");
    scanner
        .write_batch(&Batch {
            paper_state: vec![state(2)],
            ..Batch::default()
        })
        .await
        .expect("scanner upserts paper_state (needs UPDATE)");
    assert_eq!(scanner.load_paper_state().await.expect("reads its state")[0].updated_ms, 2);
    for sql in [
        "UPDATE paper_ledger SET qty = qty",
        "DELETE FROM paper_ledger",
        "UPDATE paper_accounts SET bankroll = bankroll",
        "DELETE FROM paper_state",
    ] {
        let e = scanner.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "history is append-only, state never deleted: {sql}: {e}");
    }
    let analyze = db.role("analyze_ro").await;
    assert_eq!(analyze.paper_ledger_for_run(run).await.expect("analyze reads").len(), 1);
    for sql in [
        "INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('x', '{}', 1)",
        "UPDATE paper_state SET updated_ms = 3",
        "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, \
         fees_paid, locked, max_drawdown, n_positions) VALUES (1, 'x', 1, 0, 0, 0, 0, 0, 0, 0, 0)",
    ] {
        let e = analyze.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    analyze.close().await;
    scanner.close().await;
    db.drop(owner).await;
}
```

In `crates/store/tests/migrate.rs`, extend the table list with the new tables:
```rust
    for t in [
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
        "runs",
        "universe_snapshots",
    ] {
```

- [ ] **Step 2: Run them to verify they fail.**
Run: `docker compose -f compose.yml -f compose.dev.yml up -d postgres && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: compile errors: `cannot find struct PaperLedgerRow`, `no field paper_ledger on type Batch`, `no variant PaperAccounts`.

- [ ] **Step 3: Add the row types and the `Batch` helpers.** In `crates/store/src/rows.rs`, replace the `Batch` struct and its `impl` with:
```rust
/// One paper account of a run: its pre-registered spec (spec §11) and the policies file hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperAccountRow {
    pub run_id: i64,
    pub account_id: String,
    pub policy: String,
    pub scenario: String,
    pub fee_bips: Decimal,
    pub bankroll: Decimal,
    pub per_trade: Decimal,
    pub policies_sha256: String,
}

/// A policy's decision on an opened episode (taken, or skipped with a reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperIntentRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub decided_ms: i64,
    pub exec_ms: Option<i64>,
    pub skip_reason: Option<String>,
    pub q_target: Option<Decimal>,
    pub detected_q: Option<Decimal>,
    pub detected_pnl: Option<Decimal>,
    pub reserve: Option<Decimal>,
}

/// What happened at t_x.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperExecRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub exec_ms: i64,
    pub q_eff: Decimal,
    pub leg_risk_qty: Decimal,
    pub pnl_at_tx: Option<Decimal>,
    pub hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperFillRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub market_id: String,
    pub yes_token: String,
    /// yes | no
    pub outcome: String,
    /// buy | sell
    pub side: String,
    pub t_ms: i64,
    pub limit_price: Option<Decimal>,
    pub requested: Decimal,
    pub filled: Decimal,
    pub vwap: Option<Decimal>,
    pub fees: Decimal,
    pub levels_used: i32,
}

/// One `sim::account::LedgerEntry`. Keyed (run_id, account_id, seq).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperLedgerRow {
    pub run_id: i64,
    pub account_id: String,
    pub seq: i64,
    pub t_ms: i64,
    /// buy | sell | convert | settle | mark
    pub kind: String,
    pub episode_id: Option<String>,
    pub market_id: Option<String>,
    pub yes_token: Option<String>,
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
pub struct PaperMarkoutRow {
    pub run_id: i64,
    pub account_id: String,
    pub episode_id: String,
    pub market_id: String,
    pub yes_token: String,
    pub outcome: String,
    pub fill_vwap: Decimal,
    pub offset_ms: i64,
    pub mid: Option<Decimal>,
    pub markout: Option<Decimal>,
}

/// One `sim::rows::AccountRow` (a mark). Identity: equity = bankroll + realized + unrealized.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperMarkRow {
    pub run_id: i64,
    pub account_id: String,
    pub t_ms: i64,
    pub cash: Decimal,
    pub equity: Decimal,
    pub realized: Decimal,
    pub unrealized: Decimal,
    pub fees_paid: Decimal,
    pub locked: Decimal,
    pub max_drawdown: Decimal,
    pub n_positions: i64,
}

/// Live only: one serialized `sim::account::Account` per account id (restored at start).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperStateRow {
    pub account_id: String,
    pub state: String,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    pub episodes: Vec<EpisodeWrite>,
    pub observations: Vec<ObservationRow>,
    pub integrity: Vec<IntegrityRow>,
    pub health: Vec<HealthRow>,
    /// Paper trading (S3c). Accounts are written first: every other paper row references one.
    pub paper_accounts: Vec<PaperAccountRow>,
    pub paper_intents: Vec<PaperIntentRow>,
    pub paper_execs: Vec<PaperExecRow>,
    pub paper_fills: Vec<PaperFillRow>,
    pub paper_ledger: Vec<PaperLedgerRow>,
    pub paper_markouts: Vec<PaperMarkoutRow>,
    pub paper_marks: Vec<PaperMarkRow>,
    /// Live only: upserted by account id (the newest `updated_ms` wins).
    pub paper_state: Vec<PaperStateRow>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.rows() == 0
    }

    pub fn paper_rows(&self) -> usize {
        self.paper_accounts.len()
            + self.paper_intents.len()
            + self.paper_execs.len()
            + self.paper_fills.len()
            + self.paper_ledger.len()
            + self.paper_markouts.len()
            + self.paper_marks.len()
            + self.paper_state.len()
    }

    pub fn rows(&self) -> usize {
        self.episodes.len()
            + self.observations.len()
            + self.integrity.len()
            + self.health.len()
            + self.paper_rows()
    }

    /// Move the paper rows out into a batch of their own (the rest stays here).
    pub fn take_paper(&mut self) -> Batch {
        Batch {
            paper_accounts: std::mem::take(&mut self.paper_accounts),
            paper_intents: std::mem::take(&mut self.paper_intents),
            paper_execs: std::mem::take(&mut self.paper_execs),
            paper_fills: std::mem::take(&mut self.paper_fills),
            paper_ledger: std::mem::take(&mut self.paper_ledger),
            paper_markouts: std::mem::take(&mut self.paper_markouts),
            paper_marks: std::mem::take(&mut self.paper_marks),
            paper_state: std::mem::take(&mut self.paper_state),
            ..Batch::default()
        }
    }

    /// Append `other`'s paper rows after ours, in order. `other`'s non-paper rows are ignored.
    pub fn extend_paper(&mut self, other: Batch) {
        self.paper_accounts.extend(other.paper_accounts);
        self.paper_intents.extend(other.paper_intents);
        self.paper_execs.extend(other.paper_execs);
        self.paper_fills.extend(other.paper_fills);
        self.paper_ledger.extend(other.paper_ledger);
        self.paper_markouts.extend(other.paper_markouts);
        self.paper_marks.extend(other.paper_marks);
        self.paper_state.extend(other.paper_state);
    }

    /// Every row as its own batch (the writer's quarantine path). Episodes come before the
    /// observations that reference them, and paper accounts before the paper rows.
    pub fn into_singles(self) -> Vec<Batch> {
        let mut out = Vec::with_capacity(self.rows());
        macro_rules! each {
            ($($field:ident),*) => {$(
                for x in self.$field {
                    out.push(Batch { $field: vec![x], ..Batch::default() });
                }
            )*};
        }
        each!(
            episodes,
            observations,
            integrity,
            health,
            paper_accounts,
            paper_intents,
            paper_execs,
            paper_fills,
            paper_ledger,
            paper_markouts,
            paper_marks,
            paper_state
        );
        out
    }
}
```

- [ ] **Step 4: Write the migration.** Create `crates/store/migrations/0004_paper.sql`:
```sql
-- S3c: paper trading (spec §5.6 accounting, §11 policies). Rows are per run: live runs and replay
-- runs each write under their own run_id, so two replays of one capture compare with EXCEPT.
-- Money, sizes and prices are unconstrained NUMERIC (house rule since S2b: exact, never rounded on
-- insert). Timestamps are BIGINT ms; *_at columns are for humans.
-- Grants are explicit (0003 revoked the scanner's default INSERT/UPDATE and sequence USAGE on
-- every table created after it): the scanner may only APPEND history (ON CONFLICT DO NOTHING needs
-- no UPDATE) and upsert paper_state. No DELETE anywhere. Every key is natural, so there are no
-- sequences to grant. analyze_ro reads everything (its default SELECT still applies; spelled out).

CREATE TABLE paper_accounts (
    run_id           BIGINT  NOT NULL REFERENCES runs (run_id),
    account_id       TEXT    NOT NULL,
    policy           TEXT    NOT NULL CHECK (policy IN ('P0-converter', 'P1-settlement', 'P2-combo', 'P3-naive')),
    scenario         TEXT    NOT NULL,
    fee_bips         NUMERIC NOT NULL,
    bankroll         NUMERIC NOT NULL,
    per_trade        NUMERIC NOT NULL,
    policies_sha256  TEXT    NOT NULL,
    PRIMARY KEY (run_id, account_id)
);

CREATE TABLE paper_intents (
    run_id        BIGINT NOT NULL,
    account_id    TEXT   NOT NULL,
    episode_id    TEXT   NOT NULL,
    decided_ms    BIGINT NOT NULL,
    exec_ms       BIGINT,
    skip_reason   TEXT,
    q_target      NUMERIC,
    detected_q    NUMERIC,
    detected_pnl  NUMERIC,
    reserve       NUMERIC,
    PRIMARY KEY (run_id, account_id, episode_id),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_execs (
    run_id        BIGINT  NOT NULL,
    account_id    TEXT    NOT NULL,
    episode_id    TEXT    NOT NULL,
    exec_ms       BIGINT  NOT NULL,
    q_eff         NUMERIC NOT NULL,
    leg_risk_qty  NUMERIC NOT NULL,
    pnl_at_tx     NUMERIC,
    hit           BOOLEAN NOT NULL,
    PRIMARY KEY (run_id, account_id, episode_id),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_fills (
    run_id       BIGINT  NOT NULL,
    account_id   TEXT    NOT NULL,
    episode_id   TEXT    NOT NULL,
    market_id    TEXT    NOT NULL,
    yes_token    TEXT    NOT NULL,
    outcome      TEXT    NOT NULL CHECK (outcome IN ('yes', 'no')),
    side         TEXT    NOT NULL CHECK (side IN ('buy', 'sell')),
    t_ms         BIGINT  NOT NULL,
    limit_price  NUMERIC,
    requested    NUMERIC NOT NULL,
    filled       NUMERIC NOT NULL,
    vwap         NUMERIC,
    fees         NUMERIC NOT NULL,
    levels_used  INTEGER NOT NULL,
    PRIMARY KEY (run_id, account_id, episode_id, market_id, outcome, side),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

CREATE TABLE paper_ledger (
    run_id          BIGINT  NOT NULL,
    account_id      TEXT    NOT NULL,
    seq             BIGINT  NOT NULL,
    t_ms            BIGINT  NOT NULL,
    kind            TEXT    NOT NULL CHECK (kind IN ('buy', 'sell', 'convert', 'settle', 'mark')),
    episode_id      TEXT,
    market_id       TEXT,
    yes_token       TEXT,
    outcome         TEXT CHECK (outcome IN ('yes', 'no')),
    qty             NUMERIC NOT NULL,
    price           NUMERIC,
    cash_delta      NUMERIC NOT NULL,
    fees            NUMERIC NOT NULL,
    realized_delta  NUMERIC NOT NULL,
    cash_after      NUMERIC NOT NULL,
    equity          NUMERIC,
    t_at            TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(t_ms::double precision / 1000)) STORED,
    PRIMARY KEY (run_id, account_id, seq),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);
CREATE INDEX paper_ledger_episode ON paper_ledger (episode_id);

CREATE TABLE paper_markouts (
    run_id      BIGINT  NOT NULL,
    account_id  TEXT    NOT NULL,
    episode_id  TEXT    NOT NULL,
    market_id   TEXT    NOT NULL,
    yes_token   TEXT    NOT NULL,
    outcome     TEXT    NOT NULL CHECK (outcome IN ('yes', 'no')),
    fill_vwap   NUMERIC NOT NULL,
    offset_ms   BIGINT  NOT NULL,
    mid         NUMERIC,
    markout     NUMERIC,
    PRIMARY KEY (run_id, account_id, episode_id, market_id, outcome, offset_ms),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

-- One AccountRow per mark (every minute of input time, at resolutions, at close).
CREATE TABLE paper_marks (
    run_id        BIGINT  NOT NULL,
    account_id    TEXT    NOT NULL,
    t_ms          BIGINT  NOT NULL,
    cash          NUMERIC NOT NULL,
    equity        NUMERIC NOT NULL,
    realized      NUMERIC NOT NULL,
    unrealized    NUMERIC NOT NULL,
    fees_paid     NUMERIC NOT NULL,
    locked        NUMERIC NOT NULL,
    max_drawdown  NUMERIC NOT NULL,
    n_positions   BIGINT  NOT NULL,
    t_at          TIMESTAMPTZ GENERATED ALWAYS AS (to_timestamp(t_ms::double precision / 1000)) STORED,
    PRIMARY KEY (run_id, account_id, t_ms),
    FOREIGN KEY (run_id, account_id) REFERENCES paper_accounts (run_id, account_id)
);

-- Live only: the accounts that survive a restart (one serialized sim Account per row, exact JSON).
CREATE TABLE paper_state (
    account_id  TEXT   PRIMARY KEY,
    state       TEXT   NOT NULL,
    updated_ms  BIGINT NOT NULL
);

GRANT SELECT, INSERT ON
    paper_accounts, paper_intents, paper_execs, paper_fills, paper_ledger, paper_markouts, paper_marks
TO scanner_rw;
GRANT SELECT, INSERT, UPDATE ON paper_state TO scanner_rw;
GRANT SELECT ON
    paper_accounts, paper_intents, paper_execs, paper_fills, paper_ledger, paper_markouts,
    paper_marks, paper_state
TO analyze_ro;

-- Q19: a resolution whose payout vector is not {1,0}/{0,1} (UMA 50-50, voided leg) is a flagged risk class.
-- The list is 0003's (which added unit_oversize) plus non_binary_payout.
ALTER TABLE integrity_events DROP CONSTRAINT IF EXISTS integrity_events_kind_check;
ALTER TABLE integrity_events ADD CONSTRAINT integrity_events_kind_check CHECK (kind IN (
    'crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
    'drop', 'reconnect_gap', 'parse_error', 'shard_parked',
    'capture_drop', 'writer_drop', 'capture_budget_delete',
    'unit_oversize', 'non_binary_payout'
));
```

- [ ] **Step 5: Write the paper SQL.** Create `crates/store/src/paper.rs`:
```rust
//! Paper-trading rows (S3c, migration 0004). Every table but `paper_state` is append-only and
//! idempotent (`ON CONFLICT DO NOTHING` on its natural key), so a batch the writer re-sends after
//! a commit whose ack was lost changes nothing. `paper_state` keeps the newest save per account.
use crate::text::{clean, clean_opt};
use crate::{Batch, PaperLedgerRow, PaperMarkRow, PaperStateRow, Store, StoreError};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use std::collections::HashMap;

/// ≤ 16 binds per row: 1000 rows stay far under Postgres's 65,535-parameter limit.
const CHUNK: usize = 1000;

const LEDGER_COLS: &str = "run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
                           qty, price, cash_delta, fees, realized_delta, cash_after, equity";
const MARK_COLS: &str = "run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
                         max_drawdown, n_positions";

/// Keep only the last save of each account (one statement can't upsert the same key twice).
fn dedupe_last_state(rows: &[PaperStateRow]) -> Vec<&PaperStateRow> {
    let mut idx: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<&PaperStateRow> = Vec::new();
    for r in rows {
        match idx.get(r.account_id.as_str()) {
            Some(&i) => out[i] = r,
            None => {
                idx.insert(r.account_id.as_str(), out.len());
                out.push(r);
            }
        }
    }
    out
}

/// Inside `write_batch`'s transaction. Accounts first: every other paper row references one.
pub(crate) async fn write_paper(conn: &mut PgConnection, b: &Batch) -> Result<(), StoreError> {
    for chunk in b.paper_accounts.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, \
             per_trade, policies_sha256) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.policy))
                .push_bind(clean(&r.scenario))
                .push_bind(r.fee_bips)
                .push_bind(r.bankroll)
                .push_bind(r.per_trade)
                .push_bind(clean(&r.policies_sha256));
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_intents.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_intents (run_id, account_id, episode_id, decided_ms, exec_ms, skip_reason, \
             q_target, detected_q, detected_pnl, reserve) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(r.decided_ms)
                .push_bind(r.exec_ms)
                .push_bind(clean_opt(&r.skip_reason))
                .push_bind(r.q_target)
                .push_bind(r.detected_q)
                .push_bind(r.detected_pnl)
                .push_bind(r.reserve);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_execs.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_execs (run_id, account_id, episode_id, exec_ms, q_eff, leg_risk_qty, \
             pnl_at_tx, hit) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(r.exec_ms)
                .push_bind(r.q_eff)
                .push_bind(r.leg_risk_qty)
                .push_bind(r.pnl_at_tx)
                .push_bind(r.hit);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_fills.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_fills (run_id, account_id, episode_id, market_id, yes_token, outcome, side, \
             t_ms, limit_price, requested, filled, vwap, fees, levels_used) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(clean(&r.market_id))
                .push_bind(clean(&r.yes_token))
                .push_bind(clean(&r.outcome))
                .push_bind(clean(&r.side))
                .push_bind(r.t_ms)
                .push_bind(r.limit_price)
                .push_bind(r.requested)
                .push_bind(r.filled)
                .push_bind(r.vwap)
                .push_bind(r.fees)
                .push_bind(r.levels_used);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_ledger.chunks(CHUNK) {
        let mut qb =
            QueryBuilder::<Postgres>::new(format!("INSERT INTO paper_ledger ({LEDGER_COLS}) "));
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(r.seq)
                .push_bind(r.t_ms)
                .push_bind(clean(&r.kind))
                .push_bind(clean_opt(&r.episode_id))
                .push_bind(clean_opt(&r.market_id))
                .push_bind(clean_opt(&r.yes_token))
                .push_bind(clean_opt(&r.outcome))
                .push_bind(r.qty)
                .push_bind(r.price)
                .push_bind(r.cash_delta)
                .push_bind(r.fees)
                .push_bind(r.realized_delta)
                .push_bind(r.cash_after)
                .push_bind(r.equity);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_markouts.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_markouts (run_id, account_id, episode_id, market_id, yes_token, outcome, \
             fill_vwap, offset_ms, mid, markout) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(clean(&r.episode_id))
                .push_bind(clean(&r.market_id))
                .push_bind(clean(&r.yes_token))
                .push_bind(clean(&r.outcome))
                .push_bind(r.fill_vwap)
                .push_bind(r.offset_ms)
                .push_bind(r.mid)
                .push_bind(r.markout);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in b.paper_marks.chunks(CHUNK) {
        let mut qb =
            QueryBuilder::<Postgres>::new(format!("INSERT INTO paper_marks ({MARK_COLS}) "));
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.run_id)
                .push_bind(clean(&r.account_id))
                .push_bind(r.t_ms)
                .push_bind(r.cash)
                .push_bind(r.equity)
                .push_bind(r.realized)
                .push_bind(r.unrealized)
                .push_bind(r.fees_paid)
                .push_bind(r.locked)
                .push_bind(r.max_drawdown)
                .push_bind(r.n_positions);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
    for chunk in dedupe_last_state(&b.paper_state).chunks(CHUNK) {
        let mut qb =
            QueryBuilder::<Postgres>::new("INSERT INTO paper_state (account_id, state, updated_ms) ");
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(clean(&r.account_id))
                .push_bind(clean(&r.state))
                .push_bind(r.updated_ms);
        });
        // A restored (older) batch must never overwrite a newer save.
        qb.push(
            " ON CONFLICT (account_id) DO UPDATE SET state = EXCLUDED.state, updated_ms = EXCLUDED.updated_ms \
             WHERE paper_state.updated_ms <= EXCLUDED.updated_ms",
        );
        qb.build().execute(&mut *conn).await?;
    }
    Ok(())
}

impl Store {
    pub async fn paper_ledger_for_run(&self, run_id: i64) -> Result<Vec<PaperLedgerRow>, StoreError> {
        let sql = format!(
            "SELECT {LEDGER_COLS} FROM paper_ledger WHERE run_id = $1 ORDER BY account_id, seq"
        );
        Ok(sqlx::query_as::<_, PaperLedgerRow>(&sql)
            .bind(run_id)
            .fetch_all(self.pool())
            .await?)
    }

    pub async fn paper_marks_for_run(&self, run_id: i64) -> Result<Vec<PaperMarkRow>, StoreError> {
        let sql =
            format!("SELECT {MARK_COLS} FROM paper_marks WHERE run_id = $1 ORDER BY account_id, t_ms");
        Ok(sqlx::query_as::<_, PaperMarkRow>(&sql)
            .bind(run_id)
            .fetch_all(self.pool())
            .await?)
    }

    /// The live accounts saved by the last run (replay never calls this).
    pub async fn load_paper_state(&self) -> Result<Vec<PaperStateRow>, StoreError> {
        Ok(sqlx::query_as::<_, PaperStateRow>(
            "SELECT account_id, state, updated_ms FROM paper_state ORDER BY account_id",
        )
        .fetch_all(self.pool())
        .await?)
    }
}
```

In `crates/store/src/lib.rs`, add `mod paper;` below `mod batch;`.

In `crates/store/src/batch.rs`:
- Add the paper tables to `Table`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Observations,
    IntegrityEvents,
    Health,
    EpisodeLegs,
    PaperAccounts,
    PaperIntents,
    PaperExecs,
    PaperFills,
    PaperLedger,
    PaperMarkouts,
    PaperMarks,
    PaperState,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Observations => "observations",
            Table::IntegrityEvents => "integrity_events",
            Table::Health => "health",
            Table::EpisodeLegs => "episode_legs",
            Table::PaperAccounts => "paper_accounts",
            Table::PaperIntents => "paper_intents",
            Table::PaperExecs => "paper_execs",
            Table::PaperFills => "paper_fills",
            Table::PaperLedger => "paper_ledger",
            Table::PaperMarkouts => "paper_markouts",
            Table::PaperMarks => "paper_marks",
            Table::PaperState => "paper_state",
        }
    }
}
```
- In `write_batch`, directly before its final `tx.commit().await?;` (after the health upsert's `qb.build().execute(&mut *tx).await?;`), add:
```rust
        crate::paper::write_paper(&mut tx, b).await?;
```
(`tx` is a `Transaction<'_, Postgres>`, whose `DerefMut` target is `PgConnection`; the existing inserts in `write_batch` pass `&mut *tx`. If deref coercion does not kick in, write `crate::paper::write_paper(&mut *tx, b)`.) `write_batch` returns early on `b.is_empty()`, which Step 3 redefines as `rows() == 0`, so a batch carrying only `paper_state` rows is written.

- [ ] **Step 6: Keep the scanner compiling.** S3a's `Pending::take` in `bins/scanner/src/writer.rs` builds `Batch { episodes, observations, integrity, health }` field by field (the three queues via `std::mem::take(&mut self.…).into()`). Add `..Batch::default()` as the last line of that literal. Task 6 replaces this function. No other scanner code builds a `Batch` without `..Batch::default()` (`quarantine` and `replay_cmd` already use it or push into a default batch).

- [ ] **Step 7: Run the tests to verify they pass.**
Run: `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store && cargo check -p scanner --all-targets`
Expected: PASS. `paper.rs` has 6 tests. `roles.rs` includes the new test and S3a's `a_table_created_after_0003_…`. `migrate.rs` sees every paper table. The output has no `SKIPPED`, and `scanner` compiles.

- [ ] **Step 8: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p store -p scanner --all-targets -- -D warnings`
Expected: clean.
```bash
git add crates/store/migrations/0004_paper.sql crates/store/src/paper.rs crates/store/src/rows.rs crates/store/src/batch.rs crates/store/src/lib.rs crates/store/tests/paper.rs crates/store/tests/roles.rs crates/store/tests/migrate.rs bins/scanner/src/writer.rs
git commit -m "store: paper_* tables (append-only, idempotent), paper_state upsert, non_binary_payout kind"
```

---

### Task 2: pm-api: Gamma `/markets` resolutions (read-only route)

**Files:**
- Modify: `crates/pm-api/src/routes.rs`, `crates/pm-api/src/gamma.rs`, `crates/pm-api/tests/routes.rs`, `crates/pm-api/tests/live.rs`, `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` (§16 item 3)
- Create: `crates/pm-api/tests/resolutions.rs`

**Interfaces:**
- Consumes: `wire::string_array`, `GammaMarket`, `http_client()`.
- Produces:
  - `routes::GAMMA_MARKETS = "/markets"` and `ALLOWED[1] = ("GET", "https://gamma-api.polymarket.com/markets")`.
  - `GammaMarket.uma_resolution_status: Option<String>`.
  - `gamma::Resolution { market_id: String, payout_yes: Decimal, payout_no: Decimal }` (Debug, Clone, PartialEq, Eq).
  - `gamma::payout_vector(&GammaMarket) -> Option<(Decimal, Decimal)>` and `gamma::parse_markets(&str) -> Result<Vec<GammaMarket>, ApiError>`.
  - `gamma::parse_resolutions(text: &str, wanted: &[String]) -> Result<Vec<Resolution>, ApiError>` (only markets in `wanted`, in response order).
  - `gamma::RESOLUTION_CHUNK: usize = 50` and `GammaClient::resolutions(&self, market_ids: &[String]) -> Result<Vec<Resolution>, ApiError>`.

**The acceptance rule** (spec §5.6 "the market's payout vector", Q19). A market's `outcomePrices` count as its payout only when all of the following hold:
- the market is `closed`;
- it has exactly two values, each in [0, 1], and they sum to exactly 1;
- the vector is binary (1/0 or 0/1), or `umaResolutionStatus == "resolved"`. A closed market still in dispute shows its last trading prices, which also sum to 1, so a fractional vector needs that positive confirmation. See the rulings.

- [ ] **Step 1: Write the failing tests.** Create `crates/pm-api/tests/resolutions.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::gamma::{Resolution, parse_markets, parse_resolutions, payout_vector};
use rust_decimal_macros::dec;

/// Shaped like `GET /markets?id=…&closed=true`: an array of markets, `outcomePrices` as a
/// JSON-encoded string (Gamma's habit), ids as strings or numbers, many fields we ignore.
const MARKETS: &str = r#"[
  {"id": "501001", "conditionId": "0xaa01", "question": "Will A win?", "slug": "will-a-win",
   "active": true, "closed": true, "acceptingOrders": false, "enableOrderBook": true, "negRisk": true,
   "clobTokenIds": "[\"111\", \"112\"]", "outcomes": "[\"Yes\", \"No\"]",
   "outcomePrices": "[\"1\", \"0\"]", "umaResolutionStatus": "resolved",
   "endDate": "2026-09-20T00:00:00Z", "closedTime": "2026-09-21 14:02:11+00"},
  {"id": "501002", "active": true, "closed": true, "outcomePrices": "[\"0\", \"1\"]",
   "umaResolutionStatus": "resolved"},
  {"id": 501003, "active": true, "closed": true, "outcomePrices": "[\"0.5\", \"0.5\"]",
   "umaResolutionStatus": "resolved"},
  {"id": "501004", "active": true, "closed": true, "outcomePrices": "[\"0.62\", \"0.38\"]",
   "umaResolutionStatus": "proposed"},
  {"id": "501005", "active": true, "closed": false, "outcomePrices": "[\"0.45\", \"0.55\"]"},
  {"id": "501006", "closed": true, "outcomePrices": "[\"0.7\", \"0.7\"]", "umaResolutionStatus": "resolved"},
  {"id": "501007", "closed": true, "outcomePrices": "[\"1.2\", \"-0.2\"]", "umaResolutionStatus": "resolved"},
  {"id": "501008", "closed": true, "outcomePrices": null},
  {"id": "501009", "closed": true, "outcomePrices": "[\"1\", \"0\"]"},
  {"id": "501010", "closed": true, "outcomePrices": "[\"0.5\", \"0.5\"]"}
]"#;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn all_ids() -> Vec<String> {
    (1..=10).map(|i| format!("5010{i:02}")).collect()
}

fn r(id: &str, y: rust_decimal::Decimal, n: rust_decimal::Decimal) -> Resolution {
    Resolution { market_id: id.into(), payout_yes: y, payout_no: n }
}

#[test]
fn parses_payout_vectors_exactly() {
    assert_eq!(
        parse_resolutions(MARKETS, &all_ids()).unwrap(),
        vec![
            r("501001", dec!(1), dec!(0)),
            r("501002", dec!(0), dec!(1)),
            r("501003", dec!(0.5), dec!(0.5)), // Q19: a confirmed 50-50 is a payout, flagged later
            r("501009", dec!(1), dec!(0)),     // binary: accepted without a UMA status
        ]
    );
}

#[test]
fn a_closed_market_awaiting_resolution_is_not_a_payout() {
    let ms = parse_markets(MARKETS).unwrap();
    let by_id = |id: &str| ms.iter().find(|m| m.id == id).unwrap();
    assert_eq!(payout_vector(by_id("501004")), None, "closed, still in dispute: trading prices");
    assert_eq!(payout_vector(by_id("501005")), None, "still open");
    assert_eq!(payout_vector(by_id("501006")), None, "sums to 1.4");
    assert_eq!(payout_vector(by_id("501007")), None, "outside [0, 1]");
    assert_eq!(payout_vector(by_id("501008")), None, "no prices");
    assert_eq!(payout_vector(by_id("501010")), None, "fractional without a resolved status");
    assert_eq!(payout_vector(by_id("501003")), Some((dec!(0.5), dec!(0.5))));
}

#[test]
fn only_the_requested_markets_come_back() {
    // if Gamma ever ignores the id filter, unrelated closed markets must not leak through
    assert_eq!(
        parse_resolutions(MARKETS, &ids(&["501002", "999"])).unwrap(),
        vec![r("501002", dec!(0), dec!(1))]
    );
    assert!(parse_resolutions(MARKETS, &[]).unwrap().is_empty());
}

#[test]
fn a_body_that_is_not_a_market_list_is_an_error() {
    assert!(parse_resolutions(r#"{"error": "rate limited"}"#, &all_ids()).is_err());
    assert!(parse_resolutions("[]", &all_ids()).unwrap().is_empty());
}
```
In `crates/pm-api/tests/routes.rs`, replace the first two tests with:
```rust
#[test]
fn route_table_is_exactly_the_read_only_set() {
    assert_eq!(
        ALLOWED,
        &[
            ("GET", "https://gamma-api.polymarket.com/events/keyset"),
            ("GET", "https://gamma-api.polymarket.com/markets"),
            ("GET", "https://clob.polymarket.com/book"),
            ("POST", "https://clob.polymarket.com/books"),
            ("GET", "https://clob.polymarket.com/ok"),
            ("WS", "wss://ws-subscriptions-clob.polymarket.com/ws/market"),
            ("WS", "wss://sports-api.polymarket.com/ws"),
        ]
    );
}

#[test]
fn table_agrees_with_the_constants() {
    assert_eq!(ALLOWED[0].1, format!("{GAMMA_BASE}{GAMMA_EVENTS_KEYSET}"));
    assert_eq!(ALLOWED[1].1, format!("{GAMMA_BASE}{GAMMA_MARKETS}"));
    assert_eq!(ALLOWED[2].1, format!("{CLOB_BASE}{CLOB_BOOK}"));
    assert_eq!(ALLOWED[3].1, format!("{CLOB_BASE}{CLOB_BOOKS}"));
    assert_eq!(ALLOWED[4].1, format!("{CLOB_BASE}{CLOB_OK}"));
    assert_eq!(ALLOWED[5].1, WS_MARKET);
    assert_eq!(ALLOWED[6].1, WS_SPORTS);
}
```
Append to `crates/pm-api/tests/live.rs` (ignored by default, like the others):
```rust
#[tokio::test]
#[ignore]
async fn live_resolutions_route_answers_for_open_markets() {
    let client = GammaClient::new().unwrap();
    let page = client.page(None).await.unwrap();
    let ids: Vec<String> = page
        .events
        .iter()
        .flat_map(|e| e.markets().iter())
        .map(|m| m.id.clone())
        .take(10)
        .collect();
    assert!(!ids.is_empty());
    let found = client.resolutions(&ids).await.unwrap();
    assert!(found.is_empty(), "active markets are not resolved: {found:?}");
}
```

- [ ] **Step 2: Run them to verify they fail.**
Run: `cargo test -p pm-api --test resolutions --test routes`
Expected: compile errors: `parse_resolutions`, `payout_vector`, `Resolution` and `GAMMA_MARKETS` not found.

- [ ] **Step 3: Implement.** In `crates/pm-api/src/routes.rs`, add the constant under `GAMMA_EVENTS_KEYSET` and the table entry as the second row:
```rust
pub const GAMMA_MARKETS: &str = "/markets";
```
```rust
pub const ALLOWED: &[(&str, &str)] = &[
    ("GET", "https://gamma-api.polymarket.com/events/keyset"),
    ("GET", "https://gamma-api.polymarket.com/markets"),
    ("GET", "https://clob.polymarket.com/book"),
    ("POST", "https://clob.polymarket.com/books"),
    ("GET", "https://clob.polymarket.com/ok"),
    ("WS", "wss://ws-subscriptions-clob.polymarket.com/ws/market"),
    ("WS", "wss://sports-api.polymarket.com/ws"),
];
```
In `crates/pm-api/src/gamma.rs`:
- change the routes import to `use crate::routes::{GAMMA_BASE, GAMMA_EVENTS_KEYSET, GAMMA_MARKETS};`;
- add a field to `GammaMarket`, after `end_date`:
```rust
    /// UMA adapter state, e.g. "proposed", "disputed", "resolved" (S3c payout acceptance).
    pub uma_resolution_status: Option<String>,
```
- add, after `leg_status`:
```rust
/// A market's final payout per share of YES and of NO (spec §5.6: never an assumed {0,1}).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub market_id: String,
    pub payout_yes: Decimal,
    pub payout_no: Decimal,
}

/// Ids per `/markets` request.
pub const RESOLUTION_CHUNK: usize = 50;

/// The payout vector of a CLOSED market: two values in [0, 1] summing to exactly 1. A fractional
/// vector (50-50, Q19) also needs `umaResolutionStatus == "resolved"`, because a closed market
/// still in dispute shows its last trading prices, which sum to 1 as well.
pub fn payout_vector(m: &GammaMarket) -> Option<(Decimal, Decimal)> {
    if m.closed != Some(true) {
        return None;
    }
    let p = string_array(&m.outcome_prices, "outcomePrices").ok()?;
    let [y, n] = p.as_slice() else {
        return None;
    };
    let y = Decimal::from_str_exact(y.trim()).ok()?;
    let n = Decimal::from_str_exact(n.trim()).ok()?;
    let unit = |x: Decimal| x >= Decimal::ZERO && x <= Decimal::ONE;
    if !unit(y) || !unit(n) || y + n != Decimal::ONE {
        return None;
    }
    let binary = (y == Decimal::ONE && n == Decimal::ZERO) || (y == Decimal::ZERO && n == Decimal::ONE);
    if !binary && m.uma_resolution_status.as_deref() != Some("resolved") {
        return None;
    }
    Some((y, n))
}

pub fn parse_markets(text: &str) -> Result<Vec<GammaMarket>, ApiError> {
    Ok(serde_json::from_str(text)?)
}

/// Accepted payouts among the markets we asked about (`wanted`), in response order. Anything
/// else in the response is ignored, so a filter Gamma silently drops can't settle a stranger.
pub fn parse_resolutions(text: &str, wanted: &[String]) -> Result<Vec<Resolution>, ApiError> {
    Ok(parse_markets(text)?
        .into_iter()
        .filter(|m| wanted.contains(&m.id))
        .filter_map(|m| {
            payout_vector(&m).map(|(payout_yes, payout_no)| Resolution {
                market_id: m.id.clone(),
                payout_yes,
                payout_no,
            })
        })
        .collect())
}
```
- add to `impl GammaClient`:
```rust
    /// Closed markets among `market_ids` with an accepted payout vector (read-only `GET /markets`).
    pub async fn resolutions(&self, market_ids: &[String]) -> Result<Vec<Resolution>, ApiError> {
        let mut out = Vec::new();
        for chunk in market_ids.chunks(RESOLUTION_CHUNK) {
            let mut query: Vec<(&str, String)> = vec![
                ("closed", "true".to_string()),
                ("limit", chunk.len().to_string()),
            ];
            for id in chunk {
                query.push(("id", id.clone()));
            }
            let text = self
                .http
                .get(format!("{}{}", self.base, GAMMA_MARKETS))
                .query(&query)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            out.extend(parse_resolutions(&text, chunk)?);
        }
        Ok(out)
    }
```

- [ ] **Step 4: Note the route in the spec.** In `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` §16, replace item 3 with:
```markdown
3. `pm-api` exposes only `GET /events/keyset`, `GET /markets` (Gamma, closed markets by id: the paper trader's resolutions, S3c), `GET /book`, `POST /books`, `GET /ok`, the market websocket, and the Sports websocket. A test asserts that route table.
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p pm-api`
Expected: PASS. That includes `resolutions` (4 tests), `routes` (3, including `no_url_literals_outside_routes_rs`) and the existing `gamma` tests. The `live` tests show as ignored.

- [ ] **Step 6: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p pm-api --all-targets -- -D warnings`
Expected: clean.
```bash
git add crates/pm-api/src/routes.rs crates/pm-api/src/gamma.rs crates/pm-api/tests/routes.rs crates/pm-api/tests/resolutions.rs crates/pm-api/tests/live.rs docs/superpowers/specs/2026-09-22-arb-scanner-design.md
git commit -m "pm-api: read-only Gamma /markets resolutions with a strict payout-vector rule"
```

---

### Task 3: scanner paper glue: `[paper]` config, policies + hash, row mapping, state, resolver loop

**Files:**
- Modify: `bins/scanner/Cargo.toml`, `bins/scanner/src/lib.rs`, `bins/scanner/src/config.rs`, `config/scanner.toml`, `docker/rust.Dockerfile`
- Create: `bins/scanner/src/paper.rs`

**Interfaces:**
- Consumes: store `Paper*Row` + `Batch` (Task 1), `pm_api::gamma::Resolution` + `pm_api::ApiError` (Task 2); `sim::{config::SimConfig, trader::{PaperTrader, TraderState}, rows::SimOut, account::Account}` (S3b).
- Produces:
  - `config::PaperCfg { enabled: bool, policies: String, resolver_every_secs: u64, state_save_every_secs: u64 }` as `Config.paper`.
  - `paper::Policies { sim: SimConfig, sha256: String }`.
  - `paper::sha256_hex(&[u8]) -> String` and `paper::parse_policies(&str) -> anyhow::Result<Policies>`.
  - `paper::load_policies(&PaperCfg) -> anyhow::Result<Option<Policies>>` (None when disabled).
  - `paper::run_hash(cfg_hash: &str, Option<&Policies>) -> String` (`"<cfg>+<policies sha256>"`) and `paper::with_policies(&PaperCfg, cfg_hash: &str) -> anyhow::Result<(Option<Policies>, String)>`.
  - `paper::account_rows(run_id: i64, &Policies) -> Vec<PaperAccountRow>` and `paper::push_sim(run_id: i64, SimOut, &mut Batch)`.
  - `paper::state_batch(&TraderState, updated_ms: i64) -> anyhow::Result<Batch>` and `paper::restore_trader(&Policies, &[PaperStateRow]) -> anyhow::Result<PaperTrader>`.
  - `paper::RESOLVE_HINT_DELAY: Duration` (30 s) and `paper::run_resolver(fetch, held: watch::Receiver<Vec<String>>, hint: Arc<Notify>, every: Duration, hint_delay: Duration, out: mpsc::Sender<Resolution>)`, where `fetch: FnMut(Vec<String>) -> Future<Output = Result<Vec<Resolution>, ApiError>>`.

- [ ] **Step 1: Write the failing tests.** Create `bins/scanner/src/paper.rs` containing only the test module for now (the implementation arrives in Step 3):
```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use rust_decimal::Decimal;
    use sim::account::{EntryKind, LedgerEntry};
    use sim::rows::{AccountRow, ExecRow, FillRow, IntentRow, MarkoutRow};
    use sim::types::{LegKey, Outcome};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const POLICIES: &str = include_str!("../../../config/policies.toml");

    fn d(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    #[test]
    fn repo_policies_parse_and_join_the_run_hash() {
        let p = parse_policies(POLICIES).unwrap();
        assert_eq!(p.sha256.len(), 64);
        assert_eq!(p.sha256, parse_policies(POLICIES).unwrap().sha256, "stable");
        assert_eq!(run_hash("abc", Some(&p)), format!("abc+{}", p.sha256));
        assert_eq!(run_hash("abc", None), "abc");
        let rows = account_rows(7, &p);
        assert_eq!(rows.len(), p.sim.accounts.len());
        assert!(rows.iter().all(|r| r.run_id == 7 && r.policies_sha256 == p.sha256));
        assert!(rows.iter().any(|r| r.account_id == "P0-converter/sp-idle/fee10" && r.fee_bips == d("10")));
        let zero = POLICIES.replacen("bankroll = \"1000\"", "bankroll = \"0\"", 1);
        assert!(parse_policies(&zero).is_err(), "validated, not just parsed");
    }

    fn leg() -> LegKey {
        LegKey { market_id: "m0".into(), yes_token: "y0".into(), outcome: Outcome::No }
    }

    #[test]
    fn every_sim_row_maps_to_its_table() {
        let mut b = Batch::default();
        let rows = [
            SimOut::Intent(IntentRow {
                account: "a".into(),
                episode_id: "e".into(),
                decided_ms: 1,
                exec_ms: None,
                skip_reason: Some("no_cash".into()),
                q_target: None,
                detected_q: None,
                detected_pnl: None,
                reserve: None,
            }),
            SimOut::Exec(ExecRow {
                account: "a".into(),
                episode_id: "e".into(),
                exec_ms: 2,
                q_eff: d("0"),
                leg_risk_qty: d("0"),
                pnl_at_tx: None,
                hit: false,
            }),
            SimOut::Fill(FillRow {
                account: "a".into(),
                episode_id: "e".into(),
                leg: leg(),
                t_ms: 2,
                side: "buy",
                limit: Some(d("0.6")),
                requested: d("51.2"),
                filled: d("51.2"),
                vwap: Some(d("0.6")),
                fees: d("0"),
                levels_used: 1,
            }),
            SimOut::Ledger(LedgerEntry {
                account: "a".into(),
                seq: 4,
                t_ms: 2,
                kind: EntryKind::Buy,
                episode_id: Some("e".into()),
                leg: Some(leg()),
                qty: d("51.2"),
                price: Some(d("0.6")),
                cash_delta: d("-30.72"),
                fees: d("0"),
                realized_delta: d("0"),
                cash_after: d("969.28"),
                equity: None,
            }),
            SimOut::Markout(MarkoutRow {
                account: "a".into(),
                episode_id: "e".into(),
                leg: leg(),
                fill_vwap: d("0.6"),
                offset_ms: 1000,
                mid: None,
                markout: None,
            }),
            SimOut::Account(AccountRow {
                account: "a".into(),
                t_ms: 60_000,
                cash: d("969.28"),
                equity: d("1000"),
                realized: d("0"),
                unrealized: d("0"),
                fees_paid: d("0"),
                locked: d("30.72"),
                max_drawdown: d("0"),
                n_positions: 1,
            }),
        ];
        for r in rows {
            push_sim(3, r, &mut b);
        }
        assert_eq!(b.rows(), 6);
        assert_eq!(b.paper_intents[0].skip_reason.as_deref(), Some("no_cash"));
        assert_eq!(b.paper_execs[0].pnl_at_tx, None, "unmeasured stays NULL");
        assert_eq!((b.paper_fills[0].outcome.as_str(), b.paper_fills[0].side.as_str()), ("no", "buy"));
        let l = &b.paper_ledger[0];
        assert_eq!(
            (l.run_id, l.seq, l.kind.as_str(), l.outcome.as_deref(), l.realized_delta),
            (3, 4, "buy", Some("no"), d("0"))
        );
        assert_eq!((b.paper_markouts[0].offset_ms, b.paper_marks[0].n_positions), (1000, 1));
    }

    #[test]
    fn a_saved_account_whose_spec_changed_starts_fresh() {
        let p = parse_policies(POLICIES).unwrap();
        let mut st = PaperTrader::new(p.sim.clone()).unwrap().state();
        st.accounts[0].cash = d("900");
        st.accounts[1].cash = d("800");
        let rows = state_batch(&st, 5).unwrap().paper_state;
        assert_eq!(rows.len(), st.accounts.len());
        assert!(rows.iter().all(|r| r.updated_ms == 5));
        let same = restore_trader(&p, &rows).unwrap();
        assert_eq!((same.accounts()[0].cash, same.accounts()[1].cash), (d("900"), d("800")));
        // the first account's per_trade changed: a different pre-registered experiment
        let changed =
            parse_policies(&POLICIES.replacen("per_trade = \"100\"", "per_trade = \"50\"", 1)).unwrap();
        let t = restore_trader(&changed, &rows).unwrap();
        assert_eq!((t.accounts()[0].cash, t.accounts()[1].cash), (d("1000"), d("800")));
        let bad = vec![store::PaperStateRow {
            account_id: "x".into(),
            state: "{not json".into(),
            updated_ms: 1,
        }];
        assert!(restore_trader(&p, &bad).is_err(), "a corrupt save is loud, never a silent reset");
    }

    fn resolved(id: &str) -> Resolution {
        Resolution { market_id: id.into(), payout_yes: Decimal::ONE, payout_no: Decimal::ZERO }
    }

    #[tokio::test]
    async fn resolver_asks_about_held_markets_and_forwards_what_resolved() {
        let (_held_tx, held_rx) = watch::channel(vec!["m1".to_string(), "m2".to_string()]);
        let (tx, mut rx) = mpsc::channel(8);
        let asked = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
        let seen = asked.clone();
        let task = tokio::spawn(run_resolver(
            move |ids: Vec<String>| {
                seen.lock().unwrap().push(ids);
                async { Ok::<_, pm_api::ApiError>(vec![resolved("m1")]) }
            },
            held_rx,
            Arc::new(Notify::new()),
            Duration::from_millis(20),
            Duration::ZERO,
            tx,
        ));
        let r = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(r, resolved("m1"));
        assert_eq!(asked.lock().unwrap()[0], vec!["m1".to_string(), "m2".to_string()]);
        task.abort();
    }

    #[tokio::test]
    async fn a_ws_hint_polls_soon_without_waiting_for_the_period() {
        let (_held_tx, held_rx) = watch::channel(vec!["m1".to_string()]);
        let (tx, mut rx) = mpsc::channel(8);
        let hint = Arc::new(Notify::new());
        let task = tokio::spawn(run_resolver(
            |_ids: Vec<String>| async { Ok::<_, pm_api::ApiError>(vec![resolved("m1")]) },
            held_rx,
            hint.clone(),
            Duration::from_secs(3_600),
            Duration::from_millis(10),
            tx,
        ));
        hint.notify_one();
        let r = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(r.market_id, "m1");
        task.abort();
    }

    #[tokio::test]
    async fn nothing_held_means_no_gamma_calls() {
        let (_held_tx, held_rx) = watch::channel(Vec::<String>::new());
        let (tx, _rx) = mpsc::channel(8);
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let task = tokio::spawn(run_resolver(
            move |_ids: Vec<String>| {
                c.fetch_add(1, Ordering::Relaxed);
                async { Ok::<_, pm_api::ApiError>(Vec::new()) }
            },
            held_rx,
            Arc::new(Notify::new()),
            Duration::from_millis(10),
            Duration::ZERO,
            tx,
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        task.abort();
    }
}
```
Add `sim = { path = "../../crates/sim" }` to `[dependencies]` in `bins/scanner/Cargo.toml` (after `store`), and add `pub mod paper;` to `bins/scanner/src/lib.rs` (after `pub mod metrics;`).

In `bins/scanner/src/config.rs` `mod tests`, extend `repo_config_parses_with_a_stable_hash` with the lines below (after S3a's `(max_in_flight, resnapshot_reserved_per_sec) == (32, 3)` assertion, before its closing brace), and add one test:
```rust
        assert!(cfg.paper.enabled);
        assert_eq!(cfg.paper.policies, "/etc/cute-trading/policies.toml");
        assert_eq!((cfg.paper.resolver_every_secs, cfg.paper.state_save_every_secs), (600, 60));
```
```rust
    #[test]
    fn rejects_a_hammering_resolver_or_odd_save_cadence() {
        let fast = REPO.replace("resolver_every_secs = 600", "resolver_every_secs = 10");
        assert!(Config::parse(&fast).is_err());
        let never = REPO.replace("state_save_every_secs = 60", "state_save_every_secs = 0");
        assert!(Config::parse(&never).is_err());
    }
```

- [ ] **Step 2: Run them to verify they fail.**
Run: `cargo test -p scanner --lib -- paper:: config::`
Expected: compile errors: `parse_policies`, `run_resolver`, `push_sim` not found; no field `paper` on `Config`.

- [ ] **Step 3: Implement the config.** In `bins/scanner/src/config.rs`, add `pub paper: PaperCfg,` as the last field of `Config`, and this struct after `StoreCfg`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperCfg {
    /// Run the paper trader (live and replay). Off = scanner only, as in S2.
    pub enabled: bool,
    /// The frozen policies file (spec §11). Its sha256 joins the run's config_hash.
    pub policies: String,
    /// Ask Gamma about held markets this often (and soon after a ws market_resolved).
    pub resolver_every_secs: u64,
    /// Save live account state to paper_state this often (restored at start).
    pub state_save_every_secs: u64,
}
```
and at the end of `validate` (after S3a's `engine.issue_throttle_ms` check, before `Ok(())`):
```rust
        anyhow::ensure!(
            self.paper.resolver_every_secs >= 60,
            "paper.resolver_every_secs must be ≥ 60 (Gamma is polled, not hammered)"
        );
        anyhow::ensure!(
            (10..=3600).contains(&self.paper.state_save_every_secs),
            "paper.state_save_every_secs must be 10..=3600"
        );
        anyhow::ensure!(
            !self.paper.enabled || !self.paper.policies.trim().is_empty(),
            "paper.policies must name the policies file"
        );
```
Append to `config/scanner.toml`:
```toml

[paper]
enabled = true                  # spec §11: P0–P3 paper accounts, live and in replay
policies = "/etc/cute-trading/policies.toml"   # frozen per evaluation run; its sha256 joins config_hash
resolver_every_secs = 600       # ask Gamma about held markets (and 30 s after a ws market_resolved)
state_save_every_secs = 60      # live account state → paper_state (restored at the next start)
```
In `docker/rust.Dockerfile`, directly below `COPY config/scanner.toml /etc/cute-trading/scanner.toml`, add:
```dockerfile
COPY config/policies.toml /etc/cute-trading/policies.toml
```

- [ ] **Step 4: Implement `paper.rs`.** Put this ABOVE the test module in `bins/scanner/src/paper.rs`:
```rust
//! Paper-trading glue for the scanner (S3c):
//! - the frozen policies file and its hash (spec §11 pre-registration);
//! - `SimOut` → `store` rows (one mapping for live and replay);
//! - the live `paper_state` rows (restore at start);
//! - the live resolver loop.
//! PAPER ONLY: nothing here can place an order. `sim` produces ledger rows and nothing else.
use crate::config::PaperCfg;
use anyhow::Context;
use pm_api::gamma::Resolution;
use sha2::{Digest, Sha256};
use sim::account::Account;
use sim::config::SimConfig;
use sim::rows::SimOut;
use sim::trader::{PaperTrader, TraderState};
use std::fmt::Write as _;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use store::{
    Batch, PaperAccountRow, PaperExecRow, PaperFillRow, PaperIntentRow, PaperLedgerRow, PaperMarkRow,
    PaperMarkoutRow, PaperStateRow,
};
use tokio::sync::{Notify, mpsc, watch};

/// After a ws `market_resolved`, wait this long before asking Gamma (its payout lags the event).
pub const RESOLVE_HINT_DELAY: Duration = Duration::from_secs(30);

/// The parsed policies file and the sha256 of its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policies {
    pub sim: SimConfig,
    pub sha256: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

pub fn parse_policies(text: &str) -> anyhow::Result<Policies> {
    let sim: SimConfig = toml::from_str(text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(Policies {
        sim,
        sha256: sha256_hex(text.as_bytes()),
    })
}

/// None when paper trading is off.
pub fn load_policies(cfg: &PaperCfg) -> anyhow::Result<Option<Policies>> {
    if !cfg.enabled {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&cfg.policies)
        .with_context(|| format!("read {}", cfg.policies))?;
    parse_policies(&text).map(Some)
}

/// `runs.config_hash` (and every episode's): the scanner config hash, plus `+<policies sha256>`
/// when paper trading is on, so a run names the exact policies it was evaluated under.
pub fn run_hash(cfg_hash: &str, p: Option<&Policies>) -> String {
    match p {
        Some(p) => format!("{cfg_hash}+{}", p.sha256),
        None => cfg_hash.to_string(),
    }
}

pub fn with_policies(cfg: &PaperCfg, cfg_hash: &str) -> anyhow::Result<(Option<Policies>, String)> {
    let p = load_policies(cfg)?;
    let h = run_hash(cfg_hash, p.as_ref());
    Ok((p, h))
}

/// One row per configured account for this run (written before any other paper row).
pub fn account_rows(run_id: i64, p: &Policies) -> Vec<PaperAccountRow> {
    p.sim
        .accounts
        .iter()
        .map(|a| PaperAccountRow {
            run_id,
            account_id: a.id(),
            policy: a.policy.as_str().to_string(),
            scenario: a.scenario.clone(),
            fee_bips: a.fee_bips,
            bankroll: a.bankroll,
            per_trade: a.per_trade,
            policies_sha256: p.sha256.clone(),
        })
        .collect()
}

/// Map one paper-trader output onto its table (live: `Io::route`; replay: `replay_cmd`).
pub fn push_sim(run_id: i64, o: SimOut, b: &mut Batch) {
    match o {
        SimOut::Intent(r) => b.paper_intents.push(PaperIntentRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            decided_ms: r.decided_ms,
            exec_ms: r.exec_ms,
            skip_reason: r.skip_reason,
            q_target: r.q_target,
            detected_q: r.detected_q,
            detected_pnl: r.detected_pnl,
            reserve: r.reserve,
        }),
        SimOut::Exec(r) => b.paper_execs.push(PaperExecRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            exec_ms: r.exec_ms,
            q_eff: r.q_eff,
            leg_risk_qty: r.leg_risk_qty,
            pnl_at_tx: r.pnl_at_tx,
            hit: r.hit,
        }),
        SimOut::Fill(r) => b.paper_fills.push(PaperFillRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            outcome: r.leg.outcome.as_str().to_string(),
            market_id: r.leg.market_id,
            yes_token: r.leg.yes_token,
            side: r.side.to_string(),
            t_ms: r.t_ms,
            limit_price: r.limit,
            requested: r.requested,
            filled: r.filled,
            vwap: r.vwap,
            fees: r.fees,
            levels_used: i32::try_from(r.levels_used).unwrap_or(i32::MAX),
        }),
        SimOut::Ledger(e) => {
            let (market_id, yes_token, outcome) = match e.leg {
                Some(l) => (
                    Some(l.market_id),
                    Some(l.yes_token),
                    Some(l.outcome.as_str().to_string()),
                ),
                None => (None, None, None),
            };
            b.paper_ledger.push(PaperLedgerRow {
                run_id,
                account_id: e.account,
                seq: i64::try_from(e.seq).unwrap_or(i64::MAX),
                t_ms: e.t_ms,
                kind: e.kind.as_str().to_string(),
                episode_id: e.episode_id,
                market_id,
                yes_token,
                outcome,
                qty: e.qty,
                price: e.price,
                cash_delta: e.cash_delta,
                fees: e.fees,
                realized_delta: e.realized_delta,
                cash_after: e.cash_after,
                equity: e.equity,
            });
        }
        SimOut::Markout(m) => b.paper_markouts.push(PaperMarkoutRow {
            run_id,
            account_id: m.account,
            episode_id: m.episode_id,
            outcome: m.leg.outcome.as_str().to_string(),
            market_id: m.leg.market_id,
            yes_token: m.leg.yes_token,
            fill_vwap: m.fill_vwap,
            offset_ms: m.offset_ms,
            mid: m.mid,
            markout: m.markout,
        }),
        SimOut::Account(a) => b.paper_marks.push(PaperMarkRow {
            run_id,
            account_id: a.account,
            t_ms: a.t_ms,
            cash: a.cash,
            equity: a.equity,
            realized: a.realized,
            unrealized: a.unrealized,
            fees_paid: a.fees_paid,
            locked: a.locked,
            max_drawdown: a.max_drawdown,
            n_positions: a.n_positions,
        }),
    }
}

/// The live accounts as `paper_state` rows (exact JSON: decimals serialize as strings).
pub fn state_batch(st: &TraderState, updated_ms: i64) -> anyhow::Result<Batch> {
    let paper_state = st
        .accounts
        .iter()
        .map(|a| {
            Ok(PaperStateRow {
                account_id: a.id.clone(),
                state: serde_json::to_string(a).context("serialize paper account")?,
                updated_ms,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Batch {
        paper_state,
        ..Batch::default()
    })
}

/// Rebuild the live trader from `paper_state`. A saved account whose spec differs from the
/// policies file (same id, e.g. a new per_trade) is a different pre-registered experiment: it
/// starts fresh. `PaperTrader::restore` (S3b) already does that on its own; filtering here only
/// adds the log line. A row that isn't a saved account is an error, never a silent reset.
pub fn restore_trader(p: &Policies, rows: &[PaperStateRow]) -> anyhow::Result<PaperTrader> {
    let mut accounts = Vec::new();
    for r in rows {
        let a: Account = serde_json::from_str(&r.state)
            .with_context(|| format!("paper_state row {} is not a saved account", r.account_id))?;
        match p.sim.accounts.iter().find(|s| s.id() == a.id) {
            Some(spec) if *spec == a.spec => accounts.push(a),
            Some(_) => eprintln!(
                "paper: account {} changed in policies.toml since it was saved; it starts fresh",
                a.id
            ),
            None => {} // no longer configured: PaperTrader::restore would drop it anyway
        }
    }
    PaperTrader::restore(p.sim.clone(), TraderState { accounts }).context("restore paper trader")
}

/// Live resolver: every `every`, and `hint_delay` after each ws hint, ask `fetch` about the
/// markets the trader holds and forward each resolution found. The engine loop captures it as
/// `Ctl::Resolved` BEFORE applying it, so replay settles identically. Nothing held → no call.
pub async fn run_resolver<F, Fut>(
    mut fetch: F,
    held: watch::Receiver<Vec<String>>,
    hint: Arc<Notify>,
    every: Duration,
    hint_delay: Duration,
    out: mpsc::Sender<Resolution>,
) where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<Vec<Resolution>, pm_api::ApiError>>,
{
    loop {
        tokio::select! {
            _ = tokio::time::sleep(every) => {}
            _ = hint.notified() => { tokio::time::sleep(hint_delay).await; }
        }
        let markets = held.borrow().clone();
        if markets.is_empty() {
            continue;
        }
        match fetch(markets).await {
            Ok(found) => {
                for r in found {
                    if out.send(r).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => eprintln!("resolver: {e}"),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib -- paper:: config::`
Expected: PASS: 6 `paper::tests` and 3 `config::tests`.
Run: `grep -n "policies.toml" docker/rust.Dockerfile && docker compose -f compose.yml config --quiet`
Expected: the new `COPY` line is printed, and compose validates quietly. Do not build the image.

- [ ] **Step 6: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p scanner --all-targets -- -D warnings && cargo test -p scanner --test arch`
Expected: clean. `arch` passes: `sim` is pure, and the scanner still reaches the network only through `pm-api`.
```bash
git add bins/scanner/Cargo.toml Cargo.lock bins/scanner/src/lib.rs bins/scanner/src/config.rs bins/scanner/src/paper.rs config/scanner.toml docker/rust.Dockerfile
git commit -m "scanner: [paper] config, policies hash, SimOut→row mapping, state restore, resolver loop"
```

---

### Task 4: engine: the paper trader rides the engine's own books (call contract, open context, lock-up, resolutions)

**Files:**
- Modify: `crates/sim/src/trader.rs`, `crates/sim/src/consumed.rs`, `crates/sim/tests/lifecycle.rs`
- Modify: `bins/scanner/src/engine.rs`, `bins/scanner/src/run.rs` (one placeholder `Io::route` arm; Task 7 replaces it), `bins/scanner/tests/bench_engine.rs`
- Create: `bins/scanner/tests/engine_paper.rs`

**Interfaces:**
- Consumes: `sim::trader::{PaperTrader, Books, TraderState}`, `sim::policy::{OpenCtx, LegInfo}`, `sim::rows::SimOut`, `sim::types::BookSide`; `arb_core::stats::{parse_rfc3339_ms, days_to_end, annualized}`; `arb_core::fill::BasketQuote`. The S3b call contract as landed: `on_frame(recv_ms, books)` BEFORE a frame's events, `on_level`/`on_snapshot` AFTER each applied change, `on_open` at each Open; `on_resolution(t, market, payout_yes, payout_no, books)`; `close(t, books)` only after `on_frame(t, books)`.
- The engine this task hooks into (current code at `8a16757`, `bins/scanner/src/engine.rs`):
  - `on_frame(conn, recv_ms, events)` declares `let mut frame_tokens: Vec<String> = Vec::new();` and then loops `for ev in events`. A `Snapshot` is applied at once through `on_book`; inside `if self.on_book(…) { … }` it calls `self.pending_remove(token)` and pushes the token onto `frame_tokens` (its crossed check runs at frame end via `check_book(conn, token, ts, recv_ms, None, out)`, no trim). A `Delta` first settles an older-ts pending check for its token (`resolve_pending(token, recv_ms, out)`, BEFORE applying the delta), applies the delta through `on_book` (`let found = …`), then `if found { self.pending_insert(token, PendingTop { conn, ts, bid: bb, ask: ba }); }`. At frame end, the conn's stale pending checks (`p.ts < ts − 1 000`) are found through the `pending_by_conn` index and resolved in token order, then `frame_tokens` are checked, then every touched group is evaluated.
  - `evaluate(g, ts, recv_ms, out)` runs a detector only when its exact top-of-book gate is true or its key has an open episode (S3a-2); a skipped detector is provably `None`, so outputs are unchanged.
  - `check_book(conn, token, ts, recv_ms, server: Option<(Option<Price>, Option<Price>)>, out)`: on a top mismatch it calls `Book::trim_to_top(best_bid, best_ask) -> bool` on the token's book (bids strictly above the server's best bid and asks strictly below its best ask are removed), throttles the integrity row per (kind, token) (`issue_throttle_ms`), marks the group suspect via `suspect(g, until, since, out)`, and, if the trim changed the book, re-evaluates the group at `max(ts, group_last_ts)` with the resolving frame's `recv_ms`. **A trim is a book mutation outside the delta path: the paper trader must hear about each removed level.**
  - `rest_snapshot(token, book)` replaces a group leg's book when `book.server_ts_ms() >= ours` (`self.groups[g].legs[l].book = book;`), then `self.pending_remove(token)`, and evaluates with `recv_ms = eval_ts` (a server timestamp, not a receive time).
  - `open_episode(id, key, at, s, out)` is reached from `evaluate`, which runs from frame ends, `check_book` re-evaluations and `rest_snapshot`.
  - `close_all(ts)` clears `pending_tops` and both indexes, closes every episode, and flushes suppressed throttle counts as integrity rows; `mark_suspect` and held-closed re-flags only re-emit episode rows.
  - None of the code below touches `pending_tops` or its indexes.
- Produces:
  - `PaperTrader::held_markets(&self) -> BTreeSet<String>` (sim): markets with a position or returned YES in any account.
  - `Out::Paper(Box<SimOut>)` and `Out::ResolvedHint`.
  - `Engine::set_paper(&mut self, PaperTrader)`, `Engine::paper(&self) -> Option<&PaperTrader>`.
  - `Engine::paper_held_markets(&self) -> Vec<String>` (sorted), `Engine::paper_state(&self) -> Option<TraderState>`, `Engine::paper_close(&mut self, t_ms: i64) -> Vec<Out>` (runs the trader's `on_frame(t_ms)` then `close(t_ms)`).
  - `Engine::on_resolution(&mut self, t_ms: i64, market_id: &str, payout_yes: Decimal, payout_no: Decimal) -> Vec<Out>` (runs the trader's `on_frame(t_ms)` then `on_resolution(…, books)`). A non-binary vector also yields an `Out::Integrity` of kind `non_binary_payout`.
  - `EpisodeRow.days_to_end / annualized_return / post_end` are now filled when an end date is known.
  - Private: `Engine.last_recv_ms` (the latest input instant the trader has been driven to: frame receive times, resolutions, the close) and the free fn `gone_levels(bids_before, asks_before, now: &Book) -> Vec<(BookSide, Decimal)>` (levels a trim removed).

- [ ] **Step 1: Write the failing sim test.** Append to `crates/sim/tests/lifecycle.rs` (it already has `cfg(&[PolicyKind])`, `NO3`, `books_of`, and imports `ctx`, `d`, `no_group`, `detect_no_basket`, `FeeSchedule`):
```rust
#[test]
fn held_markets_lists_positions_and_returned_yes() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[PolicyKind::P0Converter])).unwrap();
    assert!(t.held_markets().is_empty());
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books);
    assert_eq!(t.held_markets().into_iter().collect::<Vec<_>>(), vec!["m0", "m1", "m2"]);
    t.on_frame(3100, &books);
    assert_eq!(
        t.held_markets().into_iter().collect::<Vec<_>>(),
        vec!["m3"],
        "after conversion only the returned YES is held"
    );
    t.on_resolution(9_000, "m3", d("1"), d("0"), &books);
    assert!(t.held_markets().is_empty());
}
```

- [ ] **Step 2: Run it to verify it fails.**
Run: `cargo test -p sim --test lifecycle held_markets`
Expected: compile error, no method `held_markets` on `PaperTrader`.

- [ ] **Step 3: Implement it, plus two per-frame cost guards.** In `crates/sim/src/trader.rs`, change `use std::collections::HashMap;` to `use std::collections::{BTreeSet, HashMap};` and add to `impl PaperTrader` (after `accounts`):
```rust
    /// Markets any account holds a position or returned YES in: what the live resolver asks
    /// Gamma about (S3c). Sorted, so callers can compare snapshots cheaply.
    pub fn held_markets(&self) -> BTreeSet<String> {
        self.accounts
            .iter()
            .flat_map(|a| a.positions.keys().chain(a.returned.keys()))
            .map(|k| k.market_id.clone())
            .collect()
    }

    /// Does any account hold consumed liquidity at all? The engine calls `on_level` for every
    /// applied delta and `on_snapshot` for every applied snapshot: with nothing consumed (the
    /// common case between trades) both return before allocating a key or copying a book.
    fn any_consumed(&self) -> bool {
        self.consumed.iter().any(|c| !c.is_empty())
    }
```
In the same file, make `on_level` and `on_snapshot` start with the guard (the rest of each body is unchanged):
```rust
    pub fn on_level(&mut self, yes_token: &str, side: BookSide, price: Decimal, new_size: Decimal) {
        if !self.any_consumed() {
            return;
        }
        let key = crate::consumed::LevelKey {
            yes_token: yes_token.to_string(),
            side,
            price,
        };
        for c in &mut self.consumed {
            c.on_level(&key, new_size);
        }
    }

    pub fn on_snapshot(&mut self, yes_token: &str, book: &Book) {
        if !self.any_consumed() {
            return;
        }
        for c in &mut self.consumed {
            c.on_snapshot(yes_token, book);
        }
    }
```
In `crates/sim/src/consumed.rs`, reorder `ConsumedLiquidity::on_snapshot` so it collects this token's keys FIRST and returns before copying the book's levels into the two maps when there are none (today it builds both maps, for every account, on every snapshot):
```rust
    /// A full book for `yes_token` arrived: re-check every consumed level of that token.
    pub fn on_snapshot(&mut self, yes_token: &str, book: &Book) {
        let keys: Vec<LevelKey> = self
            .levels
            .keys()
            .filter(|k| k.yes_token == yes_token)
            .cloned()
            .collect();
        if keys.is_empty() {
            return; // nothing consumed on this token: don't copy its book
        }
        let side_map = |v: Vec<(arb_core::money::Price, arb_core::money::Shares)>| -> BTreeMap<Decimal, Decimal> {
            v.into_iter().map(|(p, s)| (p.get(), s.get())).collect()
        };
        let bids = side_map(book.bids_best_first());
        let asks = side_map(book.asks_best_first());
        for k in keys {
            let side = match k.side {
                BookSide::Bid => &bids,
                BookSide::Ask => &asks,
            };
            let size = side.get(&k.price).copied().unwrap_or(Decimal::ZERO);
            self.on_level(&k, size);
        }
    }
```
Both guards change no output (an empty ledger has nothing to clear); `tests/consumed.rs` and `tests/trader.rs` cover the clearing rules.
Run: `cargo test -p sim`
Expected: PASS. `lifecycle` has 9 tests (the 8 from S3b plus `held_markets_lists_positions_and_returned_yes`).

- [ ] **Step 4: Write the failing engine tests.** Create `bins/scanner/tests/engine_paper.rs`. It uses the shared helpers in `tests/common/mod.rs` (`engine`, `ctx`, `meta`, `snap`, `ask`, `seed`, `episodes`, `p`, `sh`, and the S3a-2 synthetic script `Synth`, `synth_engine`, `drive`). Every fill figure below comes from `decide`'s landed sizing: the raw feed must `Take` first (carry-over), then the account's consumed-filtered depth caps `q_target`.
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::{Book, Side};
use arb_core::stats::{annualized, days_to_end};
use common::*;
use pm_api::ws::MarketEvent;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::engine::{Engine, Out};
use scanner::universe::Universe;
use sim::account::EntryKind;
use sim::config::{AccountSpec, P1Params, PolicyKind, Scenario, SimConfig};
use sim::rows::SimOut;
use sim::trader::PaperTrader;
use std::collections::HashMap;
use std::sync::Arc;

/// One account; t_x = r_open + 100 ms; settle delay 2 s; one markout at +1 s.
fn sim_cfg(policy: PolicyKind) -> SimConfig {
    SimConfig {
        settle_delay_ms: 2000,
        markout_offsets_ms: vec![1000],
        mark_every_ms: 60_000,
        burst_window_ms: 10_000,
        min_order_shares: dec!(5),
        p1: P1Params { min_annualized: dec!(0.10), max_days_to_end: dec!(30) },
        scenarios: vec![Scenario {
            name: "t".into(),
            proc_ms: 0,
            net_ms: 0,
            match_ms: 100,
            honor_seconds_delay: true,
            burst: vec![],
        }],
        accounts: vec![AccountSpec {
            policy,
            scenario: "t".into(),
            bankroll: dec!(1000),
            per_trade: dec!(100),
            fee_bips: Decimal::ZERO,
        }],
    }
}

fn paper_engine(policy: PolicyKind) -> Engine {
    let mut e = engine();
    e.set_paper(PaperTrader::new(sim_cfg(policy)).unwrap());
    e
}

fn sims(outs: &[Out]) -> Vec<SimOut> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Paper(s) => Some((**s).clone()),
            _ => None,
        })
        .collect()
}

fn fills(outs: &[Out]) -> Vec<(String, Decimal)> {
    sims(outs)
        .into_iter()
        .filter_map(|s| match s {
            SimOut::Fill(f) => Some((f.leg.market_id, f.filled)),
            _ => None,
        })
        .collect()
}

fn all3(a: Decimal, b: Decimal, c: Decimal) -> Vec<(String, Decimal)> {
    vec![("e1-m0".to_string(), a), ("e1-m1".to_string(), b), ("e1-m2".to_string(), c)]
}

/// YES books: bids `bids` (size 100), asks 0.45 / 0.40 / 0.35 (ΣA = 1.20: never a YES basket).
fn books(e: &mut Engine, recv_ms: i64, ts: i64, bids: [&str; 3]) -> Vec<Out> {
    let asks = ["0.45", "0.40", "0.35"];
    let evs: Vec<MarketEvent> = (0..3)
        .map(|i| snap(&format!("e1-y{i}"), ts, &[(bids[i], "100")], &[(asks[i], "100")]))
        .collect();
    e.on_frame(0, recv_ms, &evs)
}

/// A frame that changes no book: only time passes.
fn quiet(e: &mut Engine, recv_ms: i64) -> Vec<Out> {
    e.on_frame(
        0,
        recv_ms,
        &[MarketEvent::LastTrade { token: "e1-y0".into(), server_ts_ms: recv_ms }],
    )
}

fn bid(token: &str, ts: i64, price: &str, size: &str) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side: Side::Bid,
        price: p(price),
        size: sh(size),
        best_bid: None,
        best_ask: None,
    }
}

/// ΣNO = 3 × 0.70 = 2.10 first (nothing), then NO asks 0.60/0.65/0.70 (Σ 1.95 < 2): a NO basket
/// opens at recv 2 100, so the account's execution is due at 2 200 (budget 100 / 1.95 → 51.2).
fn open_no_basket(e: &mut Engine) -> Vec<Out> {
    books(e, 1_100, 1_000, ["0.30", "0.30", "0.30"]);
    books(e, 2_100, 2_000, ["0.40", "0.35", "0.30"])
}

#[test]
fn a_no_basket_open_reaches_the_paper_trader_and_fills_at_t_x() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    let outs = open_no_basket(&mut e);
    let eps = episodes(&outs);
    assert_eq!((eps.len(), eps[0].kind.as_str()), (1, "no_basket"));
    let intents: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| if let SimOut::Intent(i) = s { Some(i) } else { None })
        .collect();
    assert_eq!(intents.len(), 1);
    assert_eq!((intents[0].account.as_str(), intents[0].episode_id.as_str()), ("P0-converter/t", "live-7-00000001"));
    assert_eq!((intents[0].exec_ms, intents[0].q_target), (Some(2_200), Some(dec!(51.2))));
    assert!(fills(&quiet(&mut e, 2_150)).is_empty(), "not due yet");
    assert_eq!(fills(&quiet(&mut e, 2_300)), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
    assert_eq!(e.paper_held_markets(), vec!["e1-m0", "e1-m1", "e1-m2"]);
}

#[test]
fn due_work_sees_the_book_before_the_frame_that_changes_it() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e); // execution due at recv 2 200
    // The frame received at 2 200 thins leg 2's YES bid (its NO ask 0.70) to 20. The execution
    // due at 2 200 must see the book as it stood BEFORE this frame (S3b call contract).
    let outs = e.on_frame(0, 2_200, &[bid("e1-y2", 2_150, "0.30", "20")]);
    assert_eq!(fills(&outs), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
}

#[test]
fn an_emptied_level_clears_consumed_liquidity() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    assert_eq!(fills(&quiet(&mut e, 2_300)), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
    // leg 0's YES bid 0.40 empties (the basket closes) and comes back with 60 (episode 2 opens)
    let outs = e.on_frame(0, 2_400, &[bid("e1-y0", 2_400, "0.40", "0")]);
    assert!(episodes(&outs).iter().any(|r| r.closed_server_ms.is_some()));
    let outs = e.on_frame(0, 2_500, &[bid("e1-y0", 2_500, "0.40", "60")]);
    assert!(episodes(&outs).iter().any(|r| r.episode_id == "live-7-00000002"));
    // on_level cleared leg 0's consumed 51.2, so it offers all 60; legs 1 and 2 still carry ours
    // (100 − 51.2 = 48.8), which caps the second basket at 48.8. Had the removal not reached
    // on_level, leg 0 would offer only 60 − 51.2 = 8.8 (a re-added 60 is no shrink below 48.8)
    // and every leg would fill 8.8.
    assert_eq!(fills(&quiet(&mut e, 2_700)), all3(dec!(48.8), dec!(48.8), dec!(48.8)));
}

/// A YES-bid delta whose message reports the server's final best bid (S3a deferred top-check).
fn bid_top(token: &str, ts: i64, price: &str, size: &str, best_bid: &str) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side: Side::Bid,
        price: p(price),
        size: sh(size),
        best_bid: Some(p(best_bid)),
        best_ask: None,
    }
}

#[test]
fn a_trimmed_phantom_level_clears_consumed_liquidity() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    assert_eq!(fills(&quiet(&mut e, 2_300)), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
    // The server says leg 0's best bid is 0.30: our 0.40 (the level we consumed) is a phantom,
    // a removal we never received. The check is deferred until the update is complete...
    let outs = e.on_frame(0, 2_400, &[bid_top("e1-y0", 2_400, "0.30", "100", "0.30")]);
    assert!(!outs.iter().any(|o| matches!(o, Out::Integrity(_))), "deferred, not yet checked");
    // ...which the next update on the token proves: S3a trims 0.40 (the basket closes), then
    // this frame's delta re-adds 0.40 with 60 (episode 2 opens at recv 2 500).
    let outs = e.on_frame(0, 2_500, &[bid_top("e1-y0", 2_500, "0.40", "60", "0.40")]);
    assert!(outs.iter().any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")));
    assert!(episodes(&outs).iter().any(|r| r.closed_server_ms.is_some()));
    assert!(episodes(&outs).iter().any(|r| r.episode_id == "live-7-00000002"));
    // The trim reached on_level as a removal: leg 0's consumed 51.2 cleared, exactly as for a
    // size-0 delta, and legs 1 and 2 (48.8 left each) cap the basket. Without the trim report,
    // leg 0 would offer only 60 − 51.2 = 8.8 and every leg would fill 8.8.
    assert_eq!(fills(&quiet(&mut e, 2_700)), all3(dec!(48.8), dec!(48.8), dec!(48.8)));
}

#[test]
fn an_episode_opened_by_a_rest_snapshot_executes_after_the_latest_frame() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    quiet(&mut e, 5_000); // the engine has now seen a frame received at 5 000
    // A REST snapshot (server ts 1 500, accepted: newer than our 1 000) lifts leg 0's YES bid to
    // 0.50: NO asks 0.50 / 0.70 / 0.70, Σ 1.90 < 2, so a NO basket opens from rest_snapshot.
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_500);
    let outs = e.rest_snapshot("e1-y0", b);
    assert_eq!(episodes(&outs).len(), 1);
    let intents: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| if let SimOut::Intent(i) = s { Some(i) } else { None })
        .collect();
    assert_eq!(intents.len(), 1);
    // rest_snapshot evaluates with recv = the REST book's server ts (1 500); the trader's clock
    // is never behind the latest frame the engine received (5 000), plus match_ms 100.
    assert_eq!(intents[0].exec_ms, Some(5_100));
}

/// P3 opens a YES basket (asks 0.30/0.35/0.30, size 10): execution due at 2 250, not yet run.
fn yes_basket_open() -> Engine {
    let mut e = paper_engine(PolicyKind::P3Naive);
    seed(&mut e);
    e.on_frame(
        0,
        2_150,
        &[ask("e1-y0", 2_000, "0.30", "10"), ask("e1-y0", 2_000, "0.40", "0")],
    );
    e
}

/// ...and holds it to resolution.
fn yes_basket_held() -> Engine {
    let mut e = yes_basket_open();
    assert_eq!(fills(&quiet(&mut e, 2_300)), all3(dec!(10), dec!(10), dec!(10)));
    e
}

fn settles(outs: &[Out]) -> Vec<(Decimal, Option<Decimal>, Decimal)> {
    sims(outs)
        .into_iter()
        .filter_map(|s| match s {
            SimOut::Ledger(l) if l.kind == EntryKind::Settle => Some((l.qty, l.price, l.cash_delta)),
            _ => None,
        })
        .collect()
}

#[test]
fn resolutions_settle_at_the_payout_vector_and_flag_non_binary_ones() {
    let mut e = yes_basket_held();
    assert_eq!(e.paper_held_markets(), vec!["e1-m0", "e1-m1", "e1-m2"]);
    let outs = e.on_resolution(9_000, "e1-m0", dec!(0.5), dec!(0.5));
    assert!(outs.iter().any(|o| matches!(o, Out::Integrity(i)
        if i.kind == "non_binary_payout" && i.event_id.as_deref() == Some("e1") && i.ts_ms == 9_000)));
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(0.5)), dec!(5))]);
    let outs = e.on_resolution(9_001, "e1-m1", dec!(1), dec!(0));
    assert!(!outs.iter().any(|o| matches!(o, Out::Integrity(_))), "a binary payout is not flagged");
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(1)), dec!(10))]);
    assert_eq!(e.paper_held_markets(), vec!["e1-m2"]);
}

#[test]
fn a_repeated_resolution_settles_once() {
    let mut e = yes_basket_held();
    assert_eq!(settles(&e.on_resolution(9_000, "e1-m1", dec!(1), dec!(0))).len(), 1);
    // the ws hint and the periodic poll both found it
    let again = e.on_resolution(9_500, "e1-m1", dec!(1), dec!(0));
    assert!(sims(&again).iter().all(|s| !matches!(s, SimOut::Ledger(_))), "{again:?}");
}

#[test]
fn a_resolution_first_runs_the_work_due_before_it() {
    // No frame arrives between the open and the resolution: the execution due at 2 250 must
    // still run (on_frame(2 260) first), so the settlement includes the shares it bought.
    let mut e = yes_basket_open();
    let outs = e.on_resolution(2_260, "e1-m0", dec!(1), dec!(0));
    assert_eq!(fills(&outs), all3(dec!(10), dec!(10), dec!(10)));
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(1)), dec!(10))]);
}

#[test]
fn lockup_figures_use_the_latest_live_end_date() {
    let mut m = meta("e1", 3);
    m.legs[0].end_date = Some("1970-01-06T00:00:00Z".into()); // 5 days
    m.legs[1].end_date = Some("1970-01-11T00:00:00Z".into()); // 10 days: the max
    let mut e = Engine::new(ctx());
    let conn: HashMap<String, usize> = [("e1".to_string(), 0)].into_iter().collect();
    let _ = e.apply_universe(Arc::new(Universe { taken_ms: 0, groups: vec![m], canary: vec![] }), &conn, 0);
    e.set_paper(PaperTrader::new(sim_cfg(PolicyKind::P0Converter)).unwrap());
    let outs = open_no_basket(&mut e);
    let r = &episodes(&outs)[0];
    let days = days_to_end(864_000_000, 2_000); // relative to the opening server ts
    assert_eq!(r.days_to_end, Some(days));
    assert_eq!(r.annualized_return, annualized(r.peak_pnl_net_usd / r.peak_cost_usd, days));
    assert_eq!(r.post_end, Some(false));
    // no end dates: unmeasured (NULL), never 0
    let mut plain = paper_engine(PolicyKind::P0Converter);
    let r = &episodes(&open_no_basket(&mut plain))[0];
    assert_eq!((r.days_to_end, r.annualized_return, r.post_end), (None, None, None));
}

#[test]
fn close_completes_conversions_marks_every_account_and_exposes_state() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    quiet(&mut e, 2_300); // executed; the conversion is due at 4 200
    let outs = e.paper_close(3_000);
    assert!(sims(&outs).iter().any(|s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Convert && l.t_ms == 3_000)));
    let marks: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| if let SimOut::Account(a) = s { Some(a) } else { None })
        .collect();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].equity, dec!(1000) + marks[0].realized + marks[0].unrealized, "equity identity");
    assert_eq!(e.paper_state().unwrap().accounts.len(), 1);

    let mut bare = engine();
    assert!(bare.paper_state().is_none());
    assert!(bare.paper_close(1).is_empty());
    assert!(bare.paper_held_markets().is_empty());
}

#[test]
fn close_first_runs_the_work_due_before_the_close_instant() {
    // The stop comes at 2 250 with no frame since the open: the execution due at 2 200 runs at
    // its own time first (S3b: close follows on_frame), then close completes its conversion.
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    let outs = e.paper_close(2_250);
    assert_eq!(fills(&outs), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
    assert!(sims(&outs).iter().any(|s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Convert && l.t_ms == 2_250)));
}

#[test]
fn a_ws_market_resolved_asks_for_a_resolver_poll() {
    let mut e = engine();
    let outs = e.on_frame(
        0,
        100,
        &[MarketEvent::Resolved { market: Some("0xabc".into()), server_ts_ms: Some(90) }],
    );
    assert!(outs.contains(&Out::ResolvedHint) && outs.contains(&Out::UniverseChanged));
}

#[test]
fn the_paper_trader_never_changes_the_scanners_own_outputs() {
    // The S3a-2 synthetic high-rate script: YES and NO baskets open and close, 1 in 50 deltas
    // carries a wrong server top (trims), conn 0 drops every 4 000 frames. With all 15 repo
    // accounts attached, every non-paper output must be byte-identical to the bare engine's.
    let mut s = Synth::new(200, 42);
    let frames = s.script(20_000, 50);
    let policies: SimConfig = toml::from_str(include_str!("../../../config/policies.toml")).unwrap();
    let run = |paper: bool| {
        let mut e = synth_engine(&s, true);
        if paper {
            e.set_paper(PaperTrader::new(policies.clone()).unwrap());
        }
        let mut outs: Vec<Out> = Vec::new();
        drive(&mut e, &frames, 4_000, &mut |o| outs.push(o));
        outs
    };
    let (bare, with) = (run(false), run(true));
    let scanner_only: Vec<String> = with
        .iter()
        .filter(|o| !matches!(o, Out::Paper(_)))
        .map(|o| format!("{o:?}"))
        .collect();
    let bare_txt: Vec<String> = bare.iter().map(|o| format!("{o:?}")).collect();
    assert_eq!(scanner_only.len(), bare_txt.len(), "output counts differ");
    for (i, (a, b)) in scanner_only.iter().zip(&bare_txt).enumerate() {
        assert_eq!(a, b, "scanner output {i} differs with the trader attached");
    }
    // the fixture exercises the trader: every open reaches the P3 accounts (they take every kind)
    assert!(
        with.iter().any(|o| matches!(o, Out::Paper(s) if matches!(**s, SimOut::Intent(_)))),
        "no paper intent: the trader never saw an open"
    );
}
```
Also add an ignored bench to `bins/scanner/tests/bench_engine.rs` (after the existing test; the file already imports `common::*`, `Out`, `Instant` and has `env_usize`):
```rust
#[test]
#[ignore = "profiling: run by hand (S3c plan, Task 4)"]
fn engine_us_per_frame_with_and_without_the_paper_trader() {
    let groups = env_usize("BENCH_GROUPS", 2_000);
    let n = env_usize("BENCH_FRAMES", 100_000);
    let mut s = Synth::new(groups, 7);
    let frames = s.script(n, 0);
    let policies: sim::config::SimConfig =
        toml::from_str(include_str!("../../../config/policies.toml")).unwrap();
    for paper in [false, true] {
        let mut e = synth_engine(&s, true);
        if paper {
            e.set_paper(sim::trader::PaperTrader::new(policies.clone()).unwrap());
        }
        let mut paper_rows = 0_usize;
        let t = Instant::now();
        drive(&mut e, &frames, 0, &mut |o| {
            if matches!(o, Out::Paper(_)) {
                paper_rows += 1;
            }
        });
        let ns = t.elapsed().as_nanos() / u128::try_from(frames.len()).unwrap();
        println!(
            "paper={paper:<5} groups={groups} frames={} engine={}.{:03} µs/frame paper_rows={paper_rows}",
            frames.len(),
            ns / 1_000,
            ns % 1_000,
        );
    }
}
```

- [ ] **Step 5: Run them to verify they fail.**
Run: `cargo test -p scanner --test engine_paper`
Expected: compile errors: no method `set_paper`, no variant `Out::Paper`, no method `on_resolution`.

- [ ] **Step 6: Implement in `bins/scanner/src/engine.rs`.**

Imports (add next to the existing ones):
```rust
use arb_core::fill::BasketQuote;
use arb_core::stats::{annualized, days_to_end, parse_rfc3339_ms};
use sim::policy::{LegInfo, OpenCtx};
use sim::rows::SimOut;
use sim::trader::{Books, PaperTrader, TraderState};
use sim::types::BookSide;
```
(`arb_core::fill::{BasketRule, BuyLeg, walk}`, `arb_core::book::Book` and `arb_core::money::{Price, Shares}` are already imported. Add `BasketQuote` to the `fill` line instead if you prefer.)

Add two variants at the end of `pub enum Out`:
```rust
    /// A paper-trader row (S3c), boxed like `Episode` to keep `Out` small.
    Paper(Box<SimOut>),
    /// A ws `market_resolved` arrived: the live resolver should ask Gamma soon.
    ResolvedHint,
```
Add one field to `struct OpenEp` (after `censor_left`):
```rust
    /// Latest endDate over the group's live legs at open (spec §5.5 lock-up estimate).
    end_ms: Option<i64>,
```
Add two fields to `pub struct Engine` (after S3a's `issue_throttle`), and `paper: None, last_recv_ms: i64::MIN,` to the struct literal in `Engine::new` (after `issue_throttle: HashMap::new(),`):
```rust
    /// The paper trader (S3c), fed by the S3b call contract. None = scanner only.
    paper: Option<PaperTrader>,
    /// The latest input instant the trader has been driven to: frame receive times, and the
    /// instants of `on_resolution` and `paper_close` (all inputs, so replay sees the same). The
    /// paper trader's open clock never runs behind it (see `open_ctx`).
    last_recv_ms: i64,
```
Add these free items below `fn leg_fee`:
```rust
/// The engine's YES books seen through `sim::trader::Books`. The fields are borrowed one by one,
/// so the trader (another field of the engine) can read them with no clone.
struct EngineBooks<'a> {
    token_ix: &'a HashMap<String, (usize, usize)>,
    groups: &'a [Group],
}

impl Books for EngineBooks<'_> {
    fn yes_book(&self, yes_token: &str) -> Option<&Book> {
        let &(g, l) = self.token_ix.get(yes_token)?;
        self.groups.get(g)?.legs.get(l).map(|leg| &leg.book)
    }
}

fn paper_outs(v: Vec<SimOut>, out: &mut Vec<Out>) {
    out.extend(v.into_iter().map(|o| Out::Paper(Box::new(o))));
}

/// Spec §5.5's secondary figure for a quote: (1 + Π/cost)^(365/max(days, 7)) − 1 (no cost → None).
fn lockup_annualized(q: &BasketQuote, days: Decimal) -> Option<Decimal> {
    if q.cost <= Decimal::ZERO {
        return None;
    }
    annualized(q.pnl.checked_div(q.cost)?, days)
}

/// The levels present before a `Book::trim_to_top` and absent from `now` (a trim only removes),
/// as (side, price): each is a removal the paper trader's consumed-liquidity ledger must see.
fn gone_levels(
    bids_before: &[(Price, Shares)],
    asks_before: &[(Price, Shares)],
    now: &Book,
) -> Vec<(BookSide, Decimal)> {
    let (bids_now, asks_now) = (now.bids_best_first(), now.asks_best_first());
    let mut gone = Vec::new();
    for (side, before, after) in [
        (BookSide::Bid, bids_before, &bids_now),
        (BookSide::Ask, asks_before, &asks_now),
    ] {
        for (p, _) in before {
            if !after.iter().any(|(q, _)| q == p) {
                gone.push((side, p.get()));
            }
        }
    }
    gone
}
```
In `on_frame`, directly after `let mut frame_tokens: Vec<String> = Vec::new();` (before `for ev in events`). This runs before ANY book change of the frame: its snapshots and deltas, and the deferred top-checks (with their phantom trims) that its deltas or its end settle.
```rust
        self.last_recv_ms = self.last_recv_ms.max(recv_ms);
        // S3b call contract: work due at or before this frame runs on the books as they were
        // just BEFORE it (the state at its due time).
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks { token_ix: &self.token_ix, groups: &self.groups };
            paper_outs(t.on_frame(recv_ms, &books), &mut out);
        }
```
In the `MarketEvent::Snapshot` arm, inside `if self.on_book(token, &mut touched, |b| b.apply_snapshot(bids, asks, ts)) { … }`, after S3a-2's `self.pending_remove(token);` line and the `frame_tokens` push block (`on_book` also returns true for canary tokens, hence the `token_ix` lookup):
```rust
                        if let (Some(t), Some(&(g, l))) =
                            (self.paper.as_mut(), self.token_ix.get(token.as_str()))
                        {
                            t.on_snapshot(token, &self.groups[g].legs[l].book);
                        }
```
In the `MarketEvent::Delta` arm, after S3a-2's `if found { self.pending_insert(token, PendingTop { conn, ts, bid: bb, ask: ba }); }` block (leave that block exactly as it is: the pending index helpers stay the only writers of `pending_tops`). `side`, `price` and `size` are the copied locals of `let (side, price, size) = (*side, *price, *size);`. Any `resolve_pending` this delta triggered ran BEFORE `on_book`, and reports its own trim through `check_book` below. The trader check comes first, so the bare engine pays nothing extra per delta:
```rust
                    if found
                        && let Some(t) = self.paper.as_mut()
                        && self.token_ix.contains_key(token.as_str())
                    {
                        t.on_level(token, BookSide::from_core(side), price.get(), size.get());
                    }
```
In `check_book`, inside `if let Some(mut detail) = mismatch { … }`, replace S3a's trim block
```rust
            let (sb, sa) = server.unwrap_or((None, None));
            let mut changed = false;
            let repaired = self.book_mut(token).is_some_and(|b| {
                changed = b.trim_to_top(sb, sa);
                let t = b.top();
                (sb.is_none() || sb == t.best_bid.map(|(x, _)| x))
                    && (sa.is_none() || sa == t.best_ask.map(|(x, _)| x))
            });
```
with (same trim, plus the removal report; canary tokens have no `group` and are skipped):
```rust
            let (sb, sa) = server.unwrap_or((None, None));
            // S3c: a trimmed level is a removal the paper trader's consumed-liquidity ledger must
            // see, exactly like a size-0 delta. Remember the levels before the trim: group legs
            // only, only when a trader is attached, and only on a top mismatch (one book's levels,
            // never a scan of the universe).
            let before = match (group, self.paper.is_some()) {
                (Some(_), true) => self
                    .book_of(token)
                    .map(|b| (b.bids_best_first(), b.asks_best_first())),
                _ => None,
            };
            let mut changed = false;
            let repaired = self.book_mut(token).is_some_and(|b| {
                changed = b.trim_to_top(sb, sa);
                let t = b.top();
                (sb.is_none() || sb == t.best_bid.map(|(x, _)| x))
                    && (sa.is_none() || sa == t.best_ask.map(|(x, _)| x))
            });
            if changed && let Some((bids, asks)) = before {
                let gone = self
                    .book_of(token)
                    .map(|b| gone_levels(&bids, &asks, b))
                    .unwrap_or_default();
                if let Some(t) = self.paper.as_mut() {
                    for (side, price) in gone {
                        t.on_level(token, side, price, Decimal::ZERO);
                    }
                }
            }
```
The rest of `check_book` (throttled `top_mismatch` row, `Resnapshot` when not repaired, `suspect`, re-evaluation when `changed`, crossed check) is unchanged. The removal is reported BEFORE the re-evaluation, so an episode that re-opens on the trimmed book is decided against a ledger that already forgot the phantom.
Replace the arm
```rust
                MarketEvent::Resolved { .. } | MarketEvent::NewMarket { .. } => {
                    out.push(Out::UniverseChanged)
                }
```
with:
```rust
                MarketEvent::Resolved { .. } => {
                    out.push(Out::UniverseChanged);
                    out.push(Out::ResolvedHint);
                }
                MarketEvent::NewMarket { .. } => out.push(Out::UniverseChanged),
```
Split `snap` so the open context can reuse its levels. Replace `fn snap` with:
```rust
    /// The bought outcome's ask levels per used leg: YES asks, or NO asks (= 1 − YES bids, L1).
    fn buy_legs(&self, at: At, s: &Signal) -> Vec<BuyLeg> {
        s.legs_used
            .iter()
            .map(|&i| {
                let leg = &self.groups[at.g].legs[i];
                BuyLeg {
                    levels: match at.det {
                        Det::Yes => leg.book.asks_best_first(),
                        Det::No => leg.book.no_asks_best_first(),
                    },
                    fee: leg.fee,
                }
            })
            .collect()
    }

    fn snap(&self, at: At, s: &Signal) -> Snap {
        let legs = self.buy_legs(at, s);
        let rule = match at.det {
            Det::Yes => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
            Det::No => BasketRule::NestedNo,
        };
        let budget100 = walk(&legs, rule, Some(Decimal::from(100))).pnl;
        let best = legs
            .iter()
            .map(|l| {
                l.levels
                    .first()
                    .map_or((None, None), |&(p, q)| (Some(p), Some(q)))
            })
            .collect();
        let leg_fees = legs
            .iter()
            .zip(&s.quote.legs)
            .map(|(l, f)| leg_fee(l, f.filled))
            .collect();
        Snap {
            signal: s.clone(),
            budget100,
            best,
            leg_fees,
            ts_ms: at.ts,
        }
    }

    /// The latest endDate over the group's live legs (spec §5.5: an estimate of lock-up).
    fn group_end_ms(&self, g: usize) -> Option<i64> {
        self.universe
            .groups
            .get(g)?
            .legs
            .iter()
            .filter(|l| l.status == Status::Live)
            .filter_map(|l| l.end_date.as_deref().and_then(parse_rfc3339_ms))
            .max()
    }

    /// Everything a policy may look at when this episode opens (S3b `OpenCtx`).
    fn open_ctx(&self, ep: &OpenEp, at: At, s: &Signal) -> OpenCtx {
        let meta = &self.universe.groups[at.g];
        let days = ep.end_ms.map(|e| days_to_end(e, at.ts));
        OpenCtx {
            episode_id: ep.episode_id.clone(),
            event_id: meta.event_id.clone(),
            // A frame (or a deferred check it settled) passes its own recv; `rest_snapshot`
            // passes the REST book's SERVER ts, which can be far behind. Never start the paper
            // clock before the latest instant the trader was driven to (rulings 15, 24).
            r_open_ms: at.recv_ms.max(self.last_recv_ms),
            signal: s.clone(),
            legs: meta
                .legs
                .iter()
                .map(|l| LegInfo {
                    market_id: l.market_id.clone(),
                    yes_token: l.yes_token.clone(),
                    fee: l.fee(),
                    seconds_delay: l.seconds_delay,
                })
                .collect(),
            open_levels: self.buy_legs(at, s).into_iter().map(|l| l.levels).collect(),
            days_to_end: days,
            annualized: days.and_then(|d| lockup_annualized(&s.quote, d)),
            pair_approved: false,
        }
    }
```
In `open_episode`:
- add `end_ms: self.group_end_ms(at.g),` to the `OpenEp { … }` literal (after `censor_left,`);
- directly before `self.open_by_key.insert(key.to_string(), id);`, add:
```rust
        if self.paper.is_some() {
            let ctx = self.open_ctx(&ep, at, s);
            if let Some(t) = self.paper.as_mut() {
                paper_outs(t.on_open(&ctx), out);
            }
        }
```
In `fn row`, after `let q = &s.quote;` add
```rust
        let days = ep.end_ms.map(|e| days_to_end(e, ep.opened_ms));
```
and replace the three `None` lines:
```rust
            days_to_end: days,
            annualized_return: days.and_then(|d| lockup_annualized(q, d)),
            post_end: days.map(|d| d < Decimal::ZERO),
```
In `rest_snapshot`, in the group-leg branch, directly after `self.groups[g].legs[l].book = book;` (i.e. only for an ACCEPTED snapshot: an older one returned early above; the canary branch is not a group leg):
```rust
            if let Some(t) = self.paper.as_mut() {
                t.on_snapshot(token, &self.groups[g].legs[l].book);
            }
```
Add these methods at the end of `impl Engine`:
```rust
    pub fn set_paper(&mut self, trader: PaperTrader) {
        self.paper = Some(trader);
    }

    pub fn paper(&self) -> Option<&PaperTrader> {
        self.paper.as_ref()
    }

    /// Sorted market ids the trader holds (the live resolver's question list).
    pub fn paper_held_markets(&self) -> Vec<String> {
        self.paper
            .as_ref()
            .map(|t| t.held_markets().into_iter().collect())
            .unwrap_or_default()
    }

    pub fn paper_state(&self) -> Option<TraderState> {
        self.paper.as_ref().map(PaperTrader::state)
    }

    /// Shutdown at `t_ms` (live: the Ctl::Shutdown instant; replay: the same line, or the last
    /// input). S3b contract: `close` must follow `on_frame(t_ms)`, so work due at or before the
    /// close instant first runs at its own due time; then pending conversions complete at
    /// `t_ms`, other pending work is dropped, and every account marks.
    pub fn paper_close(&mut self, t_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks { token_ix: &self.token_ix, groups: &self.groups };
            paper_outs(t.on_frame(t_ms, &books), &mut out);
            paper_outs(t.close(t_ms, &books), &mut out);
        }
        out
    }

    /// A market resolved with this payout vector (live: from Gamma via `Ctl::Resolved`; replay:
    /// the same line). A vector other than {1,0}/{0,1} is also a Q19 integrity row. The trader is
    /// first driven to `t_ms` (`on_frame`), so an execution already due settles with the shares
    /// it bought; then every account holding the market settles, and its `AccountRow` values the
    /// account's other open positions on the current books.
    pub fn on_resolution(
        &mut self,
        t_ms: i64,
        market_id: &str,
        payout_yes: Decimal,
        payout_no: Decimal,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        let binary = (payout_yes == Decimal::ONE && payout_no == Decimal::ZERO)
            || (payout_yes == Decimal::ZERO && payout_no == Decimal::ONE);
        if !binary {
            let event_id = self
                .universe
                .groups
                .iter()
                .find(|m| m.legs.iter().any(|l| l.market_id == market_id))
                .map(|m| m.event_id.clone());
            out.push(self.integrity(
                t_ms,
                Issue {
                    kind: "non_binary_payout",
                    event_id,
                    detail: Some(format!(
                        "market {market_id}: payout yes {payout_yes} no {payout_no}"
                    )),
                    ..Issue::default()
                },
            ));
        }
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks { token_ix: &self.token_ix, groups: &self.groups };
            paper_outs(t.on_frame(t_ms, &books), &mut out);
            paper_outs(
                t.on_resolution(t_ms, market_id, payout_yes, payout_no, &books),
                &mut out,
            );
        }
        out
    }
```
`Io::route` in `run.rs` matches `Out` exhaustively, so keep the build green now: add this arm after `Out::Lag(ms) => self.metrics.push_lag(ms),` (before S3a's `Out::Drift { .. }` arm; Task 7 replaces it with the real routing). `replay_cmd` and the `Drift` arm's `mark_suspect` loop use `_ =>`/`if let` and need no change:
```rust
                Out::Paper(_) | Out::ResolvedHint => {} // routed in S3c Task 7
```

- [ ] **Step 7: Run the tests to verify they pass.**
Run: `cargo test -p scanner --test engine_paper`
Expected: PASS (13). If a fill quantity differs, recompute it from the fixture before touching the code:
- The per-trade budget is 100 / 1.95 → 51.2 (sizes must keep price × size at ≤ 2 dp for limits 0.60/0.65/0.70; `settle()` finds that fixpoint).
- A second episode is sized by the raw feed first (it must `Take`), then capped by the account's consumed-filtered depth: 48.8 when leg 0's consumption cleared (legs 1 and 2 still carry 51.2 of their 100), 8.8 when it did not (leg 0 re-added at 60, 51.2 still consumed).
- In `a_trimmed_phantom_level_clears_consumed_liquidity`, first confirm the top_mismatch row and the episode-2 open (S3a's path); only then look at the fills: 8.8 means the trim never reached `on_level`.
- In `the_paper_trader_never_changes_the_scanners_own_outputs`, a differing scanner output means a hook mutated engine state (a book, the tracker, a pending check): the trader may only read.
Run: `cargo test -p scanner && cargo test -p sim`
Expected: PASS. The existing engine, engine_checks and replay tests are unchanged: without a paper trader the engine emits exactly what it did before, plus `ResolvedHint` on `market_resolved` (no existing test covers that event). `last_recv_ms` only feeds the paper clock, and the `before` copy in `check_book` is skipped when no trader is attached.
Run (by hand, debug build, the ratio matters, not the absolute): `cargo test -p scanner --test bench_engine -- --ignored --nocapture engine_us_per_frame_with_and_without_the_paper_trader`
Expected: two lines, `paper=false` and `paper=true`. Note both in the task report. If `paper=true` costs more than 2× `paper=false` µs/frame, stop and profile before continuing (the budget is the S3a-2 bench line: ~232 µs/frame on the VPS, shared with `parse_frame`).

- [ ] **Step 8: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p scanner -p sim --all-targets -- -D warnings`
Expected: clean.
```bash
git add crates/sim/src/trader.rs crates/sim/src/consumed.rs crates/sim/tests/lifecycle.rs bins/scanner/src/engine.rs bins/scanner/src/run.rs bins/scanner/tests/engine_paper.rs bins/scanner/tests/bench_engine.rs
git commit -m "scanner/engine: paper trader on the engine's own books (call contract, open context, lock-up, resolutions)"
```

---

### Task 5: capture `Ctl::Resolved` + replay with the paper trader (golden test) + streaming `replay_cmd`

**Files:**
- Modify: `bins/scanner/src/capture.rs`, `bins/scanner/src/replay.rs`, `bins/scanner/src/run.rs` (`replay_cmd` only), `bins/scanner/src/main.rs`
- Test: `bins/scanner/tests/replay.rs`

**Interfaces:**
- Consumes: `Engine::{set_paper, on_resolution, paper_close}` (Task 4); `paper::{push_sim, account_rows, with_policies, Policies}` (Task 3); `Batch::rows` (Task 1). The current replay (`aba9ce5`): `replay_dir_with(dir: &Path, ctx: EngineCtx, mut sink: impl FnMut(Out)) -> anyhow::Result<ReplayStats>` walks the hour files in order and hands every engine output to `sink`. Its arms: `Ctl::Down` → `engine.disconnect`, `Ctl::Universe` → `apply_universe`, `Ctl::RestSnapshot` → `rest_snapshot`, `Ctl::Suspect` → `mark_suspect(&event_id, until_ms, ms)`, `Ctl::Shutdown` → `for o in engine.close_all(ms) { sink(o); } shut_down = true;`, and after the loop the crash close `if !shut_down { for o in engine.close_all(last_ms) { sink(o); } }`. `replay_dir(dir, ctx) -> ReplayOut { outs, frames, parse_errors }` collects through `replay_dir_with` (tests only). `run::replay_cmd(cfg, cfg_hash, dir, dry_run)` passes a sink. The test file's `ctx()` already sets S3a's `issue_throttle_ms: 5_000`.
- Produces:
  - `capture::Ctl::Resolved { market_id: String, payout_yes: String, payout_no: String }`. The decimals are exact strings, like `RestSnapshot`, serialized as `{"kind":"resolved",…}`.
  - `replay::replay_dir_paper_with(dir: &Path, ctx: EngineCtx, paper: Option<PaperTrader>, sink: impl FnMut(Out)) -> anyhow::Result<ReplayStats>`: the old `replay_dir_with` body plus the trader. `replay_dir_with(dir, ctx, sink)` keeps its signature and calls `replay_dir_paper_with(dir, ctx, None, sink)`; `replay_dir` is unchanged.
  - `run::replay_cmd(cfg, cfg_hash, dir, dry_run, policies: Option<&Policies>)`: the replay runs on a blocking thread and its rows reach Postgres in chunks of `REPLAY_CHUNK` (2 000) over a bounded channel (ruling 21).

- [ ] **Step 1: Write the failing golden tests.** In `bins/scanner/tests/replay.rs`, change the replay import to
```rust
use scanner::replay::{replay_dir, replay_dir_paper_with, replay_dir_with};
```
add these imports
```rust
use scanner::paper::push_sim;
use sim::config::SimConfig;
use sim::trader::PaperTrader;
use std::path::Path;
use store::Batch;
```
and append:
```rust
const POLICIES: &str = include_str!("../../../config/policies.toml");
const T0: i64 = 1_790_121_600_000; // a whole minute: the first mark boundary is T0 + 60 000

/// Book snapshots for y0..y2 with one bid and one ask level each, size 100.
fn paper_frame(ts: i64, bids: [&str; 3], asks: [&str; 3]) -> String {
    let items: Vec<String> = (0..3)
        .map(|i| {
            format!(
                r#"{{"event_type":"book","asset_id":"y{i}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{{"price":"{}","size":"100"}}],"asks":[{{"price":"{}","size":"100"}}]}}"#,
                bids[i], asks[i]
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// A NO basket opens at T0 + 1 100, every scenario executes by T0 + 2 000 and converts by
/// T0 + 4 000; m0 resolves 50-50 at T0 + 5 000; the capture ends with a Shutdown line at
/// T0 + 6 000, or (a crash) without one.
fn write_paper_capture(dir: &Path, clean_stop: bool) {
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let ufile = universe_file_name(u.taken_ms, 1);
    let asks = ["0.45", "0.40", "0.35"]; // ΣA = 1.20: never a YES basket
    let open = ["0.40", "0.35", "0.30"]; // NO asks 0.60/0.65/0.70: Σ 1.95 < 2
    let mut msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord { universe: u.clone(), conn_of, file: ufile.clone() })),
        line(T0, None, Some(Ctl::Universe { taken_ms: u.taken_ms, file: ufile })),
        line(T0 + 100, Some(paper_frame(T0, ["0.30", "0.30", "0.30"], asks)), None), // ΣNO 2.10: nothing
        line(T0 + 1_100, Some(paper_frame(T0 + 1_000, open, asks)), None), // the NO basket opens
        line(T0 + 2_000, Some(paper_frame(T0 + 1_900, open, asks)), None), // every scenario's t_x has passed
        line(T0 + 4_000, Some(paper_frame(T0 + 3_900, open, asks)), None), // conversions due (+2 s)
        line(
            T0 + 5_000,
            None,
            Some(Ctl::Resolved { market_id: "m0".into(), payout_yes: "0.5".into(), payout_no: "0.5".into() }),
        ),
    ];
    if clean_stop {
        msgs.push(line(T0 + 6_000, None, Some(Ctl::Shutdown)));
    }
    let mut w = Writer::new(dir.to_path_buf());
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

fn replay_paper(dir: &Path, policies: &SimConfig) -> Vec<Out> {
    let mut outs = Vec::new();
    replay_dir_paper_with(dir, ctx(), Some(PaperTrader::new(policies.clone()).unwrap()), |o| outs.push(o))
        .unwrap();
    outs
}

fn paper_rows(outs: &[Out]) -> Batch {
    let mut b = Batch::default();
    for o in outs {
        if let Out::Paper(s) = o {
            push_sim(1, (**s).clone(), &mut b);
        }
    }
    b
}

#[test]
fn replay_paper_trades_deterministically_and_keeps_the_equity_identity() {
    let dir = tmp("paper");
    write_paper_capture(&dir, true);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let (a, b) = (replay_paper(&dir, &policies), replay_paper(&dir, &policies));
    assert_eq!(a, b, "same capture, same outputs, paper rows included");
    let (ra, rb) = (paper_rows(&a), paper_rows(&b));
    assert_eq!(ra, rb, "same paper ledger rows");
    assert!(!ra.paper_ledger.is_empty());

    assert!(a.iter().any(|o| matches!(o, Out::Episode(w) if w.row.kind == "no_basket")));
    assert!(a.iter().any(|o| matches!(o, Out::Integrity(i) if i.kind == "non_binary_payout" && i.ts_ms == T0 + 5_000)));
    let p0 = "P0-converter/colo-idle";
    assert!(ra.paper_execs.iter().any(|x| x.account_id == p0 && x.q_eff == dec!(51.2)));
    assert!(ra.paper_ledger.iter().any(|l| l.account_id == p0 && l.kind == "convert"));

    // Ctl::Shutdown closes the trader: one final mark per account, and the identity holds on each
    let finals: Vec<_> = ra.paper_marks.iter().filter(|m| m.t_ms == T0 + 6_000).collect();
    assert_eq!(finals.len(), policies.accounts.len());
    for m in &finals {
        assert_eq!(m.equity, dec!(1000) + m.realized + m.unrealized, "{}", m.account_id);
    }
    let p0_final = finals.iter().find(|m| m.account_id == p0).unwrap();
    assert_eq!(p0_final.realized, dec!(2.56), "51.2 sets × (2 − 1.95)");

    // the bare replay (no trader) emits exactly the scanner outputs of the paper replay
    let mut bare = Vec::new();
    replay_dir_with(&dir, ctx(), |o| bare.push(o)).unwrap();
    let scanner_only: Vec<Out> = a.iter().filter(|o| !matches!(o, Out::Paper(_))).cloned().collect();
    assert_eq!(scanner_only, bare);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_capture_without_a_shutdown_line_closes_the_trader_at_its_last_instant() {
    let dir = tmp("paper-crash");
    write_paper_capture(&dir, false);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let rows = paper_rows(&replay_paper(&dir, &policies));
    // the last captured instant is the Resolved line (T0 + 5 000): every account marks there
    let finals = rows.paper_marks.iter().filter(|m| m.t_ms == T0 + 5_000).count();
    assert!(finals >= policies.accounts.len(), "{finals} marks at the crash close");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_resolved_line_round_trips_through_the_capture_format() {
    let l = Line {
        recv_ts_us: 5,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::Resolved { market_id: "m0".into(), payout_yes: "0.5".into(), payout_no: "0.5".into() }),
    };
    let json = serde_json::to_string(&l).unwrap();
    assert!(json.contains(r#""kind":"resolved""#), "{json}");
    assert_eq!(serde_json::from_str::<Line>(&json).unwrap(), l);
}
```
(In the crash test the count is `>=`: an account that held m0 would also write its resolution `AccountRow` at T0 + 5 000. After the conversions nobody holds m0 in this fixture, so it is exactly 15 today; the table keeps the first row per `(run, account, t_ms)` anyway, ruling 4.)

- [ ] **Step 2: Run them to verify they fail.**
Run: `cargo test -p scanner --test replay`
Expected: compile errors: no variant `Ctl::Resolved`, no function `replay_dir_paper_with`.

- [ ] **Step 3: Implement the control line.** In `bins/scanner/src/capture.rs`, add a variant to `enum Ctl` before `Shutdown`:
```rust
    /// A market resolution the live resolver found (S3c), with the recv instant the engine got.
    /// Payouts are exact decimal strings, like `RestSnapshot` levels.
    Resolved {
        market_id: String,
        payout_yes: String,
        payout_no: String,
    },
```
Extend the module doc's list of control lines to "(down, universe, rest_snapshot, suspect, resolved, shutdown)".

- [ ] **Step 4: Implement replay.** In `bins/scanner/src/replay.rs`:
- add the imports `use rust_decimal::Decimal;` and `use sim::trader::PaperTrader;`;
- rename `pub fn replay_dir_with(` (with its doc comment) to:
```rust
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
```
- directly after `let mut engine = Engine::new(ctx);` add
```rust
    if let Some(t) = paper {
        engine.set_paper(t);
    }
```
- add an arm before `Some(Ctl::Shutdown) => {`:
```rust
                Some(Ctl::Resolved {
                    market_id,
                    payout_yes,
                    payout_no,
                }) => {
                    let y = Decimal::from_str_exact(&payout_yes)
                        .with_context(|| format!("bad payout_yes {payout_yes:?}"))?;
                    let n = Decimal::from_str_exact(&payout_no)
                        .with_context(|| format!("bad payout_no {payout_no:?}"))?;
                    for o in engine.on_resolution(ms, &market_id, y, n) {
                        sink(o);
                    }
                }
```
- replace the `Some(Ctl::Shutdown)` arm with (live does the same, in the same order, on the same instant):
```rust
                Some(Ctl::Shutdown) => {
                    for o in engine.close_all(ms) {
                        sink(o);
                    }
                    for o in engine.paper_close(ms) {
                        sink(o);
                    }
                    shut_down = true;
                }
```
- replace the crash-close block after the loop with:
```rust
    if !shut_down {
        // crash: close at the last captured instant
        for o in engine.close_all(last_ms) {
            sink(o);
        }
        for o in engine.paper_close(last_ms) {
            sink(o);
        }
    }
```
- add the unchanged-signature wrapper directly below `replay_dir_paper_with` (above `replay_dir`, which keeps calling `replay_dir_with`):
```rust
/// Replay without a paper trader (the S2 scanner outputs only).
pub fn replay_dir_with(
    dir: &Path,
    ctx: EngineCtx,
    sink: impl FnMut(Out),
) -> anyhow::Result<ReplayStats> {
    replay_dir_paper_with(dir, ctx, None, sink)
}
```

- [ ] **Step 5: Write paper rows from `scanner replay`, streamed.** In `bins/scanner/src/run.rs`:
- change `use crate::replay::replay_dir_with;` to `use crate::replay::replay_dir_paper_with;`;
- add `use crate::paper::{self, Policies};` and `use sim::trader::PaperTrader;`;
- add below `END_RUN_TIMEOUT`:
```rust
/// Rows per `scanner replay` write, and per message on its bounded replay → writer channel.
const REPLAY_CHUNK: usize = 2_000;
```
- replace `replay_cmd` with:
```rust
pub async fn replay_cmd(
    cfg: &Config,
    cfg_hash: &str,
    dir: &Path,
    dry_run: bool,
    policies: Option<&Policies>,
) -> anyhow::Result<()> {
    // Every replay starts every account fresh with the SAME policies file (spec §11 replay).
    let trader = || -> anyhow::Result<Option<PaperTrader>> {
        policies
            .map(|p| PaperTrader::new(p.sim.clone()))
            .transpose()
            .context("paper trader")
    };
    if dry_run {
        let mut eps: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut paper_rows = 0_u64;
        let stats = replay_dir_paper_with(
            dir,
            engine_ctx(cfg, cfg_hash, 0, "replay"),
            trader()?,
            |o| match o {
                Out::Episode(w) => {
                    eps.insert(w.row.episode_id.clone());
                }
                Out::Paper(_) => paper_rows += 1,
                _ => {}
            },
        )?;
        println!(
            "frames {} parse_errors {} episodes {} paper_rows {paper_rows}",
            stats.frames,
            stats.parse_errors,
            eps.len()
        );
        return Ok(());
    }
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    let store = Store::connect(&url, 2).await?;
    let run_id = store.start_run(&new_run(cfg_hash, "replay")).await?;
    if let Some(p) = policies {
        // Pre-registration first: every other paper row references its account row.
        store
            .write_batch(&Batch {
                paper_accounts: paper::account_rows(run_id, p),
                ..Batch::default()
            })
            .await?;
    }
    // The replay runs on a blocking thread and hands full chunks to this task, which writes
    // them as they come: at most a few chunks are ever in memory, whatever the capture's length
    // (aba9ce5). A failed write closes the channel; the thread then finishes without sending.
    let (tx, mut rx) = mpsc::channel::<Batch>(4);
    let (dir_buf, ctx, t) = (
        dir.to_path_buf(),
        engine_ctx(cfg, cfg_hash, run_id, "replay"),
        trader()?,
    );
    let replay = tokio::task::spawn_blocking(move || -> anyhow::Result<crate::replay::ReplayStats> {
        let mut batch = Batch::default();
        let stats = replay_dir_paper_with(&dir_buf, ctx, t, |o| {
            match o {
                // DEVIATION: unbox, same reason as in Io::route.
                Out::Episode(w) => batch.episodes.push(*w),
                Out::Integrity(i) => batch.integrity.push(i),
                Out::Paper(s) => paper::push_sim(run_id, *s, &mut batch),
                _ => return,
            }
            if batch.rows() >= REPLAY_CHUNK {
                let _ = tx.blocking_send(std::mem::take(&mut batch));
            }
        })?;
        let _ = tx.blocking_send(batch);
        Ok(stats)
    });
    let mut write_err = None;
    while let Some(b) = rx.recv().await {
        if let Err(e) = store.write_batch(&b).await {
            write_err = Some(e);
            break;
        }
    }
    drop(rx); // never leave the replay thread blocked on a full channel
    let stats = replay.await.context("replay thread")??;
    if let Some(e) = write_err {
        return Err(anyhow::Error::new(e).context("write replay rows"));
    }
    store
        .end_run(
            run_id,
            now_ms(),
            &format!("replay of {} ({} frames)", dir.display(), stats.frames),
        )
        .await?;
    println!(
        "replay run {run_id}: {} frames, {} parse errors",
        stats.frames, stats.parse_errors
    );
    Ok(())
}
```
(`mpsc` is already imported from `tokio::sync`; `Batch` from `store`. Several versions of one episode can share a chunk: `write_batch` keeps the last, as before.)
In `bins/scanner/src/main.rs`, replace the `Cmd::Replay` arm with:
```rust
        Cmd::Replay { dir, dry_run } => {
            let (policies, hash) = scanner::paper::with_policies(&cfg.paper, &hash)?;
            scanner::run::replay_cmd(&cfg, &hash, &dir, dry_run, policies.as_ref()).await
        }
```

- [ ] **Step 6: Run the tests to verify they pass.**
Run: `cargo test -p scanner --test replay`
Expected: PASS. That includes the three new tests, `replay_reproduces_episodes_and_censoring_byte_for_byte` (still `replay_dir`) and `replay_dir_with_streams_the_same_outputs_as_replay_dir` (still `replay_dir_with`).
Run: `cargo test -p scanner`
Expected: PASS.

- [ ] **Step 7: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p scanner --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/capture.rs bins/scanner/src/replay.rs bins/scanner/src/run.rs bins/scanner/src/main.rs bins/scanner/tests/replay.rs
git commit -m "scanner/replay: paper trader in replay (fresh accounts, Ctl::Resolved, close at shutdown), streamed replay writes + golden test"
```

---

### Task 6: writer: paper rows through the batching writer (never evicted), quarantine via `into_singles`

**Files:**
- Modify: `bins/scanner/src/writer.rs`
- Test: `mod tests` in `writer.rs`

**Interfaces:**
- Consumes: `Batch::{rows, take_paper, extend_paper, into_singles}` (Task 1). S3a's `Pending`, which keeps `VecDeque` queues and the same public API.
- Produces: `WriterMsg::Paper(Box<Batch>)`. Only the batch's `paper_*` fields are used. `Pending` keeps paper rows in arrival order, the cap never evicts them, `take()` returns them in the batch, and `restore()` puts the older ones first. The public API is otherwise unchanged.

- [ ] **Step 1: Write the failing tests.** Add to `mod tests` in `writer.rs`:
```rust
    fn paper_ledger(seq: i64) -> WriterMsg {
        WriterMsg::Paper(Box::new(Batch {
            paper_ledger: vec![store::PaperLedgerRow {
                run_id: 1,
                account_id: "a".into(),
                seq,
                t_ms: seq,
                kind: "mark".into(),
                episode_id: None,
                market_id: None,
                yes_token: None,
                outcome: None,
                qty: 0.into(),
                price: None,
                cash_delta: 0.into(),
                fees: 0.into(),
                realized_delta: 0.into(),
                cash_after: 1000.into(),
                equity: Some(1000.into()),
            }],
            ..Batch::default()
        }))
    }

    #[test]
    fn paper_rows_are_never_evicted_by_the_cap() {
        let mut p = Pending::new(3);
        for s in 1..=5 {
            p.push(paper_ledger(s));
        }
        for t in 0..3 {
            p.push(integ(t));
        }
        assert_eq!(p.rows(), 5, "only the integrity rows could go");
        assert_eq!(p.dropped, 3);
        let b = p.take();
        assert_eq!(b.paper_ledger.iter().map(|l| l.seq).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert!(b.integrity.is_empty());
        assert_eq!(p.rows(), 0);
    }

    #[test]
    fn restore_keeps_older_paper_rows_first() {
        let mut p = Pending::new(100);
        p.push(paper_ledger(1));
        let failed = p.take();
        p.push(paper_ledger(2)); // arrived while the write was failing
        p.restore(failed);
        assert_eq!(p.take().paper_ledger.iter().map(|l| l.seq).collect::<Vec<_>>(), vec![1, 2]);
    }
```

- [ ] **Step 2: Run them to verify they fail.**
Run: `cargo test -p scanner --lib writer`
Expected: compile error, no variant `WriterMsg::Paper`.

- [ ] **Step 3: Implement.** In `bins/scanner/src/writer.rs`:
- extend the module doc's last sentence to "… (episodes and paper rows never are, except a single row Postgres itself rejects)";
- add a variant to `WriterMsg` before `Flush`:
```rust
    /// Paper-trader rows (S3c). Only the batch's `paper_*` fields are used.
    Paper(Box<Batch>),
```
- replace S3a's `struct Pending` and its whole `impl` with:
```rust
/// Rows waiting for the next flush. Episodes are deduplicated by id (latest version wins). The
/// other rows are FIFO queues, so the cap evicts the oldest in O(1). Paper rows are kept in
/// arrival order and, like episodes, are never evicted: a gap in `paper_ledger` would break the
/// equity identity.
#[derive(Debug, Default)]
pub struct Pending {
    episodes: Vec<EpisodeWrite>,
    observations: VecDeque<ObservationRow>,
    integrity: VecDeque<IntegrityRow>,
    health: VecDeque<HealthRow>,
    /// Only the `paper_*` fields are ever filled.
    paper: Batch,
    ep_index: HashMap<String, usize>,
    max_rows: usize,
    pub dropped: u64,
}

impl Pending {
    pub fn new(max_rows: usize) -> Pending {
        Pending {
            max_rows,
            ..Pending::default()
        }
    }

    pub fn rows(&self) -> usize {
        self.episodes.len()
            + self.observations.len()
            + self.integrity.len()
            + self.health.len()
            + self.paper.paper_rows()
    }

    fn upsert_episode(&mut self, w: EpisodeWrite, overwrite: bool) {
        match self.ep_index.get(&w.row.episode_id) {
            Some(&i) => {
                if overwrite {
                    self.episodes[i] = w;
                }
            }
            None => {
                self.ep_index
                    .insert(w.row.episode_id.clone(), self.episodes.len());
                self.episodes.push(w);
            }
        }
    }

    pub fn push(&mut self, m: WriterMsg) {
        match m {
            WriterMsg::Episode(w) => self.upsert_episode(w, true),
            WriterMsg::Observation(o) => self.observations.push_back(o),
            WriterMsg::Integrity(i) => self.integrity.push_back(i),
            WriterMsg::Health(h) => self.health.push_back(h),
            WriterMsg::Paper(b) => self.paper.extend_paper(*b),
            WriterMsg::Flush(_) => {}
        }
        self.enforce_cap();
    }

    /// Over the cap, drop the oldest health, then integrity, then observation rows (episodes and
    /// paper rows never), counting each.
    fn enforce_cap(&mut self) {
        while self.rows() > self.max_rows {
            let evicted = self.health.pop_front().is_some()
                || self.integrity.pop_front().is_some()
                || self.observations.pop_front().is_some();
            if !evicted {
                break; // only episodes and paper rows left: never dropped
            }
            self.dropped += 1;
        }
    }

    pub fn take(&mut self) -> Batch {
        self.ep_index.clear();
        let mut b = std::mem::take(&mut self.paper);
        b.episodes = std::mem::take(&mut self.episodes);
        b.observations = std::mem::take(&mut self.observations).into();
        b.integrity = std::mem::take(&mut self.integrity).into();
        b.health = std::mem::take(&mut self.health).into();
        b
    }

    /// Put back a batch whose write failed. It is OLDER than anything pushed since, so newer
    /// episode versions already here win, and its other rows go first.
    pub fn restore(&mut self, mut older: Batch) {
        let mut newer = self.take();
        let mut paper = older.take_paper();
        paper.extend_paper(newer.take_paper());
        for w in older.episodes {
            self.upsert_episode(w, false);
        }
        for w in newer.episodes {
            self.upsert_episode(w, true);
        }
        self.observations = older
            .observations
            .into_iter()
            .chain(newer.observations)
            .collect();
        self.integrity = older.integrity.into_iter().chain(newer.integrity).collect();
        self.health = older.health.into_iter().chain(newer.health).collect();
        self.paper = paper;
        self.enforce_cap();
    }
}
```
- replace `quarantine` with:
```rust
/// Write each row of a data-rejected batch on its own; returns how many rows still failed
/// (dropped). `Batch::into_singles` puts episodes before their observations and paper accounts
/// before the paper rows that reference them.
async fn quarantine(store: &Store, b: Batch) -> u64 {
    let mut dropped = 0;
    for single in b.into_singles() {
        // Same 10 s bound as the main flush: a stalled row must not freeze the writer (it's dropped).
        let r = match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&single)).await {
            Ok(r) => r.map_err(|e| e.to_string()),
            Err(_) => Err("timed out".to_string()),
        };
        if let Err(e) = r {
            if let Some(w) = single.episodes.first() {
                eprintln!("writer: dropped episode {}: {e}", w.row.episode_id);
            } else if let Some(l) = single.paper_ledger.first() {
                eprintln!("writer: dropped paper ledger row {}#{}: {e}", l.account_id, l.seq);
            }
            dropped += 1;
        }
    }
    dropped
}
```
- in `flush`, replace the row sum in the transient-failure log with `b.rows()`:
```rust
        Ok(Err(e)) => {
            eprintln!("writer: batch failed ({} rows): {e}", b.rows());
            pending.restore(b);
            *fails += 1;
            false
        }
```

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib writer`
Expected: PASS. That includes the two new tests and S3a's `evicting_a_large_backlog_is_fast_and_keeps_the_newest_rows`, `keeps_only_the_latest_version_of_an_episode`, `restore_after_a_failed_write_lets_newer_versions_win` and `over_the_cap_drops_non_episode_rows_first`.

- [ ] **Step 5: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p scanner --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/writer.rs
git commit -m "scanner/writer: paper rows ride the batching writer, never evicted; quarantine row by row via into_singles"
```

---

### Task 7: live wiring (restore, routing, resolver, state saves, shutdown) + the Phase 3 exit runbook

**Files:**
- Modify: `bins/scanner/src/run.rs`, `bins/scanner/src/main.rs`
- Create: `docs/superpowers/runbooks/s3-paper.md`
- Test: `mod tests` in `run.rs`

**Interfaces:**
- Consumes: everything above. From S3a (current `run.rs`): `SHUTDOWN_BUDGET` (20 s) + `END_RUN_TIMEOUT` (5 s) < compose's 30 s; `by_deadline(deadline, step, fut)` (a ready step still completes after the deadline, a stuck one is skipped); `wait_thread`; and `run_live`'s stop block, in this order: `let deadline = …; drop(plan_tx); supervisor_task.abort(); refresher_task.abort(); let t = now_ms(); io.ctl(0, Ctl::Shutdown, t); close_all(t) → by_deadline("close episodes"); Flush → by_deadline("writer flush"); abort health/rechecks/resnapshots/samplers; drop(io), drop(capture_tx), drop(writer_tx); by_deadline("writer exit", writer_task); wait_thread(capture)`. `Io` today has exactly `writer, capture, recheck, resnap, refresh, metrics, run_id, l1_window_ms, suspect_window_ms`. From S3a-2: the engine loop records every frame's `parse_frame` / `engine.on_frame` time in `FrameTiming` (`timing.record(parse_us, on_frame_us, now_ms())`) and logs one `engine: frames=… on_frame_us p50=… p99=… max=…` line a minute; the trader's per-frame hook runs inside `engine.on_frame`, so that line measures it too (the runbook reads it). Before the loop: `let mut tick = tokio::time::interval(Duration::from_secs(1)); tick.set_missed_tick_behavior(…Delay); let shutdown = shutdown_signal(); tokio::pin!(shutdown); let mut timing = FrameTiming::new(now_ms());`. `Engine::paper_close(t)` drives the trader to `t` (`on_frame`) before `close` (Task 4).
- Produces:
  - `run::run(cfg: Config, cfg_hash: String, policies: Option<Policies>)`.
  - `Io` gains `resolve_hint: Arc<Notify>`. `Io::route` sends `Out::Paper` to the writer as `WriterMsg::Paper` (rows stamped with the run id) and turns `Out::ResolvedHint` into a resolver wake-up.

- [ ] **Step 1: Write the failing test.** Add to `mod tests` in `run.rs`:
```rust
    #[tokio::test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    async fn paper_rows_go_to_the_writer_and_resolution_hints_wake_the_resolver() {
        use rust_decimal::Decimal;
        let (writer, mut wrx) = mpsc::channel(16);
        let (capture, _crx) = mpsc::channel(16);
        let (recheck, _rrx) = mpsc::channel(16);
        let (resnap, _srx) = mpsc::channel(16);
        let io = Io {
            writer,
            capture,
            recheck,
            resnap,
            refresh: Arc::new(tokio::sync::Notify::new()),
            metrics: Arc::new(Metrics::default()),
            run_id: 9,
            l1_window_ms: 1,
            suspect_window_ms: 1,
            resolve_hint: Arc::new(tokio::sync::Notify::new()),
        };
        let mut engine = Engine::new(EngineCtx {
            run_id: 9,
            source: "live".into(),
            scanner_version: "t".into(),
            config_hash: "c".into(),
            gap_ms: 30_000,
            stale_lag_ms: 500,
            suspect_window_ms: 60_000,
            l1_suspect_window_ms: 3_600_000,
            drift_settle_ms: 2_000,
            issue_throttle_ms: 5_000,
        });
        let mark = sim::rows::AccountRow {
            account: "P0-converter/sp-idle".into(),
            t_ms: 60_000,
            cash: Decimal::from(1000),
            equity: Decimal::from(1000),
            realized: Decimal::ZERO,
            unrealized: Decimal::ZERO,
            fees_paid: Decimal::ZERO,
            locked: Decimal::ZERO,
            max_drawdown: Decimal::ZERO,
            n_positions: 0,
        };
        io.route(
            &mut engine,
            vec![Out::Paper(Box::new(sim::rows::SimOut::Account(mark))), Out::ResolvedHint],
        )
        .await
        .unwrap();
        match wrx.recv().await {
            Some(WriterMsg::Paper(b)) => {
                assert_eq!(b.paper_marks.len(), 1);
                assert_eq!((b.paper_marks[0].run_id, b.paper_marks[0].t_ms), (9, 60_000));
            }
            other => panic!("expected paper rows, got {other:?}"),
        }
        tokio::time::timeout(Duration::from_secs(1), io.resolve_hint.notified())
            .await
            .expect("the hint is stored as a permit for the resolver");
    }
```

- [ ] **Step 2: Run it to verify it fails.**
Run: `cargo test -p scanner --lib run::tests::paper_rows_go_to_the_writer`
Expected: compile error, no field `resolve_hint` on `Io`.

- [ ] **Step 3: Implement the routing.** In `bins/scanner/src/run.rs`:
- add the import `use pm_api::gamma::Resolution;` (`crate::paper::{self, Policies}` came in Task 5);
- add a field to `struct Io`, after `suspect_window_ms`:
```rust
    /// Wakes the resolver soon after a ws `market_resolved` (S3c).
    resolve_hint: Arc<tokio::sync::Notify>,
```
- in `Io::route`, replace Task 4's placeholder arm `Out::Paper(_) | Out::ResolvedHint => {}` with:
```rust
                Out::Paper(o) => {
                    let mut b = Batch::default();
                    paper::push_sim(self.run_id, *o, &mut b);
                    self.writer
                        .send(WriterMsg::Paper(Box::new(b)))
                        .await
                        .context("writer gone")?
                }
                Out::ResolvedHint => self.resolve_hint.notify_one(),
```

- [ ] **Step 4: Wire the live run.** Still in `run.rs`:
- `run`: change the signature to `pub async fn run(cfg: Config, cfg_hash: String, policies: Option<Policies>) -> anyhow::Result<()>` and the call to `let result = run_live(&cfg, &cfg_hash, policies.as_ref(), &store, run_id).await;`.
- `run_live`: change the signature to
```rust
async fn run_live(
    cfg: &Config,
    cfg_hash: &str,
    policies: Option<&Policies>,
    store: &Store,
    run_id: i64,
) -> anyhow::Result<()> {
```
- directly after `let capture_thread = capture::spawn(cfg.capture.clone(), capture_rx, metrics.clone());`, add:
```rust
    // Paper trader (S3c): restore the live accounts from paper_state, register this run's
    // accounts (pre-registration, spec §11) before any other paper row, start the resolver.
    // Replay never touches paper_state.
    let trader = match policies {
        Some(p) => {
            let saved = store.load_paper_state().await?;
            let t = paper::restore_trader(p, &saved)?;
            let accounts = Batch {
                paper_accounts: paper::account_rows(run_id, p),
                ..Batch::default()
            };
            writer_tx
                .send(WriterMsg::Paper(Box::new(accounts)))
                .await
                .context("writer gone")?;
            Some(t)
        }
        None => None,
    };
    let (held_tx, held_rx) = watch::channel(Vec::<String>::new());
    let (resolved_tx, mut resolved_rx) = mpsc::channel::<Resolution>(1024);
    let resolve_hint = Arc::new(tokio::sync::Notify::new());
    let resolver_task = if trader.is_some() {
        let gamma = Arc::new(GammaClient::new()?);
        Some(tokio::spawn(paper::run_resolver(
            move |ids: Vec<String>| {
                let g = gamma.clone();
                async move { g.resolutions(&ids).await }
            },
            held_rx,
            resolve_hint.clone(),
            Duration::from_secs(cfg.paper.resolver_every_secs),
            paper::RESOLVE_HINT_DELAY,
            resolved_tx,
        )))
    } else {
        drop(resolved_tx); // resolved_rx then yields None, which disables its select arm
        None
    };
```
- in the `let io = Io { … }` literal, add `resolve_hint: resolve_hint.clone(),`;
- directly after `let mut engine = Engine::new(engine_ctx(cfg, cfg_hash, run_id, "live"));`, add:
```rust
    if let Some(t) = trader {
        engine.set_paper(t);
    }
```
- directly after `tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);`, add:
```rust
    let mut save_tick =
        tokio::time::interval(Duration::from_secs(cfg.paper.state_save_every_secs));
    save_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    save_tick.tick().await; // the first tick is immediate; a just-restored state needs no save
```
- in the `tokio::select!` loop, add two arms directly before `_ = tick.tick() => {`, and replace the `tick` arm as shown:
```rust
            Some(r) = resolved_rx.recv() => {
                // One instant for the capture line and the engine, so replay settles identically.
                let t = now_ms();
                io.ctl(0, Ctl::Resolved {
                    market_id: r.market_id.clone(),
                    payout_yes: r.payout_yes.to_string(),
                    payout_no: r.payout_no.to_string(),
                }, t);
                let outs = engine.on_resolution(t, &r.market_id, r.payout_yes, r.payout_no);
                if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
            }
            _ = save_tick.tick() => {
                // Queued behind every row already routed, so it lands in the same or a later batch.
                if let Some(st) = engine.paper_state() {
                    match paper::state_batch(&st, now_ms()) {
                        Ok(b) => {
                            if writer_tx.send(WriterMsg::Paper(Box::new(b))).await.is_err() {
                                break Err(anyhow::anyhow!("writer gone"));
                            }
                        }
                        Err(e) => eprintln!("paper: state not saved: {e:#}"),
                    }
                }
            }
            _ = tick.tick() => {
                let outs = engine.tick(now_ms());
                if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
                let held = engine.paper_held_markets();
                let _ = held_tx.send_if_modified(|h| {
                    if *h == held {
                        false
                    } else {
                        *h = held;
                        true
                    }
                });
            }
```
- in S3a's shutdown block (all of it runs under the ONE `deadline = now + SHUTDOWN_BUDGET`):
  - directly after `refresher_task.abort();` (step 1, "stop taking new inputs"), add:
```rust
    // No resolution may arrive after the Shutdown line below (replay must see what live saw).
    if let Some(r) = &resolver_task {
        r.abort();
    }
```
  - directly after `let _ = by_deadline(deadline, "close episodes", io.route(&mut engine, outs)).await;` and BEFORE the `WriterMsg::Flush` step, add:
```rust
    // Paper (S3c): drive the trader to `t`, the Ctl::Shutdown instant (work already due runs at
    // its own time), complete pending conversions and mark every account at `t` (replay does the
    // same after close_all on that line), then queue the account state. Both go BEFORE the flush,
    // so the final flush commits them, inside the same deadline. `paper_close` is pure CPU (15
    // accounts); the route and the send are the only waits, bounded like every other step.
    let outs = engine.paper_close(t);
    let _ = by_deadline(deadline, "close paper", io.route(&mut engine, outs)).await;
    if let Some(st) = engine.paper_state() {
        match paper::state_batch(&st, t) {
            Ok(b) => {
                let _ = by_deadline(
                    deadline,
                    "save paper state",
                    writer_tx.send(WriterMsg::Paper(Box::new(b))),
                )
                .await;
            }
            Err(e) => eprintln!("paper: state not saved at shutdown: {e:#}"),
        }
    }
```
  - update S3a's order comment above `let deadline` to: `// 1. stop taking new shards/universes/resolutions; 2. close every open episode, then the paper trader (final marks + state); 3. flush the writer; 4. …` (the rest unchanged).
  If the deadline is already spent when the paper steps run, `by_deadline` still completes a READY step (a `send` into a writer channel with room is ready at once), so the state normally still reaches the writer; a stuck writer skips it with a log line, and the next start restores the previous periodic save (at most `state_save_every_secs` old). That loss is bounded and logged, never silent (ruling 17).
In `bins/scanner/src/main.rs`, replace the `Cmd::Run` arm with:
```rust
        Cmd::Run => {
            let (policies, hash) = scanner::paper::with_policies(&cfg.paper, &hash)?;
            scanner::run::run(cfg, hash, policies).await
        }
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib run::tests`
Expected: PASS: the new test plus S3a's `the_whole_stop_fits_the_compose_grace_period`, `shutdown_steps_never_outlive_the_deadline`, `waiting_for_a_busy_thread_stops_at_the_deadline`, `breaker_trips_at_max_starts`, `warm_start_only_when_fresh`, `an_oversize_unit_becomes_an_integrity_row_naming_it` and S3a-2's `percentiles_us_sorts_then_indexes_with_integer_math`.

- [ ] **Step 6: Write the Phase 3 exit runbook.** Create `docs/superpowers/runbooks/s3-paper.md`:
````markdown
# S3 paper trading (Phase 3 exit)

Spec §19 exit: **deterministic replay; equity identity holds.** Everything here is read-only: no
ports, no credentials beyond the database role passwords in `.env`. It runs on the PC (PowerShell)
or on the VPS (bash): the `docker compose … psql` commands are the same.
Project disk budget: **30 GB**. Run `sh scripts/disk-budget.sh` (VPS) or
`powershell -ExecutionPolicy Bypass -File scripts\disk-budget.ps1` (PC) before step 1 and after
step 3.

## 0. Image and schema

The image must contain S3c, including `/etc/cute-trading/policies.toml`. Rebuild it once (it costs
disk: prune the build cache afterwards), then apply migration 0004:
```bash
docker compose --profile core build migrate
docker builder prune -f
docker compose --profile core run --rm migrate
docker compose exec postgres psql -U ct_owner -d cute_trading -c "select count(*) from paper_state"
```
Note the policies hash; every paper account row must carry it:
```bash
sha256sum config/policies.toml                                            # VPS
Get-FileHash config\policies.toml -Algorithm SHA256 | % { $_.Hash.ToLower() }   # PC
```
Write down `POLICIES_SHA`.

## 1. Deterministic replay: the same prefix twice

Use a replay prefix copied as in `s2c-soak.md` §2 / `s2c-soak-vps.md` §3 (two complete hours plus
every `universe-*` file, under `/data/replay-prefix`). Replay it TWICE:
```bash
docker compose run --rm --no-deps scanner replay /data/replay-prefix
docker compose run --rm --no-deps scanner replay /data/replay-prefix
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id, config_hash from runs where mode='replay' order by run_id desc limit 2"
```
Write down the two run ids as `R1` < `R2`. Both `config_hash` values must end in `+<POLICIES_SHA>`.

Episode ids embed the run id (`replay-<run>-<n>`), so strip that prefix before comparing:
```sql
with a as (select account_id, seq, t_ms, kind, regexp_replace(coalesce(episode_id, ''), '^replay-[0-9]+-', '') ep,
                  market_id, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity
           from paper_ledger where run_id = <R1>),
     b as (select account_id, seq, t_ms, kind, regexp_replace(coalesce(episode_id, ''), '^replay-[0-9]+-', '') ep,
                  market_id, outcome, qty, price, cash_delta, fees, realized_delta, cash_after, equity
           from paper_ledger where run_id = <R2>)
select (select count(*) from a) a_rows, (select count(*) from b) b_rows,
       (select count(*) from (select * from a except select * from b) x) only_a,
       (select count(*) from (select * from b except select * from a) x) only_b;
```
Pass: `only_a = only_b = 0` and `a_rows = b_rows > 0` (every account marks at least once, at the
replay's close). The same check on marks:
```sql
select (select count(*) from (select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R1>
                              except select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R2>) x) only_a,
       (select count(*) from (select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R2>
                              except select account_id, t_ms, cash, equity, realized, unrealized from paper_marks where run_id = <R1>) x) only_b;
```
Pass: `0, 0`.

## 2. The equity identity

`equity = bankroll + realized + unrealized`, written out from the mark's own cash:
`cash + (equity − cash) = bankroll + realized + unrealized`.
```sql
select count(*) marks,
       count(*) filter (where m.cash + (m.equity - m.cash) <> a.bankroll + m.realized + m.unrealized) broken
from paper_marks m join paper_accounts a using (run_id, account_id)
where m.run_id in (<R1>, <R2>);
select distinct policies_sha256 from paper_accounts where run_id in (<R1>, <R2>);
```
Pass: `broken = 0` with `marks > 0`, and the only hash is `POLICIES_SHA`.

Context for the write-up (not a pass/fail):
```sql
select a.account_id, count(*) filter (where i.skip_reason is null) taken, count(i.*) decided,
       max(m.realized) filter (where m.t_ms = (select max(t_ms) from paper_marks where run_id = <R1>)) realized_at_end
from paper_accounts a left join paper_intents i using (run_id, account_id)
     left join paper_marks m using (run_id, account_id)
where a.run_id = <R1> group by 1 order by 1;
select kind, count(*) from integrity_events where run_id = <R1> and kind = 'non_binary_payout' group by 1;
```
Then free the prefix: `docker run --rm -v cute-trading_data:/data alpine rm -rf /data/replay-prefix`.

## 3. Live check (short)

```bash
docker compose --profile core up -d
docker compose exec postgres psql -U ct_owner -d cute_trading -tAc "select run_id from runs where mode='live' order by run_id desc limit 1"
```
Write down `LIVE`. After about 3 minutes:
```sql
select count(*) from paper_accounts where run_id = <LIVE>;                              -- = accounts in policies.toml (15)
select account_id, count(*) marks, max(t_ms) from paper_marks where run_id = <LIVE> group by 1 order by 1;   -- ≥ 2 each
select count(*) marks, count(*) filter (where m.cash + (m.equity - m.cash) <> a.bankroll + m.realized + m.unrealized) broken
from paper_marks m join paper_accounts a using (run_id, account_id) where m.run_id = <LIVE>;                  -- broken = 0
select count(*), max(updated_ms) from paper_state;                                       -- 15, within the last ~60 s
select kind, count(*) from paper_ledger where run_id = <LIVE> group by 1;
```
Engine cost with the trader attached (S3a-2 timing line, one per minute; it includes the trader's
per-frame hooks):
```bash
docker compose logs --since 5m scanner | grep "engine: frames"
```
Pass: `on_frame_us` p50 and p99 in the same range as the last pre-S3c soak's lines (write both
down), and `select coalesce(sum(drops), 0) from health where run_id = <LIVE>` is 0 (the
shard→engine channel never filled). A p99 that doubled means a paper hook is too expensive: stop
and profile before the soak.
Restart and check that the accounts carry over (the restored cash equals the last saved cash):
```bash
docker compose --profile core restart scanner
```
```sql
select run_id, account_id, t_ms, cash, realized from paper_marks
where account_id = 'P0-converter/sp-idle' order by t_ms desc limit 3;
```
Pass: the new run's first mark has the same `cash` and `realized` as the old run's final (close)
mark, unless that account traded in between. Then `docker compose --profile core stop scanner`
(or keep it running for the soak).

## Results
(fill in: date, POLICIES_SHA, R1/R2, ledger rows, only_a/only_b, marks/broken, LIVE, on_frame_us p50/p99 before and after, restart carry-over)
````

- [ ] **Step 7: Full verification.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all PASS (the `live` tests and the two `bench_engine` tests show as ignored).
Run: `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: PASS, no `SKIPPED`.
Run: `grep -rnE "private_key|mnemonic|sign\(|post_order|create_order" bins/scanner/src crates/sim/src || echo "no order or signing code"`
Expected: `no order or signing code`.

- [ ] **Step 8: Commit.**
```bash
git add bins/scanner/src/run.rs bins/scanner/src/main.rs docs/superpowers/runbooks/s3-paper.md
git commit -m "scanner/run: live paper trading (restore, resolver, state saves, bounded shutdown) + Phase 3 runbook"
```

---

## Rulings recorded in this plan (owner: check)

1. **`Out::Paper(Box<SimOut>)`**, not `Out::Paper(SimOut)`. It is boxed like `Out::Episode`: `SimOut` is ~250 bytes and would trip clippy's `large_enum_variant` under `-D warnings`.
2. **The paper history is append-only.** `scanner_rw` gets `SELECT, INSERT` on the seven history tables, and `UPDATE` only on `paper_state`. The brief said SELECT/INSERT/UPDATE, but `ON CONFLICT DO NOTHING` needs no UPDATE, and a writable ledger could be rewritten. There are no sequences: every key is natural.
3. **`paper_state.state` is `TEXT`**, the exact JSON serde wrote (decimals as strings), not `jsonb`. The upsert also refuses an older `updated_ms`, so a restored failed batch can't roll the state back.
4. **Natural keys:**
   - fills: (run, account, episode, market, outcome, side). Each leg has one buy and at most one unwind per episode.
   - markouts: (…, market, outcome, offset_ms).
   - marks: (run, account, t_ms). A resolution row at exactly a mark's ms keeps the first.

   Every paper row has an FK to `paper_accounts(run_id, account_id)`, but none to `episodes`, so a quarantined episode can't cascade.
5. **Payout acceptance is stricter than "closed, in [0,1], sums to 1".** A fractional vector also needs `umaResolutionStatus = "resolved"`, because a closed market still in dispute shows its trading prices, which sum to 1 as well. A binary 1/0 on a closed market is accepted without the status. The field name `umaResolutionStatus` is **unverified**: run the ignored live test and look at one resolved NegRisk market before relying on 50-50 settlement.
6. **Resolver results are filtered to the ids requested** (the keyset lesson). A ws `market_resolved` triggers a poll 30 s later (`RESOLVE_HINT_DELAY`), because the ws `market` field is a condition id and Gamma's payout lags the event.
7. **`non_binary_payout` is written for every non-binary resolution received**, held or not. It is stamped with the receive time. `event_id` comes from the current universe, or is NULL if the market has left it.
8. **`config_hash` becomes `"<scanner sha256>+<policies sha256>"`** when paper trading is on: it is transparent and needs no new column. Every `paper_accounts` row also carries the policies sha256 alone.
9. **State is saved on a `state_save_every_secs` timer** (60 s, the mark cadence), not hooked to mark rows. The save goes through the writer after every row already routed, so it commits with or after them. A saved account whose spec differs from the policies file starts fresh (logged). A corrupt row is a start-up error, never a silent reset.
10. **Lock-up:**
    - `days_to_end` = the max endDate over the group's **live** legs, fixed at open, relative to the opening **server** ts.
    - `annualized_return` in the episode row uses the **peak** quote; `OpenCtx.annualized` uses the open quote.
    - `post_end` = `days < 0`.
    - No parseable end date → all three NULL.
11. **Paper rows are never evicted by the writer cap** (like episodes). Their volume is bounded: 15 accounts × 2 rows per minute of marks, plus trades.
12. **Held markets include returned YES** from conversions, since their settlement books a windfall.
13. **`on_level` is called only for group-leg tokens**, not canaries: after each applied delta, and (S3a refresh) for each level a phantom trim removed. A refresh that blanks a disconnected group's books does not call `on_snapshot`: the next ws snapshot for that token does. `on_frame` runs for every parsed frame, before its events and before any deferred top-check the frame settles, whatever the frame contains.

Added when this plan was refreshed against the landed S3a (2026-09-25):

14. **A phantom-level trim is reported to the trader as removals.** S3a's `check_book` trims levels the server's top disproves (`Book::trim_to_top`). Each removed level becomes `on_level(token, side, price, 0)`, reported right after the trim and before the re-evaluation. The removed set is a before/after diff (`gone_levels`), not a re-statement of `trim_to_top`'s rule, so arb-core is untouched and a future change to the trim rule stays covered. Without this, a consumed phantom would stay consumed forever (a re-added size is not a shrink), and every later fill on that leg would come up short.
15. **The paper open clock is `max(recv of the opening input, latest frame recv)`.** `rest_snapshot` evaluates with `recv_ms = server ts` (S3a kept this for episode rows). Used as the trader's `r_open_ms`, that would schedule t_x in the past and execute at the next frame with no latency. `Engine.last_recv_ms` is an input (frames), so replay reproduces it. Episode rows keep S3a's `open_lag_ms`/`stale` semantics unchanged.
16. **Policies do not see `engine_suspect`.** S3b's `OpenCtx` has no such field, and a group can turn suspect after open (held-closed re-flags, late `mark_suspect`). A trade on an episode opened over a phantom is still decided, but it fills at t_x against the book as it then is (trimmed), never against the phantom. Analysis separates the two by joining `paper_intents` to `episodes.engine_suspect` on `episode_id`. Changing the decision would need an S3b `OpenCtx` field: out of scope here.
17. **Shutdown order under S3a's one deadline:** the resolver is aborted with the refresher (no resolution after the `Shutdown` line). The paper close and the state save are queued after `close_all` and before the writer flush, so the final flush commits them within `SHUTDOWN_BUDGET` (20 s; plus `END_RUN_TIMEOUT` it stays under compose's 30 s). If the writer is stuck, the save is skipped with a log line, and the next start restores the last periodic save (≤ 60 s old).
18. **Integrity and REST classes:**
    - `non_binary_payout` bypasses S3a's integrity throttle, which covers only `top_mismatch`/`crossed_book`. Resolutions are rare and each is its own measurement.
    - The resolver's Gamma `/markets` calls do not go through S3a's CLOB `Limiter`: a different host, at most ⌈held/50⌉ requests per 10 min or per hint.
    - Migration 0004 spells out every grant under 0003's regime: SELECT+INSERT on the seven history tables, SELECT+INSERT+UPDATE on `paper_state`, no DELETE anywhere, and no sequences.

Added when this plan was refreshed against the landed S3b, S3a-2 and aba9ce5 (2026-09-25, second refresh at `8a16757`):

19. **Policy verdicts before consumption (S3b carry-over, done first).** `decide_for` runs `decide` on the raw open context; only a raw `Take` meets the consumed filter (`Skip(Consumed)` when the filtered edge ≤ 0 or its depth is 0, else the filtered `decide`). `paper_intents.skip_reason = 'consumed'` therefore always means "the raw feed would have traded". Cost: up to two `decide` calls (each a bisection walk) per account per episode open, never per frame.
20. **A resolution and the close drive the trader to their instant first.** `Engine::on_resolution(t, …)` runs the trader's `on_frame(t)` before `on_resolution(t, …, books)`, and `Engine::paper_close(t)` runs `on_frame(t)` before `close(t)` (the landed S3b contract for `close`). Work already due (an execution, an unwind, a conversion, a markout, a minute mark) happens at its own due time, never skipped by a stop or settled out of order. Replay does the same on the same captured instants, so outputs stay identical. Each such call also counts once in the trader's burst-rate window, identically live and in replay. This amends rulings 13 and 17.
21. **Replay streams paper rows in bounded chunks.** `replay_dir_paper_with(dir, ctx, paper, sink)` is the replay core; `replay_dir_with(dir, ctx, sink)` keeps its signature as the no-trader wrapper, and `replay_dir` stays the collecting test helper. `scanner replay` writes the accounts first, then runs the replay on `spawn_blocking`. Its sink maps each episode, integrity and paper output straight into a `Batch` and sends every 2 000 rows (`REPLAY_CHUNK`) over a bounded channel (capacity 4) to the async side, which commits each chunk. Memory is bounded whatever the capture's length. This replaces the collect-then-write that aba9ce5 had narrowed to episode and integrity rows. A failed write closes the channel; the replay finishes without sending, and the error is returned without `end_run`, as before.
22. **The consumed-clearing fixtures re-add the level at 60, not 100.** The landed `decide_for` sizes a later episode by the account's filtered depth. With a re-added 100, a cleared leg 0 (100 available) and an uncleared one (100 − 51.2 = 48.8 available) would both leave legs 1 and 2 (48.8) as the cap, so the tests could not tell them apart. At 60 the cleared case fills 48.8 on every leg and the uncleared case 8.8.
23. **Per-frame cost of the trader (soak budget).** The trader never adds O(universe) or O(pending top-checks) work per frame:
    - `on_frame`: `FrameRate::push` is amortized O(1); `earliest_due` scans only the trader's own queues (intents, unwinds, conversions, markouts), once per due item, which is O(due × queued). The queues hold in-flight trades only, a few per account.
    - `EngineBooks::yes_book` is one `token_ix` hash lookup plus two index reads.
    - `on_level` per applied delta: returns at once while no account has consumed anything; otherwise one key allocation and one ordered-map lookup per account (15). The engine checks `self.paper` before the `token_ix` lookup, so the bare engine pays nothing.
    - `on_snapshot` per applied snapshot: the same early-out, and `ConsumedLiquidity::on_snapshot` no longer copies the book's levels into two maps unless that account consumed something on that token.
    - The trim report copies one book's levels, and only on a top mismatch of a group leg with a trader attached.
    - `open_ctx` (legs and open levels) runs once per episode open; `paper_held_markets` once per 1 s tick (O(positions)); `paper_state` clones 15 accounts once per `state_save_every_secs`.

    Pinned by `the_paper_trader_never_changes_the_scanners_own_outputs` (S3a-2's synthetic high-rate script with trims and drops), measured by the ignored bench `engine_us_per_frame_with_and_without_the_paper_trader`, and checked live through the S3a-2 `engine:` timing line (runbook §3).
24. **The paper open clock follows every instant the trader is driven to.** `Engine.last_recv_ms` now takes the max over frame receive times AND the instants of `on_resolution` and `paper_close`, so an open from a REST snapshot after a resolution never schedules t_x before the trader's own horizon. All are inputs, so replay reproduces it. This amends ruling 15.
25. **Row-shape notes from the landed `sim`:**
    - An unwind that cannot trade (below `min_order_shares`, or no book) is still a `paper_fills` row: side `sell`, `filled` 0, `vwap` NULL, a measured zero. The fills key (ruling 4) still holds: at most one buy and one unwind per (episode, leg).
    - A mark, a resolution's `AccountRow` and the close's mark can share one millisecond (e.g. a close exactly on a minute boundary). `paper_marks` keeps the first per `(run, account, t_ms)` (ruling 4); `paper_ledger` keeps every entry, since each has its own `seq`.
    - `PaperTrader::restore` already starts a changed account fresh and releases reservations; `paper::restore_trader` keeps its filter only for the log line (amends ruling 9's wording, not its effect).

## Self-review

- **Spec coverage:**
  - §5.5 lock-up: Task 4.
  - §5.6 execution model: S3b, driven here by the S3b call contract (Task 4), payout vector (Tasks 2 and 4).
  - §8.2 paper trader beside the engine; §8.3 same core for live and replay: Tasks 4, 5 and 7.
  - §10 tables, roles, NUMERIC, zero vs NULL: Task 1.
  - §11 pre-registration hash (Tasks 3 and 7), replay with the same policies (Task 5).
  - §15 sim determinism and the equity identity through the scanner (Task 5 golden), store role test (Task 1), route table (Task 2).
  - §16 route list (Task 2).
  - Q19 flag (Tasks 1 and 4).
  - §19 exit runbook (Task 7).
- **Types across tasks:**
  - `Batch` paper fields and helpers (T1) → `paper.rs` (T3), writer (T6), replay_cmd (T5).
  - `replay_dir_paper_with` (T5) → `replay_cmd` (T5); `replay_dir_with` keeps its signature.
  - `Resolution` (T2) → resolver (T3), run (T7).
  - `push_sim`, `account_rows`, `state_batch`, `restore_trader`, `with_policies`, `run_resolver`, `RESOLVE_HINT_DELAY` (T3) → T5 and T7.
  - `Out::{Paper, ResolvedHint}`, `Engine::{set_paper, paper_close, paper_state, paper_held_markets, on_resolution}` (T4) → T5 and T7.
  - `WriterMsg::Paper` (T6) → T7.
  - `Ctl::Resolved` (T5) → T7.
- **First refresh, S3a (checked against `088c43d`, still true at `8a16757`):**
  - `EngineCtx` literals carry `issue_throttle_ms` (Task 7 test; the scanner test helpers already do).
  - Paper hooks cover every book mutation: ws snapshot, delta, trim (in `check_book`), accepted REST snapshot.
  - `on_frame` runs before any of them.
  - Migration 0004 comes after 0003, with explicit grants; its kind list extends 0003's.
  - The writer patch matches the `VecDeque` `Pending`.
  - The shutdown additions sit inside the `SHUTDOWN_BUDGET` block.
- **Second refresh (checked against `8a16757`):**
  - The carry-over matches the landed `decide_for` (`takes_kind` → `filtered_ctx` → `Consumed` short-circuit → `decide`); its import change keeps clippy clean.
  - Every `on_resolution` call site passes books: `Engine::on_resolution` (Task 4) builds `EngineBooks`, and the lifecycle test passes `&books`. The replay (Task 5) and live (Task 7) paths reach the trader only through `Engine::on_resolution`.
  - Every `close` call follows `on_frame(t)`: `Engine::paper_close` is its only caller.
  - No new code touches `pending_tops`; the Delta and Snapshot anchors name S3a-2's `pending_insert` / `pending_remove` lines, which stay as they are.
  - The replay task builds on `replay_dir_with` (now a wrapper of `replay_dir_paper_with`), keeps `replay_dir_with_streams_the_same_outputs_as_replay_dir` green, and never collects a whole replay.
  - Shutdown: resolver abort → `Ctl::Shutdown` line → `close_all(t)` → `paper_close(t)` (on_frame + close) → state save → flush, all under the one `SHUTDOWN_BUDGET` deadline; replay's `Ctl::Shutdown` arm runs `close_all` then `paper_close` on the same instant.
  - Test counts: `lifecycle` 9, `engine_paper` 13, `replay` +3, `paper::tests` 6, `config::tests` 3.
- **Build stays green between tasks:**
  - The carry-over only touches `sim`.
  - T1 patches S3a's `Pending::take` literal.
  - T4 adds a placeholder `Io::route` arm that T7 replaces.
  - T5 changes `replay_cmd`'s signature together with `main.rs`; T7 does the same for `run`.
