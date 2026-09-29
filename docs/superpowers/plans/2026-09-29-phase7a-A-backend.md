# Phase 7a-A: latency ladder, paper epochs and reset (backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four exact-latency P0 paper accounts (a latency ladder), paper epochs with a once-a-day reset the scanner applies in place, epoch-aware readers, and a read endpoint that answers "at which latency would this pay?", deployed as step A.
**Architecture:** `sim` gains an in-place `PaperTrader::reset` (ledger `seq` carries on) and `mark_now`; the scanner polls `paper_reset_requests` from a spawned task, applies a request at its paper clock, captures a `Ctl::PaperReset { t }` line so replay resets at the same instant, and writes the close rows, the `paper_epochs` row, the marks and `paper_state` as ONE `WriterMsg::PaperReset` transaction. Readers (store, analyze, api) assign rows to epochs by time; the api gets its first write route (`POST /api/paper/reset`) through a second one-connection pool as `paper_reset_rw`, and `GET /api/paper/ladder` sums each ladder account's gains, leg-risk cost and settlements up to one common mark, with a pure `break_even` in `cards`.
**Tech Stack:** Rust 1.98.1 (edition 2024), tokio, axum 0.8, sqlx 0.8 (inside `store` only), rust_decimal 1.41 (`serde-with-str`, `maths`), toml 0.8, proptest; Postgres 17; Docker compose on the VPS.
**Spec:** docs/superpowers/specs/2026-09-29-phase7a-latency-answer-design.md

## Global Constraints

- **Paper only (parent §16):** no order placement, wallets, keys, signing or exchange clients anywhere; the new api route writes one row in `paper_reset_requests` and nothing else.
- **No new crates.io crate** without owner approval: new Rust dependencies come from `[workspace.dependencies]` only (`toml` for `cards`, the path crate `cards` for `api`). No web change in this plan (the web is plan 7a-B).
- **Exact decimals as strings** in every JSON body (`api::dto::Dec`); no floats in Rust (clippy denies `float_arithmetic`, `unwrap_used`, `expect_used`; tests may unwrap per `clippy.toml`, integration test files start with `#![allow(clippy::unwrap_used, clippy::expect_used)]`); `crates/cards` keeps its no-`f32`/`f64` source scan (`crates/cards/tests/no_floats.rs`).
- **Only `store` uses sqlx** (`deny.toml` wrappers); `analyze`, `api`, `compact` reach Postgres only through `store`; `api` must not depend directly on `sim`, `arb-core`, `pm-api`, `reqwest`, `tokio-tungstenite` or `sqlx` (`bins/scanner/tests/arch.rs`); `crates/cards`, `crates/sim`, `crates/tape` stay pure (no tokio, sqlx, reqwest, tokio-tungstenite, pm-api, store).
- **`*.sql` are LF** (`.gitattributes`: `*.sql text eol=lf`, `config/*.toml text eol=lf`); migration 0009 is pinned in `crates/store/tests/migration_checksums.rs` only AFTER the deploy, from the checksum production recorded (runbook step A, last step).
- **Explicit grants since 0003:** every new table spells out its grants; `scanner_rw` gets no default INSERT/UPDATE; new role `paper_reset_rw` follows `0008_compact.sql:30-40` (NOLOGIN in the migration, password set by the runbook, `docker/postgres/init/01-roles.sh` for fresh volumes).
- **The api's main pool stays `analyze_ro`** with `default_transaction_read_only=on` and the `role_can_write` start-up refusal; the reset pool is a second `Option<Store>` with one connection.
- **Premises:** any edit to a file listed in `crates/arb-core/src/premises.rs` `PREMISES` needs that area's version bumped and `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock` (Task 1 bumps `sim` to `2`; no other area is touched).
- **Commits:** stage only the files the task touched (`git add <paths>`, never `-A` or `.`); never stage `PROJECT.md` or `README.md`; commit messages NEVER carry a `Co-Authored-By` line or any other attribution trailer.
- **Runbook format:** command → expected (+ `if not:`), self-contained, no prose paragraphs, **STOP** after migrate (and after every gate).
- **CI (`.github/workflows/ci.yml`), run before the last commit of every task that touches Rust:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (CI sets `STORE_TESTS_REQUIRED=1`), `cargo deny check bans advisories`; compose: `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q`.
- **Shell prefix for every command** (Git Bash, in the worktree): `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-latency-answer && export PATH="$HOME/.cargo/bin:$PATH" &&`.
- **Postgres-backed tests** (store, analyze, api) use the PC Postgres on **127.0.0.1:15432**; prefix them with (PG prefix) `set -a; . /c/Users/Isabelly/PycharmProjects/cute-trading/.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1 &&`. With `STORE_TESTS_REQUIRED=1` a missing URL fails instead of printing `SKIPPED`. Before the first such run in a task: `(exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up` must print `pg-up`; if not, stop and ask the owner. Never print `.env`.
- **Debug builds only** for the implementer (no `--release`, no `cargo install`); Docker builds and VPS steps are owner-run (Task 3's gate, Task 13's runbook).

## Review Focus

1. **A transient write failure of a reset group while newer rows arrive.** The reset must stay ahead of every row pushed after it (new-epoch trades written before the epoch row would give analyze a mark it reads as a delta from the old epoch), and a failed open batch that is retried while a reset arrives must still go before the reset. Pinned in Task 7 by `a_transient_failure_keeps_the_reset_ahead_of_newer_rows` and `a_failed_open_batch_retried_during_a_reset_still_goes_first`.
2. **Two requests across UTC midnight, collapsed into one epoch.** Both rows land (different `utc_day`), the scanner writes one epoch naming the higher id; the lower id must never read as pending (`id > max(paper_epochs.request_id)`), or the api answers 409 forever. Pinned in Task 5 by `requests_collapse_into_the_epoch_that_names_the_highest_one`.
3. **A row in the cut's own millisecond, and a resolution mark after the last common mark.** A 0 ms rung can execute in the same millisecond as a minute mark or the `mark_now` row, after it; a resolution writes a `paper_marks` row for one account only. The ladder's sums must stop at each account's own mark row (by `(run_id, seq)`) at the latest instant every ladder account has a ledger `mark`, so `gains + leg_risk + settle + unrealized = net` stays exact. Pinned in Task 6 by `the_cut_is_the_latest_common_mark_and_each_sum_stops_at_its_accounts_mark_row` and in Task 12 by `the_ladder_sums_each_kind_since_the_epoch_start_and_reconciles_exactly`.
4. **A request pending at start, then the poller.** The start-time epoch consumes request R; the poller must be seeded with R so it never forwards R again (a second reset minutes after start). Pinned in Task 8 by `the_poller_forwards_new_ids_and_asks_after_the_last_seen`.
5. **Rows exactly at an epoch boundary, and unknown epochs.** A mark at exactly `start_ms` belongs to the new epoch, one at `start_ms − 1` to the old; `?epoch=N` for an epoch that doesn't exist is empty, not an error; with no epoch row everything is epoch 0. Pinned in Task 6 by `equity_and_ledger_follow_epoch_bounds_to_the_millisecond`.

---

## File Structure

```
crates/sim/src/trader.rs                         MODIFY: PaperTrader::reset, PaperTrader::mark_now
crates/sim/tests/trader.rs                       MODIFY: reset/seq/rate/0 ms tests
crates/arb-core/src/premises.rs, premises.lock   MODIFY: sim 1 → 2
crates/tape/src/line.rs, feed.rs                 MODIFY: Ctl::PaperReset (a no-op for books)
crates/tape/tests/{feed,files}.rs                MODIFY
bins/compact/tests/reduce.rs                     MODIFY: a reset line changes no row
bins/scanner/src/engine.rs                       MODIFY: paper_clock, seed_paper_clock, paper_reset, entry-point clamp
bins/scanner/src/replay.rs                       MODIFY: the Ctl::PaperReset arm
bins/scanner/src/writer.rs                       MODIFY: WriterMsg::PaperReset, sealed groups
bins/scanner/src/paper.rs                        MODIFY: start epochs, ResetTracker, epoch_batch, run_reset_poller
bins/scanner/src/run.rs                          MODIFY: start epoch, poller, reset arm, dry-run wall/RSS line
bins/scanner/tests/{engine_paper,replay,premises}.rs MODIFY
config/policies.toml                             MODIFY: 4 scenarios + 4 accounts appended
crates/sim/tests/config.rs                       MODIFY: 19 accounts, the rungs
crates/store/migrations/0009_paper_epochs.sql    CREATE
crates/store/src/{rows,paper,read_paper,lib,error}.rs MODIFY
crates/store/src/{epochs,ladder}.rs              CREATE
crates/store/tests/{roles,migrate,batch,read_paper}.rs MODIFY; {epochs,ladder}.rs CREATE
docker/postgres/init/01-roles.sh, compose.yml    MODIFY: RESET_RW_DB_PASSWORD
crates/cards/src/{edge,lib}.rs, Cargo.toml       MODIFY: LADDER, EXTRAS, toml
crates/cards/src/{ladder,scenarios}.rs           CREATE; crates/cards/tests/{ladder,scenarios}.rs CREATE
bins/analyze/src/{scenarios,tick}.rs             MODIFY; bins/analyze/tests/tick.rs MODIFY
bins/api/src/{lib,main,config,error,paper}.rs    MODIFY; {reset,ladder}.rs CREATE; Cargo.toml MODIFY
bins/api/tests/{common/mod,paper}.rs             MODIFY; {reset,ladder}.rs CREATE
compose.web.yml, .env.example                    MODIFY: RESET_DATABASE_URL, POLICIES_PATH, RESET_RW_DB_PASSWORD
docs/superpowers/runbooks/phase7a-latency-vps.md CREATE; s3-paper.md MODIFY
```

---

### Task 1: sim — reset in place, mark_now, the 0 ms scenario, sim premise 2

**Files:**
- Modify: `crates/sim/src/trader.rs:369-402` (insert two methods after `close`, before `state` at 404)
- Modify: `crates/arb-core/src/premises.rs:26-39` (the `sim` entry's version `"1"` at line 28)
- Modify: `crates/arb-core/premises.lock` (refreshed by the lock test, never by hand)
- Modify: `bins/scanner/tests/premises.rs:70-73`
- Test: `crates/sim/tests/trader.rs` (append; reuses `cfg` at 14, `books_of` at 46, `NO3` at 54, `fills` at 56)

**Interfaces:**
- Consumes: `Account::new(spec: AccountSpec) -> Account` (`account.rs:90`), `ConsumedLiquidity: Default`, private `PaperTrader::mark_all(&mut self, t_ms: i64, books: &impl Books, out: &mut Vec<SimOut>)` (`trader.rs:758`).
- Produces: `pub fn reset(&mut self)`; `pub fn mark_now(&mut self, t_ms: i64, books: &impl Books) -> Vec<SimOut>` on `sim::trader::PaperTrader`; `arb_core::premises::code_version("sim") == Some("2")`.

- [ ] **Step 1: Write the failing tests** — append to `crates/sim/tests/trader.rs`:

```rust
fn ledger_rows(outs: &[SimOut]) -> Vec<(i64, u64)> {
    outs.iter()
        .filter_map(|o| match o {
            SimOut::Ledger(e) => Some((e.t_ms, e.seq)),
            _ => None,
        })
        .collect()
}

#[test]
fn reset_keeps_each_accounts_ledger_seq_so_the_new_epoch_never_collides() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    let mut rows = Vec::new();
    rows.extend(t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3)));
    rows.extend(t.on_frame(1100, &books)); // three buys
    rows.extend(t.close(1500, &books)); // the conversion (due 3100) completes at 1500, then a mark
    t.reset();
    rows.extend(t.mark_now(1501, &books));
    rows.extend(t.on_open(&ctx("e2", 1600, &g, detect_no_basket(&g).unwrap(), &NO3)));
    let e2 = t.on_frame(1700, &books);
    assert_eq!(
        fills(&e2),
        vec![
            ("m0".into(), d("51.2")),
            ("m1".into(), d("51.2")),
            ("m2".into(), d("51.2"))
        ],
        "the reset cleared consumed liquidity: the new epoch retakes the depth"
    );
    rows.extend(e2);
    let seqs: Vec<u64> = ledger_rows(&rows).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        seqs,
        (1..=9).collect::<Vec<u64>>(),
        "seq carries on across the reset (review C1): paper_ledger's key would drop a restarted seq"
    );
}

#[test]
fn close_reset_mark_now_starts_every_account_at_its_bankroll_after_the_close_rows() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[
        (PolicyKind::P0Converter, "1000"),
        (PolicyKind::P3Naive, "500"),
    ]))
    .unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books);
    let close = t.close(1500, &books);
    assert!(!ledger_rows(&close).is_empty());
    assert!(
        ledger_rows(&close).iter().all(|&(at, _)| at == 1500),
        "close rows are stamped at t, before the epoch start t + 1"
    );
    t.reset();
    for a in t.accounts() {
        assert_eq!(
            (a.cash, a.reserved, a.realized, a.fees_paid),
            (a.spec.bankroll, d("0"), d("0"), d("0")),
            "{}",
            a.id
        );
        assert!(a.positions.is_empty() && a.returned.is_empty(), "{}", a.id);
        assert_eq!(
            (a.peak_equity, a.max_drawdown),
            (a.spec.bankroll, d("0")),
            "{}",
            a.id
        );
    }
    let start = t.mark_now(1501, &books);
    let marks: Vec<(String, i64, Decimal)> = start
        .iter()
        .filter_map(|o| match o {
            SimOut::Account(a) => Some((a.account.clone(), a.t_ms, a.equity)),
            _ => None,
        })
        .collect();
    assert_eq!(
        marks,
        vec![
            ("P0-converter/t".to_string(), 1501, d("1000")),
            ("P3-naive/t".to_string(), 1501, d("500"))
        ]
    );
    assert!(ledger_rows(&start).iter().all(|&(at, _)| at == 1501));
}

#[test]
fn reset_keeps_the_frame_rate_so_a_burst_scenario_stays_in_its_burst() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut c = cfg(&[(PolicyKind::P0Converter, "1000")]);
    c.scenarios[0].burst = vec![sim::config::BurstStep {
        frames_per_s_above: 0,
        multiplier: 3,
    }];
    let mut t = PaperTrader::new(c).unwrap();
    for i in 0..20 {
        t.on_frame(100 + i * 10, &books); // 20 frames in the 10 s window: 2 frames/s
    }
    t.close(400, &books);
    t.reset();
    let outs = t.on_open(&ctx("e1", 500, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.exec_ms == Some(800)),
        "match 100 × 3: the frame rate is a market measure, kept across the reset: {outs:?}"
    );
}

#[test]
fn a_zero_ms_scenario_is_valid_and_fills_on_the_book_that_showed_the_edge() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut c = cfg(&[(PolicyKind::P0Converter, "1000")]);
    c.scenarios[0].match_ms = 0; // proc = net = match = 0 (spec 7a §3: rung-0)
    c.validate().unwrap();
    let mut t = PaperTrader::new(c).unwrap();
    let outs = t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(matches!(&outs[..], [SimOut::Intent(i)] if i.exec_ms == Some(1000)));
    assert_eq!(
        fills(&t.on_frame(1000, &books)),
        vec![
            ("m0".into(), d("51.2")),
            ("m1".into(), d("51.2")),
            ("m2".into(), d("51.2"))
        ]
    );
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sim --test trader`
Expected: FAIL to compile: `no method named 'reset' found for struct 'PaperTrader'` and `no method named 'mark_now'`.

- [ ] **Step 3: Implement** — in `crates/sim/src/trader.rs`, insert after `close` (after line 402, before `pub fn state`):

```rust
    /// Spec 7a §4.2: every account back to its bankroll IN PLACE (a paper reset). Cash,
    /// reservations, realized, fees, positions, returned YES, peak equity and drawdown restart
    /// from the account's spec; consumed liquidity, due work and the minute-mark schedule are
    /// cleared. Kept: `cfg`, `scenario_ix`, `rate` (a market measure), and each account's ledger
    /// `seq` (review C1: `paper_ledger`'s key is (run_id, account_id, seq) with ON CONFLICT DO
    /// NOTHING, so a restarted seq would silently drop the new epoch's ledger). `next_seq` only
    /// orders the now-empty due queue and carries on too. Callers `close` first.
    pub fn reset(&mut self) {
        for a in &mut self.accounts {
            let seq = a.seq;
            *a = Account::new(a.spec.clone());
            a.seq = seq;
        }
        for c in &mut self.consumed {
            *c = ConsumedLiquidity::default();
        }
        self.due.clear();
        self.next_mark_ms = None;
    }

    /// Spec 7a §4.2: mark every account at `t_ms` now (a reset's start), exactly as a minute mark
    /// would: one `mark` ledger row and one `AccountRow` each. The minute schedule is untouched.
    pub fn mark_now(&mut self, t_ms: i64, books: &impl Books) -> Vec<SimOut> {
        let mut out = Vec::new();
        self.mark_all(t_ms, books, &mut out);
        out
    }
```

- [ ] **Step 4: Run the sim tests**

Run: `cargo test -p sim`
Expected: PASS (every test, the four new ones included).

- [ ] **Step 5: Bump the sim premise and refresh the lock**

Run: `cargo test -p arb-core --test premises_lock`
Expected: FAIL: `premises.lock is out of step: sim …` (trader.rs changed).
In `crates/arb-core/src/premises.rs` change the `sim` entry's version (line 28) from `"1",` to `"2",`. Then:
Run: `PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock -- --nocapture`
Expected: prints `premises.lock refreshed`, `test result: ok`.
Run: `cargo test -p arb-core --test premises_lock`
Expected: PASS.

- [ ] **Step 6: The scanner's premise test follows the bump** — `bins/scanner/tests/premises.rs:70-73`, replace

```rust
    assert_eq!(
        sim.version,
        effective_version("1", &format!("policies_sha256={}", pol.sha256))
    );
```
with
```rust
    assert_eq!(
        sim.version,
        effective_version("2", &format!("policies_sha256={}", pol.sha256)),
        "spec 7a: PaperTrader::reset/mark_now bumped the sim code version"
    );
```
Run: `cargo test -p scanner --test premises`
Expected: PASS.

- [ ] **Step 7: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no output from fmt, clippy `Finished` with no warnings.
```bash
git add crates/sim/src/trader.rs crates/sim/tests/trader.rs crates/arb-core/src/premises.rs crates/arb-core/premises.lock bins/scanner/tests/premises.rs
git commit -m "sim: PaperTrader::reset in place (seq carries on) and mark_now; sim premise 2"
```

---

### Task 2: tape and engine — `Ctl::PaperReset`, `Engine::paper_reset`, the entry-point clamp, the replay arm

**Files:**
- Modify: `crates/tape/src/line.rs:17-44` (new variant before `Shutdown`)
- Modify: `crates/tape/src/feed.rs:320-321` (the no-op arm)
- Modify: `bins/scanner/src/engine.rs:196-202` (field doc), `:736-747` (`on_frame`), `:1614-1648` (new methods; `paper_close`), `:1684-1697` (`on_resolution`)
- Modify: `bins/scanner/src/replay.rs:113-134` (new arm)
- Test: `crates/tape/tests/feed.rs`, `crates/tape/tests/files.rs`, `bins/compact/tests/reduce.rs`, `bins/scanner/tests/engine_paper.rs`, `bins/scanner/tests/replay.rs` (append each)

**Interfaces:**
- Consumes: `PaperTrader::{reset, mark_now, on_frame, close, on_resolution}` (Task 1).
- Produces: `tape::line::Ctl::PaperReset { t: i64 }` (serialized `{"kind":"paper_reset","t":…}`); `Engine::paper_clock(&self) -> i64`; `Engine::seed_paper_clock(&mut self, start_ms: i64)`; `Engine::paper_reset(&mut self, t_ms: i64) -> Option<(i64, Vec<SimOut>)>` (the epoch's `start_ms` and every close/mark row); every trader entry point driven at `max(its t, last_recv_ms)`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/tape/tests/feed.rs`:
```rust
#[test]
fn a_paper_reset_line_touches_no_book() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let ap = run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
            ctl(T + 20, 0, Ctl::PaperReset { t: T + 15 }),
        ],
    );
    assert_eq!(ap[2].event, LineEvent::None);
    assert!(ap[2].touched.is_empty() && ap[2].snapshotted.is_empty());
    assert_eq!(asks(&f, "a1"), vec!["0.45x10".to_string()]);
}
```
Append to `crates/tape/tests/files.rs`:
```rust
#[test]
fn a_paper_reset_line_round_trips_with_its_payload() {
    let l = Line {
        recv_ts_us: 7_000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::PaperReset { t: 6 }),
    };
    let json = serde_json::to_string(&l).unwrap();
    assert_eq!(
        json,
        r#"{"recv_ts_us":7000,"shard":0,"ctl":{"kind":"paper_reset","t":6}}"#
    );
    assert_eq!(serde_json::from_str::<Line>(&json).unwrap(), l);
}
```
Append to `bins/compact/tests/reduce.rs` (add `use tape::line::{Ctl, Line};` to its imports):
```rust
#[test]
fn a_paper_reset_line_changes_no_row() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let base = vec![
        uni(T0 + 1_000, &r),
        frame(
            T0 + 2_000,
            0,
            book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
        ),
    ];
    let mut with = base.clone();
    with.push(Line {
        recv_ts_us: (T0 + 3_000) * 1000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::PaperReset { t: T0 + 2_999 }),
    });
    let reduce = |lines: &[Line]| {
        let mut red = Reducer::new(rcfg());
        let mut s = VecSink::default();
        red.begin_hour(T0, vec![]);
        run(&mut red, lines, std::slice::from_ref(&r), &mut s);
        red.end_hour(&mut s).unwrap();
        s
    };
    assert_eq!(reduce(&base), reduce(&with));
}
```
Append to `bins/scanner/tests/engine_paper.rs`:
```rust
/// Every instant a paper row carries (spec 7a §4.2: none may fall before the epoch start).
fn row_times(s: &SimOut) -> Vec<i64> {
    match s {
        SimOut::Intent(i) => std::iter::once(i.decided_ms).chain(i.exec_ms).collect(),
        SimOut::Exec(x) => vec![x.exec_ms],
        SimOut::Fill(f) => vec![f.t_ms],
        SimOut::Ledger(l) => vec![l.t_ms],
        SimOut::Markout(_) => vec![],
        SimOut::Account(a) => vec![a.t_ms],
    }
}

fn zero_ms_engine() -> Engine {
    let mut c = sim_cfg(PolicyKind::P0Converter);
    c.scenarios[0].match_ms = 0;
    let mut e = engine();
    e.set_paper(PaperTrader::new(c).unwrap());
    e
}

#[test]
fn a_reset_closes_at_the_paper_clock_then_marks_every_account_at_its_bankroll() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e); // execution due at 2 200
    quiet(&mut e, 2_300); // executed; its conversion is due at 4 200
    let (start, rows) = e.paper_reset(2_250).unwrap(); // an older t never moves the clock back
    assert_eq!((start, e.paper_clock()), (2_301, 2_301));
    let led: Vec<(EntryKind, i64)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Ledger(l) => Some((l.kind, l.t_ms)),
            _ => None,
        })
        .collect();
    assert_eq!(
        led,
        vec![
            (EntryKind::Convert, 2_300),
            (EntryKind::Mark, 2_300),
            (EntryKind::Mark, 2_301)
        ]
    );
    let marks: Vec<(i64, Decimal)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Account(a) => Some((a.t_ms, a.equity)),
            _ => None,
        })
        .collect();
    assert_eq!(marks.last(), Some(&(2_301, dec!(1000))));
    assert!(
        e.paper_state()
            .unwrap()
            .accounts
            .iter()
            .all(|a| a.cash == dec!(1000) && a.positions.is_empty())
    );
    let mut bare = engine();
    assert!(bare.paper_reset(1).is_none(), "no trader: nothing to reset");
}

#[test]
fn no_new_epoch_row_is_stamped_before_the_epoch_start() {
    let mut e = zero_ms_engine();
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    let (start, _) = e.paper_reset(1_500).unwrap();
    assert_eq!(start, 1_501);
    // frames received in the reset's own millisecond and before it (review C2): the NO basket
    // opens and the 0 ms account executes right away
    let mut after = books(&mut e, 1_500, 1_450, ["0.40", "0.35", "0.30"]);
    after.extend(quiet(&mut e, 1_500));
    after.extend(quiet(&mut e, 1_499));
    let rows = sims(&after);
    assert!(
        rows.iter()
            .any(|s| matches!(s, SimOut::Exec(x) if x.exec_ms == 1_501)),
        "{rows:?}"
    );
    for s in &rows {
        assert!(row_times(s).iter().all(|&t| t >= start), "{s:?}");
    }
}

#[test]
fn a_rest_snapshot_open_a_resolution_and_a_close_after_a_reset_are_stamped_at_or_after_its_start() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    let (start, _) = e.paper_reset(1_200).unwrap();
    assert_eq!(start, 1_201);
    // a REST snapshot stamped with an older server time lifts leg 0's YES bid: NO asks
    // 0.50 / 0.70 / 0.70, Σ 1.90 < 2
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_150);
    let mut outs = e.rest_snapshot("e1-y0", b);
    outs.extend(quiet(&mut e, 1_400)); // executes at 1 301
    outs.extend(e.on_resolution(1_000, "e1-m0", dec!(0), dec!(1))); // an older instant
    outs.extend(e.paper_close(900)); // an older instant too
    let rows = sims(&outs);
    assert!(
        rows.iter().any(|s| matches!(s, SimOut::Intent(i) if i.decided_ms == 1_201 && i.exec_ms == Some(1_301))),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Settle && l.t_ms == 1_400)),
        "{rows:?}"
    );
    for s in &rows {
        assert!(row_times(s).iter().all(|&t| t >= start), "{s:?}");
    }
}

#[test]
fn a_seeded_paper_clock_stamps_the_first_rows_of_a_start_time_epoch() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    e.seed_paper_clock(5_000); // review C3: never the i64::MIN clock
    assert_eq!(e.paper_clock(), 5_000);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]);
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_500);
    let rows = sims(&e.rest_snapshot("e1-y0", b));
    assert!(
        rows.iter().any(|s| matches!(s, SimOut::Intent(i) if i.decided_ms == 5_000 && i.exec_ms == Some(5_100))),
        "{rows:?}"
    );
    e.seed_paper_clock(10);
    assert_eq!(e.paper_clock(), 5_000, "seeding never moves the clock back");
}
```
Append to `bins/scanner/tests/replay.rs`:
```rust
/// `write_paper_capture`'s NO basket (frames to T0 + 4 000), then a paper reset whose payload
/// `t` (T0 + 4 500) is 100 ms before its own line, then a clean stop at T0 + 6 000.
fn write_reset_capture(dir: &Path) {
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let ufile = universe_file_name(u.taken_ms, 1);
    let asks = ["0.45", "0.40", "0.35"];
    let open = ["0.40", "0.35", "0.30"];
    let msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
            file: ufile.clone(),
        })),
        line(
            T0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
                file: ufile,
            }),
        ),
        line(
            T0 + 100,
            Some(paper_frame(T0, ["0.30", "0.30", "0.30"], asks)),
            None,
        ),
        line(T0 + 1_100, Some(paper_frame(T0 + 1_000, open, asks)), None),
        line(T0 + 2_000, Some(paper_frame(T0 + 1_900, open, asks)), None),
        line(T0 + 4_000, Some(paper_frame(T0 + 3_900, open, asks)), None),
        line(T0 + 4_600, None, Some(Ctl::PaperReset { t: T0 + 4_500 })),
        line(T0 + 6_000, None, Some(Ctl::Shutdown)),
    ];
    let mut w = Writer::new(dir.to_path_buf());
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

#[test]
fn replay_resets_at_the_payload_instant_and_keeps_every_seq_rising() {
    let dir = tmp("paper-reset");
    write_reset_capture(&dir);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let (a, b) = (replay_paper(&dir, &policies), replay_paper(&dir, &policies));
    assert_eq!(a, b, "deterministic across the reset");
    let rows = paper_rows(&a);
    let n = policies.accounts.len();
    let at = |t: i64| rows.paper_marks.iter().filter(|m| m.t_ms == t).count();
    assert_eq!(
        (at(T0 + 4_500), at(T0 + 4_501), at(T0 + 4_600)),
        (n, n, 0),
        "the close at the payload t, the new epoch at t + 1, nothing at the line's own time"
    );
    assert!(
        rows.paper_marks
            .iter()
            .filter(|m| m.t_ms == T0 + 4_501)
            .all(|m| m.equity == dec!(1000))
    );
    let mut by: std::collections::BTreeMap<&str, Vec<i64>> = Default::default();
    for l in &rows.paper_ledger {
        by.entry(l.account_id.as_str()).or_default().push(l.seq);
    }
    for (acct, seqs) in by {
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{acct}: {seqs:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p tape --test feed --test files; cargo test -p scanner --test engine_paper --test replay`
Expected: FAIL to compile: `no variant named 'PaperReset' found for enum 'Ctl'`, `no method named 'paper_reset'`/`'paper_clock'`/`'seed_paper_clock'`.

- [ ] **Step 3: Implement the tape variant** — `crates/tape/src/line.rs`, between `Resolved { … },` and `Shutdown,` (line 43):

```rust
    /// A paper reset the live scanner applied (Phase 7a spec §4.2). `t` is the paper clock it
    /// closed at: replay calls `Engine::paper_reset(t)` with this payload, never the line's own
    /// receive time. Books are untouched. Pre-7a binaries can't read it (`for_each_line`
    /// hard-errors on an unknown line), so compact must run the 7a image before the first reset.
    PaperReset {
        t: i64,
    },
```
`crates/tape/src/feed.rs:320-321`, replace
```rust
            // engine `mark_suspect` and `on_resolution` never touch a book
            None | Some(Ctl::Suspect { .. }) | Some(Ctl::Resolved { .. }) => {}
```
with
```rust
            // engine `mark_suspect`, `on_resolution` and `paper_reset` never touch a book
            None
            | Some(Ctl::Suspect { .. })
            | Some(Ctl::Resolved { .. })
            | Some(Ctl::PaperReset { .. }) => {}
```

- [ ] **Step 4: Implement the engine** — `bins/scanner/src/engine.rs`.

Field doc (lines 199-202), replace the comment above `last_recv_ms: i64,` with:
```rust
    /// The paper clock: the latest input instant the trader has been driven to (frame receive
    /// times, `on_resolution`, `paper_close`, a reset's start). Spec 7a §4.2: EVERY trader entry
    /// point is driven at max(its own time, this), so no row of a new epoch is ever stamped
    /// before its start; outside a reset this changes nothing, because everything due at or
    /// before this clock already ran at the frame that set it.
```
`on_frame` (lines 738-747), replace
```rust
        self.last_recv_ms = self.last_recv_ms.max(recv_ms);
        // S3b call contract: work due at or before this frame runs on the books as they were
        // just BEFORE it (the state at its due time).
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(recv_ms, &books), &mut out);
        }
```
with
```rust
        self.last_recv_ms = self.last_recv_ms.max(recv_ms);
        let clock = self.last_recv_ms;
        // S3b call contract: work due at or before this frame runs on the books as they were
        // just BEFORE it (the state at its due time). Spec 7a §4.2: at the paper clock.
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(clock, &books), &mut out);
        }
```
After `pub fn paper_state(&self) -> Option<TraderState> { … }` (line 1632), insert:
```rust
    /// The paper clock (spec 7a §4.2): the instant a live reset closes at.
    pub fn paper_clock(&self) -> i64 {
        self.last_recv_ms
    }

    /// Spec 7a §4.2: a start-time epoch starts the paper clock at `start_ms` (never i64::MIN,
    /// review C3). Never moves the clock back.
    pub fn seed_paper_clock(&mut self, start_ms: i64) {
        self.last_recv_ms = self.last_recv_ms.max(start_ms);
    }

    /// Spec 7a §4.2: a paper reset at `t_ms` (live: the paper clock, captured as
    /// `Ctl::PaperReset { t }`; replay: that payload). Drives the trader to `t` and closes it
    /// there (conversions complete, pending intents release, unwinds and markouts drop, every
    /// account marks), resets every account in place, then marks every account at `t + 1`, the
    /// epoch's start, which becomes the paper clock. Returns (start_ms, every row) for ONE
    /// `WriterMsg::PaperReset`; None without a trader.
    pub fn paper_reset(&mut self, t_ms: i64) -> Option<(i64, Vec<SimOut>)> {
        let t = self.last_recv_ms.max(t_ms);
        self.last_recv_ms = t;
        let tr = self.paper.as_mut()?;
        let books = EngineBooks {
            token_ix: &self.token_ix,
            groups: &self.groups,
        };
        let mut rows = tr.on_frame(t, &books);
        rows.extend(tr.close(t, &books));
        tr.reset();
        let start_ms = t + 1;
        rows.extend(tr.mark_now(start_ms, &books));
        self.last_recv_ms = start_ms;
        Some((start_ms, rows))
    }
```
`paper_close` (lines 1638-1648), replace its body with:
```rust
        let mut out = Vec::new();
        // spec 7a §4.2: at the paper clock, never before it
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        let t_ms = self.last_recv_ms;
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(t_ms, &books), &mut out);
            paper_outs(t.close(t_ms, &books), &mut out);
        }
        out
```
`on_resolution` (lines 1686-1697), replace
```rust
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(t_ms, &books), &mut out);
            paper_outs(
                t.on_resolution(t_ms, market_id, payout_yes, payout_no, &books),
                &mut out,
            );
        }
```
with
```rust
        self.last_recv_ms = self.last_recv_ms.max(t_ms);
        let clock = self.last_recv_ms; // spec 7a §4.2: at the paper clock, never before it
        if let Some(t) = self.paper.as_mut() {
            let books = EngineBooks {
                token_ix: &self.token_ix,
                groups: &self.groups,
            };
            paper_outs(t.on_frame(clock, &books), &mut out);
            paper_outs(
                t.on_resolution(clock, market_id, payout_yes, payout_no, &books),
                &mut out,
            );
        }
```

- [ ] **Step 5: Implement the replay arm** — `bins/scanner/src/replay.rs`, insert before `Some(Ctl::Shutdown) => {` (line 126):

```rust
        Some(Ctl::PaperReset { t }) => {
            // spec 7a §4.2: the payload `t` (the live paper clock), not this line's receive time.
            // No epoch row: only the live scanner writes paper_epochs.
            if let Some((_, rows)) = engine.paper_reset(*t) {
                for r in rows {
                    sink(Out::Paper(Box::new(r)));
                }
            }
        }
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p tape && cargo test -p compact --test reduce && cargo test -p scanner`
Expected: PASS (the agreement tests `tape_agreement`/`tob_agreement` stay ignored-by-default as before).

- [ ] **Step 7: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add crates/tape/src/line.rs crates/tape/src/feed.rs crates/tape/tests/feed.rs crates/tape/tests/files.rs bins/compact/tests/reduce.rs bins/scanner/src/engine.rs bins/scanner/src/replay.rs bins/scanner/tests/engine_paper.rs bins/scanner/tests/replay.rs
git commit -m "scanner: Engine::paper_reset at the paper clock, entry-point clamp, Ctl::PaperReset (no-op for books) and its replay arm"
```

---

### Task 3: the ladder `policies.toml` and the cost gate

**Files:**
- Modify: `config/policies.toml` (insert after line 47, append after line 139; every existing line byte-identical)
- Modify: `crates/sim/tests/config.rs:11-28`
- Modify: `bins/analyze/src/scenarios.rs:37-57`
- Modify: `bins/scanner/src/run.rs:1185-1205` (dry-run line) and its `tests` module (from 1275)

**Interfaces:**
- Consumes: `SimConfig::validate` (accepts 0/0/0, Task 1).
- Produces: scenarios `rung-0` (0 ms), `rung-25` (25), `rung-100` (100), `rung-200` (200), accounts `P0-converter/rung-{0,25,100,200}` (19 accounts in all); `scanner::run::vm_hwm_kb(status: &str) -> Option<u64>`, `scanner::run::peak_rss_kb() -> Option<u64>`; the dry-run line `frames N parse_errors N episodes N paper_rows N wall_ms N peak_rss_kb N|n/a`.

- [ ] **Step 1: Write the failing tests**

`crates/sim/tests/config.rs`, in `default_file_parses_validates_and_is_exact`, replace `assert_eq!(c.accounts.len(), 15);` with `assert_eq!(c.accounts.len(), 19);` and append at the end of that test:
```rust
    // spec 7a §3: the latency ladder, exact latencies (no burst steps), secondsDelay honored
    for (name, total) in [
        ("rung-0", 0),
        ("rung-25", 25),
        ("rung-100", 100),
        ("rung-200", 200),
    ] {
        let s = c.scenario(name).unwrap();
        assert_eq!(
            (
                s.proc_ms + s.net_ms + s.match_ms,
                s.burst.len(),
                s.honor_seconds_delay
            ),
            (total, 0, true),
            "{name}"
        );
        assert!(ids.contains(&format!("P0-converter/{name}")), "{name}");
    }
```
`bins/analyze/src/scenarios.rs`, replace the test `the_repo_policies_give_four_scenarios` (lines 37-57) with:
```rust
    #[test]
    fn the_repo_policies_give_eight_scenarios() {
        let s = load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/policies.toml"
        ))
        .unwrap();
        let got: Vec<(&str, i64, i64)> = s
            .iter()
            .map(|x| (x.name.as_str(), x.net_ms, x.latency_ms))
            .collect();
        assert_eq!(
            got,
            vec![
                ("colo-idle", 1, 52),
                ("sp-idle", 145, 300),
                ("sp-burst", 145, 450),
                ("sp-idle-nodelay", 145, 300),
                ("rung-0", 0, 0),
                ("rung-25", 0, 25),
                ("rung-100", 45, 100),
                ("rung-200", 95, 200)
            ]
        );
    }
```
`bins/scanner/src/run.rs` `tests` module, append:
```rust
    #[test]
    fn vm_hwm_is_read_from_a_proc_status_text() {
        let status = "Name:\tscanner\nVmPeak:\t  900 kB\nVmHWM:\t  123456 kB\nVmRSS:\t 100 kB\n";
        assert_eq!(vm_hwm_kb(status), Some(123_456));
        assert_eq!(vm_hwm_kb("Name:\tx\n"), None);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p sim --test config; cargo test -p analyze --lib scenarios; cargo test -p scanner --lib vm_hwm`
Expected: FAIL: `left: 15, right: 19`; the eight-scenario list differs (four found); `cannot find function 'vm_hwm_kb'`.

- [ ] **Step 3: Edit `config/policies.toml`** — insert after line 47 (`honor_seconds_delay = false` of `sp-idle-nodelay`), before the blank line 48:

```toml

# Phase 7a latency ladder (spec 2026-09-29 §2, §3): exact latencies, no burst steps. d = proc +
# net + match is the time from the exchange showing the edge to our order matching; rung-0 is the
# impossible baseline (filled on the book that showed the edge). sp-burst and sp-idle-nodelay stay
# extras beside the ladder.
[[scenarios]]
name = "rung-0"
proc_ms = 0
net_ms = 0
match_ms = 0
honor_seconds_delay = true

[[scenarios]]
name = "rung-25"
proc_ms = 0
net_ms = 0
match_ms = 25
honor_seconds_delay = true

[[scenarios]]
name = "rung-100"                       # a bot in the exchange's cloud region
proc_ms = 5
net_ms = 45
match_ms = 50
honor_seconds_delay = true

[[scenarios]]
name = "rung-200"                       # us on a good day
proc_ms = 5
net_ms = 95
match_ms = 100
honor_seconds_delay = true
```
and append after line 139 (the end of the file):
```toml

[[accounts]]                            # Phase 7a latency ladder (spec 2026-09-29 §3)
policy = "P0-converter"
scenario = "rung-0"
bankroll = "1000"
per_trade = "100"

[[accounts]]
policy = "P0-converter"
scenario = "rung-25"
bankroll = "1000"
per_trade = "100"

[[accounts]]
policy = "P0-converter"
scenario = "rung-100"
bankroll = "1000"
per_trade = "100"

[[accounts]]
policy = "P0-converter"
scenario = "rung-200"
bankroll = "1000"
per_trade = "100"
```
Check: `git diff --stat config/policies.toml` shows only insertions: `1 file changed, <n> insertions(+)` with no `-`.

- [ ] **Step 4: The dry-run's wall time and peak RSS** — `bins/scanner/src/run.rs`, add near `secs_to_ms_saturating` (line 70):

```rust
/// `VmHWM` (peak resident set) in kB from a `/proc/<pid>/status` text; None when absent.
pub fn vm_hwm_kb(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|n| n.parse().ok())
}

/// This process's peak RSS (Linux only; None elsewhere, e.g. the Windows PC). Spec 7a §3's cost
/// gate reads it from `scanner replay --dry-run`, the compact bench pattern.
pub fn peak_rss_kb() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .as_deref()
        .and_then(vm_hwm_kb)
}
```
In `replay_cmd`, in the `if dry_run {` branch (line 1185), make its first line `let started = Instant::now();` and replace the `println!` (lines 1200-1205) with:
```rust
        println!(
            "frames {} parse_errors {} episodes {} paper_rows {paper_rows} wall_ms {} peak_rss_kb {}",
            stats.frames,
            stats.parse_errors,
            eps.len(),
            started.elapsed().as_millis(),
            peak_rss_kb().map_or_else(|| "n/a".to_string(), |k| k.to_string())
        );
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sim && cargo test -p analyze --lib && cargo test -p scanner`
Expected: PASS (`paper.rs`'s tests restore accounts 0 and 1, unchanged by the appended accounts; `replay.rs` counts accounts from the file).

- [ ] **Step 6: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
```bash
git add config/policies.toml crates/sim/tests/config.rs bins/analyze/src/scenarios.rs bins/scanner/src/run.rs
git commit -m "policies: the latency ladder (rung-0/25/100/200); replay --dry-run prints wall_ms and peak_rss_kb"
```

- [ ] **Step 7 (owner-run): fetch one busy raw hour from the VPS** — the largest closed hour of the last two days, plus the universe records its lines name:

```bash
rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'command -v zstdcat >/dev/null || apt-get install -y zstd >/dev/null; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $(zstdcat "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && (cd "$HOME/probe/cap" && find . -type f | sort)
```
→ expected: `busy hour: ./<YYYY-MM-DD>/<HH>.jsonl.zst`, then that file and one or more `./<YYYY-MM-DD>/universe-<ms>-<seq>.json.zst`.
if not: an empty `busy hour:` = no closed hour in the last 2 days: drop `-mmin -2880` and run again.

- [ ] **Step 8 (owner-run): the pre-ladder policies and an image of this commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-latency-answer && git show main:config/policies.toml > "$HOME/probe/policies-old.toml" && grep -c '^\[\[accounts\]\]' "$HOME/probe/policies-old.toml" config/policies.toml && docker build -f docker/rust.Dockerfile --build-arg GIT_SHA="$(git rev-parse --short HEAD)" -t cute-trading/rust:gate . 2>&1 | tail -2
```
→ expected: `…/policies-old.toml:15`, `config/policies.toml:19`, then the build's last lines with no `ERROR`.
if not: a `CRLF found` error: `git checkout -- config crates/store/migrations` and run again.

- [ ] **Step 9 (owner-run): replay the hour twice with and twice without the ladder**

```bash
P="$(cygpath -m "$HOME/probe")"; for i in 1 2; do for v in old new; do if [ "$v" = old ]; then M="-v $P/policies-old.toml:/etc/cute-trading/policies.toml:ro"; else M=""; fi; printf '%s ' "$v"; MSYS_NO_PATHCONV=1 docker run --rm -v "$P/cap:/probe/cap:ro" $M --entrypoint /usr/local/bin/scanner cute-trading/rust:gate replay /probe/cap --dry-run 2>/dev/null | tail -1; done; done | tee "$HOME/probe/gate.txt"
```
→ expected: four lines `old|new frames <N> parse_errors <n> episodes <n> paper_rows <n> wall_ms <ms> peak_rss_kb <kb>`, the same `<N>` (> 0) on all four.
if not: `peak_rss_kb n/a` = not run in the Linux container; an error line = a universe record is missing: redo Step 7.

- [ ] **Step 10 (owner-run): the verdict** (spec §3: peak RSS grows < 10 % and < 50 MB, wall time < 10 %)

```bash
python - "$HOME/probe/gate.txt" <<'PY' | tee "$HOME/probe/verdict.txt"
import sys
rows = {"old": [], "new": []}
for line in open(sys.argv[1]):
    f = line.split()
    kv = dict(zip(f[1::2], f[2::2]))
    rows[f[0]].append((int(kv["wall_ms"]), int(kv["peak_rss_kb"]), int(kv["frames"])))
(ow, orss, of), (nw, nrss, nf) = [
    (min(r[0] for r in rows[k]), max(r[1] for r in rows[k]), rows[k][0][2]) for k in ("old", "new")
]
assert of == nf and of > 0, f"frames differ or zero: {of} {nf}"
drss = nrss - orss
print(f"wall_ms old {ow} new {nw} (+{100 * (nw - ow) / ow:.1f} %); peak_rss_kb old {orss} new {nrss} (+{100 * drss / orss:.1f} %, +{drss // 1024} MB)")
print("GATE PASS" if nw < ow * 1.10 and drss < orss * 0.10 and drss < 50 * 1024 else "GATE FAIL")
PY
```
→ expected: one figures line, then `GATE PASS`. Copy both lines into this plan's **Results**.
if not: `GATE FAIL`: **STOP**; do not go on to deploy A; bring the figures line to the owner.

- [ ] **Step 11 (owner-run): drop the gate image** — `docker image rm cute-trading/rust:gate` → expected: `Untagged: cute-trading/rust:gate` (keep `$HOME/probe` for the runbook's final gate).

---

### Task 4: store — migration 0009, its grants and the reset role

**Files:**
- Create: `crates/store/migrations/0009_paper_epochs.sql`
- Modify: `crates/store/src/lib.rs:152-162` (a test-tooling read after `role_db_settings`)
- Modify: `crates/store/tests/migrate.rs:12-35` (table list), `crates/store/tests/roles.rs` (append)
- Modify: `docker/postgres/init/01-roles.sh` (append), `compose.yml:14-21` (postgres env)

**Interfaces:**
- Consumes: the roles `scanner_rw`, `analyze_ro` (0001), `analyze_rw` (0006).
- Produces: tables `paper_reset_requests(id, requested_ms, utc_day UNIQUE)`, `paper_epochs(epoch, start_ms, cause, request_id, run_id, policies_sha256)`, index `paper_ledger_account_t`, role `paper_reset_rw` (NOLOGIN, `statement_timeout=5s`); `Store::has_table_privilege(&self, role: &str, table: &str, privilege: &str) -> Result<bool, StoreError>`.

- [ ] **Step 1: Write the failing tests** — `crates/store/tests/migrate.rs`: add `"paper_epochs",` after `"paper_accounts",` and `"paper_reset_requests",` after `"paper_marks",` in the list (lines 12-35, keep it sorted). Append to `crates/store/tests/roles.rs`:

```rust
#[tokio::test]
async fn migration_0009_grants_epochs_and_reset_requests_to_exactly_four_roles() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    for (role, table, privilege, want) in [
        ("scanner_rw", "paper_reset_requests", "SELECT", true),
        ("scanner_rw", "paper_reset_requests", "INSERT", false),
        ("scanner_rw", "paper_epochs", "SELECT", true),
        ("scanner_rw", "paper_epochs", "INSERT", true),
        ("scanner_rw", "paper_epochs", "UPDATE", false),
        ("scanner_rw", "paper_epochs", "DELETE", false),
        ("analyze_ro", "paper_reset_requests", "SELECT", true),
        ("analyze_ro", "paper_epochs", "SELECT", true),
        ("analyze_ro", "paper_epochs", "INSERT", false),
        ("analyze_rw", "paper_reset_requests", "SELECT", true),
        ("analyze_rw", "paper_epochs", "SELECT", true),
        ("analyze_rw", "paper_epochs", "INSERT", false),
        ("paper_reset_rw", "paper_reset_requests", "SELECT", true),
        ("paper_reset_rw", "paper_reset_requests", "INSERT", true),
        ("paper_reset_rw", "paper_reset_requests", "UPDATE", false),
        ("paper_reset_rw", "paper_reset_requests", "DELETE", false),
        ("paper_reset_rw", "paper_epochs", "SELECT", true),
        ("paper_reset_rw", "paper_epochs", "INSERT", false),
        ("paper_reset_rw", "episodes", "SELECT", false),
        ("paper_reset_rw", "paper_ledger", "INSERT", false),
    ] {
        assert_eq!(
            owner
                .has_table_privilege(role, table, privilege)
                .await
                .unwrap(),
            want,
            "{role} {privilege} {table}"
        );
    }
    let settings = owner.role_db_settings("paper_reset_rw").await.unwrap();
    assert!(
        settings.iter().any(|s| s == "statement_timeout=5s"),
        "{settings:?}"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn the_reset_role_inserts_one_request_per_utc_day_and_nothing_else() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let day = 86_400_000_i64 * 20_720; // a UTC midnight
    let reset = db.role("paper_reset_rw").await;
    let insert = |ms: i64| format!("INSERT INTO paper_reset_requests (requested_ms) VALUES ({ms})");
    reset
        .raw_exec(&insert(day + 5))
        .await
        .expect("the day's first request");
    let again = reset.raw_exec(&insert(day + 86_399_999)).await.unwrap_err();
    assert!(again.to_string().contains("duplicate key"), "{again}");
    reset
        .raw_exec(&insert(day + 86_400_000))
        .await
        .expect("the next UTC day");
    for sql in [
        "UPDATE paper_reset_requests SET requested_ms = 1",
        "DELETE FROM paper_reset_requests",
        "INSERT INTO paper_epochs (epoch, start_ms, cause, run_id, policies_sha256) VALUES (1, 1, 'reset', 1, 's')",
        "INSERT INTO runs (service, started_ms, version, config_hash, mode) VALUES ('x', 1, 'v', 'c', 'live')",
    ] {
        let e = reset.raw_exec(sql).await.unwrap_err();
        assert!(is_permission_denied(&e), "{sql}: {e}");
    }
    let run = owner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap();
    for (epoch, cause) in [(0, "reset"), (1, "manual")] {
        let e = owner
            .raw_exec(&format!(
                "INSERT INTO paper_epochs (epoch, start_ms, cause, run_id, policies_sha256) VALUES ({epoch}, 1, '{cause}', {run}, 's')"
            ))
            .await
            .unwrap_err();
        assert!(e.to_string().contains("check constraint"), "{e}");
    }
    reset.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run them to see them fail**

Run (PG prefix): `cargo test -p store --test roles migration_0009 -- --nocapture; cargo test -p store --test migrate`
Expected: FAIL to compile: `no method named 'has_table_privilege'`; after Step 4's tooling, the migrate test fails with `missing table paper_epochs`.

- [ ] **Step 3: Write the migration** — `crates/store/migrations/0009_paper_epochs.sql` (LF):

```sql
-- Phase 7a (spec 2026-09-29-phase7a-latency-answer-design.md §4.1, §5.1): paper epochs, reset
-- requests and the api's reset role. Runs as the owner (ct_owner). The runbook runs it in the
-- stopped-scanner window: the new paper_ledger index locks inserts on that table while it builds.

-- 1. One reset request per UTC day (the unique utc_day is the whole race story: a second request
--    the same day fails, the api answers 429). The scanner applies requests; the api inserts them.
CREATE TABLE paper_reset_requests (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    requested_ms  BIGINT NOT NULL,
    utc_day       BIGINT GENERATED ALWAYS AS (requested_ms / 86400000) STORED UNIQUE
);

-- 2. Epochs are time boundaries: a paper row belongs to epoch e when its time is in
--    [start_ms(e), start_ms(e + 1)). Epoch 0 is implicit (everything before epoch 1). No key of any
--    paper table changes; account ids stay the same across epochs.
CREATE TABLE paper_epochs (
    epoch            INTEGER PRIMARY KEY CHECK (epoch >= 1),
    start_ms         BIGINT  NOT NULL UNIQUE,
    cause            TEXT    NOT NULL CHECK (cause IN ('policies', 'reset')),
    request_id       BIGINT  UNIQUE REFERENCES paper_reset_requests (id),
    run_id           BIGINT  NOT NULL REFERENCES runs (run_id),
    policies_sha256  TEXT    NOT NULL
);

-- 3. The ladder's per-account sums since an epoch start (spec §7.1).
CREATE INDEX paper_ledger_account_t ON paper_ledger (account_id, t_ms);

-- 4. Grants (explicit since 0003). scanner_rw reads requests and writes epoch rows; the readers
--    read both.
GRANT SELECT ON paper_reset_requests TO scanner_rw;
GRANT SELECT, INSERT ON paper_epochs TO scanner_rw;
GRANT SELECT ON paper_reset_requests, paper_epochs TO analyze_ro, analyze_rw;

-- 5. paper_reset_rw: the api's second pool. NOLOGIN here (a migration can't carry a secret); the
--    runbook sets LOGIN PASSWORD once and fresh volumes get it from 01-roles.sh. Roles are
--    cluster-wide: tolerate one that already exists. INSERT on the identity column needs no
--    sequence grant.
DO $$ BEGIN
    CREATE ROLE paper_reset_rw NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;
DO $$ BEGIN
    EXECUTE format('ALTER ROLE paper_reset_rw IN DATABASE %I SET statement_timeout = %L',
                   current_database(), '5s');
END $$;
GRANT USAGE ON SCHEMA public TO paper_reset_rw;
GRANT SELECT, INSERT ON paper_reset_requests TO paper_reset_rw;
GRANT SELECT ON paper_epochs TO paper_reset_rw;
```

- [ ] **Step 4: The test tooling read** — `crates/store/src/lib.rs`, after `role_db_settings` (line 162), inside `impl Store`:

```rust
    /// Admin/test tooling: `has_table_privilege(role, table, privilege)` in this database.
    pub async fn has_table_privilege(
        &self,
        role: &str,
        table: &str,
        privilege: &str,
    ) -> Result<bool, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT has_table_privilege($1::name, $2::text, $3::text)")
                .bind(role)
                .bind(table)
                .bind(privilege)
                .fetch_one(self.pool())
                .await?,
        )
    }
```

- [ ] **Step 5: Fresh volumes and compose** — append to `docker/postgres/init/01-roles.sh`:

```sh
# paper_reset_rw (Phase 7a) from RESET_RW_DB_PASSWORD (migration 0009 otherwise).
if [ -n "${RESET_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
       -v reset_rw_pw="$RESET_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE paper_reset_rw LOGIN PASSWORD :'reset_rw_pw';
ALTER ROLE paper_reset_rw SET statement_timeout = '5s';
SQL
fi
```
and update its header comment line 6 to end `… compact_rw (Phase 5b) likewise from COMPACT_RW_DB_PASSWORD (migration 0008 otherwise); paper_reset_rw (Phase 7a) from RESET_RW_DB_PASSWORD (0009).`. In `compose.yml`, postgres `environment:` (after line 21 `COMPACT_RW_DB_PASSWORD: ${COMPACT_RW_DB_PASSWORD:-}`) add:
```yaml
      RESET_RW_DB_PASSWORD: ${RESET_RW_DB_PASSWORD:-}
```

- [ ] **Step 6: Run the tests**

Run (PG prefix): `cargo test -p store`
Expected: PASS, including `migrations_apply_from_empty_and_are_idempotent`, both new role tests and `migration_checksums` (`2 passed`: 0009 is newer than every pinned one).
Run: `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok`
Expected: `config-ok`.

- [ ] **Step 7: CI checks and commit**

Run: `git ls-files --eol crates/store/migrations/0009_paper_epochs.sql; cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `i/lf    w/lf    attr/text eol=lf …0009_paper_epochs.sql` (after `git add`), clean fmt and clippy.
```bash
git add crates/store/migrations/0009_paper_epochs.sql crates/store/src/lib.rs crates/store/tests/migrate.rs crates/store/tests/roles.rs docker/postgres/init/01-roles.sh compose.yml
git commit -m "store: migration 0009 (paper_epochs, paper_reset_requests, paper_reset_rw, paper_ledger_account_t)"
```

---

### Task 5: store — `paper_epochs` in `Batch`, epoch status, reset requests, the reset pool

**Files:**
- Modify: `crates/store/src/rows.rs:316-411` (new row type; `Batch` field and its five methods)
- Modify: `crates/store/src/paper.rs:1-3, 159-177` (write epochs after the marks)
- Modify: `crates/store/src/error.rs:12-47` (`is_unique_violation`)
- Create: `crates/store/src/epochs.rs`
- Modify: `crates/store/src/lib.rs:5-34` (module and re-exports)
- Test: create `crates/store/tests/epochs.rs`; append to `crates/store/tests/batch.rs`

**Interfaces:**
- Consumes: migration 0009 (Task 4).
- Produces:
  - `pub struct PaperEpochRow { pub epoch: i32, pub start_ms: i64, pub cause: String, pub request_id: Option<i64>, pub run_id: i64, pub policies_sha256: String }`; `Batch::paper_epochs: Vec<PaperEpochRow>` (counted by `paper_rows`, moved by `take_paper`/`extend_paper`, split by `into_singles`, written by `write_batch` after the marks, `ON CONFLICT DO NOTHING`).
  - `pub struct PaperEpoch { epoch: i32, start_ms: i64, cause: String, request_id: Option<i64> }`, `pub struct ResetRequest { id: i64, requested_ms: i64 }` (Copy), `pub struct EpochStatus { current: i32, last_applied: Option<i64> }`, `pub enum ResetInsert { Queued(ResetRequest), DailyLimit }`, `pub const RESET_TABLES: [&str; 1]`.
  - `Store::connect_reset(opts: PgConnectOptions, statement_timeout_ms: u32, role: Option<&str>) -> Result<Store, StoreError>`; `role_writes_outside_reset(&self) -> Result<Vec<String>, StoreError>`; `paper_epochs(&self) -> Result<Vec<PaperEpoch>, StoreError>`; `paper_epoch_starts(&self) -> Result<Vec<i64>, StoreError>`; `epoch_status(&self) -> Result<EpochStatus, StoreError>`; `reset_requests_after(&self, after_id: i64) -> Result<Vec<ResetRequest>, StoreError>`; `pending_reset_request(&self) -> Result<Option<ResetRequest>, StoreError>`; `latest_reset_request(&self) -> Result<Option<ResetRequest>, StoreError>`; `insert_reset_request(&self, now_ms: i64) -> Result<ResetInsert, StoreError>`; `StoreError::is_unique_violation(&self) -> bool`.

- [ ] **Step 1: Write the failing tests** — create `crates/store/tests/epochs.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use store::*;

const DAY: i64 = 86_400_000;
/// A UTC midnight.
const D0: i64 = DAY * 20_720;

async fn live_run(store: &Store) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap()
}

fn epoch_row(
    epoch: i32,
    start_ms: i64,
    cause: &str,
    request_id: Option<i64>,
    run_id: i64,
) -> PaperEpochRow {
    PaperEpochRow {
        epoch,
        start_ms,
        cause: cause.into(),
        request_id,
        run_id,
        policies_sha256: "sha".into(),
    }
}

async fn reset_pool(db: &common::TestDb) -> Store {
    Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .unwrap()
}

#[tokio::test]
async fn an_epoch_row_is_written_with_its_state_in_one_batch_and_a_resend_changes_nothing() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = live_run(&owner).await;
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 0,
            last_applied: None
        }
    );
    let b = Batch {
        paper_epochs: vec![epoch_row(1, D0 + 1, "policies", None, run)],
        paper_state: vec![PaperStateRow {
            account_id: "P0-converter/rung-0".into(),
            state: "{}".into(),
            updated_ms: D0 + 2,
            policies_sha256: Some("sha".into()),
        }],
        ..Batch::default()
    };
    let scanner = db.role("scanner_rw").await;
    scanner
        .write_batch(&b)
        .await
        .expect("scanner_rw writes the epoch row and the state together");
    scanner
        .write_batch(&b)
        .await
        .expect("a resend after a lost ack is a no-op");
    assert_eq!(
        owner.paper_epochs().await.unwrap(),
        vec![PaperEpoch {
            epoch: 1,
            start_ms: D0 + 1,
            cause: "policies".into(),
            request_id: None
        }]
    );
    assert_eq!(owner.paper_epoch_starts().await.unwrap(), vec![D0 + 1]);
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 1,
            last_applied: None
        }
    );
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn requests_collapse_into_the_epoch_that_names_the_highest_one() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = live_run(&owner).await;
    let reset = reset_pool(&db).await;
    // two requests across UTC midnight: both land (spec 7a §5.1 races)
    let ResetInsert::Queued(a) = reset.insert_reset_request(D0 + DAY - 1).await.unwrap() else {
        panic!("queued")
    };
    let ResetInsert::Queued(b) = reset.insert_reset_request(D0 + DAY + 1).await.unwrap() else {
        panic!("queued")
    };
    assert_eq!(owner.reset_requests_after(0).await.unwrap(), vec![a, b]);
    assert_eq!(owner.reset_requests_after(a.id).await.unwrap(), vec![b]);
    assert_eq!(reset.pending_reset_request().await.unwrap(), Some(b));
    assert_eq!(reset.latest_reset_request().await.unwrap(), Some(b));
    // the scanner collapses both into ONE epoch naming the highest id
    owner
        .write_batch(&Batch {
            paper_epochs: vec![epoch_row(1, D0 + DAY + 5, "reset", Some(b.id), run)],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        reset.pending_reset_request().await.unwrap(),
        None,
        "the lower id collapsed into the epoch is not pending either"
    );
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 1,
            last_applied: Some(b.id)
        }
    );
    reset.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_second_request_on_the_same_utc_day_is_the_daily_limit() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = reset_pool(&db).await;
    assert!(matches!(
        reset.insert_reset_request(D0 + 10).await.unwrap(),
        ResetInsert::Queued(r) if r.requested_ms == D0 + 10
    ));
    assert_eq!(
        reset.insert_reset_request(D0 + DAY - 1).await.unwrap(),
        ResetInsert::DailyLimit
    );
    assert!(matches!(
        reset.insert_reset_request(D0 + DAY).await.unwrap(),
        ResetInsert::Queued(_)
    ));
    reset.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn the_reset_role_writes_only_reset_requests() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = reset_pool(&db).await;
    assert_eq!(
        reset.role_writes_outside_reset().await.unwrap(),
        Vec::<String>::new()
    );
    assert!(
        !owner.role_writes_outside_reset().await.unwrap().is_empty(),
        "the owner can write everything: refused"
    );
    owner
        .raw_exec(
            "GRANT INSERT ON episodes TO paper_reset_rw; GRANT UPDATE ON paper_reset_requests TO paper_reset_rw",
        )
        .await
        .unwrap();
    assert_eq!(
        reset.role_writes_outside_reset().await.unwrap(),
        vec!["episodes".to_string(), "paper_reset_requests".to_string()]
    );
    reset.close().await;
    db.drop(owner).await;
}
```
Append to `crates/store/tests/batch.rs`:
```rust
#[test]
fn paper_epochs_travel_with_the_paper_rows() {
    let row = PaperEpochRow {
        epoch: 1,
        start_ms: 5,
        cause: "reset".into(),
        request_id: Some(3),
        run_id: 1,
        policies_sha256: "s".into(),
    };
    let mut a = Batch {
        paper_epochs: vec![row.clone()],
        ..Batch::default()
    };
    assert_eq!((a.paper_rows(), a.rows()), (1, 1));
    let taken = a.take_paper();
    assert_eq!(taken.paper_epochs, vec![row.clone()]);
    assert!(a.is_empty());
    let mut b = Batch::default();
    b.extend_paper(taken);
    assert_eq!(b.paper_epochs, vec![row.clone()]);
    let singles = b.into_singles();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].paper_epochs, vec![row]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run (PG prefix): `cargo test -p store --test epochs --test batch`
Expected: FAIL to compile: `cannot find struct 'PaperEpochRow'`, `no method named 'connect_reset'`, `epoch_status`, ….

- [ ] **Step 3: The row type and the `Batch` field** — `crates/store/src/rows.rs`, before `pub struct Batch` (line 316):

```rust
/// Phase 7a (migration 0009): an epoch the live scanner started. Written only by the scanner,
/// inside a `WriterMsg::PaperReset` batch with the rows it closes and the state it saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperEpochRow {
    pub epoch: i32,
    pub start_ms: i64,
    /// 'policies' or 'reset'.
    pub cause: String,
    pub request_id: Option<i64>,
    pub run_id: i64,
    pub policies_sha256: String,
}
```
In `Batch`, after `pub paper_marks: Vec<PaperMarkRow>,` add:
```rust
    /// Phase 7a: written after the marks, before `paper_state` (spec 7a §4.2).
    pub paper_epochs: Vec<PaperEpochRow>,
```
`paper_rows`: add `+ self.paper_epochs.len()` after `+ self.paper_marks.len()`. `take_paper`: add `paper_epochs: std::mem::take(&mut self.paper_epochs),` after the `paper_marks` line. `extend_paper`: add `self.paper_epochs.extend(other.paper_epochs);` after the `paper_marks` line. `into_singles`: add `paper_epochs,` after `paper_marks,` in the `each!` list.

- [ ] **Step 4: Write epochs in `write_paper`** — `crates/store/src/paper.rs`: import `PaperEpochRow` is not needed (fields only). Insert after the `paper_marks` loop (after line 177), before the `paper_state` loop:

```rust
    for chunk in b.paper_epochs.chunks(CHUNK) {
        let mut qb = QueryBuilder::<Postgres>::new(
            "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) ",
        );
        qb.push_values(chunk, |mut s, r| {
            s.push_bind(r.epoch)
                .push_bind(r.start_ms)
                .push_bind(clean(&r.cause))
                .push_bind(r.request_id)
                .push_bind(r.run_id)
                .push_bind(clean(&r.policies_sha256));
        });
        // a resend after a lost ack changes nothing, as for every other paper table
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(&mut *conn).await?;
    }
```
Update the module doc's first line to `//! Paper-trading rows (S3c, migration 0004; epochs 0009). Every table but \`paper_state\` is append-only and`.

- [ ] **Step 5: `is_unique_violation`** — `crates/store/src/error.rs`, inside `impl StoreError` after `is_data_error`:

```rust
    /// True for a unique-constraint violation (SQLSTATE 23505): e.g. a second reset request on
    /// the same UTC day (spec 7a §4.1).
    pub fn is_unique_violation(&self) -> bool {
        match self {
            StoreError::Db(sqlx::Error::Database(e)) => e.code().is_some_and(|c| c == "23505"),
            _ => false,
        }
    }
```

- [ ] **Step 6: The epochs module** — create `crates/store/src/epochs.rs`:

```rust
//! Phase 7a (spec 2026-09-29-phase7a-latency-answer-design.md §4, §5): paper epochs and reset
//! requests. The scanner reads the epoch status once at start and polls new requests off its
//! engine loop; the api's reset pool (paper_reset_rw) inserts one request per UTC day and reads
//! what is pending. Epoch rows are written by the scanner through `Batch::paper_epochs`.
use crate::{Store, StoreError};
use sqlx::postgres::PgConnectOptions;

/// The only table the reset role may write, and only INSERT (spec 7a §5.1).
pub const RESET_TABLES: [&str; 1] = ["paper_reset_requests"];
/// One epoch per UTC day at most: this is decades.
const MAX_EPOCHS: i64 = 10_000;
const MAX_REQUESTS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PaperEpoch {
    pub epoch: i32,
    pub start_ms: i64,
    /// 'policies' or 'reset'.
    pub cause: String,
    pub request_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct ResetRequest {
    pub id: i64,
    pub requested_ms: i64,
}

/// What the scanner reads once at start (spec 7a §4.2); the loop then keeps it in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochStatus {
    /// The newest epoch; 0 before the first (epoch 0 is implicit).
    pub current: i32,
    /// The highest request id an epoch applied.
    pub last_applied: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetInsert {
    Queued(ResetRequest),
    /// The unique `utc_day` refused it: one request per UTC day.
    DailyLimit,
}

impl Store {
    /// The api's reset pool: ONE writable connection with its own statement timeout (on top of
    /// the role's 5 s). `role` is for tests (SET ROLE from an owner URL).
    pub async fn connect_reset(
        opts: PgConnectOptions,
        statement_timeout_ms: u32,
        role: Option<&str>,
    ) -> Result<Self, StoreError> {
        let timeout = statement_timeout_ms.to_string();
        let opts = opts.options([("statement_timeout", timeout.as_str())]);
        match role {
            Some(r) => Self::connect_with_role(opts, 1, r).await,
            None => Self::connect_with(opts, 1).await,
        }
    }

    /// Tables this role could write beyond an INSERT into paper_reset_requests: INSERT, UPDATE,
    /// DELETE or TRUNCATE on any other table, or UPDATE, DELETE or TRUNCATE on that one. The api
    /// refuses to start unless this is empty (spec 7a §5.1).
    pub async fn role_writes_outside_reset(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') \
               AND (has_table_privilege(current_user, c.oid, 'UPDATE') \
                 OR has_table_privilege(current_user, c.oid, 'DELETE') \
                 OR has_table_privilege(current_user, c.oid, 'TRUNCATE') \
                 OR (NOT (c.relname::text = ANY($1)) \
                     AND has_table_privilege(current_user, c.oid, 'INSERT'))) \
             ORDER BY 1",
        )
        .bind(RESET_TABLES.map(str::to_string).to_vec())
        .fetch_all(self.pool())
        .await?)
    }

    /// Every epoch, oldest first.
    pub async fn paper_epochs(&self) -> Result<Vec<PaperEpoch>, StoreError> {
        Ok(sqlx::query_as::<_, PaperEpoch>(
            "SELECT epoch, start_ms, cause, request_id FROM paper_epochs ORDER BY epoch LIMIT $1",
        )
        .bind(MAX_EPOCHS)
        .fetch_all(self.pool())
        .await?)
    }

    /// Every epoch's start, ascending (analyze's Edge capture, spec 7a §6).
    pub async fn paper_epoch_starts(&self) -> Result<Vec<i64>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT start_ms FROM paper_epochs ORDER BY start_ms LIMIT $1")
                .bind(MAX_EPOCHS)
                .fetch_all(self.pool())
                .await?,
        )
    }

    pub async fn epoch_status(&self) -> Result<EpochStatus, StoreError> {
        let (current, last_applied): (i32, Option<i64>) = sqlx::query_as(
            "SELECT COALESCE(max(epoch), 0)::int4, max(request_id) FROM paper_epochs",
        )
        .fetch_one(self.pool())
        .await?;
        Ok(EpochStatus {
            current,
            last_applied,
        })
    }

    /// Requests with id > `after_id`, oldest first (the scanner's poll).
    pub async fn reset_requests_after(
        &self,
        after_id: i64,
    ) -> Result<Vec<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests WHERE id > $1 ORDER BY id LIMIT $2",
        )
        .bind(after_id)
        .bind(MAX_REQUESTS)
        .fetch_all(self.pool())
        .await?)
    }

    /// The newest request no epoch has applied yet (`id > max(paper_epochs.request_id)`).
    pub async fn pending_reset_request(&self) -> Result<Option<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests \
             WHERE id > COALESCE((SELECT max(request_id) FROM paper_epochs), 0) \
             ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }

    /// The newest request, applied or not (`next_allowed_ms`).
    pub async fn latest_reset_request(&self) -> Result<Option<ResetRequest>, StoreError> {
        Ok(sqlx::query_as::<_, ResetRequest>(
            "SELECT id, requested_ms FROM paper_reset_requests ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }

    /// One reset request at `now_ms`; the unique `utc_day` makes a second one the same UTC day
    /// `DailyLimit`, even when two requests race the api's pending check.
    pub async fn insert_reset_request(&self, now_ms: i64) -> Result<ResetInsert, StoreError> {
        let r = sqlx::query_as::<_, ResetRequest>(
            "INSERT INTO paper_reset_requests (requested_ms) VALUES ($1) RETURNING id, requested_ms",
        )
        .bind(now_ms)
        .fetch_one(self.pool())
        .await;
        match r {
            Ok(q) => Ok(ResetInsert::Queued(q)),
            Err(e) => {
                let e = StoreError::from(e);
                if e.is_unique_violation() {
                    Ok(ResetInsert::DailyLimit)
                } else {
                    Err(e)
                }
            }
        }
    }
}
```
`crates/store/src/lib.rs`: add `mod epochs;` after `mod compact_io;` and, after the `pub use compact_io::{…};` block:
```rust
pub use epochs::{EpochStatus, PaperEpoch, RESET_TABLES, ResetInsert, ResetRequest};
```

- [ ] **Step 7: Run the tests**

Run (PG prefix): `cargo test -p store`
Expected: PASS.

- [ ] **Step 8: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
```bash
git add crates/store/src/rows.rs crates/store/src/paper.rs crates/store/src/error.rs crates/store/src/epochs.rs crates/store/src/lib.rs crates/store/tests/epochs.rs crates/store/tests/batch.rs
git commit -m "store: paper_epochs in Batch, epoch status, reset requests and the reset pool"
```

---

### Task 6: store — epoch-aware dashboard reads and the ladder's sums

**Files:**
- Modify: `crates/store/src/read_paper.rs:35-46` (`PaperPosition`), `:74-85` (`paper_ready`), `:111-132` (`paper_equity`), `:140-176` (`paper_positions`), `:180-224` (`paper_ledger_page`)
- Create: `crates/store/src/ladder.rs`
- Modify: `crates/store/src/lib.rs` (module, re-export)
- Modify: `bins/api/src/paper.rs:169-172, 214-217` (pass `None`: the api's epoch parameter is Task 12)
- Test: `crates/store/tests/read_paper.rs` (update calls and two position tests; append two tests), create `crates/store/tests/ladder.rs`; `bins/api/tests/paper.rs:129-155` (the positions test gets its account)

**Interfaces:**
- Consumes: `paper_epochs` (Task 4), the `paper_ledger_account_t` index.
- Produces:
  - `paper_equity(&self, account_id: &str, from_ms: i64, to_ms: i64, epoch: Option<i32>) -> Result<Vec<EquityPoint>, StoreError>` and `paper_ledger_page(&self, account_id: &str, before: Option<(i64, i64)>, limit: i64, hide_marks: bool, epoch: Option<i32>) -> Result<LedgerPage, StoreError>` (`None` = the current epoch; epoch 0 = before epoch 1; an unknown epoch = empty).
  - `PaperPosition::end_date: Option<String>`; `paper_positions` keeps only the newest live run's account ids.
  - `pub struct LadderSums { account_id: String, n_exec: i64, n_leg_risk: i64, n_missed: i64, gains: Decimal, leg_risk: Decimal, settle: Decimal, unrealized: Option<Decimal>, equity: Option<Decimal> }`; `ladder_as_of(&self, accounts: &[String], start_ms: i64) -> Result<Option<i64>, StoreError>`; `ladder_sums(&self, accounts: &[String], start_ms: i64, as_of_ms: i64) -> Result<Vec<LadderSums>, StoreError>`; `live_run_ids_between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<i64>, StoreError>`; `newest_live_host(&self) -> Result<Option<String>, StoreError>`.

- [ ] **Step 1: Write the failing tests** — in `crates/store/tests/read_paper.rs`: append `, None` as the last argument of every `paper_equity(` and `paper_ledger_page(` call (lines 84, 91, 171, 324, 338, 380, 386, 392). In `positions_parse_a_real_sim_account_and_ignore_odd_states` insert after the events/legs `raw_exec` (after line 205):
```rust
    let live = run(&owner, "live").await;
    account(&owner, live, "P1-settlement/sp-idle", "P1-settlement", "sp-idle").await;
```
and add `end_date: None,` to both expected `PaperPosition`s. In `positions_skip_a_row_with_non_numeric_qty_or_basis` insert as its first statements after `fresh()`:
```rust
    let live = run(&owner, "live").await;
    account(&owner, live, "good", "P0-converter", "x").await;
```
and add `end_date: None,` to its expected `PaperPosition`. Append:
```rust
async fn epochs(owner: &Store, run_id: i64, starts: &[(i32, i64)]) {
    for (e, s) in starts {
        owner
            .raw_exec(&format!(
                "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
                 VALUES ({e}, {s}, 'policies', NULL, {run_id}, 'sha')"
            ))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn equity_and_ledger_follow_epoch_bounds_to_the_millisecond() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let id = "P0-converter/sp-idle";
    account(&owner, live, id, "P0-converter", "sp-idle").await;
    for (t, eq) in [
        (1_000, "999"),
        (2_000, "1000"),
        (2_999, "1001"),
        (3_000, "1000"),
        (4_000, "1002"),
    ] {
        mark(&owner, live, id, t, eq).await;
    }
    for seq in 1..=4 {
        ledger(&owner, live, id, seq).await; // t = seq × 1 000
    }
    let analyze = ro(&db).await;
    let pts = |v: Vec<EquityPoint>| v.into_iter().map(|p| p.t_ms).collect::<Vec<_>>();
    assert_eq!(
        pts(analyze.paper_equity(id, 0, 10_000, None).await.unwrap()),
        vec![1_000, 2_000, 2_999, 3_000, 4_000],
        "no epoch row yet: everything is epoch 0, the current one"
    );
    epochs(&owner, live, &[(1, 2_000), (2, 3_000)]).await;
    assert_eq!(
        pts(analyze.paper_equity(id, 0, 10_000, None).await.unwrap()),
        vec![3_000, 4_000],
        "the current epoch starts AT its start_ms"
    );
    assert_eq!(
        pts(analyze.paper_equity(id, 0, 10_000, Some(1)).await.unwrap()),
        vec![2_000, 2_999]
    );
    assert_eq!(
        pts(analyze.paper_equity(id, 0, 10_000, Some(0)).await.unwrap()),
        vec![1_000]
    );
    assert!(
        analyze
            .paper_equity(id, 0, 10_000, Some(7))
            .await
            .unwrap()
            .is_empty(),
        "an unknown epoch is empty, not an error"
    );
    let seqs = |p: LedgerPage| p.rows.into_iter().map(|r| r.seq).collect::<Vec<_>>();
    assert_eq!(
        seqs(analyze.paper_ledger_page(id, None, 50, false, None).await.unwrap()),
        vec![4, 3]
    );
    assert_eq!(
        seqs(analyze.paper_ledger_page(id, None, 50, false, Some(1)).await.unwrap()),
        vec![2]
    );
    assert_eq!(
        seqs(analyze.paper_ledger_page(id, None, 50, false, Some(0)).await.unwrap()),
        vec![1]
    );
    analyze.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_are_the_newest_live_runs_accounts_with_their_markets_end_date() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) \
             VALUES ('ev1', 's', 'BTC above 100k on October 3?', NULL, false, 2, 1, 1); \
             INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, end_date, first_seen_ms, last_seen_ms) \
             VALUES ('m1', 'ev1', 'y1', 'n1', 'live', 0.07, 1, '2026-10-03T12:00:00Z', 1, 1)",
        )
        .await
        .unwrap();
    let old = run(&owner, "live").await;
    let live = run(&owner, "live").await;
    account(&owner, old, "P0-converter/gone", "P0-converter", "gone").await;
    account(&owner, live, "P0-converter/rung-0", "P0-converter", "rung-0").await;
    let state = r#"{"positions":[[{"market_id":"m1","yes_token":"y1","outcome":"no"},{"qty":"10","basis":"6"}]],"returned":[]}"#;
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_state (account_id, state, updated_ms) VALUES \
             ('P0-converter/rung-0', '{state}', 1), ('P0-converter/gone', '{state}', 1)"
        ))
        .await
        .unwrap();
    let analyze = ro(&db).await;
    assert_eq!(
        analyze.paper_positions().await.unwrap(),
        vec![PaperPosition {
            account_id: "P0-converter/rung-0".into(),
            market_id: "m1".into(),
            outcome: "no".into(),
            qty: dec!(10),
            basis: dec!(6),
            returned: false,
            event_title: Some("BTC above 100k on October 3?".into()),
            end_date: Some("2026-10-03T12:00:00Z".into()),
        }],
        "a removed account's stale paper_state row never shows"
    );
    analyze.close().await;
    db.drop(owner).await;
}
```
Create `crates/store/tests/ladder.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use rust_decimal_macros::dec;
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

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

async fn account(owner: &Store, run: i64, id: &str) {
    sql(owner, format!(
        "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
         VALUES ({run}, '{id}', 'P0-converter', 'x', 0, 1000, 100, 'sha')"
    ))
    .await;
}

async fn led(owner: &Store, run: i64, id: &str, seq: i64, t: i64, kind: &str, realized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
         qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
         VALUES ({run}, '{id}', {seq}, {t}, '{kind}', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, {realized}, 1000, NULL)"
    ))
    .await;
}

async fn mark(owner: &Store, run: i64, id: &str, t: i64, equity: &str, unrealized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
         max_drawdown, n_positions) VALUES ({run}, '{id}', {t}, 1000, {equity}, 0, {unrealized}, 0, 0, 0, 0)"
    ))
    .await;
}

async fn exec(owner: &Store, run: i64, id: &str, ep: &str, t: i64, q_eff: &str, leg: &str) {
    sql(owner, format!(
        "INSERT INTO paper_execs (run_id, account_id, episode_id, exec_ms, q_eff, leg_risk_qty, pnl_at_tx, hit) \
         VALUES ({run}, '{id}', '{ep}', {t}, {q_eff}, {leg}, NULL, false)"
    ))
    .await;
}

#[tokio::test]
async fn the_cut_is_the_latest_common_mark_and_each_sum_stops_at_its_accounts_mark_row() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = run(&owner, "live").await;
    let replay = run(&owner, "replay").await;
    for r in [live, replay] {
        account(&owner, r, "A").await;
        account(&owner, r, "B").await;
    }
    led(&owner, live, "A", 1, 50, "convert", "100").await; // before the epoch start (100)
    led(&owner, live, "A", 2, 100, "mark", "0").await;
    led(&owner, live, "A", 3, 150, "convert", "3").await;
    led(&owner, live, "A", 4, 200, "mark", "0").await;
    led(&owner, live, "A", 5, 200, "sell", "-7").await; // same ms, after A's mark: not in it
    led(&owner, live, "A", 6, 300, "mark", "0").await; // only A: not a common cut
    led(&owner, live, "B", 1, 100, "mark", "0").await;
    led(&owner, live, "B", 2, 180, "settle", "1").await;
    led(&owner, live, "B", 3, 200, "mark", "0").await;
    led(&owner, replay, "A", 1, 150, "convert", "999").await; // replay rows never count
    mark(&owner, live, "A", 200, "1005", "2").await;
    mark(&owner, live, "B", 200, "1001", "0").await;
    mark(&owner, live, "B", 250, "1004", "3").await; // a resolution row: no ledger mark
    exec(&owner, live, "A", "e1", 120, "0", "5").await;
    exec(&owner, live, "A", "e2", 130, "10", "0").await;
    exec(&owner, live, "A", "e3", 250, "10", "0").await; // after the cut
    exec(&owner, replay, "A", "e1", 120, "10", "0").await;
    let ro = Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    let ids = vec!["A".to_string(), "B".to_string()];
    assert_eq!(ro.ladder_as_of(&ids, 100).await.unwrap(), Some(200));
    assert_eq!(ro.ladder_as_of(&ids[..1], 100).await.unwrap(), Some(300));
    assert_eq!(ro.ladder_as_of(&ids, 250).await.unwrap(), None);
    assert_eq!(ro.ladder_as_of(&[], 0).await.unwrap(), None);
    assert_eq!(
        ro.ladder_sums(&ids, 100, 200).await.unwrap(),
        vec![
            LadderSums {
                account_id: "A".into(),
                n_exec: 2,
                n_leg_risk: 1,
                n_missed: 1,
                gains: dec!(3),
                leg_risk: dec!(0),
                settle: dec!(0),
                unrealized: Some(dec!(2)),
                equity: Some(dec!(1005)),
            },
            LadderSums {
                account_id: "B".into(),
                n_exec: 0,
                n_leg_risk: 0,
                n_missed: 0,
                gains: dec!(0),
                leg_risk: dec!(0),
                settle: dec!(1),
                unrealized: Some(dec!(0)),
                equity: Some(dec!(1001)),
            },
        ],
        "A: 1005 − 1000 = 3 + 2; B: 1001 − 1000 = 1 + 0"
    );
    ro.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn measured_latency_reads_live_runs_in_the_window_and_the_newest_host() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let old = run(&owner, "live").await;
    owner.end_run(old, 500, "shutdown").await.unwrap();
    let _replay = run(&owner, "replay").await;
    let live = run(&owner, "live").await;
    let ro = Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap();
    assert_eq!(ro.live_run_ids_between(1_000, 2_000).await.unwrap(), vec![live]);
    assert_eq!(ro.live_run_ids_between(0, 2_000).await.unwrap(), vec![old, live]);
    assert_eq!(ro.newest_live_host().await.unwrap(), None);
    let host = |run_id: i64, v: &str| RunPremiseRow {
        run_id,
        area: "host".into(),
        version: v.into(),
    };
    owner
        .insert_run_premises(&[host(old, "1:pc-dev|lt25")])
        .await
        .unwrap();
    assert_eq!(
        ro.newest_live_host().await.unwrap().as_deref(),
        Some("1:pc-dev|lt25"),
        "the newest live run WITH a host row (it arrives after 10 health minutes)"
    );
    owner
        .insert_run_premises(&[host(live, "1:vps-kvm2|100-250")])
        .await
        .unwrap();
    assert_eq!(
        ro.newest_live_host().await.unwrap().as_deref(),
        Some("1:vps-kvm2|100-250")
    );
    ro.close().await;
    db.drop(owner).await;
}
```
In `bins/api/tests/paper.rs`, `positions_come_from_the_live_paper_state` (line 130): insert after `fresh()`:
```rust
    let live = common::run(&owner, "live").await;
    account(&owner, live, "P1-settlement/sp-idle", "P1-settlement", "sp-idle").await;
```

- [ ] **Step 2: Run them to see them fail**

Run (PG prefix): `cargo test -p store --test read_paper --test ladder`
Expected: FAIL to compile: `this method takes 3 arguments but 4 arguments were supplied` (`paper_equity`), `struct 'PaperPosition' has no field named 'end_date'`, `cannot find struct 'LadderSums'`.

- [ ] **Step 3: Epoch filters and positions** — `crates/store/src/read_paper.rs`.

Add after `const LEDGER_MAX: i64 = 200;`:
```rust
/// Phase 7a (spec §4.1, §6): the bounds of epoch `$k` (NULL: the current, newest one) as a CTE
/// `eb(lo, hi)`: a row belongs to it when lo ≤ t < hi (hi NULL: open). Epoch 0 is everything
/// before epoch 1; an unknown epoch has lo NULL, so it matches nothing (empty, never an error).
fn epoch_bounds(k: usize) -> String {
    format!(
        "n AS (SELECT COALESCE(${k}::int4, (SELECT max(epoch) FROM paper_epochs), 0) AS e), \
         eb AS (SELECT CASE WHEN n.e = 0 THEN '-9223372036854775808'::int8 \
                            ELSE (SELECT p.start_ms FROM paper_epochs p WHERE p.epoch = n.e) END AS lo, \
                       (SELECT min(p.start_ms) FROM paper_epochs p WHERE p.epoch > n.e) AS hi \
                FROM n)"
    )
}
```
`PaperPosition` (lines 37-46): add after `pub event_title: Option<String>,`:
```rust
    /// The market's end date (`legs.end_date`, as Gamma sent it), for days to resolution. The
    /// 09-27 spec §7.4 says `events.end_date`; the column lives on `legs` (0001_init.sql:58).
    pub end_date: Option<String>,
```
`paper_ready` (lines 77-84): add `AND to_regclass('public.paper_epochs') IS NOT NULL` inside the SQL after the `paper_state` check, and change its doc to `/// True once S3c's migration 0004 and Phase 7a's 0009 are applied (every table the dashboard reads).`.
`paper_equity` (lines 111-132), replace the whole function with:
```rust
    /// Minute marks of one account across live runs (restarts restore the account), oldest
    /// first, inside epoch `epoch` (None: the current one, spec 7a §6).
    pub async fn paper_equity(
        &self,
        account_id: &str,
        from_ms: i64,
        to_ms: i64,
        epoch: Option<i32>,
    ) -> Result<Vec<EquityPoint>, StoreError> {
        if !self.paper_ready().await? {
            return Ok(Vec::new());
        }
        let sql = format!(
            "WITH {} \
             SELECT m.t_ms, m.equity FROM paper_marks m CROSS JOIN eb \
             WHERE m.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND m.account_id = $1 AND m.t_ms >= $2 AND m.t_ms < $3 \
               AND m.t_ms >= eb.lo AND (eb.hi IS NULL OR m.t_ms < eb.hi) \
             ORDER BY m.t_ms, m.run_id LIMIT 20160",
            epoch_bounds(4)
        );
        Ok(sqlx::query_as::<_, EquityPoint>(&sql)
            .bind(account_id)
            .bind(from_ms)
            .bind(to_ms)
            .bind(epoch)
            .fetch_all(self.pool())
            .await?)
    }
```
`paper_positions` (lines 145-172): in the `WITH s AS (…)` CTE, replace
```
                 SELECT account_id, state::jsonb AS j FROM paper_state WHERE state IS JSON OBJECT \
```
with
```
                 SELECT account_id, state::jsonb AS j FROM paper_state WHERE state IS JSON OBJECT \
                   AND account_id IN (SELECT a.account_id FROM paper_accounts a \
                       WHERE a.run_id = (SELECT max(a2.run_id) FROM paper_accounts a2 \
                                         JOIN runs r2 ON r2.run_id = a2.run_id WHERE r2.mode = 'live')) \
```
and replace `e.title AS event_title \` with `e.title AS event_title, l.end_date \` (the `legs l` join is already there). Extend its doc with: `/// Only the newest live run's account ids (spec 7a §6: a removed account's stale row never shows).`.
`paper_ledger_page` (lines 180-224): add the parameter `epoch: Option<i32>,` after `hide_marks: bool,`, and replace its query string and binds with:
```rust
        let sql = format!(
            "WITH {} \
             SELECT l.run_id, l.account_id, l.seq, l.t_ms, l.kind, l.episode_id, l.market_id, \
                    l.outcome, l.qty, l.price, l.cash_delta, l.fees, l.realized_delta, \
                    l.cash_after, l.equity \
             FROM paper_ledger l CROSS JOIN eb \
             WHERE l.run_id IN (SELECT run_id FROM runs WHERE mode = 'live') \
               AND l.account_id = $1 \
               AND ($2::int8 IS NULL OR (l.run_id, l.seq) < ($2::int8, $3::int8)) \
               AND (NOT $5 OR l.kind <> 'mark') \
               AND l.t_ms >= eb.lo AND (eb.hi IS NULL OR l.t_ms < eb.hi) \
             ORDER BY l.run_id DESC, l.seq DESC LIMIT $4",
            epoch_bounds(6)
        );
        let mut rows = sqlx::query_as::<_, PaperLedgerEntry>(&sql)
            .bind(account_id)
            .bind(b_run)
            .bind(b_seq)
            .bind(limit + 1)
            .bind(hide_marks)
            .bind(epoch)
            .fetch_all(self.pool())
            .await?;
```
and extend its doc: `/// Inside epoch \`epoch\` (None: the current one, spec 7a §6).`.

- [ ] **Step 4: The ladder module** — create `crates/store/src/ladder.rs`:

```rust
//! Phase 7a (spec 2026-09-29 §7.1): what `GET /api/paper/ladder` reads, live runs only. Every
//! sum stops at ONE common cut, the latest instant every ladder account has a ledger `mark` row,
//! and within it at each account's own mark row by (run_id, seq). So equity − bankroll =
//! Σ realized_delta + unrealized holds exactly even when a trade lands in the mark's own
//! millisecond (a 0 ms rung) or a resolution marks one account later.
use crate::{Store, StoreError};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LadderSums {
    pub account_id: String,
    /// Executed attempts in [start, cut].
    pub n_exec: i64,
    /// … with leg risk (`leg_risk_qty > 0`).
    pub n_leg_risk: i64,
    /// … that missed (`q_eff = 0`, SWEET's meaning).
    pub n_missed: i64,
    /// Σ realized_delta of `convert` rows (basket gains, fees included).
    pub gains: Decimal,
    /// Σ realized_delta of `sell` rows (unwinds, fees included).
    pub leg_risk: Decimal,
    /// Σ realized_delta of `settle` rows.
    pub settle: Decimal,
    /// The account's `paper_marks` row at the cut (None: no mark there).
    pub unrealized: Option<Decimal>,
    pub equity: Option<Decimal>,
}

impl Store {
    /// The latest t_ms ≥ `start_ms` at which EVERY one of `accounts` has a live ledger `mark`
    /// row (minute marks, closes and `mark_now` mark every account; a resolution row does not).
    /// None before the first common mark, or with no accounts.
    pub async fn ladder_as_of(
        &self,
        accounts: &[String],
        start_ms: i64,
    ) -> Result<Option<i64>, StoreError> {
        if accounts.is_empty() {
            return Ok(None);
        }
        Ok(sqlx::query_scalar(
            "SELECT l.t_ms FROM paper_ledger l JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
             WHERE l.account_id = ANY($1) AND l.kind = 'mark' AND l.t_ms >= $2 \
             GROUP BY l.t_ms HAVING count(DISTINCT l.account_id) = $3 \
             ORDER BY l.t_ms DESC LIMIT 1",
        )
        .bind(accounts.to_vec())
        .bind(start_ms)
        .bind(i64::try_from(accounts.len()).unwrap_or(i64::MAX))
        .fetch_optional(self.pool())
        .await?)
    }

    /// Per account, sorted by id: counts of execs in [start_ms, as_of_ms], Σ realized_delta per
    /// kind from `start_ms` up to the account's own mark row at `as_of_ms`, and that mark.
    pub async fn ladder_sums(
        &self,
        accounts: &[String],
        start_ms: i64,
        as_of_ms: i64,
    ) -> Result<Vec<LadderSums>, StoreError> {
        Ok(sqlx::query_as::<_, LadderSums>(
            "SELECT a.account_id, \
                    COALESCE(ex.n_exec, 0) AS n_exec, COALESCE(ex.n_leg_risk, 0) AS n_leg_risk, \
                    COALESCE(ex.n_missed, 0) AS n_missed, \
                    COALESCE(lg.gains, 0) AS gains, COALESCE(lg.leg_risk, 0) AS leg_risk, \
                    COALESCE(lg.settle, 0) AS settle, mk.unrealized, mk.equity \
             FROM unnest($1::text[]) AS a(account_id) \
             LEFT JOIN LATERAL ( \
                 SELECT l.run_id, l.seq FROM paper_ledger l \
                 JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
                 WHERE l.account_id = a.account_id AND l.kind = 'mark' AND l.t_ms = $3 \
                 ORDER BY l.run_id DESC, l.seq DESC LIMIT 1) cut ON true \
             LEFT JOIN LATERAL ( \
                 SELECT m.unrealized, m.equity FROM paper_marks m \
                 WHERE m.run_id = cut.run_id AND m.account_id = a.account_id AND m.t_ms = $3) mk ON true \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) AS n_exec, \
                        count(*) FILTER (WHERE x.leg_risk_qty > 0) AS n_leg_risk, \
                        count(*) FILTER (WHERE x.q_eff = 0) AS n_missed \
                 FROM paper_execs x JOIN runs r ON r.run_id = x.run_id AND r.mode = 'live' \
                 WHERE x.account_id = a.account_id AND x.exec_ms >= $2 AND x.exec_ms <= $3) ex ON true \
             LEFT JOIN LATERAL ( \
                 SELECT sum(l.realized_delta) FILTER (WHERE l.kind = 'convert') AS gains, \
                        sum(l.realized_delta) FILTER (WHERE l.kind = 'sell') AS leg_risk, \
                        sum(l.realized_delta) FILTER (WHERE l.kind = 'settle') AS settle \
                 FROM paper_ledger l JOIN runs r ON r.run_id = l.run_id AND r.mode = 'live' \
                 WHERE l.account_id = a.account_id AND l.t_ms >= $2 \
                   AND (l.run_id, l.seq) <= (cut.run_id, cut.seq)) lg ON true \
             ORDER BY a.account_id",
        )
        .bind(accounts.to_vec())
        .bind(start_ms)
        .bind(as_of_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// Live scanner runs that overlap [from_ms, to_ms) (Edge capture's measured latency).
    pub async fn live_run_ids_between(
        &self,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<i64>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT run_id FROM runs WHERE service = 'scanner' AND mode = 'live' \
               AND started_ms < $2 AND (ended_ms IS NULL OR ended_ms >= $1) \
             ORDER BY run_id LIMIT 1000",
        )
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(self.pool())
        .await?)
    }

    /// The host version (`1:<label>|<band>`) of the newest live scanner run that recorded one.
    pub async fn newest_live_host(&self) -> Result<Option<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT p.version FROM run_premises p JOIN runs r ON r.run_id = p.run_id \
             WHERE p.area = 'host' AND r.service = 'scanner' AND r.mode = 'live' \
             ORDER BY p.run_id DESC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await?)
    }
}
```
`crates/store/src/lib.rs`: add `mod ladder;` after `mod epochs;` and `pub use ladder::LadderSums;` after the `pub use epochs::…;` line.

- [ ] **Step 5: The api keeps compiling** — `bins/api/src/paper.rs`: line 171 `.paper_equity(&account_id, from_ms, to_ms)` → `.paper_equity(&account_id, from_ms, to_ms, None)`; line 216 `.paper_ledger_page(&account_id, before, limit, hide_marks)` → `.paper_ledger_page(&account_id, before, limit, hide_marks, None)`.

- [ ] **Step 6: Run the tests**

Run (PG prefix): `cargo test -p store && cargo test -p api`
Expected: PASS.

- [ ] **Step 7: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
```bash
git add crates/store/src/read_paper.rs crates/store/src/ladder.rs crates/store/src/lib.rs crates/store/tests/read_paper.rs crates/store/tests/ladder.rs bins/api/src/paper.rs bins/api/tests/paper.rs
git commit -m "store: epoch filters in paper_equity and paper_ledger_page, current-account positions with end_date, the ladder's sums"
```

---

### Task 7: scanner writer — `WriterMsg::PaperReset`, written alone and never split

**Files:**
- Modify: `bins/scanner/src/writer.rs:1-6` (doc), `:15-25` (`WriterMsg`), `:31-42` (`Pending`), `:52-58` (`rows`), `:75-99` (`push`, `enforce_cap`), `:211-245` (`flush`)
- Test: `bins/scanner/src/writer.rs` `tests` module (append)

**Interfaces:**
- Consumes: `Batch::paper_epochs` (Task 5).
- Produces: `WriterMsg::PaperReset(Box<Batch>)`; `pub enum GroupKind { Open, Sealed, Reset }`; `pub struct Group { pub kind: GroupKind, pub batch: Batch }`; `Pending::open_rows(&self) -> usize`; `Pending::next_group(&mut self) -> Option<Group>`; `Pending::failed(&mut self, g: Group, transient: bool) -> Option<Batch>` (Some = rows for the row-by-row quarantine).

- [ ] **Step 1: Write the failing tests** — append to the `tests` module of `bins/scanner/src/writer.rs`:

```rust
    fn reset_msg(seq: i64) -> (WriterMsg, Batch) {
        let mut b = match paper_ledger(seq) {
            WriterMsg::Paper(b) => *b,
            _ => unreachable!(),
        };
        b.paper_epochs.push(store::PaperEpochRow {
            epoch: 2,
            start_ms: seq,
            cause: "reset".into(),
            request_id: Some(7),
            run_id: 1,
            policies_sha256: "s".into(),
        });
        (WriterMsg::PaperReset(Box::new(b.clone())), b)
    }

    fn seqs(b: &Batch) -> Vec<i64> {
        b.paper_ledger.iter().map(|l| l.seq).collect()
    }

    #[test]
    fn a_reset_is_written_alone_after_the_rows_queued_before_it() {
        let mut p = Pending::new(100);
        p.push(WriterMsg::Episode(ep("poison", None)));
        p.push(paper_ledger(1));
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        p.push(paper_ledger(3));
        let first = p.next_group().unwrap();
        assert_eq!(first.kind, GroupKind::Sealed);
        assert_eq!((first.batch.episodes.len(), seqs(&first.batch)), (1, vec![1]));
        // the pending batch is rejected for its poison episode: it goes to the quarantine
        assert!(p.failed(first, false).is_some());
        let second = p.next_group().unwrap();
        assert_eq!(second.kind, GroupKind::Reset);
        assert_eq!(second.batch, reset, "the reset is written whole, alone");
        let third = p.next_group().unwrap();
        assert_eq!((third.kind, seqs(&third.batch)), (GroupKind::Open, vec![3]));
        assert!(p.next_group().is_none());
    }

    #[test]
    fn a_rejected_reset_is_dropped_whole_never_split() {
        let mut p = Pending::new(100);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        let g = p.next_group().unwrap();
        assert!(
            p.failed(g, false).is_none(),
            "never handed to the row-by-row quarantine"
        );
        assert_eq!(p.dropped, u64::try_from(reset.rows()).unwrap());
        assert!(p.next_group().is_none());
    }

    #[test]
    fn a_transient_failure_keeps_the_reset_ahead_of_newer_rows() {
        let mut p = Pending::new(100);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        let g = p.next_group().unwrap();
        p.push(paper_ledger(9)); // a new-epoch trade arrives while the write is failing
        assert!(p.failed(g, true).is_none());
        let again = p.next_group().unwrap();
        assert_eq!((again.kind, again.batch), (GroupKind::Reset, reset));
        let next = p.next_group().unwrap();
        assert_eq!((next.kind, seqs(&next.batch)), (GroupKind::Open, vec![9]));
    }

    #[test]
    fn a_failed_open_batch_retried_during_a_reset_still_goes_first() {
        let mut p = Pending::new(100);
        p.push(paper_ledger(1));
        let g = p.next_group().unwrap();
        assert_eq!(g.kind, GroupKind::Open);
        assert!(p.failed(g, true).is_none()); // back into the open rows
        let (msg, _) = reset_msg(2);
        p.push(msg);
        let first = p.next_group().unwrap();
        assert_eq!((first.kind, seqs(&first.batch)), (GroupKind::Sealed, vec![1]));
        assert_eq!(p.next_group().unwrap().kind, GroupKind::Reset);
    }

    #[test]
    fn the_cap_never_evicts_a_reset() {
        let mut p = Pending::new(2);
        let (msg, reset) = reset_msg(2);
        p.push(msg);
        for t in 0..4 {
            p.push(integ(t));
        }
        assert_eq!(p.open_rows(), 2, "only the open integrity rows are capped");
        assert_eq!(p.next_group().unwrap().batch, reset);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p scanner --lib writer`
Expected: FAIL to compile: `no variant named 'PaperReset'`, `cannot find type 'GroupKind'`, `no method named 'next_group'`.

- [ ] **Step 3: Implement** — `bins/scanner/src/writer.rs`.

Module doc (lines 1-6): append the line `//! A paper reset (Phase 7a §4.2) is a group of its own: written alone after everything queued before it, retried whole, and on a data error dropped whole (never quarantined row by row: the epoch row and \`paper_state\` must never commit apart).`.
`WriterMsg`, before `Flush(…)`:
```rust
    /// Phase 7a §4.2: one epoch's rows (close rows, the paper_epochs row, marks, paper_state),
    /// written alone in one transaction after everything queued before it.
    PaperReset(Box<Batch>),
```
Before `pub struct Pending`:
```rust
/// What a flush writes next, in order (spec 7a §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// The open rows (merged, deduplicated and capped as always).
    Open,
    /// Rows that were open when a reset arrived: written before it, quarantined if rejected.
    Sealed,
    /// A reset: written alone; a data error drops it whole.
    Reset,
}

#[derive(Debug)]
pub struct Group {
    pub kind: GroupKind,
    pub batch: Batch,
}
```
In `Pending`, add the field (after `paper: Batch,`):
```rust
    /// Groups ahead of the open rows, oldest first: rows sealed by a reset, then the reset.
    sealed: VecDeque<Group>,
```
Replace `rows` (lines 52-58) with:
```rust
    /// Every row waiting, sealed groups included (what the flush tick checks).
    pub fn rows(&self) -> usize {
        self.open_rows() + self.sealed.iter().map(|g| g.batch.rows()).sum::<usize>()
    }

    /// The open rows only (what the cap counts: sealed groups are never evicted).
    pub fn open_rows(&self) -> usize {
        self.episodes.len()
            + self.observations.len()
            + self.integrity.len()
            + self.health.len()
            + self.paper.paper_rows()
    }
```
In `push`, add the arm before `WriterMsg::Flush(_) => {}`:
```rust
            WriterMsg::PaperReset(b) => {
                let open = self.take();
                if open.rows() > 0 {
                    self.sealed.push_back(Group {
                        kind: GroupKind::Sealed,
                        batch: open,
                    });
                }
                self.sealed.push_back(Group {
                    kind: GroupKind::Reset,
                    batch: *b,
                });
            }
```
In `enforce_cap`, change `while self.rows() > self.max_rows {` to `while self.open_rows() > self.max_rows {`. After `restore` (line 132) add:
```rust
    /// The next group to write alone: a sealed group first, else the open rows.
    pub fn next_group(&mut self) -> Option<Group> {
        if let Some(g) = self.sealed.pop_front() {
            return Some(g);
        }
        if self.open_rows() == 0 {
            return None;
        }
        Some(Group {
            kind: GroupKind::Open,
            batch: self.take(),
        })
    }

    /// A group's write failed. Transient: it goes back, still ahead of everything newer (open
    /// rows through `restore`, sealed groups and resets to the front). Data error: a reset is
    /// dropped whole and counted (spec 7a §4.2, never split); other rows come back for the
    /// row-by-row quarantine.
    pub fn failed(&mut self, g: Group, transient: bool) -> Option<Batch> {
        match (g.kind, transient) {
            (GroupKind::Open, true) => {
                self.restore(g.batch);
                None
            }
            (_, true) => {
                self.sealed.push_front(g);
                None
            }
            (GroupKind::Reset, false) => {
                self.dropped += u64::try_from(g.batch.rows()).unwrap_or(u64::MAX);
                None
            }
            (_, false) => Some(g.batch),
        }
    }
```
Replace `flush` (lines 211-245) with:
```rust
/// Write every waiting group, each in its own transaction, in order. Transient failure → the
/// group goes back for a retry (returns false); data error → quarantine row by row, except a
/// reset, which is dropped whole and logged at error level. Returns true when nothing is left.
async fn flush(store: &Store, pending: &mut Pending, fails: &mut u32) -> bool {
    while let Some(g) = pending.next_group() {
        let rows = g.batch.rows();
        match tokio::time::timeout(Duration::from_secs(10), store.write_batch(&g.batch)).await {
            Ok(Ok(())) => *fails = 0,
            Ok(Err(e)) if e.is_data_error() => {
                // Postgres rejected the DATA (SQLSTATE 22/23): retrying the same rows can never succeed.
                *fails = 0;
                match pending.failed(g, false) {
                    Some(b) => {
                        let dropped = quarantine(store, b).await;
                        pending.dropped += dropped;
                        eprintln!("writer: data error ({e}); quarantined batch, dropped {dropped} rows");
                    }
                    None => eprintln!(
                        "writer: ERROR: paper reset rejected ({e}); dropped whole ({rows} rows), never split"
                    ),
                }
            }
            Ok(Err(e)) => {
                eprintln!("writer: batch failed ({rows} rows): {e}");
                let _ = pending.failed(g, true);
                *fails += 1;
                return false;
            }
            Err(_) => {
                eprintln!("writer: batch timed out");
                let _ = pending.failed(g, true);
                *fails += 1;
                return false;
            }
        }
    }
    true
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p scanner --lib writer`
Expected: PASS (the five existing writer tests and the five new ones).

- [ ] **Step 5: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p scanner`
```bash
git add bins/scanner/src/writer.rs
git commit -m "scanner: WriterMsg::PaperReset, a group written alone after the rows before it, never split"
```

---

### Task 8: scanner — start-time epochs, the request poller, the reset in the engine loop

**Files:**
- Modify: `bins/scanner/src/paper.rs:1-24` (doc, imports), append items before `#[cfg(test)]` (line 327), tests module (from 327)
- Modify: `bins/scanner/src/run.rs:626-643` (start), `:855-858` (engine, start epoch, poller), the `select!` loop (after the resolved arm, lines 995-1005), `:1042-1046` (abort), new helpers near `oversize_row` (line 127), tests module
- Test: `bins/scanner/src/paper.rs` and `bins/scanner/src/run.rs` test modules; `bins/scanner/tests/replay.rs` (append)

**Interfaces:**
- Consumes: `Engine::{paper_reset, seed_paper_clock, paper_clock, paper_state}` (Task 2); `Store::{epoch_status, reset_requests_after}`, `PaperEpochRow`, `EpochStatus` (Task 5); `WriterMsg::PaperReset` (Task 7).
- Produces (in `scanner::paper`): `pub const RESET_POLL_EVERY: Duration`; `pub enum StartEpoch { Continue, Policies, Reset { request_id: i64 } }`; `pub fn start_epoch(p: &Policies, saved: &[PaperStateRow], pending: &[i64]) -> StartEpoch`; `pub enum Cause { Policies, Reset }` with `as_str`; `pub struct EpochStart { pub epoch: i32, pub start_ms: i64, pub cause: Cause, pub request_id: Option<i64> }`; `pub fn epoch_batch(run_id: i64, p: &Policies, e: &EpochStart, rows: Vec<SimOut>, st: &TraderState, updated_ms: i64) -> anyhow::Result<Batch>`; `pub struct ResetTracker` with `new(current: i32, last_applied: Option<i64>)`, `accept(&mut self, ids: &[i64]) -> Option<i64>`, `next_epoch(&mut self) -> i32`, `last_applied(&self) -> Option<i64>`, `current(&self) -> i32`; `pub async fn run_reset_poller<F, Fut, E>(fetch: F, last_seen: i64, every: Duration, out: mpsc::Sender<Vec<i64>>)`. In `scanner::run` (private): `fn start_batch(engine: &mut Engine, p: &Policies, run_id: i64, resets: &mut paper::ResetTracker, start: paper::StartEpoch, start_ms: i64) -> anyhow::Result<Option<Batch>>`; `fn reset_batch(engine: &mut Engine, io: &Io, p: &Policies, resets: &mut paper::ResetTracker, request_id: i64, wall_ms: i64) -> anyhow::Result<Option<Batch>>`.

- [ ] **Step 1: Write the failing tests** — append to the `tests` module of `bins/scanner/src/paper.rs`:

```rust
    #[test]
    fn start_epoch_names_a_policies_epoch_a_pending_request_or_nothing() {
        let p = parse_policies(POLICIES).unwrap();
        let rows = saved_rows(&p);
        assert_eq!(start_epoch(&p, &[], &[]), StartEpoch::Policies, "a fresh database");
        assert_eq!(
            start_epoch(&p, &rows, &[]),
            StartEpoch::Continue,
            "a restart; also the start after a policies epoch's first save committed, so a crash \
             loop never writes a second one"
        );
        let changed = parse_policies(&POLICIES.replacen(
            "min_annualized = \"0.10\"",
            "min_annualized = \"0.20\"",
            1,
        ))
        .unwrap();
        assert_eq!(start_epoch(&changed, &rows, &[]), StartEpoch::Policies);
        assert_eq!(
            start_epoch(&changed, &rows, &[3, 5]),
            StartEpoch::Reset { request_id: 5 },
            "new policies and a pending request at once: one reset epoch"
        );
        assert_eq!(
            start_epoch(&p, &rows, &[2]),
            StartEpoch::Reset { request_id: 2 }
        );
    }

    #[test]
    fn the_tracker_applies_each_request_once_and_collapses_a_backlog() {
        let mut r = ResetTracker::new(1, Some(4));
        assert_eq!(r.accept(&[5]), Some(5));
        assert_eq!(r.accept(&[5]), None, "never twice, even if sent again");
        assert_eq!(r.accept(&[]), None, "a tick with no new request does nothing");
        assert_eq!(r.accept(&[3, 4]), None, "older than last_applied");
        assert_eq!(
            r.accept(&[6, 8, 7]),
            Some(8),
            "several collapse into one epoch naming the highest"
        );
        assert_eq!(r.last_applied(), Some(8));
        assert_eq!((r.next_epoch(), r.next_epoch(), r.current()), (2, 3, 3));
        let mut fresh = ResetTracker::new(0, None);
        assert_eq!(fresh.accept(&[1]), Some(1));
    }

    #[test]
    fn an_epoch_batch_carries_the_rows_the_epoch_row_and_the_wall_clock_save() {
        let p = parse_policies(POLICIES).unwrap();
        let st = PaperTrader::new(p.sim.clone()).unwrap().state();
        let mark = SimOut::Account(AccountRow {
            account: "P0-converter/rung-0".into(),
            t_ms: 11,
            cash: d("1000"),
            equity: d("1000"),
            realized: d("0"),
            unrealized: d("0"),
            fees_paid: d("0"),
            locked: d("0"),
            max_drawdown: d("0"),
            n_positions: 0,
        });
        let e = EpochStart {
            epoch: 2,
            start_ms: 11,
            cause: Cause::Reset,
            request_id: Some(9),
        };
        let b = epoch_batch(3, &p, &e, vec![mark], &st, 77).unwrap();
        assert_eq!(
            b.paper_epochs,
            vec![store::PaperEpochRow {
                epoch: 2,
                start_ms: 11,
                cause: "reset".into(),
                request_id: Some(9),
                run_id: 3,
                policies_sha256: p.sha256.clone(),
            }]
        );
        assert_eq!((b.paper_marks.len(), b.paper_marks[0].run_id), (1, 3));
        assert!(
            b.paper_state.len() == st.accounts.len()
                && b.paper_state.iter().all(|s| s.updated_ms == 77)
        );
    }

    #[tokio::test]
    async fn the_poller_forwards_new_ids_and_asks_after_the_last_seen() {
        let asked = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
        let seen = asked.clone();
        let (tx, mut rx) = mpsc::channel(8);
        let task = tokio::spawn(run_reset_poller(
            move |after: i64| {
                seen.lock().unwrap().push(after);
                let found = if after < 6 { vec![5, 6] } else { Vec::new() };
                async move { Ok::<_, String>(found) }
            },
            4, // seeded with the request a start-time epoch already applied
            Duration::from_millis(10),
            tx,
        ));
        let ids = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ids, vec![5, 6]);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let asked = asked.lock().unwrap().clone();
        assert_eq!(
            asked[0], 4,
            "the seed: a request applied at start is never asked for again"
        );
        assert!(asked[1..].iter().all(|&a| a == 6), "{asked:?}");
        assert!(rx.try_recv().is_err(), "nothing new: nothing sent");
        task.abort();
    }

    #[tokio::test]
    async fn a_failed_poll_logs_and_the_next_one_still_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (tx, mut rx) = mpsc::channel(8);
        let task = tokio::spawn(run_reset_poller(
            move |_after: i64| {
                let n = c.fetch_add(1, Ordering::Relaxed);
                async move {
                    if n == 0 {
                        Err("database unreachable".to_string())
                    } else {
                        Ok(vec![3])
                    }
                }
            },
            0,
            Duration::from_millis(10),
            tx,
        ));
        let ids = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ids, vec![3]);
        task.abort();
    }
```
Append to the `tests` module of `bins/scanner/src/run.rs`:
```rust
    fn test_ctx() -> EngineCtx {
        EngineCtx {
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
        }
    }

    #[allow(clippy::unwrap_used)]
    fn repo_paper_engine() -> (Policies, Engine) {
        let p = paper::parse_policies(include_str!("../../../config/policies.toml")).unwrap();
        let mut e = Engine::new(test_ctx());
        e.set_paper(PaperTrader::new(p.sim.clone()).unwrap());
        (p, e)
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_start_time_epoch_seeds_the_clock_and_writes_its_row_with_the_first_save() {
        let (p, mut e) = repo_paper_engine();
        let n = p.sim.accounts.len();
        let mut resets = paper::ResetTracker::new(0, None);
        let b = start_batch(&mut e, &p, 9, &mut resets, paper::StartEpoch::Policies, 50_000)
            .unwrap()
            .unwrap();
        assert_eq!(
            b.paper_epochs,
            vec![store::PaperEpochRow {
                epoch: 1,
                start_ms: 50_000,
                cause: "policies".into(),
                request_id: None,
                run_id: 9,
                policies_sha256: p.sha256.clone(),
            }]
        );
        assert!(b.paper_state.len() == n && b.paper_state.iter().all(|s| s.updated_ms == 50_000));
        assert!(
            b.paper_ledger.is_empty() && b.paper_marks.is_empty(),
            "the previous run's shutdown already closed: no close rows"
        );
        assert_eq!(e.paper_clock(), 50_000, "never the i64::MIN clock (review C3)");

        let (p, mut e) = repo_paper_engine();
        let mut resets = paper::ResetTracker::new(3, Some(4));
        let b = start_batch(
            &mut e,
            &p,
            9,
            &mut resets,
            paper::StartEpoch::Reset { request_id: 7 },
            60_000,
        )
        .unwrap()
        .unwrap();
        let row = &b.paper_epochs[0];
        assert_eq!(
            (row.epoch, row.cause.as_str(), row.request_id),
            (4, "reset", Some(7))
        );
        assert_eq!(resets.last_applied(), Some(7));

        let (p, mut e) = repo_paper_engine();
        assert!(
            start_batch(&mut e, &p, 9, &mut resets, paper::StartEpoch::Continue, 70_000)
                .unwrap()
                .is_none()
        );
        assert_eq!(e.paper_clock(), i64::MIN, "a plain restart seeds nothing");
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    async fn a_reset_is_one_batch_captured_at_the_paper_clock_and_saved_at_the_wall_clock() {
        let (writer, _wrx) = mpsc::channel(16);
        let (capture, mut crx) = mpsc::channel(16);
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
        let (p, mut e) = repo_paper_engine();
        let n = p.sim.accounts.len();
        e.seed_paper_clock(30_000); // the paper clock runs ahead of the wall clock here
        let mut resets = paper::ResetTracker::new(1, Some(4));
        let b = reset_batch(&mut e, &io, &p, &mut resets, 5, 20_000)
            .unwrap()
            .unwrap();
        let Some(CaptureMsg::Line(l)) = crx.recv().await else {
            panic!("expected the reset line first");
        };
        assert_eq!(
            (l.recv_ts_us, l.ctl),
            (30_000_000, Some(Ctl::PaperReset { t: 30_000 }))
        );
        assert_eq!(
            b.paper_epochs,
            vec![store::PaperEpochRow {
                epoch: 2,
                start_ms: 30_001,
                cause: "reset".into(),
                request_id: Some(5),
                run_id: 9,
                policies_sha256: p.sha256.clone(),
            }]
        );
        let at = |t: i64| b.paper_marks.iter().filter(|m| m.t_ms == t).count();
        assert_eq!((at(30_000), at(30_001)), (n, n), "close marks at t, mark_now at t + 1");
        assert!(
            b.paper_state.len() == n && b.paper_state.iter().all(|s| s.updated_ms == 20_000),
            "stamped with the wall clock, never the paper clock (the paper_state upsert guard)"
        );
        assert_eq!(e.paper_clock(), 30_001);
    }
```
Append to `bins/scanner/tests/replay.rs`:
```rust
#[test]
fn replay_rows_never_carry_an_epoch_row() {
    let dir = tmp("paper-reset-epochs");
    write_reset_capture(&dir);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let rows = paper_rows(&replay_paper(&dir, &policies));
    assert!(!rows.paper_marks.is_empty());
    assert!(
        rows.paper_epochs.is_empty(),
        "only the live scanner writes paper_epochs (spec 7a §4.2), so live and replay ledgers agree"
    );
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p scanner --lib paper; cargo test -p scanner --lib run`
Expected: FAIL to compile: `cannot find function 'start_epoch'`, `cannot find type 'ResetTracker'`, `cannot find function 'start_batch'`/`'reset_batch'`.

- [ ] **Step 3: Implement `paper.rs`** — update the module doc list (lines 1-5) with a line `//! - Phase 7a paper epochs: the start-time epoch, the reset-request poller and the epoch batch.`; extend the imports: `use sim::rows::SimOut;` already exists; change the store import to
```rust
use store::{
    Batch, PaperAccountRow, PaperEpochRow, PaperExecRow, PaperFillRow, PaperIntentRow,
    PaperLedgerRow, PaperMarkRow, PaperMarkoutRow, PaperStateRow,
};
```
Insert before `#[cfg(test)]`:
```rust
/// Spec 7a §4.2: how often the poller asks for new reset requests (~30 s to apply one).
pub const RESET_POLL_EVERY: Duration = Duration::from_secs(30);

/// What the scanner does to the paper accounts at start (spec 7a §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartEpoch {
    /// A restart under the same policies: restore the saved accounts, no epoch row.
    Continue,
    /// An empty paper_state (a fresh database) or a policies sha mismatch (ruling 26).
    Policies,
    /// A reset request pending at start (the highest one): a fresh trader (review C3).
    Reset { request_id: i64 },
}

/// A pending request wins (one epoch, cause `reset`, even when the policies changed too); then an
/// empty `paper_state` or any row whose sha isn't the current one is a policies epoch.
pub fn start_epoch(p: &Policies, saved: &[PaperStateRow], pending: &[i64]) -> StartEpoch {
    if let Some(&request_id) = pending.iter().max() {
        return StartEpoch::Reset { request_id };
    }
    if saved.is_empty()
        || saved
            .iter()
            .any(|r| r.policies_sha256.as_deref() != Some(p.sha256.as_str()))
    {
        return StartEpoch::Policies;
    }
    StartEpoch::Continue
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Policies,
    Reset,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Policies => "policies",
            Cause::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochStart {
    pub epoch: i32,
    pub start_ms: i64,
    pub cause: Cause,
    pub request_id: Option<i64>,
}

/// One `WriterMsg::PaperReset` (spec 7a §4.2): the rows the epoch closes and opens with, its
/// `paper_epochs` row and a `paper_state` save stamped `updated_ms` — the WALL clock (the upsert
/// guard `paper_state.updated_ms <= EXCLUDED.updated_ms` would reject a paper-clock stamp behind
/// a later save tick). Never split: a crash can't leave an epoch row next to pre-reset state.
pub fn epoch_batch(
    run_id: i64,
    p: &Policies,
    e: &EpochStart,
    rows: Vec<SimOut>,
    st: &TraderState,
    updated_ms: i64,
) -> anyhow::Result<Batch> {
    let mut b = state_batch(st, updated_ms, &p.sha256)?;
    for o in rows {
        push_sim(run_id, o, &mut b);
    }
    b.paper_epochs.push(PaperEpochRow {
        epoch: e.epoch,
        start_ms: e.start_ms,
        cause: e.cause.as_str().to_string(),
        request_id: e.request_id,
        run_id,
        policies_sha256: p.sha256.clone(),
    });
    Ok(b)
}

/// The engine loop's epoch state, read ONCE at start and then kept in memory: re-reading
/// `paper_epochs` while the writer is backlogged would re-apply a request (spec 7a §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetTracker {
    current: i32,
    last_applied: Option<i64>,
}

impl ResetTracker {
    pub fn new(current: i32, last_applied: Option<i64>) -> Self {
        Self {
            current,
            last_applied,
        }
    }

    /// The request to apply now: the highest id above `last_applied` (several collapse into
    /// one epoch), which becomes `last_applied`. None: nothing new.
    pub fn accept(&mut self, ids: &[i64]) -> Option<i64> {
        let max = ids
            .iter()
            .copied()
            .filter(|&i| self.last_applied.is_none_or(|l| i > l))
            .max()?;
        self.last_applied = Some(max);
        Some(max)
    }

    /// The number of the epoch about to start.
    pub fn next_epoch(&mut self) -> i32 {
        self.current += 1;
        self.current
    }

    pub fn last_applied(&self) -> Option<i64> {
        self.last_applied
    }

    pub fn current(&self) -> i32 {
        self.current
    }
}

/// Spec 7a §4.2: every `every`, ask `fetch` for reset request ids above the last one seen and
/// forward them (one message per poll that found any). A spawned task, like the resolver: the
/// engine loop has no database awaits. A failed poll logs one line and waits for the next.
/// `last_seen` is seeded with the start's `last_applied` (a start-time epoch included), so a
/// request applied at start is never forwarded again. Ends when `out` is gone.
pub async fn run_reset_poller<F, Fut, E>(
    mut fetch: F,
    mut last_seen: i64,
    every: Duration,
    out: mpsc::Sender<Vec<i64>>,
) where
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = Result<Vec<i64>, E>>,
    E: std::fmt::Display,
{
    loop {
        tokio::time::sleep(every).await;
        match fetch(last_seen).await {
            Ok(ids) => {
                if let Some(&max) = ids.iter().max() {
                    last_seen = last_seen.max(max);
                    if out.send(ids).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => eprintln!("paper: reset poll failed: {e}"),
        }
    }
}
```

- [ ] **Step 4: Implement `run.rs`** — add after `oversize_row` (line 140):

```rust
/// Spec 7a §4.2: a start-time epoch (a request pending at start, new policies, or an empty
/// paper_state). The trader is already fresh; the paper clock starts at `start_ms` (never
/// i64::MIN, review C3) and the epoch row goes with an immediate `paper_state` save as one batch.
/// No close rows: the previous run's shutdown closed. None for a plain restart.
fn start_batch(
    engine: &mut Engine,
    p: &Policies,
    run_id: i64,
    resets: &mut paper::ResetTracker,
    start: paper::StartEpoch,
    start_ms: i64,
) -> anyhow::Result<Option<Batch>> {
    let (cause, request_id) = match start {
        paper::StartEpoch::Continue => return Ok(None),
        paper::StartEpoch::Policies => (paper::Cause::Policies, None),
        paper::StartEpoch::Reset { request_id } => {
            resets.accept(&[request_id]);
            (paper::Cause::Reset, Some(request_id))
        }
    };
    engine.seed_paper_clock(start_ms);
    let Some(st) = engine.paper_state() else {
        return Ok(None);
    };
    let epoch = resets.next_epoch();
    match request_id {
        Some(r) => eprintln!("paper: reset → epoch {epoch} (request {r}, pending at start)"),
        None => eprintln!("paper: policies → epoch {epoch}"),
    }
    let e = paper::EpochStart {
        epoch,
        start_ms,
        cause,
        request_id,
    };
    paper::epoch_batch(run_id, p, &e, Vec::new(), &st, start_ms).map(Some)
}

/// Spec 7a §4.2: apply reset request `request_id` now. `t` = the paper clock (never behind the
/// wall clock, so never i64::MIN) is captured first as `Ctl::PaperReset { t }`, so replay resets
/// at the same instant; the close rows, the epoch row, the `mark_now` rows and a `paper_state`
/// save stamped `wall_ms` come back as ONE batch for `WriterMsg::PaperReset`.
fn reset_batch(
    engine: &mut Engine,
    io: &Io,
    p: &Policies,
    resets: &mut paper::ResetTracker,
    request_id: i64,
    wall_ms: i64,
) -> anyhow::Result<Option<Batch>> {
    let t = engine.paper_clock().max(wall_ms);
    io.ctl(0, Ctl::PaperReset { t }, t);
    let Some((start_ms, rows)) = engine.paper_reset(t) else {
        return Ok(None);
    };
    let Some(st) = engine.paper_state() else {
        return Ok(None);
    };
    let epoch = resets.next_epoch();
    let e = paper::EpochStart {
        epoch,
        start_ms,
        cause: paper::Cause::Reset,
        request_id: Some(request_id),
    };
    let b = paper::epoch_batch(io.run_id, p, &e, rows, &st, wall_ms)?;
    eprintln!("paper: reset → epoch {epoch} (request {request_id})");
    Ok(Some(b))
}
```
Start (lines 626-643), replace the `let trader = match policies { … };` block with:
```rust
    // Paper trader (S3c): restore the live accounts from paper_state, register this run's
    // accounts (pre-registration, spec §11) before any other paper row, start the resolver.
    // Replay never touches paper_state. Phase 7a §4.2: the epoch status is read ONCE here; the
    // loop then keeps it in memory.
    let (trader, start, status) = match policies {
        Some(p) => {
            let saved = store.load_paper_state().await?;
            let status = store.epoch_status().await?;
            let pending: Vec<i64> = store
                .reset_requests_after(status.last_applied.unwrap_or(0))
                .await?
                .into_iter()
                .map(|r| r.id)
                .collect();
            let start = paper::start_epoch(p, &saved, &pending);
            let t = match start {
                // a request pending at start: a fresh trader, never the saved accounts (C3)
                paper::StartEpoch::Reset { .. } => {
                    PaperTrader::new(p.sim.clone()).context("paper trader")?
                }
                // a policies epoch is fresh through restore_trader (and keeps its log line)
                _ => paper::restore_trader(p, &saved)?,
            };
            let accounts = Batch {
                paper_accounts: paper::account_rows(run_id, p),
                ..Batch::default()
            };
            writer_tx
                .send(WriterMsg::Paper(Box::new(accounts)))
                .await
                .context("writer gone")?;
            (Some(t), start, status)
        }
        None => (
            None,
            paper::StartEpoch::Continue,
            store::EpochStatus {
                current: 0,
                last_applied: None,
            },
        ),
    };
```
Engine (lines 855-858), replace
```rust
    let mut engine = Engine::new(engine_ctx(cfg, cfg_hash, run_id, "live"));
    if let Some(t) = trader {
        engine.set_paper(t);
    }
```
with
```rust
    let mut engine = Engine::new(engine_ctx(cfg, cfg_hash, run_id, "live"));
    if let Some(t) = trader {
        engine.set_paper(t);
    }
    let mut resets = paper::ResetTracker::new(status.current, status.last_applied);
    if let Some(p) = policies
        && let Some(b) = start_batch(&mut engine, p, run_id, &mut resets, start, now_ms())?
    {
        writer_tx
            .send(WriterMsg::PaperReset(Box::new(b)))
            .await
            .context("writer gone")?;
    }
    // Phase 7a §4.2: reset requests, polled off the engine loop.
    let (reset_tx, mut reset_rx) = mpsc::channel::<Vec<i64>>(16);
    let reset_task = if engine.paper().is_some() {
        let s = store.clone();
        Some(tokio::spawn(paper::run_reset_poller(
            move |after: i64| {
                let s = s.clone();
                async move {
                    s.reset_requests_after(after)
                        .await
                        .map(|v| v.into_iter().map(|r| r.id).collect::<Vec<i64>>())
                }
            },
            resets.last_applied().unwrap_or(0),
            paper::RESET_POLL_EVERY,
            reset_tx,
        )))
    } else {
        drop(reset_tx); // reset_rx then yields None, which disables its select arm
        None
    };
```
In the `select!` loop, after the `Some(r) = resolved_rx.recv() => { … }` arm (ends line 1005), add:
```rust
            Some(ids) = reset_rx.recv() => {
                // Phase 7a §4.2: new requests; several collapse into one epoch (the highest id)
                if let (Some(p), Some(request_id)) = (policies, resets.accept(&ids)) {
                    match reset_batch(&mut engine, &io, p, &mut resets, request_id, now_ms()) {
                        Ok(Some(b)) => {
                            if writer_tx.send(WriterMsg::PaperReset(Box::new(b))).await.is_err() {
                                break Err(anyhow::anyhow!("writer gone"));
                            }
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("paper: reset not written: {e:#}"),
                    }
                }
            }
```
Shutdown (lines 1042-1046), after the resolver abort add:
```rust
    // …and no reset either
    if let Some(r) = &reset_task {
        r.abort();
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p scanner`
Expected: PASS.

- [ ] **Step 6: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
```bash
git add bins/scanner/src/paper.rs bins/scanner/src/run.rs bins/scanner/tests/replay.rs
git commit -m "scanner: start-time epochs, the reset-request poller and the reset as one PaperReset batch"
```

---

### Task 9: cards — `LADDER`, `break_even`, the shared scenario parse and measured latency

**Files:**
- Modify: `crates/cards/src/edge.rs:11-15` (constants next to `HEADLINE`)
- Create: `crates/cards/src/ladder.rs`, `crates/cards/src/scenarios.rs`
- Modify: `crates/cards/src/lib.rs:6-14`, `crates/cards/Cargo.toml` (dependencies)
- Modify: `bins/analyze/src/scenarios.rs:6-18` (use the shared parse), `bins/analyze/src/tick.rs:574-581` (use `minute_latency`)
- Test: create `crates/cards/tests/ladder.rs`, `crates/cards/tests/scenarios.rs`

**Interfaces:**
- Consumes: `cards::stats::{CountMap, total, nearest_rank}`, `cards::sweet::MIN_EXEC` (30), `cards::body::ScenarioLat`.
- Produces: `pub const LADDER: [&str; 6]`, `pub const EXTRAS: [&str; 2]` in `cards::edge`; `cards::ladder::{Rung { latency_ms: Option<i64>, extra: bool, n_exec: i64, net: Decimal }, Direction { LosesBeyond, PaysBeyond }, BreakEven { Thin, PaysEverywhere, PaysNowhere, At { latency_ms: i64, direction: Direction, not_monotone: bool } }, break_even(rungs: &[Rung]) -> BreakEven, minute_latency(lag_p50_ms: Option<i32>, rest_rtt_p50_ms: Option<i32>) -> Option<i64>, measured(minutes: impl Iterator<Item = (Option<i32>, Option<i32>)>) -> Option<i64>}`; `cards::scenarios::parse(text: &str) -> Result<Vec<ScenarioLat>, String>`. `BreakEven` serializes as `{"kind":"thin"}`, `{"kind":"all"}`, `{"kind":"none"}`, `{"kind":"at","latency_ms":N,"direction":"loses_beyond"|"pays_beyond"[,"not_monotone":true]}`.

- [ ] **Step 1: Write the failing tests** — create `crates/cards/tests/ladder.rs`:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::ladder::{BreakEven, Direction, Rung, break_even, measured};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn rung(latency_ms: i64, net: Decimal) -> Rung {
    Rung {
        latency_ms: Some(latency_ms),
        extra: false,
        n_exec: 30,
        net,
    }
}

#[test]
fn too_few_trades_on_any_rung_is_thin() {
    let mut r = vec![rung(0, dec!(1)), rung(25, dec!(-1))];
    r[1].n_exec = 29;
    assert_eq!(break_even(&r), BreakEven::Thin);
    assert_eq!(break_even(&[]), BreakEven::Thin);
}

#[test]
fn every_rung_paying_is_all_and_every_rung_losing_is_none() {
    assert_eq!(
        break_even(&[rung(0, dec!(0)), rung(25, dec!(3))]),
        BreakEven::PaysEverywhere
    );
    assert_eq!(
        break_even(&[rung(0, dec!(-0.01)), rung(25, dec!(-3))]),
        BreakEven::PaysNowhere
    );
}

#[test]
fn the_first_sign_change_is_interpolated_and_rounded_to_5_ms() {
    // 25 + 27 × 5 / (5 + 5) = 38.5 → 40
    assert_eq!(
        break_even(&[rung(0, dec!(10)), rung(25, dec!(5)), rung(52, dec!(-5))]),
        BreakEven::At {
            latency_ms: 40,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }
    );
    // the faster rungs lose: 0 + 100 × 4 / 8 = 50
    assert_eq!(
        break_even(&[rung(100, dec!(4)), rung(0, dec!(-4))]),
        BreakEven::At {
            latency_ms: 50,
            direction: Direction::PaysBeyond,
            not_monotone: false
        },
        "rungs are sorted by latency first"
    );
    // 52 + 48 / 101 = 52.475 → rounds to 50, clamped into [52, 100]
    assert_eq!(
        break_even(&[rung(52, dec!(1)), rung(100, dec!(-100))]),
        BreakEven::At {
            latency_ms: 52,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }
    );
}

#[test]
fn more_than_one_sign_change_is_flagged_not_monotone() {
    // first change between 0 and 25 at 12.5 → 15 (half away from zero)
    assert_eq!(
        break_even(&[rung(0, dec!(1)), rung(25, dec!(-1)), rung(52, dec!(1))]),
        BreakEven::At {
            latency_ms: 15,
            direction: Direction::LosesBeyond,
            not_monotone: true
        }
    );
}

#[test]
fn extras_never_enter_the_computation() {
    let extra = Rung {
        latency_ms: Some(450),
        extra: true,
        n_exec: 0,
        net: dec!(-100),
    };
    assert_eq!(
        break_even(&[rung(0, dec!(1)), rung(25, dec!(2)), extra]),
        BreakEven::PaysEverywhere
    );
}

#[test]
fn the_json_shape_matches_the_spec() {
    let j = |b: BreakEven| serde_json::to_string(&b).unwrap();
    assert_eq!(j(BreakEven::Thin), r#"{"kind":"thin"}"#);
    assert_eq!(j(BreakEven::PaysEverywhere), r#"{"kind":"all"}"#);
    assert_eq!(j(BreakEven::PaysNowhere), r#"{"kind":"none"}"#);
    assert_eq!(
        j(BreakEven::At {
            latency_ms: 40,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }),
        r#"{"kind":"at","latency_ms":40,"direction":"loses_beyond"}"#
    );
    assert_eq!(
        j(BreakEven::At {
            latency_ms: 15,
            direction: Direction::PaysBeyond,
            not_monotone: true
        }),
        r#"{"kind":"at","latency_ms":15,"direction":"pays_beyond","not_monotone":true}"#
    );
}

#[test]
fn measured_is_the_nearest_rank_median_of_lag_plus_half_the_rtt() {
    let minutes = [
        (Some(78), Some(150)), // 153
        (Some(80), None),      // not measured
        (None, Some(10)),      // not measured
        (Some(70), Some(20)),  // 80
    ];
    assert_eq!(measured(minutes.into_iter()), Some(80));
    assert_eq!(measured(std::iter::empty()), None);
}

const LAT: [i64; 6] = [0, 25, 52, 100, 200, 300];

proptest! {
    #[test]
    fn every_sign_pattern_has_a_defined_answer_between_the_rungs_it_names(
        nets in proptest::collection::vec(-100i64..100, 1..=6),
    ) {
        let rungs: Vec<Rung> = nets.iter().zip(LAT).map(|(&n, l)| rung(l, Decimal::from(n))).collect();
        let pays: Vec<bool> = nets.iter().map(|&n| n >= 0).collect();
        match break_even(&rungs) {
            BreakEven::Thin => prop_assert!(false, "30 execs each: never thin"),
            BreakEven::PaysEverywhere => prop_assert!(pays.iter().all(|&p| p)),
            BreakEven::PaysNowhere => prop_assert!(pays.iter().all(|&p| !p)),
            BreakEven::At { latency_ms, direction, not_monotone } => {
                let i = pays.windows(2).position(|w| w[0] != w[1]).unwrap();
                prop_assert!(LAT[i] <= latency_ms && latency_ms <= LAT[i + 1]);
                prop_assert_eq!(direction == Direction::LosesBeyond, pays[i]);
                let changes = pays.windows(2).filter(|w| w[0] != w[1]).count();
                prop_assert_eq!(not_monotone, changes > 1);
            }
        }
    }
}
```
Create `crates/cards/tests/scenarios.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
fn the_repo_policies_give_every_scenarios_latency() {
    let s = cards::scenarios::parse(include_str!("../../../config/policies.toml")).unwrap();
    let got: Vec<(&str, i64)> = s.iter().map(|x| (x.name.as_str(), x.latency_ms)).collect();
    assert_eq!(
        got,
        vec![
            ("colo-idle", 52),
            ("sp-idle", 300),
            ("sp-burst", 450),
            ("sp-idle-nodelay", 300),
            ("rung-0", 0),
            ("rung-25", 25),
            ("rung-100", 100),
            ("rung-200", 200)
        ]
    );
    for name in cards::edge::LADDER.iter().chain(cards::edge::EXTRAS.iter()) {
        assert!(s.iter().any(|x| x.name == *name), "{name} is in policies.toml");
    }
}

#[test]
fn a_file_that_is_not_toml_is_an_error() {
    assert!(cards::scenarios::parse("[[scenarios]\nname = ").is_err());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p cards --test ladder --test scenarios`
Expected: FAIL to compile: `unresolved import cards::ladder`, `could not find 'scenarios' in 'cards'`, `cannot find value 'LADDER'`.

- [ ] **Step 3: Implement** — `crates/cards/src/edge.rs`, after `pub const HEADLINE: &str = "sp-idle";` (line 12):

```rust
/// Phase 7a (spec 2026-09-29 §2): the latency ladder, fastest first; each is a P0-converter
/// account at fee 0. `ladder::break_even` interpolates between these only.
pub const LADDER: [&str; 6] = [
    "rung-0",
    "rung-25",
    "colo-idle",
    "rung-100",
    "rung-200",
    "sp-idle",
];
/// Drawn beside the ladder, never interpolated: variable latency (sp-burst) and a sensitivity
/// check that ignores secondsDelay (sp-idle-nodelay).
pub const EXTRAS: [&str; 2] = ["sp-burst", "sp-idle-nodelay"];
```
Create `crates/cards/src/ladder.rs`:
```rust
//! Phase 7a (spec 2026-09-29 §2, §7.1): the latency ladder's pure parts. `break_even` answers
//! "at which latency would this pay?" from each rung's net since the epoch start; `measured` is
//! Edge capture's latency estimate (the same per-minute figure and nearest-rank median).
use crate::stats::{self, CountMap};
use crate::sweet::MIN_EXEC;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;

/// The break-even latency is rounded to this many ms.
pub const ROUND_MS: i64 = 5;

/// One paper account's figures since the epoch start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rung {
    /// proc + net + match from policies.toml; None: unknown (never placed).
    pub latency_ms: Option<i64>,
    /// sp-burst / sp-idle-nodelay: shown, never computed with.
    pub extra: bool,
    pub n_exec: i64,
    /// Latest mark equity − bankroll.
    pub net: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// ≥ 0 then < 0: the expected shape.
    LosesBeyond,
    /// < 0 then ≥ 0: the faster rungs lose.
    PaysBeyond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BreakEven {
    /// Some ladder rung has fewer than `MIN_EXEC` executed attempts (SWEET's minimum).
    Thin,
    #[serde(rename = "all")]
    PaysEverywhere,
    #[serde(rename = "none")]
    PaysNowhere,
    At {
        latency_ms: i64,
        direction: Direction,
        #[serde(skip_serializing_if = "is_false")]
        not_monotone: bool,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Spec 7a §7.1, defined for every sign pattern: `thin` if any ladder rung has n_exec < 30;
/// otherwise `all` (every net ≥ 0), `none` (every net < 0), or `at` the FIRST sign change between
/// adjacent rungs by latency, linearly interpolated, rounded to 5 ms and clamped between those two
/// rungs, with `not_monotone` when the sign changes more than once. Extras and rungs with an
/// unknown latency never enter. One epoch is one path: there is no interval.
pub fn break_even(rungs: &[Rung]) -> BreakEven {
    let mut ladder: Vec<(i64, i64, Decimal)> = rungs
        .iter()
        .filter(|r| !r.extra)
        .filter_map(|r| r.latency_ms.map(|l| (l, r.n_exec, r.net)))
        .collect();
    if ladder.is_empty() || ladder.iter().any(|&(_, n, _)| n < MIN_EXEC) {
        return BreakEven::Thin;
    }
    ladder.sort_by_key(|&(l, _, _)| l);
    let pays = |x: Decimal| x >= Decimal::ZERO;
    if ladder.iter().all(|&(_, _, x)| pays(x)) {
        return BreakEven::PaysEverywhere;
    }
    if ladder.iter().all(|&(_, _, x)| !pays(x)) {
        return BreakEven::PaysNowhere;
    }
    let changes: Vec<usize> = ladder
        .windows(2)
        .enumerate()
        .filter(|(_, w)| pays(w[0].2) != pays(w[1].2))
        .map(|(i, _)| i)
        .collect();
    let Some(&i) = changes.first() else {
        return BreakEven::PaysNowhere; // unreachable: mixed signs change somewhere
    };
    let ((a, _, x), (b, _, y)) = (ladder[i], ladder[i + 1]);
    // x and y have opposite signs, so x − y ≠ 0 and x / (x − y) is in [0, 1]
    let frac = x.checked_div(x - y).unwrap_or(Decimal::ZERO);
    let v = Decimal::from(a) + Decimal::from(b - a) * frac;
    let step = Decimal::from(ROUND_MS);
    let rounded = (v / step).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero) * step;
    let latency_ms = rounded.to_i64().unwrap_or(a).clamp(a, b);
    BreakEven::At {
        latency_ms,
        direction: if pays(x) {
            Direction::LosesBeyond
        } else {
            Direction::PaysBeyond
        },
        not_monotone: changes.len() > 1,
    }
}

/// Edge capture's measured latency of one health minute: feed lag p50 + REST round trip p50 / 2,
/// when both were measured (Phase 5a spec §6.2). Shared by analyze and the api.
pub fn minute_latency(lag_p50_ms: Option<i32>, rest_rtt_p50_ms: Option<i32>) -> Option<i64> {
    Some(i64::from(lag_p50_ms?) + i64::from(rest_rtt_p50_ms?) / 2)
}

/// The nearest-rank median of `minute_latency` over the minutes that have one.
pub fn measured(minutes: impl Iterator<Item = (Option<i32>, Option<i32>)>) -> Option<i64> {
    let mut m = CountMap::new();
    for (lag, rtt) in minutes {
        if let Some(v) = minute_latency(lag, rtt) {
            *m.entry(v).or_insert(0) += 1;
        }
    }
    let n = stats::total(&m);
    stats::nearest_rank(m.iter().map(|(v, k)| (*v, *k)), n, 50)
}
```
Create `crates/cards/src/scenarios.rs`:
```rust
//! The latency scenarios of policies.toml (the same frozen file the scanner's paper trader runs),
//! shared by analyze (Edge capture's decay chart) and the api (the ladder, spec 7a §7.1): x =
//! proc + net + match. Reads only `[[scenarios]]`; analyze also validates the whole file with sim.
use crate::body::ScenarioLat;
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    scenarios: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    name: String,
    proc_ms: i64,
    net_ms: i64,
    match_ms: i64,
}

pub fn parse(text: &str) -> Result<Vec<ScenarioLat>, String> {
    let f: File = toml::from_str(text).map_err(|e| format!("parse policies: {e}"))?;
    Ok(f.scenarios
        .into_iter()
        .map(|s| ScenarioLat {
            latency_ms: s.proc_ms + s.net_ms + s.match_ms,
            net_ms: s.net_ms,
            name: s.name,
        })
        .collect())
}
```
`crates/cards/src/lib.rs`: add `pub mod ladder;` after `pub mod gebele;` and `pub mod scenarios;` after `pub mod papers;`. `crates/cards/Cargo.toml` `[dependencies]`: add `toml.workspace = true` after `sha2.workspace = true`; `[dev-dependencies]`: add `serde_json.workspace = true` only if absent (it is a normal dependency already, so no change).

- [ ] **Step 4: analyze uses the shared pieces** — `bins/analyze/src/scenarios.rs`, replace `parse` (lines 6-18) with:

```rust
pub fn parse(text: &str) -> anyhow::Result<Vec<ScenarioLat>> {
    let sim: sim::config::SimConfig = toml::from_str(text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    cards::scenarios::parse(text).map_err(anyhow::Error::msg)
}
```
`bins/analyze/src/tick.rs:576-581`, replace
```rust
        if let (Some(l), Some(r)) = (m.lag_p50_ms, m.rest_rtt_p50_ms) {
            lat.entry(hour_of(m.minute_ms))
                .or_default()
                .push(i64::from(l) + i64::from(r) / 2);
        }
```
with
```rust
        if let Some(v) = cards::ladder::minute_latency(m.lag_p50_ms, m.rest_rtt_p50_ms) {
            lat.entry(hour_of(m.minute_ms)).or_default().push(v);
        }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p cards && cargo test -p analyze --lib`
Expected: PASS (`no_floats` included).
Run (PG prefix): `cargo test -p analyze`
Expected: PASS (Edge capture's measured latency unchanged).

- [ ] **Step 6: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo deny check bans advisories`
Expected: clean; deny `bans ok, advisories ok`.
```bash
git add crates/cards/src/edge.rs crates/cards/src/ladder.rs crates/cards/src/scenarios.rs crates/cards/src/lib.rs crates/cards/Cargo.toml crates/cards/tests/ladder.rs crates/cards/tests/scenarios.rs bins/analyze/src/scenarios.rs bins/analyze/src/tick.rs Cargo.lock
git commit -m "cards: LADDER and EXTRAS, break_even for every sign pattern, the shared scenario parse and measured latency"
```

---

### Task 10: analyze — an epoch's first mark counts from the bankroll

**Files:**
- Modify: `bins/analyze/src/tick.rs:561-633` (`edge_inputs` doc and loop)
- Test: `bins/analyze/tests/tick.rs` (append; reuses `run`, `premises`, `ep`, `write`, `mark` at 677, `p0` at 935, `paper` at 948, `rw`, `body`, `cell`, `CAPTURED`)

**Interfaces:**
- Consumes: `Store::paper_epoch_starts` (Task 5); `PaperEpochRow` in `Batch` (Task 5).
- Produces: Edge capture deltas where an account's first mark at or after an epoch start is `fresh()` (equity − bankroll).

- [ ] **Step 1: Write the failing test** — append to `bins/analyze/tests/tick.rs`:

```rust
async fn epoch(owner: &Store, run_id: i64, epoch: i32, start_ms: i64) {
    owner
        .write_batch(&Batch {
            paper_epochs: vec![PaperEpochRow {
                epoch,
                start_ms,
                cause: "reset".into(),
                request_id: None,
                run_id,
                policies_sha256: "x".into(),
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn the_first_mark_of_a_new_epoch_counts_from_the_bankroll() {
    for with_mark_now in [true, false] {
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
        let mut marks = vec![
            mark(r, T0 - H + 10 * M, dec!(1010)),
            mark(r, T0 - H + 20 * M, dec!(1003)),
        ];
        if with_mark_now {
            marks.push(mark(r, T0 - H + 15 * M, dec!(1000))); // the reset's mark_now row
        }
        paper(&owner, vec![p0(r, dec!(1000))], marks).await;
        epoch(&owner, r, 1, T0 - H + 15 * M).await;
        let rw = rw(&db).await;
        Analyzer::new(rw.clone(), Duration::ZERO, "test", scenarios())
            .await
            .unwrap()
            .tick(T0 + 30 * M)
            .await
            .unwrap();
        assert_eq!(
            cell(&body(&rw, "24h", "EDGE").await, CAPTURED, "ours"),
            "13",
            "with_mark_now={with_mark_now}: +10 vs the bankroll, then +3 vs the new epoch's \
             bankroll (a delta from the old epoch's mark would give 3)"
        );
        rw.close().await;
        db.drop(owner).await;
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run (PG prefix): `cargo test -p analyze --test tick the_first_mark_of_a_new_epoch -- --nocapture`
Expected: FAIL: `left: "3", right: "13"`.

- [ ] **Step 3: Implement** — `bins/analyze/src/tick.rs`. Append to `edge_inputs`' doc comment (after line 567):
```rust
/// Spec 7a §6: an account's first mark at or after a paper epoch's start also counts from the
/// bankroll (the reset started every account there); with the reset's `mark_now` row that delta
/// is 0, without it the real P&L after the reset still counts. Epochs are read AFTER the marks,
/// in the same tick, so a reset committed between the two reads is never half-seen.
```
After `let marks = env.store.marks_with_prev(&ids, from, to).await?;` (line 588) add:
```rust
    let epoch_starts = env.store.paper_epoch_starts().await?;
```
Inside the `for m in &marks` loop, after `let fresh = || …;` (line 610) add:
```rust
        // a paper epoch started in (p, m]: every account restarted there
        let new_epoch = |p: &MarkPoint| epoch_starts.iter().any(|&s| p.t_ms < s && s <= m.t_ms);
```
and make the first arm of `let delta = match p {` (line 611):
```rust
            Some(p) if new_epoch(p) => fresh(),
```
(before the existing `Some(p) if fam.runs.contains_key(&p.run_id) => …` arm).

- [ ] **Step 4: Run the tests**

Run (PG prefix): `cargo test -p analyze`
Expected: PASS (the existing Edge capture tests unchanged: no epoch rows there).

- [ ] **Step 5: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
```bash
git add bins/analyze/src/tick.rs bins/analyze/tests/tick.rs
git commit -m "analyze: an epoch's first mark counts from the bankroll in Edge capture"
```

---

### Task 11: api — `POST /api/paper/reset`, `GET /api/paper/epochs`, the second pool and its start-up refusal

**Files:**
- Modify: `bins/api/src/error.rs:12-56` (three variants), `bins/api/src/lib.rs:1-2, 5-15, 18-19, 41-62, 72-79, 87-107, 109-154`, `bins/api/src/main.rs:14-29`, `bins/api/src/config.rs:5-66` (+ test)
- Create: `bins/api/src/reset.rs`
- Modify: `bins/api/tests/common/mod.rs` (helpers), `compose.web.yml:21-23`, `.env.example` (end)
- Test: create `bins/api/tests/reset.rs`

**Interfaces:**
- Consumes: `Store::{connect_reset, role_writes_outside_reset, paper_epochs, pending_reset_request, latest_reset_request, insert_reset_request}`, `ResetInsert` (Task 5).
- Produces: `ApiError::{Forbidden(&'static str, String), Conflict(&'static str, String), TooMany(&'static str, String)}` (403/409/429); `AppState::reset: Option<Store>`, `AppState::with_reset(self, reset: Option<Store>) -> AppState`; `api::ensure_reset_role(reset: &Store) -> anyhow::Result<()>`; `api::run(state: AppState, bind: &str, health_every: Duration, shutdown: impl Future<Output = ()> + Send + 'static)`; `api::run_on_state(listener: TcpListener, state: AppState, health_every: Duration, shutdown: …)`; `Config::reset_database_url: Option<String>`; `api::reset::{reset, epochs, next_midnight(ms: i64) -> i64, next_allowed(latest_requested_ms: Option<i64>, now: i64) -> i64, utc_date(ms: i64) -> String, INTENT_HEADER, INTENT}`; routes `POST /api/paper/reset` → 202 `{request_id, requested_ms}` | 400 `bad_request` | 403 `forbidden` | 409 `reset_pending` | 429 `reset_daily_limit` | 503 `reset_disabled`; `GET /api/paper/epochs` → `{epochs: [{epoch, start_ms, cause}], current, pending_request: {id, requested_ms} | null, next_allowed_ms}`.

- [ ] **Step 1: Write the failing tests** — `bins/api/tests/common/mod.rs`, append:

```rust
/// The api with its reset pool as deployed (paper_reset_rw, one connection).
pub async fn app_with_reset(db: &TestDb) -> (Router, AppState) {
    let ro = Store::connect_read_only(db.opts.clone(), 4, 5_000, Some("analyze_ro"))
        .await
        .expect("read-only pool");
    let reset = Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .expect("reset pool");
    let st = AppState::new(ro).with_reset(Some(reset));
    (api::router(st.clone()), st)
}

pub async fn post(
    app: &Router,
    uri: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::post(uri).header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
```
Create `bins/api/tests/reset.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use store::*;

const URI: &str = "/api/paper/reset";
const OK_BODY: &str = r#"{"confirm": "reset"}"#;
const INTENT: (&str, &str) = ("X-CT-Intent", "reset-paper");
const SAME: (&str, &str) = ("Sec-Fetch-Site", "same-origin");

async fn apply(owner: &Store, request_id: i64) {
    let run = common::run(owner, "live").await;
    owner
        .raw_exec(&format!(
            "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
             VALUES ((SELECT COALESCE(max(epoch), 0) + 1 FROM paper_epochs), {}, 'reset', {request_id}, {run}, 'sha')",
            common::now()
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn reset_needs_the_intent_header_and_a_same_origin_fetch() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    for headers in [
        vec![],
        vec![("X-CT-Intent", "yes")],
        vec![INTENT, ("Sec-Fetch-Site", "cross-site")],
        vec![INTENT, ("Sec-Fetch-Site", "same-site")],
        vec![INTENT, ("Sec-Fetch-Site", "none")],
    ] {
        let (status, v) = common::post(&app, URI, &headers, OK_BODY).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}: {v}");
        assert_eq!(v["error"], "forbidden");
    }
    assert!(owner.reset_requests_after(0).await.unwrap().is_empty());
    db.drop(owner).await;
}

#[tokio::test]
async fn a_bad_body_is_a_400() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    for body in [
        "",
        "{}",
        "not json",
        r#"{"confirm": "yes"}"#,
        r#"{"confirm": "reset", "also": 1}"#,
    ] {
        let (status, v) = common::post(&app, URI, &[INTENT, SAME], body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {v}");
        assert_eq!(v["error"], "bad_request");
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn one_reset_is_queued_then_pending_then_the_daily_limit() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v}");
    let id = v["request_id"].as_i64().unwrap();
    assert!(v["requested_ms"].as_i64().unwrap() > 0);
    // no Sec-Fetch-Site at all is fine (a non-browser client still needs the custom header)
    let (status, v) = common::post(&app, URI, &[INTENT], OK_BODY).await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::CONFLICT, Some("reset_pending"))
    );
    apply(&owner, id).await; // the scanner applied it
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert_eq!(v["error"], "reset_daily_limit");
    let tomorrow = api::reset::utc_date(api::reset::next_midnight(common::now()));
    assert!(
        v["detail"].as_str().unwrap().contains(&tomorrow),
        "{v} names {tomorrow}"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn without_a_reset_pool_the_route_is_503() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    assert_eq!(
        (status, v["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("reset_disabled"))
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn the_api_refuses_an_over_granted_reset_role() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .unwrap();
    api::ensure_reset_role(&reset).await.expect("as migrated");
    let as_owner = Store::connect_reset(db.opts.clone(), 5_000, None)
        .await
        .unwrap();
    assert!(api::ensure_reset_role(&as_owner).await.is_err());
    owner
        .raw_exec("GRANT INSERT ON episodes TO paper_reset_rw")
        .await
        .unwrap();
    let e = api::ensure_reset_role(&reset).await.unwrap_err();
    assert!(e.to_string().contains("episodes"), "{e}");
    reset.close().await;
    as_owner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn epochs_list_current_pending_and_next_allowed() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_reset(&db).await;
    let before = common::now();
    let (status, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["epochs"], serde_json::json!([]));
    assert_eq!(v["current"], 0);
    assert!(v["pending_request"].is_null());
    assert!(v["next_allowed_ms"].as_i64().unwrap() >= before, "now: allowed");
    let (_, q) = common::post(&app, URI, &[INTENT, SAME], OK_BODY).await;
    let id = q["request_id"].as_i64().unwrap();
    let midnight = api::reset::next_midnight(q["requested_ms"].as_i64().unwrap());
    let (_, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(v["pending_request"]["id"], id);
    assert_eq!(v["next_allowed_ms"], midnight);
    apply(&owner, id).await;
    let (_, v) = common::get(&app, "/api/paper/epochs").await;
    assert_eq!(v["current"], 1);
    assert_eq!(v["epochs"][0]["cause"], "reset");
    assert!(v["epochs"][0]["start_ms"].is_i64());
    assert!(v["pending_request"].is_null());
    assert_eq!(v["next_allowed_ms"], midnight);
    db.drop(owner).await;
}
```
Append to the `tests` module of `bins/api/src/config.rs`:
```rust
    #[test]
    fn the_reset_url_is_optional_and_redacted() {
        let url = (
            "DATABASE_URL",
            "postgres://analyze_ro:pw@postgres/cute_trading",
        );
        assert_eq!(cfg(&[url]).unwrap().reset_database_url, None);
        assert_eq!(
            cfg(&[url, ("RESET_DATABASE_URL", "")])
                .unwrap()
                .reset_database_url,
            None
        );
        let c = cfg(&[
            url,
            (
                "RESET_DATABASE_URL",
                "postgres://paper_reset_rw:hunter3@postgres/cute_trading",
            ),
        ])
        .unwrap();
        assert!(c.reset_database_url.is_some());
        assert!(!format!("{c:?}").contains("hunter3"));
    }
```

- [ ] **Step 2: Run them to see them fail**

Run (PG prefix): `cargo test -p api --test reset; cargo test -p api --lib config`
Expected: FAIL to compile: `no method named 'with_reset'`, `could not find 'reset' in 'api'`, `no field 'reset_database_url'`.

- [ ] **Step 3: Errors** — `bins/api/src/error.rs`, in `ApiError` after `BadRequest(…)`:

```rust
    /// 403: (code, detail)
    Forbidden(&'static str, String),
    /// 409: (code, detail)
    Conflict(&'static str, String),
    /// 429: (code, detail)
    TooMany(&'static str, String),
```
In `into_response`'s match, after the `BadRequest` arm:
```rust
            ApiError::Forbidden(code, detail) => (StatusCode::FORBIDDEN, code, detail),
            ApiError::Conflict(code, detail) => (StatusCode::CONFLICT, code, detail),
            ApiError::TooMany(code, detail) => (StatusCode::TOO_MANY_REQUESTS, code, detail),
```

- [ ] **Step 4: Config** — `bins/api/src/config.rs`: in `Config` after `pub statement_timeout_ms: u32,`:

```rust
    /// Phase 7a §5.1: the reset pool's URL (paper_reset_rw); None when unset or empty (POST
    /// /api/paper/reset then answers 503).
    pub reset_database_url: Option<String>,
```
In `Debug`, before `.finish()`: `.field("reset_database_url", &self.reset_database_url.as_ref().map(|_| "<redacted>"))`. In `from_lookup`, before `Ok(Self {`: `let reset_database_url = get("RESET_DATABASE_URL").filter(|s| !s.is_empty());` and add `reset_database_url,` to the struct literal.

- [ ] **Step 5: The reset module** — create `bins/api/src/reset.rs`:

```rust
//! POST /api/paper/reset and GET /api/paper/epochs (Phase 7a spec §5). The api's only write: one
//! row in paper_reset_requests, through its own one-connection pool as paper_reset_rw; the
//! scanner applies it within ~30 s. Traefik's forward-auth (owner only) guards every /api route;
//! the custom header forces a CORS preflight no cross-site page can pass, and Sec-Fetch-Site,
//! when a browser sends it, must be same-origin.
use crate::error::ApiError;
use crate::{AppState, now_ms};
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use store::ResetInsert;

pub const INTENT_HEADER: &str = "x-ct-intent";
pub const INTENT: &str = "reset-paper";
const DAY_MS: i64 = 86_400_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    confirm: String,
}

#[derive(Debug, Serialize)]
pub struct QueuedDto {
    pub request_id: i64,
    pub requested_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct EpochDto {
    pub epoch: i32,
    pub start_ms: i64,
    pub cause: String,
}

#[derive(Debug, Serialize)]
pub struct RequestDto {
    pub id: i64,
    pub requested_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct EpochsDto {
    pub epochs: Vec<EpochDto>,
    /// 0 before the first epoch row (epoch 0 is implicit).
    pub current: i32,
    pub pending_request: Option<RequestDto>,
    pub next_allowed_ms: i64,
}

/// The next UTC midnight after `ms`.
pub fn next_midnight(ms: i64) -> i64 {
    (ms.div_euclid(DAY_MS) + 1) * DAY_MS
}

/// After today's request: the next UTC midnight; otherwise now.
pub fn next_allowed(latest_requested_ms: Option<i64>, now: i64) -> i64 {
    match latest_requested_ms {
        Some(r) if r.div_euclid(DAY_MS) == now.div_euclid(DAY_MS) => next_midnight(r),
        _ => now,
    }
}

/// `YYYY-MM-DD` of a UTC instant (civil-from-days, H. Hinnant; integers only).
pub fn utc_date(ms: i64) -> String {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn check_headers(h: &HeaderMap) -> Result<(), ApiError> {
    if h.get(INTENT_HEADER).and_then(|v| v.to_str().ok()) != Some(INTENT) {
        return Err(ApiError::Forbidden(
            "forbidden",
            "A reset needs the X-CT-Intent: reset-paper header.".to_string(),
        ));
    }
    if let Some(site) = h.get("sec-fetch-site")
        && site.to_str().ok() != Some("same-origin")
    {
        return Err(ApiError::Forbidden(
            "forbidden",
            "Only the dashboard itself can ask for a reset.".to_string(),
        ));
    }
    Ok(())
}

/// Checks in order: headers (403), body (400), the pool (503), a pending request (409), then the
/// insert (202, or 429 on the unique utc_day: one per UTC day, races included).
pub async fn reset(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<QueuedDto>), ApiError> {
    check_headers(&headers)?;
    match serde_json::from_slice::<Confirm>(&body) {
        Ok(c) if c.confirm == "reset" => {}
        _ => {
            return Err(ApiError::BadRequest(
                "bad_request",
                "The body must be {\"confirm\": \"reset\"}.".to_string(),
            ));
        }
    }
    let Some(reset) = st.reset.as_ref() else {
        return Err(ApiError::Unavailable(
            "reset_disabled",
            "Paper reset is not configured on this server.",
        ));
    };
    if let Some(p) = reset.pending_reset_request().await? {
        return Err(ApiError::Conflict(
            "reset_pending",
            format!(
                "Request {} is queued; the scanner applies it within about 30 s.",
                p.id
            ),
        ));
    }
    let now = now_ms();
    match reset.insert_reset_request(now).await? {
        ResetInsert::Queued(r) => Ok((
            StatusCode::ACCEPTED,
            Json(QueuedDto {
                request_id: r.id,
                requested_ms: r.requested_ms,
            }),
        )),
        ResetInsert::DailyLimit => Err(ApiError::TooMany(
            "reset_daily_limit",
            format!(
                "One reset per UTC day. The next one is allowed from {} 00:00 UTC.",
                utc_date(next_midnight(now))
            ),
        )),
    }
}

pub async fn epochs(State(st): State<AppState>) -> Result<Json<EpochsDto>, ApiError> {
    let list = st.store.paper_epochs().await?;
    let pending = st.store.pending_reset_request().await?;
    let latest = st.store.latest_reset_request().await?;
    Ok(Json(EpochsDto {
        current: list.last().map_or(0, |e| e.epoch),
        epochs: list
            .into_iter()
            .map(|e| EpochDto {
                epoch: e.epoch,
                start_ms: e.start_ms,
                cause: e.cause,
            })
            .collect(),
        pending_request: pending.map(|r| RequestDto {
            id: r.id,
            requested_ms: r.requested_ms,
        }),
        next_allowed_ms: next_allowed(latest.map(|r| r.requested_ms), now_ms()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_midnights_are_utc() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_790_121_600_000), "2026-09-23");
        assert_eq!(utc_date(951_782_400_000), "2000-02-29");
        assert_eq!(next_midnight(1_790_121_600_000), 1_790_208_000_000);
        assert_eq!(next_midnight(1_790_208_000_000 - 1), 1_790_208_000_000);
        let now = 1_790_121_600_000 + 5_000;
        assert_eq!(next_allowed(None, now), now);
        assert_eq!(next_allowed(Some(now - DAY_MS), now), now, "yesterday's");
        assert_eq!(next_allowed(Some(now - 1_000), now), 1_790_208_000_000);
    }
}
```

- [ ] **Step 6: `lib.rs` and `main.rs`** — `bins/api/src/lib.rs`: replace lines 1-2 with
```rust
//! api: the dashboard's JSON + SSE server (Phase 4 spec §4). Reads Postgres only through `store`,
//! as analyze_ro, on read-only sessions. Its one write (Phase 7a §5) is a row in
//! paper_reset_requests, through a second one-connection pool as paper_reset_rw; paper only
//! (spec §16).
```
Add `pub mod reset;` after `pub mod params;`; change `use axum::routing::get;` to `use axum::routing::{get, post};`. `AppState`: add after `pub store: Store,`:
```rust
    /// Phase 7a: the reset pool (paper_reset_rw, INSERT on paper_reset_requests only). None:
    /// POST /api/paper/reset answers 503 reset_disabled.
    pub reset: Option<Store>,
```
In `AppState::new`'s literal add `reset: None,`; after `new` add:
```rust
    pub fn with_reset(mut self, reset: Option<Store>) -> Self {
        self.reset = reset;
        self
    }
```
After `ensure_read_only` add:
```rust
/// Spec 7a §5.1: the reset pool's role may INSERT into paper_reset_requests and nothing else
/// (mirrors `ensure_read_only`).
pub async fn ensure_reset_role(reset: &Store) -> anyhow::Result<()> {
    let extra = reset.role_writes_outside_reset().await?;
    anyhow::ensure!(
        extra.is_empty(),
        "refusing to start: the reset role can write {}; connect as paper_reset_rw",
        extra.join(", ")
    );
    Ok(())
}
```
In `router`, after the `/api/paper/ledger` route:
```rust
        .route("/api/paper/reset", post(reset::reset))
        .route("/api/paper/epochs", get(reset::epochs))
```
Replace `run` and `run_on` (lines 109-154) with:
```rust
/// Bind `bind` and serve `state` (see `run_on_state`); also logs the startup line ops tooling
/// greps for.
pub async fn run(
    state: AppState,
    bind: &str,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    eprintln!("api: listening on {bind} (read-only except paper reset requests, paper only)");
    run_on_state(listener, state, health_every, shutdown).await
}

/// `run_on_state` with no reset pool.
pub async fn run_on(
    listener: TcpListener,
    store: Store,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    run_on_state(listener, AppState::new(store), health_every, shutdown).await
}

/// Serve `router(state)` on `listener` until `shutdown` resolves, then shut down cleanly: every
/// open SSE stream watches `AppState::shutdown_tx` and ends itself so `axum::serve`'s graceful
/// shutdown (which otherwise waits forever for an infinite stream) can return; only then are the
/// bridge's two tasks aborted and joined (dropping a `JoinHandle` does NOT stop its task — it
/// must be `abort()`ed) and the pools closed. The bridge is stopped, and its pooled LISTEN
/// connection freed, strictly before `store.close()`, which waits for every checked-out
/// connection and would otherwise hang forever on that one.
pub async fn run_on_state(
    listener: TcpListener,
    state: AppState,
    health_every: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let store = state.store.clone();
    let reset = state.reset.clone();
    let bridge = stream::spawn_bridge(store.clone(), state.events.clone(), health_every);
    let shutdown_tx = state.shutdown_tx.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            shutdown.await;
            // `send` is a no-op (and returns `Err`) when there are no receivers yet — e.g. a
            // stream that is mid-handler, between `AppState::new`'s `subscribe()` call site in
            // `stream::sse` and actually polling, would then read the old `false` and never end.
            // `send_replace` always sets the value, so any receiver that subscribes even after
            // this line still observes `true`.
            shutdown_tx.send_replace(true);
        })
        .await?;
    for h in bridge {
        h.abort();
        let _ = h.await;
    }
    if let Some(r) = reset {
        r.close().await;
    }
    store.close().await;
    Ok(())
}
```
`bins/api/src/main.rs`, replace `real_main` (lines 14-29) with:
```rust
async fn real_main() -> anyhow::Result<()> {
    api::guard::check_env()?;
    let cfg = api::config::Config::from_env()?;
    let opts = store::Store::options(&cfg.database_url)?;
    let store =
        store::Store::connect_read_only(opts, cfg.max_conns, cfg.statement_timeout_ms, None)
            .await?;
    api::ensure_read_only(&store).await?;
    // Phase 7a §5.1: the second pool, the api's one write. Unset: POST /api/paper/reset is a 503.
    let reset = match &cfg.reset_database_url {
        Some(url) => {
            let r = store::Store::connect_reset(
                store::Store::options(url)?,
                cfg.statement_timeout_ms,
                None,
            )
            .await?;
            api::ensure_reset_role(&r).await?;
            Some(r)
        }
        None => {
            eprintln!(
                "api: RESET_DATABASE_URL is not set: POST /api/paper/reset answers 503 reset_disabled"
            );
            None
        }
    };
    api::run(
        api::AppState::new(store).with_reset(reset),
        &cfg.bind,
        std::time::Duration::from_secs(15),
        shutdown(),
    )
    .await
}
```

- [ ] **Step 7: Compose and `.env.example`** — `compose.web.yml`, api `environment:` after `API_BIND: 0.0.0.0:8081` (line 23):

```yaml
      # Phase 7a §5.1: the reset pool (paper_reset_rw: INSERT on paper_reset_requests only)
      RESET_DATABASE_URL: postgres://paper_reset_rw:${RESET_RW_DB_PASSWORD:?set RESET_RW_DB_PASSWORD in .env}@postgres:5432/cute_trading
```
Append to `.env.example`:
```
# Phase 7a paper reset role (paper_reset_rw). Migration 0009 creates it NOLOGIN; the runbook sets this password once.
RESET_RW_DB_PASSWORD=change-me-reset-rw
```

- [ ] **Step 8: Run the tests**

Run (PG prefix): `cargo test -p api`
Expected: PASS (`stream.rs` still calls `run_on(listener, store, …)`).
Run: `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok`
Expected: `config-ok`.

- [ ] **Step 9: CI checks and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p scanner --test arch`
```bash
git add bins/api/src/error.rs bins/api/src/lib.rs bins/api/src/main.rs bins/api/src/config.rs bins/api/src/reset.rs bins/api/tests/common/mod.rs bins/api/tests/reset.rs compose.web.yml .env.example
git commit -m "api: POST /api/paper/reset and GET /api/paper/epochs through a second pool as paper_reset_rw"
```

---

### Task 12: api — `GET /api/paper/ladder`, `?epoch=` on equity and ledger, `Position.end_date`

**Files:**
- Modify: `bins/api/Cargo.toml` (`cards` dependency), `bins/api/src/config.rs` (`POLICIES_PATH` + test), `bins/api/src/lib.rs` (`scenarios` field, `with_scenarios`, route, module), `bins/api/src/main.rs` (load scenarios), `bins/api/src/paper.rs:72-95, 160-222`
- Create: `bins/api/src/ladder.rs`
- Modify: `bins/api/tests/common/mod.rs` (helper), `compose.web.yml` (api env)
- Test: create `bins/api/tests/ladder.rs`; append to `bins/api/tests/paper.rs`

**Interfaces:**
- Consumes: `Store::{paper_epochs, paper_accounts_now, ladder_as_of, ladder_sums, live_run_ids_between, health_latency, newest_live_host, card_results_for, paper_equity(…, epoch), paper_ledger_page(…, epoch), paper_positions}` (Tasks 5-6); `cards::edge::{LADDER, EXTRAS, host_from_key}`, `cards::ladder::{Rung, BreakEven, break_even, measured}`, `cards::scenarios::parse`, `cards::body::ScenarioLat` (Task 9).
- Produces: `AppState::scenarios: Option<Arc<Vec<ScenarioLat>>>`, `AppState::with_scenarios(self, s: Option<Vec<ScenarioLat>>) -> AppState`; `Config::policies_path: String` (default `/etc/cute-trading/policies.toml`); route `GET /api/paper/ladder` → `{epoch, start_ms, as_of_ms, rungs: [{account_id, scenario, latency_ms, extra, n_exec, n_leg_risk, n_missed, gains, leg_risk, settle, unrealized, net}], markets: SweetRow[], measured: {latency_ms, host} | null, markets_status: "live"|"warming"|"under_review"|"missing", break_even}`; `?epoch=N` (N ≥ 0) on `/api/paper/equity` and `/api/paper/ledger`; `PositionDto::end_date: Option<String>`.

- [ ] **Step 1: Write the failing tests** — `bins/api/tests/common/mod.rs`, append:

```rust
/// The api with the repo's policies file (POLICIES_PATH in production).
pub async fn app_with_policies(db: &TestDb) -> (Router, AppState) {
    let ro = Store::connect_read_only(db.opts.clone(), 4, 5_000, Some("analyze_ro"))
        .await
        .expect("read-only pool");
    let scenarios = cards::scenarios::parse(include_str!("../../../../config/policies.toml"))
        .expect("repo policies");
    let st = AppState::new(ro).with_scenarios(Some(scenarios));
    (api::router(st.clone()), st)
}
```
Create `bins/api/tests/ladder.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde_json::json;
use store::*;

/// Epoch 1's start.
const S: i64 = 1_790_121_600_000;
const R0: &str = "P0-converter/rung-0";
const COLO: &str = "P0-converter/colo-idle";
const BURST: &str = "P0-converter/sp-burst";

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

async fn accounts(owner: &Store, run: i64) {
    for (id, scenario, bips) in [
        (R0, "rung-0", 0),
        (COLO, "colo-idle", 0),
        (BURST, "sp-burst", 0),
        ("P0-converter/sp-idle/fee10", "sp-idle", 10), // a fee variant is never a rung
    ] {
        sql(owner, format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run}, '{id}', 'P0-converter', '{scenario}', {bips}, 1000, 100, 'sha')"
        ))
        .await;
    }
}

async fn led(owner: &Store, run: i64, id: &str, seq: i64, t: i64, kind: &str, realized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
         qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
         VALUES ({run}, '{id}', {seq}, {t}, '{kind}', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, {realized}, 1000, NULL)"
    ))
    .await;
}

async fn mark(owner: &Store, run: i64, id: &str, t: i64, equity: &str, unrealized: &str) {
    sql(owner, format!(
        "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
         max_drawdown, n_positions) VALUES ({run}, '{id}', {t}, 1000, {equity}, 0, {unrealized}, 0, 0, 0, 0)"
    ))
    .await;
}

async fn exec(owner: &Store, run: i64, id: &str, ep: &str, t: i64, q_eff: &str, leg: &str) {
    sql(owner, format!(
        "INSERT INTO paper_execs (run_id, account_id, episode_id, exec_ms, q_eff, leg_risk_qty, pnl_at_tx, hit) \
         VALUES ({run}, '{id}', '{ep}', {t}, {q_eff}, {leg}, NULL, false)"
    ))
    .await;
}

async fn sweet(owner: &Store, status: &str) {
    let body = json!({
        "status": status,
        "series": {"sweet": {"rows": [
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "category", "bucket": "crypto", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "rung-0", "dimension": "days", "bucket": "[0,1)", "label": "thin"},
            {"side": "yes", "fee_bips": 0, "scenario": "colo-idle", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 10, "scenario": "sp-idle", "dimension": "all", "bucket": "all", "label": "thin"},
            {"side": "no", "fee_bips": 0, "scenario": "sp-burst", "dimension": "all", "bucket": "all", "label": "thin"}
        ]}}
    });
    sql(owner, format!(
        "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('SWEET', '30d', 'k', '{status}', '{body}'::jsonb, 1)"
    ))
    .await;
}

#[tokio::test]
async fn the_ladder_sums_each_kind_since_the_epoch_start_and_reconciles_exactly() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let r1 = common::run(&owner, "live").await;
    let r2 = common::run(&owner, "live").await; // a restart inside the same epoch
    let rep = common::run(&owner, "replay").await;
    for r in [r1, r2, rep] {
        accounts(&owner, r).await;
    }
    sql(&owner, format!(
        "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
         VALUES (1, {S}, 'policies', NULL, {r1}, 'sha')"
    ))
    .await;
    for id in [R0, COLO, BURST] {
        led(&owner, r1, id, 1, S - 10, "convert", "100").await; // before the epoch
        led(&owner, r1, id, 2, S, "mark", "0").await;
        led(&owner, r1, id, 3, S + 10, "convert", "3").await;
        led(&owner, r1, id, 4, S + 20, "sell", "-5").await;
        led(&owner, r1, id, 5, S + 60_000, "mark", "0").await;
        led(&owner, r2, id, 1, S + 100_000, "settle", "1").await;
        led(&owner, r2, id, 2, S + 120_000, "mark", "0").await;
        led(&owner, r2, id, 3, S + 120_000, "sell", "-7").await; // its mark's ms, after it
        led(&owner, r2, id, 4, S + 130_000, "convert", "50").await; // after the cut
        mark(&owner, r2, id, S + 120_000, "1001", "2").await;
    }
    mark(&owner, r2, COLO, S + 125_000, "999", "0").await; // a resolution row: no ledger mark
    led(&owner, rep, R0, 1, S + 50, "convert", "999").await; // replay never counts
    exec(&owner, r1, R0, "e1", S + 5, "10", "0").await;
    exec(&owner, r1, R0, "e2", S + 15, "0", "5").await;
    exec(&owner, r2, R0, "e3", S + 130_000, "10", "0").await; // after the cut
    exec(&owner, rep, R0, "e4", S + 50, "10", "0").await;
    let mut h = common::health(r2, common::now() - 600_000, 0);
    h.rest_rtt_p50_ms = Some(150); // lag 78 + 150 / 2 = 153
    owner
        .write_batch(&Batch {
            health: vec![h],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .insert_run_premises(&[RunPremiseRow {
            run_id: r2,
            area: "host".into(),
            version: "1:vps-kvm2|100-250".into(),
        }])
        .await
        .unwrap();
    sweet(&owner, "live").await;
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (v["epoch"].clone(), v["start_ms"].clone(), v["as_of_ms"].clone()),
        (json!(1), json!(S), json!(S + 120_000))
    );
    let rungs = v["rungs"].as_array().unwrap();
    let ids: Vec<&str> = rungs.iter().map(|r| r["account_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![R0, COLO, BURST], "ladder by latency, then the extras");
    assert_eq!(
        rungs.iter().map(|r| (r["latency_ms"].clone(), r["extra"].clone())).collect::<Vec<_>>(),
        vec![(json!(0), json!(false)), (json!(52), json!(false)), (json!(450), json!(true))]
    );
    let r0 = &rungs[0];
    assert_eq!(
        (r0["n_exec"].clone(), r0["n_leg_risk"].clone(), r0["n_missed"].clone()),
        (json!(2), json!(1), json!(1))
    );
    for (k, want) in [("gains", "3"), ("leg_risk", "-5"), ("settle", "1"), ("unrealized", "2"), ("net", "1")] {
        assert_eq!(r0[k], want, "{k}");
    }
    for r in rungs {
        let d = |k: &str| r[k].as_str().unwrap().parse::<Decimal>().unwrap();
        assert_eq!(
            d("gains") + d("leg_risk") + d("settle") + d("unrealized"),
            d("net"),
            "equity − bankroll reconciles exactly: {r}"
        );
    }
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    assert_eq!(v["measured"], json!({"latency_ms": 153, "host": "vps-kvm2"}));
    assert_eq!(v["markets_status"], "live");
    let markets = v["markets"].as_array().unwrap();
    assert_eq!(
        markets.iter().map(|m| (m["scenario"].as_str().unwrap(), m["dimension"].as_str().unwrap())).collect::<Vec<_>>(),
        vec![("rung-0", "all"), ("rung-0", "category")],
        "NO side, fee 0, LADDER scenarios, dimensions all + category"
    );
    db.drop(owner).await;
}

#[tokio::test]
async fn before_any_epoch_the_ladder_is_empty_and_thin_and_under_review_markets_are_withheld() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (app, _st) = common::app_with_policies(&db).await;
    let (status, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (v["epoch"].clone(), v["start_ms"].clone(), v["as_of_ms"].clone()),
        (json!(0), json!(null), json!(null))
    );
    assert_eq!(v["rungs"], json!([]));
    assert_eq!(v["break_even"], json!({"kind": "thin"}));
    assert!(v["measured"].is_null());
    assert_eq!((v["markets_status"].clone(), v["markets"].clone()), (json!("missing"), json!([])));
    sweet(&owner, "under_review").await;
    let (_, v) = common::get(&app, "/api/paper/ladder").await;
    assert_eq!(
        (v["markets_status"].clone(), v["markets"].clone()),
        (json!("under_review"), json!([])),
        "right after deploy A SWEET recomputes under the new key"
    );
    db.drop(owner).await;
}
```
Append to `bins/api/tests/paper.rs`:
```rust
async fn epochs(owner: &Store, run_id: i64, starts: &[(i32, i64)]) {
    for (e, s) in starts {
        owner
            .raw_exec(&format!(
                "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
                 VALUES ({e}, {s}, 'policies', NULL, {run_id}, 'sha')"
            ))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn equity_and_ledger_default_to_the_current_epoch_and_take_an_epoch() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let live = common::run(&owner, "live").await;
    let n = common::now();
    account(&owner, live, P0, "P0-converter", "sp-idle").await;
    mark(&owner, live, P0, n - 3 * 3_600_000, "1001").await;
    mark(&owner, live, P0, n - 2 * 3_600_000, "1000").await;
    mark(&owner, live, P0, n - 3_600_000, "1005").await;
    for seq in 1..=3 {
        ledger(&owner, live, P0, seq).await; // t = seq × 1 000
    }
    epochs(&owner, live, &[(1, 2_000), (2, n - 2 * 3_600_000)]).await;
    let (app, _st) = common::app(&db).await;
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}")).await;
    assert_eq!(eq["points"].as_array().unwrap().len(), 2, "{eq}");
    let (_, eq) = common::get(&app, &format!("/api/paper/equity?account={P0Q}&epoch=1")).await;
    assert_eq!(eq["to_ms"], n - 2 * 3_600_000, "an earlier epoch's window ends where the next begins");
    assert_eq!(eq["points"][0]["equity"], "1001");
    assert_eq!(eq["points"].as_array().unwrap().len(), 1);
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}")).await;
    assert_eq!(l["items"], serde_json::json!([]), "no ledger row in epoch 2");
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&epoch=1")).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 2);
    let (_, l) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&epoch=99")).await;
    assert_eq!(l["items"], serde_json::json!([]));
    for bad in ["-1", "x", "99999999999"] {
        let (status, v) = common::get(&app, &format!("/api/paper/ledger?account={P0Q}&epoch={bad}")).await;
        assert_eq!((status, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("bad_epoch")), "{bad}");
    }
    db.drop(owner).await;
}

#[tokio::test]
async fn positions_carry_the_markets_end_date() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    owner
        .raw_exec(
            "INSERT INTO events (event_id, slug, title, neg_risk_market_id, augmented, n_legs, first_seen_ms, last_seen_ms) \
             VALUES ('ev1', 's', 'BTC above 100k on October 3?', NULL, false, 2, 1, 1); \
             INSERT INTO legs (market_id, event_id, yes_token, no_token, status, fee_rate, fee_exponent, end_date, first_seen_ms, last_seen_ms) \
             VALUES ('m1', 'ev1', 'y1', 'n1', 'live', 0.07, 1, '2026-10-03T12:00:00Z', 1, 1)",
        )
        .await
        .unwrap();
    let live = common::run(&owner, "live").await;
    account(&owner, live, "P0-converter/rung-0", "P0-converter", "rung-0").await;
    owner
        .raw_exec(r#"INSERT INTO paper_state (account_id, state, updated_ms) VALUES ('P0-converter/rung-0', '{"positions":[[{"market_id":"m1","yes_token":"y1","outcome":"no"},{"qty":"10","basis":"6"}]],"returned":[]}', 1)"#)
        .await
        .unwrap();
    let (app, _st) = common::app(&db).await;
    let (status, v) = common::get(&app, "/api/paper/positions").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["items"][0]["end_date"], "2026-10-03T12:00:00Z");
    db.drop(owner).await;
}
```
Append to `bins/api/src/config.rs` tests:
```rust
    #[test]
    fn the_policies_path_defaults_to_the_baked_copy() {
        let url = (
            "DATABASE_URL",
            "postgres://analyze_ro:pw@postgres/cute_trading",
        );
        assert_eq!(
            cfg(&[url]).unwrap().policies_path,
            "/etc/cute-trading/policies.toml"
        );
        assert_eq!(
            cfg(&[url, ("POLICIES_PATH", "/tmp/p.toml")])
                .unwrap()
                .policies_path,
            "/tmp/p.toml"
        );
    }
```

- [ ] **Step 2: Run them to see them fail**

Run (PG prefix): `cargo test -p api --test ladder --test paper; cargo test -p api --lib config`
Expected: FAIL to compile: `use of undeclared crate 'cards'`, `no method named 'with_scenarios'`, `no field 'policies_path'`.

- [ ] **Step 3: Dependency, config, state** — `bins/api/Cargo.toml` `[dependencies]`: add `cards = { path = "../../crates/cards" }` after the `store` line. `bins/api/src/config.rs`: in `Config` add
```rust
    /// Phase 7a §7.1: the policies file the ladder reads its latencies from (the copy baked into
    /// the shared image, as analyze uses).
    pub policies_path: String,
```
Debug: `.field("policies_path", &self.policies_path)`; `from_lookup`: `let policies_path = get("POLICIES_PATH").filter(|s| !s.is_empty()).unwrap_or_else(|| "/etc/cute-trading/policies.toml".to_string());` and `policies_path,` in the literal. `bins/api/src/lib.rs`: add `pub mod ladder;` after `pub mod integrity;`; in `AppState` after `pub reset: Option<Store>,`:
```rust
    /// Phase 7a: the policies file's scenario latencies (POLICIES_PATH). None: unknown (every
    /// rung's latency_ms is null).
    pub scenarios: Option<Arc<Vec<cards::body::ScenarioLat>>>,
```
`scenarios: None,` in `new`; after `with_reset`:
```rust
    pub fn with_scenarios(mut self, s: Option<Vec<cards::body::ScenarioLat>>) -> Self {
        self.scenarios = s.map(Arc::new);
        self
    }
```
Route after `/api/paper/epochs`: `.route("/api/paper/ladder", get(ladder::ladder))`.

- [ ] **Step 4: The ladder handler** — create `bins/api/src/ladder.rs`:

```rust
//! GET /api/paper/ladder (Phase 7a spec §7.1): "at which latency would this pay?" For the current
//! epoch, each LADDER account (P0-converter, fee 0) sorted by latency, then the two extras, with
//! its gains (converts), leg-risk cost (unwinds) and settlements since the epoch start, all cut at
//! one common mark so gains + leg_risk + settle + unrealized = net exactly. Live runs only;
//! read-only (analyze_ro). `markets` carries SWEET's 30d NO rows so the panel makes one fetch.
use crate::dto::Dec;
use crate::error::ApiError;
use crate::{AppState, now_ms};
use axum::Json;
use axum::extract::State;
use cards::edge::{EXTRAS, LADDER, host_from_key};
use cards::ladder::{BreakEven, Rung, break_even, measured};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;

const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Serialize)]
pub struct RungDto {
    pub account_id: String,
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

#[derive(Debug, Serialize)]
pub struct MeasuredDto {
    pub latency_ms: i64,
    pub host: String,
}

#[derive(Debug, Serialize)]
pub struct LadderDto {
    pub epoch: i32,
    pub start_ms: Option<i64>,
    pub as_of_ms: Option<i64>,
    pub rungs: Vec<RungDto>,
    pub markets: Vec<Value>,
    pub measured: Option<MeasuredDto>,
    pub markets_status: &'static str,
    pub break_even: BreakEven,
}

/// SWEET's card status as the hero's fold needs it: a `thin` card still carries its rows.
fn markets_status(body: &Value) -> &'static str {
    match body["status"].as_str() {
        Some("live") | Some("thin") => "live",
        Some("warming") => "warming",
        Some("under_review") => "under_review",
        _ => "missing",
    }
}

/// NO-side rows at fee 0 for the LADDER scenarios, dimensions `all` and `category`.
fn ladder_rows(body: &Value) -> Vec<Value> {
    body["series"]["sweet"]["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|r| {
                    r["side"].as_str() == Some("no")
                        && r["fee_bips"].as_i64() == Some(0)
                        && r["scenario"].as_str().is_some_and(|s| LADDER.contains(&s))
                        && r["dimension"]
                            .as_str()
                            .is_some_and(|d| d == "all" || d == "category")
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub async fn ladder(State(st): State<AppState>) -> Result<Json<LadderDto>, ApiError> {
    let now = now_ms();
    let epochs = st.store.paper_epochs().await?;
    let (epoch, start_ms) = epochs
        .last()
        .map_or((0, None), |e| (e.epoch, Some(e.start_ms)));
    let latency = |scenario: &str| {
        st.scenarios
            .as_ref()
            .and_then(|s| s.iter().find(|x| x.name == scenario))
            .map(|x| x.latency_ms)
    };
    let (mut rungs, as_of_ms) = match start_ms {
        // epoch 0 (no epoch row yet): nothing to sum from a common start
        None => (Vec::new(), None),
        Some(start) => {
            let accounts: Vec<_> = st
                .store
                .paper_accounts_now()
                .await?
                .into_iter()
                .filter(|a| {
                    a.policy == "P0-converter"
                        && a.fee_bips.is_zero()
                        && (LADDER.contains(&a.scenario.as_str())
                            || EXTRAS.contains(&a.scenario.as_str()))
                })
                .collect();
            let ids: Vec<String> = accounts.iter().map(|a| a.account_id.clone()).collect();
            let as_of = st.store.ladder_as_of(&ids, start).await?;
            let sums = match as_of {
                Some(cut) => st.store.ladder_sums(&ids, start, cut).await?,
                None => Vec::new(),
            };
            let z = Decimal::ZERO;
            let rungs = accounts
                .iter()
                .map(|a| {
                    let s = sums.iter().find(|x| x.account_id == a.account_id);
                    RungDto {
                        account_id: a.account_id.clone(),
                        scenario: a.scenario.clone(),
                        latency_ms: latency(&a.scenario),
                        extra: EXTRAS.contains(&a.scenario.as_str()),
                        n_exec: s.map_or(0, |s| s.n_exec),
                        n_leg_risk: s.map_or(0, |s| s.n_leg_risk),
                        n_missed: s.map_or(0, |s| s.n_missed),
                        gains: Dec(s.map_or(z, |s| s.gains)),
                        leg_risk: Dec(s.map_or(z, |s| s.leg_risk)),
                        settle: Dec(s.map_or(z, |s| s.settle)),
                        unrealized: Dec(s.and_then(|s| s.unrealized).unwrap_or(z)),
                        net: Dec(s.and_then(|s| s.equity).map_or(z, |e| e - a.bankroll)),
                    }
                })
                .collect::<Vec<_>>();
            (rungs, as_of)
        }
    };
    rungs.sort_by(|a, b| {
        (a.extra, a.latency_ms.unwrap_or(i64::MAX), &a.account_id).cmp(&(
            b.extra,
            b.latency_ms.unwrap_or(i64::MAX),
            &b.account_id,
        ))
    });
    let be = break_even(
        &rungs
            .iter()
            .map(|r| Rung {
                latency_ms: r.latency_ms,
                extra: r.extra,
                n_exec: r.n_exec,
                net: r.net.0,
            })
            .collect::<Vec<_>>(),
    );
    let from = now - DAY_MS;
    let runs = st.store.live_run_ids_between(from, now).await?;
    let minutes = st.store.health_latency(&runs, from, now).await?;
    let measured = match measured(minutes.iter().map(|m| (m.lag_p50_ms, m.rest_rtt_p50_ms))) {
        None => None,
        Some(latency_ms) => {
            let host = st.store.newest_live_host().await?;
            Some(MeasuredDto {
                latency_ms,
                host: host
                    .as_deref()
                    .and_then(|v| host_from_key(&format!("host={v}")).map(str::to_string))
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        }
    };
    let sweet = st
        .store
        .card_results_for("30d")
        .await?
        .into_iter()
        .find(|(c, _)| c == "SWEET");
    let (markets_status, markets) = match sweet {
        None => ("missing", Vec::new()),
        Some((_, body)) => {
            let v: Value = serde_json::from_str(&body).map_err(|e| {
                eprintln!("api: unreadable card SWEET: {e}");
                ApiError::Internal
            })?;
            match markets_status(&v) {
                s @ ("live" | "warming") => (s, ladder_rows(&v)),
                s => (s, Vec::new()),
            }
        }
    };
    Ok(Json(LadderDto {
        epoch,
        start_ms,
        as_of_ms,
        rungs,
        markets,
        measured,
        markets_status,
        break_even: be,
    }))
}
```

- [ ] **Step 5: `?epoch=` and `end_date`** — `bins/api/src/paper.rs`: in `PositionDto` add `pub end_date: Option<String>,` after `event_title`, and `end_date: p.end_date,` in its `From`. Add after `decode_ledger_cursor`:

```rust
/// `?epoch=N` (Phase 7a §6): 0 or more; absent = the current epoch.
fn epoch(q: &Params) -> Result<Option<i32>, ApiError> {
    match params::opt_i64(q, "epoch", "bad_epoch")? {
        None => Ok(None),
        Some(n) => i32::try_from(n)
            .ok()
            .filter(|n| *n >= 0)
            .map(Some)
            .ok_or_else(|| {
                ApiError::BadRequest("bad_epoch", "epoch must be 0 or more.".to_string())
            }),
    }
}
```
In `equity`, replace
```rust
    let hours = params::hours(&q, 24)?;
    let to_ms = now_ms();
```
with
```rust
    let hours = params::hours(&q, 24)?;
    let epoch = epoch(&q)?;
    // an earlier epoch's window ends where the next one starts (its last point is its final equity)
    let to_ms = match epoch {
        None => now_ms(),
        Some(n) => st
            .store
            .paper_epochs()
            .await?
            .iter()
            .find(|e| e.epoch > n)
            .map_or_else(now_ms, |e| e.start_ms),
    };
```
and `.paper_equity(&account_id, from_ms, to_ms, None)` → `.paper_equity(&account_id, from_ms, to_ms, epoch)`. In `ledger`, before `let page = …`: `let epoch = epoch(&q)?;` and `.paper_ledger_page(&account_id, before, limit, hide_marks, None)` → `.paper_ledger_page(&account_id, before, limit, hide_marks, epoch)`.

- [ ] **Step 6: `main.rs` and compose** — `bins/api/src/main.rs`, in `real_main` before `api::run(`:

```rust
    // Phase 7a §7.1: the ladder's latencies. Missing or unreadable: every rung's latency is null.
    let scenarios = match std::fs::read_to_string(&cfg.policies_path) {
        Ok(text) => match cards::scenarios::parse(&text) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("api: {}: {e}; ladder latencies are null", cfg.policies_path);
                None
            }
        },
        Err(e) => {
            eprintln!("api: read {}: {e}; ladder latencies are null", cfg.policies_path);
            None
        }
    };
```
and change `api::AppState::new(store).with_reset(reset)` to `api::AppState::new(store).with_reset(reset).with_scenarios(scenarios)`. In `compose.web.yml` api `environment:` after the `RESET_DATABASE_URL` line add `      POLICIES_PATH: /etc/cute-trading/policies.toml`.

- [ ] **Step 7: Run the tests**

Run (PG prefix): `cargo test -p api`
Expected: PASS.
Run: `cargo test -p scanner --test arch && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok`
Expected: arch `test result: ok` (api depends on `cards`, never directly on `sim`/`arb-core`), `config-ok`.

- [ ] **Step 8: Full CI and commit**

Run (PG prefix): `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E '^test result|SKIPPED|panicked|FAILED' | sort | uniq -c && cargo deny check bans advisories`
Expected: only `test result: ok` lines (no `SKIPPED`, `FAILED`, `panicked`); deny ok.
```bash
git add bins/api/Cargo.toml bins/api/src/config.rs bins/api/src/lib.rs bins/api/src/main.rs bins/api/src/paper.rs bins/api/src/ladder.rs bins/api/tests/common/mod.rs bins/api/tests/ladder.rs bins/api/tests/paper.rs compose.web.yml Cargo.lock
git commit -m "api: GET /api/paper/ladder (break-even, markets, measured), ?epoch= on equity and ledger, Position.end_date"
```

---

### Task 13: the runbook, step A (deploy A)

**Files:**
- Create: `docs/superpowers/runbooks/phase7a-latency-vps.md`
- Modify: `docs/superpowers/runbooks/s3-paper.md:12-17` (one more bullet after the `capture_drops` bullet)

**Interfaces:**
- Consumes: everything above; Task 3's probe hour in `$HOME/probe` (re-fetched by step 9 if gone).
- Produces: the owner's deploy procedure; after it, migration 0009's production checksum pinned in `crates/store/tests/migration_checksums.rs`.

- [ ] **Step 1: The s3-paper note** — in `docs/superpowers/runbooks/s3-paper.md`, after the `capture_drops > 0` bullet (ends line 17), insert:

```markdown
- **A paper reset (Phase 7a) is a capture line too.** The live scanner captures
  `Ctl::PaperReset { t }` before it resets, and replay resets at that payload `t`, so live and
  replay ledgers still agree across a reset. A dropped `PaperReset` line (capture is best-effort)
  voids the comparison across it, like a dropped `Resolved`. Replay writes no `paper_epochs` row.
  Pre-7a images can neither replay nor compact an hour holding one.
```

- [ ] **Step 2: Write the runbook** — create `docs/superpowers/runbooks/phase7a-latency-vps.md` with exactly this content:

````markdown
# Phase 7a step A on the VPS: latency ladder, paper epochs and the reset route (backend + api)

One deploy: migration 0009 (paper_epochs, paper_reset_requests, paper_reset_rw, a paper_ledger index), the scanner with the ladder policies (this starts epoch 1), analyze, compact on the 7a image, the api with its reset pool. The web is unchanged (step B, plan 7a-B).
Rules: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; PC steps in Git Bash from the main checkout on the merged commit, VPS steps as root over SSH. Fill `'<…>'` values before running a step.

## On the PC

1. The repo on the commit being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s'
```
→ expected: no status lines, the branch name, then `<sha> <subject>` of the commit to deploy.
if not: commit or stash the listed changes, then run this step again.

2. Line endings: migrations and configs baked into the image must be LF.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git ls-files --eol crates/store/migrations config | grep -v 'w/lf' ; grep -rlI "$(printf '\r')" crates/store/migrations config || echo lf-ok
```
→ expected: only `lf-ok`.
if not: `git checkout -- <each listed file>` and run this step again.
**STOP**

3. Migration checksums: 0001–0008 must match production.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p store --test migration_checksums 2>&1 | tail -3
```
→ expected: `test result: ok. 2 passed; 0 failed`.
if not: a CRLF or edited migration: fix it as in step 2 and run again.
**STOP**

4. The Postgres-backed tests really run (store, api, analyze, scanner).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && (exec 3<>/dev/tcp/127.0.0.1/15432) && echo pg-up && ( set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:15432/postgres" STORE_TESTS_REQUIRED=1; cargo test -p store -p api -p analyze -p scanner -- --nocapture 2>&1 | grep -E '^test result|SKIPPED|panicked' | sort | uniq -c; echo "cargo exit=${PIPESTATUS[0]}" )
```
→ expected: `pg-up`, only `<n> test result: ok. …; 0 failed …` lines, then `cargo exit=0`.
if not: no `pg-up` = start the PC Postgres first; any `SKIPPED`/`panicked`/`FAILED`: **STOP**.
**STOP**

5. The new `.env` key on the PC (the value is never printed).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && (grep -q '^RESET_RW_DB_PASSWORD=' .env || printf 'RESET_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env) && grep -c -E '^(ANALYZE_RW_DB_PASSWORD|COMPACT_RW_DB_PASSWORD|RESET_RW_DB_PASSWORD)=' .env
```
→ expected: `3`.
if not: add the missing key the same way, never with an editor that echoes it.

6. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `cargo clean` and `docker builder prune -f --keep-storage 5GB`, then run this step again.

7. Build the Rust image (scanner, api, analyze, compact). The web image is unchanged.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export GIT_SHA="$(git rev-parse --short HEAD)" && echo "$GIT_SHA" && docker compose build --build-arg GIT_SHA="$GIT_SHA" migrate 2>&1 | tail -2
```
→ expected: the short sha, then the build's last lines without `ERROR`.
if not: a `CRLF found` failure means step 2 was skipped; read the first `ERROR` line, fix, run again.
**STOP**

8. The image carries the ladder policies (19 accounts) and the new migration.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint /usr/local/bin/scanner cute-trading/rust:dev --help | head -1; docker create --name ct-7a-peek cute-trading/rust:dev >/dev/null && docker cp ct-7a-peek:/etc/cute-trading/policies.toml - | tar -xO | grep -c '^\[\[accounts\]\]'; docker rm ct-7a-peek >/dev/null
```
→ expected: `read-only Polymarket arbitrage scanner (paper trading only)`, then `19`.
if not: `15` = the image was built from an old tree: redo step 7 from the deployed commit.

9. (Cost gate, spec §3) The busy raw hour from the VPS, unless Task 3 left it in `$HOME/probe`.
```bash
[ -d "$HOME/probe/cap" ] && find "$HOME/probe/cap" -name '*.jsonl.zst' | head -1 && echo probe-kept || { rm -rf "$HOME/probe" && mkdir -p "$HOME/probe/cap" && ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 'command -v zstdcat >/dev/null || apt-get install -y zstd >/dev/null; cd /var/lib/docker/volumes/cute-trading_data/_data/capture && F=$(find . -name "[0-9][0-9].jsonl.zst" -mmin +70 -mmin -2880 -printf "%s %p\n" | sort -n | tail -1 | cut -d" " -f2) && U=$(for u in $(zstdcat "$F" | grep -o "universe-[0-9]*-[0-9]*\.json\.zst" | sort -u); do find . -name "$u"; done) && echo "busy hour: $F" >&2 && tar -cf - "$F" $U' | tar -xf - -C "$HOME/probe/cap" && find "$HOME/probe/cap" -type f | sort; }; cd /c/Users/Isabelly/PycharmProjects/cute-trading && git log --format=%h -1 -- config/policies.toml && git show "$(git log --format=%h -1 -- config/policies.toml)~1:config/policies.toml" > "$HOME/probe/policies-old.toml" && grep -c '^\[\[accounts\]\]' "$HOME/probe/policies-old.toml"
```
→ expected: a `.jsonl.zst` path and `probe-kept` (or `busy hour: …` and the fetched files), then the ladder commit's short sha, then `15`.
if not: `busy hour:` empty = no closed hour in 2 days: drop `-mmin -2880` and run again.

10. (Cost gate) Replay the hour twice with and twice without the ladder on the image just built.
```bash
P="$(cygpath -m "$HOME/probe")"; for i in 1 2; do for v in old new; do if [ "$v" = old ]; then M="-v $P/policies-old.toml:/etc/cute-trading/policies.toml:ro"; else M=""; fi; printf '%s ' "$v"; MSYS_NO_PATHCONV=1 docker run --rm -v "$P/cap:/probe/cap:ro" $M --entrypoint /usr/local/bin/scanner cute-trading/rust:dev replay /probe/cap --dry-run 2>/dev/null | tail -1; done; done | tee "$HOME/probe/gate.txt"
```
→ expected: four lines `old|new frames <N> parse_errors <n> episodes <n> paper_rows <n> wall_ms <ms> peak_rss_kb <kb>` with the same `<N>` (> 0).
if not: an error line = a universe record is missing: `rm -rf "$HOME/probe"` and redo step 9.

11. (Cost gate) The verdict: peak RSS grows < 10 % and < 50 MB, wall time < 10 %.
```bash
python - "$HOME/probe/gate.txt" <<'PY'
import sys
rows = {"old": [], "new": []}
for line in open(sys.argv[1]):
    f = line.split()
    kv = dict(zip(f[1::2], f[2::2]))
    rows[f[0]].append((int(kv["wall_ms"]), int(kv["peak_rss_kb"]), int(kv["frames"])))
(ow, orss, of), (nw, nrss, nf) = [(min(r[0] for r in rows[k]), max(r[1] for r in rows[k]), rows[k][0][2]) for k in ("old", "new")]
assert of == nf and of > 0, f"frames differ or zero: {of} {nf}"
drss = nrss - orss
print(f"wall_ms old {ow} new {nw} (+{100 * (nw - ow) / ow:.1f} %); peak_rss_kb old {orss} new {nrss} (+{100 * drss / orss:.1f} %, +{drss // 1024} MB)")
print("GATE PASS" if nw < ow * 1.10 and drss < orss * 0.10 and drss < 50 * 1024 else "GATE FAIL")
PY
```
→ expected: the figures line, then `GATE PASS`; write both lines in Results.
if not: `GATE FAIL`: **STOP**, do not deploy; bring the figures to the owner.
**STOP**

12. Bundle the image, compose files, `.env.example` and roles script.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7a/docker/postgres/init" && cd "$HOME/ct-7a" && docker save cute-trading/rust:dev | gzip -1 > image.tar.gz && cp "$R/compose.yml" "$R/compose.web.yml" "$R/.env.example" . && cp "$R/docker/postgres/init/01-roles.sh" docker/postgres/init/ && tar -cf "$HOME/ct-7a.tar" image.tar.gz compose.yml compose.web.yml .env.example docker && cd "$HOME" && rm -rf "$HOME/ct-7a" && ls -l "$HOME/ct-7a.tar" && docker builder prune -f --keep-storage 5GB >/dev/null && cd "$R"
```
→ expected: one `ls -l` line for `ct-7a.tar`, roughly 25–60 MB.
if not: free disk (step 6) and run this step again.

13. Ship the bundle.
```bash
scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7a.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7a.tar"
```
→ expected: the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

14. Rollback tags on the current images (before step 16's load moves `:dev`).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/rust:dev cute-trading/rust:pre7a && docker tag cute-trading/web:dev cute-trading/web:pre7a; docker image inspect -f '{{.Id}}' cute-trading/rust:pre7a cute-trading/web:pre7a; docker inspect -f '{{.Name}} {{.Image}}' cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1 cute-trading-api-1 cute-trading-web-1
```
→ expected: two `sha256:…` IDs, then scanner, analyze, compact and api on the first ID and web on the second.
if not: a container on another ID: `docker tag <its sha256 ID> cute-trading/rust:pre7a` and run the last two commands again until they match.
**STOP**

15. Back up the database.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres pg_dump -U ct_owner -d cute_trading -Fc > /root/ct-pre-7a.dump && ls -lh /root/ct-pre-7a.dump
```
→ expected: one `ls` line, a few hundred MB.
if not: free space and run this step again; do not migrate without it.
**STOP**

16. Load the bundle (replaces compose.yml, compose.web.yml, .env.example and the roles script).
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7a.tar && rm /root/ct-7a.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/*' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/rust:dev`; `rust:dev` on a new ID, `rust:pre7a` and `web:dev`/`web:pre7a` on the old ones; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: truncated tar = re-ship (step 13); over budget = `docker image prune -f` and run the budget again.

17. Compose parses with the new keys.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'RESET_RW_DB_PASSWORD' compose.yml compose.web.yml && grep -c 'POLICIES_PATH' compose.web.yml
```
→ expected: `config-ok`, `compose.yml:1`, `compose.web.yml:1`, then `1`.
if not: the bundle's compose files are old: re-bundle (step 12).
**STOP**

18. The new `.env` key on the VPS (never printed).
```bash
cd /home/isoca/projects/cute-trading && { grep -q '^RESET_RW_DB_PASSWORD=' .env || printf 'RESET_RW_DB_PASSWORD=%s\n' "$(openssl rand -hex 24)" >> .env; } && chmod 600 .env && grep -c '^RESET_RW_DB_PASSWORD=' .env
```
→ expected: `1`.
if not: remove duplicate lines with `sed -i` on the key name (never print values), then run again.

19. Stop the scanner, analyze and compact (capture pauses until step 23), then recreate postgres on purpose (its environment gained `RESET_RW_DB_PASSWORD`; the Radar shows errors for a few seconds).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze compact && docker compose up -d --no-build --timeout 60 postgres && for i in $(seq 90); do [ "$(docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1)" = healthy ] && break; sleep 2; done; docker inspect -f '{{.State.Health.Status}}' cute-trading-postgres-1
```
→ expected: scanner, analyze and compact `Stopped`, `Container cute-trading-postgres-1 Recreated` then `Started`, then `healthy`.
if not: `docker compose logs --tail 50 postgres`; start nothing else.
**STOP**

20. Migrate ALONE (it also builds the paper_ledger index while nothing inserts).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core run --rm --no-deps migrate; echo "migrate exit=$?"
```
→ expected: `migrate exit=0`.
if not: `docker compose --profile core logs --tail 50 migrate`; start nothing; a checksum error = a CRLF image: redo steps 2–16.
**STOP**

21. Migration 9 is applied, the grants are right and the index exists.
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT max(version) FROM _sqlx_migrations WHERE success" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT has_table_privilege('scanner_rw','paper_epochs','INSERT'), has_table_privilege('scanner_rw','paper_epochs','UPDATE'), has_table_privilege('scanner_rw','paper_reset_requests','INSERT'), has_table_privilege('analyze_ro','paper_epochs','SELECT'), has_table_privilege('analyze_rw','paper_reset_requests','SELECT'), has_table_privilege('paper_reset_rw','paper_reset_requests','INSERT'), has_table_privilege('paper_reset_rw','paper_reset_requests','UPDATE'), has_table_privilege('paper_reset_rw','paper_epochs','INSERT'), has_table_privilege('paper_reset_rw','episodes','SELECT')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM pg_indexes WHERE indexname = 'paper_ledger_account_t'"
```
→ expected: `9`, then `t|f|f|t|t|t|f|f|f`, then `1`.
if not: start nothing; full rollback (Rollback below).
**STOP**

22. Activate `paper_reset_rw` (pgdata exists, so the init script won't; the password is never printed).
```bash
cd /home/isoca/projects/cute-trading && PW="$(grep -E '^RESET_RW_DB_PASSWORD=' .env | cut -d= -f2-)"; case "$PW" in ''|*[!0-9a-f]*) echo "pw-bad"; PW= ;; *) echo pw-read ;; esac; [ -n "$PW" ] && printf '\\set pw %s\nSET log_min_error_statement = panic;\nALTER ROLE paper_reset_rw LOGIN PASSWORD :'"'"'pw'"'"';\n' "$PW" | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -q -U ct_owner -d cute_trading && echo role-set; unset PW; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT rolcanlogin FROM pg_roles WHERE rolname = 'paper_reset_rw'"
```
→ expected: `pw-read`, `role-set`, `t`.
if not: `pw-bad` = the `.env` line isn't an `openssl rand -hex 24` value: `sed -i '/^RESET_RW_DB_PASSWORD=/d' .env`, redo step 18, run again.
**STOP**

23. Start the new scanner (ladder policies: a policies epoch, epoch 1) and note the deploy time.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7a_deploy_ms && docker compose --profile core up -d --no-build --no-deps scanner && sleep 60 && docker compose --profile core logs --since 70s scanner 2>&1 | grep -E 'paper:|error|panicked' | head
```
→ expected: `paper: policies changed (saved … != current …); all accounts start fresh`, then `paper: policies → epoch 1`, and no `error`/`panicked` lines.
if not: full rollback (Rollback below).
**STOP**

24. Gate: epoch 1 belongs to this run, 19 accounts are registered.
```bash
cd /home/isoca/projects/cute-trading && Q="(SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT epoch, cause, request_id IS NULL, run_id = $Q FROM paper_epochs ORDER BY epoch" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM paper_accounts WHERE run_id = $Q"
```
→ expected: `1|policies|t|t`, then `19`.
if not: **STOP**; scanner logs (`docker compose --profile core logs --tail 50 scanner`) to the owner.
**STOP**

25. Gate: the first minute mark of epoch 1 has 19 accounts at $1,000 (wait for the first minute boundary).
```bash
cd /home/isoca/projects/cute-trading && sleep 90 && Q="(SELECT max(run_id) FROM runs WHERE service = 'scanner' AND mode = 'live')" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*), min(equity), max(equity) FROM paper_marks WHERE run_id = $Q AND t_ms = (SELECT min(t_ms) FROM paper_marks WHERE run_id = $Q AND t_ms >= (SELECT start_ms FROM paper_epochs WHERE epoch = 1))"
```
→ expected: `19|1000|1000` (a few cents off on one or two accounts only if a trade executed in the first minute).
if not: fewer than 19 = accounts missing: **STOP** and report.
**STOP**

26. Start the new analyze.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps analyze && sleep 20 && docker inspect -f '{{.Name}} {{.State.Status}}' cute-trading-analyze-1 && docker compose --profile core logs --since 30s analyze 2>&1 | grep -c -E 'error|panicked'
```
→ expected: `/cute-trading-analyze-1 running`, then `0`.
if not: `docker compose --profile core logs --tail 30 analyze`.

27. Recreate compact on the 7a image (it must read `paper_reset` lines before any reset happens).
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core up -d --no-build --no-deps compact && sleep 60 && [ "$(docker inspect -f '{{.Image}}' cute-trading-compact-1)" = "$(docker image inspect -f '{{.Id}}' cute-trading/rust:dev)" ] && echo compact-on-7a; docker compose --profile core logs --since 70s compact 2>&1 | grep -c -E 'error|refusing'
```
→ expected: `compact-on-7a`, then `0`.
if not: no `compact-on-7a` = compact still runs the old image: run this step again; **no reset may be requested until it prints `compact-on-7a`**.
**STOP**

28. Scanner websockets baseline, then recreate the api only (the web container is not touched: the 2026-09-28 websocket-drop lesson).
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7a_api_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7a_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3" && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && sleep 10 && docker compose -f compose.yml -f compose.web.yml --profile web logs --since 20s api 2>&1 | grep -E 'api:'
```
→ expected: the scanner's start time, three `HH:MI|<shards>|0|0|<msgs>` rows, then `api: listening on 0.0.0.0:8081 (read-only except paper reset requests, paper only)` and no `refusing` or `RESET_DATABASE_URL is not set` line.
if not: `refusing to start: the reset role can write …` = over-granted role: stop the api (`docker compose -f compose.yml -f compose.web.yml --profile web stop api`) and report; `database …` errors = redo step 22.

29. After the api restart: no websocket disconnect.
```bash
cd /home/isoca/projects/cute-trading && sleep 120; T0=$(cat /root/7a_api_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7a_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, then `0`.
if not: reconnects > 0: do NOT roll back; write the count in Results.

## On the VPS / in the browser: checks

30. Unauthenticated requests never reach the api (forward-auth).
```bash
curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/api/paper/epochs; curl -sS -o /dev/null -w '%{http_code}\n' -X POST -H 'X-CT-Intent: reset-paper' -H 'Content-Type: application/json' --data '{"confirm":"reset"}' https://radar.isoca.space/api/paper/reset
```
→ expected: `302` or `401` twice.
if not: `200`/`202` = the api is reachable without login: stop the api at once (step 28's stop command) and report.

31. Logged in (browser, owner account): open `https://radar.isoca.space/api/paper/epochs` and `https://radar.isoca.space/api/paper/ladder`.
→ expected: epochs `{"epochs":[{"epoch":1,…,"cause":"policies"}],"current":1,"pending_request":null,"next_allowed_ms":…}`; ladder with `"epoch":1`, eight `rungs` (six with `"extra":false`, latencies 0, 25, 52, 100, 200, 300, then two extras), `"break_even":{"kind":"thin"}`, `"markets_status":"under_review"` or `"missing"`, `measured` with the host `vps-kvm2`.
if not: a 500 = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 api`; latencies `null` = `POLICIES_PATH` missing (step 17).

32. (Optional, uses today's reset: the whole path end to end, while epoch 1 is minutes old.) Only after step 27 printed `compact-on-7a`. In the browser console on `https://radar.isoca.space/` (logged in):
```js
fetch('/api/paper/reset', {method: 'POST', headers: {'X-CT-Intent': 'reset-paper', 'Content-Type': 'application/json'}, body: JSON.stringify({confirm: 'reset'})}).then(async r => console.log(r.status, await r.text()))
```
→ expected: `202 {"request_id":1,"requested_ms":…}`; the same call again prints `409 {"error":"reset_pending",…}`.
if not: `403` = the page wasn't same-origin (use the Radar tab itself); `503` = `RESET_DATABASE_URL` missing (step 17).

33. (Only after step 32) The scanner applied it as epoch 2 within ~30 s.
```bash
cd /home/isoca/projects/cute-trading && sleep 45 && docker compose --profile core logs --since 60s scanner 2>&1 | grep 'paper: reset' ; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT epoch, cause, request_id FROM paper_epochs ORDER BY epoch" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*), min(equity), max(equity) FROM paper_marks WHERE t_ms = (SELECT start_ms FROM paper_epochs WHERE epoch = 2)"
```
→ expected: `paper: reset → epoch 2 (request 1)`, then `1|policies|` and `2|reset|1`, then `19|1000|1000`; a third console call now prints `429 {"error":"reset_daily_limit",…}`.
if not: no `paper: reset` line after 60 s = the poller isn't running: scanner logs to the owner.

34. No hour lost and compact healthy (run 3 h after step 27).
```bash
cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM compact_hours WHERE status = 'lost' AND done_ms > $(cat /root/7a_deploy_ms)" && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT lag_hours, last_error FROM compact_status"
```
→ expected: `0`, then `<1 or 2>|` with an empty last_error.
if not: a `bad capture line` in last_error = compact runs a pre-7a image: redo step 27.

35. (On the PC) Pin 0009's production checksum and commit it.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && H=$(ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123 "cd /home/isoca/projects/cute-trading && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc \"SELECT encode(checksum, 'hex') FROM _sqlx_migrations WHERE version = 9\"" | tr -d '\r\n ') && echo "hex_len ${#H}" && python - "$H" <<'PY' && cargo test -p store --test migration_checksums 2>&1 | tail -3 && git add crates/store/tests/migration_checksums.rs && git commit -q -m "store: pin migration 0009's checksum" && git log -1 --format='%h %s'
import re, sys
h = sys.argv[1]
assert re.fullmatch(r"[0-9a-f]{96}", h), "not a 96-char hex checksum"
p = "crates/store/tests/migration_checksums.rs"
raw = open(p, newline="").read()
s = raw.replace("\r\n", "\n")
old = "    ),\n];\n"
assert s.count(old) == 1 and "        9,\n" not in s, "APPLIED already has 9 or changed shape"
s = s.replace(old, '    ),\n    (\n        9,\n        "%s",\n    ),\n];\n' % h, 1)
open(p, "w", newline="").write(s.replace("\n", "\r\n") if "\r\n" in raw else s)
print("pinned 9")
PY
```
→ expected: `hex_len 96`, `pinned 9`, `test result: ok. 2 passed; 0 failed`, then `<sha> store: pin migration 0009's checksum`.
if not: `2 passed` missing = the local 0009 differs from production (CRLF? edit?): `git checkout -- crates/store/tests/migration_checksums.rs` and report.

## Rollback

36. Back to the pre-7a images and policies (the old policies file is baked into `rust:pre7a`). Migration 0009 is additive and stays.
```bash
cd /home/isoca/projects/cute-trading && docker compose --profile core stop --timeout 60 scanner analyze && docker tag cute-trading/rust:pre7a cute-trading/rust:dev && docker compose --profile core up -d --no-build --no-deps scanner analyze && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps api && docker inspect -f '{{.Name}} {{.Image}}' cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-api-1
```
→ expected: all three on the `rust:pre7a` ID.
if not: `docker image inspect -f '{{.Id}}' cute-trading/rust:pre7a` and retag.
Notes: a pre-7a scanner never writes `paper_epochs`; its own epoch is recorded only at the next 7a start (sha mismatch). **If a reset happened (step 32), leave compact on the 7a image**: a pre-7a compact hard-errors on an hour holding a `paper_reset` line; otherwise also `docker compose --profile core up -d --no-build --no-deps compact`. A full restore from `/root/ct-pre-7a.dump` is a last resort only (it loses every row since step 15).

## Results

(Fill in: step 11's two lines; step 25's row; step 29's reconnect count; steps 32–33 done or skipped; step 34's row; step 35's commit.)
````

- [ ] **Step 3: Check the runbook's shape**

Run: `grep -c '^→ expected:' docs/superpowers/runbooks/phase7a-latency-vps.md; grep -c '^\*\*STOP\*\*' docs/superpowers/runbooks/phase7a-latency-vps.md; grep -A2 '^→ expected: `migrate exit=0`' docs/superpowers/runbooks/phase7a-latency-vps.md | grep -c '^\*\*STOP\*\*'`
Expected: `36` (one per step, the browser check included), a count ≥ 15, then `1` (the **STOP** right after migrate).

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/runbooks/phase7a-latency-vps.md docs/superpowers/runbooks/s3-paper.md
git commit -m "runbook: phase 7a step A on the VPS (migrate 0009, ladder, compact on 7a before any reset, reset role, pin 0009)"
```

---

## Spec coverage

| Spec item | Task |
|---|---|
| §2 ladder as an explicit list, extras | 9 (`LADDER`, `EXTRAS`), 12 |
| §3 four scenarios/accounts, byte-identical file, 19 accounts; `the_repo_policies_give_four_scenarios` → eight | 3 |
| §3 cost gate (absolute: < 10 % and < 50 MB RSS, < 10 % wall) | 3 (early, owner-run), 13 steps 9–11 (final image) |
| §3 0 ms validation (no 1 ms fallback needed: `validate` accepts ≥ 0) | 1, 3 |
| §4.1 migration 0009, grants for four roles, reset role, index in the stopped window, 01-roles.sh, pin after deploy | 4, 13 (steps 19–22, 35) |
| §4.2 poller off the loop, `last_applied` in memory, collapse to highest id | 8, 5 |
| §4.2 `paper_close` → `reset` (seq kept, reserved/peak cleared, rate kept) → `t + 1` → `mark_now` → log line | 1, 2, 8 |
| §4.2 clamp on every entry point (C2) | 2 |
| §4.2 one `WriterMsg::PaperReset`, flushed alone, dropped whole on data error, cap never evicts, wall-clock `paper_state` | 7, 8, 5 |
| §4.2 request pending at start (C3), policies epoch on empty state / sha mismatch, both → one reset epoch | 8, 2 |
| §4.2 `Ctl::PaperReset`: capture, replay with payload `t`, no-op in feed/compact, no epoch row in replay | 2, 8 |
| §4.2 stale rows of removed accounts filtered | 6 |
| §5.1 POST route: header, Sec-Fetch-Site, body, 202/409/429/503, second pool, start-up refusal, doc/log updated | 11, 5 |
| §5.2 GET epochs | 11 |
| §6 readers: equity/ledger epoch filters, positions filter, accounts_now unchanged, analyze fresh() at epoch, SWEET unchanged | 6, 10, 12 |
| §7.1 GET ladder: common cut, live only, exact reconciliation, markets + status, latency from `POLICIES_PATH`, measured, break_even | 6, 9, 12 |
| 09-27 §7.4 `Position.end_date` | 6, 12 |
| §10 Rust tests (sim, scanner, store, api, break_even incl. property test, analyze, checksum pin) | 1–12, 13 step 35 |
| §11 step A order and gates, rollback, compact before any reset, s3-paper note | 13 |

Gaps not filled here (by design or out of reach of a unit test):
- **Web (§5.3, §7.2, §8, §9, §10 web tests, deploy B):** plan 7a-B.
- **Writer against a real Postgres:** the "poison episode pending, reset still written whole" and "data error drops the reset whole" rules are tested on `Pending`'s group state (Task 7); `flush`'s own `Store` calls have no DB test (the scanner crate has no Postgres test harness).
- **"A failed poll … and the save tick still saves `paper_state`":** the poller's failure path is tested (Task 8); the save tick lives in the engine loop, which has no unit harness; it is independent of the poller task by construction.
- **Exit "Edge capture shows no jump at an epoch start":** pinned by Task 10's test; observed on the VPS only after a real reset (optional runbook steps 32–33 and the next analyze tick).

## Results

(Filled in during execution: Task 3 Step 10's two lines.)
