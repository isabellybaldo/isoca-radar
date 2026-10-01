# Phase 7b-A (Rust + api snapshot writer) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Rust half of the public `/math` playground: the paper sim's basket fill arithmetic extracted into pure `sim::venue` functions (byte-identical paper rows), the 09-27 `arb-core` additions, a tested `crates/arb-wasm` JSON facade (the six `*_json` functions, `basket_json` included) whose golden fixtures are the contract plan 7b-B consumes, and an api task that writes the public snapshot `public.json` into a volume every 5 minutes; deployed as step A (api only).
**Architecture:** `PaperTrader::execute`/`unwind` call new pure functions `sim::venue::{basket_buy, unwind_sell}` (the `Books` trait moves to `venue`), and `sim::venue::basket_cash` values a basket exactly as the ledger books it; `sim::policy::settle` becomes public. `arb-core` gains `payoff`, `book::d1_sums`, and, in new files so no premise-locked file changes, `steps::walk_steps` and `classify::classify`. `crates/arb-wasm` has `src/input.rs` (parsing and limits), `src/api.rs` plus one module per function, and wasm32-only `#[wasm_bindgen]` wrappers; its fixtures are generated from cases in `tests/fixtures.rs`. The api splits its ladder and today handlers into `load_ladder`/`load_today`, and `bins/api/src/public.rs` turns their DTOs into a typed, whitelisted `PublicSnapshot`, written tmp → fsync → rename → dir fsync on a blocking thread by one tokio task started only with `PUBLIC_DIR`.
**Tech Stack:** Rust 1.98.1 (edition 2024), rust_decimal 1.41 (`serde-with-str`, `maths`), serde_json (`arbitrary_precision`), proptest, wasm-bindgen `=0.2.129` (wasm32 only; the only new crate), tokio, axum 0.8, sqlx 0.8 (inside `store` only); Postgres 17; Docker compose on the VPS; GitHub Actions.
**Spec:** docs/superpowers/specs/2026-09-30-phase7b-math-playground-design.md (§3, §4, §5, §8 Rust part, §9 step A, §10 7b-A), amending docs/superpowers/specs/2026-09-27-phase7-playground-explain-design.md (§3 and §4 "as 09-27").

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere. Nothing in this plan talks to Polymarket.
- **The facade holds no logic (09-27 §2, 7b §2):** "Everything that decides or computes money is in `arb-core` or `sim`, where the scanner and the paper sim use it." `arb-wasm` only parses JSON, checks limits, builds arb-core/sim inputs, calls them and prints. Its one decision is `episode_json`'s "the latest update with `t ≤ t_x`, or 0 if the episode has closed by then". `basket_json` plans "exactly as the policy does" (`sim::policy::decide`'s edge, full-set size and minimum checks, then `sim::policy::settle`).
- **No floats in Rust:** clippy denies `float_arithmetic`, `unwrap_used`, `expect_used` (`[workspace.lints.clippy]`; tests may unwrap per `clippy.toml`; integration test files start with `#![allow(clippy::unwrap_used, clippy::expect_used)]`). `arb-core`, `cards` and `arb-wasm` keep the no-`f32`/`f64` source scan (`tests/no_floats.rs`).
- **"Decimals cross as JSON strings in both directions. A JSON number where a decimal is expected is rejected with an error."** Every output decimal is `normalize()`d and zero is `"0"`, never `"-0"` (`arb-wasm`: `input::d`; api: `dto::Dec`). "Every output is a `#[derive(Serialize)]` struct (fixed field order)."
- **"Invalid input returns `{"error": "<plain sentence>"}` and never panics (`unwrap_used`/`expect_used` stay denied)."** "Anything outside these returns an error; nothing is silently clamped."
- **arb-wasm limits, checked before any work (09-27 §4.1, 7b §3.4):** input JSON ≤ 16 KiB (16,384 bytes); ≤ 6 legs (`basket_json`: 2–4 legs); ≤ 8 levels per side; prices on the 0.01 grid within 0.01–0.99; sizes > 0 and ≤ 1,000,000 with ≤ 6 dp; latency (`proc_ms`, `net_ms`, `match_ms`) 0–5,000 ms; `s` 2–6; `seconds_delay` 0–10; `gap_ms` 1–600,000; updates 2–8 with strictly increasing `t_ms` in 0–600,000; `convert_fee_bips` 0–10,000 (NO only); `min_order` default 5 (a size).
- **"All book inputs are YES-book levels, as the sim takes them: a NO buy walks the YES bids (mirrored), and a NO unwind sells into the YES asks (mirrored)."**
- **wasm-bindgen:** `wasm-bindgen = "=0.2.129"` (checked with `cargo search wasm-bindgen --limit 1` on 2026-09-30: `0.2.129`, and `wasm-bindgen-cli` `0.2.129`), "the only exact pin (`=x.y.z`), because its CLI must match"; under `[target.'cfg(target_arch = "wasm32")'.dependencies]` only. Plan 7b-B installs `wasm-bindgen-cli --locked --version 0.2.129`.
- **No other new dependency:** everything else comes from `[workspace.dependencies]`; no web change in this plan (web is plan 7b-B).
- **`sim` and `arb-core` stay pure** (no time, fs, threads, tokio, sqlx, reqwest): `cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown` passes (CI job `wasm`, Task 12; the target is installed on the PC since 2026-09-27).
- **The identity guard (7b §3.3):** "Every existing `sim` test passes unchanged, and so does `cargo test -p scanner`." The fill/ledger row order of `PaperTrader::execute` is kept (`pnl_now` first, then per leg Fill then Ledger). The replay identity check (Task 5, runbook step 5) passes before merging.
- **Premises:** any edit to a file listed in `crates/arb-core/src/premises.rs` `PREMISES` needs that area's version bumped and `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock`. Task 1 bumps `sim` 2 → 3; the lock is refreshed once, at the end of Task 4 (a refresh refuses a second change under the same version). Until then `premises_lock_matches_the_sources` is the ONE expected red test (`sim: version 2 → 3; refresh the lock`); Tasks 1–3 run the workspace with `-- --skip premises_lock_matches_the_sources`. `depth`, `detector_no`, `detector_yes` stay at 1: `walk_steps` and `classify` go in NEW files (`steps.rs`, `classify.rs`), never in `fill.rs`/`detect.rs`.
- **The api:** its main pool stays `analyze_ro` with `default_transaction_read_only=on`; `read_only: true` in compose, and `public-data` (mounted at `/var/lib/ct-public`) is its only writable mount. "No public request reaches the api, postgres, analyze or the scanner." The api must not depend on `sim`, `arb-core`, `pm-api`, `reqwest`, `tokio-tungstenite` or `sqlx` (`bins/scanner/tests/arch.rs`); only `store` uses sqlx (`deny.toml`).
- **What never ships (7b §1, §5.2):** "market names, market ids, token ids, episode ids, account ids or host names on any public path." The snapshot is typed structs only, pinned by the whitelist, content and size tests (Tasks 13, 15).
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A` or `.`); never stage `PROJECT.md` or `README.md`; commit messages never carry a `Co-Authored-By` line or any other attribution trailer.
- **Runbook format:** flat numbered steps, each one command block then `→ expected: …` and `if not: …`, **STOP** after any step whose failure must halt, rollback as commands, self-contained, no prose paragraphs.
- **CI (`.github/workflows/ci.yml`), run before the last commit of every task that touches Rust:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (with the PG prefix), `cargo deny check bans advisories`; from Task 12 also `cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown` and `cargo test -p arb-wasm`; compose: `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q`.
- **Shell prefix for every command** (Git Bash, in the worktree): `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store, api, analyze, scanner) use the PC Postgres on **127.0.0.1:15432**; prefix them with (PG prefix) `set -a; . /c/Users/Isabelly/PycharmProjects/cute-trading/.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1 &&`. Before the first such run in a task: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` must print `pg-up`; if not, stop and ask the owner. Never print `.env`.
- **Debug builds only** for the implementer (no `--release`, no `cargo install`); Docker builds and VPS steps are owner-run (Task 17's runbook).
- **Windows note:** `cargo test -p arb-wasm` on the PC prints `warning: linker stdout: Criando biblioteca …arb_wasm.dll.lib…` (the MSVC linker's note for the `cdylib`). It is not a rustc warning and does not fail clippy or CI.

## Review Focus

1. **A bracket bomb under 16 KiB.** 16,384 bytes of `[` (or `{"bids":` a thousand times) is inside the size limit; a recursive parser that overflows the stack aborts the WASM instance, which no `{"error"}` can report. serde_json's 128-level recursion limit must turn it into one `not valid input` sentence. Pinned in Task 9 by `bracket_bombs_under_16_kib_are_errors_not_crashes`.
2. **A request past the open depth, and a size the venue can't take.** `q_request = 500` on a 100-deep book must plan `q_target = q_open = 100` (the policy's `min(budget, Q*(open))`, shown beside it, not a silent clamp), and `7.5` shares at a 0.67 limit (5.025, 3 dp) must settle to 7 exactly as `sim::policy::settle` does, never 7.5. Pinned in Task 11 by `a_request_past_the_open_depth_plans_q_open_and_sizes_round_like_the_policy`.
3. **A SWEET row that no longer parses.** `markets` rows are raw `Value` clones of SWEET's body; a future field type change (`n_exec` as a string, a missing `label`) must drop that row, not fail the snapshot run (the file would freeze) and not leak the row untyped. Pinned in Task 13 by `a_sweet_row_that_does_not_parse_is_left_out`.
4. **A longer `public.json.tmp` left by a failed run.** Without `truncate(true)` the next, shorter write leaves the old tail in the tmp file and the rename publishes invalid JSON. Pinned in Task 14 by `a_leftover_longer_tmp_is_truncated_not_reused`.
5. **The file's mode.** The api writes as uid 65532; nginx (7b-B) reads the volume as another uid through a read-only mount. A file created `0600` (or under a strict umask without an explicit mode) is a 403 on the public page. Pinned in Task 14 by `the_file_is_readable_by_nginx_s_uid` (`#[cfg(unix)]`: runs in CI on Linux, not on the Windows PC).

---

## File Structure

```
crates/sim/src/venue.rs                          MODIFY: Books (moved in), FillLeg, BasketBuy, basket_buy, unwind_sell, ConvertTerms, HeldUnsold, BasketCash, basket_cash
crates/sim/src/trader.rs                         MODIFY: Books re-exported; execute calls basket_buy; unwind calls unwind_sell
crates/sim/src/policy.rs                         MODIFY: pub enum Settle, pub fn settle
crates/sim/tests/{venue,policy}.rs               MODIFY; crates/sim/tests/basket.rs CREATE
crates/arb-core/src/premises.rs, premises.lock   MODIFY: sim 2 → 3
bins/scanner/tests/premises.rs                   MODIFY: sim version 3
bins/scanner/examples/replay_dump.rs             CREATE; scripts/replay-identity.sh CREATE
crates/arb-core/src/{payoff,steps,classify}.rs   CREATE; crates/arb-core/src/{book,lib}.rs MODIFY
crates/arb-core/tests/{payoff,steps,classify}.rs CREATE
Cargo.toml, Cargo.lock                           MODIFY: members += crates/arb-wasm; wasm-bindgen 0.2.129 locked
crates/arb-wasm/Cargo.toml                       CREATE
crates/arb-wasm/src/{lib,input,api}.rs           CREATE; crates/arb-wasm/src/api/{mirror,payoff,classify,depth_walk,episode,basket}.rs CREATE
crates/arb-wasm/tests/{fixtures,values,never_panics,no_floats}.rs CREATE
crates/arb-wasm/fixtures/{mirror,payoff,classify,depth_walk,episode,basket}_json.json  CREATE (generated)
.github/workflows/ci.yml                         MODIFY: job `wasm`
bins/api/src/public.rs                           CREATE; bins/api/src/{lib,config,ladder,episodes,main}.rs MODIFY
bins/api/tests/public.rs                         CREATE
docker/rust.Dockerfile, compose.web.yml          MODIFY: /var/lib/ct-public (65532), volume public-data on api
docs/superpowers/runbooks/phase7b-api-vps.md     CREATE
```

---

### Task 1: sim — `Books` moves to `venue`, `settle` is public, sim premise 3

**Files:**
- Modify: `crates/sim/src/venue.rs:9` (after `use serde::{Deserialize, Serialize};`)
- Modify: `crates/sim/src/trader.rs:21-30` (the `Books` trait and its `HashMap` impl)
- Modify: `crates/sim/src/policy.rs:153-164` (`enum Settle`, `fn settle`)
- Modify: `crates/arb-core/src/premises.rs:28` (the `sim` entry's `"2"`)
- Modify: `bins/scanner/tests/premises.rs:69-74`
- Test: `crates/sim/tests/venue.rs` (append), `crates/sim/tests/policy.rs` (append)

**Interfaces:**
- Consumes: `sim::policy::walk_cost(levels: &[(Price, Shares)], q: Decimal, fee: FeeSchedule, gross: bool) -> Option<(Decimal, Price)>` (`policy.rs:123`).
- Produces: `sim::venue::Books` (`fn yes_book(&self, yes_token: &str) -> Option<&Book>`, implemented for `HashMap<String, Book>`), re-exported as `sim::trader::Books`; `pub enum sim::policy::Settle { BadInput, TooSmall }` (`Debug, Clone, Copy, PartialEq, Eq`); `pub fn sim::policy::settle(at: &impl Fn(Decimal) -> Option<(Decimal, Vec<Price>)>, q: Decimal, min: Decimal) -> Result<(Decimal, Vec<Price>), Settle>`; `arb_core::premises::code_version("sim") == Some("3")`.

- [ ] **Step 1: Write the failing tests** — append to `crates/sim/tests/venue.rs`:

```rust
#[test]
fn books_live_in_venue_and_the_trader_re_exports_them() {
    use arb_core::book::Book;
    use std::collections::HashMap;
    fn via_venue(b: &impl sim::venue::Books) -> bool {
        b.yes_book("y1").is_some()
    }
    fn via_trader(b: &impl sim::trader::Books) -> bool {
        b.yes_book("y1").is_some() && b.yes_book("y2").is_none()
    }
    let mut m: HashMap<String, Book> = HashMap::new();
    m.insert("y1".into(), book(&[("0.40", "10")], &[]));
    assert!(via_venue(&m) && via_trader(&m));
}
```

and append to `crates/sim/tests/policy.rs`:

```rust
#[test]
fn settle_is_public_and_rounds_like_the_policy() {
    use arb_core::money::{Price, Shares};
    use sim::policy::{Settle, settle};
    let lv: Vec<(Price, Shares)> = vec![(p("0.33"), Shares::parse("100").unwrap())];
    let at = |q: Decimal| walk_cost(&lv, q, FeeSchedule::ZERO, false).map(|(c, w)| (c, vec![w]));
    // 0.33 × size has ≤ 2 dp only for whole sizes: 7.5 settles to 7 at the same limit
    assert_eq!(settle(&at, d("7.5"), d("5")), Ok((d("7"), vec![p("0.33")])));
    assert_eq!(settle(&at, d("4.9"), d("5")), Err(Settle::TooSmall));
    assert_eq!(
        settle(&at, d("101"), d("5")),
        Err(Settle::BadInput),
        "past the book's depth"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sim --test venue --test policy`
Expected: FAIL to compile: `cannot find trait `Books` in module `sim::venue`` and `function `settle` is private`.

- [ ] **Step 3: Implement**

In `crates/sim/src/venue.rs`, replace `use serde::{Deserialize, Serialize};` (line 9) with:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Read access to the engine's YES books. It lives here, not in `trader`, so the pure fill
/// functions below never refer back to the trader (7b §3.2); `trader` re-exports it.
pub trait Books {
    fn yes_book(&self, yes_token: &str) -> Option<&Book>;
}

impl Books for HashMap<String, Book> {
    fn yes_book(&self, yes_token: &str) -> Option<&Book> {
        self.get(yes_token)
    }
}
```

In `crates/sim/src/trader.rs`, replace lines 21-30 (the `/// Read access to the engine's YES books.` trait and `impl Books for HashMap<String, Book>`) with:

```rust
/// Read access to the engine's YES books: defined in `venue` (7b §3.2), re-exported here so
/// `sim::trader::Books` keeps working for the scanner.
pub use crate::venue::Books;
```

(`HashMap` stays imported: `restore` uses it.)

In `crates/sim/src/policy.rs`, replace lines 153-164 (the `Settle` doc comment through `fn settle(`) with:

```rust
/// A `settle` failure: either the walk itself can't be quoted at `q` (should only happen for a
/// `q` past what `q_open` already validated), or the venue-rounded size fell below `min`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    BadInput,
    TooSmall,
}

/// Re-quotes the walk at `q` and rounds it down to venue precision (`order_size`), repeating
/// because rounding can cross a book breakpoint — which changes the worst price touched, which
/// can change the rounded size again (spec §5.6). Bounded so a pathological book can't loop
/// forever; on return, `limits` always matches the settled `q` exactly. Public since 7b §3.2:
/// arb-wasm's `basket_json` plans a basket with it exactly as `decide` does.
pub fn settle(
```

In `crates/arb-core/src/premises.rs`, change the `sim` entry's version (line 28) from `"2",` to `"3",`.

In `bins/scanner/tests/premises.rs`, replace lines 70-74:

```rust
    assert_eq!(
        sim.version,
        effective_version("2", &format!("policies_sha256={}", pol.sha256)),
        "spec 7a: PaperTrader::reset/mark_now bumped the sim code version"
    );
```

with:

```rust
    assert_eq!(
        sim.version,
        effective_version("3", &format!("policies_sha256={}", pol.sha256)),
        "spec 7b: the basket fill extraction (venue::basket_buy/unwind_sell) bumped the sim code version"
    );
```

- [ ] **Step 4: Run to verify they pass, and that nothing else moved**

Run: `cargo test -p sim && cargo test -p arb-core --test premises_lock`
Expected: every `sim` test passes; `premises_lock` FAILS with exactly `sim: version 2 → 3; refresh the lock` (expected until Task 4).
Run: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && (PG prefix) cargo test -p scanner --test premises --test replay --test engine_paper`
Expected: `pg-up`, then all pass.

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace -- --skip premises_lock_matches_the_sources`
Expected: no warnings; every `test result: ok`.

```bash
git add crates/sim/src/venue.rs crates/sim/src/trader.rs crates/sim/src/policy.rs crates/sim/tests/venue.rs crates/sim/tests/policy.rs crates/arb-core/src/premises.rs bins/scanner/tests/premises.rs
git commit -m "sim: Books moves to venue (trader re-exports it), settle and Settle public; sim premise 3"
```

---

### Task 2: sim — `venue::basket_buy`, called by `PaperTrader::execute`

**Files:**
- Modify: `crates/sim/src/venue.rs` (append after `order_size`)
- Modify: `crates/sim/src/trader.rs:11` (the `crate::venue` import), `:606-642` (`execute`'s buy loop and `q_eff`)
- Test: `crates/sim/tests/basket.rs` (create)

**Interfaces:**
- Consumes: `Books` (Task 1); `fak_buy(book, leg, fee, limit, qty, consumed) -> OrderFill` (`venue.rs:106`); `IntentLeg { key, limit, fee, seconds_delay }` (`policy.rs:38`).
- Produces: `pub struct FillLeg { pub key: LegKey, pub fee: FeeSchedule, pub limit: Price }`; `pub struct BasketBuy { pub per_leg: Vec<OrderFill>, pub q_eff: Decimal, pub excess: Vec<Decimal> }` (both `Debug, Clone, PartialEq, Eq`; `BasketBuy: Default`); `pub fn basket_buy(books: &impl Books, legs: &[FillLeg], q_target: Decimal, consumed: &mut ConsumedLiquidity) -> BasketBuy` in `sim::venue`.

- [ ] **Step 1: Write the failing tests** — create `crates/sim/tests/basket.rs`:

```rust
//! 7b §3.2–3.3: the basket fill arithmetic extracted from `PaperTrader::execute`/`unwind`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use common::{book, d, p};
use proptest::prelude::*;
use rust_decimal::Decimal;
use sim::consumed::ConsumedLiquidity;
use sim::types::{LegKey, Outcome};
use sim::venue::{FillLeg, basket_buy};
use std::collections::HashMap;

fn key(i: usize, outcome: Outcome) -> LegKey {
    LegKey {
        market_id: format!("m{i}"),
        yes_token: format!("y{i}"),
        outcome,
    }
}

fn yes_leg(i: usize, limit: &str) -> FillLeg {
    FillLeg {
        key: key(i, Outcome::Yes),
        fee: FeeSchedule::ZERO,
        limit: p(limit),
    }
}

fn books(v: Vec<Book>) -> HashMap<String, Book> {
    v.into_iter()
        .enumerate()
        .map(|(i, b)| (format!("y{i}"), b))
        .collect()
}

fn filled(b: &sim::venue::BasketBuy) -> Vec<Decimal> {
    b.per_leg.iter().map(|f| f.filled).collect()
}

#[test]
fn all_legs_fill_and_nothing_is_left_over() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.60", "20")]),
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("10")]);
    assert_eq!((b.q_eff, b.excess.clone()), (d("10"), vec![d("0"), d("0")]));
    assert_eq!(b.per_leg[0].notional, d("3"));
}

#[test]
fn one_empty_leg_makes_every_other_legs_whole_fill_excess() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.61", "20")]), // above the 0.60 limit: nothing
        book(&[], &[("0.05", "20")]),
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60"), yes_leg(2, "0.05")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(b.q_eff, d("0"));
    assert_eq!(b.excess, vec![d("10"), d("0"), d("10")]);
}

#[test]
fn partial_fills_leave_the_difference_as_excess() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.60", "6"), ("0.61", "50")]), // the limit stops at 6
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("6")]);
    assert_eq!((b.q_eff, b.excess), (d("6"), vec![d("4"), d("0")]));
}

#[test]
fn a_missing_book_is_an_empty_fill_and_no_legs_is_zero() {
    let bk = books(vec![book(&[], &[("0.30", "20")])]);
    let legs = [yes_leg(0, "0.30"), yes_leg(7, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("0")]);
    assert_eq!(b.per_leg[1], sim::venue::OrderFill::default());
    assert_eq!(b.q_eff, d("0"));
    let none = basket_buy(&bk, &[], d("10"), &mut ConsumedLiquidity::default());
    assert_eq!((none.q_eff, none.per_leg.len()), (d("0"), 0));
}

#[test]
fn a_no_basket_buys_the_mirrored_yes_bids_and_consumes_them() {
    let bk = books(vec![
        book(&[("0.60", "8")], &[]),
        book(&[("0.55", "20")], &[]),
    ]);
    let legs: Vec<FillLeg> = (0..2)
        .map(|i| FillLeg {
            key: key(i, Outcome::No),
            fee: FeeSchedule::ZERO,
            limit: p("0.45"),
        })
        .collect();
    let mut c = ConsumedLiquidity::default();
    let b = basket_buy(&bk, &legs, d("10"), &mut c);
    assert_eq!(filled(&b), vec![d("8"), d("10")]);
    assert_eq!((b.q_eff, b.excess.clone()), (d("8"), vec![d("0"), d("2")]));
    let again = basket_buy(&bk, &legs, d("10"), &mut c);
    assert_eq!(
        filled(&again),
        vec![d("0"), d("10")],
        "leg 0's bid is consumed"
    );
}

fn side() -> impl Strategy<Value = Vec<(Price, Shares)>> {
    prop::collection::btree_map(1u32..=99, 1u32..=400, 0..=4).prop_map(|m| {
        m.into_iter()
            .map(|(c, s)| {
                (
                    Price::new(Decimal::new(c.into(), 2)).unwrap(),
                    Shares::new(Decimal::new(s.into(), 1)).unwrap(),
                )
            })
            .collect()
    })
}

fn yes_book() -> impl Strategy<Value = Book> {
    (side(), side()).prop_map(|(bids, asks)| {
        let mut b = Book::default();
        b.apply_snapshot(&bids, &asks, 0);
        b
    })
}

proptest! {
    #[test]
    fn q_eff_is_the_smallest_fill_and_excess_is_never_negative(
        bks in prop::collection::vec(yes_book(), 2..=6),
        no in any::<bool>(),
        limit in 1u32..=99,
        q in 1u32..=500,
    ) {
        let outcome = if no { Outcome::No } else { Outcome::Yes };
        let legs: Vec<FillLeg> = (0..bks.len())
            .map(|i| FillLeg {
                key: key(i, outcome),
                fee: FeeSchedule::new(d("0.07"), 1).unwrap(),
                limit: Price::new(Decimal::new(limit.into(), 2)).unwrap(),
            })
            .collect();
        let b = basket_buy(&books(bks), &legs, Decimal::from(q), &mut ConsumedLiquidity::default());
        prop_assert_eq!(b.per_leg.len(), legs.len());
        for (f, x) in b.per_leg.iter().zip(&b.excess) {
            prop_assert!(b.q_eff <= f.filled);
            prop_assert!(*x >= Decimal::ZERO);
            prop_assert_eq!(*x, f.filled - b.q_eff);
            prop_assert!(f.filled <= Decimal::from(q));
        }
        prop_assert!(b.excess.iter().any(|x| x.is_zero()), "the smallest leg has no excess");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --test basket`
Expected: FAIL to compile: `unresolved imports `sim::venue::FillLeg`, `sim::venue::basket_buy``.

- [ ] **Step 3: Implement** — append to `crates/sim/src/venue.rs`:

```rust
/// One leg of a basket buy (7b §3.2): the token, its fee schedule and the policy's limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillLeg {
    pub key: LegKey,
    pub fee: FeeSchedule,
    pub limit: Price,
}

/// What a basket buy got: one fill per leg (in `legs` order), the full sets it made (`q_eff`,
/// the smallest leg fill, 0 with no legs) and each leg's shares beyond them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasketBuy {
    pub per_leg: Vec<OrderFill>,
    pub q_eff: Decimal,
    /// `per_leg[i].filled − q_eff`: the leg-risk inventory the sim unwinds.
    pub excess: Vec<Decimal>,
}

/// The paper sim's execution at t_x (spec §5.6, 7b §3.2): a FAK buy of `q_target` on every leg at
/// its limit, against the books minus `consumed`, in leg order. A leg whose book is missing gets an
/// empty fill.
pub fn basket_buy(
    books: &impl Books,
    legs: &[FillLeg],
    q_target: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> BasketBuy {
    let per_leg: Vec<OrderFill> = legs
        .iter()
        .map(|l| match books.yes_book(&l.key.yes_token) {
            Some(b) => fak_buy(b, &l.key, l.fee, l.limit, q_target, consumed),
            None => OrderFill::default(),
        })
        .collect();
    let q_eff = per_leg
        .iter()
        .map(|f| f.filled)
        .min()
        .unwrap_or(Decimal::ZERO);
    let excess = per_leg.iter().map(|f| f.filled - q_eff).collect();
    BasketBuy {
        per_leg,
        q_eff,
        excess,
    }
}
```

In `crates/sim/src/trader.rs`, change line 11 to:

```rust
use crate::venue::{FillLeg, OrderFill, basket_buy, fak_sell, outcome_levels, yes_level};
```

and in `execute`, replace from `let mut filled = Vec::with_capacity(p.planned.legs.len());` (line 607) through `let excess = f.filled - q_eff;` (line 645) with:

```rust
        let fill_legs: Vec<FillLeg> = p
            .planned
            .legs
            .iter()
            .map(|l| FillLeg {
                key: l.key.clone(),
                fee: l.fee,
                limit: l.limit,
            })
            .collect();
        // Account::buy never touches consumed liquidity, so buying every leg first and then
        // booking each leg's rows in order is the same as the old buy-then-book loop.
        let buy = basket_buy(books, &fill_legs, p.planned.q_target, &mut self.consumed[a]);
        for (leg, f) in p.planned.legs.iter().zip(&buy.per_leg) {
            out.push(SimOut::Fill(FillRow {
                account: self.accounts[a].id.clone(),
                episode_id: p.episode_id.clone(),
                leg: leg.key.clone(),
                t_ms: p.t_exec,
                side: "buy",
                limit: Some(leg.limit.get()),
                requested: p.planned.q_target,
                filled: f.filled,
                vwap: f.vwap(),
                fees: f.fees,
                levels_used: f.levels_used,
            }));
            if let Some(e) = self.accounts[a].buy(p.t_exec, &p.episode_id, &leg.key, f) {
                out.push(SimOut::Ledger(e));
            }
        }
        let q_eff = buy.q_eff;
        let mut leg_risk = Decimal::ZERO;
        for ((leg, f), &excess) in p.planned.legs.iter().zip(&buy.per_leg).zip(&buy.excess) {
```

The rest of the loop body (`if excess > Decimal::ZERO { … }`, the markouts on `f.vwap()`) and everything after it stay as they are. `pnl_now` is still called first (line 606), unchanged.

- [ ] **Step 4: Run to verify it passes, and the identity guard**

Run: `cargo test -p sim`
Expected: all pass, `basket` 6 tests (5 + the proptest).
Run: `(PG prefix) cargo test -p scanner --test replay --test engine_paper`
Expected: all pass (the paper rows are unchanged).

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace -- --skip premises_lock_matches_the_sources`
Expected: no warnings; every `test result: ok`.

```bash
git add crates/sim/src/venue.rs crates/sim/src/trader.rs crates/sim/tests/basket.rs
git commit -m "sim: venue::basket_buy (per-leg FAK fills, q_eff, excess), used by PaperTrader::execute"
```

---

### Task 3: sim — `venue::unwind_sell` (the held rounding moves in), called by `PaperTrader::unwind`

**Files:**
- Modify: `crates/sim/src/venue.rs` (append)
- Modify: `crates/sim/src/trader.rs:11`, `:17`, `:715-735` (`unwind` up to the `FillRow`)
- Test: `crates/sim/tests/basket.rs` (the `use sim::venue` line; append)

**Interfaces:**
- Consumes: `fak_sell(book, leg, fee, qty, consumed) -> OrderFill` (`venue.rs:118`).
- Produces: `pub fn sim::venue::unwind_sell(book: Option<&Book>, leg: &LegKey, fee: FeeSchedule, qty: Decimal, held: Decimal, min_order: Decimal, consumed: &mut ConsumedLiquidity) -> (Decimal, OrderFill)`: (requested = min(round2↓(qty), round2↓(held)), fill); an empty fill when `book` is `None` or requested < `min_order`.

- [ ] **Step 1: Write the failing tests** — in `crates/sim/tests/basket.rs` change `use sim::venue::{FillLeg, basket_buy};` to `use sim::venue::{FillLeg, OrderFill, basket_buy, unwind_sell};` and append:

```rust
fn no_key() -> LegKey {
    key(0, Outcome::No)
}

#[test]
fn an_unwind_below_the_minimum_sends_nothing_but_keeps_the_request() {
    let b = book(&[], &[("0.40", "100")]);
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("4.99"),
        d("30"),
        d("5"),
        &mut c,
    );
    assert_eq!((req, f), (d("4.99"), OrderFill::default()));
    assert!(c.is_empty(), "no order, nothing consumed");
}

#[test]
fn an_unwind_with_no_book_sends_nothing() {
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        None,
        &no_key(),
        FeeSchedule::ZERO,
        d("12"),
        d("12"),
        d("5"),
        &mut c,
    );
    assert_eq!((req, f), (d("12"), OrderFill::default()));
}

#[test]
fn held_and_qty_are_both_rounded_to_cents_toward_zero() {
    // NO unwind sells into the mirrored YES asks: NO bid = 1 − 0.40 = 0.60
    let b = book(&[], &[("0.40", "100")]);
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("9.999"),
        d("7.4567"),
        d("5"),
        &mut c,
    );
    assert_eq!(
        req,
        d("7.45"),
        "min(round2(9.999) = 9.99, round2(7.4567) = 7.45)"
    );
    assert_eq!((f.filled, f.notional), (d("7.45"), d("4.47")));
    let (req, _) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("6.019"),
        d("50"),
        d("5"),
        &mut c,
    );
    assert_eq!(req, d("6.01"));
}

proptest! {
    #[test]
    fn an_unwind_never_sells_more_than_it_may(
        bk in yes_book(),
        no in any::<bool>(),
        qty in 0u32..=100_000,
        held in 0u32..=100_000,
        min in 1u32..=10,
    ) {
        let leg = key(0, if no { Outcome::No } else { Outcome::Yes });
        let (qty, held) = (Decimal::new(qty.into(), 3), Decimal::new(held.into(), 3));
        let cap = qty.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToZero)
            .min(held.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToZero));
        let (req, f) = unwind_sell(Some(&bk), &leg, FeeSchedule::ZERO, qty, held, Decimal::from(min), &mut ConsumedLiquidity::default());
        prop_assert_eq!(req, cap);
        prop_assert!(f.filled <= cap);
        if cap < Decimal::from(min) {
            prop_assert_eq!(f, OrderFill::default());
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --test basket`
Expected: FAIL to compile: `unresolved import `sim::venue::unwind_sell``.

- [ ] **Step 3: Implement** — append to `crates/sim/src/venue.rs`:

```rust
/// The leg-risk unwind (spec §5.6, 7b §3.2): rounds the queued `qty` and the `held` position
/// (the raw quantity the account holds) to 2 dp toward zero and asks for the smaller, so a stale
/// unwind never sells shares the account no longer has. Below `min_order`, or with no book, no
/// order goes out: the fill is empty and the requested quantity is still returned (the sim's
/// zero-fill row keeps the held leg risk visible).
pub fn unwind_sell(
    book: Option<&Book>,
    leg: &LegKey,
    fee: FeeSchedule,
    qty: Decimal,
    held: Decimal,
    min_order: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> (Decimal, OrderFill) {
    let held = held.round_dp_with_strategy(2, RoundingStrategy::ToZero);
    let requested = qty
        .round_dp_with_strategy(2, RoundingStrategy::ToZero)
        .min(held);
    let fill = match book {
        Some(b) if requested >= min_order => fak_sell(b, leg, fee, requested, consumed),
        _ => OrderFill::default(),
    };
    (requested, fill)
}
```

In `crates/sim/src/trader.rs`, change line 11 to `use crate::venue::{FillLeg, basket_buy, outcome_levels, unwind_sell, yes_level};` and line 17 to `use rust_decimal::Decimal;`, then replace the body of `unwind` from its first line through the `let f = match books.yes_book(&u.leg.yes_token) { … };` block (lines 716-735) with:

```rust
        // Capped at what we actually hold (unwind_sell rounds both): a stale unwind (e.g. a
        // position already reduced by another event) can't sell shares we don't have. Below the
        // venue minimum, or the book gone: no order goes out, but the held inventory still gets a
        // zero-fill row (side "sell", filled 0) so it stays visible — a real bot would see the
        // same held-not-sold leg risk, not silence.
        let held = self.accounts[u.acct]
            .positions
            .get(&u.leg)
            .map_or(Decimal::ZERO, |p| p.qty);
        let (qty, f) = unwind_sell(
            books.yes_book(&u.leg.yes_token),
            &u.leg,
            u.fee,
            u.qty,
            held,
            self.cfg.min_order_shares,
            &mut self.consumed[u.acct],
        );
```

The `FillRow` push (with `requested: qty`) and the `sell` ledger row after it stay as they are.

- [ ] **Step 4: Run to verify it passes, and the identity guard**

Run: `cargo test -p sim && (PG prefix) cargo test -p scanner --test replay --test engine_paper`
Expected: all pass; `basket` 10 tests.

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace -- --skip premises_lock_matches_the_sources`
Expected: no warnings (`RoundingStrategy`, `OrderFill`, `fak_buy`, `fak_sell` are no longer imported by `trader.rs`); every `test result: ok`.

```bash
git add crates/sim/src/venue.rs crates/sim/src/trader.rs crates/sim/tests/basket.rs
git commit -m "sim: venue::unwind_sell rounds qty and held to cents and caps at held, used by PaperTrader::unwind"
```

---

### Task 4: sim — `venue::basket_cash` against the ledger; the premises lock refreshed

**Files:**
- Modify: `crates/sim/src/venue.rs` (append)
- Modify: `crates/arb-core/premises.lock` (refreshed by the lock test, never by hand)
- Test: `crates/sim/tests/basket.rs` (the `use sim::venue` line; append)

**Interfaces:**
- Consumes: `BasketBuy` (Task 2); `Account::{new, buy, sell, convert, valuation}` (`account.rs:90,166,195,226,339`) in the test.
- Produces: `pub struct ConvertTerms { pub s: usize, pub q: Decimal, pub fee_bips: Decimal }` (`Copy`); `pub struct HeldUnsold { pub qty: Decimal, pub basis: Decimal }`; `pub struct BasketCash { pub spent, pub fees, pub unwind_back, pub convert_back, pub net_cash: Decimal, pub held_unsold: Vec<HeldUnsold> }`; `pub fn sim::venue::basket_cash(buy: &BasketBuy, unwinds: &[OrderFill], convert: Option<ConvertTerms>) -> BasketCash`.
- Semantics: `net_cash` = Σ `cash_delta` of the ledger rows `Account::buy`/`sell`/`convert` produce for the same fills (buys, then each leg's unwind, then the conversion); the conversion is `(s − 1)·q − (s − 1)·q·fee_bips/10000` with q capped by what every leg still holds, as `Account::convert`; `held_unsold[i]` = leg i's excess never sold, at its basis (12 dp, as `Account::take_out`).
- **The ledger identity, corrected from 7b §3.3:** the ledger's realized total = `net_cash` + the open basis; for a NO basket (a conversion) the open basis = Σ `held_unsold.basis`; for a YES basket the open basis also holds the `q_eff` full sets kept to resolution. (The spec's "realized + open basis = net_cash + held_unsold basis" fails on its own example: A 10 @ 0.50, B 6 @ 0.50, 6 converted: realized 0, net_cash −2, open basis 2.)

- [ ] **Step 1: Write the failing tests** — in `crates/sim/tests/basket.rs` replace `use sim::venue::{FillLeg, OrderFill, basket_buy, unwind_sell};` with:

```rust
use sim::venue::{
    BasketBuy, ConvertTerms, FillLeg, HeldUnsold, OrderFill, basket_buy, basket_cash, unwind_sell,
};
```

and append:

```rust
fn of(filled: &str, notional: &str, fees: &str) -> OrderFill {
    OrderFill {
        filled: d(filled),
        notional: d(notional),
        fees: d(fees),
        levels_used: 1,
    }
}

#[test]
fn a_caught_no_basket_with_one_short_leg_loses_cash_and_holds_the_rest() {
    // A fills 10 @ 0.50, B 6 @ 0.50: 6 sets convert (s = 2 pays 1 each), A's 4 extra stay held
    let buy = BasketBuy {
        per_leg: vec![of("10", "5", "0"), of("6", "3", "0")],
        q_eff: d("6"),
        excess: vec![d("4"), d("0")],
    };
    let c = basket_cash(
        &buy,
        &[OrderFill::default(), OrderFill::default()],
        Some(ConvertTerms {
            s: 2,
            q: d("6"),
            fee_bips: d("50"),
        }),
    );
    assert_eq!((c.spent, c.unwind_back), (d("8"), d("0")));
    assert_eq!(
        (c.convert_back, c.fees),
        (d("5.97"), d("0.03")),
        "6 − 6 × 50 bips"
    );
    assert_eq!(c.net_cash, d("-2.03"));
    assert_eq!(
        c.held_unsold,
        vec![
            HeldUnsold {
                qty: d("4"),
                basis: d("2")
            },
            HeldUnsold {
                qty: d("0"),
                basis: d("0")
            }
        ]
    );
}

#[test]
fn a_yes_basket_keeps_its_full_sets_and_counts_only_the_unsold_excess() {
    let buy = BasketBuy {
        per_leg: vec![of("10", "3", "0.1"), of("6", "3.6", "0.1")],
        q_eff: d("6"),
        excess: vec![d("4"), d("0")],
    };
    // 3 of A's 4 extra sold at 0.25 with a 0.01 fee
    let c = basket_cash(&buy, &[of("3", "0.75", "0.01"), OrderFill::default()], None);
    assert_eq!(
        (c.spent, c.unwind_back, c.convert_back),
        (d("6.8"), d("0.74"), d("0"))
    );
    assert_eq!((c.fees, c.net_cash), (d("0.21"), d("-6.06")));
    assert_eq!(
        c.held_unsold[0].qty,
        d("1"),
        "10 − 3 sold − 6 kept to resolution"
    );
    assert_eq!(c.held_unsold[1].qty, d("0"));
}

fn spec(fee_bips: Decimal) -> sim::config::AccountSpec {
    sim::config::AccountSpec {
        policy: sim::config::PolicyKind::P0Converter,
        scenario: "t".into(),
        bankroll: d("1000000"),
        per_trade: d("100"),
        fee_bips,
    }
}

proptest! {
    #[test]
    fn basket_cash_is_what_the_ledger_books(
        tx in prop::collection::vec(yes_book(), 2..=6),
        unwind in prop::collection::vec(yes_book(), 6),
        no in any::<bool>(),
        limit in 1u32..=99,
        q in 5u32..=500,
        bips in prop_oneof![Just(0u32), Just(50u32), 0u32..=10_000],
    ) {
        let outcome = if no { Outcome::No } else { Outcome::Yes };
        let fee = FeeSchedule::new(d("0.07"), 1).unwrap();
        let legs: Vec<FillLeg> = (0..tx.len())
            .map(|i| FillLeg { key: key(i, outcome), fee, limit: Price::new(Decimal::new(limit.into(), 2)).unwrap() })
            .collect();
        let mut c = ConsumedLiquidity::default();
        let buy = basket_buy(&books(tx), &legs, Decimal::from(q), &mut c);
        let unwinds: Vec<OrderFill> = legs
            .iter()
            .zip(buy.per_leg.iter().zip(&buy.excess))
            .enumerate()
            .map(|(i, (l, (f, x)))| {
                if *x > Decimal::ZERO {
                    unwind_sell(Some(&unwind[i]), &l.key, fee, *x, f.filled, d("5"), &mut c).1
                } else {
                    OrderFill::default()
                }
            })
            .collect();
        let fee_bips = Decimal::from(bips);
        let convert = no.then_some(ConvertTerms { s: legs.len(), q: buy.q_eff, fee_bips });
        let cash = basket_cash(&buy, &unwinds, convert);

        let mut acct = sim::account::Account::new(spec(fee_bips));
        let mut rows = Vec::new();
        for (l, f) in legs.iter().zip(&buy.per_leg) {
            rows.extend(acct.buy(0, "e", &l.key, f));
        }
        for (l, u) in legs.iter().zip(&unwinds) {
            rows.extend(acct.sell(1, "e", &l.key, u));
        }
        if no && buy.q_eff > Decimal::ZERO {
            let s_legs: Vec<LegKey> = legs.iter().map(|l| l.key.clone()).collect();
            rows.extend(acct.convert(2, "e", &s_legs, &[], buy.q_eff));
        }
        let cash_delta: Decimal = rows.iter().map(|r| r.cash_delta).sum();
        let realized: Decimal = rows.iter().map(|r| r.realized_delta).sum();
        let open_basis = acct.valuation(|_| None).basis_open;
        prop_assert_eq!(cash.net_cash, cash_delta);
        prop_assert_eq!(realized, cash.net_cash + open_basis, "realized = net cash + the basis still held");
        let held_basis: Decimal = cash.held_unsold.iter().map(|h| h.basis).sum();
        if no {
            prop_assert_eq!(held_basis, open_basis, "after the conversion only unsold excess is held");
        } else {
            prop_assert!(held_basis <= open_basis);
        }
        for ((h, x), u) in cash.held_unsold.iter().zip(&buy.excess).zip(&unwinds) {
            prop_assert_eq!(h.qty, *x - u.filled);
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sim --test basket`
Expected: FAIL to compile: `unresolved imports `sim::venue::BasketBuy` … `basket_cash``.

- [ ] **Step 3: Implement** — append to `crates/sim/src/venue.rs`:

```rust
/// A NO→collateral conversion's terms (7b §3.2): `s` legs, `q` sets, the adapter fee in basis
/// points. Valued as `Account::convert` books it; the complement YES it returns count 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertTerms {
    pub s: usize,
    pub q: Decimal,
    pub fee_bips: Decimal,
}

/// Shares a leg still holds that were bought beyond the full sets and never sold, at their cost
/// basis (fees included). Cost still held, not cash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldUnsold {
    pub qty: Decimal,
    pub basis: Decimal,
}

/// A basket's cash, as the ledger books it (7b §3.2). Every figure is ≥ 0 except `net_cash`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasketCash {
    /// Σ buy notional + buy fees.
    pub spent: Decimal,
    /// Every fee paid: buys, unwinds and the conversion (already inside the other figures).
    pub fees: Decimal,
    /// Σ unwind notional − unwind fees.
    pub unwind_back: Decimal,
    /// (s − 1)·q − the conversion fee.
    pub convert_back: Decimal,
    /// unwind_back + convert_back − spent: the Σ cash_delta of the ledger rows.
    pub net_cash: Decimal,
    /// One per leg, in leg order.
    pub held_unsold: Vec<HeldUnsold>,
}

/// `Account::take_out` on a (qty, basis) pair: the same 12 dp basis rounding.
fn take_out(pos: &mut (Decimal, Decimal), qty: Decimal) -> (Decimal, Decimal) {
    let (held, basis) = *pos;
    let q = qty.min(held);
    if q <= Decimal::ZERO {
        return (Decimal::ZERO, Decimal::ZERO);
    }
    let out = if q == held {
        basis
    } else {
        (basis * q / held).round_dp_with_strategy(12, RoundingStrategy::MidpointNearestEven)
    };
    *pos = (held - q, basis - out);
    (q, out)
}

/// What a basket did to cash (7b §3.2), in the ledger's order: every buy, then each leg's unwind
/// (`unwinds[i]` is leg i's; a missing entry is no unwind), then the conversion when `convert` is
/// given (capped, like `Account::convert`, by what every leg still holds). `held_unsold` is each
/// leg's excess that was never sold: for a conversion everything left after it, otherwise what
/// is left beyond the `q_eff` full sets kept to resolution.
pub fn basket_cash(
    buy: &BasketBuy,
    unwinds: &[OrderFill],
    convert: Option<ConvertTerms>,
) -> BasketCash {
    let mut cash = BasketCash::default();
    let mut pos: Vec<(Decimal, Decimal)> = Vec::with_capacity(buy.per_leg.len());
    for f in &buy.per_leg {
        if f.filled > Decimal::ZERO {
            let cost = f.notional + f.fees;
            cash.spent += cost;
            cash.fees += f.fees;
            pos.push((f.filled, cost));
        } else {
            pos.push((Decimal::ZERO, Decimal::ZERO));
        }
    }
    for (i, u) in unwinds.iter().enumerate() {
        if u.filled > Decimal::ZERO {
            cash.unwind_back += u.notional - u.fees;
            cash.fees += u.fees;
            if let Some(p) = pos.get_mut(i) {
                take_out(p, u.filled);
            }
        }
    }
    if let Some(t) = convert
        && t.s >= 2
    {
        let held = pos.iter().map(|p| p.0).min().unwrap_or(Decimal::ZERO);
        let q = t.q.min(held);
        if q > Decimal::ZERO {
            for p in &mut pos {
                take_out(p, q);
            }
            let gross = (Decimal::from(t.s) - Decimal::ONE) * q;
            let fee = gross * t.fee_bips / Decimal::from(10_000);
            cash.convert_back = gross - fee;
            cash.fees += fee;
        }
    }
    cash.net_cash = cash.unwind_back + cash.convert_back - cash.spent;
    let kept = if convert.is_some() {
        Decimal::ZERO
    } else {
        buy.q_eff
    };
    cash.held_unsold = pos
        .into_iter()
        .map(|(qty, basis)| {
            let unsold = (qty - kept).max(Decimal::ZERO);
            let basis = if unsold == qty {
                basis
            } else {
                (basis * unsold / qty)
                    .round_dp_with_strategy(12, RoundingStrategy::MidpointNearestEven)
            };
            HeldUnsold { qty: unsold, basis }
        })
        .collect();
    cash
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p sim --test basket`
Expected: 13 passed (`basket_cash_is_what_the_ledger_books` runs 256 cases).

- [ ] **Step 5: Refresh the premises lock** (every `sim` file edit of Tasks 1–4 is in; version 3 since Task 1)

Run: `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock && cargo test -p arb-core --test premises_lock && grep '^sim ' crates/arb-core/premises.lock`
Expected: `premises.lock refreshed`, then `test result: ok. 1 passed`, then `sim 3 <64 hex>`.

- [ ] **Step 6: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace && cargo deny check bans advisories`
Expected: no warnings; every `test result: ok` (no skip any more).

```bash
git add crates/sim/src/venue.rs crates/sim/tests/basket.rs crates/arb-core/premises.lock
git commit -m "sim: venue::basket_cash books a basket as the ledger does (net cash, held unsold at basis); premises.lock at sim 3"
```

---

### Task 5: scanner — the replay identity harness (example + script)

**Files:**
- Create: `bins/scanner/examples/replay_dump.rs`
- Create: `scripts/replay-identity.sh`

**Interfaces:**
- Consumes: `scanner::config::Config::load(&Path) -> anyhow::Result<(Config, String)>` (`config.rs:134`), `Config.engine.{gap_ms, stale_lag_ms, suspect_window_ms, l1_suspect_window_ms, drift_settle_ms, issue_throttle_ms}`, `scanner::engine::{EngineCtx, Out}` (`Out::Paper(Box<SimOut>)`), `scanner::paper::parse_policies(&str) -> anyhow::Result<Policies>`, `scanner::replay::replay_dir_paper_with(dir, ctx, Option<PaperTrader>, sink) -> anyhow::Result<ReplayStats>`, `PaperTrader::new(SimConfig)`.
- Produces: `cargo run -p scanner --example replay_dump -- <capture dir> <scanner.toml> <policies.toml> <out file>` writing one line per paper Fill/Exec/Ledger row (`fill FillRow { … }`, `exec ExecRow { … }`, `ledger LedgerEntry { … }`) in emission order and printing `frames <n> parse_errors <n> paper_rows <n>`; `scripts/replay-identity.sh <capture dir> [base ref, default main]` printing `IDENTICAL` (exit 0) or `DIFFERENT` (exit 1). The example uses only APIs that exist at `main` (384e765), so the script can copy it into a temporary worktree of the base.

- [ ] **Step 1: The failing check**

Run: `cargo run -q -p scanner --example replay_dump`
Expected: FAIL: `error: no example target named `replay_dump``.

- [ ] **Step 2: Implement** — create `bins/scanner/examples/replay_dump.rs`:

```rust
//! 7b §3.3, the replay identity check: replay one capture directory with a policies file through
//! the engine and the paper trader, and write every paper fill, exec and ledger row, one per line,
//! in the order the trader emitted them. `scripts/replay-identity.sh` runs this same file at the
//! base commit and at the branch and compares the two dumps' sha256.
//!     cargo run -p scanner --example replay_dump -- <capture dir> <scanner.toml> <policies.toml> <out>
use anyhow::Context;
use scanner::config::Config;
use scanner::engine::{EngineCtx, Out};
use scanner::paper::parse_policies;
use scanner::replay::replay_dir_paper_with;
use sim::rows::SimOut;
use sim::trader::PaperTrader;
use std::fmt::Write as _;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, scanner_toml, policies_toml, out] = args.as_slice() else {
        anyhow::bail!("usage: replay_dump <capture dir> <scanner.toml> <policies.toml> <out file>");
    };
    let (cfg, hash) = Config::load(Path::new(scanner_toml))?;
    let text =
        std::fs::read_to_string(policies_toml).with_context(|| format!("read {policies_toml}"))?;
    let policies = parse_policies(&text)?;
    let ctx = EngineCtx {
        run_id: 0,
        source: "replay".to_string(),
        scanner_version: "replay-dump".to_string(),
        config_hash: hash,
        gap_ms: cfg.engine.gap_ms,
        stale_lag_ms: cfg.engine.stale_lag_ms,
        suspect_window_ms: cfg.engine.suspect_window_ms,
        l1_suspect_window_ms: cfg.engine.l1_suspect_window_ms,
        drift_settle_ms: cfg.engine.drift_settle_ms,
        issue_throttle_ms: cfg.engine.issue_throttle_ms,
    };
    let trader = PaperTrader::new(policies.sim).context("paper trader")?;
    let mut dump = String::new();
    let mut rows = 0u64;
    let stats = replay_dir_paper_with(Path::new(dir), ctx, Some(trader), |o| {
        if let Out::Paper(s) = o {
            let line = match *s {
                SimOut::Fill(f) => format!("fill {f:?}"),
                SimOut::Exec(e) => format!("exec {e:?}"),
                SimOut::Ledger(l) => format!("ledger {l:?}"),
                _ => return,
            };
            let _ = writeln!(dump, "{line}");
            rows += 1;
        }
    })?;
    std::fs::write(out, dump).with_context(|| format!("write {out}"))?;
    println!(
        "frames {} parse_errors {} paper_rows {rows}",
        stats.frames, stats.parse_errors
    );
    Ok(())
}
```

Create `scripts/replay-identity.sh` (LF line endings) and `chmod +x` it:

```bash
#!/usr/bin/env bash
# 7b §3.3: the sim extraction changes no paper row. Replays one capture directory at BASE (default
# main) and at the working tree with the same harness (bins/scanner/examples/replay_dump.rs, copied
# into a temporary worktree of BASE) and the same scanner.toml and policies.toml, then compares the
# dumps: in emission order and sorted. Prints IDENTICAL and exits 0, or DIFFERENT and exits 1.
#   scripts/replay-identity.sh <capture dir> [base ref]
set -euo pipefail
CAP="$(cd "$1" && pwd)"
BASE="${2:-main}"
ROOT="$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d)"
cleanup() {
  git -C "$ROOT" worktree remove --force "$WORK/base" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT
git -C "$ROOT" worktree add --detach "$WORK/base" "$BASE" >/dev/null
mkdir -p "$WORK/base/bins/scanner/examples"
cp "$ROOT/bins/scanner/examples/replay_dump.rs" "$WORK/base/bins/scanner/examples/replay_dump.rs"
# One target dir for both builds: the dependencies compile once.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
for side in base head; do
  if [ "$side" = base ]; then SRC="$WORK/base"; else SRC="$ROOT"; fi
  echo "== $side: $(git -C "$SRC" log -1 --format='%h %s')"
  (cd "$SRC" && cargo run -q -p scanner --example replay_dump -- \
    "$CAP" "$ROOT/config/scanner.toml" "$ROOT/config/policies.toml" "$WORK/$side.txt")
done
ROWS="$(wc -l < "$WORK/head.txt" | tr -d ' ')"
if [ "$ROWS" -eq 0 ]; then
  echo "DIFFERENT: no paper rows at all, so this hour proves nothing; pick a busier hour"
  exit 1
fi
for side in base head; do
  sort "$WORK/$side.txt" > "$WORK/$side.sorted"
  echo "$side rows $(wc -l < "$WORK/$side.txt" | tr -d ' ') order $(sha256sum < "$WORK/$side.txt" | cut -c1-16) sorted $(sha256sum < "$WORK/$side.sorted" | cut -c1-16)"
done
if cmp -s "$WORK/base.txt" "$WORK/head.txt" && cmp -s "$WORK/base.sorted" "$WORK/head.sorted"; then
  echo IDENTICAL
else
  diff "$WORK/base.txt" "$WORK/head.txt" | head -20 || true
  echo DIFFERENT
  exit 1
fi
```

- [ ] **Step 3: Run the checks**

Run: `cargo clippy -p scanner --examples -- -D warnings && bash -n scripts/replay-identity.sh && echo syntax-ok && cargo run -q -p scanner --example replay_dump 2>&1 | tail -1`
Expected: no warnings, `syntax-ok`, then `Error: usage: replay_dump <capture dir> <scanner.toml> <policies.toml> <out file>`.

- [ ] **Step 4: Run the identity check itself, if 7a's busy hour is on the PC**

Run: `ls "$HOME"/probe/cap/*/*.jsonl.zst && bash scripts/replay-identity.sh "$HOME/probe/cap" main 2>&1 | tail -7`
Expected: two `frames <N> parse_errors <n> paper_rows <R>` lines with the same numbers, `base rows R order <h1> sorted <h2>`, `head rows R order <h1> sorted <h2>` (identical hashes), `IDENTICAL`. Measured while writing this plan, on the PC with this plan's code against `main` 384e765 and the 2026-09-28 17:00 hour (812 MB, 6,721,810 frames): `paper_rows 2871` on both sides, identical hashes, `IDENTICAL`, 29 minutes in debug. No `.jsonl.zst` under `$HOME/probe/cap`: skip this step; the runbook fetches an hour (steps 4–5).

- [ ] **Step 5: Commit**

```bash
git add bins/scanner/examples/replay_dump.rs scripts/replay-identity.sh
git update-index --chmod=+x scripts/replay-identity.sh
git commit -m "scanner: replay_dump example and scripts/replay-identity.sh (paper fills, execs and ledger at base vs branch, sha256)"
```

---

### Task 6: arb-core — `payoff`

**Files:**
- Create: `crates/arb-core/src/payoff.rs`
- Modify: `crates/arb-core/src/lib.rs` (`pub mod payoff;` after `pub mod money;`)
- Test: `crates/arb-core/tests/payoff.rs`

**Interfaces:**
- Produces: `arb_core::payoff::{PayoffError::Size(usize), Matrix { rows: Vec<String>, cols: Vec<String>, possible: Vec<bool>, cells: Vec<Vec<Option<Decimal>>>, basket: Vec<Option<Decimal>>, guaranteed: Decimal, broken: Vec<bool> }, yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError>, no_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError>, implication() -> Matrix}`. Outcome names `A`…`L`; the uncovered column `Other`; rows `YES_A`/`NO_A`; implication columns `A∧B`, `A∧¬B`, `¬A∧B`, `¬A∧¬B` with `possible = [true, false, true, true]`.

- [ ] **Step 1: Write the failing tests** — create `crates/arb-core/tests/payoff.rs`:

```rust
//! Phase 7 spec §3.1: the §5.7 (b) payoff tables as code (L3, L4, L10).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::payoff::{PayoffError, implication, no_basket, yes_basket};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn yes_basket_with_other_uncovered_matches_the_spec_table() {
    let m = yes_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["YES_A", "YES_B", "YES_C"]);
    assert_eq!(m.cols, vec!["A", "B", "C", "Other"]);
    assert_eq!(
        m.cells[0],
        vec![Some(dec!(1)), Some(dec!(0)), Some(dec!(0)), Some(dec!(0))]
    );
    assert_eq!(
        m.basket,
        vec![Some(dec!(1)), Some(dec!(1)), Some(dec!(1)), Some(dec!(0))]
    );
    assert_eq!(m.guaranteed, dec!(1));
    assert_eq!(m.broken, vec![false, false, false, true]);
}

#[test]
fn no_basket_pays_s_minus_one_and_s_in_other() {
    let m = no_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["NO_A", "NO_B", "NO_C"]);
    assert_eq!(
        m.basket,
        vec![Some(dec!(2)), Some(dec!(2)), Some(dec!(2)), Some(dec!(3))]
    );
    assert_eq!(m.guaranteed, dec!(2));
    assert!(m.broken.iter().all(|b| !b));
}

#[test]
fn implication_has_four_columns_one_impossible() {
    let m = implication();
    assert_eq!(m.cols, vec!["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"]);
    assert_eq!(m.possible, vec![true, false, true, true]);
    assert_eq!(m.rows, vec!["NO_A", "YES_B"]);
    assert_eq!(
        m.cells[0],
        vec![Some(dec!(0)), None, Some(dec!(1)), Some(dec!(1))]
    );
    assert_eq!(
        m.cells[1],
        vec![Some(dec!(1)), None, Some(dec!(1)), Some(dec!(0))]
    );
    assert_eq!(
        m.basket,
        vec![Some(dec!(1)), None, Some(dec!(2)), Some(dec!(1))]
    );
    assert_eq!(m.broken, vec![false, false, false, false]);
}

#[test]
fn sizes_outside_2_to_12_are_errors_not_clamped() {
    assert_eq!(yes_basket(1, true).unwrap_err(), PayoffError::Size(1));
    assert_eq!(no_basket(13, true).unwrap_err(), PayoffError::Size(13));
    assert_eq!(yes_basket(0, false).unwrap_err(), PayoffError::Size(0));
}

proptest! {
    #[test]
    fn l3_yes_basket_broken_iff_other_uncovered(s in 2usize..=12, listed: bool) {
        let m = yes_basket(s, listed).unwrap();
        prop_assert_eq!(m.broken.iter().any(|b| *b), !listed);
    }

    #[test]
    fn l4_no_basket_never_broken(s in 2usize..=12, listed: bool) {
        let m = no_basket(s, listed).unwrap();
        prop_assert!(m.broken.iter().all(|b| !b));
        let floor = Decimal::from(s) - Decimal::ONE;
        for b in m.basket.iter().flatten() {
            prop_assert!(*b >= floor);
        }
    }
}

#[test]
fn l10_implication_pays_at_least_one_where_possible() {
    let m = implication();
    for (c, b) in m.basket.iter().enumerate() {
        if m.possible[c] {
            assert!(b.unwrap() >= dec!(1));
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test payoff`
Expected: FAIL to compile: `could not find `payoff` in `arb_core``.

- [ ] **Step 3: Implement** — create `crates/arb-core/src/payoff.rs`:

```rust
//! Payoff tables (parent spec §5.7 (b); Phase 7 spec §3.1). Rows are holdings, columns are the
//! winner. `broken[c]`: a possible column where the basket pays less than it guarantees (L3).
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PayoffError {
    #[error("a basket needs 2 to 12 outcomes, got {0}")]
    Size(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    pub possible: Vec<bool>,
    pub cells: Vec<Vec<Option<Decimal>>>,
    pub basket: Vec<Option<Decimal>>,
    pub guaranteed: Decimal,
    pub broken: Vec<bool>,
}

const NAMES: [&str; 12] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"];

fn check(s: usize) -> Result<(), PayoffError> {
    if (2..=12).contains(&s) {
        Ok(())
    } else {
        Err(PayoffError::Size(s))
    }
}

fn cols(s: usize, other_listed: bool) -> Vec<String> {
    let mut c: Vec<String> = NAMES.iter().take(s).map(|n| (*n).to_string()).collect();
    if !other_listed {
        c.push("Other".to_string());
    }
    c
}

fn finish(
    rows: Vec<String>,
    cols: Vec<String>,
    possible: Vec<bool>,
    cells: Vec<Vec<Option<Decimal>>>,
    guaranteed: Decimal,
) -> Matrix {
    let basket: Vec<Option<Decimal>> = (0..cols.len())
        .map(|c| {
            if possible[c] {
                Some(cells.iter().filter_map(|r| r[c]).sum())
            } else {
                None
            }
        })
        .collect();
    let broken = (0..cols.len())
        .map(|c| possible[c] && basket[c].is_some_and(|b| b < guaranteed))
        .collect();
    Matrix {
        rows,
        cols,
        possible,
        cells,
        basket,
        guaranteed,
        broken,
    }
}

fn unit(pays: bool) -> Option<Decimal> {
    Some(if pays { Decimal::ONE } else { Decimal::ZERO })
}

/// D2a: one YES of each listed outcome. Pays 1 if a listed outcome wins, 0 if "Other" does (L3).
pub fn yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES.iter().take(s).map(|n| format!("YES_{n}")).collect();
    let cells = (0..s)
        .map(|i| (0..cols.len()).map(|c| unit(c == i)).collect())
        .collect();
    let possible = vec![true; cols.len()];
    Ok(finish(rows, cols, possible, cells, Decimal::ONE))
}

/// D2b: one NO of each listed outcome. Pays s − 1 if a listed outcome wins, s otherwise (L4).
pub fn no_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError> {
    check(s)?;
    let cols = cols(s, other_listed);
    let rows = NAMES.iter().take(s).map(|n| format!("NO_{n}")).collect();
    let cells = (0..s)
        .map(|i| (0..cols.len()).map(|c| unit(c != i)).collect())
        .collect();
    let possible = vec![true; cols.len()];
    let guaranteed = Decimal::from(s) - Decimal::ONE;
    Ok(finish(rows, cols, possible, cells, guaranteed))
}

/// D3: A ⇒ B, holding NO_A + YES_B. A∧¬B is impossible (L10).
pub fn implication() -> Matrix {
    let cols: Vec<String> = ["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    let possible = vec![true, false, true, true];
    let (z, o) = (unit(false), unit(true));
    let cells = vec![vec![z, None, o, o], vec![o, None, o, z]];
    finish(
        vec!["NO_A".into(), "YES_B".into()],
        cols,
        possible,
        cells,
        Decimal::ONE,
    )
}
```

and add `pub mod payoff;` to `crates/arb-core/src/lib.rs` after `pub mod money;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core --test payoff && cargo test -p arb-core --test no_floats && cargo test -p arb-core --test premises_lock && cargo clippy -p arb-core --all-targets -- -D warnings`
Expected: 7 passed, the float scan and the premises lock pass (no locked file changed), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/payoff.rs crates/arb-core/src/lib.rs crates/arb-core/tests/payoff.rs
git commit -m "arb-core: payoff matrices for the playground (L3, L4, L10)"
```

---

### Task 7: arb-core — `book::d1_sums` and `steps::walk_steps`

**Files:**
- Modify: `crates/arb-core/src/book.rs` (append after `impl Book`)
- Create: `crates/arb-core/src/steps.rs` (09-27 §3.4 says `fill::walk_steps`; `fill.rs` is the `depth` premise's locked file, so the function lives beside it)
- Modify: `crates/arb-core/src/lib.rs` (`pub mod steps;` after `pub mod stats;`)
- Test: `crates/arb-core/tests/steps.rs`

**Interfaces:**
- Consumes: `arb_core::book::Top`, `arb_core::fill::{BuyLeg, BasketRule, walk}`, `FeeSchedule::phi`.
- Produces: `arb_core::book::d1_sums(top: &Top) -> Option<(Decimal, Decimal)>` = (long `A + (1 − B)`, short `B + (1 − A)`); `arb_core::steps::{Step { q_from, q_to, cost_per_basket, payoff_per_basket: Decimal, profitable: bool }, walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step>}`.

- [ ] **Step 1: Write the failing tests** — create `crates/arb-core/tests/steps.rs`:

```rust
//! Phase 7 spec §3.2 (D1 sums, L2) and §3.4 (walk_steps agrees with walk, L8).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::{Book, d1_sums};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use arb_core::steps::walk_steps;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn px(s: &str) -> Price {
    Price::parse(s).unwrap()
}

fn sh(s: &str) -> Shares {
    Shares::parse(s).unwrap()
}

#[test]
fn d1_sums_on_the_fed_book() {
    let mut b = Book::default();
    b.apply_snapshot(
        &[(px("0.54"), sh("5603.74"))],
        &[(px("0.55"), sh("32356.03"))],
        0,
    );
    assert_eq!(d1_sums(&b.top()), Some((dec!(1.01), dec!(0.99))));
}

#[test]
fn d1_sums_needs_both_sides() {
    let mut b = Book::default();
    b.apply_snapshot(&[(px("0.54"), sh("1"))], &[], 0);
    assert_eq!(d1_sums(&b.top()), None);
}

#[test]
fn walk_steps_includes_the_unprofitable_tail() {
    let legs = vec![
        BuyLeg {
            levels: vec![(px("0.30"), sh("10")), (px("0.40"), sh("10"))],
            fee: FeeSchedule::ZERO,
        },
        BuyLeg {
            levels: vec![(px("0.60"), sh("15"))],
            fee: FeeSchedule::ZERO,
        },
    ];
    let rule = BasketRule::AllLegs {
        payoff: Decimal::ONE,
    };
    let steps = walk_steps(&legs, rule);
    assert_eq!(steps.len(), 2);
    let s = |i: usize| {
        (
            steps[i].q_from,
            steps[i].q_to,
            steps[i].cost_per_basket,
            steps[i].profitable,
        )
    };
    assert_eq!(s(0), (dec!(0), dec!(10), dec!(0.90), true));
    assert_eq!(s(1), (dec!(10), dec!(15), dec!(1.00), false));
    assert_eq!(walk(&legs, rule, None).q_max, dec!(10));
}

fn level() -> impl Strategy<Value = (Price, Shares)> {
    (1u32..=99, 1u32..=500).prop_map(|(c, s)| {
        (
            Price::new(Decimal::new(c.into(), 2)).unwrap(),
            Shares::new(Decimal::from(s)).unwrap(),
        )
    })
}

fn legs() -> impl Strategy<Value = Vec<BuyLeg>> {
    prop::collection::vec(
        (
            prop::collection::btree_map(1u32..=99, 1u32..=500, 1..=4),
            any::<bool>(),
        ),
        2..=5,
    )
    .prop_map(|v| {
        v.into_iter()
            .map(|(m, fee)| BuyLeg {
                levels: m
                    .into_iter()
                    .map(|(c, s)| {
                        (
                            Price::new(Decimal::new(c.into(), 2)).unwrap(),
                            Shares::new(Decimal::from(s)).unwrap(),
                        )
                    })
                    .collect(),
                fee: if fee {
                    FeeSchedule::new(dec!(0.07), 1).unwrap()
                } else {
                    FeeSchedule::ZERO
                },
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn the_profitable_prefix_is_walk(legs in legs(), nested: bool) {
        let rule = if nested {
            BasketRule::NestedNo
        } else {
            BasketRule::AllLegs { payoff: Decimal::ONE }
        };
        let steps = walk_steps(&legs, rule);
        let w = walk(&legs, rule, None);
        let prefix: Vec<_> = steps.iter().take_while(|s| s.profitable).collect();
        let q: Decimal = prefix.iter().map(|s| s.q_to - s.q_from).sum();
        let pnl: Decimal = prefix
            .iter()
            .map(|s| (s.payoff_per_basket - s.cost_per_basket) * (s.q_to - s.q_from))
            .sum();
        prop_assert_eq!(q, w.q_max);
        prop_assert_eq!(pnl, w.pnl_exact);
        for pair in steps.windows(2) {
            prop_assert_eq!(pair[0].q_to, pair[1].q_from);
        }
        if !nested {
            for pair in steps.windows(2) {
                prop_assert!(pair[0].cost_per_basket <= pair[1].cost_per_basket, "L8");
            }
        }
    }

    #[test]
    fn d1_crossed_iff_long_below_one(b in level(), a in level()) {
        let mut book = Book::default();
        book.apply_snapshot(&[b], &[a], 0);
        let (long, short) = d1_sums(&book.top()).unwrap();
        prop_assert_eq!(book.is_crossed(), long < Decimal::ONE);
        prop_assert_eq!(book.is_crossed(), short > Decimal::ONE);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test steps`
Expected: FAIL to compile: `unresolved import `arb_core::book::d1_sums`` and `could not find `steps` in `arb_core``.

- [ ] **Step 3: Implement** — append to `crates/arb-core/src/book.rs`:

```rust
/// D1 sums (parent §5.7 (a), L2): long = A + (1 − B), short = B + (1 − A), from the best bid B and
/// best ask A; `None` without both sides. The book is crossed (B > A) exactly when long < 1 and
/// short > 1.
pub fn d1_sums(top: &Top) -> Option<(Decimal, Decimal)> {
    let (b, _) = top.best_bid?;
    let (a, _) = top.best_ask?;
    let (a, b) = (a.get(), b.get());
    Some((a + (Decimal::ONE - b), b + (Decimal::ONE - a)))
}
```

create `crates/arb-core/src/steps.rs`:

```rust
//! The full depth staircase for drawing (Phase 7 spec §3.4). It lives beside `fill`, not in it:
//! `fill.rs` is the `depth` premise's file, and an edit there restarts every card (premises.rs).
use crate::fill::{BasketRule, BuyLeg};
use rust_decimal::Decimal;

/// One segment of the merged level breakpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub q_from: Decimal,
    pub q_to: Decimal,
    /// Σ price + Σ φ(price) with exact fees: what one more basket costs on this segment.
    pub cost_per_basket: Decimal,
    pub payoff_per_basket: Decimal,
    /// payoff − cost > 0.
    pub profitable: bool,
}

/// Every segment `fill::walk` would visit, without its stop at the first unprofitable one, until
/// the rule runs out of legs. The profitable prefix is exactly `walk(legs, rule, None)` (tested).
pub fn walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step> {
    let n = legs.len();
    let mut idx = vec![0usize; n];
    let mut rem: Vec<Decimal> = legs
        .iter()
        .map(|l| l.levels.first().map_or(Decimal::ZERO, |x| x.1.get()))
        .collect();
    let mut q = Decimal::ZERO;
    let mut out = Vec::new();
    loop {
        let active: Vec<usize> = (0..n).filter(|&i| idx[i] < legs[i].levels.len()).collect();
        let payoff = match rule {
            BasketRule::AllLegs { payoff } => {
                if n == 0 || active.len() < n {
                    break;
                }
                payoff
            }
            BasketRule::NestedNo => {
                if active.len() < 2 {
                    break;
                }
                Decimal::from(active.len()) - Decimal::ONE
            }
        };
        let cost: Decimal = active
            .iter()
            .map(|&i| {
                let p = legs[i].levels[idx[i]].0;
                p.get() + legs[i].fee.phi(p)
            })
            .sum();
        let seg = active
            .iter()
            .map(|&i| rem[i])
            .min()
            .unwrap_or(Decimal::ZERO);
        if seg <= Decimal::ZERO {
            break;
        }
        out.push(Step {
            q_from: q,
            q_to: q + seg,
            cost_per_basket: cost,
            payoff_per_basket: payoff,
            profitable: payoff - cost > Decimal::ZERO,
        });
        q += seg;
        for &i in &active {
            rem[i] -= seg;
            if rem[i] == Decimal::ZERO {
                idx[i] += 1;
                rem[i] = legs[i]
                    .levels
                    .get(idx[i])
                    .map_or(Decimal::ZERO, |x| x.1.get());
            }
        }
    }
    out
}
```

and add `pub mod steps;` to `crates/arb-core/src/lib.rs` after `pub mod stats;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core && cargo clippy -p arb-core --all-targets -- -D warnings`
Expected: all pass (the existing lemma tests too; `premises_lock` passes: `book.rs` is in no premise), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/book.rs crates/arb-core/src/steps.rs crates/arb-core/src/lib.rs crates/arb-core/tests/steps.rs
git commit -m "arb-core: d1_sums and steps::walk_steps (the full depth staircase for drawing)"
```

---

### Task 8: arb-core — `classify::classify`

**Files:**
- Create: `crates/arb-core/src/classify.rs` (09-27 §3.3 says `detect::classify` with shared guards; `detect.rs` is the `detector_no`/`detector_yes` premises' locked file, so the guards are re-read here and the signal always comes from the detectors)
- Modify: `crates/arb-core/src/lib.rs` (`pub mod classify;` after `pub mod book;`)
- Test: `crates/arb-core/tests/classify.rs`

**Interfaces:**
- Consumes: `arb_core::detect::{Group, GroupLeg, LegStatus, Kind, Signal, detect_no_basket, detect_yes_basket}`.
- Produces: `arb_core::classify::{Node, Classification { path: Vec<Node>, signal: Option<Signal> }, classify(g: &Group) -> Classification}`. `Node` variants in evaluation order: `Decided`; `NoBasketTried`, `NoBasketFewLegs`, `NoBasketCrossed`, `NoBasketNotPositive`, `NoBasketPositive`; `YesBasketTried`, `YesBasketNoLive`, `YesBasketCrossed`, `YesBasketMissingAsk`, `YesBasketNotPositive`, `YesBasketPositive`; `Complete`, `Incomplete`. `Classification` is `Debug, Clone` (no `PartialEq`: `Signal` has none and `detect.rs` stays untouched).

- [ ] **Step 1: Write the failing tests** — create `crates/arb-core/tests/classify.rs`:

```rust
//! Phase 7 spec §3.3: classify = the existing detectors plus the path taken (parent §5.7 (c)).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::classify::{Node, classify};
use arb_core::detect::{
    Group, GroupLeg, Kind, LegStatus, Realization, Signal, detect_no_basket, detect_yes_basket,
};
use arb_core::fee::FeeSchedule;
use arb_core::fill::BasketQuote;
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;

fn leg(status: LegStatus, bid: Option<&str>, ask: Option<&str>) -> GroupLeg {
    let side = |p: Option<&str>| {
        p.map(|p| vec![(Price::parse(p).unwrap(), Shares::parse("100").unwrap())])
            .unwrap_or_default()
    };
    let mut book = Book::default();
    book.apply_snapshot(&side(bid), &side(ask), 0);
    GroupLeg {
        status,
        book,
        fee: FeeSchedule::ZERO,
    }
}

fn live(bid: &str, ask: &str) -> GroupLeg {
    leg(LegStatus::Live, Some(bid), Some(ask))
}

/// `Signal` has no `PartialEq` (detect.rs is premise-locked): compare what it carries.
type Key = (Kind, Realization, BasketQuote, BasketQuote, Vec<usize>);
fn key(s: &Signal) -> Key {
    (
        s.kind,
        s.realization,
        s.quote.clone(),
        s.gross.clone(),
        s.legs_used.clone(),
    )
}

#[test]
fn a_no_basket_edge_stops_after_d2b() {
    let g = Group {
        legs: vec![
            live("0.40", "0.41"),
            live("0.35", "0.36"),
            live("0.30", "0.31"),
        ],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::NoBasketTried, Node::NoBasketPositive]);
    assert_eq!(c.signal.unwrap().kind, Kind::NoBasket);
}

#[test]
fn an_other_leg_makes_the_yes_basket_incomplete() {
    let g = Group {
        legs: vec![
            live("0.20", "0.30"),
            live("0.20", "0.30"),
            leg(LegStatus::Other, None, None),
        ],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(
        c.path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketNotPositive,
            Node::YesBasketTried,
            Node::YesBasketPositive,
            Node::Incomplete
        ]
    );
    assert_eq!(c.signal.unwrap().kind, Kind::YesBasketIncomplete);
}

#[test]
fn a_decided_event_yields_nothing() {
    let g = Group {
        legs: vec![leg(LegStatus::ClosedYes, None, None), live("0.20", "0.30")],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::Decided]);
    assert!(c.signal.is_none());
}

#[test]
fn a_missing_ask_and_a_crossed_book_are_named() {
    let g = Group {
        legs: vec![
            live("0.20", "0.30"),
            leg(LegStatus::Live, Some("0.20"), None),
        ],
        augmented: false,
    };
    assert_eq!(classify(&g).path.last(), Some(&Node::YesBasketMissingAsk));
    let crossed = Group {
        legs: vec![live("0.56", "0.55"), live("0.60", "0.61")],
        augmented: false,
    };
    assert_eq!(
        classify(&crossed).path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketCrossed,
            Node::YesBasketTried,
            Node::YesBasketCrossed
        ]
    );
}

fn status() -> impl Strategy<Value = LegStatus> {
    prop_oneof![
        Just(LegStatus::Live),
        Just(LegStatus::Live),
        Just(LegStatus::Live),
        Just(LegStatus::Placeholder),
        Just(LegStatus::Other),
        Just(LegStatus::ClosedNo),
        Just(LegStatus::ClosedYes),
        Just(LegStatus::Halted),
    ]
}

fn side() -> impl Strategy<Value = Vec<(Price, Shares)>> {
    prop::collection::btree_map(1u32..=99, 1u32..=300, 0..=3).prop_map(|m| {
        m.into_iter()
            .map(|(c, s)| {
                (
                    Price::new(Decimal::new(c.into(), 2)).unwrap(),
                    Shares::new(Decimal::from(s)).unwrap(),
                )
            })
            .collect()
    })
}

fn group() -> impl Strategy<Value = Group> {
    (
        prop::collection::vec((status(), side(), side(), any::<bool>()), 1..=6),
        any::<bool>(),
    )
        .prop_map(|(legs, augmented)| Group {
            legs: legs
                .into_iter()
                .map(|(status, bids, asks, fee)| {
                    let mut book = Book::default();
                    book.apply_snapshot(&bids, &asks, 0);
                    GroupLeg {
                        status,
                        book,
                        fee: if fee {
                            FeeSchedule::new(Decimal::new(7, 2), 1).unwrap()
                        } else {
                            FeeSchedule::ZERO
                        },
                    }
                })
                .collect(),
            augmented,
        })
}

proptest! {
    #[test]
    fn classify_agrees_with_the_detectors(g in group()) {
        let c = classify(&g);
        let want = detect_no_basket(&g).or_else(|| detect_yes_basket(&g));
        prop_assert_eq!(c.signal.as_ref().map(key), want.as_ref().map(key));
        let last = c.path.last().copied();
        let positive = matches!(
            last,
            Some(Node::NoBasketPositive | Node::Complete | Node::Incomplete)
        );
        prop_assert_eq!(positive, c.signal.is_some(), "the path ends positive iff there is a signal: {:?}", c.path);
        prop_assert!(
            !(detect_no_basket(&g).is_some() && detect_yes_basket(&g).is_some()),
            "L7"
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-core --test classify`
Expected: FAIL to compile: `could not find `classify` in `arb_core``.

- [ ] **Step 3: Implement** — create `crates/arb-core/src/classify.rs`:

```rust
//! The §5.7 (c) decision tree as data (Phase 7 spec §3.3): the detectors' answer plus the path of
//! tests that led to it. It lives beside `detect`, not in it: `detect.rs` is the `detector_no` and
//! `detector_yes` premises' file, and an edit there restarts every card (premises.rs). So the
//! guards below re-read the same conditions `detect_no_basket`/`detect_yes_basket` check, and the
//! signal itself always comes from those detectors (a proptest pins the two together).
use crate::detect::{Group, Kind, LegStatus, Signal, detect_no_basket, detect_yes_basket};

/// One test on the parent §5.7 (c) decision tree, in the order `classify` evaluates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Decided,
    NoBasketTried,
    NoBasketFewLegs,
    NoBasketCrossed,
    NoBasketNotPositive,
    NoBasketPositive,
    YesBasketTried,
    YesBasketNoLive,
    YesBasketCrossed,
    YesBasketMissingAsk,
    YesBasketNotPositive,
    YesBasketPositive,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone)]
pub struct Classification {
    pub path: Vec<Node>,
    pub signal: Option<Signal>,
}

/// `detect`'s `decided`: an event with a leg already resolved YES yields nothing.
fn decided(g: &Group) -> bool {
    g.legs.iter().any(|l| l.status == LegStatus::ClosedYes)
}

/// The legs D2a buys: every live leg.
fn yes_live(g: &Group) -> Vec<usize> {
    (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live)
        .collect()
}

/// The legs D2b buys: every live leg with a YES bid.
fn no_used(g: &Group) -> Vec<usize> {
    (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live && g.legs[i].book.top().best_bid.is_some())
        .collect()
}

fn any_crossed(g: &Group, legs: &[usize]) -> bool {
    legs.iter().any(|&i| g.legs[i].book.is_crossed())
}

/// D2b first; when it is positive D2a is not tried (L7: they are never both positive).
/// `post_end` is the scanner's (from event dates), never part of the kind.
pub fn classify(g: &Group) -> Classification {
    let mut path = Vec::new();
    if decided(g) {
        path.push(Node::Decided);
        return Classification { path, signal: None };
    }
    path.push(Node::NoBasketTried);
    let used = no_used(g);
    if used.len() < 2 {
        path.push(Node::NoBasketFewLegs);
    } else if any_crossed(g, &used) {
        path.push(Node::NoBasketCrossed);
    } else if let Some(sig) = detect_no_basket(g) {
        path.push(Node::NoBasketPositive);
        return Classification {
            path,
            signal: Some(sig),
        };
    } else {
        path.push(Node::NoBasketNotPositive);
    }
    path.push(Node::YesBasketTried);
    let live = yes_live(g);
    if live.is_empty() {
        path.push(Node::YesBasketNoLive);
        return Classification { path, signal: None };
    }
    if any_crossed(g, &live) {
        path.push(Node::YesBasketCrossed);
        return Classification { path, signal: None };
    }
    if live
        .iter()
        .any(|&i| g.legs[i].book.top().best_ask.is_none())
    {
        path.push(Node::YesBasketMissingAsk);
        return Classification { path, signal: None };
    }
    match detect_yes_basket(g) {
        Some(sig) => {
            path.push(Node::YesBasketPositive);
            path.push(if sig.kind == Kind::YesBasket {
                Node::Complete
            } else {
                Node::Incomplete
            });
            Classification {
                path,
                signal: Some(sig),
            }
        }
        None => {
            path.push(Node::YesBasketNotPositive);
            Classification { path, signal: None }
        }
    }
}
```

and add `pub mod classify;` to `crates/arb-core/src/lib.rs` after `pub mod book;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p arb-core && cargo test -p sim && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass (`classify_agrees_with_the_detectors` 256 cases; `premises_lock` passes: no locked file changed), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arb-core/src/classify.rs crates/arb-core/src/lib.rs crates/arb-core/tests/classify.rs
git commit -m "arb-core: classify returns the detectors' answer plus the decision path"
```

---

### Task 9: arb-wasm — the crate, its limits, `mirror_json`, `payoff_json`, golden fixtures

**Files:**
- Modify: `Cargo.toml` (members: `"crates/arb-wasm"` after `"crates/cards"`), `Cargo.lock` (by cargo)
- Create: `crates/arb-wasm/Cargo.toml`, `src/lib.rs`, `src/input.rs`, `src/api.rs`, `src/api/mirror.rs`, `src/api/payoff.rs`
- Create: `crates/arb-wasm/tests/{fixtures,values,no_floats}.rs`, `crates/arb-wasm/fixtures/{mirror,payoff}_json.json` (generated)

**Interfaces:**
- Consumes: Tasks 6–7 (`payoff`, `d1_sums`), `arb_core::book::Book::{apply_snapshot, top, no_asks_best_first, no_bids_best_first}`, `arb_core::detect::canary`, `arb_core::fill::{walk, BuyLeg, BasketRule}`, `arb_core::money::{Price, Shares}`, `FeeSchedule::new`.
- Produces: `arb_wasm::api::{mirror_json, payoff_json}: fn(&str) -> String`; `pub mod arb_wasm::input` (`MAX_INPUT = 16 * 1024`, `MAX_LEGS = 6`, `MAX_LEVELS = 8`, `MAX_SIZE = 1_000_000`, `LevelIn`, `FeeIn`, `LegIn`, `BookIn`, `decimal`, `price`, `quantity`, `size`, `levels`, `fee`, `status`, `book`, `group`, `d`); wasm32 exports `mirror_json`, `payoff_json`. The JSON shapes are in "Contract for 7b-B" below.

- [ ] **Step 1: The crate and the failing tests**

Add `"crates/arb-wasm"` to `members` in the root `Cargo.toml`, right after `"crates/cards"`. Create `crates/arb-wasm/Cargo.toml`:

```toml
[package]
name = "arb-wasm"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# Phase 7 (09-27 §4.1, 7b §3.4): the public playground's engine, a JSON facade over arb-core and
# sim. Features are declared here explicitly so a lone wasm32 build and `cargo test --workspace`
# parse and print identically.
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
arb-core = { path = "../arb-core" }
sim = { path = "../sim" }
rust_decimal = { workspace = true, features = ["serde-with-str"] }
serde.workspace = true
serde_json = { workspace = true, features = ["arbitrary_precision"] }

# The only exact pin in the workspace: the wasm-bindgen CLI (scripts/build-wasm.sh, plan 7b-B)
# must be this same version.
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.129"

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

Create `crates/arb-wasm/src/lib.rs` with only the module lines, so the tests compile against a missing `api`:

```rust
#![forbid(unsafe_code)]
pub mod input;
```

Create `crates/arb-wasm/tests/fixtures.rs`:

```rust
//! Golden fixtures (09-27 spec §4.3, 7b §3.4). The cases live here; `fixtures/<name>_json.json`
//! holds `[{ "name", "input", "output" }]` with `input` and `output` as the exact strings passed and
//! returned, so the web's parity test (plan 7b-B) runs the same files through the WASM build and
//! compares byte for byte. `UPDATE_FIXTURES=1` rewrites the files from these cases; a reviewer
//! reads the diff. Without it, a file whose names, inputs or outputs differ from here fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{mirror_json, payoff_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;

#[derive(Serialize, Deserialize)]
struct Case {
    name: String,
    input: String,
    output: String,
}

/// `s` padded with trailing spaces (valid JSON whitespace) to exactly `len` bytes.
fn pad(s: &str, len: usize) -> String {
    assert!(s.len() <= len, "{} > {len}", s.len());
    format!("{s}{}", " ".repeat(len - s.len()))
}

fn check(func: &str, f: fn(&str) -> String, cases: Vec<(&str, String)>) {
    let path = format!("{}/fixtures/{func}.json", env!("CARGO_MANIFEST_DIR"));
    let want: Vec<Case> = cases
        .into_iter()
        .map(|(name, input)| Case {
            name: name.to_string(),
            output: f(&input),
            input,
        })
        .collect();
    if std::env::var("UPDATE_FIXTURES").is_ok_and(|v| v == "1") {
        fs::write(&path, serde_json::to_string_pretty(&want).unwrap() + "\n").unwrap();
        return;
    }
    let have: Vec<Case> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let names = |v: &[Case]| v.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&have),
        names(&want),
        "{func}: case names (run with UPDATE_FIXTURES=1)"
    );
    for (h, w) in have.iter().zip(&want) {
        assert_eq!(
            h.input, w.input,
            "{func}/{}: the fixture's input is not this test's",
            w.name
        );
        assert_eq!(h.output, w.output, "{func}/{}: output drifted", w.name);
    }
}

fn lv(p: &str, s: &str) -> Value {
    json!({ "p": p, "s": s })
}

// ---- mirror ----
fn fed_bids() -> Value {
    json!([
        lv("0.54", "5603.74"),
        lv("0.53", "85887.16"),
        lv("0.52", "42239.53"),
        lv("0.51", "28133.09")
    ])
}

fn fed_asks() -> Value {
    json!([
        lv("0.55", "32356.03"),
        lv("0.56", "252281.27"),
        lv("0.57", "23963.36"),
        lv("0.58", "5637.30")
    ])
}

fn mirror_in(bids: Value, asks: Value) -> String {
    json!({ "bids": bids, "asks": asks }).to_string()
}

fn eight(p0: u32) -> Value {
    Value::Array(
        (0..8)
            .map(|i| lv(&format!("0.{:02}", p0 + i), "1"))
            .collect(),
    )
}

#[test]
fn mirror() {
    let fed = mirror_in(fed_bids(), fed_asks());
    check(
        "mirror_json",
        mirror_json,
        vec![
            ("fed-book", fed.clone()),
            (
                "crossed",
                mirror_in(json!([lv("0.56", "10")]), json!([lv("0.55", "10")])),
            ),
            ("bids-only", mirror_in(json!([lv("0.54", "10")]), json!([]))),
            ("empty", mirror_in(json!([]), json!([]))),
            (
                "json-number-price",
                r#"{"bids":[{"p":0.55,"s":"1"}],"asks":[]}"#.to_string(),
            ),
            (
                "unknown-field",
                r#"{"bids":[],"asks":[],"depth":3}"#.to_string(),
            ),
            ("off-grid", mirror_in(json!([lv("0.555", "1")]), json!([]))),
            ("price-0.01", mirror_in(json!([lv("0.01", "1")]), json!([]))),
            ("price-0.00", mirror_in(json!([lv("0.00", "1")]), json!([]))),
            ("price-0.99", mirror_in(json!([]), json!([lv("0.99", "1")]))),
            ("price-1.00", mirror_in(json!([]), json!([lv("1.00", "1")]))),
            (
                "duplicate-price",
                mirror_in(json!([lv("0.50", "1"), lv("0.50", "2")]), json!([])),
            ),
            ("size-zero", mirror_in(json!([lv("0.50", "0")]), json!([]))),
            (
                "size-1000000",
                mirror_in(json!([lv("0.50", "1000000")]), json!([])),
            ),
            (
                "size-1000001",
                mirror_in(json!([lv("0.50", "1000001")]), json!([])),
            ),
            (
                "size-7dp",
                mirror_in(json!([lv("0.50", "0.0000001")]), json!([])),
            ),
            ("eight-levels", mirror_in(eight(10), json!([]))),
            (
                "nine-levels",
                mirror_in(
                    json!([
                        lv("0.01", "1"),
                        lv("0.02", "1"),
                        lv("0.03", "1"),
                        lv("0.04", "1"),
                        lv("0.05", "1"),
                        lv("0.06", "1"),
                        lv("0.07", "1"),
                        lv("0.08", "1"),
                        lv("0.09", "1")
                    ]),
                    json!([]),
                ),
            ),
            ("input-16384", pad(&fed, 16 * 1024)),
            ("input-16385", pad(&fed, 16 * 1024 + 1)),
        ],
    );
}

// ---- payoff ----
fn pr(bid: &str, ask: &str) -> Value {
    json!({ "bid": bid, "ask": ask })
}

fn payoff_in(kind: &str, s: u32, other_listed: bool, prices: Value) -> String {
    json!({ "kind": kind, "s": s, "other_listed": other_listed, "prices": prices }).to_string()
}

#[test]
fn payoff() {
    check(
        "payoff_json",
        payoff_json,
        vec![
            ("yes-3-other", payoff_in("yes", 3, false, Value::Null)),
            ("yes-3-listed", payoff_in("yes", 3, true, Value::Null)),
            ("no-3-other", payoff_in("no", 3, false, Value::Null)),
            (
                "no-2-listed-prices",
                payoff_in(
                    "no",
                    2,
                    true,
                    json!([pr("0.60", "0.61"), pr("0.50", "0.51")]),
                ),
            ),
            (
                "yes-2-prices",
                payoff_in(
                    "yes",
                    2,
                    true,
                    json!([pr("0.40", "0.45"), pr("0.50", "0.52")]),
                ),
            ),
            (
                "implication-prices",
                payoff_in(
                    "implication",
                    2,
                    true,
                    json!([pr("0.70", "0.72"), pr("0.40", "0.42")]),
                ),
            ),
            ("yes-s1", payoff_in("yes", 1, true, Value::Null)),
            ("yes-s2", payoff_in("yes", 2, false, Value::Null)),
            ("no-s6", payoff_in("no", 6, false, Value::Null)),
            ("no-s7", payoff_in("no", 7, false, Value::Null)),
            (
                "prices-wrong-count",
                payoff_in(
                    "no",
                    3,
                    true,
                    json!([pr("0.60", "0.61"), pr("0.50", "0.51")]),
                ),
            ),
            ("bad-kind", payoff_in("maybe", 3, true, Value::Null)),
        ],
    );
}
```

Create `crates/arb-wasm/tests/values.rs`:

```rust
//! Hand-computed expectations for the facade (09-27 spec §4.1, 7b §3.4) and its limits.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{mirror_json, payoff_json};
use serde_json::Value;

fn v(out: &str) -> Value {
    serde_json::from_str(out).unwrap()
}

fn err(out: &str) -> String {
    v(out)["error"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error, got {out}"))
        .to_string()
}

#[test]
fn mirror_reflects_the_fed_book() {
    let out = mirror_json(
        r#"{"bids":[{"p":"0.54","s":"5603.74"},{"p":"0.53","s":"85887.16"}],"asks":[{"p":"0.55","s":"32356.03"}]}"#,
    );
    assert_eq!(
        out,
        r#"{"no_asks":[{"p":"0.46","s":"5603.74"},{"p":"0.47","s":"85887.16"}],"no_bids":[{"p":"0.45","s":"32356.03"}],"best_bid":{"p":"0.54","s":"5603.74"},"best_ask":{"p":"0.55","s":"32356.03"},"spread":"0.01","d1_long":"1.01","d1_short":"0.99","crossed":false}"#
    );
}

#[test]
fn payoff_top_edges_follow_l5() {
    // YES: 1 − (0.45 + 0.52) = 0.03
    let yes = payoff_json(
        r#"{"kind":"yes","s":2,"other_listed":true,"prices":[{"bid":"0.40","ask":"0.45"},{"bid":"0.50","ask":"0.52"}]}"#,
    );
    assert_eq!(v(&yes)["top_edge_gross"], "0.03");
    // NO: Σ bids − 1 = (0.60 + 0.50) − 1 = 0.10
    let no = payoff_json(
        r#"{"kind":"no","s":2,"other_listed":true,"prices":[{"bid":"0.60","ask":"0.61"},{"bid":"0.50","ask":"0.51"}]}"#,
    );
    assert_eq!(v(&no)["top_edge_gross"], "0.1");
}

#[test]
fn bad_mirror_and_payoff_inputs_are_one_sentence_errors() {
    let cases = [
        (
            r#"{"bids":[{"p":0.55,"s":"1"}],"asks":[]}"#,
            "not valid input",
        ),
        (
            r#"{"bids":[{"p":"0.555","s":"1"}],"asks":[]}"#,
            "steps of 0.01",
        ),
        (
            r#"{"bids":[{"p":"1.00","s":"1"}],"asks":[]}"#,
            "steps of 0.01",
        ),
        (r#"{"bids":[{"p":"0","s":"1"}],"asks":[]}"#, "steps of 0.01"),
        (
            r#"{"bids":[{"p":"0.50","s":"1"},{"p":"0.50","s":"2"}],"asks":[]}"#,
            "same price",
        ),
        (r#"{"bids":[{"p":"0.50","s":"0"}],"asks":[]}"#, "size"),
        (r#"{"bids":[{"p":"0.50","s":"1000001"}],"asks":[]}"#, "size"),
    ];
    for (input, needle) in cases {
        let e = err(&mirror_json(input));
        assert!(e.contains(needle), "{input} → {e}");
        assert!(!e.contains('\n'));
    }
    let s1 = payoff_json(r#"{"kind":"yes","s":1,"other_listed":true,"prices":null}"#);
    assert!(err(&s1).contains("2 to 6"));
    let big = format!(r#"{{"bids":[],"asks":[]}}{}"#, " ".repeat(16 * 1024));
    assert!(err(&mirror_json(&big)).contains("16 KiB"));
}

#[test]
fn bracket_bombs_under_16_kib_are_errors_not_crashes() {
    // serde_json stops at 128 levels of nesting; a stack overflow would abort the WASM instance
    for f in [mirror_json, payoff_json] {
        for bomb in ["[".repeat(16 * 1024), r#"{"bids":"#.repeat(1_000)] {
            let e = err(&f(&bomb));
            assert!(e.starts_with("not valid input"), "{e}");
        }
    }
}
```

Create `crates/arb-wasm/tests/no_floats.rs`:

```rust
//! Phase 7: arb-wasm must never use floating point (same scan as arb-core).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;
use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, hits);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let bytes = line.as_bytes();
                for needle in ["f32", "f64"] {
                    let mut start = 0;
                    while let Some(pos) = line[start..].find(needle) {
                        let at = start + pos;
                        let before_ok = at == 0
                            || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
                        let end = at + needle.len();
                        let after_ok = end >= bytes.len()
                            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
                        if before_ok && after_ok {
                            hits.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                        }
                        start = end;
                    }
                }
            }
        }
    }
}

#[test]
fn no_floating_point_in_src() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&src, &mut hits);
    assert!(
        hits.is_empty(),
        "floating point found:\n{}",
        hits.join("\n")
    );
}

#[test]
fn detector_sees_a_float() {
    let dir = std::env::temp_dir().join("arb_wasm_no_floats_probe");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.rs"), "let x: f64 = 1.0;\nlet buf64 = 1;\n").unwrap();
    let mut hits = Vec::new();
    scan(&dir, &mut hits);
    assert_eq!(hits.len(), 1, "{hits:?}");
}
```

Create both fixture files with an empty list: `printf '[]\n' > crates/arb-wasm/fixtures/mirror_json.json && printf '[]\n' > crates/arb-wasm/fixtures/payoff_json.json`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-wasm`
Expected: FAIL to compile: `file not found for module `input`` and `unresolved import `arb_wasm::api``.

- [ ] **Step 3: Implement** — create `crates/arb-wasm/src/input.rs`:

```rust
//! Parsing and limits (09-27 spec §4.1, 7b §3.4). Every check runs before any arb-core or sim
//! call; nothing is clamped. Decimals arrive as JSON strings only: a JSON number where a decimal
//! is expected fails deserialization ("not valid input: …").
use arb_core::book::Book;
use arb_core::detect::{Group, GroupLeg, LegStatus};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::Deserialize;

pub const MAX_INPUT: usize = 16 * 1024;
pub const MAX_LEGS: usize = 6;
pub const MAX_LEVELS: usize = 8;
pub const MAX_SIZE: u32 = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelIn {
    pub p: String,
    pub s: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeIn {
    pub rate: String,
    pub exponent: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegIn {
    pub status: String,
    #[serde(default)]
    pub bids: Vec<LevelIn>,
    #[serde(default)]
    pub asks: Vec<LevelIn>,
    #[serde(default)]
    pub fee: Option<FeeIn>,
}

/// One YES book: bids and asks, each at most 8 levels.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookIn {
    #[serde(default)]
    pub bids: Vec<LevelIn>,
    #[serde(default)]
    pub asks: Vec<LevelIn>,
}

/// A decimal written as a JSON string.
pub fn decimal(s: &str, what: &str) -> Result<Decimal, String> {
    Decimal::from_str_exact(s.trim()).map_err(|_| format!("{what} {s:?} is not a decimal"))
}

pub fn price(s: &str) -> Result<Price, String> {
    let bad = || format!("price {s:?} must be 0.01 to 0.99 in steps of 0.01");
    let p = Price::parse(s).map_err(|_| bad())?;
    let d = p.get();
    if d < Decimal::new(1, 2)
        || d > Decimal::new(99, 2)
        || (d * Decimal::ONE_HUNDRED).fract() != Decimal::ZERO
    {
        return Err(bad());
    }
    Ok(p)
}

/// A share quantity above 0 and at most 1,000,000, with at most 6 decimals. `what` names it.
pub fn quantity(s: &str, what: &str) -> Result<Decimal, String> {
    let bad =
        || format!("{what} {s:?} must be above 0 and at most 1,000,000, with at most 6 decimals");
    let q = Shares::parse(s).map_err(|_| bad())?.get();
    if q <= Decimal::ZERO || q > Decimal::from(MAX_SIZE) {
        return Err(bad());
    }
    Ok(q)
}

pub fn size(s: &str) -> Result<Shares, String> {
    let q = quantity(s, "size")?;
    Shares::new(q).map_err(|e| e.to_string())
}

pub fn levels(v: &[LevelIn]) -> Result<Vec<(Price, Shares)>, String> {
    if v.len() > MAX_LEVELS {
        return Err(format!("at most {MAX_LEVELS} levels per side"));
    }
    let mut out: Vec<(Price, Shares)> = Vec::with_capacity(v.len());
    for l in v {
        let p = price(&l.p)?;
        if out.iter().any(|(q, _)| *q == p) {
            return Err(format!("two levels at the same price {}", l.p));
        }
        out.push((p, size(&l.s)?));
    }
    Ok(out)
}

pub fn fee(f: &Option<FeeIn>) -> Result<FeeSchedule, String> {
    match f {
        None => Ok(FeeSchedule::ZERO),
        Some(f) => {
            let rate = decimal(&f.rate, "fee rate")?;
            FeeSchedule::new(rate, f.exponent).map_err(|e| format!("fee: {e}"))
        }
    }
}

pub fn status(s: &str) -> Result<LegStatus, String> {
    Ok(match s {
        "live" => LegStatus::Live,
        "placeholder" => LegStatus::Placeholder,
        "other" => LegStatus::Other,
        "closed_no" => LegStatus::ClosedNo,
        "closed_yes" => LegStatus::ClosedYes,
        "halted" => LegStatus::Halted,
        _ => return Err(format!("unknown leg status {s:?}")),
    })
}

pub fn book(bids: &[LevelIn], asks: &[LevelIn]) -> Result<Book, String> {
    let mut b = Book::default();
    b.apply_snapshot(&levels(bids)?, &levels(asks)?, 0);
    Ok(b)
}

pub fn group(legs: &[LegIn], augmented: bool) -> Result<Group, String> {
    if legs.is_empty() || legs.len() > MAX_LEGS {
        return Err(format!("1 to at most {MAX_LEGS} legs"));
    }
    let legs = legs
        .iter()
        .map(|l| {
            Ok(GroupLeg {
                status: status(&l.status)?,
                book: book(&l.bids, &l.asks)?,
                fee: fee(&l.fee)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Group { legs, augmented })
}

/// Decimal → JSON string: normalized, and zero is always "0" (never "-0").
pub fn d(x: Decimal) -> String {
    if x.is_zero() {
        "0".to_string()
    } else {
        x.normalize().to_string()
    }
}
```

Create `crates/arb-wasm/src/api.rs`:

```rust
//! JSON facade over arb-core and sim (09-27 spec §4.1, 7b spec §3.4): each `<name>_json` parses,
//! checks the limits, calls arb-core/sim and prints. It computes no money figure; its one decision
//! is which update is in force at t_x (`episode_json`, 09-27 §4.1).
use crate::input::{MAX_INPUT, d};
use arb_core::money::{Price, Shares};
use serde::Serialize;
use serde::de::DeserializeOwned;

mod mirror;
mod payoff;

pub use mirror::mirror_json;
pub use payoff::payoff_json;

#[derive(Serialize)]
struct ErrOut<'a> {
    error: &'a str,
}

fn err(m: &str) -> String {
    serde_json::to_string(&ErrOut { error: m })
        .unwrap_or_else(|_| r#"{"error":"internal"}"#.to_string())
}

/// Size check, parse, run, print. Every failure is one `{"error": "<sentence>"}`.
fn run<I: DeserializeOwned, O: Serialize>(
    input: &str,
    f: impl FnOnce(I) -> Result<O, String>,
) -> String {
    if input.len() > MAX_INPUT {
        return err("input is larger than 16 KiB");
    }
    let parsed: I = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => {
            return err(&format!(
                "not valid input: {}",
                e.to_string().replace('\n', " ")
            ));
        }
    };
    match f(parsed).and_then(|o| serde_json::to_string(&o).map_err(|e| e.to_string())) {
        Ok(s) => s,
        Err(m) => err(&m),
    }
}

#[derive(Serialize)]
struct LevelOut {
    p: String,
    s: String,
}

fn lv((p, s): (Price, Shares)) -> LevelOut {
    LevelOut {
        p: d(p.get()),
        s: d(s.get()),
    }
}
```

Create `crates/arb-wasm/src/api/mirror.rs`:

```rust
//! `mirror_json`: one YES book reflected (L1), its D1 sums and the canary (09-27 §4.1).
use super::{LevelOut, lv, run};
use crate::input::{LevelIn, d, levels};
use arb_core::book::{Book, d1_sums};
use arb_core::detect::canary;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MirrorIn {
    bids: Vec<LevelIn>,
    asks: Vec<LevelIn>,
}

#[derive(Serialize)]
struct MirrorOut {
    no_asks: Vec<LevelOut>,
    no_bids: Vec<LevelOut>,
    best_bid: Option<LevelOut>,
    best_ask: Option<LevelOut>,
    spread: Option<String>,
    d1_long: Option<String>,
    d1_short: Option<String>,
    crossed: bool,
}

/// One YES book reflected (L1) with its D1 sums and the canary.
pub fn mirror_json(input: &str) -> String {
    run(input, |i: MirrorIn| {
        let mut b = Book::default();
        b.apply_snapshot(&levels(&i.bids)?, &levels(&i.asks)?, 0);
        let top = b.top();
        let sums = d1_sums(&top);
        Ok(MirrorOut {
            no_asks: b.no_asks_best_first().into_iter().map(lv).collect(),
            no_bids: b.no_bids_best_first().into_iter().map(lv).collect(),
            best_bid: top.best_bid.map(lv),
            best_ask: top.best_ask.map(lv),
            spread: match (top.best_bid, top.best_ask) {
                (Some((bp, _)), Some((ap, _))) => Some(d(ap.get() - bp.get())),
                _ => None,
            },
            d1_long: sums.map(|s| d(s.0)),
            d1_short: sums.map(|s| d(s.1)),
            crossed: canary(&b),
        })
    })
}
```

Create `crates/arb-wasm/src/api/payoff.rs`:

```rust
//! `payoff_json`: the §5.7 (b) table; with prices, the gross edge at top of book (09-27 §4.1).
use super::run;
use crate::input::{d, price};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use arb_core::payoff;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceIn {
    bid: String,
    ask: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayoffIn {
    kind: String,
    s: usize,
    other_listed: bool,
    prices: Option<Vec<PriceIn>>,
}

#[derive(Serialize)]
struct PayoffOut {
    rows: Vec<String>,
    cols: Vec<String>,
    possible: Vec<bool>,
    cells: Vec<Vec<Option<String>>>,
    basket: Vec<Option<String>>,
    guaranteed: String,
    broken: Vec<bool>,
    top_edge_gross: Option<String>,
}

fn one_share(p: Price) -> Result<Vec<(Price, Shares)>, String> {
    Ok(vec![(
        p,
        Shares::new(Decimal::ONE).map_err(|e| e.to_string())?,
    )])
}

/// The §5.7 (b) payoff table; with prices, the gross edge at top of book (L5).
pub fn payoff_json(input: &str) -> String {
    run(input, |i: PayoffIn| {
        if i.kind != "implication" && !(2..=6).contains(&i.s) {
            return Err("s must be 2 to 6".to_string());
        }
        let m = match i.kind.as_str() {
            "yes" => payoff::yes_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "no" => payoff::no_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "implication" => payoff::implication(),
            k => return Err(format!("unknown kind {k:?}")),
        };
        let top_edge_gross = match &i.prices {
            None => None,
            Some(ps) => {
                let want = if i.kind == "implication" { 2 } else { i.s };
                if ps.len() != want {
                    return Err(format!("expected {want} prices, got {}", ps.len()));
                }
                // One share per leg at the top of book, zero fees: the detector's own rule.
                let mut legs = Vec::with_capacity(ps.len());
                for (n, pr) in ps.iter().enumerate() {
                    let (bid, ask) = (price(&pr.bid)?, price(&pr.ask)?);
                    let buy_no = i.kind == "no" || (i.kind == "implication" && n == 0);
                    let p = if buy_no { bid.complement() } else { ask };
                    legs.push(BuyLeg {
                        levels: one_share(p)?,
                        fee: FeeSchedule::ZERO,
                    });
                }
                let rule = if i.kind == "no" {
                    BasketRule::NestedNo
                } else {
                    BasketRule::AllLegs {
                        payoff: Decimal::ONE,
                    }
                };
                Some(d(walk(&legs, rule, None).top_edge_gross))
            }
        };
        let opt = |x: &Option<Decimal>| x.map(d);
        Ok(PayoffOut {
            rows: m.rows,
            cols: m.cols,
            possible: m.possible,
            cells: m
                .cells
                .iter()
                .map(|r| r.iter().map(opt).collect())
                .collect(),
            basket: m.basket.iter().map(opt).collect(),
            guaranteed: d(m.guaranteed),
            broken: m.broken,
            top_edge_gross,
        })
    })
}
```

Replace `crates/arb-wasm/src/lib.rs` with:

```rust
//! arb-wasm: the public playground's engine (Phase 7: 09-27 spec §4, 7b spec §3.4). All logic is
//! arb-core's and sim's; `api` is the JSON facade (plain Rust, tested natively), and the
//! wasm32-only wrappers below only forward strings.
#![forbid(unsafe_code)]

pub mod api;
pub mod input;

#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    pub fn mirror_json(input: &str) -> String {
        crate::api::mirror_json(input)
    }

    #[wasm_bindgen]
    pub fn payoff_json(input: &str) -> String {
        crate::api::payoff_json(input)
    }
}
```

- [ ] **Step 4: Generate the fixtures and review them**

Run: `UPDATE_FIXTURES=1 cargo test -p arb-wasm --test fixtures && git diff --stat -- crates/arb-wasm/fixtures; git status --short crates/arb-wasm/fixtures`
Then check by hand (these are the outputs the plan's own run produced; fix the code, never the fixture, if one differs):
- `mirror_json.json` `fed-book`: `"no_asks":[{"p":"0.46","s":"5603.74"},{"p":"0.47","s":"85887.16"},{"p":"0.48","s":"42239.53"},{"p":"0.49","s":"28133.09"}]`, `"no_bids"` starting `{"p":"0.45","s":"32356.03"}` and ending `{"p":"0.42","s":"5637.3"}`, `"spread":"0.01","d1_long":"1.01","d1_short":"0.99","crossed":false`.
- `crossed`: `"spread":"-0.01","d1_long":"0.99","d1_short":"1.01","crossed":true`.
- `json-number-price`: `{"error":"not valid input: invalid type: number, expected a string at line 1 column 18"}`; `price-0.00` and `price-1.00`: `price "…" must be 0.01 to 0.99 in steps of 0.01`; `size-1000000` is a model, `size-1000001` and `size-7dp` errors; `nine-levels`: `at most 8 levels per side`; `input-16384` is the `fed-book` model, `input-16385`: `input is larger than 16 KiB`.
- `payoff_json.json`: `no-2-listed-prices` ends `"top_edge_gross":"0.1"}`, `yes-2-prices` `"0.03"`, `implication-prices` `"0.28"`; `yes-s1` and `no-s7`: `s must be 2 to 6`; `yes-s2` and `no-s6` are models; `prices-wrong-count`: `expected 3 prices, got 2`.

- [ ] **Step 5: Run everything**

Run: `cargo test -p arb-wasm && cargo clippy -p arb-wasm --all-targets -- -D warnings && cargo check -p arb-wasm --target wasm32-unknown-unknown`
Expected: `fixtures` 2, `values` 4 (`bracket_bombs_under_16_kib_are_errors_not_crashes` included), `no_floats` 2 pass; no warnings; the wasm32 check `Finished` (it downloads `wasm-bindgen 0.2.129` once).

- [ ] **Step 6: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace && cargo deny check bans advisories`
Expected: every `test result: ok`; `advisories ok, bans ok`.

```bash
git add Cargo.toml Cargo.lock crates/arb-wasm
git commit -m "arb-wasm: JSON facade crate (limits, mirror_json, payoff_json) with golden fixtures; wasm-bindgen =0.2.129"
```

---

### Task 10: arb-wasm — `classify_json`, `depth_walk_json`, `episode_json`

**Files:**
- Create: `crates/arb-wasm/src/api/{classify,depth_walk,episode}.rs`
- Modify: `crates/arb-wasm/src/api.rs`, `crates/arb-wasm/src/lib.rs`
- Modify: `crates/arb-wasm/tests/fixtures.rs`, `crates/arb-wasm/tests/values.rs`
- Create: `crates/arb-wasm/fixtures/{classify,depth_walk,episode}_json.json` (generated)

**Interfaces:**
- Consumes: Task 7 `steps::walk_steps`, Task 8 `classify::{classify, Node}`, `arb_core::episode::{EpisodeTracker::new(gap_ms), on_state(key, ts_ms, Option<Decimal>) -> Vec<EpisodeEvent>}`, `sim::config::Scenario`, `sim::latency::exec_delay_ms(&Scenario, Option<i32>, u64) -> i64`.
- Produces: `arb_wasm::api::{classify_json, depth_walk_json, episode_json}: fn(&str) -> String`; wasm32 exports of the same names.

- [ ] **Step 1: Write the failing tests** — in both `crates/arb-wasm/tests/fixtures.rs` and `crates/arb-wasm/tests/values.rs` replace `use arb_wasm::api::{mirror_json, payoff_json};` with:

```rust
use arb_wasm::api::{classify_json, depth_walk_json, episode_json, mirror_json, payoff_json};
```

Append to `crates/arb-wasm/tests/fixtures.rs`:

```rust
// ---- classify ----
fn leg(status: &str, bid: Option<&str>, ask: Option<&str>, fee: Value) -> Value {
    let side = |p: Option<&str>| p.map_or_else(|| json!([]), |p| json!([lv(p, "10")]));
    json!({ "status": status, "bids": side(bid), "asks": side(ask), "fee": fee })
}

fn live(bid: &str, ask: &str) -> Value {
    leg("live", Some(bid), Some(ask), Value::Null)
}

fn fee7() -> Value {
    json!({ "rate": "0.07", "exponent": 1 })
}

fn classify_in(augmented: bool, legs: Vec<Value>) -> String {
    json!({ "augmented": augmented, "legs": legs }).to_string()
}

fn no_edge_3() -> Vec<Value> {
    vec![
        live("0.40", "0.41"),
        live("0.35", "0.36"),
        live("0.30", "0.31"),
    ]
}

#[test]
fn classify() {
    let fees_on: Vec<Value> = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")]
        .iter()
        .map(|(b, a)| leg("live", Some(b), Some(a), fee7()))
        .collect();
    check(
        "classify_json",
        classify_json,
        vec![
            ("no-edge-3", classify_in(false, no_edge_3())),
            (
                "yes-incomplete-other",
                classify_in(
                    false,
                    vec![
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                        leg("other", None, None, Value::Null),
                    ],
                ),
            ),
            (
                "decided",
                classify_in(
                    false,
                    vec![
                        leg("closed_yes", None, None, Value::Null),
                        live("0.20", "0.30"),
                    ],
                ),
            ),
            (
                "missing-ask",
                classify_in(
                    false,
                    vec![
                        live("0.20", "0.30"),
                        leg("live", Some("0.20"), None, Value::Null),
                    ],
                ),
            ),
            (
                "crossed-leg",
                classify_in(false, vec![live("0.56", "0.55"), live("0.30", "0.31")]),
            ),
            (
                "nothing",
                classify_in(false, vec![live("0.40", "0.52"), live("0.40", "0.52")]),
            ),
            ("fees-on", classify_in(false, fees_on)),
            (
                "augmented-complete",
                classify_in(
                    true,
                    vec![
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                    ],
                ),
            ),
            (
                "six-legs",
                classify_in(false, vec![live("0.10", "0.12"); 6]),
            ),
            (
                "seven-legs",
                classify_in(false, vec![live("0.10", "0.12"); 7]),
            ),
            ("zero-legs", classify_in(false, vec![])),
            (
                "bad-status",
                classify_in(
                    false,
                    vec![leg("won", None, None, Value::Null), live("0.20", "0.30")],
                ),
            ),
            (
                "fee-rate-1",
                classify_in(
                    false,
                    vec![
                        leg(
                            "live",
                            Some("0.40"),
                            Some("0.41"),
                            json!({ "rate": "1", "exponent": 1 }),
                        ),
                        live("0.35", "0.36"),
                    ],
                ),
            ),
        ],
    );
}

// ---- depth walk ----
fn wleg(levels: Value, fee: Value) -> Value {
    json!({ "levels": levels, "fee": fee })
}

fn walk_in(rule: &str, legs: Vec<Value>) -> String {
    json!({ "rule": rule, "legs": legs }).to_string()
}

#[test]
fn depth_walk() {
    let yes_two = vec![
        wleg(json!([lv("0.30", "10"), lv("0.40", "10")]), Value::Null),
        wleg(json!([lv("0.60", "15")]), Value::Null),
    ];
    let no_three = vec![
        wleg(json!([lv("0.60", "10"), lv("0.62", "20")]), Value::Null),
        wleg(json!([lv("0.65", "15"), lv("0.70", "30")]), Value::Null),
        wleg(json!([lv("0.70", "5"), lv("0.72", "40")]), Value::Null),
    ];
    let three_levels = |a: &str, b: &str, c: &str| {
        wleg(json!([lv(a, "10"), lv(b, "10"), lv(c, "10")]), Value::Null)
    };
    check(
        "depth_walk_json",
        depth_walk_json,
        vec![
            ("yes-two-legs", walk_in("yes", yes_two.clone())),
            ("no-nested-three", walk_in("no", no_three)),
            (
                "exactly-one",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([lv("0.50", "10")]), Value::Null),
                        wleg(json!([lv("0.50", "10")]), Value::Null),
                    ],
                ),
            ),
            (
                "fees-on",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([lv("0.30", "10"), lv("0.40", "10")]), fee7()),
                        wleg(json!([lv("0.60", "15")]), fee7()),
                    ],
                ),
            ),
            (
                "zero-depth-leg",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([]), Value::Null),
                        wleg(json!([lv("0.60", "15")]), Value::Null),
                    ],
                ),
            ),
            (
                "four-legs-three-levels",
                walk_in(
                    "yes",
                    vec![
                        three_levels("0.20", "0.21", "0.25"),
                        three_levels("0.22", "0.23", "0.24"),
                        three_levels("0.24", "0.26", "0.28"),
                        three_levels("0.25", "0.27", "0.30"),
                    ],
                ),
            ),
            ("bad-rule", walk_in("maybe", yes_two.clone())),
            (
                "seven-legs",
                walk_in("yes", vec![wleg(json!([lv("0.10", "1")]), Value::Null); 7]),
            ),
        ],
    );
}

// ---- episode ----
fn two(b0: &str, a0: &str, b1: &str, a1: &str) -> Value {
    json!([live(b0, a0), live(b1, a1)])
}

fn open_no() -> Value {
    two("0.60", "0.61", "0.50", "0.51") // NO asks 0.40 + 0.50 = 0.90 < 1
}

fn shut_no() -> Value {
    two("0.40", "0.41", "0.50", "0.51")
}

fn episode_in(
    kind: &str,
    gap_ms: Option<i64>,
    updates: Vec<(i64, Value)>,
    net_ms: i64,
    match_ms: i64,
    honor: bool,
    seconds_delay: Value,
) -> String {
    let updates: Vec<Value> = updates
        .into_iter()
        .map(|(t, legs)| json!({ "t_ms": t, "legs": legs }))
        .collect();
    let mut v = json!({
        "kind": kind,
        "updates": updates,
        "scenario": { "proc_ms": 0, "net_ms": net_ms, "match_ms": match_ms, "honor_seconds_delay": honor },
        "seconds_delay": seconds_delay,
    });
    if let Some(g) = gap_ms {
        v["gap_ms"] = json!(g);
    }
    v.to_string()
}

#[test]
fn episode() {
    let short = || vec![(0, shut_no()), (1000, open_no()), (1200, shut_no())];
    let yes_open = two("0.20", "0.30", "0.30", "0.40"); // YES asks 0.70 < 1
    let shut_yes = two("0.50", "0.60", "0.40", "0.50"); // YES asks 1.10, YES bids 0.90: nothing
    let many = |n: i64| (0..n).map(|i| (i * 100, open_no())).collect::<Vec<_>>();
    check(
        "episode_json",
        episode_json,
        vec![
            (
                "slow-miss",
                episode_in("no", Some(30_000), short(), 145, 150, false, Value::Null),
            ),
            (
                "fast-catch",
                episode_in("no", Some(30_000), short(), 1, 50, false, Value::Null),
            ),
            (
                "never-opens",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (500, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "still-open-at-end",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (1000, open_no()), (5000, open_no())],
                    145,
                    150,
                    false,
                    Value::Null,
                ),
            ),
            (
                "seconds-delay-ignored",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (100, open_no()), (900, shut_no())],
                    1,
                    50,
                    false,
                    json!(1),
                ),
            ),
            (
                "seconds-delay-honored",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (100, open_no()), (900, shut_no())],
                    1,
                    50,
                    true,
                    json!(1),
                ),
            ),
            (
                "yes-kind",
                episode_in(
                    "yes",
                    None,
                    vec![(0, shut_yes.clone()), (100, yes_open), (2000, shut_yes)],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "gap-censor",
                episode_in(
                    "no",
                    Some(100),
                    vec![(0, open_no()), (500, open_no()), (600, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "one-update",
                episode_in("no", None, vec![(0, open_no())], 1, 50, false, Value::Null),
            ),
            (
                "eight-updates",
                episode_in("no", None, many(8), 1, 50, false, Value::Null),
            ),
            (
                "nine-updates",
                episode_in("no", None, many(9), 1, 50, false, Value::Null),
            ),
            (
                "non-increasing-t",
                episode_in(
                    "no",
                    None,
                    vec![(100, open_no()), (100, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "latency-5000",
                episode_in("no", None, short(), 5000, 0, false, Value::Null),
            ),
            (
                "latency-5001",
                episode_in("no", None, short(), 5001, 0, false, Value::Null),
            ),
            (
                "seconds-delay-10",
                episode_in("no", None, short(), 1, 50, true, json!(10)),
            ),
            (
                "seconds-delay-11",
                episode_in("no", None, short(), 1, 50, true, json!(11)),
            ),
        ],
    );
}
```

Append to `crates/arb-wasm/tests/values.rs`:

```rust
#[test]
fn classify_names_its_path() {
    let out = classify_json(
        r#"{"augmented":false,"legs":[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.35","s":"10"}],"asks":[{"p":"0.36","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.30","s":"10"}],"asks":[{"p":"0.31","s":"10"}],"fee":null}]}"#,
    );
    assert!(
        out.starts_with(
            r#"{"path":["no_basket_tried","no_basket_positive"],"kind":"no_basket","realization":"converter""#
        ),
        "{out}"
    );
}

#[test]
fn depth_walk_draws_past_q_star() {
    let out = depth_walk_json(
        r#"{"rule":"yes","legs":[{"levels":[{"p":"0.30","s":"10"},{"p":"0.40","s":"10"}],"fee":null},{"levels":[{"p":"0.60","s":"15"}],"fee":null}]}"#,
    );
    assert_eq!(
        out,
        r#"{"steps":[{"q_from":"0","q_to":"10","cost":"0.9","payoff":"1","profitable":true},{"q_from":"10","q_to":"15","cost":"1","payoff":"1","profitable":false}],"q_star":"10","pi":"1","pi_gross":"1"}"#
    );
    let seven = format!(
        r#"{{"rule":"yes","legs":[{}]}}"#,
        [r#"{"levels":[{"p":"0.10","s":"1"}],"fee":null}"#; 7].join(",")
    );
    assert!(err(&depth_walk_json(&seven)).contains("at most 6 legs"));
}

#[test]
fn a_slow_trader_misses_a_short_episode() {
    let open = r#"[{"status":"live","bids":[{"p":"0.60","s":"10"}],"asks":[{"p":"0.61","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let shut = r#"[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let input = format!(
        r#"{{"kind":"no","gap_ms":30000,"updates":[{{"t_ms":0,"legs":{shut}}},{{"t_ms":1000,"legs":{open}}},{{"t_ms":1200,"legs":{shut}}}],"scenario":{{"proc_ms":0,"net_ms":145,"match_ms":150,"honor_seconds_delay":false}},"seconds_delay":null}}"#
    );
    let out = episode_json(&input);
    assert!(out.contains(r#""open_ms":1000,"close_ms":1200"#), "{out}");
    assert!(
        out.ends_with(r#""t_x":1295,"captured":"0","caught":false}"#),
        "{out}"
    );
}
```

Create the three fixture files with `[]`: `for f in classify depth_walk episode; do printf '[]\n' > crates/arb-wasm/fixtures/${f}_json.json; done`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-wasm`
Expected: FAIL to compile: `unresolved imports `arb_wasm::api::classify_json`, `arb_wasm::api::depth_walk_json`, `arb_wasm::api::episode_json``.

- [ ] **Step 3: Implement** — create `crates/arb-wasm/src/api/classify.rs`:

```rust
//! `classify_json`: one update through the §5.7 (c) tree (09-27 §4.1).
use super::run;
use crate::input::{LegIn, d, group};
use arb_core::classify::{Node, classify};
use arb_core::detect::{Kind, Realization, Signal};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyIn {
    #[serde(default)]
    augmented: bool,
    legs: Vec<LegIn>,
}

#[derive(Serialize)]
struct ClassifyOut {
    path: Vec<&'static str>,
    kind: Option<&'static str>,
    realization: Option<&'static str>,
    net: Option<String>,
    gross: Option<String>,
    q_star: Option<String>,
}

fn node(n: Node) -> &'static str {
    match n {
        Node::Decided => "decided",
        Node::NoBasketTried => "no_basket_tried",
        Node::NoBasketFewLegs => "no_basket_few_legs",
        Node::NoBasketCrossed => "no_basket_crossed",
        Node::NoBasketNotPositive => "no_basket_not_positive",
        Node::NoBasketPositive => "no_basket_positive",
        Node::YesBasketTried => "yes_basket_tried",
        Node::YesBasketNoLive => "yes_basket_no_live",
        Node::YesBasketCrossed => "yes_basket_crossed",
        Node::YesBasketMissingAsk => "yes_basket_missing_ask",
        Node::YesBasketNotPositive => "yes_basket_not_positive",
        Node::YesBasketPositive => "yes_basket_positive",
        Node::Complete => "complete",
        Node::Incomplete => "incomplete",
    }
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::YesBasket => "yes_basket",
        Kind::YesBasketIncomplete => "yes_basket_incomplete",
        Kind::NoBasket => "no_basket",
        Kind::Implication => "implication",
    }
}

fn realization(r: Realization) -> &'static str {
    match r {
        Realization::Settlement => "settlement",
        Realization::Converter => "converter",
        Realization::NotArbitrage => "not_arbitrage",
    }
}

/// One update through the §5.7 (c) tree: the path taken and the detectors' answer.
pub fn classify_json(input: &str) -> String {
    run(input, |i: ClassifyIn| {
        let c = classify(&group(&i.legs, i.augmented)?);
        let s: Option<&Signal> = c.signal.as_ref();
        Ok(ClassifyOut {
            path: c.path.into_iter().map(node).collect(),
            kind: s.map(|s| kind(s.kind)),
            realization: s.map(|s| realization(s.realization)),
            net: s.map(|s| d(s.quote.pnl)),
            gross: s.map(|s| d(s.gross.pnl_gross)),
            q_star: s.map(|s| d(s.quote.q_max)),
        })
    })
}
```

Create `crates/arb-wasm/src/api/depth_walk.rs`:

```rust
//! `depth_walk_json`: the cost staircase past Q* and `walk`'s totals (09-27 §4.1).
use super::run;
use crate::input::{FeeIn, LevelIn, MAX_LEGS, d, fee, levels};
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::steps::walk_steps;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkLegIn {
    levels: Vec<LevelIn>,
    fee: Option<FeeIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkIn {
    rule: String,
    legs: Vec<WalkLegIn>,
}

#[derive(Serialize)]
struct StepOut {
    q_from: String,
    q_to: String,
    cost: String,
    payoff: String,
    profitable: bool,
}

#[derive(Serialize)]
struct WalkOut {
    steps: Vec<StepOut>,
    q_star: String,
    pi: String,
    pi_gross: String,
}

/// The cost staircase, past Q*, and `walk`'s totals.
pub fn depth_walk_json(input: &str) -> String {
    run(input, |i: WalkIn| {
        if i.legs.is_empty() || i.legs.len() > MAX_LEGS {
            return Err(format!("1 to at most {MAX_LEGS} legs"));
        }
        let rule = match i.rule.as_str() {
            "yes" => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
            "no" => BasketRule::NestedNo,
            r => return Err(format!("unknown rule {r:?}")),
        };
        let legs = i
            .legs
            .iter()
            .map(|l| {
                Ok(BuyLeg {
                    levels: levels(&l.levels)?,
                    fee: fee(&l.fee)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let w = walk(&legs, rule, None);
        Ok(WalkOut {
            steps: walk_steps(&legs, rule)
                .into_iter()
                .map(|s| StepOut {
                    q_from: d(s.q_from),
                    q_to: d(s.q_to),
                    cost: d(s.cost_per_basket),
                    payoff: d(s.payoff_per_basket),
                    profitable: s.profitable,
                })
                .collect(),
            q_star: d(w.q_max),
            pi: d(w.pnl),
            pi_gross: d(w.pnl_gross),
        })
    })
}
```

Create `crates/arb-wasm/src/api/episode.rs`:

```rust
//! `episode_json`: the teaching model of one episode and one taker at t_x (09-27 §4.1).
use super::run;
use crate::input::{LegIn, d, group};
use arb_core::classify::classify;
use arb_core::detect::Kind;
use arb_core::episode::{EpisodeEvent, EpisodeTracker};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioIn {
    proc_ms: i64,
    net_ms: i64,
    match_ms: i64,
    honor_seconds_delay: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateIn {
    t_ms: i64,
    legs: Vec<LegIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeIn {
    kind: String,
    #[serde(default = "default_gap_ms")]
    gap_ms: i64,
    updates: Vec<UpdateIn>,
    scenario: ScenarioIn,
    seconds_delay: Option<i32>,
}

/// `config/scanner.toml`'s `gap_ms`.
fn default_gap_ms() -> i64 {
    30_000
}

#[derive(Serialize)]
struct UpdateOut {
    t_ms: i64,
    net: Option<String>,
}

#[derive(Serialize)]
struct EpisodeOut {
    updates: Vec<UpdateOut>,
    open_ms: Option<i64>,
    close_ms: Option<i64>,
    peak_ms: Option<i64>,
    peak_net: Option<String>,
    t_x: Option<i64>,
    captured: Option<String>,
    caught: bool,
}

/// A teaching model (09-27 §4.1): one taker at t_x gets the net Π of the latest update at or
/// before t_x, or 0 once the episode has closed.
pub fn episode_json(input: &str) -> String {
    run(input, |i: EpisodeIn| {
        let want = match i.kind.as_str() {
            "yes" => Kind::YesBasket,
            "no" => Kind::NoBasket,
            k => return Err(format!("unknown kind {k:?}")),
        };
        if !(1..=600_000).contains(&i.gap_ms) {
            return Err("gap_ms must be 1 to 600,000".to_string());
        }
        if !(2..=8).contains(&i.updates.len()) {
            return Err("2 to 8 updates".to_string());
        }
        if i.updates.iter().any(|u| !(0..=600_000).contains(&u.t_ms)) {
            return Err("update times must be 0 to 600,000 ms".to_string());
        }
        if i.updates.windows(2).any(|w| w[1].t_ms <= w[0].t_ms) {
            return Err("update times must strictly increase".to_string());
        }
        let sc = &i.scenario;
        for (n, v) in [
            ("proc_ms", sc.proc_ms),
            ("net_ms", sc.net_ms),
            ("match_ms", sc.match_ms),
        ] {
            if !(0..=5000).contains(&v) {
                return Err(format!("{n} must be 0 to 5,000"));
            }
        }
        if i.seconds_delay.is_some_and(|s| !(0..=10).contains(&s)) {
            return Err("seconds_delay must be 0 to 10".to_string());
        }
        let scenario = sim::config::Scenario {
            name: "playground".to_string(),
            proc_ms: sc.proc_ms,
            net_ms: sc.net_ms,
            match_ms: sc.match_ms,
            honor_seconds_delay: sc.honor_seconds_delay,
            burst: Vec::new(),
        };
        let mut tracker = EpisodeTracker::new(i.gap_ms);
        let mut nets: Vec<Option<Decimal>> = Vec::with_capacity(i.updates.len());
        let (mut open_ms, mut close_ms, mut peak_ms, mut peak_net) = (None, None, None, None);
        for u in &i.updates {
            let c = classify(&group(&u.legs, false)?);
            let net = c.signal.filter(|s| s.kind == want).map(|s| s.quote.pnl);
            nets.push(net);
            for ev in tracker.on_state("e", u.t_ms, net) {
                match ev {
                    EpisodeEvent::Open { ts_ms, pnl, .. } if open_ms.is_none() => {
                        open_ms = Some(ts_ms);
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Peak { ts_ms, pnl, .. } if close_ms.is_none() => {
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Close { ts_ms, .. } if close_ms.is_none() => {
                        close_ms = Some(ts_ms)
                    }
                    _ => {}
                }
            }
        }
        let t_x = open_ms.map(|o| o + sim::latency::exec_delay_ms(&scenario, i.seconds_delay, 0));
        // The one decision here (09-27 §4.1): the latest update at or before t_x; 0 once closed.
        let captured = t_x.map(|t| {
            if close_ms.is_some_and(|c| c <= t) {
                return Decimal::ZERO;
            }
            i.updates
                .iter()
                .zip(&nets)
                .rev()
                .find(|(u, _)| u.t_ms <= t)
                .and_then(|(_, n)| *n)
                .unwrap_or(Decimal::ZERO)
        });
        Ok(EpisodeOut {
            updates: i
                .updates
                .iter()
                .zip(&nets)
                .map(|(u, n)| UpdateOut {
                    t_ms: u.t_ms,
                    net: n.map(d),
                })
                .collect(),
            open_ms,
            close_ms,
            peak_ms,
            peak_net: peak_net.map(d),
            t_x,
            caught: captured.is_some_and(|c| c > Decimal::ZERO),
            captured: captured.map(d),
        })
    })
}
```

In `crates/arb-wasm/src/api.rs` replace the module block with:

```rust
mod classify;
mod depth_walk;
mod episode;
mod mirror;
mod payoff;

pub use classify::classify_json;
pub use depth_walk::depth_walk_json;
pub use episode::episode_json;
pub use mirror::mirror_json;
pub use payoff::payoff_json;
```

In `crates/arb-wasm/src/lib.rs` add, inside `mod bindings` after `payoff_json`:

```rust

    #[wasm_bindgen]
    pub fn classify_json(input: &str) -> String {
        crate::api::classify_json(input)
    }

    #[wasm_bindgen]
    pub fn depth_walk_json(input: &str) -> String {
        crate::api::depth_walk_json(input)
    }

    #[wasm_bindgen]
    pub fn episode_json(input: &str) -> String {
        crate::api::episode_json(input)
    }
```

- [ ] **Step 4: Generate the fixtures and review them**

Run: `UPDATE_FIXTURES=1 cargo test -p arb-wasm --test fixtures && git status --short crates/arb-wasm/fixtures`
Check by hand:
- `classify_json.json`: `no-edge-3` = `{"path":["no_basket_tried","no_basket_positive"],"kind":"no_basket","realization":"converter","net":"0.5","gross":"0.5","q_star":"10"}`; `fees-on` `"net":"0.02575","gross":"0.5"`; `decided` `"path":["decided"]` with every other field `null`; `crossed-leg` `["no_basket_tried","no_basket_crossed","yes_basket_tried","yes_basket_crossed"]`; `augmented-complete` ends in `"incomplete"` with `"kind":"yes_basket_incomplete"`; `six-legs` `"kind":"yes_basket"`; `seven-legs` and `zero-legs`: `1 to at most 6 legs`; `fee-rate-1`: `fee: fee rate must be < 1 (L8 monotonicity), got 1`.
- `depth_walk_json.json`: `yes-two-legs` equals `depth_walk_draws_past_q_star`'s string; `no-nested-three` has five steps, the last `{"q_from":"30","q_to":"45","cost":"1.42","payoff":"1","profitable":false}`, `"q_star":"15","pi":"0.45"`; `exactly-one` one unprofitable step and `"q_star":"0"`; `fees-on` `"pi":"0.685","pi_gross":"1"`; `zero-depth-leg` `{"steps":[],"q_star":"0","pi":"0","pi_gross":"0"}`.
- `episode_json.json`: `slow-miss` `"t_x":1295,"captured":"0","caught":false`; `fast-catch` `"t_x":1051,"captured":"1","caught":true`; `seconds-delay-ignored` `"t_x":151 … "caught":true` and `seconds-delay-honored` `"t_x":1151,"captured":"0","caught":false`; `yes-kind` `"open_ms":100,"close_ms":2000 … "t_x":151,"captured":"3"`; `latency-5000` `"t_x":6000`; `latency-5001`: `net_ms must be 0 to 5,000`; `seconds-delay-11`: `seconds_delay must be 0 to 10`; `nine-updates` and `one-update`: `2 to 8 updates`; `non-increasing-t`: `update times must strictly increase`.

- [ ] **Step 5: Run everything, CI and commit**

Run: `cargo test -p arb-wasm && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && cargo check -p arb-wasm --target wasm32-unknown-unknown && (PG prefix) cargo test --workspace`
Expected: `fixtures` 5, `values` 7 pass; no warnings; wasm32 `Finished`; every `test result: ok`.

```bash
git add crates/arb-wasm
git commit -m "arb-wasm: classify_json, depth_walk_json and episode_json with their fixtures"
```

---

### Task 11: arb-wasm — `basket_json` (section f: one leg misses)

**Files:**
- Create: `crates/arb-wasm/src/api/basket.rs`
- Modify: `crates/arb-wasm/src/api.rs`, `crates/arb-wasm/src/lib.rs`
- Modify: `crates/arb-wasm/tests/fixtures.rs`, `crates/arb-wasm/tests/values.rs`
- Create: `crates/arb-wasm/fixtures/basket_json.json` (generated)

**Interfaces:**
- Consumes: Task 1 `sim::policy::{settle, Settle}` and `walk_cost`; Tasks 2–4 `sim::venue::{FillLeg, basket_buy, unwind_sell, ConvertTerms, basket_cash, OrderFill}`; `sim::venue::bought_levels(&Book, Outcome)`; `sim::consumed::ConsumedLiquidity`; `sim::types::{LegKey, Outcome}`; `arb_core::fill::walk`.
- Produces: `arb_wasm::api::{basket_json: fn(&str) -> String, MAX_BASKET_LEGS: usize = 4}`; wasm32 export `basket_json`.
- The plan, as `sim::policy::decide` for P0/P1 (net quote): the open books' walk (NO: `NestedNo` on the mirrored YES bids; YES: `AllLegs { payoff: 1 }` on the asks) must have `pnl > 0`; `q_open` = the smallest leg fill of that walk must be ≥ `min_order`; `q_request` ≥ `min_order`; then `settle(at, min(q_request, q_open), min_order)` gives `q_target` and each leg's limit, with `at(q)` = per leg `walk_cost(open levels, q, fee, false)`. At t_x: `basket_buy` on the t_x books; each leg with excess > 0: `unwind_sell(Some(unwind book), …, qty = excess, held = that leg's filled, min_order)` at once, one `ConsumedLiquidity` for all; `basket_cash(buy, unwinds, NO ? Some(ConvertTerms { s: legs, q: q_eff, fee_bips }) : None)`.

- [ ] **Step 1: Write the failing tests** — in both test files replace the `use arb_wasm::api::{…};` line with:

```rust
use arb_wasm::api::{
    basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json,
};
```

Append to `crates/arb-wasm/tests/fixtures.rs`:

```rust
// ---- basket (7b §3.4) ----
fn bk(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> Value {
    let side = |v: &[(&str, &str)]| Value::Array(v.iter().map(|(p, s)| lv(p, s)).collect());
    json!({ "bids": side(bids), "asks": side(asks) })
}

fn bleg(open: Value, t_x: Value, unwind: Value, fee: Value) -> Value {
    json!({ "open": open, "t_x": t_x, "unwind": unwind, "fee": fee })
}

/// Section f's default: a 3-leg NO basket, YES bids 0.40/0.35/0.30 (NO asks 0.60/0.65/0.70,
/// Σ 1.95 < 2), YES asks one tick up; 100 deep everywhere; t_x and unwind books = open.
fn f_book(i: usize) -> Value {
    let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
    bk(&[(b, "100")], &[(a, "100")])
}

fn f_legs() -> Vec<Value> {
    (0..3)
        .map(|i| bleg(f_book(i), f_book(i), f_book(i), Value::Null))
        .collect()
}

fn basket_in(kind: &str, legs: Vec<Value>, q: &str, extra: &[(&str, &str)]) -> String {
    let mut v = json!({ "kind": kind, "legs": legs, "q_request": q });
    for (k, x) in extra {
        v[*k] = json!(x);
    }
    v.to_string()
}

/// "The usual burst": leg 1 empty at t_x, and the unwind books 5 ticks against us (for NO the
/// YES asks rise 0.05).
fn burst_legs() -> Vec<Value> {
    let against = [
        bk(&[("0.40", "100")], &[("0.46", "100")]),
        bk(&[("0.35", "100")], &[("0.41", "100")]),
        bk(&[("0.30", "100")], &[("0.36", "100")]),
    ];
    (0..3)
        .map(|i| {
            let tx = if i == 1 {
                bk(&[], &[("0.36", "100")])
            } else {
                f_book(i)
            };
            bleg(f_book(i), tx, against[i].clone(), Value::Null)
        })
        .collect()
}

fn yes_book(b: &str, a: &str) -> Value {
    bk(&[(b, "100")], &[(a, "100")])
}

#[test]
fn basket() {
    let f = f_legs();
    let mut partial = f_legs();
    partial[2]["t_x"] = bk(&[("0.30", "20")], &[("0.31", "100")]); // leg 2: 20 of 50
    let mut below_min = f_legs();
    below_min[0]["t_x"] = bk(&[("0.40", "47")], &[("0.41", "100")]); // excess 3 on legs 1, 2
    let mut unwind_empty = burst_legs();
    for l in &mut unwind_empty {
        l["unwind"] = bk(&[("0.40", "100")], &[]);
    }
    let fees_on: Vec<Value> = (0..3)
        .map(|i| bleg(f_book(i), f_book(i), f_book(i), fee7()))
        .collect();
    let yes: Vec<Value> = [("0.29", "0.30"), ("0.29", "0.30"), ("0.29", "0.30")]
        .iter()
        .map(|(b, a)| bleg(yes_book(b, a), yes_book(b, a), yes_book(b, a), Value::Null))
        .collect();
    let mut yes_empty = yes.clone();
    yes_empty[0]["t_x"] = bk(&[("0.29", "100")], &[]);
    yes_empty[1]["unwind"] = bk(&[("0.20", "100")], &[("0.30", "100")]);
    yes_empty[2]["unwind"] = bk(&[("0.20", "100")], &[("0.30", "100")]);
    let no_edge: Vec<Value> = (0..3)
        .map(|_| {
            bleg(
                yes_book("0.30", "0.31"),
                yes_book("0.30", "0.31"),
                yes_book("0.30", "0.31"),
                Value::Null,
            )
        })
        .collect();
    let thin: Vec<Value> = (0..3)
        .map(|i| {
            let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
            let t = bk(&[(b, "4")], &[(a, "100")]);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let two_legs: Vec<Value> = [("0.60", "0.61"), ("0.50", "0.51")]
        .iter()
        .map(|(b, a)| {
            let t = yes_book(b, a);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let four_legs: Vec<Value> = ["0.30", "0.30", "0.25", "0.25"]
        .iter()
        .map(|b| {
            let t = bk(&[(b, "100")], &[("0.99", "100")]);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let mut five_legs = four_legs.clone();
    five_legs.push(four_legs[0].clone());
    let one_leg = vec![f[0].clone()];
    let mut eight_levels = f_legs();
    eight_levels[0]["unwind"] = json!({ "bids": [], "asks": eight(41) });
    let mut nine_levels = f_legs();
    nine_levels[0]["unwind"] = json!({ "bids": [], "asks": eight(41) });
    nine_levels[0]["unwind"]["asks"]
        .as_array_mut()
        .unwrap()
        .push(lv("0.49", "1"));
    let mut penny = burst_legs();
    penny[0]["unwind"] = bk(&[], &[("0.99", "100")]); // NO sells at 0.01
    let mut penny_bad = burst_legs();
    penny_bad[0]["unwind"] = bk(&[], &[("1.00", "100")]);
    let mut top = burst_legs();
    top[2]["unwind"] = bk(&[], &[("0.01", "100")]); // NO sells at 0.99
    let mut top_bad = burst_legs();
    top_bad[2]["unwind"] = bk(&[], &[("0.00", "100")]);
    let big = |s: &str| -> Vec<Value> {
        (0..3)
            .map(|i| {
                let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
                let t = bk(&[(b, s)], &[(a, s)]);
                bleg(t.clone(), t.clone(), t, Value::Null)
            })
            .collect()
    };
    let default = basket_in("no", f.clone(), "50", &[]);
    check(
        "basket_json",
        basket_json,
        vec![
            ("no-all-fill", default.clone()),
            (
                "no-one-leg-empty-burst",
                basket_in("no", burst_legs(), "50", &[]),
            ),
            ("no-one-leg-partial", basket_in("no", partial, "50", &[])),
            ("no-excess-below-min", basket_in("no", below_min, "50", &[])),
            (
                "no-unwind-book-empty",
                basket_in("no", unwind_empty, "50", &[]),
            ),
            (
                "no-convert-fee-0",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "0")]),
            ),
            (
                "no-convert-fee-50",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "50")]),
            ),
            (
                "no-convert-fee-10000",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "10000")]),
            ),
            (
                "no-convert-fee-10001",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "10001")]),
            ),
            ("no-fees-on", basket_in("no", fees_on, "50", &[])),
            (
                "no-min-order-10",
                basket_in("no", f.clone(), "50", &[("min_order", "10")]),
            ),
            ("yes-all-fill", basket_in("yes", yes.clone(), "50", &[])),
            ("yes-one-leg-empty", basket_in("yes", yes_empty, "50", &[])),
            (
                "yes-convert-fee",
                basket_in("yes", yes, "50", &[("convert_fee_bips", "50")]),
            ),
            ("no-edge-at-open", basket_in("no", no_edge, "50", &[])),
            ("too-thin-at-open", basket_in("no", thin, "50", &[])),
            (
                "q-request-below-min",
                basket_in("no", f.clone(), "4.99", &[]),
            ),
            (
                "q-request-json-number",
                default.replace(r#""q_request":"50""#, r#""q_request":50"#),
            ),
            (
                "q-request-1000000",
                basket_in("no", big("1000000"), "1000000", &[]),
            ),
            (
                "q-request-1000001",
                basket_in("no", big("1000000"), "1000001", &[]),
            ),
            ("one-leg", basket_in("no", one_leg, "50", &[])),
            ("two-legs", basket_in("no", two_legs, "50", &[])),
            ("four-legs", basket_in("no", four_legs, "50", &[])),
            ("five-legs", basket_in("no", five_legs, "50", &[])),
            ("eight-levels", basket_in("no", eight_levels, "50", &[])),
            ("nine-levels", basket_in("no", nine_levels, "50", &[])),
            ("price-0.99", basket_in("no", penny, "50", &[])),
            ("price-1.00", basket_in("no", penny_bad, "50", &[])),
            ("price-0.01", basket_in("no", top, "50", &[])),
            ("price-0.00", basket_in("no", top_bad, "50", &[])),
            ("bad-kind", basket_in("maybe", f.clone(), "50", &[])),
            ("input-16384", pad(&default, 16 * 1024)),
            ("input-16385", pad(&default, 16 * 1024 + 1)),
        ],
    );
}
```

Append to `crates/arb-wasm/tests/values.rs`:

```rust
/// Section f's three NO legs (YES bids 0.40/0.35/0.30, asks a tick up, 100 deep), with leg 1's
/// t_x bid ("" = no bid) and the three unwind books' asks given.
fn f_input(tx1_bids: &str, unwind_asks: [&str; 3]) -> String {
    let book = |b: &str, a: &str| {
        let bids = if b.is_empty() {
            String::new()
        } else {
            format!(r#"{{"p":"{b}","s":"100"}}"#)
        };
        format!(r#"{{"bids":[{bids}],"asks":[{{"p":"{a}","s":"100"}}]}}"#)
    };
    let (b, a) = (["0.40", "0.35", "0.30"], ["0.41", "0.36", "0.31"]);
    let legs: Vec<String> = (0..3)
        .map(|i| {
            let tx = if i == 1 {
                book(tx1_bids, a[i])
            } else {
                book(b[i], a[i])
            };
            format!(
                r#"{{"open":{},"t_x":{tx},"unwind":{},"fee":null}}"#,
                book(b[i], a[i]),
                book(b[i], unwind_asks[i])
            )
        })
        .collect();
    format!(
        r#"{{"kind":"no","legs":[{}],"q_request":"50"}}"#,
        legs.join(",")
    )
}

#[test]
fn the_default_no_basket_fills_and_converts_for_a_small_gain() {
    // NO asks 0.60 + 0.65 + 0.70 = 1.95 per set, 3 legs convert for 2: 50 sets spend 97.5, get 100
    let o = v(&basket_json(&f_input("0.35", ["0.41", "0.36", "0.31"])));
    assert_eq!(
        (o["q_open"].clone(), o["q_target"].clone()),
        ("100".into(), "50".into())
    );
    assert_eq!(o["q_eff"], "50");
    assert_eq!(o["legs"][0]["limit"], "0.6");
    assert_eq!(o["cash"]["spent"], "97.5");
    assert_eq!(o["cash"]["convert_back"], "100");
    assert_eq!(o["cash"]["net_cash"], "2.5");
    assert!(o["value_at_resolution"].is_null());
}

#[test]
fn the_usual_burst_is_caught_and_loses() {
    // leg 1 empty at t_x: q_eff 0, legs 0 and 2 unwind 50 each into YES asks 5 ticks up:
    // NO bids 1 − 0.46 = 0.54 and 1 − 0.36 = 0.64 → back 27 + 32 = 59; spent 30 + 35 = 65
    let o = v(&basket_json(&f_input("", ["0.46", "0.41", "0.36"])));
    assert_eq!(o["q_eff"], "0");
    assert_eq!(
        (
            o["legs"][0]["excess"].clone(),
            o["legs"][1]["excess"].clone()
        ),
        ("50".into(), "0".into())
    );
    assert_eq!(o["legs"][0]["unwind"]["vwap"], "0.54");
    assert_eq!(o["cash"]["unwind_back"], "59");
    assert_eq!(o["cash"]["net_cash"], "-6");
    assert_eq!(o["cash"]["convert_back"], "0");
}

#[test]
fn a_request_below_the_minimum_is_an_error_not_a_clamp() {
    let input = f_input("0.35", ["0.41", "0.36", "0.31"])
        .replace(r#""kind":"no""#, r#""kind":"no","min_order":"60""#);
    assert_eq!(
        err(&basket_json(&input)),
        "q_request must be at least the minimum order of 60 shares"
    );
}

#[test]
fn a_request_past_the_open_depth_plans_q_open_and_sizes_round_like_the_policy() {
    // NO asks 1 − 0.33 = 0.67 and 1 − 0.70 = 0.30: Σ 0.97 < 1, 100 deep on each leg
    let input = |q: &str| {
        let book = |b: &str, a: &str| {
            format!(r#"{{"bids":[{{"p":"{b}","s":"100"}}],"asks":[{{"p":"{a}","s":"100"}}]}}"#)
        };
        let leg = |b: &str, a: &str| {
            let k = book(b, a);
            format!(r#"{{"open":{k},"t_x":{k},"unwind":{k},"fee":null}}"#)
        };
        format!(
            r#"{{"kind":"no","legs":[{},{}],"q_request":"{q}"}}"#,
            leg("0.33", "0.34"),
            leg("0.70", "0.71")
        )
    };
    let past = v(&basket_json(&input("500")));
    assert_eq!(
        (past["q_open"].clone(), past["q_target"].clone()),
        ("100".into(), "100".into()),
        "the policy's min(budget, depth at open): q_open is shown beside it"
    );
    // 0.67 × 7.5 = 5.025 has 3 dp: `settle` rounds the size down to 7 (0.67 × 7 = 4.69)
    let odd = v(&basket_json(&input("7.5")));
    assert_eq!(odd["q_target"], "7");
    assert_eq!(odd["legs"][0]["buy"]["filled"], "7");
}
```

Run `printf '[]\n' > crates/arb-wasm/fixtures/basket_json.json`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p arb-wasm`
Expected: FAIL to compile: `unresolved import `arb_wasm::api::basket_json``.

- [ ] **Step 3: Implement** — create `crates/arb-wasm/src/api/basket.rs`:

```rust
//! `basket_json`: section f, one leg misses (7b §3.4), on the paper sim's own rules.
use super::run;
use crate::input::{BookIn, FeeIn, book, d, decimal, fee, quantity};
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::Price;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sim::consumed::ConsumedLiquidity;
use sim::policy::{Settle, settle, walk_cost};
use sim::types::{LegKey, Outcome};
use sim::venue::{
    ConvertTerms, FillLeg, OrderFill, basket_buy, basket_cash, bought_levels, unwind_sell,
};
use std::collections::HashMap;

pub const MAX_BASKET_LEGS: usize = 4;
const MAX_FEE_BIPS: u32 = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketLegIn {
    open: BookIn,
    t_x: BookIn,
    unwind: BookIn,
    #[serde(default)]
    fee: Option<FeeIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketIn {
    kind: String,
    legs: Vec<BasketLegIn>,
    q_request: String,
    #[serde(default)]
    min_order: Option<String>,
    #[serde(default)]
    convert_fee_bips: Option<String>,
}

#[derive(Serialize)]
struct FillOut {
    requested: String,
    filled: String,
    vwap: Option<String>,
    fees: String,
    levels_used: u32,
}

fn fill_out(requested: Decimal, f: &OrderFill) -> FillOut {
    FillOut {
        requested: d(requested),
        filled: d(f.filled),
        vwap: f.vwap().map(d),
        fees: d(f.fees),
        levels_used: f.levels_used,
    }
}

#[derive(Serialize)]
struct BasketLegOut {
    limit: String,
    buy: FillOut,
    excess: String,
    unwind: FillOut,
}

#[derive(Serialize)]
struct HeldOut {
    qty: String,
    basis: String,
}

#[derive(Serialize)]
struct CashOut {
    spent: String,
    fees: String,
    unwind_back: String,
    convert_back: String,
    net_cash: String,
    held_unsold: Vec<HeldOut>,
}

#[derive(Serialize)]
struct BasketOut {
    kind: &'static str,
    min_order: String,
    q_open: String,
    q_target: String,
    legs: Vec<BasketLegOut>,
    q_eff: String,
    cash: CashOut,
    value_at_resolution: Option<String>,
}

struct BasketLeg {
    key: LegKey,
    fee: FeeSchedule,
    open: Book,
    t_x: Book,
    unwind: Book,
}

/// Section f (7b §3.4): plan at open as the policy does (`sim::policy::settle`), buy at t_x
/// (`sim::venue::basket_buy`), unwind each leg's excess at once (`unwind_sell`), and book the cash
/// (`basket_cash`). Every book is a YES book: a NO buy walks the mirrored YES bids, a NO unwind
/// sells into the mirrored YES asks.
pub fn basket_json(input: &str) -> String {
    run(input, |i: BasketIn| {
        let (outcome, kind_out) = match i.kind.as_str() {
            "yes" => (Outcome::Yes, "yes"),
            "no" => (Outcome::No, "no"),
            k => return Err(format!("unknown kind {k:?}")),
        };
        if !(2..=MAX_BASKET_LEGS).contains(&i.legs.len()) {
            return Err(format!("a basket has 2 to {MAX_BASKET_LEGS} legs"));
        }
        let q_request = quantity(&i.q_request, "q_request")?;
        let min_order = match &i.min_order {
            None => Decimal::from(5),
            Some(s) => quantity(s, "min_order")?,
        };
        let fee_bips = match &i.convert_fee_bips {
            None => Decimal::ZERO,
            Some(s) => decimal(s, "convert_fee_bips")?,
        };
        if fee_bips < Decimal::ZERO || fee_bips > Decimal::from(MAX_FEE_BIPS) {
            return Err("convert_fee_bips must be 0 to 10,000".to_string());
        }
        if outcome == Outcome::Yes && !fee_bips.is_zero() {
            return Err("convert_fee_bips applies to a NO basket only".to_string());
        }
        let legs = i
            .legs
            .iter()
            .enumerate()
            .map(|(n, l)| {
                Ok(BasketLeg {
                    key: LegKey {
                        market_id: format!("m{n}"),
                        yes_token: format!("y{n}"),
                        outcome,
                    },
                    fee: fee(&l.fee)?,
                    open: book(&l.open.bids, &l.open.asks)?,
                    t_x: book(&l.t_x.bids, &l.t_x.asks)?,
                    unwind: book(&l.unwind.bids, &l.unwind.asks)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let too_thin = || format!("the book at open is too thin for {} shares", d(min_order));

        // Plan at open exactly as `sim::policy::decide` does: the net walk's edge and full-set
        // size, then `settle` for the venue-rounded size and each leg's limit.
        let open_levels: Vec<_> = legs
            .iter()
            .map(|l| bought_levels(&l.open, outcome))
            .collect();
        let buy_legs: Vec<BuyLeg> = open_levels
            .iter()
            .zip(&legs)
            .map(|(levels, l)| BuyLeg {
                levels: levels.clone(),
                fee: l.fee,
            })
            .collect();
        let rule = match outcome {
            Outcome::No => BasketRule::NestedNo,
            Outcome::Yes => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
        };
        let quote = walk(&buy_legs, rule, None);
        if quote.pnl <= Decimal::ZERO {
            return Err(
                "the basket has no edge at open after fees, so the policy would not trade"
                    .to_string(),
            );
        }
        let q_open = quote
            .legs
            .iter()
            .map(|l| l.filled)
            .min()
            .unwrap_or(Decimal::ZERO);
        if q_open < min_order {
            return Err(too_thin());
        }
        if q_request < min_order {
            return Err(format!(
                "q_request must be at least the minimum order of {} shares",
                d(min_order)
            ));
        }
        let at = |q: Decimal| -> Option<(Decimal, Vec<Price>)> {
            let mut total = Decimal::ZERO;
            let mut limits = Vec::with_capacity(legs.len());
            for (levels, l) in open_levels.iter().zip(&legs) {
                let (c, w) = walk_cost(levels, q, l.fee, false)?;
                total += c;
                limits.push(w);
            }
            Some((total, limits))
        };
        let (q_target, limits) = match settle(&at, q_request.min(q_open), min_order) {
            Ok(v) => v,
            Err(Settle::TooSmall) => return Err(too_thin()),
            Err(Settle::BadInput) => {
                return Err("the book at open can't be quoted at that size".to_string());
            }
        };
        if q_target <= Decimal::ZERO {
            return Err(too_thin());
        }

        // At t_x: the sim's basket buy, then each leg's excess unwound at once into its unwind
        // book (the live sim waits the account's delay; one consumed ledger, as the sim keeps).
        let fill_legs: Vec<FillLeg> = legs
            .iter()
            .zip(&limits)
            .map(|(l, &limit)| FillLeg {
                key: l.key.clone(),
                fee: l.fee,
                limit,
            })
            .collect();
        let tx: HashMap<String, Book> = legs
            .iter()
            .map(|l| (l.key.yes_token.clone(), l.t_x.clone()))
            .collect();
        let mut consumed = ConsumedLiquidity::default();
        let buy = basket_buy(&tx, &fill_legs, q_target, &mut consumed);
        let mut unwinds = Vec::with_capacity(legs.len());
        let mut requested = Vec::with_capacity(legs.len());
        for ((l, f), &excess) in legs.iter().zip(&buy.per_leg).zip(&buy.excess) {
            if excess > Decimal::ZERO {
                let (r, u) = unwind_sell(
                    Some(&l.unwind),
                    &l.key,
                    l.fee,
                    excess,
                    f.filled,
                    min_order,
                    &mut consumed,
                );
                requested.push(r);
                unwinds.push(u);
            } else {
                requested.push(Decimal::ZERO);
                unwinds.push(OrderFill::default());
            }
        }
        let convert = (outcome == Outcome::No).then_some(ConvertTerms {
            s: legs.len(),
            q: buy.q_eff,
            fee_bips,
        });
        let cash = basket_cash(&buy, &unwinds, convert);
        let legs_out = limits
            .iter()
            .zip(&buy.per_leg)
            .zip(&buy.excess)
            .zip(unwinds.iter().zip(&requested))
            .map(|(((limit, f), &excess), (u, &r))| BasketLegOut {
                limit: d(limit.get()),
                buy: fill_out(q_target, f),
                excess: d(excess),
                unwind: fill_out(r, u),
            })
            .collect();
        Ok(BasketOut {
            kind: kind_out,
            min_order: d(min_order),
            q_open: d(q_open),
            q_target: d(q_target),
            legs: legs_out,
            q_eff: d(buy.q_eff),
            cash: CashOut {
                spent: d(cash.spent),
                fees: d(cash.fees),
                unwind_back: d(cash.unwind_back),
                convert_back: d(cash.convert_back),
                net_cash: d(cash.net_cash),
                held_unsold: cash
                    .held_unsold
                    .iter()
                    .map(|h| HeldOut {
                        qty: d(h.qty),
                        basis: d(h.basis),
                    })
                    .collect(),
            },
            value_at_resolution: (outcome == Outcome::Yes).then(|| d(buy.q_eff)),
        })
    })
}
```

In `crates/arb-wasm/src/api.rs` add `mod basket;` above `mod classify;` and `pub use basket::{MAX_BASKET_LEGS, basket_json};` above `pub use classify::classify_json;`. In `crates/arb-wasm/src/lib.rs` add, inside `mod bindings` after `episode_json`:

```rust

    #[wasm_bindgen]
    pub fn basket_json(input: &str) -> String {
        crate::api::basket_json(input)
    }
```

- [ ] **Step 4: Generate the fixture and review it**

Run: `UPDATE_FIXTURES=1 cargo test -p arb-wasm --test fixtures && git status --short crates/arb-wasm/fixtures`
Check `basket_json.json` by hand:
- `no-all-fill`: `"q_open":"100","q_target":"50"`, limits `"0.6"`, `"0.65"`, `"0.7"`, every leg `"filled":"50"`, `"excess":"0"`, `"q_eff":"50"`, `"cash":{"spent":"97.5","fees":"0","unwind_back":"0","convert_back":"100","net_cash":"2.5",…}`, `"value_at_resolution":null`.
- `no-one-leg-empty-burst`: leg 1 `"filled":"0"`, legs 0 and 2 `"excess":"50"` with unwind `"vwap":"0.54"` and `"0.64"`, `"q_eff":"0"`, `"net_cash":"-6"`.
- `no-one-leg-partial`: leg 2 `"filled":"20"`, legs 0 and 1 `"excess":"30"` sold at `0.59`/`0.64`.
- `no-excess-below-min`: legs 1 and 2 `"excess":"3"`, unwind `"requested":"3","filled":"0"`, `held_unsold` `{"qty":"3",…}` on both.
- `no-unwind-book-empty`: unwinds `"requested":"50","filled":"0"`, `held_unsold` 50 at basis on legs 0 and 2.
- `no-convert-fee-50`: `"convert_back":"99.5"`; `no-convert-fee-10000`: `"convert_back":"0"`; `no-convert-fee-10001`: `convert_fee_bips must be 0 to 10,000`; `yes-convert-fee`: `convert_fee_bips applies to a NO basket only`.
- `no-fees-on`: buy fees `"0.84"`, `"0.79625"`, `"0.735"`; `yes-all-fill`: `"value_at_resolution":"50"`; `yes-one-leg-empty`: leg 0 `"filled":"0"`, legs 1 and 2 sold at `0.2`.
- Errors: `no-edge-at-open` `the basket has no edge at open after fees, so the policy would not trade`; `too-thin-at-open` `the book at open is too thin for 5 shares`; `q-request-below-min` `q_request must be at least the minimum order of 5 shares`; `q-request-json-number` `not valid input: invalid type: integer `50`, expected a string …`; `q-request-1000001` `q_request "1000001" must be above 0 and at most 1,000,000, with at most 6 decimals`; `one-leg` and `five-legs` `a basket has 2 to 4 legs`; `nine-levels` `at most 8 levels per side`; `price-1.00`/`price-0.00` the grid sentence; `input-16385` `input is larger than 16 KiB`. Models: `two-legs`, `four-legs`, `eight-levels`, `price-0.99` (unwind at `0.01`), `price-0.01` (unwind at `0.54` on leg 0), `q-request-1000000`, `input-16384`.

- [ ] **Step 5: Run everything, CI and commit**

Run: `cargo test -p arb-wasm && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check && cargo check -p arb-wasm --target wasm32-unknown-unknown && (PG prefix) cargo test --workspace`
Expected: `fixtures` 6, `values` 11 (`a_request_past_the_open_depth_plans_q_open_and_sizes_round_like_the_policy` included) pass; no warnings; wasm32 `Finished`; every `test result: ok`.

```bash
git add crates/arb-wasm
git commit -m "arb-wasm: basket_json plans with sim::policy::settle, fills with venue::basket_buy/unwind_sell, books with basket_cash"
```

---

### Task 12: arb-wasm — never panics, and the CI `wasm` job

**Files:**
- Create: `crates/arb-wasm/tests/never_panics.rs`
- Modify: `.github/workflows/ci.yml` (a job `wasm` after `rust`)

**Interfaces:**
- Consumes: all six `*_json` functions.
- Produces: CI job `wasm`: the wasm32 check of `sim`, `arb-core`, `arb-wasm` and the native `cargo test -p arb-wasm` built alone (09-27 §4.3: "the native side, built alone"); the `rust` job keeps running it under workspace feature unification and never downloads the target (09-27 §4.2).

- [ ] **Step 1: Write the test** — create `crates/arb-wasm/tests/never_panics.rs`:

```rust
//! 09-27 spec §8: any input up to 16 KiB returns a model or {"error"}, never a panic.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{
    basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json,
};
use proptest::prelude::*;

const ALL: [fn(&str) -> String; 6] = [
    mirror_json,
    payoff_json,
    classify_json,
    depth_walk_json,
    episode_json,
    basket_json,
];

fn level() -> impl Strategy<Value = String> {
    (
        r#"(0\.[0-9]{1,3}|1\.00|0|-0\.5)"#,
        r#"([0-9]{1,8}|0\.0000001|-1)"#,
    )
        .prop_map(|(p, s)| format!(r#"{{"p":"{p}","s":"{s}"}}"#))
}

fn book() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(level(), 0..10),
        prop::collection::vec(level(), 0..10),
    )
        .prop_map(|(b, a)| format!(r#"{{"bids":[{}],"asks":[{}]}}"#, b.join(","), a.join(",")))
}

fn basket() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("no"), Just("yes"), Just("x")],
        prop::collection::vec((book(), book(), book(), any::<bool>()), 0..6),
        r#"([0-9]{1,7}(\.[0-9]{1,7})?|-1)"#,
        prop_oneof![
            Just(""),
            Just(r#","convert_fee_bips":"50""#),
            Just(r#","min_order":"0""#)
        ],
    )
        .prop_map(|(kind, legs, q, extra)| {
            let legs: Vec<String> = legs
                .into_iter()
                .map(|(o, t, u, fee)| {
                    let fee = if fee {
                        r#"{"rate":"0.07","exponent":1}"#
                    } else {
                        "null"
                    };
                    format!(r#"{{"open":{o},"t_x":{t},"unwind":{u},"fee":{fee}}}"#)
                })
                .collect();
            format!(
                r#"{{"kind":"{kind}","legs":[{}],"q_request":"{q}"{extra}}}"#,
                legs.join(",")
            )
        })
}

fn json_ish() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,300}",
        r#"\{"(bids|asks|legs|kind|s|rule|updates|q_request)":\[?\{?"?[a-z0-9.\-]{0,8}"?\]?\}"#,
        book(),
        basket(),
    ]
}

proptest! {
    #[test]
    fn never_panics(s in json_ish()) {
        for f in ALL {
            let out = f(&s);
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            prop_assert!(v.is_object());
        }
    }
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p arb-wasm --test never_panics`
Expected: `1 passed` (256 inputs, every output a JSON object). A panic here is a bug in `input.rs` or an `api/*.rs` module: fix it there.

- [ ] **Step 3: The CI job** — in `.github/workflows/ci.yml`, insert between the `rust` job's last step (`- run: cargo deny check bans advisories`) and `  web:`:

```yaml
  wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: rustup target add wasm32-unknown-unknown
      # 7b §2, §8: sim and arb-core stay buildable for the public playground (no time, fs, threads)
      - run: cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown
      # 09-27 §4.3: the native side of the parity check, built alone (arb-wasm's own feature set)
      - run: cargo test -p arb-wasm
```

- [ ] **Step 4: Run the job's commands locally**

Run: `cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown && cargo test -p arb-wasm 2>&1 | grep -E '^test result' && grep -E '^  [a-z]+:$' .github/workflows/ci.yml`
Expected: `Finished`, six `test result: ok` lines (the lib, fixtures, never_panics, no_floats, values, doc-tests), then `  rust:`, `  wasm:`, `  web:`, `  compose:`.

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

```bash
git add crates/arb-wasm/tests/never_panics.rs .github/workflows/ci.yml
git commit -m "ci: wasm job (wasm32 check of sim, arb-core, arb-wasm; arb-wasm tests built alone); arb-wasm never-panics proptest"
```

---

### Task 13: api — `PublicSnapshot`, built from the dashboard's DTOs, whitelisted and bounded

**Files:**
- Create: `bins/api/src/public.rs`
- Modify: `bins/api/src/lib.rs:17-18` (`pub mod public;` between `pub mod params;` and `pub mod reset;`)

**Interfaces:**
- Consumes: `api::ladder::{LadderDto, RungDto, MeasuredDto}` (`ladder.rs:19-52`, all fields `pub`), `api::episodes::{TodayDto, HourDto}` (`episodes.rs:266-281`), `api::dto::Dec`, `cards::ladder::{BreakEven, Direction}`, `cards::edge::LADDER`.
- Produces: `api::public::{PUBLIC_MAX_CATEGORIES = 20, PUBLIC_MAX_BYTES = 65_536, PublicSnapshot, PublicLadder, PublicMeasured, PublicRung, PublicSweetRow, PublicToday, PublicHour, trim_markets(rows: Vec<PublicSweetRow>, max: usize) -> Vec<PublicSweetRow>, build(ladder: LadderDto, today: TodayDto, written_ms: i64) -> PublicSnapshot, to_bytes(&PublicSnapshot) -> Result<Vec<u8>, String>}`.
- **`PublicSweetRow`'s fields, from the web code** (`lib/sweet.ts` `grid` reads `side`, `dimension`, `fee_bips`, `bucket`, `scenario`, `latency_ms`; `glyphKey` `label`, `mean`; `cellDetail`/`interval` `n_exec`, `clusters`, `mean`, `lo`, `hi`, `label`): `side, scenario, fee_bips, latency_ms, dimension, bucket, label, n_exec, clusters, mean, lo, hi`. **`PublicRung`** = `RungDto` minus `account_id` (`LatencyLadder`/`lib/ladder.ts` read `scenario, latency_ms, extra, n_exec, n_leg_risk, n_missed, gains, leg_risk, settle, unrealized, net` and key rows by `account_id`, which 7b-B §7.2 re-keys by `scenario`). `usText` reads only `measured.latency_ms`, so `host` is dropped.
- `markets` keeps only what the grid draws: side `no`, fee 0, the six LADDER scenarios, dimensions `all` and `category`; at most 20 category buckets by summed `n_exec` (ties by name), trimmed before serializing. Worst case (21 buckets × 6 scenarios, long decimals, 8 rungs, 24 hours): 32,654 bytes. (Unfiltered, the spec's "every scenario, both sides" is 82,483 bytes: over the limit, and never drawn.)

- [ ] **Step 1: Write the failing test** — create `bins/api/src/public.rs` with only its test module (it is replaced in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::episodes::HourDto;
    use crate::ladder::{MeasuredDto, RungDto};
    use cards::ladder::Direction;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};

    /// Every key path of a JSON value; array elements share one `[]` path.
    fn paths(v: &Value, at: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if at.is_empty() {
                        k.clone()
                    } else {
                        format!("{at}.{k}")
                    };
                    out.insert(p.clone());
                    paths(x, &p, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    paths(x, &format!("{at}[]"), out);
                }
            }
            _ => {}
        }
    }

    /// A long decimal, so the size test is a worst case.
    fn dec(s: &str) -> Dec {
        Dec(Decimal::from_str_exact(s).unwrap())
    }

    fn rung(scenario: &str, extra: bool) -> RungDto {
        RungDto {
            account_id: format!("P0-converter/{scenario}"),
            scenario: scenario.to_string(),
            latency_ms: Some(300),
            extra,
            n_exec: 123_456,
            n_leg_risk: 12_345,
            n_missed: 1_234,
            gains: dec("-12345.678901234567"),
            leg_risk: dec("-12345.678901234567"),
            settle: dec("12345.678901234567"),
            unrealized: dec("-12345.678901234567"),
            net: dec("-12345.678901234567"),
        }
    }

    /// A SWEET body row with every field `cards::sweet::SweetRow` has, as the ladder passes it.
    fn sweet(side: &str, scenario: &str, dimension: &str, bucket: &str, n_exec: i64) -> Value {
        json!({
            "side": side, "scenario": scenario, "fee_bips": 0, "latency_ms": 300,
            "dimension": dimension, "bucket": bucket, "label": "inconclusive", "few_trades": false,
            "n_exec": n_exec, "n_filled": 1, "n_missed": 1, "clusters": 12_345, "n_attempts": 1,
            "skipped": {"consumed": 3}, "episodes": 1, "flagged": 0, "on_table": "1",
            "on_table_eligible": "1", "captured": "1", "captured_leftover0": "1",
            "mean_leftover0": "1", "mean": "-0.1234567890123", "lo": "-0.1234567890123",
            "hi": "0.1234567890123", "eff_n": "1", "cost": "1", "annualized": null,
            "annualized_lo": null, "annualized_upper": null, "leftover_unpriced": 0, "pooled": 0,
            "episode_id": "live-9-00000001", "market_id": "0xmarket"
        })
    }

    const SCENARIOS: [&str; 8] = [
        "rung-0",
        "rung-25",
        "colo-idle",
        "rung-100",
        "rung-200",
        "sp-idle",
        "sp-burst",
        "sp-idle-nodelay",
    ];

    fn full(markets: Vec<Value>) -> (LadderDto, TodayDto) {
        let ladder = LadderDto {
            epoch: 12,
            start_ms: Some(1_790_121_600_000),
            as_of_ms: Some(1_790_121_600_000),
            rungs: SCENARIOS
                .iter()
                .map(|s| rung(s, s.starts_with("sp-b")))
                .collect(),
            markets,
            measured: Some(MeasuredDto {
                latency_ms: 153,
                host: "vps-kvm2".to_string(),
            }),
            markets_status: "live",
            break_even: BreakEven::At {
                latency_ms: 140,
                direction: Direction::LosesBeyond,
                not_monotone: true,
            },
        };
        let today = TodayDto {
            day_start_ms: 1_790_121_600_000,
            arb: 123_456,
            no: 123_455,
            yes: 1,
            edge_budget100: Some(dec("12345.678901234567")),
            hourly: (0..24).map(|hour| HourDto { hour, arb: 99_999 }).collect(),
        };
        (ladder, today)
    }

    const WHITELIST: [&str; 48] = [
        "v",
        "written_ms",
        "ladder",
        "ladder.epoch",
        "ladder.start_ms",
        "ladder.as_of_ms",
        "ladder.break_even",
        "ladder.break_even.kind",
        "ladder.break_even.latency_ms",
        "ladder.break_even.direction",
        "ladder.break_even.not_monotone",
        "ladder.measured",
        "ladder.measured.latency_ms",
        "ladder.markets_status",
        "ladder.rungs",
        "ladder.rungs[].scenario",
        "ladder.rungs[].latency_ms",
        "ladder.rungs[].extra",
        "ladder.rungs[].n_exec",
        "ladder.rungs[].n_leg_risk",
        "ladder.rungs[].n_missed",
        "ladder.rungs[].gains",
        "ladder.rungs[].leg_risk",
        "ladder.rungs[].settle",
        "ladder.rungs[].unrealized",
        "ladder.rungs[].net",
        "ladder.markets",
        "ladder.markets[].side",
        "ladder.markets[].scenario",
        "ladder.markets[].fee_bips",
        "ladder.markets[].latency_ms",
        "ladder.markets[].dimension",
        "ladder.markets[].bucket",
        "ladder.markets[].label",
        "ladder.markets[].n_exec",
        "ladder.markets[].clusters",
        "ladder.markets[].mean",
        "ladder.markets[].lo",
        "ladder.markets[].hi",
        "today",
        "today.day_start_ms",
        "today.arb",
        "today.no",
        "today.yes",
        "today.edge_budget100",
        "today.hourly",
        "today.hourly[].hour",
        "today.hourly[].arb",
    ];

    #[test]
    fn every_key_path_is_on_the_whitelist_and_nothing_else_ships() {
        let (ladder, today) = full(vec![sweet("no", "rung-0", "category", "crypto", 5)]);
        let raw = String::from_utf8(to_bytes(&build(ladder, today, 7)).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let mut got = BTreeSet::new();
        paths(&v, "", &mut got);
        let want: BTreeSet<String> = WHITELIST.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(got, want, "a new field ships only once this list names it");
        let text = raw.as_str();
        for gone in [
            "account_id",
            "P0-converter",
            "host",
            "vps-kvm2",
            "episode_id",
            "market_id",
            "skipped",
        ] {
            assert!(!text.contains(gone), "{gone} shipped: {text}");
        }
        assert!(
            text.starts_with(r#"{"v":1,"written_ms":7,"ladder":{"epoch":12,"#),
            "the fixed field order: {text}"
        );
    }

    #[test]
    fn twenty_categories_of_every_scenario_both_sides_and_24_hours_fit_in_64_kib() {
        let mut rows = Vec::new();
        for side in ["no", "yes"] {
            for s in SCENARIOS {
                rows.push(sweet(side, s, "all", "all", 1));
                for c in 0..PUBLIC_MAX_CATEGORIES {
                    rows.push(sweet(side, s, "category", &format!("category-{c:02}"), 1));
                }
            }
        }
        let (ladder, today) = full(rows);
        let snap = build(ladder, today, 1_790_121_600_000);
        assert_eq!(
            snap.ladder.markets.len(),
            6 * 21,
            "what the grid draws: NO side, fee 0, the six LADDER scenarios"
        );
        let bytes = to_bytes(&snap).unwrap();
        println!("worst case: {} bytes", bytes.len());
        assert!(bytes.len() < PUBLIC_MAX_BYTES, "{} bytes", bytes.len());
    }

    #[test]
    fn markets_keep_all_rows_and_the_20_busiest_categories_and_drop_other_dimensions() {
        let mut rows = vec![
            sweet("no", "rung-0", "all", "all", 0),
            sweet("no", "rung-0", "days", "lt1", 999),
        ];
        for c in 0..25 {
            // category-24 is the busiest; category-00..04 are the five left out
            rows.push(sweet(
                "no",
                "rung-0",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
            rows.push(sweet(
                "no",
                "sp-idle",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
        }
        let (ladder, today) = full(rows);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1 + 2 * 20);
        assert_eq!(
            (m[0].dimension.as_str(), m[0].bucket.as_str()),
            ("all", "all")
        );
        let buckets: BTreeSet<&str> = m.iter().skip(1).map(|r| r.bucket.as_str()).collect();
        assert!(!buckets.contains("category-04") && buckets.contains("category-05"));
        assert!(m.iter().all(|r| r.dimension != "days"));
    }

    #[test]
    fn a_file_over_64_kib_is_refused_not_written() {
        let (mut ladder, today) = full(Vec::new());
        ladder.rungs[0].scenario = "x".repeat(PUBLIC_MAX_BYTES);
        let e = to_bytes(&build(ladder, today, 1)).unwrap_err();
        assert!(e.ends_with("bytes, over 64 KiB"), "{e}");
    }

    #[test]
    fn a_sweet_row_that_does_not_parse_is_left_out() {
        let mut bad = sweet("no", "rung-0", "category", "sports", 5);
        bad["n_exec"] = json!("five");
        let mut no_label = sweet("no", "rung-0", "category", "politics", 5);
        no_label.as_object_mut().unwrap().remove("label");
        let (ladder, today) = full(vec![sweet("no", "rung-0", "all", "all", 5), bad, no_label]);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1, "only the good row: {m:?}");
        assert_eq!(m[0].dimension, "all");
    }
}
```

and add `pub mod public;` to `bins/api/src/lib.rs` between `pub mod params;` and `pub mod reset;`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p api --lib public`
Expected: FAIL to compile: `cannot find function `build` in this scope`, `cannot find value `PUBLIC_MAX_BYTES``, … .

- [ ] **Step 3: Implement** — replace `bins/api/src/public.rs` with:

```rust
//! The public snapshot (7b spec §4–§5): the latency ladder and today's counts as one static file
//! that nginx serves at /math-data/public.json, so no public request ever reaches the api. Built
//! from the same load functions as GET /api/paper/ladder and GET /api/today, every
//! PUBLIC_EVERY_S seconds, by one task beside the server. Only typed, whitelisted fields ship: no
//! account, market, token or episode id and no host name.
use crate::dto::Dec;
use crate::episodes::TodayDto;
use crate::ladder::LadderDto;
use cards::edge::LADDER;
use cards::ladder::BreakEven;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Category buckets kept in `ladder.markets`, most executed first (§5.1).
pub const PUBLIC_MAX_CATEGORIES: usize = 20;
/// A bigger file is never written: the run is skipped with a log line (§5.1).
pub const PUBLIC_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
pub struct PublicSnapshot {
    pub v: u32,
    pub written_ms: i64,
    pub ladder: PublicLadder,
    pub today: PublicToday,
}

#[derive(Debug, Serialize)]
pub struct PublicLadder {
    pub epoch: i32,
    pub start_ms: Option<i64>,
    pub as_of_ms: Option<i64>,
    pub break_even: BreakEven,
    pub measured: Option<PublicMeasured>,
    pub markets_status: &'static str,
    pub rungs: Vec<PublicRung>,
    pub markets: Vec<PublicSweetRow>,
}

/// `MeasuredDto` without `host` (it names the VPS).
#[derive(Debug, Serialize)]
pub struct PublicMeasured {
    pub latency_ms: i64,
}

/// `RungDto` without `account_id`: every other field `LatencyLadder` reads.
#[derive(Debug, Serialize)]
pub struct PublicRung {
    pub scenario: String,
    pub latency_ms: Option<i64>,
    pub extra: bool,
    pub n_exec: i64,
    pub n_leg_risk: i64,
    pub n_missed: i64,
    pub gains: Dec,
    pub leg_risk: Dec,
    pub settle: Dec,
    pub unrealized: Dec,
    pub net: Dec,
}

/// One SWEET row as `lib/sweet.ts` (`grid`, `ladderGrid`, `glyphKey`, `cellDetail`, `interval`)
/// and `SweetLadderGrid` read it; every other SWEET field is dropped. Decimals stay strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSweetRow {
    pub side: String,
    pub scenario: String,
    pub fee_bips: i64,
    pub latency_ms: Option<i64>,
    pub dimension: String,
    pub bucket: String,
    pub label: String,
    pub n_exec: i64,
    pub clusters: i64,
    pub mean: Option<String>,
    pub lo: Option<String>,
    pub hi: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicToday {
    pub day_start_ms: i64,
    pub arb: i64,
    pub no: i64,
    pub yes: i64,
    pub edge_budget100: Option<Dec>,
    pub hourly: Vec<PublicHour>,
}

#[derive(Debug, Serialize)]
pub struct PublicHour {
    pub hour: i64,
    pub arb: i64,
}

/// Keep only what `LatencyLadder`'s "which markets" grid draws (`SweetLadderGrid` with side "no";
/// `ladderGrid`'s columns are the LADDER scenarios at fee 0), then the `all` rows and the `max`
/// category buckets with the most executed attempts (summed over the bucket's scenarios; ties by
/// name), in their original order. Other dimensions are dropped.
pub fn trim_markets(rows: Vec<PublicSweetRow>, max: usize) -> Vec<PublicSweetRow> {
    let rows: Vec<PublicSweetRow> = rows
        .into_iter()
        .filter(|r| r.side == "no" && r.fee_bips == 0 && LADDER.contains(&r.scenario.as_str()))
        .collect();
    let mut n_exec: BTreeMap<&str, i64> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.dimension == "category") {
        *n_exec.entry(r.bucket.as_str()).or_default() += r.n_exec;
    }
    let mut ranked: Vec<(&str, i64)> = n_exec.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let keep: BTreeSet<String> = ranked
        .into_iter()
        .take(max)
        .map(|(b, _)| b.to_string())
        .collect();
    rows.into_iter()
        .filter(|r| r.dimension == "all" || (r.dimension == "category" && keep.contains(&r.bucket)))
        .collect()
}

/// The snapshot from the dashboard's own DTOs: the dropped fields never enter it.
pub fn build(ladder: LadderDto, today: TodayDto, written_ms: i64) -> PublicSnapshot {
    let mut bad = 0usize;
    let rows: Vec<PublicSweetRow> = ladder
        .markets
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<PublicSweetRow>(v) {
            Ok(r) => Some(r),
            Err(_) => {
                bad += 1;
                None
            }
        })
        .collect();
    if bad > 0 {
        eprintln!("api: public snapshot: {bad} SWEET rows did not parse; left out");
    }
    PublicSnapshot {
        v: 1,
        written_ms,
        ladder: PublicLadder {
            epoch: ladder.epoch,
            start_ms: ladder.start_ms,
            as_of_ms: ladder.as_of_ms,
            break_even: ladder.break_even,
            measured: ladder.measured.map(|m| PublicMeasured {
                latency_ms: m.latency_ms,
            }),
            markets_status: ladder.markets_status,
            rungs: ladder
                .rungs
                .into_iter()
                .map(|r| PublicRung {
                    scenario: r.scenario,
                    latency_ms: r.latency_ms,
                    extra: r.extra,
                    n_exec: r.n_exec,
                    n_leg_risk: r.n_leg_risk,
                    n_missed: r.n_missed,
                    gains: r.gains,
                    leg_risk: r.leg_risk,
                    settle: r.settle,
                    unrealized: r.unrealized,
                    net: r.net,
                })
                .collect(),
            markets: trim_markets(rows, PUBLIC_MAX_CATEGORIES),
        },
        today: PublicToday {
            day_start_ms: today.day_start_ms,
            arb: today.arb,
            no: today.no,
            yes: today.yes,
            edge_budget100: today.edge_budget100,
            hourly: today
                .hourly
                .into_iter()
                .map(|h| PublicHour {
                    hour: h.hour,
                    arb: h.arb,
                })
                .collect(),
        },
    }
}

/// The file's bytes, or why the run is skipped (over `PUBLIC_MAX_BYTES`).
pub fn to_bytes(snap: &PublicSnapshot) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(snap).map_err(|e| format!("serialize: {e}"))?;
    if bytes.len() > PUBLIC_MAX_BYTES {
        return Err(format!("{} bytes, over 64 KiB", bytes.len()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episodes::HourDto;
    use crate::ladder::{MeasuredDto, RungDto};
    use cards::ladder::Direction;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};

    /// Every key path of a JSON value; array elements share one `[]` path.
    fn paths(v: &Value, at: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if at.is_empty() {
                        k.clone()
                    } else {
                        format!("{at}.{k}")
                    };
                    out.insert(p.clone());
                    paths(x, &p, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    paths(x, &format!("{at}[]"), out);
                }
            }
            _ => {}
        }
    }

    /// A long decimal, so the size test is a worst case.
    fn dec(s: &str) -> Dec {
        Dec(Decimal::from_str_exact(s).unwrap())
    }

    fn rung(scenario: &str, extra: bool) -> RungDto {
        RungDto {
            account_id: format!("P0-converter/{scenario}"),
            scenario: scenario.to_string(),
            latency_ms: Some(300),
            extra,
            n_exec: 123_456,
            n_leg_risk: 12_345,
            n_missed: 1_234,
            gains: dec("-12345.678901234567"),
            leg_risk: dec("-12345.678901234567"),
            settle: dec("12345.678901234567"),
            unrealized: dec("-12345.678901234567"),
            net: dec("-12345.678901234567"),
        }
    }

    /// A SWEET body row with every field `cards::sweet::SweetRow` has, as the ladder passes it.
    fn sweet(side: &str, scenario: &str, dimension: &str, bucket: &str, n_exec: i64) -> Value {
        json!({
            "side": side, "scenario": scenario, "fee_bips": 0, "latency_ms": 300,
            "dimension": dimension, "bucket": bucket, "label": "inconclusive", "few_trades": false,
            "n_exec": n_exec, "n_filled": 1, "n_missed": 1, "clusters": 12_345, "n_attempts": 1,
            "skipped": {"consumed": 3}, "episodes": 1, "flagged": 0, "on_table": "1",
            "on_table_eligible": "1", "captured": "1", "captured_leftover0": "1",
            "mean_leftover0": "1", "mean": "-0.1234567890123", "lo": "-0.1234567890123",
            "hi": "0.1234567890123", "eff_n": "1", "cost": "1", "annualized": null,
            "annualized_lo": null, "annualized_upper": null, "leftover_unpriced": 0, "pooled": 0,
            "episode_id": "live-9-00000001", "market_id": "0xmarket"
        })
    }

    const SCENARIOS: [&str; 8] = [
        "rung-0",
        "rung-25",
        "colo-idle",
        "rung-100",
        "rung-200",
        "sp-idle",
        "sp-burst",
        "sp-idle-nodelay",
    ];

    fn full(markets: Vec<Value>) -> (LadderDto, TodayDto) {
        let ladder = LadderDto {
            epoch: 12,
            start_ms: Some(1_790_121_600_000),
            as_of_ms: Some(1_790_121_600_000),
            rungs: SCENARIOS
                .iter()
                .map(|s| rung(s, s.starts_with("sp-b")))
                .collect(),
            markets,
            measured: Some(MeasuredDto {
                latency_ms: 153,
                host: "vps-kvm2".to_string(),
            }),
            markets_status: "live",
            break_even: BreakEven::At {
                latency_ms: 140,
                direction: Direction::LosesBeyond,
                not_monotone: true,
            },
        };
        let today = TodayDto {
            day_start_ms: 1_790_121_600_000,
            arb: 123_456,
            no: 123_455,
            yes: 1,
            edge_budget100: Some(dec("12345.678901234567")),
            hourly: (0..24).map(|hour| HourDto { hour, arb: 99_999 }).collect(),
        };
        (ladder, today)
    }

    const WHITELIST: [&str; 48] = [
        "v",
        "written_ms",
        "ladder",
        "ladder.epoch",
        "ladder.start_ms",
        "ladder.as_of_ms",
        "ladder.break_even",
        "ladder.break_even.kind",
        "ladder.break_even.latency_ms",
        "ladder.break_even.direction",
        "ladder.break_even.not_monotone",
        "ladder.measured",
        "ladder.measured.latency_ms",
        "ladder.markets_status",
        "ladder.rungs",
        "ladder.rungs[].scenario",
        "ladder.rungs[].latency_ms",
        "ladder.rungs[].extra",
        "ladder.rungs[].n_exec",
        "ladder.rungs[].n_leg_risk",
        "ladder.rungs[].n_missed",
        "ladder.rungs[].gains",
        "ladder.rungs[].leg_risk",
        "ladder.rungs[].settle",
        "ladder.rungs[].unrealized",
        "ladder.rungs[].net",
        "ladder.markets",
        "ladder.markets[].side",
        "ladder.markets[].scenario",
        "ladder.markets[].fee_bips",
        "ladder.markets[].latency_ms",
        "ladder.markets[].dimension",
        "ladder.markets[].bucket",
        "ladder.markets[].label",
        "ladder.markets[].n_exec",
        "ladder.markets[].clusters",
        "ladder.markets[].mean",
        "ladder.markets[].lo",
        "ladder.markets[].hi",
        "today",
        "today.day_start_ms",
        "today.arb",
        "today.no",
        "today.yes",
        "today.edge_budget100",
        "today.hourly",
        "today.hourly[].hour",
        "today.hourly[].arb",
    ];

    #[test]
    fn every_key_path_is_on_the_whitelist_and_nothing_else_ships() {
        let (ladder, today) = full(vec![sweet("no", "rung-0", "category", "crypto", 5)]);
        let raw = String::from_utf8(to_bytes(&build(ladder, today, 7)).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let mut got = BTreeSet::new();
        paths(&v, "", &mut got);
        let want: BTreeSet<String> = WHITELIST.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(got, want, "a new field ships only once this list names it");
        let text = raw.as_str();
        for gone in [
            "account_id",
            "P0-converter",
            "host",
            "vps-kvm2",
            "episode_id",
            "market_id",
            "skipped",
        ] {
            assert!(!text.contains(gone), "{gone} shipped: {text}");
        }
        assert!(
            text.starts_with(r#"{"v":1,"written_ms":7,"ladder":{"epoch":12,"#),
            "the fixed field order: {text}"
        );
    }

    #[test]
    fn twenty_categories_of_every_scenario_both_sides_and_24_hours_fit_in_64_kib() {
        let mut rows = Vec::new();
        for side in ["no", "yes"] {
            for s in SCENARIOS {
                rows.push(sweet(side, s, "all", "all", 1));
                for c in 0..PUBLIC_MAX_CATEGORIES {
                    rows.push(sweet(side, s, "category", &format!("category-{c:02}"), 1));
                }
            }
        }
        let (ladder, today) = full(rows);
        let snap = build(ladder, today, 1_790_121_600_000);
        assert_eq!(
            snap.ladder.markets.len(),
            6 * 21,
            "what the grid draws: NO side, fee 0, the six LADDER scenarios"
        );
        let bytes = to_bytes(&snap).unwrap();
        println!("worst case: {} bytes", bytes.len());
        assert!(bytes.len() < PUBLIC_MAX_BYTES, "{} bytes", bytes.len());
    }

    #[test]
    fn markets_keep_all_rows_and_the_20_busiest_categories_and_drop_other_dimensions() {
        let mut rows = vec![
            sweet("no", "rung-0", "all", "all", 0),
            sweet("no", "rung-0", "days", "lt1", 999),
        ];
        for c in 0..25 {
            // category-24 is the busiest; category-00..04 are the five left out
            rows.push(sweet(
                "no",
                "rung-0",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
            rows.push(sweet(
                "no",
                "sp-idle",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
        }
        let (ladder, today) = full(rows);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1 + 2 * 20);
        assert_eq!(
            (m[0].dimension.as_str(), m[0].bucket.as_str()),
            ("all", "all")
        );
        let buckets: BTreeSet<&str> = m.iter().skip(1).map(|r| r.bucket.as_str()).collect();
        assert!(!buckets.contains("category-04") && buckets.contains("category-05"));
        assert!(m.iter().all(|r| r.dimension != "days"));
    }

    #[test]
    fn a_file_over_64_kib_is_refused_not_written() {
        let (mut ladder, today) = full(Vec::new());
        ladder.rungs[0].scenario = "x".repeat(PUBLIC_MAX_BYTES);
        let e = to_bytes(&build(ladder, today, 1)).unwrap_err();
        assert!(e.ends_with("bytes, over 64 KiB"), "{e}");
    }

    #[test]
    fn a_sweet_row_that_does_not_parse_is_left_out() {
        let mut bad = sweet("no", "rung-0", "category", "sports", 5);
        bad["n_exec"] = json!("five");
        let mut no_label = sweet("no", "rung-0", "category", "politics", 5);
        no_label.as_object_mut().unwrap().remove("label");
        let (ladder, today) = full(vec![sweet("no", "rung-0", "all", "all", 5), bad, no_label]);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1, "only the good row: {m:?}");
        assert_eq!(m[0].dimension, "all");
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p api --lib public -- --nocapture 2>&1 | grep -E 'worst case|test result'`
Expected: `worst case: 32654 bytes` (± a few bytes), `test result: ok. 5 passed`.

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test -p api`
Expected: no warnings (the public types are `pub`, so nothing is dead); every `test result: ok`.

```bash
git add bins/api/src/public.rs bins/api/src/lib.rs
git commit -m "api: PublicSnapshot from the ladder and today DTOs (typed whitelist, NO/fee-0/LADDER markets, 20 categories, 64 KiB cap)"
```

---

### Task 14: api — the atomic write and `PUBLIC_DIR` / `PUBLIC_EVERY_S`

**Files:**
- Modify: `bins/api/src/config.rs` (fields, `Debug`, `from_lookup`, a test)
- Modify: `bins/api/src/public.rs` (imports, three constants, the write functions, `WriterPlan`, `writer_plan`, module `write_tests`)

**Interfaces:**
- Consumes: `api::config::Config::from_lookup(impl Fn(&str) -> Option<String>) -> anyhow::Result<Config>`.
- Produces: `Config { …, pub public_dir: Option<String>, pub public_every_s: u32 }` (`PUBLIC_DIR`: unset or empty → `None`; `PUBLIC_EVERY_S`: default 300, 10..=3600); `api::public::{PUBLIC_FILE = "public.json", PUBLIC_TMP = "public.json.tmp", PUBLIC_FIRST_S = 30, write_atomic_with(dir: &Path, bytes: &[u8], before_rename: impl FnOnce(&Path) -> std::io::Result<()>) -> std::io::Result<()>, write_atomic(dir: &Path, bytes: &[u8]) -> std::io::Result<()>, WriterPlan { pub dir: PathBuf, pub first: Duration, pub every: Duration }, writer_plan(&Config) -> Option<WriterPlan>}`.
- The write: `public.json.tmp` opened write+create+truncate (unix mode `0o644`), `write_all`, `sync_all`; `before_rename(&tmp)`; `rename` over `public.json`; unix: `File::open(dir)?.sync_all()` (Windows can't open a directory as a file; the PC tests skip that line).

- [ ] **Step 1: Write the failing tests** — in `bins/api/src/config.rs`, add inside `mod tests`, before `fn the_policies_path_defaults_to_the_baked_copy`:

```rust
    #[test]
    fn the_public_dir_is_optional_and_the_period_is_bounded() {
        let url = (
            "DATABASE_URL",
            "postgres://analyze_ro:pw@postgres/cute_trading",
        );
        let c = cfg(&[url]).unwrap();
        assert_eq!((c.public_dir, c.public_every_s), (None, 300));
        assert_eq!(cfg(&[url, ("PUBLIC_DIR", "")]).unwrap().public_dir, None);
        let c = cfg(&[
            url,
            ("PUBLIC_DIR", "/var/lib/ct-public"),
            ("PUBLIC_EVERY_S", "10"),
        ])
        .unwrap();
        assert_eq!(
            (c.public_dir.as_deref(), c.public_every_s),
            (Some("/var/lib/ct-public"), 10)
        );
        assert!(cfg(&[url, ("PUBLIC_EVERY_S", "9")]).is_err());
        assert!(cfg(&[url, ("PUBLIC_EVERY_S", "3601")]).is_err());
        assert!(cfg(&[url, ("PUBLIC_EVERY_S", "5m")]).is_err());
    }
```

and append to `bins/api/src/public.rs`:

```rust
#[cfg(test)]
mod write_tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ct_public_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn bytes_go_to_the_tmp_file_and_the_visible_file_changes_only_by_the_rename() {
        let d = tmp_dir("rename");
        write_atomic(&d, b"old").unwrap();
        write_atomic_with(&d, b"new", |tmp| {
            assert_eq!(tmp, d.join(PUBLIC_TMP));
            assert_eq!(std::fs::read(tmp).unwrap(), b"new");
            assert_eq!(
                std::fs::read(d.join(PUBLIC_FILE)).unwrap(),
                b"old",
                "not yet visible"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(d.join(PUBLIC_FILE)).unwrap(), b"new");
        assert!(
            !d.join(PUBLIC_TMP).exists(),
            "the rename consumed the tmp file"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failure_before_the_rename_leaves_the_old_file_byte_identical() {
        let d = tmp_dir("fail");
        write_atomic(&d, b"{\"v\":1,\"old\":true}").unwrap();
        let before = std::fs::read(d.join(PUBLIC_FILE)).unwrap();
        let r = write_atomic_with(&d, b"{\"v\":1,\"new\":true}", |_| {
            Err(std::io::Error::other("injected"))
        });
        assert_eq!(r.unwrap_err().to_string(), "injected");
        assert_eq!(std::fs::read(d.join(PUBLIC_FILE)).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_directory_is_an_error_not_a_panic() {
        let d = std::env::temp_dir().join(format!("ct_public_{}_absent", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(write_atomic(&d, b"x").is_err());
    }

    #[test]
    fn a_leftover_longer_tmp_is_truncated_not_reused() {
        let d = tmp_dir("leftover");
        let long = b"{\"v\":1,\"a much longer body from a run that failed\":true}";
        let failed = write_atomic_with(&d, long, |_| Err(std::io::Error::other("injected")));
        assert!(failed.is_err());
        write_atomic(&d, b"{\"v\":1}").unwrap();
        assert_eq!(std::fs::read(d.join(PUBLIC_FILE)).unwrap(), b"{\"v\":1}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_nginx_s_uid() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp_dir("mode");
        write_atomic(&d, b"{}").unwrap();
        let mode = std::fs::metadata(d.join(PUBLIC_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_public_dir_means_no_writer() {
        let cfg = |pairs: &[(&str, &str)]| {
            let m: BTreeMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            crate::config::Config::from_lookup(|k| m.get(k).cloned()).unwrap()
        };
        let url = (
            "DATABASE_URL",
            "postgres://analyze_ro:pw@postgres/cute_trading",
        );
        assert_eq!(writer_plan(&cfg(&[url])), None);
        assert_eq!(writer_plan(&cfg(&[url, ("PUBLIC_DIR", "")])), None);
        assert_eq!(
            writer_plan(&cfg(&[url, ("PUBLIC_DIR", "/var/lib/ct-public")])),
            Some(WriterPlan {
                dir: PathBuf::from("/var/lib/ct-public"),
                first: Duration::from_secs(30),
                every: Duration::from_secs(300),
            })
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p api --lib`
Expected: FAIL to compile: `no field `public_dir` on type `Config``, `cannot find function `write_atomic``, … .

- [ ] **Step 3: Implement**

In `bins/api/src/config.rs`, after the `policies_path` field (the struct's last field) add:

```rust
    /// Phase 7b §4: where the public snapshot is written (compose: /var/lib/ct-public). None when
    /// unset or empty: no snapshot task (local dev and tests).
    pub public_dir: Option<String>,
    /// Seconds between snapshots (PUBLIC_EVERY_S, default 300, 10..=3600).
    pub public_every_s: u32,
```

in `impl fmt::Debug for Config`, after `.field("policies_path", &self.policies_path)`:

```rust
            .field("public_dir", &self.public_dir)
            .field("public_every_s", &self.public_every_s)
```

in `from_lookup`, after the `let policies_path = …;` statement:

```rust
        let public_dir = get("PUBLIC_DIR").filter(|s| !s.is_empty());
        let public_every_s = parse_or(get("PUBLIC_EVERY_S"), 300, "PUBLIC_EVERY_S")?;
        anyhow::ensure!(
            (10..=3_600).contains(&public_every_s),
            "PUBLIC_EVERY_S must be 10..=3600"
        );
```

and in its `Ok(Self { … })` add `public_dir,` and `public_every_s,` after `policies_path,`.

In `bins/api/src/public.rs`, add to the imports (after `use std::collections::{BTreeMap, BTreeSet};`):

```rust
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
```

after `pub const PUBLIC_MAX_BYTES: usize = 64 * 1024;` add:

```rust
pub const PUBLIC_FILE: &str = "public.json";
pub const PUBLIC_TMP: &str = "public.json.tmp";
/// The first run waits this long after start (§4).
pub const PUBLIC_FIRST_S: u64 = 30;
```

and between `to_bytes` and `#[cfg(test)] mod tests` add:

```rust
/// Write `bytes` to `dir/public.json.tmp`, fsync it, call `before_rename` (tests inject a failure
/// or look at the files there), rename it over `dir/public.json`, then fsync the directory (unix).
/// Readers only ever see the old file or the new one; a failure before the rename leaves the old
/// file as it was.
pub fn write_atomic_with(
    dir: &Path,
    bytes: &[u8],
    before_rename: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let tmp = dir.join(PUBLIC_TMP);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644); // nginx (another uid) reads it through a read-only mount (7b-B)
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    before_rename(&tmp)?;
    std::fs::rename(&tmp, dir.join(PUBLIC_FILE))?;
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

pub fn write_atomic(dir: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_with(dir, bytes, |_| Ok(()))
}

/// Where and how often the task writes; no PUBLIC_DIR means no task at all (§4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterPlan {
    pub dir: PathBuf,
    pub first: Duration,
    pub every: Duration,
}

pub fn writer_plan(cfg: &crate::config::Config) -> Option<WriterPlan> {
    cfg.public_dir.as_ref().map(|d| WriterPlan {
        dir: PathBuf::from(d),
        first: Duration::from_secs(PUBLIC_FIRST_S),
        every: Duration::from_secs(u64::from(cfg.public_every_s)),
    })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p api --lib`
Expected: `config` and `public` tests pass: `write_tests` 5 on the PC (`a_leftover_longer_tmp_is_truncated_not_reused` included; the sixth, `the_file_is_readable_by_nginx_s_uid`, is `#[cfg(unix)]` and runs in CI).

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test -p api`
Expected: no warnings; every `test result: ok`.

```bash
git add bins/api/src/config.rs bins/api/src/public.rs
git commit -m "api: public.json written tmp, fsync, rename, dir fsync; PUBLIC_DIR (no task without it) and PUBLIC_EVERY_S"
```

---

### Task 15: api — `load_ladder` / `load_today`, the snapshot task, what never ships

**Files:**
- Modify: `bins/api/src/ladder.rs:121-220` (`ladder` → `ladder` + `load_ladder`)
- Modify: `bins/api/src/episodes.rs:394-396` (`today` → `today` + `load_today`)
- Modify: `bins/api/src/public.rs` (imports, `run_once`, `run_writer`)
- Modify: `bins/api/src/main.rs:61-68`
- Test: `bins/api/tests/public.rs` (create)

**Interfaces:**
- Consumes: Tasks 13–14; `AppState` (`Clone`); `store::Store::{today, paper_ready, paper_epochs, …}` through the handlers' existing code; test helpers `common::{fresh, run, ep, leg, health, app_with_policies, now}` (`bins/api/tests/common/mod.rs`).
- Produces: `pub(crate) async fn api::ladder::load_ladder(st: &AppState) -> Result<LadderDto, ApiError>`, `pub(crate) async fn api::episodes::load_today(st: &AppState) -> Result<TodayDto, ApiError>` (the handlers call them: "the snapshot can't drift from the dashboard"); `pub async fn api::public::run_once(st: &AppState, dir: &Path, written_ms: i64) -> Result<usize, String>` (bytes written, or the reason the old file stays); `pub async fn api::public::run_writer(st: AppState, plan: WriterPlan)` (sleep `plan.first`, then an interval of `plan.every` with `MissedTickBehavior::Delay`, one run at a time, `api: public snapshot skipped: <reason>` on failure, never returns); main spawns it only when `writer_plan(&cfg)` is `Some` and logs `api: public snapshot every <s> s into <dir>`.
- Pool use (7b §4): about 10 queries per run (the ladder's and today's), one run at a time, on the api's pool of 4 (one held by LISTEN).

- [ ] **Step 1: Write the failing test** — create `bins/api/tests/public.rs`:

```rust
//! 7b spec §4–§5: the public snapshot writer against a real database: what never ships, and a
//! failed run keeps the old file.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use api::public::{PUBLIC_FILE, PUBLIC_TMP, run_once};
use serde_json::{Value, json};
use std::path::PathBuf;
use store::*;

/// Epoch 1's start.
const S: i64 = 1_790_121_600_000;
/// Recognisable strings the fixture seeds; none may reach the public file.
const SECRETS: [&str; 7] = [
    "0xmkt-zz-secret",
    "tok-zz-secret",
    "Zzsecret referendum",
    "ev-zz-secret",
    "live-9-zzsecret",
    "P0-converter/",
    "vps-zz-secret",
];

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ct_public_it_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A ladder epoch with two rungs, a ledger naming a market, token and episode, a SWEET body whose
/// rows carry extra fields, a host premise, and today's episodes with titles and leg ids.
async fn seed(owner: &Store) {
    let run = common::run(owner, "live").await;
    for (id, scenario) in [
        ("P0-converter/rung-0", "rung-0"),
        ("P0-converter/sp-idle", "sp-idle"),
    ] {
        sql(owner, format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run}, '{id}', 'P0-converter', '{scenario}', 0, 1000, 100, 'sha')"
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
             qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run}, '{id}', 1, {}, 'sell', 'live-9-zzsecret', '0xmkt-zz-secret', 'tok-zz-secret', 'no', \
             5, 0.4, 2, 0, -1, 1002, NULL)", S + 10
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
             qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run}, '{id}', 2, {}, 'mark', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, 0, 1002, 999)", S + 60_000
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
             max_drawdown, n_positions) VALUES ({run}, '{id}', {}, 1002, 999, -1, 0, 0, 0, 0, 0)", S + 60_000
        ))
        .await;
    }
    sql(owner, format!(
        "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
         VALUES (1, {S}, 'policies', NULL, {run}, 'sha')"
    ))
    .await;
    let row = |bucket: &str| {
        json!({"side": "no", "fee_bips": 0, "scenario": "rung-0", "latency_ms": 0, "dimension": "category",
               "bucket": bucket, "label": "thin", "n_exec": 3, "clusters": 1, "mean": "-0.5", "lo": null, "hi": null,
               "episode_id": "live-9-zzsecret", "market_id": "0xmkt-zz-secret", "event_title": "Zzsecret referendum"})
    };
    let body = json!({"status": "live", "series": {"sweet": {"rows": [row("crypto"), row("sports")]}},
                      "biggest": {"episode_id": "live-9-zzsecret", "usd": "1"}});
    sql(
        owner,
        format!(
            "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('SWEET', '30d', 'k', 'live', '{body}'::jsonb, 1)"
        ),
    )
    .await;
    let mut h = common::health(run, common::now() - 600_000, 0);
    h.rest_rtt_p50_ms = Some(150);
    owner
        .write_batch(&Batch {
            health: vec![h],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .insert_run_premises(&[RunPremiseRow {
            run_id: run,
            area: "host".into(),
            version: "1:vps-zz-secret|100-250".into(),
        }])
        .await
        .unwrap();
    let n = common::now();
    let mut ep = common::ep("live-9-zzsecret", run, n - n.rem_euclid(86_400_000) + 1_000);
    ep.event_id = "ev-zz-secret".into();
    ep.event_title = Some("Zzsecret referendum".into());
    let mut leg = common::leg("live-9-zzsecret", 0, true);
    leg.market_id = "0xmkt-zz-secret".into();
    leg.token = "tok-zz-secret".into();
    owner
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: ep,
                legs: vec![leg],
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn the_snapshot_names_no_market_token_episode_account_or_host() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    seed(&owner).await;
    let (_app, st) = common::app_with_policies(&db).await;
    let d = dir("content");
    let n = run_once(&st, &d, 42).await.unwrap();
    let text = std::fs::read_to_string(d.join(PUBLIC_FILE)).unwrap();
    assert_eq!(text.len(), n);
    for s in SECRETS {
        assert!(!text.contains(s), "{s} shipped: {text}");
    }
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (v["v"].clone(), v["written_ms"].clone()),
        (json!(1), json!(42))
    );
    assert_eq!(v["ladder"]["epoch"], 1);
    assert_eq!(
        v["ladder"]["rungs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["scenario"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["rung-0", "sp-idle"],
        "the rungs are there, by scenario"
    );
    assert_eq!(v["ladder"]["measured"], json!({"latency_ms": 153}));
    assert_eq!(v["ladder"]["markets"].as_array().unwrap().len(), 2);
    assert_eq!(v["today"]["arb"], 1, "today counts the seeded episode");
    assert!(!d.join(PUBLIC_TMP).exists());
    let _ = std::fs::remove_dir_all(&d);
    db.drop(owner).await;
}

#[tokio::test]
async fn a_failed_query_keeps_the_old_file() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (_app, st) = common::app_with_policies(&db).await;
    let d = dir("query");
    run_once(&st, &d, 1).await.unwrap();
    let before = std::fs::read(d.join(PUBLIC_FILE)).unwrap();
    owner.raw_exec("DROP TABLE episodes CASCADE").await.unwrap();
    let e = run_once(&st, &d, 2).await.unwrap_err();
    assert!(e.starts_with("today: "), "{e}");
    assert_eq!(std::fs::read(d.join(PUBLIC_FILE)).unwrap(), before);
    let _ = std::fs::remove_dir_all(&d);
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && (PG prefix) cargo test -p api --test public`
Expected: `pg-up`, then FAIL to compile: `unresolved import `api::public::run_once``.

- [ ] **Step 3: Implement**

In `bins/api/src/ladder.rs` replace the handler's first two lines

```rust
pub async fn ladder(State(st): State<AppState>) -> Result<Json<LadderDto>, ApiError> {
    let now = now_ms();
```

with

```rust
pub async fn ladder(State(st): State<AppState>) -> Result<Json<LadderDto>, ApiError> {
    Ok(Json(load_ladder(&st).await?))
}

/// The ladder as the handler serves it; the public snapshot (`public.rs`) builds from it too, so
/// the two can't drift (7b §4). It needs nothing from the request.
pub(crate) async fn load_ladder(st: &AppState) -> Result<LadderDto, ApiError> {
    let now = now_ms();
```

and at its end replace `Ok(Json(LadderDto {` … `}))` with `Ok(LadderDto {` … `})` (the same eight fields: `epoch, start_ms, as_of_ms, rungs, markets, measured, markets_status, break_even: be`). The body in between is unchanged (it reads `st.store` and `st.scenarios` through the reference).

In `bins/api/src/episodes.rs` replace

```rust
pub async fn today(State(st): State<AppState>) -> Result<Json<TodayDto>, ApiError> {
    Ok(Json(st.store.today(now_ms()).await?.into()))
}
```

with

```rust
pub async fn today(State(st): State<AppState>) -> Result<Json<TodayDto>, ApiError> {
    Ok(Json(load_today(&st).await?))
}

/// Today's counts as the handler serves them; the public snapshot builds from it too (7b §4).
pub(crate) async fn load_today(st: &AppState) -> Result<TodayDto, ApiError> {
    Ok(st.store.today(now_ms()).await?.into())
}
```

In `bins/api/src/public.rs` change the imports `use crate::episodes::TodayDto;` → `use crate::episodes::{TodayDto, load_today};`, `use crate::ladder::LadderDto;` → `use crate::ladder::{LadderDto, load_ladder};`, and add `use crate::{AppState, now_ms};` after them; then between `writer_plan` and `#[cfg(test)] mod tests` add:

```rust
/// One run: load the ladder and today exactly as the dashboard does, build, size-check, and write
/// on a blocking thread (the api's tokio has no `fs` feature). Returns the bytes written, or the
/// reason the old file was left alone.
pub async fn run_once(st: &AppState, dir: &Path, written_ms: i64) -> Result<usize, String> {
    let ladder = load_ladder(st)
        .await
        .map_err(|e| format!("ladder: {e:?}"))?;
    let today = load_today(st).await.map_err(|e| format!("today: {e:?}"))?;
    let bytes = to_bytes(&build(ladder, today, written_ms))?;
    let n = bytes.len();
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || write_atomic(&dir, &bytes))
        .await
        .map_err(|e| format!("write task: {e}"))?
        .map_err(|e| format!("write {PUBLIC_TMP}: {e}"))?;
    Ok(n)
}

/// The task (§4): the first run after `plan.first`, then one every `plan.every`, one at a time. A
/// failed run logs one line and keeps the old file; nothing here panics or stops the server.
pub async fn run_writer(st: AppState, plan: WriterPlan) {
    tokio::time::sleep(plan.first).await;
    let mut tick = tokio::time::interval(plan.every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if let Err(reason) = run_once(&st, &plan.dir, now_ms()).await {
            eprintln!("api: public snapshot skipped: {reason}");
        }
    }
}
```

In `bins/api/src/main.rs` replace

```rust
    api::run(
        api::AppState::new(store)
            .with_reset(reset)
            .with_scenarios(scenarios),
        &cfg.bind,
        std::time::Duration::from_secs(15),
        shutdown(),
    )
    .await
}
```

with

```rust
    let state = api::AppState::new(store)
        .with_reset(reset)
        .with_scenarios(scenarios);
    // Phase 7b §4: the public snapshot task, only with PUBLIC_DIR.
    let writer = api::public::writer_plan(&cfg).map(|plan| {
        eprintln!(
            "api: public snapshot every {} s into {}",
            plan.every.as_secs(),
            plan.dir.display()
        );
        tokio::spawn(api::public::run_writer(state.clone(), plan))
    });
    let served = api::run(
        state,
        &cfg.bind,
        std::time::Duration::from_secs(15),
        shutdown(),
    )
    .await;
    if let Some(w) = writer {
        w.abort();
    }
    served
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `(PG prefix) cargo test -p api`
Expected: every `test result: ok`; `public` 2 passed (`the_snapshot_names_no_market_token_episode_account_or_host`, `a_failed_query_keeps_the_old_file`); `ladder` and `episodes` unchanged and green (the handlers now call the load functions).

- [ ] **Step 5: CI and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && (PG prefix) cargo test --workspace && cargo deny check bans advisories`
Expected: no warnings; every `test result: ok` (`bins/scanner/tests/arch.rs` still green: the api gained no dependency).

```bash
git add bins/api/src/ladder.rs bins/api/src/episodes.rs bins/api/src/public.rs bins/api/src/main.rs bins/api/tests/public.rs
git commit -m "api: load_ladder/load_today shared by the handlers and the public snapshot task (every PUBLIC_EVERY_S, skip and keep the old file on failure)"
```

---

### Task 16: docker and compose — `/var/lib/ct-public` owned by 65532, the `public-data` volume on the api

**Files:**
- Modify: `docker/rust.Dockerfile:24` (the `RUN cargo build …` line), `:32` (after the `/archive` COPY)
- Modify: `compose.web.yml:26` (api environment), the api service (volumes), `:138` (top-level `volumes:` before `networks:`)

**Interfaces:**
- Produces: the image's `/var/lib/ct-public` (empty, `65532:65532`); compose: `api` has `PUBLIC_DIR: /var/lib/ct-public` and `- public-data:/var/lib/ct-public` (read-write; still `read_only: true` otherwise); the named volume `public-data` (`cute-trading_public-data` on the VPS). The api mounts it first, so the new volume copies the directory's owner 65532; plan 7b-B adds the web's read-only mount at `/srv/ct-public-root/math-data`.

- [ ] **Step 1: The failing check**

Run: `grep -c ct-public docker/rust.Dockerfile compose.web.yml`
Expected: `docker/rust.Dockerfile:0`, `compose.web.yml:0`.

- [ ] **Step 2: Implement**

In `docker/rust.Dockerfile` replace line 24:

```dockerfile
RUN cargo build --release --package scanner --package api --package analyze --package compact && mkdir -p /data /archive && chown 65532:65532 /data /archive
```

with:

```dockerfile
# /var/lib/ct-public: the api's public snapshot dir (7b §4), owned by the distroless nonroot uid so
# the named volume mounted there first copies that owner.
RUN cargo build --release --package scanner --package api --package analyze --package compact && mkdir -p /data /archive /var/lib/ct-public && chown 65532:65532 /data /archive /var/lib/ct-public
```

and after `COPY --from=builder --chown=65532:65532 /archive /archive` add:

```dockerfile
COPY --from=builder --chown=65532:65532 /var/lib/ct-public /var/lib/ct-public
```

In `compose.web.yml`, in the `api` service, replace

```yaml
      POLICIES_PATH: /etc/cute-trading/policies.toml
    depends_on:
```

with:

```yaml
      POLICIES_PATH: /etc/cute-trading/policies.toml
      # Phase 7b §4: the public snapshot (public.json, every 5 min), served by nginx as a static file
      PUBLIC_DIR: /var/lib/ct-public
    # The api's only writable mount. It mounts public-data FIRST, so the new volume copies the
    # image's /var/lib/ct-public owner (65532); web mounts it read-only later (7b-B).
    volumes:
      - public-data:/var/lib/ct-public
    depends_on:
```

and replace the file's last block

```yaml
networks:
  ct_radar:
    external: true
```

with:

```yaml
volumes:
  public-data: {}

networks:
  ct_radar:
    external: true
```

- [ ] **Step 3: Run the checks**

Run: `grep -c ct-public docker/rust.Dockerfile compose.web.yml && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok`
Expected: `docker/rust.Dockerfile:3`, `compose.web.yml:3`, `config-ok`. (The image itself is built in the runbook, step 9, and its directory owner checked in step 10.)

- [ ] **Step 4: Commit**

```bash
git add docker/rust.Dockerfile compose.web.yml
git commit -m "docker: /var/lib/ct-public owned by 65532 in the rust image; compose: public-data volume, the api's only writable mount (PUBLIC_DIR)"
```

---

### Task 17: the runbook, step A

**Files:**
- Create: `docs/superpowers/runbooks/phase7b-api-vps.md`

**Interfaces:**
- Consumes: Tasks 1–16; `scripts/replay-identity.sh`, `scripts/disk-budget.{ps1,sh}`; the VPS layout of the 7a runbooks (deploy dir `/home/isoca/projects/cute-trading`, containers `cute-trading-{api,scanner,analyze,compact,postgres}-1`).
- Produces: the owner's deploy procedure for 7b-A.

- [ ] **Step 1: Write the runbook** — create `docs/superpowers/runbooks/phase7b-api-vps.md` with exactly this content:

````markdown
# Phase 7b step A on the VPS: the public snapshot writer (rust image, api, public-data volume)

Note: one image change (the rust image: the sim fill extraction and the api's snapshot task); only `api` is recreated, with the new `public-data` volume mounted at `/var/lib/ct-public`. The web is unchanged and nothing serves the file yet (step B, plan 7b-B).
Note: scanner, analyze and compact keep their running containers. Loading the image moves `cute-trading/rust:dev`, so they run 7b from their next restart (the boot unit, a reboot); that scanner run is the first with sim premise 3, and Edge capture and SWEET count from it anew.
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker.
Note: PC steps in Git Bash (steps 1–5 in the 7b-A worktree before merging, steps 6–12 in the main checkout on the merged commit); VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`), run by the owner.

## Before merging (PC, the 7b-A worktree)

1. The branch is clean, on the commit to merge.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s' && git log -1 --format='%h %s' main
```
→ expected: no status lines, `worktree-phase7b`, the branch's last commit, then `main`'s commit (the base of the identity check).
if not: commit the listed changes, then run this step again.

2. Lints and the wasm32 build.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -1 && cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown 2>&1 | tail -1
```
→ expected: two `Finished` lines and no `warning`/`error` line.
if not: fix on the branch and run again.
**STOP**

3. Every test, the Postgres-backed ones included.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . /c/Users/Isabelly/PycharmProjects/cute-trading/.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test --workspace 2>&1 | grep -E '^test result|SKIPPED|panicked' | sed 's/finished in.*//' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
```
→ expected: `pg-up`, only `<n> test result: ok. …; 0 failed …` lines, then `cargo exit=0`.
if not: no `pg-up` = start the PC Postgres first; any `SKIPPED`/`panicked`/`FAILED`: fix on the branch.
**STOP**

4. A busy raw hour from the VPS: kept if `$HOME/probe/cap` already holds one (7a's cost-gate hour), fetched otherwise.
```bash
K="$(find "$HOME/probe/cap" -name '*.jsonl.zst' 2>/dev/null | head -1)"; if [ -n "$K" ]; then echo "$K" && echo probe-kept; else rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'command -v zstdcat >/dev/null || apt-get install -y zstd >/dev/null; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $(zstdcat "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && find "$HOME/probe/cap" -type f | sort; fi
```
→ expected: a `.jsonl.zst` path and `probe-kept`, or `busy hour: …` and the fetched `.jsonl.zst` plus its `universe-*.json.zst` files.
if not: `busy hour:` empty = no closed hour in 2 days: drop `-mmin -2880` and run again.

5. The replay identity check (spec §3.3): `main` and the branch replay the hour to the same paper fills, execs and ledger rows (two debug builds and two replays of the hour: about 30 minutes for an 800 MB hour).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7b && export PATH="$HOME/.cargo/bin:$PATH" && bash scripts/replay-identity.sh "$HOME/probe/cap" main 2>&1 | tail -7 | tee "$HOME/probe/identity.txt"
```
→ expected: `== base: <main's sha> …`, `frames <N> parse_errors <n> paper_rows <R>`, `== head: <branch sha> …`, the same `frames`/`paper_rows` line, `base rows <R> order <h1> sorted <h2>`, `head rows <R> order <h1> sorted <h2>` (same hashes), then `IDENTICAL`.
if not: `DIFFERENT` with diff lines = the extraction changed a paper row: do not merge, bring `$HOME/probe/identity.txt` to the owner; `no paper rows at all` = a quiet hour: `rm -rf "$HOME/probe"` and redo step 4; an `error:` line = a universe record is missing: `rm -rf "$HOME/probe"` and redo step 4.
**STOP**

## After merging (PC, the main checkout)

6. Merge the 7b-A pull request on GitHub, then bring the main checkout to it; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git checkout -q main && git pull -q --ff-only && git status --short --untracked-files=no && git log -1 --format='%h %s'
```
→ expected: no status lines, then the merge commit (`Merge pull request #… from isabellybaldo/…`).
if not: `Not possible to fast-forward` = local commits on `main`: report, do not build.
**STOP**

7. Line endings: migrations and configs baked into the image must be LF.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git ls-files --eol crates/store/migrations config | grep -v 'w/lf' ; grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
→ expected: only `lf-ok`.
if not: `git checkout -- <each listed file>` and run this step again.

8. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

9. Build the rust image (scanner, api, analyze, compact).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate 2>&1 | tail -2
```
→ expected: the short sha, then the build's last lines without `ERROR`.
if not: a `CRLF found` failure = step 7 was skipped; otherwise read the first `ERROR` line, fix, run again.
**STOP**

10. The image works and carries the snapshot directory owned by 65532.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/scanner cute-trading/rust:dev --help | head -1; docker create --name ct-7b-peek cute-trading/rust:dev >/dev/null && docker cp ct-7b-peek:/var/lib/ct-public - | tar -tvf -; docker rm ct-7b-peek >/dev/null
```
→ expected: `read-only Polymarket arbitrage scanner (paper trading only)`, then one line `drwxr-xr-x 65532/65532 0 … ct-public/`.
if not: `root/root` or no line = the image was built from a tree without Task 16: redo step 9 from the merged commit.
**STOP**

11. Bundle the image and the compose files.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7b" && cd "$HOME/ct-7b" && docker save cute-trading/rust:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && tar -cf "$HOME/ct-7b.tar" image.tar.gz compose.yml compose.web.yml .env.example && cd "$HOME" && rm -rf "$HOME/ct-7b" && ls -l "$HOME/ct-7b.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-7b.tar`, roughly 25–60 MB.
if not: free disk (step 8) and run this step again.

12. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7b.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7b.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

13. The rollback tag, from the running api container's image ID (before step 14's load moves `:dev`).
```bash
cd /home/isoca/projects/cute-trading && docker tag "$(docker inspect -f '{{.Image}}' cute-trading-api-1)" cute-trading/rust:pre7b && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Name}} {{.Image}}' cute-trading-api-1 cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1
```
→ expected: one `sha256:…` ID, then four lines: the api on that ID (scanner, analyze and compact normally on the same one).
if not: `No such object: cute-trading-api-1` = the api isn't running: `docker ps --format '{{.Names}}'` and report; a scanner/analyze/compact line on another ID: write it in Results (those containers are not touched here).
**STOP**

14. Load the bundle (replaces compose.yml, compose.web.yml and .env.example).
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7b.tar && rm /root/ct-7b.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls cute-trading/rust --format '{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/rust:dev`; `dev` on a new ID and `pre7b` on step 13's; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: a truncated tar = re-ship (step 12); over budget = `docker image prune -f` and run the budget again.

15. Compose parses, the api has its directory and volume, and the volume doesn't exist yet.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'public-data' compose.web.yml && grep -c 'PUBLIC_DIR: /var/lib/ct-public' compose.web.yml && docker volume ls -q | grep -c '^cute-trading_public-data$'
```
→ expected: `config-ok`, `3`, `1`, `0` (the api creates the volume at step 17, so it copies the image directory's owner 65532).
if not: `0` on either grep = the bundle's compose.web.yml is old: re-bundle (step 11); last line `1` = a volume exists already: `docker run --rm -v cute-trading_public-data:/v alpine stat -c %u /v` must print `65532`; any other owner: `docker volume rm cute-trading_public-data` and run this step again.
**STOP**

16. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without a `t`, go on to step 17 anyway (step 20 counts the reconnects).
Note: run step 17 within 2 minutes of the `t` (or of the 15-minute limit).

17. Recreate the api only, with the new volume (the web and the scanner are not touched: the 2026-09-28 websocket-drop lesson).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_api_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7b_scanner_started && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 20s api 2>&1 | grep -E 'api:'
```
→ expected: the scanner's start time, `Volume "cute-trading_public-data"  Created`, `Container cute-trading-api-1  Recreated` then `Started`, then `api: public snapshot every 300 s into /var/lib/ct-public` and `api: listening on 0.0.0.0:8081 (read-only except paper reset requests, paper only)`, and no `refusing` or `api: reset pool:` line.
if not: no `public snapshot every` line = `PUBLIC_DIR` is missing (step 15); `refusing to start` = roll back (step 22).

18. Within a minute of the first run (30 s after start): the file, read without the api image, is owned by 65532, world-readable, under 64 KiB, and alone in the volume.
```bash
sleep 40; docker run --rm -v cute-trading_public-data:/v:ro alpine sh -c 'stat -c "%n %u %a %s" /v /v/public.json; ls -A /v; head -c 120 /v/public.json; echo' && docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*' | tee /root/7b_written_1
```
→ expected: `/v 65532 755 …`, `/v/public.json 65532 644 <bytes below 65536>`, `public.json` alone (no `public.json.tmp`), `{"v":1,"written_ms":…,"ladder":{"epoch":…`, then `"written_ms":<ms>`.
if not: `No such file` = no run yet or a failed one: `docker compose -f compose.yml -f compose.web.yml --profile web logs --since 2m api 2>&1 | grep 'public snapshot'` names the reason (`skipped: …`); owner `0` = the volume existed before the api mounted it: roll back (step 22), `docker volume rm cute-trading_public-data`, redo from step 15.
**STOP**

19. (On the PC) The file's key paths are all on the whitelist (spec §5.2).
```bash
ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json" > "$HOME/7b-public.json" && python - "$HOME/7b-public.json" <<'PY'
import json, sys
WL = {"v", "written_ms", "ladder", "ladder.epoch", "ladder.start_ms", "ladder.as_of_ms",
      "ladder.break_even", "ladder.break_even.kind", "ladder.break_even.latency_ms",
      "ladder.break_even.direction", "ladder.break_even.not_monotone", "ladder.measured",
      "ladder.measured.latency_ms", "ladder.markets_status", "ladder.rungs"} | {
      "ladder.rungs[]." + k for k in ["scenario", "latency_ms", "extra", "n_exec", "n_leg_risk",
      "n_missed", "gains", "leg_risk", "settle", "unrealized", "net"]} | {"ladder.markets"} | {
      "ladder.markets[]." + k for k in ["side", "scenario", "fee_bips", "latency_ms", "dimension",
      "bucket", "label", "n_exec", "clusters", "mean", "lo", "hi"]} | {
      "today", "today.day_start_ms", "today.arb", "today.no", "today.yes", "today.edge_budget100",
      "today.hourly", "today.hourly[].hour", "today.hourly[].arb"}
def walk(v, at, out):
    if isinstance(v, dict):
        for k, x in v.items():
            p = f"{at}.{k}" if at else k
            out.add(p)
            walk(x, p, out)
    elif isinstance(v, list):
        for x in v:
            walk(x, at + "[]", out)
raw = open(sys.argv[1], "rb").read()
v = json.loads(raw)
got = set()
walk(v, "", got)
print("v", v["v"], "bytes", len(raw), "rungs", len(v["ladder"]["rungs"]), "markets", len(v["ladder"]["markets"]))
print("extra", sorted(got - WL))
print("WHITELIST OK" if v["v"] == 1 and not (got - WL) and len(raw) < 65536 else "WHITELIST FAIL")
PY
```
→ expected: `v 1 bytes <n> rungs <8 or fewer> markets <n>`, `extra []`, `WHITELIST OK`.
if not: `WHITELIST FAIL` with extra paths = a field ships that the spec forbids: roll back (step 22) and report; nothing serves the file yet, so nothing leaked.
**STOP**

20. No websocket disconnect after the api recreate.
```bash
cd /home/isoca/projects/cute-trading && sleep 60; T0=$(cat /root/7b_api_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7b_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, then `0`.
if not: reconnects > 0: do NOT roll back; write the count in Results.

21. The next run replaces the file about 5 minutes later, and no run was skipped.
```bash
cd /home/isoca/projects/cute-trading && sleep 300; cat /root/7b_written_1; docker run --rm -v cute-trading_public-data:/v:ro alpine cat /v/public.json | grep -o '"written_ms":[0-9]*'; docker compose -f compose.yml -f compose.web.yml --profile web logs --since 7m api 2>&1 | grep -c 'public snapshot skipped'; curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/math-data/public.json
```
→ expected: step 18's `"written_ms":<a>`, then `"written_ms":<b>` with b − a about 300000, then `0`, then `302` or `401` (not served publicly until step B).
if not: the same `written_ms` twice = runs are failing: the `skipped:` lines name the reason (`today: …`/`ladder: …` = a query; `write …` = the volume); `200` = something serves the volume already: report at once.

## Rollback

22. Back to the pre-7b api image (the volume and its file stay; nothing reads them before step B).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/rust:pre7b cute-trading/rust:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && docker image inspect -f '{{.Id}}' cute-trading/rust:pre7b && docker inspect -f '{{.Image}}' cute-trading-api-1
```
→ expected: the same `sha256:…` twice.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api` and report.
Note: the pre-7b api ignores `PUBLIC_DIR` and never writes the mounted volume; with `:dev` back on `pre7b`, scanner, analyze and compact restart on the pre-7b image too.
Note: to remove the volume as well (only if step B is not coming): `docker volume rm cute-trading_public-data` after the api runs without it (a compose file without the mount).

## Results

- Replay identity (step 5): frames, paper rows, hashes, verdict:
- Image directory owner (step 10):
- Containers on another image ID than the api (step 13):
- Quiet minute (step 16), UTC time, or went on after 15 minutes:
- File owner, mode and size (step 18):
- Whitelist check (step 19):
- Reconnects after the api recreate (step 20):
- Second write and skipped runs (step 21):
````

- [ ] **Step 2: Check its format**

Run: `grep -c '^→ expected:' docs/superpowers/runbooks/phase7b-api-vps.md && grep -c '^if not:' docs/superpowers/runbooks/phase7b-api-vps.md && grep -c '^\*\*STOP\*\*' docs/superpowers/runbooks/phase7b-api-vps.md && grep -n 'cat .env\|printenv\|docker compose config\b' docs/superpowers/runbooks/phase7b-api-vps.md | grep -v 'env-file .env.example' ; echo done`
Expected: `22`, `22`, `10`, then only `done` (no line prints `.env` or runs `compose config` without `--env-file .env.example`).

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/runbooks/phase7b-api-vps.md
git commit -m "runbook: phase 7b step A (replay identity before merge, rust image, api with the public-data volume, file checks, rollback)"
```

---

## Contract for 7b-B

Everything plan 7b-B consumes from this plan, exactly. Field order is the serialized order. `Dec` is a JSON **string** holding a normalized decimal (`"0.6"`, `"-6"`, `"0"`; never `"-0"`, never a JSON number); `int` is a JSON integer.

**WASM exports** (`crates/arb-wasm`, `wasm-bindgen =0.2.129`, `wasm-bindgen --target web`; install the CLI with `cargo install --locked --version 0.2.129 wasm-bindgen-cli`): `mirror_json`, `payoff_json`, `classify_json`, `depth_walk_json`, `episode_json`, `basket_json`, each `(input: string) => string`. They never throw: invalid input returns an Error object.

**Fixtures:** `crates/arb-wasm/fixtures/<export>.json` (`mirror_json.json` … `basket_json.json`) = `[{"name": string, "input": string, "output": string}]`; `input` and `output` are the exact strings passed and returned (compare byte for byte). Generated from `crates/arb-wasm/tests/fixtures.rs` with `UPDATE_FIXTURES=1 cargo test -p arb-wasm --test fixtures`.

**Common input shapes** (unknown fields are an error everywhere; a field marked "optional" may be omitted):

```
Level  = {"p": Dec, "s": Dec}            p: 0.01–0.99 on the 0.01 grid; s: > 0, ≤ 1000000, ≤ 6 dp
Book   = {"bids": [Level], "asks": [Level]}          each side optional (→ []), ≤ 8 levels, distinct prices
Fee    = {"rate": Dec, "exponent": int} | null       rate ≥ 0 and < 1; exponent 1–4; null = no fee
LegIn  = {"status": "live"|"placeholder"|"other"|"closed_no"|"closed_yes"|"halted",
          "bids": [Level], "asks": [Level], "fee": Fee}                bids/asks/fee optional
Error  = {"error": string}               one sentence, no newline
```

Any input longer than 16,384 bytes returns `{"error":"input is larger than 16 KiB"}`; unparseable JSON, a JSON number where a Dec is expected, a missing required field or an unknown field returns `{"error":"not valid input: <serde_json message> at line L column C"}`.

**`mirror_json`**

```
in  {"bids": [Level], "asks": [Level]}                                   (both required)
out {"no_asks": [Level], "no_bids": [Level], "best_bid": Level|null, "best_ask": Level|null,
     "spread": Dec|null, "d1_long": Dec|null, "d1_short": Dec|null, "crossed": bool}
```

**`payoff_json`**

```
in  {"kind": "yes"|"no"|"implication", "s": int, "other_listed": bool,
     "prices": [{"bid": Dec, "ask": Dec}] | null}                          prices optional; s 2–6 (ignored for implication);
                                                                           len(prices) = s, or 2 for implication
out {"rows": [string], "cols": [string], "possible": [bool], "cells": [[Dec|null]],
     "basket": [Dec|null], "guaranteed": Dec, "broken": [bool], "top_edge_gross": Dec|null}
```

Rows `YES_A`…, `NO_A`…, or `NO_A`,`YES_B`; cols `A`…, plus `Other` when `other_listed` is false; implication cols `A∧B`, `A∧¬B`, `¬A∧B`, `¬A∧¬B` with `possible` `[true,false,true,true]` and `null` cells in the impossible column.

**`classify_json`**

```
in  {"augmented": bool, "legs": [LegIn]}                                  augmented optional (false); 1–6 legs
out {"path": [Node], "kind": Kind|null, "realization": Realization|null,
     "net": Dec|null, "gross": Dec|null, "q_star": Dec|null}
Node        = "decided" | "no_basket_tried" | "no_basket_few_legs" | "no_basket_crossed" | "no_basket_not_positive"
            | "no_basket_positive" | "yes_basket_tried" | "yes_basket_no_live" | "yes_basket_crossed"
            | "yes_basket_missing_ask" | "yes_basket_not_positive" | "yes_basket_positive" | "complete" | "incomplete"
Kind        = "yes_basket" | "yes_basket_incomplete" | "no_basket" | "implication"
Realization = "settlement" | "converter" | "not_arbitrage"
```


> **Ruling (controller, 2026-09-30, Task 10 review):** `depth_walk_json`'s `levels` are the **asks of the outcome being bought** (NO asks for `rule: "no"`), **cheapest first**; they are NOT YES-book levels. Non-ascending levels return `{"error":"levels must be cheapest first"}` (fixture `descending-levels`). Episode boundary fixtures `tx-at-close` and `tx-at-update` pin t_x == close_ms → captured "0" and t_x == an update's t_ms → that update's net.

**`depth_walk_json`**

```
in  {"rule": "yes"|"no", "legs": [{"levels": [Level], "fee": Fee}]}       1–6 legs; fee optional
out {"steps": [{"q_from": Dec, "q_to": Dec, "cost": Dec, "payoff": Dec, "profitable": bool}],
     "q_star": Dec, "pi": Dec, "pi_gross": Dec}
```

`yes` = every leg in every basket, payoff 1; `no` = the nested NO rule (legs exit as they run out, payoff = active legs − 1). `steps` go past Q* (the unprofitable tail).

**`episode_json`**

```
in  {"kind": "yes"|"no", "gap_ms": int, "updates": [{"t_ms": int, "legs": [LegIn]}],
     "scenario": {"proc_ms": int, "net_ms": int, "match_ms": int, "honor_seconds_delay": bool},
     "seconds_delay": int|null}
     gap_ms optional (30000), 1–600000; updates 2–8, t_ms 0–600000 strictly increasing, 1–6 legs each;
     proc/net/match 0–5000; seconds_delay optional, 0–10
out {"updates": [{"t_ms": int, "net": Dec|null}], "open_ms": int|null, "close_ms": int|null,
     "peak_ms": int|null, "peak_net": Dec|null, "t_x": int|null, "captured": Dec|null, "caught": bool}
```

`t_x = open_ms + proc + net + match + (honor_seconds_delay ? seconds_delay × 1000 : 0)`; `captured` = the net of the latest update with `t_ms ≤ t_x`, `"0"` if the episode closed at or before `t_x`; `caught` = `captured > 0`.

**`basket_json`** (section f)

```
in  {"kind": "yes"|"no",
     "legs": [{"open": Book, "t_x": Book, "unwind": Book, "fee": Fee}],    2–4 legs; fee optional
     "q_request": Dec,                                                    > 0, ≤ 1000000, ≤ 6 dp
     "min_order": Dec,                                                    optional, default "5" (the sim's min_order_shares)
     "convert_fee_bips": Dec}                                             optional, default "0"; 0–10000; non-zero only with kind "no"
out {"kind": "yes"|"no", "min_order": Dec, "q_open": Dec, "q_target": Dec,
     "legs": [{"limit": Dec,
               "buy":    {"requested": Dec, "filled": Dec, "vwap": Dec|null, "fees": Dec, "levels_used": int},
               "excess": Dec,
               "unwind": {"requested": Dec, "filled": Dec, "vwap": Dec|null, "fees": Dec, "levels_used": int}}],
     "q_eff": Dec,
     "cash": {"spent": Dec, "fees": Dec, "unwind_back": Dec, "convert_back": Dec, "net_cash": Dec,
              "held_unsold": [{"qty": Dec, "basis": Dec}]},
     "value_at_resolution": Dec|null}
```

- Every book is a **YES** book. `kind: "no"`: the buy walks the mirrored YES bids (NO ask = 1 − bid) and the unwind sells into the mirrored YES asks (NO bid = 1 − ask); "the unwind book against us by n ticks" for NO = the YES asks raised by n × 0.01. `kind: "yes"`: buys the YES asks, unwinds into the YES bids.
- `limit` and `vwap` are prices of the traded outcome (NO prices for `kind: "no"`). `buy.requested` = `q_target` on every leg. `unwind.requested` = the leg's excess rounded down to cents (0 and an empty fill when the leg has no excess); an unwind below `min_order`, or into an empty book, has `requested > 0` and `filled "0"` (no order went out).
- `q_open` = the full-set size the net walk at open can fill (Q*); `q_target` = `settle(min(q_request, q_open))`, rounded down to the venue's steps; `q_eff` = the smallest leg fill at t_x; `legs[i].excess` = that leg's fill − `q_eff`.
- `cash.net_cash` = `unwind_back + convert_back − spent`, the sum of the ledger's `cash_delta` for these rows: the big number ("caught, and lost $x" when < 0, "filled, $x on paper" otherwise). `held_unsold` (one per leg, in leg order) is cost still held, not cash. `convert_back` = `(legs − 1) × q_eff × (1 − convert_fee_bips / 10000)` for `kind: "no"`, `"0"` for `"yes"` or `q_eff = 0`.
- `value_at_resolution` = `q_eff` for `kind: "yes"` ("if any listed leg wins; not yet cash"), `null` for `"no"`.
- Errors (exact sentences): `unknown kind "<k>"`; `a basket has 2 to 4 legs`; `q_request "<s>" must be above 0 and at most 1,000,000, with at most 6 decimals` (same for `min_order`); `convert_fee_bips "<s>" is not a decimal`; `convert_fee_bips must be 0 to 10,000`; `convert_fee_bips applies to a NO basket only`; `the basket has no edge at open after fees, so the policy would not trade`; `the book at open is too thin for <min_order> shares`; `q_request must be at least the minimum order of <min_order> shares`; `the book at open can't be quoted at that size`; plus the Level/Fee sentences: `price "<p>" must be 0.01 to 0.99 in steps of 0.01`, `size "<s>" must be above 0 and at most 1,000,000, with at most 6 decimals`, `two levels at the same price <p>`, `at most 8 levels per side`, `fee rate "<r>" is not a decimal`, `fee: <FeeError>`.
- The default of section f (`no-all-fill` in the fixture): YES bids 0.40 / 0.35 / 0.30 × 100, asks a tick up, `t_x` and `unwind` books = open, `q_request "50"` → `net_cash "2.5"`. "The usual burst" (`no-one-leg-empty-burst`): leg 1's `t_x` book without bids, every unwind book's asks 5 ticks up → `q_eff "0"`, `net_cash "-6"`.


> **Contract note (controller, 2026-09-30, Task 11 review):** `basket_json`'s `cash.fees` = buy fees + unwind fees + the conversion fee (e.g. `no-convert-fee-50` → fees "0.5"; `no-convert-fee-10000` → fees "100", convert_back "0"). The facade plans exactly as `sim::policy::decide` for open books the page can produce; a crossed open book or an open leg without the side it buys is outside section f's controls.

**`/math-data/public.json`** (`PublicSnapshot`, written by the api every 300 s, at most 64 KiB):

```
{"v": 1, "written_ms": int,
 "ladder": {"epoch": int, "start_ms": int|null, "as_of_ms": int|null,
            "break_even": {"kind": "thin"} | {"kind": "all"} | {"kind": "none"}
                        | {"kind": "at", "latency_ms": int, "direction": "loses_beyond"|"pays_beyond", "not_monotone": true},
                                                                         ("not_monotone" only when true)
            "measured": {"latency_ms": int} | null,
            "markets_status": "live" | "warming" | "under_review" | "missing",
            "rungs": [{"scenario": string, "latency_ms": int|null, "extra": bool, "n_exec": int, "n_leg_risk": int,
                       "n_missed": int, "gains": Dec, "leg_risk": Dec, "settle": Dec, "unrealized": Dec, "net": Dec}],
            "markets": [{"side": "no", "scenario": string, "fee_bips": 0, "latency_ms": int|null,
                         "dimension": "all"|"category", "bucket": string,
                         "label": "candidate"|"negative"|"inconclusive"|"thin"|"not_tested"|"dead_on_paper",
                         "n_exec": int, "clusters": int, "mean": Dec|null, "lo": Dec|null, "hi": Dec|null}]},
 "today": {"day_start_ms": int, "arb": int, "no": int, "yes": int, "edge_budget100": Dec|null,
           "hourly": [{"hour": int, "arb": int}]}}                        (24 hourly entries, hour 0–23 UTC)
```

- `rungs`: the api's `LadderRung` minus `account_id`, LADDER rungs by latency then the extras (key by `scenario`, unique per ladder). `markets`: SWEET's 30d rows the "which markets" grid draws (NO side, fee 0, the six LADDER scenarios), the `all` rows plus at most 20 `category` buckets. `measured` has no `host`: `publicToLadder` must supply the `Ladder` type's `host` (e.g. `""`, never drawn by `usText`) or 7b-B narrows that type.
- The file never contains an account, market, token or episode id, an event title, or a host name (Task 13's whitelist test, Task 15's content test).

## Spec coverage (self-review)

- §3.1 (`payoff`, `d1_sums`, `classify`, `walk_steps` with property tests): Tasks 6–8 (`walk_steps`/`classify` in new files: premises).
- §3.2 extraction (`basket_buy`, `unwind_sell` with the held rounding, `basket_cash` with `ConvertTerms`/`held_unsold`, `Books` moved, `settle` pub; `PaperTrader` keeps `pnl_now` first and the row order): Tasks 1–4.
- §3.3 identity guard (sim and scanner tests unchanged; all fill / one leg empty / partial / unwind below min / missing book / held rounding; the 2–6-leg property test; `basket_cash` against the ledger; the replay identity script): Tasks 1–5, runbook step 5.
- §3.4 `arb-wasm` (09-27 §4 functions, limits, fixtures, native tests; `basket_json`; YES-book inputs; errors not clamps; fixtures listed in §3.4): Tasks 9–12.
- §4 writer (task beside the server, `PUBLIC_EVERY_S` default 300, first run at 30 s, `load_*` split, `spawn_blocking` + tmp/fsync/rename/dir fsync, read-only api with one writable mount, Dockerfile ownership, failures keep the old file, no start without `PUBLIC_DIR`): Tasks 14–16.
- §5.1–5.2 (`PublicSnapshot` shape, `PublicRung`, typed `PublicSweetRow`, 20 categories trimmed before serializing, 64 KiB skip, whitelist / content / size tests): Tasks 13, 15.
- §8 Rust testing (the wasm32 check in CI, fixtures, whitelist/content/size, the writer's four tests): Tasks 12–15.
- §9 step A (identity before merge, image, api recreated with the mount, the alpine read within 60 s: owner 65532 and whitelisted key paths, quiet minute advisory, `pre7b` tag from the running container): Task 17.
- §10 7b-A order: followed (sim extraction, arb-core, arb-wasm, api, Docker/compose, script + runbook; the script is Task 5 so the identity can be run as soon as the extraction is in).

## Open items

Resolved here from the code:
- **wasm-bindgen:** `=0.2.129` (crate and CLI, `cargo search` on 2026-09-30).
- **Fixture and export names:** 7b §3.4 names `fixtures/basket_json.json`; every fixture file and every WASM export is therefore `<name>_json` (09-27's `<function>.json` read as the `_json` function's name).
- **Premises:** `walk_steps` and `classify` live in new files (`steps.rs`, `classify.rs`) because an edit to `fill.rs`/`detect.rs` forces bumps of `depth`/`detector_*` (every card restarts) and breaks `the_backfill_literals_in_0006_are_todays_premises`; the extraction necessarily bumps `sim` 2 → 3.
- **The ledger identity in §3.3** is corrected (Task 4): realized = `net_cash` + open basis; open basis = Σ `held_unsold.basis` for a NO basket.
- **The size test's "both sides":** `markets` is NO-only, fee 0, LADDER only (what the grid draws); the worst case is 32.7 KB; the literal both-sides-every-scenario set is 82.5 KB and never ships.
- **`PublicSweetRow`/`PublicRung` fields** were listed from `lib/sweet.ts`, `SweetLadderGrid.tsx`, `LatencyLadder.tsx` and `lib/ladder.ts` (Task 13).
- **`basket_json` planning** = `decide`'s net-edge, `q_open ≥ min_order` checks, then `settle(min(q_request, q_open))`; `q_open` is in the output so the cap by depth is visible.
- **`PUBLIC_EVERY_S`** is bounded 10..=3600; the first run waits 30 s.

Still open (owner):
1. **Sim premise 3 and the scanner restart.** The runbook recreates only the api (spec §9). The scanner, analyze and compact pick up the 7b image at their next restart (the boot unit, a reboot), and that scanner run is the first with sim premise 3: Edge capture and SWEET count from it anew. Recreate the scanner in step A on purpose (a planned restart, one more reconnect window), or leave it to the next restart as written?
2. **`publicToLadder` and `measured.host`** (7b-B): the snapshot drops `host`; 7b-B either passes `""` or narrows the component's type.
3. **09-27 §11 `oha`** stays with 7b-B's flood test.
4. ~~Spec text~~: done in spec commit e64d638 (ledger identity, size test).
