# S3a · scanner hardening (before paper trading) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the S2c items deferred to "before Phase 3". Once they're fixed, the paper trader (S3) can trust the scanner's episode statistics. The fixes: honest `engine_suspect` timing; sticky shard plans (no reconnect churn every refresh); a bounded re-check fan-out with a REST share reserved for resnapshots; stopping every changed shard before any replacement connects; a bounded graceful stop; an O(1) writer eviction; dropping and reporting any unit above 1,000 tokens. It also adds store migration 0003, so that tables created later are not writable by `scanner_rw` by default.

**Architecture:** Every change stays inside the existing S2c boundaries:
- The `Engine` stays pure (no clock, no IO). A late suspect mark now re-flags the episodes it could have touched and re-emits their rows.
- `shards` gets a `replan` that keeps each unit on its previous connection between retiers.
- The `feed::Supervisor` plans slot changes as a pure function, stops every changed slot at once, and only then starts replacements.
- `checks` gets a two-class REST budget and an in-flight cap on re-checks.
- `writer::Pending` keeps its non-episode rows in `VecDeque`s.
- `run` wraps the whole stop sequence in one deadline.
- The store gets one SQL migration and a role test.

**Tech Stack:** Rust 1.98.1, tokio 1.53 (`JoinSet`, `timeout_at`), `store` (sqlx 0.8, Postgres 17), `arb-core`, `pm-api`. No new crates.

**Spec:** `docs/superpowers/specs/2026-09-22-arb-scanner-design.md`. The relevant sections are §7 (sharding, integrity), §8.1.2 (resilience: global budgets, bounded restarts), §8.2 (re-checker, REST token bucket), §10 (roles, "zero is a measurement"), §15 (store role tests) and §16 (paper-only). Phase 0 answers: `spikes/probe/RESULTS.md`. It builds on `docs/superpowers/plans/2026-09-25-s2c-scanner.md`, which is already implemented.

## Global Constraints

- PAPER TRADING ONLY. No order placement, no wallet, no keys, no signing code. The scanner reaches the network only through `pm-api`, and Postgres only through `store` (`deny.toml` enforces both).
- Money and prices are never f32/f64. The workspace clippy config denies `float_arithmetic`, `unwrap_used` and `expect_used`, so there is no `unwrap`/`expect` outside `#[cfg(test)]` code and test files. Time is integer ms/µs; amounts are `Decimal`.
- **The engine is pure.** `bins/scanner/src/engine.rs` never reads a clock and never does IO. Every time value arrives as an argument, so live and replay produce identical rows (spec §9.3). Any new engine input that live feeds must also be fed by `replay.rs` from the capture's `ctl` lines.
- **Phase 0 limits are binding:**
  - at most **40** websocket connections;
  - at most **1,000** tokens on one connection (5,000/conn failed, with a p99 lag of 15.7 s);
  - hot shards get ≤ 500 tokens;
  - connection attempts: ≤ **2/s** and ≤ **20/min** globally.
- A unit (NegRisk event or canary pair) is **never split** across connections (§7).
- **Zero is a measurement** (§10). Work we didn't do is written as `skipped = true` with a reason, never as a zero and never silently dropped.
- The compose `stop_grace_period` is **30 s** (`compose.yml`). The whole graceful stop, plus the `runs` exit row, must fit inside it.
- Disk budget: the project must stay under 30 GB. No `--release` builds and no `docker build` in this plan. Debug `cargo test`/`clippy` only.
- Commit messages are plain, with **no** `Co-Authored-By` or any attribution trailer. Stage only the files you touch (never `PROJECT.md` / `README.md`).
- Bash prefix for every command: `cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" &&`
- Store tests need Postgres. Start it once with `docker compose -f compose.yml -f compose.dev.yml up -d postgres`, then prefix store test commands with `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" &&`. Without `TEST_DATABASE_URL` the store tests print SKIPPED and pass, which does **not** count as passing for Task 1.
- Deferred (not in this plan; recorded so reviewers don't flag them):
  - A retier (every `retier_every_refreshes` refreshes, 30 min) is still a full re-plan, so it still moves units.
  - A unit that first appears between retiers is placed cold until the next retier.
  - The drift and L1 samplers use the *shared* REST class, not the reserved one.
  - `Io::route` still `try_send`s re-check requests into a 4,096-slot channel. The in-flight cap drains it fast, and a full channel is not expected.
  - Duplicate hot connections (§7 "optionally").

## Review Focus

1. **A top mismatch that resolves after the episode already CLOSED** (median episodes are ~ 3.6 s, and resolution takes ≥ 1 s). The closed row must still be re-emitted with `engine_suspect = true`, or the flag misses exactly the short episodes paper trading cares about. Pinned in Task 2 (`a_late_top_mismatch_also_flags_an_episode_that_closed_before_the_check_resolved`).
2. **A refresh where nothing changed** must produce the identical plan, and it must restart **zero** shards. Pinned in Task 3 (`an_unchanged_universe_replans_to_the_identical_plan`) and Task 4 (`slot_changes_…` no-op case).
3. **A unit leaving from the middle of the plan** must not shift the connection index of every later shard, which would reconnect them all. Its slot stays idle, and the next new unit reuses it. Pinned in Task 3 (`a_shard_whose_units_all_left_stays_an_idle_slot_…`).
4. **An oversize unit** must be reported once, not every 5 minutes. It must be subscribed again as soon as it shrinks back to ≤ 1,000 tokens. Pinned in Task 3 (`a_unit_over_the_phase0_connection_limit_…`).
5. **Postgres or the capture disk stuck at shutdown** must not push the exit past the 30 s grace period. Every step returns by one deadline. Pinned in Task 7 (`shutdown_steps_never_outlive_the_deadline`, `waiting_for_a_busy_thread_stops_at_the_deadline`, `the_whole_stop_fits_the_compose_grace_period`).

## File Structure

```
crates/store/build.rs                        NEW: rerun when migrations/ changes (sqlx::migrate! can't see new files)
crates/store/migrations/0003_explicit_grants.sql  NEW: revoke default INSERT/UPDATE for scanner_rw; explicit grants; kind unit_oversize
crates/store/tests/roles.rs                  MODIFY: a later table is not writable by scanner_rw; scanner tables still are
bins/scanner/src/engine.rs                   MODIFY: suspect() flags open + held-closed episodes; mark_suspect returns rows
bins/scanner/src/replay.rs                   MODIFY: feed mark_suspect its time and keep its rows
bins/scanner/src/run.rs                      MODIFY: route suspect rows; sticky replan + oversize rows; limiter/recheck wiring; shutdown deadline
bins/scanner/src/shards.rs                   MODIFY: MAX_UNIT_TOKENS, oversize, replan (sticky), idle slots
bins/scanner/src/feed.rs                     MODIFY: Option slots, slot_changes(), stop-all-then-start
bins/scanner/src/checks.rs                   MODIFY: RestClass/RestBudget, recheck_offsets, capacity_skips, JoinSet cap
bins/scanner/src/config.rs                   MODIFY: recheck.max_in_flight, recheck.resnapshot_reserved_per_sec
config/scanner.toml                          MODIFY: the two new keys
bins/scanner/src/writer.rs                   MODIFY: Pending on VecDeque
bins/scanner/tests/engine.rs                 MODIFY: late-mismatch tests
bins/scanner/tests/engine_checks.rs          MODIFY: mark_suspect signature + open-episode test
```

---

### Task 1: store migration 0003 — explicit scanner grants and the `unit_oversize` kind

**Files:**
- Create: `crates/store/build.rs`, `crates/store/migrations/0003_explicit_grants.sql`
- Modify: `crates/store/tests/roles.rs` (append one test)

**Interfaces:**
- Consumes: `common::fresh()`, `TestDb::role()`, `Store::{raw_exec, start_run, end_run, write_batch}` (S2b).
- Produces:
  - After this migration, a table the owner creates is **not** writable by `scanner_rw` unless a later migration grants it explicitly. It stays readable (default `SELECT` is kept).
  - The integrity kind `'unit_oversize'` is allowed. Task 4 writes it.

Today, `0001_init.sql` line 5 runs `ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE ON TABLES TO scanner_rw` (plus `USAGE, SELECT ON SEQUENCES`). As a result, every table the owner creates later is writable by the scanner, including `paper_*` and the api/lab tables. `docker/postgres/init/01-roles.sh` only creates the login roles, and all table privileges live in migrations. Default privileges belong to the role that runs the migration (the owner, `ct_owner`), and 0003 runs as that same role, so revoking them `IN SCHEMA public` undoes exactly what 0001 granted. Default privileges only apply when a table is created, so existing tables keep their grants. 0003 still spells them out per table, so the schema documents what the scanner may do.

- [ ] **Step 1: Write the failing test.** Append to `crates/store/tests/roles.rs`:
```rust
#[tokio::test]
async fn a_table_created_after_0003_is_not_writable_by_scanner_but_its_own_tables_are() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    // a later table (think paper_*, api or lab tables) created by the owner role
    owner
        .raw_exec("CREATE TABLE paper_probe (id BIGSERIAL PRIMARY KEY, note TEXT)")
        .await
        .expect("owner creates a later table");
    let scanner = db.role("scanner_rw").await;
    let ins = scanner
        .raw_exec("INSERT INTO paper_probe (note) VALUES ('x')")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&ins), "{ins}");
    let upd = scanner
        .raw_exec("UPDATE paper_probe SET note = 'y'")
        .await
        .unwrap_err();
    assert!(is_permission_denied(&upd), "{upd}");
    scanner
        .raw_exec("SELECT count(*) FROM paper_probe")
        .await
        .expect("reading stays allowed");

    // the scanner's own tables keep their full write path
    let run = scanner
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .expect("INSERT runs (+ sequence)");
    scanner.end_run(run, 2, "x").await.expect("UPDATE runs");
    scanner
        .save_universe(1, 0, "{}")
        .await
        .expect("universe_snapshots INSERT + DELETE");
    scanner
        .raw_exec("UPDATE events SET title = title")
        .await
        .expect("UPDATE events");
    scanner
        .raw_exec("UPDATE legs SET status = status")
        .await
        .expect("UPDATE legs");
    scanner
        .raw_exec("DELETE FROM episode_legs WHERE false")
        .await
        .expect("DELETE episode_legs");
    scanner
        .write_batch(&Batch {
            integrity: vec![IntegrityRow {
                run_id: run,
                ts_ms: 1,
                kind: "unit_oversize".into(),
                shard: None,
                token: None,
                event_id: Some("e1".into()),
                detail: Some("1234 tokens".into()),
            }],
            ..Batch::default()
        })
        .await
        .expect("integrity_events accepts unit_oversize");
    scanner.close().await;
    db.drop(owner).await;
}
```

- [ ] **Step 2: Run it to verify it fails.**
Run: `docker compose -f compose.yml -f compose.dev.yml up -d postgres && set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store --test roles a_table_created_after_0003`
Expected: FAIL. The panic is at the first `unwrap_err()` ("called `Result::unwrap_err()` on an `Ok` value"), because the default privileges still let `scanner_rw` insert into `paper_probe`.

- [ ] **Step 3: Make new migrations visible to the build.** Create `crates/store/build.rs`:
```rust
// `sqlx::migrate!` embeds crates/store/migrations at compile time, but on stable Rust it cannot
// see a NEW file in that directory, so rebuild whenever it changes (sqlx docs: "sqlx migrate
// build-script").
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
```

- [ ] **Step 4: Write the migration.** Create `crates/store/migrations/0003_explicit_grants.sql`:
```sql
-- S3a: least privilege for scanner_rw (spec §10 roles). 0001 granted it INSERT/UPDATE on EVERY
-- table the owner would ever create; from now on a new table (paper_*, api, lab, oracle) is
-- writable by the scanner only if its migration grants that explicitly. SELECT stays default.
-- Default privileges belong to the role running the migrations (the owner), same as in 0001.
ALTER DEFAULT PRIVILEGES IN SCHEMA public REVOKE INSERT, UPDATE ON TABLES FROM scanner_rw;
ALTER DEFAULT PRIVILEGES IN SCHEMA public REVOKE USAGE ON SEQUENCES FROM scanner_rw;

-- The scanner's own tables: exactly what it had, now spelled out per table.
GRANT SELECT, INSERT, UPDATE ON
    runs, universe_snapshots, events, legs, episodes, episode_legs,
    observations, integrity_events, health
TO scanner_rw;
GRANT DELETE ON universe_snapshots, episode_legs TO scanner_rw;
GRANT USAGE, SELECT ON SEQUENCE
    runs_run_id_seq, universe_snapshots_id_seq, observations_id_seq, integrity_events_id_seq
TO scanner_rw;

-- S3a: a unit (NegRisk event) above the Phase 0 per-connection limit is not subscribed and is
-- reported with this kind (bins/scanner shards::MAX_UNIT_TOKENS).
ALTER TABLE integrity_events DROP CONSTRAINT IF EXISTS integrity_events_kind_check;
ALTER TABLE integrity_events ADD CONSTRAINT integrity_events_kind_check CHECK (kind IN (
    'crossed_book', 'mirror_mismatch', 'top_mismatch', 'drift',
    'drop', 'reconnect_gap', 'parse_error', 'shard_parked',
    'capture_drop', 'writer_drop', 'capture_budget_delete',
    'unit_oversize'
));
```

- [ ] **Step 5: Run the store tests to verify they pass.**
Run: `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: PASS, including the new test. `scanner_can_write_its_tables_but_not_delete_episodes_or_ddl`, `analyze_can_read_but_never_write` and `migrations_apply_from_empty_and_are_idempotent` must also still pass. Check that the output does **not** contain `SKIPPED`.

- [ ] **Step 6: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy -p store --all-targets -- -D warnings`
Expected: clean.
```bash
git add crates/store/build.rs crates/store/migrations/0003_explicit_grants.sql crates/store/tests/roles.rs
git commit -m "store: tables created after 0003 need explicit scanner grants; allow unit_oversize"
```

---

### Task 2: engine — a late suspect mark flags episodes already open (and just closed)

**Files:**
- Modify: `bins/scanner/src/engine.rs` (the `Engine` struct, `Engine::new`, `apply_universe`, `resolve_pending`, `check_book`, `close_episode`, `disconnect`, `close_all`, `mark_suspect`; the new `HeldClosed`, `pending_in_group`, `prune_held`)
- Modify: `bins/scanner/src/replay.rs:101-103`, `bins/scanner/src/run.rs` (`Io::route` Drift arm, `CheckCmd::L1Mismatch` arm)
- Test: `bins/scanner/tests/engine.rs`, `bins/scanner/tests/engine_checks.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `Engine::mark_suspect(&mut self, event_id: &str, until_ms: i64, now_ms: i64) -> Vec<Out>`. It returns only `Out::Episode` rows: one per episode whose `engine_suspect` flag it just flipped.
  - `pub(crate) fn suspect(&mut self, g: usize, until_ms: i64, since_ms: i64, out: &mut Vec<Out>)`.

**The bug today.** A delta's top check is deferred (`PendingTop`) and resolved one frame or more later (`resolve_pending`). When it resolves as a real mismatch, `check_book` calls `suspect(g, ts + window)`, which only moves `suspect_until`. An episode that **already opened** in that group (typically in the very frame that created the pending check) is never re-flagged: `open_episode` read `suspect_until` before the mismatch was known, and `peak_episode` only runs on a new peak. Drift and L1 marks (`mark_suspect`) have the same gap. An episode that opened **and closed** before the resolution is also missed.

**The fix.** `suspect(g, until, since, out)` does three things. It extends `suspect_until` as before. It flips `suspect` on every open episode of `g` and re-emits its row. It also flips `suspect` on every *held closed* episode of `g` that closed at or after `since` (the pending check's ts) and re-emits its closed row. The writer and `write_batch` keep the latest version of each episode id.

An episode is *held* when it closes while an unresolved top check on its group has a ts at or before its close. `prune_held` drops held episodes once no such check remains. Held episodes are memory only: correctness comes from the `since` test, so pruning a little late is harmless. Output order is deterministic, because ids are sorted.

- [ ] **Step 1: Write the failing tests.** Append to `bins/scanner/tests/engine.rs` (the file's private `delta(...)` helper and `Side` import already exist):
```rust
/// y0 ask 0.40 → 0.30 opens a basket (ΣA = 0.95) at 2_000, but the server says the final top
/// ask is 0.31: our book is wrong, and the deferred check only finds out a frame later.
fn open_on_a_wrong_book(e: &mut scanner::engine::Engine) -> Vec<Out> {
    seed(e);
    e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.31")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.31")),
        ],
    )
}

#[test]
fn a_late_top_mismatch_flags_the_episode_already_open_in_its_group() {
    let mut e = engine();
    let open = episodes(&open_on_a_wrong_book(&mut e));
    assert_eq!(open.len(), 1);
    assert!(!open[0].engine_suspect, "nothing is known to be wrong yet");
    // a later frame on the same conn (> 1 s past 2_000) resolves the check as a real mismatch
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    let rows = episodes(&outs);
    let last = rows
        .iter()
        .rev()
        .find(|r| r.episode_id == "live-7-00000001")
        .expect("the open episode is re-emitted");
    assert!(last.engine_suspect);
    assert_eq!(last.closed_server_ms, None, "still open");
    let outs = e.close_all(4_000);
    assert!(episodes(&outs)[0].engine_suspect, "and it stays flagged");
}

#[test]
fn a_late_top_mismatch_also_flags_an_episode_that_closed_before_the_check_resolved() {
    let mut e = engine();
    open_on_a_wrong_book(&mut e);
    // y1 ask 0.35 → 0.40 at 2_500: ΣA = 1.00, the episode closes before y0's check is due
    let outs = e.on_frame(
        0,
        2_600,
        &[
            ask("e1-y1", 2_500, "0.40", "10"),
            ask("e1-y1", 2_500, "0.35", "0"),
        ],
    );
    let closed = episodes(&outs);
    assert_eq!(closed[0].closed_server_ms, Some(2_500));
    assert!(!closed[0].engine_suspect);
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    let r = episodes(&outs)
        .into_iter()
        .rev()
        .find(|r| r.episode_id == "live-7-00000001")
        .expect("the closed row is re-emitted with the flag");
    assert!(r.engine_suspect);
    assert_eq!((r.closed_server_ms, r.duration_ms), (Some(2_500), Some(500)));
}

#[test]
fn a_clean_resolution_re_emits_nothing() {
    let mut e = engine();
    seed(&mut e);
    e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.30")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.30")),
        ],
    );
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(episodes(&outs).is_empty(), "{outs:?}");
}
```
In `bins/scanner/tests/engine_checks.rs`, replace `suspect_mark_flags_the_next_episode` and add one test:
```rust
#[test]
fn suspect_mark_flags_the_next_episode() {
    let mut e = engine();
    assert!(e.mark_suspect("e1", 10_000, 0).is_empty(), "nothing open yet");
    open(&mut e);
    let outs = e.close_all(3_000);
    let r = &episodes(&outs)[0];
    assert!(r.engine_suspect);
    assert!(r.censor_right, "close_all right-censors");
}

#[test]
fn a_suspect_mark_flags_an_episode_that_is_already_open() {
    let mut e = engine();
    open(&mut e); // opened at 2_000, not suspect
    let outs = e.mark_suspect("e1", 70_000, 10_000);
    let rows = episodes(&outs);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].engine_suspect && rows[0].closed_server_ms.is_none());
    assert!(
        e.mark_suspect("e1", 80_000, 11_000).is_empty(),
        "already flagged: no duplicate row"
    );
    assert!(e.mark_suspect("nope", 80_000, 11_000).is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p scanner --test engine a_late_top_mismatch`
Expected: FAIL: `the open episode is re-emitted` / `the closed row is re-emitted with the flag` (`Option::expect` on `None`).
Run: `cargo test -p scanner --test engine_checks`
Expected: compile error, "this method takes 2 arguments but 3 arguments were supplied" (`mark_suspect`).

- [ ] **Step 3: Implement in `engine.rs`.**

Add the type after `struct PendingTop`:
```rust
/// An episode that CLOSED while a deferred top-check on its group (ts ≤ its close) was still
/// unresolved: if that check later resolves as a mismatch, the closed row is re-flagged too.
#[derive(Debug, Clone)]
struct HeldClosed {
    ep: OpenEp,
    closed: (i64, i64, Censor),
}
```
Add the field to `pub struct Engine` (after `group_last_ts`) and initialise it in `Engine::new` with `held_closed: Vec::new(),`:
```rust
    /// See `HeldClosed`. Emptied by `prune_held` once no unresolved check can flag them.
    held_closed: Vec<HeldClosed>,
```
Replace `pub(crate) fn suspect` with:
```rust
    /// Mark group `g` suspect until `until_ms`, AND flag right now every episode the bad book
    /// may already have touched: each open episode of `g`, and each held closed one that
    /// closed at or after `since_ms`. Each flag that flips re-emits that episode's row (the
    /// writer and `write_batch` keep the latest version). Sorted ids keep the output order
    /// deterministic (spec §9.3).
    pub(crate) fn suspect(&mut self, g: usize, until_ms: i64, since_ms: i64, out: &mut Vec<Out>) {
        if let Some(t) = self.suspect_until.get_mut(g) {
            *t = (*t).max(until_ms);
        }
        let mut open_ids: Vec<u64> = self
            .open
            .iter()
            .filter(|(_, ep)| ep.group == g && !ep.suspect)
            .map(|(id, _)| *id)
            .collect();
        open_ids.sort_unstable();
        for id in open_ids {
            if let Some(ep) = self.open.get_mut(&id) {
                ep.suspect = true;
            }
            if let Some(ep) = self.open.get(&id) {
                out.push(Out::Episode(Box::new(self.row(ep, None))));
            }
        }
        let held: Vec<usize> = self
            .held_closed
            .iter()
            .enumerate()
            .filter(|(_, h)| h.ep.group == g && !h.ep.suspect && h.closed.0 >= since_ms)
            .map(|(i, _)| i)
            .collect();
        for i in held {
            if let Some(h) = self.held_closed.get_mut(i) {
                h.ep.suspect = true;
            }
            if let Some(h) = self.held_closed.get(i) {
                out.push(Out::Episode(Box::new(self.row(&h.ep, Some(h.closed)))));
            }
        }
    }

    /// Is a deferred top-check on a leg of group `g` still unresolved with ts ≤ `ts_ms`?
    fn pending_in_group(&self, g: usize, ts_ms: i64) -> bool {
        self.pending_tops.iter().any(|(t, p)| {
            p.ts <= ts_ms && self.token_ix.get(t).is_some_and(|&(pg, _)| pg == g)
        })
    }

    /// Forget held closed episodes that no unresolved check can flag any more.
    fn prune_held(&mut self) {
        if self.held_closed.is_empty() {
            return;
        }
        let held = std::mem::take(&mut self.held_closed);
        self.held_closed = held
            .into_iter()
            .filter(|h| self.pending_in_group(h.ep.group, h.closed.0))
            .collect();
    }
```
In `check_book`, change **both** `self.suspect(g, ts + self.ctx.suspect_window_ms);` calls (the top-mismatch one and the crossed-book one) to:
```rust
                self.suspect(g, ts + self.ctx.suspect_window_ms, ts, out);
```
Replace `resolve_pending` with:
```rust
    fn resolve_pending(&mut self, token: &str, out: &mut Vec<Out>) {
        if let Some(p) = self.pending_tops.remove(token) {
            self.check_book(p.conn, token, p.ts, Some((p.bid, p.ask)), out);
            self.prune_held();
        }
    }
```
Replace `close_episode` with:
```rust
    pub(crate) fn close_episode(&mut self, e: EpisodeEvent, out: &mut Vec<Out>) {
        if let EpisodeEvent::Close {
            id,
            ts_ms,
            duration_ms,
            censor,
            ..
        } = e
            && let Some(ep) = self.open.remove(&id)
        {
            self.open_by_key.remove(&ep.key);
            let closed = (ts_ms, duration_ms, censor);
            out.push(Out::Episode(Box::new(self.row(&ep, Some(closed)))));
            if !ep.suspect && self.pending_in_group(ep.group, ts_ms) {
                self.held_closed.push(HeldClosed { ep, closed });
            }
        }
    }
```
In `apply_universe`, directly **before** `self.universe = u;` (the old universe is still in place, and `index` maps event id → new group index), add:
```rust
        // Held closed episodes follow their event to its new index; an event that left the
        // universe drops them (its deferred checks were dropped with its tokens above).
        let held = std::mem::take(&mut self.held_closed);
        self.held_closed = held
            .into_iter()
            .filter_map(|mut h| {
                let ev = &self.universe.groups[h.ep.group].event_id;
                index.get(ev).map(|&g| {
                    h.ep.group = g;
                    h
                })
            })
            .collect();
```
In `disconnect`, add `self.prune_held();` directly before the final `out`. In `close_all`, add `self.held_closed.clear();` after `self.pending_tops.clear();`. Replace `mark_suspect` with:
```rust
    /// A REST check (drift, L1 mirror) found this event's book wrong at `now_ms`: suspect until
    /// `until_ms`, and flag its open episodes now. Returns their re-emitted rows.
    pub fn mark_suspect(&mut self, event_id: &str, until_ms: i64, now_ms: i64) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(g) = self
            .universe
            .groups
            .iter()
            .position(|m| m.event_id == event_id)
        {
            self.suspect(g, until_ms, now_ms, &mut out);
        }
        out
    }
```

- [ ] **Step 4: Feed it from replay and live.** In `bins/scanner/src/replay.rs`, replace the `Ctl::Suspect` arm with:
```rust
                Some(Ctl::Suspect { event_id, until_ms }) => {
                    out.outs.extend(engine.mark_suspect(&event_id, until_ms, ms))
                }
```
In `bins/scanner/src/run.rs` `Io::route`, replace the `if let Some(ev) = event_id { … }` block inside the `Out::Drift` arm with:
```rust
                    if let Some(ev) = event_id {
                        let t = now_ms();
                        let until = t + self.suspect_window_ms;
                        let flagged = engine.mark_suspect(&ev, until, t);
                        self.ctl(
                            0,
                            Ctl::Suspect {
                                event_id: ev,
                                until_ms: until,
                            },
                            t,
                        );
                        // `mark_suspect` returns only re-flagged episode rows
                        for f in flagged {
                            if let Out::Episode(w) = f {
                                self.writer
                                    .send(WriterMsg::Episode(*w))
                                    .await
                                    .context("writer gone")?;
                            }
                        }
                    }
```
and in `run_live`'s `check_rx` arm, replace the `CheckCmd::L1Mismatch` arm with:
```rust
                    CheckCmd::L1Mismatch { event_id, detail } => {
                        let t = now_ms();
                        let until = t + io.l1_window_ms;
                        let mut outs = engine.mark_suspect(&event_id, until, t);
                        io.ctl(0, Ctl::Suspect { event_id: event_id.clone(), until_ms: until }, t);
                        outs.push(Out::Integrity(io.integrity("mirror_mismatch", None, Some(event_id), detail)));
                        outs
                    }
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p scanner`
Expected: PASS. That includes the five new or changed tests, and `replay_reproduces_episodes_and_censoring_byte_for_byte` must still pass.

- [ ] **Step 6: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/engine.rs bins/scanner/src/replay.rs bins/scanner/src/run.rs bins/scanner/tests/engine.rs bins/scanner/tests/engine_checks.rs
git commit -m "scanner/engine: a late suspect mark flags episodes already open or just closed"
```

---

### Task 3: shards — oversize units dropped, sticky `replan`, idle slots

**Files:**
- Modify: `bins/scanner/src/shards.rs` (whole file except `Unit`, `canary_key`, `units`)
- Test: the `#[cfg(test)] mod tests` in `bins/scanner/src/shards.rs`

**Interfaces:**
- Consumes: `ShardsCfg` (unchanged).
- Produces (Task 4 relies on these exact names):
  - `pub const MAX_UNIT_TOKENS: usize = 1000;`
  - `Plan { shards, dropped, dropped_tokens, pub oversize: Vec<(String, usize)> }`. `oversize` holds `(key, tokens)` pairs, sorted by key.
  - `Plan::live_shards(&self) -> usize` (shards with ≥ 1 token)
  - `Plan::new_oversize(&self, prev: &Plan) -> Vec<(String, usize)>` (entries of `self.oversize` whose key is not in `prev.oversize`)
  - `pub fn replan(prev: &Plan, units: Vec<Unit>, cfg: &ShardsCfg, retier: bool) -> Plan`
  - `plan(units, cfg)` keeps its signature and behaviour for units ≤ `MAX_UNIT_TOKENS`.
  - A `Shard` with empty `tokens` is an **idle slot**: no connection. It only occurs in `replan` output, never at the end of `shards`.

**Rules.**
- A unit above 1,000 tokens (Phase 0) is never planned. Before this change, `plan` gave it "a shard of its own", which is exactly the 5,000/conn failure mode.
- `replan` with `retier = false` (or an empty `prev`):
  - Every unit keeps its previous shard index while it still exists and still fits that shard's tier cap. A unit alone on its shard always fits.
  - A shard emptied by departures stays at its index as an idle, cold slot, so later shards keep their index and their connection.
  - New units, grown units and previously dropped units are placed first-fit and cold: first into a non-empty cold shard with room, then into an idle slot, then into a new shard while under `max_connections`. Otherwise they are dropped.
  - Trailing idle slots are released.
- `retier = true` is exactly `plan(units, cfg)`.

- [ ] **Step 1: Write the failing tests.** Add inside `mod tests` in `shards.rs` (the `cfg` and `unit` helpers already exist):
```rust
    fn cold(key: &str, n: usize) -> Unit {
        unit(key, n, 0)
    }

    #[test]
    fn an_unchanged_universe_replans_to_the_identical_plan() {
        let us = || vec![unit("a", 4, 9), cold("c", 6), cold("d", 6), cold("e", 3)];
        let p = plan(us(), &cfg(40));
        // a hot → 0; c → 1; d (6 + 6 > 10) → 2; e → 1
        assert_eq!(p.conn_of()["e"], 1);
        assert_eq!(replan(&p, us(), &cfg(40), false), p);
    }

    #[test]
    fn a_refresh_only_touches_the_shards_whose_units_changed() {
        let p = plan(
            vec![unit("a", 4, 9), cold("c", 6), cold("d", 6), cold("e", 3)],
            &cfg(40),
        );
        // c leaves, f (2 tokens) arrives
        let q = replan(
            &p,
            vec![unit("a", 4, 9), cold("d", 6), cold("e", 3), cold("f", 2)],
            &cfg(40),
            false,
        );
        assert_eq!(q.shards.len(), p.shards.len());
        assert_eq!(q.shards[0], p.shards[0], "a's shard is untouched");
        assert_eq!(q.shards[2], p.shards[2], "d's shard is untouched");
        assert_eq!(q.shards[1].units, vec!["e".to_string(), "f".to_string()]);
    }

    #[test]
    fn a_shard_whose_units_all_left_stays_an_idle_slot_so_later_shards_keep_their_index() {
        let p = plan(vec![cold("c", 6), cold("d", 6), cold("g", 6)], &cfg(40)); // c 0, d 1, g 2
        let q = replan(&p, vec![cold("c", 6), cold("g", 6)], &cfg(40), false);
        assert_eq!(q.shards.len(), 3);
        assert!(q.shards[1].tokens.is_empty() && q.shards[1].units.is_empty());
        assert_eq!(q.conn_of()["g"], 2, "g did not move");
        assert_eq!(q.live_shards(), 2);
        // the idle slot is where the next new unit goes
        let r = replan(&q, vec![cold("c", 6), cold("g", 6), cold("h", 8)], &cfg(40), false);
        assert_eq!(r.conn_of()["h"], 1);
        // trailing idle slots are released
        let s = replan(&q, vec![cold("c", 6)], &cfg(40), false);
        assert_eq!(s.shards.len(), 1);
    }

    #[test]
    fn a_unit_that_outgrows_its_shard_moves_and_the_rest_stay() {
        let p = plan(vec![cold("c", 6), cold("e", 3)], &cfg(40)); // one shard [c, e] = 9
        let q = replan(&p, vec![cold("c", 6), cold("e", 5)], &cfg(40), false);
        assert_eq!(q.conn_of()["c"], 0);
        assert_eq!(q.conn_of()["e"], 1, "6 + 5 > 10: e moves to a new shard");
    }

    #[test]
    fn a_retier_is_a_fresh_plan() {
        let p = plan(vec![cold("c", 6), cold("d", 6)], &cfg(40));
        let us = || vec![unit("c", 6, 5), cold("d", 6), cold("x", 1)];
        assert_eq!(replan(&p, us(), &cfg(40), true), plan(us(), &cfg(40)));
    }

    #[test]
    fn a_unit_over_the_phase0_connection_limit_is_never_planned_and_is_reported_once() {
        let big = |n: usize| unit("big", n, 50);
        let p = plan(vec![big(MAX_UNIT_TOKENS + 1), cold("c", 6)], &cfg(40));
        assert!(!p.conn_of().contains_key("big"));
        assert!(p.shards.iter().all(|s| s.tokens.len() <= MAX_UNIT_TOKENS));
        assert_eq!(p.oversize, vec![("big".to_string(), MAX_UNIT_TOKENS + 1)]);
        assert_eq!(p.new_oversize(&Plan::default()), p.oversize, "first sighting is reported");
        let q = replan(&p, vec![big(MAX_UNIT_TOKENS + 5), cold("c", 6)], &cfg(40), false);
        assert!(q.new_oversize(&p).is_empty(), "still oversize: not reported again");
        // shrinks back to the limit: subscribed again at the next refresh
        let r = replan(&q, vec![big(MAX_UNIT_TOKENS), cold("c", 6)], &cfg(40), false);
        assert!(r.conn_of().contains_key("big") && r.oversize.is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p scanner --lib shards`
Expected: compile errors, "cannot find function `replan`", "cannot find value `MAX_UNIT_TOKENS`", "no field `oversize`".

- [ ] **Step 3: Implement.** In `shards.rs`, replace the module doc, `Plan` with its `impl`, and `plan`. Add the helpers and `replan` below `units` (keep `Unit`, `Shard`, `canary_key` and `units` as they are):
```rust
//! Event-affine shard planning (spec §7, Phase 0 Q7e). A unit (NegRisk group or canary pair) is
//! never split. Hot units (top `hot_events` by message rate) share shards of ≤ hot_shard_tokens,
//! the rest ≤ cold_shard_tokens. At most max_connections shards: once the budget is used, the
//! lowest-priority units are dropped and reported (health.dropped_groups). A unit above
//! MAX_UNIT_TOKENS can't be subscribed safely at all and is never planned (integrity
//! `unit_oversize`). Between retiers, `replan` keeps every unit on its connection (S3a).
```
```rust
/// Phase 0: 5,000 tokens on one connection failed (p99 lag 15.7 s); S2c runs ≤ 1,000. A unit is
/// never split, so a unit above this has no safe connection and is left out of the plan.
pub const MAX_UNIT_TOKENS: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub shards: Vec<Shard>,
    pub dropped: Vec<String>,
    pub dropped_tokens: usize,
    /// Units above MAX_UNIT_TOKENS as (key, tokens), sorted by key: never subscribed.
    pub oversize: Vec<(String, usize)>,
}

impl Plan {
    pub fn conn_of(&self) -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for (i, s) in self.shards.iter().enumerate() {
            for u in &s.units {
                m.insert(u.clone(), i);
            }
        }
        m
    }

    pub fn tokens(&self) -> usize {
        self.shards.iter().map(|s| s.tokens.len()).sum()
    }

    /// Shards that carry a connection (an idle slot has no tokens).
    pub fn live_shards(&self) -> usize {
        self.shards.iter().filter(|s| !s.tokens.is_empty()).count()
    }

    /// Oversize units not already oversize in `prev`: each is reported once, not every refresh.
    pub fn new_oversize(&self, prev: &Plan) -> Vec<(String, usize)> {
        self.oversize
            .iter()
            .filter(|(k, _)| !prev.oversize.iter().any(|(pk, _)| pk == k))
            .cloned()
            .collect()
    }
}
```
```rust
/// Priority: canaries (integrity) → message rate → size (bigger groups carry more baskets) → key.
fn sort_units(units: &mut [Unit]) {
    units.sort_by(|a, b| {
        b.canary
            .cmp(&a.canary)
            .then(b.rate.cmp(&a.rate))
            .then(b.tokens.len().cmp(&a.tokens.len()))
            .then(a.key.cmp(&b.key))
    });
}

/// Move units above MAX_UNIT_TOKENS into `p.oversize`; return the rest.
fn split_oversize(units: Vec<Unit>, p: &mut Plan) -> Vec<Unit> {
    let (ok, big): (Vec<Unit>, Vec<Unit>) = units
        .into_iter()
        .partition(|u| u.tokens.len() <= MAX_UNIT_TOKENS);
    p.oversize = big.into_iter().map(|u| (u.key, u.tokens.len())).collect();
    p.oversize.sort();
    ok
}

/// First fit into a non-empty shard of the same tier with room, else an idle slot, else a new
/// shard while the connection budget allows; otherwise the unit is dropped and counted.
fn place(p: &mut Plan, u: Unit, is_hot: bool, cfg: &ShardsCfg) {
    let cap = if is_hot {
        cfg.hot_shard_tokens
    } else {
        cfg.cold_shard_tokens
    };
    let slot = p
        .shards
        .iter()
        .position(|s| {
            !s.tokens.is_empty() && s.hot == is_hot && s.tokens.len() + u.tokens.len() <= cap
        })
        .or_else(|| p.shards.iter().position(|s| s.tokens.is_empty()));
    let idx = match slot {
        Some(i) => i,
        None if p.shards.len() < cfg.max_connections => {
            p.shards.push(Shard {
                hot: is_hot,
                units: Vec::new(),
                tokens: Vec::new(),
            });
            p.shards.len() - 1
        }
        None => {
            p.dropped_tokens += u.tokens.len();
            p.dropped.push(u.key);
            return;
        }
    };
    let s = &mut p.shards[idx];
    if s.tokens.is_empty() {
        s.hot = is_hot;
    }
    s.units.push(u.key);
    s.tokens.extend(u.tokens);
}

pub fn plan(units: Vec<Unit>, cfg: &ShardsCfg) -> Plan {
    let mut p = Plan::default();
    let mut units = split_oversize(units, &mut p);
    sort_units(&mut units);
    let hot: HashSet<String> = units
        .iter()
        .filter(|u| !u.canary && u.rate > 0)
        .take(cfg.hot_events)
        .map(|u| u.key.clone())
        .collect();
    for u in units {
        let is_hot = hot.contains(&u.key);
        place(&mut p, u, is_hot, cfg);
    }
    p
}

/// Sticky re-plan for a universe refresh (S3a): without `retier`, every unit stays on its
/// previous connection while it still fits there, so only shards whose own units changed get a
/// new token list (and a reconnect). A shard emptied by departures stays as an idle cold slot
/// so later shards keep their index. New, grown and previously dropped units are placed cold
/// (tiers only change on a retier). `retier` (or no previous plan) is a fresh `plan`.
pub fn replan(prev: &Plan, units: Vec<Unit>, cfg: &ShardsCfg, retier: bool) -> Plan {
    if retier || prev.shards.is_empty() {
        return plan(units, cfg);
    }
    let mut p = Plan::default();
    let mut rest: HashMap<String, Unit> = split_oversize(units, &mut p)
        .into_iter()
        .map(|u| (u.key.clone(), u))
        .collect();
    for s in &prev.shards {
        let cap = if s.hot {
            cfg.hot_shard_tokens
        } else {
            cfg.cold_shard_tokens
        };
        let mut ns = Shard {
            hot: s.hot,
            units: Vec::new(),
            tokens: Vec::new(),
        };
        for key in &s.units {
            let fits = rest.get(key).is_some_and(|u| {
                ns.units.is_empty() || ns.tokens.len() + u.tokens.len() <= cap
            });
            if fits && let Some(u) = rest.remove(key) {
                ns.units.push(u.key);
                ns.tokens.extend(u.tokens);
            }
        }
        if ns.tokens.is_empty() {
            ns.hot = false; // an idle slot takes any unit
        }
        p.shards.push(ns);
    }
    let mut left: Vec<Unit> = rest.into_values().collect();
    sort_units(&mut left);
    for u in left {
        place(&mut p, u, false, cfg);
    }
    while p.shards.last().is_some_and(|s| s.tokens.is_empty()) {
        p.shards.pop();
    }
    p
}
```
Note: `if fits && let Some(u) = …` is a let-chain, which is already used in this codebase (edition 2024). If `ns.units.is_empty()` lets a unit that is bigger than the tier cap stay alone on its shard, that mirrors `plan`'s "a shard of its own" rule. `MAX_UNIT_TOKENS` still bounds such a unit.

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib shards`
Expected: PASS: the 6 new tests and the 3 existing ones (`never_splits_a_unit_and_respects_tier_caps` still gives its 14-token unit a shard of its own).

- [ ] **Step 5: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p scanner`
Expected: clean, PASS.
```bash
git add bins/scanner/src/shards.rs
git commit -m "scanner/shards: sticky replan between retiers; never plan a unit above 1000 tokens"
```

---

### Task 4: supervisor — idle slots, stop every changed slot before any start; wire sticky plans

**Files:**
- Modify: `bins/scanner/src/feed.rs` (`Slot` storage, `stop_slot`, `Supervisor::apply`, `abort_all`; new `SlotChanges`, `slot_changes`)
- Modify: `bins/scanner/src/run.rs` (start-up oversize report, refresher uses `replan`, `census` prints oversize; new `oversize_row`)
- Test: `mod tests` in `feed.rs` and in `run.rs`

**Interfaces:**
- Consumes: `shards::{Plan, Shard, replan, MAX_UNIT_TOKENS}`, `Plan::{live_shards, new_oversize, oversize}` (Task 3). The `'unit_oversize'` integrity kind (Task 1).
- Produces:
  - `feed::SlotChanges { pub stop: Vec<usize>, pub start: Vec<usize> }` (derives `Debug, Default, PartialEq, Eq`).
  - `feed::slot_changes(running: &[Option<(&[String], bool)>], plan: &[Shard]) -> SlotChanges`. `running[i]` is `Some((tokens, finished))` for a slot with a task and `None` for an idle slot.
  - `run::oversize_row(run_id: i64, ts_ms: i64, key: &str, tokens: usize) -> IntegrityRow`.

**Why.** Today `Supervisor::apply` works one slot at a time: it stops slot *i* and spawns its replacement, then moves on to *i+1*. When a refresh moves a unit from shard 7 to shard 3, the new shard 3 connects and subscribes while the old shard 7 is still connected with the same tokens, so one unit is live on two connections. At a mass change, new connections also open while old ones are still draining. After this change, `apply` first computes the whole change set, then stops every changed slot **concurrently** (a `JoinSet`, each stop bounded at 5 s as today), waits for all of them, and only then starts replacements. With `replan` (Task 3), an unchanged refresh changes nothing, and an idle slot holds no connection.

- [ ] **Step 1: Write the failing tests.** In `feed.rs` `mod tests`, add the following. `Shard` reaches the tests through `use super::*` once Step 3 changes the module import to `use crate::shards::{Plan, Shard};`.
```rust
    #[test]
    fn slot_changes_list_every_stop_and_start_for_a_plan() {
        let a = vec!["a".to_string()];
        let b = vec!["b".to_string()];
        let shard = |t: &[String]| Shard {
            hot: false,
            units: vec![],
            tokens: t.to_vec(),
        };
        let running: Vec<Option<(&[String], bool)>> = vec![
            Some((a.as_slice(), false)), // 0: unchanged → untouched
            Some((a.as_slice(), false)), // 1: tokens changed → stop + start
            Some((b.as_slice(), true)),  // 2: finished (parked) → retried
            None,                        // 3: idle → starts
            Some((b.as_slice(), false)), // 4: becomes an idle slot → stop only
            Some((a.as_slice(), false)), // 5: released (plan is shorter) → stop only
        ];
        let plan = vec![shard(&a), shard(&b), shard(&b), shard(&a), shard(&[])];
        let c = slot_changes(&running, &plan);
        assert_eq!(c.stop, vec![1, 2, 4, 5]);
        assert_eq!(c.start, vec![1, 2, 3]);
        assert_eq!(
            slot_changes(&running[..1], &plan[..1]),
            SlotChanges::default(),
            "a no-op refresh touches nothing"
        );
    }
```
In `run.rs` `mod tests`, add:
```rust
    #[test]
    fn an_oversize_unit_becomes_an_integrity_row_naming_it() {
        let r = oversize_row(3, 9, "e1", 1_234);
        assert_eq!(
            (r.run_id, r.ts_ms, r.kind.as_str(), r.event_id.as_deref()),
            (3, 9, "unit_oversize", Some("e1"))
        );
        assert!(r.detail.as_deref().is_some_and(|d| d.contains("1234 tokens")));
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p scanner --lib feed::tests run::tests`
Expected: compile errors, "cannot find function `slot_changes`", "cannot find function `oversize_row`".

- [ ] **Step 3: Implement in `feed.rs`.** Change the import to `use crate::shards::{Plan, Shard};` and add `use tokio::task::JoinSet;`. Replace everything from `type Slot = …` through the end of `impl Supervisor` (keep `spawn_shard` and everything above it) with:
```rust
type Slot = (
    Vec<String>,
    JoinHandle<()>,
    watch::Sender<bool>,
    Arc<AtomicBool>,
);

/// The slot indices `Supervisor::apply` must stop and must (re)start to go from `running` to
/// `plan`. `running[i]` is `Some((tokens, finished))` for a slot with a task, `None` for an idle
/// one; a plan shard with no tokens is an idle slot (no connection).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SlotChanges {
    pub stop: Vec<usize>,
    pub start: Vec<usize>,
}

pub fn slot_changes(running: &[Option<(&[String], bool)>], plan: &[Shard]) -> SlotChanges {
    let mut c = SlotChanges::default();
    for i in 0..running.len().max(plan.len()) {
        let old = running.get(i).copied().flatten();
        let new = plan
            .get(i)
            .map(|s| s.tokens.as_slice())
            .filter(|t| !t.is_empty());
        match (old, new) {
            (None, None) => {}
            (Some(_), None) => c.stop.push(i),
            (None, Some(_)) => c.start.push(i),
            (Some((t, finished)), Some(n)) => {
                if needs_restart(Some(t), finished, n) {
                    c.stop.push(i);
                    c.start.push(i);
                }
            }
        }
    }
    c
}

/// Ask one shard to stop, wait (bounded) for it to drain and send its own `Down`, and only then
/// return. Falls back to `abort()` only if the shard doesn't end within the timeout: aborting
/// cancels the shard's connection and its inline forwarder together (nothing survives it to
/// keep sending frames), but the shard never got to send its own `Down`, so once the abort has
/// actually completed (`handle.await`) this sends that `Down` itself — only in that fallback.
/// A free function so `Supervisor::apply` can stop many slots concurrently.
async fn stop_slot(conn: usize, slot: Slot, shared: mpsc::Sender<FeedMsg>, metrics: Arc<Metrics>) {
    let (_, mut handle, tx, parked) = slot;
    let _ = tx.send(true);
    if tokio::time::timeout(Duration::from_secs(5), &mut handle)
        .await
        .is_err()
    {
        eprintln!("feed: shard {conn} did not stop within 5s; aborting");
        handle.abort();
        let _ = handle.await; // wait until the cancellation has actually happened
        let _ = shared
            .send(FeedMsg::Down {
                conn,
                reason: "aborted after stop timeout".into(),
            })
            .await;
    }
    if parked.load(Ordering::Relaxed) {
        let _ = metrics
            .parked
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
                Some(p.saturating_sub(1))
            });
    }
}

pub struct Supervisor {
    /// One entry per plan shard index; `None` = idle slot (no task, no connection).
    handles: Vec<Option<Slot>>,
    shared: mpsc::Sender<FeedMsg>,
    budget: Arc<Mutex<ConnectBudget>>,
    cfg: ConnectCfg,
    capacity: usize,
    metrics: Arc<Metrics>,
}

impl Supervisor {
    pub fn new(
        shared: mpsc::Sender<FeedMsg>,
        budget: Arc<Mutex<ConnectBudget>>,
        cfg: ConnectCfg,
        capacity: usize,
        metrics: Arc<Metrics>,
    ) -> Supervisor {
        Supervisor {
            handles: Vec::new(),
            shared,
            budget,
            cfg,
            capacity,
            metrics,
        }
    }

    pub async fn apply(&mut self, plan: &Plan) {
        let running: Vec<Option<(&[String], bool)>> = self
            .handles
            .iter()
            .map(|s| s.as_ref().map(|(t, h, _, _)| (t.as_slice(), h.is_finished())))
            .collect();
        let changes = slot_changes(&running, &plan.shards);
        // 1. Stop EVERY changed slot, concurrently, and wait for all of them before anything new
        //    connects: live connections never exceed the budget, and a unit that moved between
        //    shards is never subscribed on two connections at once.
        let mut stopping = JoinSet::new();
        for &i in &changes.stop {
            if let Some(slot) = self.handles.get_mut(i).and_then(Option::take) {
                stopping.spawn(stop_slot(
                    i,
                    slot,
                    self.shared.clone(),
                    self.metrics.clone(),
                ));
            }
        }
        while stopping.join_next().await.is_some() {}
        // 2. Only now start the replacements (released trailing slots are simply dropped).
        self.handles.resize_with(plan.shards.len(), || None);
        for &i in &changes.start {
            let (Some(s), Some(entry)) = (plan.shards.get(i), self.handles.get_mut(i)) else {
                continue;
            };
            let (tx, stop) = watch::channel(false);
            let parked = Arc::new(AtomicBool::new(false));
            let task = spawn_shard(
                i,
                s.tokens.clone(),
                self.shared.clone(),
                self.budget.clone(),
                self.cfg.clone(),
                self.capacity,
                self.metrics.clone(),
                stop,
                parked.clone(),
            );
            *entry = Some((s.tokens.clone(), task, tx, parked));
        }
        self.metrics
            .shards
            .store(plan.live_shards() as u64, Ordering::Relaxed);
        self.metrics
            .tokens
            .store(plan.tokens() as u64, Ordering::Relaxed);
        self.metrics.dropped_groups.store(
            (plan.dropped.len() + plan.oversize.len()) as u64,
            Ordering::Relaxed,
        );
    }

    /// Signal every shard to stop and detach: the tasks drain and send their own `Down` (or exit
    /// silently if never connected) without this call waiting for them. Non-async by design —
    /// dropping the `Supervisor` itself (e.g. on process shutdown) has the same effect, since
    /// dropping `self.handles` drops every `watch::Sender`, closing each shard's stop channel.
    pub fn abort_all(&mut self) {
        for (_, _, tx, _) in self.handles.drain(..).flatten() {
            let _ = tx.send(true);
        }
    }
}
```

- [ ] **Step 4: Implement in `run.rs`.** Add the helper above `fn new_run`:
```rust
/// Integrity row for a unit above the Phase 0 per-connection limit (never subscribed).
pub fn oversize_row(run_id: i64, ts_ms: i64, key: &str, tokens: usize) -> IntegrityRow {
    IntegrityRow {
        run_id,
        ts_ms,
        kind: "unit_oversize".into(),
        shard: None,
        token: None,
        event_id: Some(key.to_string()),
        detail: Some(format!(
            "{tokens} tokens > {} per connection (Phase 0); not subscribed",
            shards::MAX_UNIT_TOKENS
        )),
    }
}
```
In `run_live`, directly after the `let writer_task = tokio::spawn(…);` statement, add:
```rust
    for (key, count) in plan.new_oversize(&Plan::default()) {
        writer_tx
            .send(WriterMsg::Integrity(oversize_row(run_id, now_ms(), &key, count)))
            .await
            .context("writer gone")?;
    }
```
Replace the whole `let refresher_task = { … };` block with:
```rust
    let refresher_task = {
        // refresher (spec §7: every 5 min, and on new_market / market_resolved)
        let (cfg, store, cmd_tx, plan_tx, u_tx, refresh, writer_tx) = (
            cfg.clone(),
            store.clone(),
            cmd_tx.clone(),
            plan_tx.clone(),
            u_tx.clone(),
            refresh.clone(),
            writer_tx.clone(),
        );
        // Sticky plans (S3a): a refresh keeps every unit on its connection; only a retier (every
        // `retier_every_refreshes`) re-plans from scratch.
        let mut prev = plan.clone();
        tokio::spawn(async move {
            let mut rates: HashMap<String, u64> = HashMap::new();
            let mut n: u32 = 0;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(cfg.universe.refresh_secs)) => {}
                    _ = refresh.notified() => {}
                }
                n = n.wrapping_add(1);
                let u = match fetch_universe(&cfg).await {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("refresh: {e:#}");
                        continue;
                    }
                };
                let retier = n.is_multiple_of(cfg.shards.retier_every_refreshes);
                if retier {
                    let (tx, rx) = oneshot::channel();
                    if cmd_tx.send(EngineCmd::Rates(tx)).await.is_err() {
                        return;
                    }
                    rates = rx.await.unwrap_or_default();
                }
                let plan = shards::replan(&prev, shards::units(&u, &rates), &cfg.shards, retier);
                for (key, count) in plan.new_oversize(&prev) {
                    let row = oversize_row(run_id, now_ms(), &key, count);
                    if writer_tx.send(WriterMsg::Integrity(row)).await.is_err() {
                        return;
                    }
                }
                prev = plan.clone();
                if let Err(e) = persist_universe(&store, &u).await {
                    eprintln!("refresh persist: {e:#}");
                }
                let u = Arc::new(u);
                let (ack_tx, ack_rx) = oneshot::channel();
                if cmd_tx
                    .send(EngineCmd::Apply {
                        universe: u.clone(),
                        conn_of: plan.conn_of(),
                        ack: ack_tx,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                let _ = ack_rx.await; // mapping applied BEFORE any new shard can deliver frames
                let _ = u_tx.send(u);
                if plan_tx.send(plan).await.is_err() {
                    return;
                }
            }
        })
    };
```
In `census`, change the plan line to count live shards, and add an oversize line after the "dropped" line:
```rust
    println!(
        "shard plan: {} shards ({} hot), {} tokens, max {} tokens/shard",
        plan.live_shards(),
        plan.shards.iter().filter(|s| s.hot).count(),
        plan.tokens(),
        plan.shards
            .iter()
            .map(|s| s.tokens.len())
            .max()
            .unwrap_or(0)
    );
```
```rust
    println!(
        "oversize units (> {} tokens, never subscribed): {:?}",
        shards::MAX_UNIT_TOKENS,
        plan.oversize
    );
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p scanner`
Expected: PASS (`slot_changes_list_every_stop_and_start_for_a_plan`, `an_oversize_unit_becomes_an_integrity_row_naming_it`, and the existing `restart_rules` etc.).

- [ ] **Step 6: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/feed.rs bins/scanner/src/run.rs
git commit -m "scanner/feed: stop every changed shard before starting any; sticky plans on refresh; report oversize units"
```

---

### Task 5: REST budget classes (reserved resnapshot share) and a re-check in-flight cap

**Files:**
- Modify: `bins/scanner/src/checks.rs` (`Limiter`, `fetch_books`, `run_rechecks`, `run_resnapshots`, `run_samplers`; new `RestClass`, `RestBudget`, `recheck_offsets`, `capacity_skips`, `CAPACITY_SKIP`)
- Modify: `bins/scanner/src/config.rs` (`RecheckCfg`, `validate`, tests), `config/scanner.toml`, `bins/scanner/src/run.rs` (limiter + re-check spawn)
- Test: `mod tests` in `checks.rs` and `config.rs`

**Interfaces:**
- Consumes: `RecheckReq` (engine), `ObservationRow` (store).
- Produces:
  - `checks::RestClass { Integrity, Shared }` (`Copy, PartialEq, Eq, Debug`).
  - `checks::RestBudget::new(per_sec: usize, reserved: usize) -> RestBudget`, `try_take(&mut self, class: RestClass, now_ms: i64) -> bool`.
  - `checks::Limiter::new(per_sec: usize, reserved: usize)`, `Limiter::wait(&self, class: RestClass)`.
  - `checks::fetch_books(clob: &ClobClient, limiter: &Limiter, class: RestClass, tokens: &[String]) -> Result<HashMap<String, Book>, String>`.
  - `checks::recheck_offsets(offsets: &[i64], delay_ms: i64) -> Vec<i64>`.
  - `checks::CAPACITY_SKIP: &str = "recheck capacity"`, `checks::capacity_skips(req: &RecheckReq, offsets: &[i64]) -> Vec<ObservationRow>`.
  - `checks::run_rechecks(rx, universe, clob, limiter, offsets: Vec<i64>, max_in_flight: usize, writer)`.
  - Config: `RecheckCfg.max_in_flight: usize` (≥ 1) and `RecheckCfg.resnapshot_reserved_per_sec: usize` (1..rest_per_sec).

**Why.** All REST work shares one 10/s limiter. Each episode spawns a detached task that makes 3–4 REST requests (offsets 1 s, 2 s, 5 s, + Δd), and a refresh or a reconnect can open hundreds of episodes at once. That fan-out saturates the limiter for minutes. A resnapshot (the repair for a book we *know* is wrong) then queues behind it while the engine keeps evaluating the broken book. After this change:
- Resnapshots use `Integrity`, which may use the whole budget.
- Everything else uses `Shared`, which may use at most `per_sec − reserved` requests in any second.
- At most `max_in_flight` episodes are re-checked at once. The rest get explicit `skipped` observation rows (reason `recheck capacity`), so "not measured" is recorded, never implied.
- Re-check tasks live in a `JoinSet` owned by the re-checker, so aborting it at shutdown cancels them all, and none of them keeps a `writer_tx` clone alive.

- [ ] **Step 1: Write the failing tests.** In `checks.rs` `mod tests` (it already has `req()`), add:
```rust
    #[test]
    fn shared_rest_work_never_takes_the_reserved_share() {
        let mut b = RestBudget::new(10, 3);
        for _ in 0..7 {
            assert!(b.try_take(RestClass::Shared, 0));
        }
        assert!(!b.try_take(RestClass::Shared, 10), "shared is capped at 7/s");
        for _ in 0..3 {
            assert!(b.try_take(RestClass::Integrity, 20));
        }
        assert!(!b.try_take(RestClass::Integrity, 30), "10/s in total");
        assert!(b.try_take(RestClass::Shared, 1_000), "the window moved on");
    }

    #[test]
    fn integrity_may_use_the_whole_budget_when_shared_is_idle() {
        let mut b = RestBudget::new(10, 3);
        for _ in 0..10 {
            assert!(b.try_take(RestClass::Integrity, 0));
        }
        assert!(!b.try_take(RestClass::Integrity, 0));
        assert!(!b.try_take(RestClass::Shared, 0));
        // a reserve as large as the budget is clamped: shared keeps at least 1/s
        let mut c = RestBudget::new(2, 5);
        assert!(c.try_take(RestClass::Shared, 0));
        assert!(!c.try_take(RestClass::Shared, 0));
        assert!(c.try_take(RestClass::Integrity, 0));
    }

    #[test]
    fn recheck_offsets_add_the_delay_once_and_sort() {
        assert_eq!(recheck_offsets(&[1000, 2000, 5000], 0), vec![1000, 2000, 5000]);
        assert_eq!(
            recheck_offsets(&[5000, 1000, 2000], 3000),
            vec![1000, 2000, 3000, 5000]
        );
        assert_eq!(recheck_offsets(&[1000, 2000, 5000], 2000), vec![1000, 2000, 5000]);
    }

    #[test]
    fn an_episode_over_the_recheck_cap_gets_explicit_skipped_rows() {
        let rows = capacity_skips(&req(), &[1000, 2000, 5000]);
        assert_eq!(
            rows.iter().map(|r| r.offset_ms).collect::<Vec<_>>(),
            vec![1000, 2000, 5000]
        );
        assert!(rows.iter().all(|r| r.episode_id == "ep"
            && r.skipped
            && r.skip_reason.as_deref() == Some(CAPACITY_SKIP)
            && r.observed_ms.is_none()
            && r.pnl.is_none()
            && r.still_profitable.is_none()));
    }
```
In `config.rs` `mod tests`, extend `repo_config_parses_with_a_stable_hash` with:
```rust
        assert_eq!(
            (cfg.recheck.max_in_flight, cfg.recheck.resnapshot_reserved_per_sec),
            (32, 3)
        );
```
and `rejects_unknown_fields_and_phase0_violations` with:
```rust
        let all_reserved = REPO.replace(
            "resnapshot_reserved_per_sec = 3",
            "resnapshot_reserved_per_sec = 10",
        );
        assert!(Config::parse(&all_reserved).is_err(), "re-checks need a share too");
        let no_cap = REPO.replace("max_in_flight = 32", "max_in_flight = 0");
        assert!(Config::parse(&no_cap).is_err());
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p scanner --lib checks::tests config::tests`
Expected: compile errors, "cannot find type `RestBudget`", "no field `max_in_flight` on type `RecheckCfg`".

- [ ] **Step 3: Config.** In `config.rs`, extend `RecheckCfg`:
```rust
pub struct RecheckCfg {
    pub offsets_ms: Vec<i64>,
    pub rest_per_sec: usize,
    pub drift_tokens_per_min: usize,
    pub l1_legs_per_min: usize,
    /// Episodes re-checked at once; more are recorded as skipped (S3a).
    pub max_in_flight: usize,
    /// REST requests/s only resnapshots may use (S3a): re-checks can't starve a book repair.
    pub resnapshot_reserved_per_sec: usize,
}
```
In `validate`, after the `rest_per_sec` check:
```rust
        anyhow::ensure!(
            self.recheck.max_in_flight >= 1,
            "recheck.max_in_flight must be ≥ 1"
        );
        anyhow::ensure!(
            self.recheck.resnapshot_reserved_per_sec >= 1
                && self.recheck.resnapshot_reserved_per_sec < self.recheck.rest_per_sec,
            "recheck.resnapshot_reserved_per_sec must be 1..rest_per_sec (re-checks need a share too)"
        );
```
In `config/scanner.toml`, add under `[recheck]` after `l1_legs_per_min = 50`:
```toml
max_in_flight = 32              # S3a: ~4 REST calls per episode at 7/s shared ≈ 18 s of work queued at most
resnapshot_reserved_per_sec = 3 # S3a: of rest_per_sec, only resnapshots (known-bad books) may use these
```

- [ ] **Step 4: Implement in `checks.rs`.** Replace the module doc's first line with `//! REST checks (spec §7 integrity, §8.2 re-checker), all through ONE two-class rate limiter:`. Change the imports: drop `use pm_api::backoff::ConnectBudget;`, use `use std::collections::{HashMap, VecDeque};`, and add `use tokio::task::JoinSet;`. Replace `Limiter` and `fetch_books` with:
```rust
/// Which REST work a request is for. A resnapshot repairs a book the engine KNOWS is wrong
/// (integrity); everything else (re-checks, samplers, /ok) is shared and may never use the
/// reserved share, so a burst of re-checks can't starve a resnapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestClass {
    Integrity,
    Shared,
}

/// Sliding 1 s REST budget: at most `per_sec` requests in any second, of which at most
/// `per_sec − reserved` may be `Shared`. Pure: time is passed in.
#[derive(Debug, Clone)]
pub struct RestBudget {
    per_sec: usize,
    reserved: usize,
    recent: VecDeque<(i64, RestClass)>,
}

impl RestBudget {
    /// `reserved` is clamped to `per_sec − 1`: shared work always keeps at least 1/s.
    pub fn new(per_sec: usize, reserved: usize) -> RestBudget {
        let per_sec = per_sec.max(1);
        RestBudget {
            per_sec,
            reserved: reserved.min(per_sec - 1),
            recent: VecDeque::new(),
        }
    }

    pub fn try_take(&mut self, class: RestClass, now_ms: i64) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|&(t, _)| now_ms - t >= 1_000)
        {
            self.recent.pop_front();
        }
        let shared = self
            .recent
            .iter()
            .filter(|(_, c)| *c == RestClass::Shared)
            .count();
        let ok = self.recent.len() < self.per_sec
            && (class == RestClass::Integrity || shared < self.per_sec - self.reserved);
        if ok {
            self.recent.push_back((now_ms, class));
        }
        ok
    }
}

pub struct Limiter(Mutex<RestBudget>);

impl Limiter {
    pub fn new(per_sec: usize, reserved: usize) -> Limiter {
        Limiter(Mutex::new(RestBudget::new(per_sec, reserved)))
    }

    pub async fn wait(&self, class: RestClass) {
        loop {
            let ok = self
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .try_take(class, now_ms());
            if ok {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
```
```rust
pub async fn fetch_books(
    clob: &ClobClient,
    limiter: &Limiter,
    class: RestClass,
    tokens: &[String],
) -> Result<HashMap<String, Book>, String> {
    let mut out = HashMap::new();
    for chunk in tokens.chunks(pm_api::clob::MAX_TOKENS_PER_REQUEST) {
        limiter.wait(class).await;
        let wires = clob.books(chunk).await.map_err(|e| e.to_string())?;
        for w in wires {
            let b = w.to_book().map_err(|e| e.to_string())?;
            out.insert(w.asset_id.clone(), b);
        }
    }
    Ok(out)
}
```
Add after `observe`:
```rust
/// Skip reason for an episode the re-checker had no capacity for.
pub const CAPACITY_SKIP: &str = "recheck capacity";

/// The re-check offsets for one episode: the configured ones plus +Δd (max secondsDelay over the
/// group's legs) when > 0 (spec §8.2), sorted and deduplicated.
pub fn recheck_offsets(offsets: &[i64], delay_ms: i64) -> Vec<i64> {
    let mut offs = offsets.to_vec();
    if delay_ms > 0 {
        offs.push(delay_ms);
    }
    offs.sort_unstable();
    offs.dedup();
    offs
}

/// One skipped observation row per offset for an episode over the in-flight cap: "not
/// measured" is explicit in `observations` (spec §10), never a silent gap.
pub fn capacity_skips(req: &RecheckReq, offsets: &[i64]) -> Vec<ObservationRow> {
    offsets
        .iter()
        .map(|&off| ObservationRow {
            episode_id: req.episode_id.clone(),
            offset_ms: i32::try_from(off).unwrap_or(i32::MAX),
            source: "rest".into(),
            observed_ms: None,
            skipped: true,
            skip_reason: Some(CAPACITY_SKIP.into()),
            edge: None,
            q_max: None,
            pnl: None,
            still_profitable: None,
            fill_at_original_size: None,
        })
        .collect()
}
```
Replace `run_rechecks` with:
```rust
pub async fn run_rechecks(
    mut rx: mpsc::Receiver<RecheckReq>,
    universe: watch::Receiver<Arc<Universe>>,
    clob: Arc<ClobClient>,
    limiter: Arc<Limiter>,
    offsets: Vec<i64>,
    max_in_flight: usize,
    writer: mpsc::Sender<WriterMsg>,
) {
    // Owned here, not detached: aborting this task (shutdown) cancels every re-check with it.
    let mut in_flight: JoinSet<()> = JoinSet::new();
    while let Some(req) = rx.recv().await {
        while in_flight.try_join_next().is_some() {}
        let received = now_ms();
        let delay_ms = universe
            .borrow()
            .group_by_event(&req.event_id)
            .and_then(|m| m.legs.iter().filter_map(|l| l.seconds_delay).max())
            .map(|s| i64::from(s) * 1000)
            .unwrap_or(0);
        let offs = recheck_offsets(&offsets, delay_ms);
        if in_flight.len() >= max_in_flight {
            for row in capacity_skips(&req, &offs) {
                if writer.send(WriterMsg::Observation(row)).await.is_err() {
                    return;
                }
            }
            continue;
        }
        let (universe, clob, limiter, writer) = (
            universe.clone(),
            clob.clone(),
            limiter.clone(),
            writer.clone(),
        );
        in_flight.spawn(async move {
            for off in offs {
                let wait = (received + off - now_ms()).max(0);
                tokio::time::sleep(Duration::from_millis(u64::try_from(wait).unwrap_or(0))).await;
                let meta = universe.borrow().group_by_event(&req.event_id).cloned();
                let books = match &meta {
                    Some(m) => {
                        fetch_books(&clob, &limiter, RestClass::Shared, &m.live_yes_tokens()).await
                    }
                    None => Ok(HashMap::new()),
                };
                let row = observe(&req, off, meta.as_ref(), books);
                if writer.send(WriterMsg::Observation(row)).await.is_err() {
                    return;
                }
            }
        });
    }
}
```
In `run_resnapshots`, call `fetch_books(&clob, &limiter, RestClass::Integrity, std::slice::from_ref(&token))`. In `run_samplers`:
- change `limiter.wait().await;` to `limiter.wait(RestClass::Shared).await;`;
- change both `fetch_books(&clob, &limiter, &pick)` and `fetch_books(&clob, &limiter, &toks)` to pass `RestClass::Shared` as the third argument.

- [ ] **Step 5: Wire in `run.rs`.** Replace the limiter and re-check spawn in `run_live`:
```rust
    let limiter = Arc::new(Limiter::new(
        cfg.recheck.rest_per_sec,
        cfg.recheck.resnapshot_reserved_per_sec,
    ));
    let rechecks_task = tokio::spawn(checks::run_rechecks(
        recheck_rx,
        u_rx.clone(),
        clob.clone(),
        limiter.clone(),
        cfg.recheck.offsets_ms.clone(),
        cfg.recheck.max_in_flight,
        writer_tx.clone(),
    ));
```

- [ ] **Step 6: Run the tests to verify they pass.**
Run: `cargo test -p scanner`
Expected: PASS (4 new tests in `checks`, and the extended config tests).

- [ ] **Step 7: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/checks.rs bins/scanner/src/config.rs config/scanner.toml bins/scanner/src/run.rs
git commit -m "scanner/checks: reserve REST share for resnapshots; cap in-flight rechecks with explicit skips"
```

---

### Task 6: writer — O(1) cap eviction with `VecDeque`

**Files:**
- Modify: `bins/scanner/src/writer.rs` (`Pending` and its `impl`)
- Test: `mod tests` in `writer.rs`

**Interfaces:**
- Consumes: `store::Batch` (fields `episodes`, `observations`, `integrity`, `health`, all `Vec`).
- Produces: unchanged public API: `Pending::new(max_rows)`, `push`, `rows`, `take() -> Batch`, `restore(Batch)`, `pub dropped: u64`.

`enforce_cap` evicts with `Vec::remove(0)`, which shifts every remaining row. During a Postgres outage the backlog sits at `max_pending_rows = 200_000`, and every new row then costs a 200,000-row shift. After this change the queues pop from the front in O(1).

- [ ] **Step 1: Write the failing test.** Add to `mod tests` in `writer.rs`:
```rust
    #[test]
    fn evicting_a_large_backlog_is_fast_and_keeps_the_newest_rows() {
        // 60,000 evictions from a 30,000-row queue: Vec::remove(0) shifted ~1.8e9 rows here.
        let started = std::time::Instant::now();
        let mut p = Pending::new(30_000);
        for t in 0..90_000 {
            p.push(integ(t));
        }
        assert_eq!((p.rows(), p.dropped), (30_000, 60_000));
        let b = p.take();
        assert_eq!(b.integrity.first().map(|i| i.ts_ms), Some(60_000));
        assert_eq!(b.integrity.last().map(|i| i.ts_ms), Some(89_999));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "eviction must be O(1) per row: {:?}",
            started.elapsed()
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails.**
Run: `cargo test -p scanner --lib writer::tests::evicting_a_large_backlog`
Expected: FAIL on the elapsed assertion ("eviction must be O(1) per row"). In a debug build the O(n²) shifting takes well over 5 s.

- [ ] **Step 3: Implement.** In `writer.rs`, change `use std::collections::HashMap;` to `use std::collections::{HashMap, VecDeque};`. Replace `struct Pending` and its whole `impl` with:
```rust
/// Rows waiting for the next flush. Episodes are deduplicated by id (latest version wins);
/// the other rows are FIFO queues, so the cap evicts the oldest in O(1).
#[derive(Debug, Default)]
pub struct Pending {
    episodes: Vec<EpisodeWrite>,
    observations: VecDeque<ObservationRow>,
    integrity: VecDeque<IntegrityRow>,
    health: VecDeque<HealthRow>,
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
        self.episodes.len() + self.observations.len() + self.integrity.len() + self.health.len()
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
            WriterMsg::Flush(_) => {}
        }
        self.enforce_cap();
    }

    /// Over the cap, drop the oldest health, then integrity, then observation rows (episodes
    /// never), counting each.
    fn enforce_cap(&mut self) {
        while self.rows() > self.max_rows {
            let evicted = self.health.pop_front().is_some()
                || self.integrity.pop_front().is_some()
                || self.observations.pop_front().is_some();
            if !evicted {
                break; // only episodes left: never dropped
            }
            self.dropped += 1;
        }
    }

    pub fn take(&mut self) -> Batch {
        self.ep_index.clear();
        Batch {
            episodes: std::mem::take(&mut self.episodes),
            observations: std::mem::take(&mut self.observations).into(),
            integrity: std::mem::take(&mut self.integrity).into(),
            health: std::mem::take(&mut self.health).into(),
        }
    }

    /// Put back a batch whose write failed. It is OLDER than anything pushed since, so newer
    /// episode versions already here win, and its other rows go first.
    pub fn restore(&mut self, older: Batch) {
        let newer = self.take();
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
        self.enforce_cap();
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib writer`
Expected: PASS: the new test in well under 5 s, plus `keeps_only_the_latest_version_of_an_episode`, `restore_after_a_failed_write_lets_newer_versions_win` and `over_the_cap_drops_non_episode_rows_first`.

- [ ] **Step 5: Lint and commit.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.
```bash
git add bins/scanner/src/writer.rs
git commit -m "scanner/writer: O(1) cap eviction with VecDeque"
```

---

### Task 7: one deadline for the whole graceful stop

**Files:**
- Modify: `bins/scanner/src/run.rs` (`run`'s `end_run`, the shutdown block at the end of `run_live`; new `SHUTDOWN_BUDGET`, `END_RUN_TIMEOUT`, `by_deadline`, `wait_thread`)
- Test: `mod tests` in `run.rs`

**Interfaces:**
- Consumes: `checks::run_rechecks` owns its tasks (Task 5), so aborting it releases their `writer_tx` clones.
- Produces:
  - `run::SHUTDOWN_BUDGET: Duration` (20 s) and `run::END_RUN_TIMEOUT: Duration` (5 s).
  - `run::by_deadline<F: Future>(deadline: tokio::time::Instant, step: &str, f: F) -> Option<F::Output>`.
  - `run::wait_thread<T>(h: &std::thread::JoinHandle<T>, deadline: tokio::time::Instant) -> bool`.

**Why.** Today each stop step has its own timeout, and some have none:
- routing the `close_all` rows (`writer.send` can block while the channel is full): none;
- the flush ack: 10 s;
- the writer join: 3 s;
- the capture thread join: unbounded;
- the `end_run` row: none.

Their sum can exceed compose's 30 s `stop_grace_period`, and then Docker SIGKILLs the process mid-write. After this change, one deadline (`SHUTDOWN_BUDGET` = 20 s from the signal) bounds every step in `run_live`, and `run` gives `end_run` at most `END_RUN_TIMEOUT` = 5 s, for 25 s in total. A step that overruns is logged and skipped. The capture thread is polled (`is_finished`) instead of joined, so a stuck disk can't hold the exit. A std thread doesn't keep the process alive once `main` returns.

- [ ] **Step 1: Write the failing tests.** Add to `mod tests` in `run.rs`:
```rust
    #[test]
    fn the_whole_stop_fits_the_compose_grace_period() {
        // compose.yml: stop_grace_period: 30s
        assert!(SHUTDOWN_BUDGET + END_RUN_TIMEOUT < Duration::from_secs(30));
    }

    #[tokio::test]
    async fn shutdown_steps_never_outlive_the_deadline() {
        let start = tokio::time::Instant::now();
        let deadline = start + Duration::from_millis(100);
        assert_eq!(by_deadline(deadline, "ready", async { 1 }).await, Some(1));
        assert_eq!(
            by_deadline(deadline, "stuck", std::future::pending::<u8>()).await,
            None
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        // past the deadline: ready work still completes, stuck work returns at once
        assert_eq!(by_deadline(deadline, "late ready", async { 2 }).await, Some(2));
        let t = tokio::time::Instant::now();
        assert_eq!(
            by_deadline(deadline, "late stuck", std::future::pending::<u8>()).await,
            None
        );
        assert!(t.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn waiting_for_a_busy_thread_stops_at_the_deadline() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let busy = std::thread::spawn(move || {
            let _ = rx.recv();
        });
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        assert!(!wait_thread(&busy, deadline).await, "still busy at the deadline");
        drop(tx); // lets the thread finish
        let later = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(wait_thread(&busy, later).await);
        let _ = busy.join();
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
Run: `cargo test -p scanner --lib run::tests`
Expected: compile errors, "cannot find value `SHUTDOWN_BUDGET`", "cannot find function `by_deadline`", "cannot find function `wait_thread`".

- [ ] **Step 3: Implement the helpers.** Add in `run.rs` below `warm_start_usable`:
```rust
/// Graceful-stop budget for `run_live` from the shutdown signal (S3a). With END_RUN_TIMEOUT for
/// the `runs` exit row, the whole stop fits compose's 30 s stop_grace_period.
pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(20);
pub const END_RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// Run one shutdown step, but never past `deadline` (a ready step still completes). An overrun
/// is logged and the step abandoned.
pub async fn by_deadline<F: std::future::Future>(
    deadline: tokio::time::Instant,
    step: &str,
    f: F,
) -> Option<F::Output> {
    match tokio::time::timeout_at(deadline, f).await {
        Ok(v) => Some(v),
        Err(_) => {
            eprintln!("shutdown: {step} overran the deadline; skipped");
            None
        }
    }
}

/// Wait for a std thread to finish without joining it, never past `deadline`. Returns whether it
/// finished (then `join` returns at once). An unfinished thread doesn't keep the process alive.
pub async fn wait_thread<T>(h: &std::thread::JoinHandle<T>, deadline: tokio::time::Instant) -> bool {
    while !h.is_finished() {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}
```

- [ ] **Step 4: Use them.** In `run`, replace `let _ = store.end_run(run_id, now_ms(), &reason).await;` with:
```rust
    let _ = tokio::time::timeout(END_RUN_TIMEOUT, store.end_run(run_id, now_ms(), &reason)).await;
```
In `run_live`, replace everything from the `// Shutdown (bounded well under Docker's stop grace):` comment through `let _ = tokio::task::spawn_blocking(move || capture_thread.join()).await;` (keep the final `result`) with:
```rust
    // Shutdown: ONE deadline for every step (SHUTDOWN_BUDGET), so the whole stop plus `run`'s
    // exit row fits compose's 30 s stop_grace_period. Order:
    // 1. stop taking new shards/universes; 2. close every open episode; 3. flush the writer;
    // 4. THEN stop everything still holding a `writer_tx` clone (the re-checker owns its tasks),
    //    so the writer's final join can return; 5. give the capture thread what time is left.
    let deadline = tokio::time::Instant::now() + SHUTDOWN_BUDGET;
    drop(plan_tx);
    supervisor_task.abort();
    refresher_task.abort();
    let t = now_ms();
    io.ctl(0, Ctl::Shutdown, t);
    let outs = engine.close_all(t);
    let _ = by_deadline(deadline, "close episodes", io.route(&mut engine, outs)).await;
    let (done_tx, done_rx) = oneshot::channel();
    let flush = async {
        if writer_tx.send(WriterMsg::Flush(done_tx)).await.is_ok() {
            let _ = done_rx.await;
        }
    };
    let _ = by_deadline(deadline, "writer flush", flush).await;
    health_task.abort();
    rechecks_task.abort();
    resnapshots_task.abort();
    samplers_task.abort();
    drop(io);
    drop(capture_tx);
    drop(writer_tx);
    let _ = by_deadline(deadline, "writer exit", writer_task).await;
    if wait_thread(&capture_thread, deadline).await {
        let _ = capture_thread.join();
    } else {
        eprintln!("shutdown: capture thread still busy at the deadline; exiting without it");
    }
```

- [ ] **Step 5: Run the tests to verify they pass.**
Run: `cargo test -p scanner --lib run::tests`
Expected: PASS (the 3 new tests plus `breaker_trips_at_max_starts`, `warm_start_only_when_fresh` and `an_oversize_unit_becomes_an_integrity_row_naming_it`).

- [ ] **Step 6: Full verification.**
Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all PASS. Then run the store suite against Postgres:
Run: `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: PASS, with no `SKIPPED` in the output.

- [ ] **Step 7: Commit.**
```bash
git add bins/scanner/src/run.rs
git commit -m "scanner/run: one deadline bounds the whole graceful stop"
```

---

### Task 8: stream repair of phantom levels, integrity throttle, universe-save timeout (added from the VPS soak)

**Why (soak evidence, 2026-09-25, VPS run 1).** From 13:31 UTC, a single token (Bitcoin price on September 25?, market 4689155, shard 1) produced **20,240 `top_mismatch` rows in 35 min**, and every one of them requested a REST resnapshot. Each row has the same shape: our best bid is ABOVE the server's (`server bid 0.47 … ours bid 0.48`, and later `0.74` vs `0.76`), while the asks agree. That is a **phantom level**, a removal we never applied.

The REST repair can never land on such a busy market. By the time `/book` answers, websocket deltas have advanced our book's `server_ts` past the snapshot's, so `Engine::rest_snapshot` discards it as older (a correct rule). The loop never ends:
- the phantom creates fake NO-basket state;
- the group stays suspect;
- the resnapshot flood starves the 10/s REST budget for every other token.

There was also one `refresh persist: … canceling statement due to statement timeout`. `scanner_rw` has `statement_timeout = 5s` (`docker/postgres/init/01-roles.sh`), and the background universe upsert of about 89k legs (plus `mark_gone`'s full scan) hit it under CPU load.

**Files:**
- Modify: `crates/arb-core/src/book.rs` (add `Book::trim_to_top`), test `crates/arb-core/tests/book.rs`
- Modify: `bins/scanner/src/engine.rs` (`EngineCtx.issue_throttle_ms`, `Engine.issue_throttle`, `book_mut`, `throttle`, `check_book`)
- Modify: `bins/scanner/src/config.rs` (`EngineCfg.issue_throttle_ms`, validation ≥ 0), `config/scanner.toml`, `bins/scanner/src/run.rs` (`engine_ctx`), `bins/scanner/tests/common/mod.rs` (`ctx()` gets `issue_throttle_ms: 5_000`)
- Modify: `crates/store/src/universe.rs` (`upsert_universe`, `mark_gone`)
- Test: `bins/scanner/tests/engine.rs`

**Interfaces:**
- Produces:
  - `arb_core::book::Book::trim_to_top(&mut self, best_bid: Option<Price>, best_ask: Option<Price>) -> bool`;
  - `EngineCtx.issue_throttle_ms: i64`;
  - `EngineCfg.issue_throttle_ms: i64`, with `issue_throttle_ms = 5000` in `[engine]`.
- Consumes: `Engine::suspect(g, until, since, out)` (Task 2), `Engine::evaluate`.

**Rules.**
1. **The stream's top is authoritative (Polymarket sends the final best bid and ask with every `price_change` item).** At resolution, when our top disagrees, drop every bid of ours strictly above the server's best bid and every ask strictly below its best ask. A `None` side (empty `"0"`/`"1"`, or not reported) is not proof and is left alone.
   - If our top now equals the server's, the book is **repaired**: the row still records the mismatch (the book *was* wrong, so the group is still marked suspect), with `→ repaired from stream` appended to `detail`, and **no** resnapshot is sent. The group is then re-evaluated at `max(ts, group_last_ts)`, so a fake opportunity the phantom created closes now rather than at the group's next update.
   - Only a **missing** level (our bid below the server's, or our ask above it) still asks for a REST resnapshot.
2. **Throttle integrity rows**, per `(kind, token)` for `top_mismatch` and `crossed_book`: at most one row per `issue_throttle_ms` of event time (ts). Suppressed occurrences are counted, and the next emitted row appends ` (+N suppressed)` to its `detail` (for `crossed_book`, `detail` becomes `+N suppressed`). A `top_mismatch` row that is suppressed also suppresses its resnapshot. Suspect marking always happens. The engine stays pure (the window is ts arithmetic), so replay reproduces it.
3. `crossed_book` is evaluated **after** the trim, so a phantom bid that crossed the book and was repaired is not also reported as crossed.
4. **Universe persistence is background work.** `upsert_universe` and `mark_gone` start their transaction with `SET LOCAL statement_timeout = '60s'`. It applies only to that transaction; the scanner's role default stays 5 s for everything else.

- [ ] **Step 1: Write the failing tests.**

Append to `crates/arb-core/tests/book.rs` (reuse its existing price/share helpers if it has them; otherwise use the inline closures below):
```rust
#[test]
fn trim_to_top_drops_only_levels_the_servers_top_disproves() {
    let lv = |p: &str, s: &str| (arb_core::money::Price::parse(p).unwrap(), arb_core::money::Shares::parse(s).unwrap());
    let px = |p: &str| arb_core::money::Price::parse(p).unwrap();
    let mut b = arb_core::book::Book::default();
    b.apply_snapshot(&[lv("0.48", "5"), lv("0.47", "9")], &[lv("0.49", "3"), lv("0.50", "7")], 0);
    // server: best bid 0.47, best ask 0.50 → our 0.48 bid and 0.49 ask are phantoms
    assert!(b.trim_to_top(Some(px("0.47")), Some(px("0.50"))));
    let t = b.top();
    assert_eq!((t.best_bid.map(|x| x.0), t.best_ask.map(|x| x.0)), (Some(px("0.47")), Some(px("0.50"))));
    // nothing left to trim; None sides are never proof
    assert!(!b.trim_to_top(Some(px("0.47")), Some(px("0.50"))));
    assert!(!b.trim_to_top(None, None));
    // a missing level (server best bid 0.49 > ours) cannot be repaired by trimming
    assert!(!b.trim_to_top(Some(px("0.49")), None));
}
```

Append to `bins/scanner/tests/engine.rs`:
```rust
#[test]
fn a_phantom_level_is_trimmed_from_the_streams_top_without_a_resnapshot() {
    let mut e = engine();
    seed(&mut e); // e1-y0 ask 0.40
    // the server removed our 0.40 ask without us seeing it; its final top ask is now 0.45
    e.on_frame(0, 2_100, &[delta("e1-y0", 2_000, Side::Ask, "0.45", "5", None, Some("0.45"))]);
    let outs = e.on_frame(0, 3_200, &[ask("e1-y1", 3_001, "0.20", "10")]); // > 1 s later: resolves
    let rows: Vec<_> = outs
        .iter()
        .filter_map(|o| match o {
            Out::Integrity(i) if i.kind == "top_mismatch" => Some(i.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 1, "{outs:?}");
    assert!(rows[0].detail.as_deref().unwrap_or("").contains("repaired from stream"), "{rows:?}");
    assert!(!outs.iter().any(|o| matches!(o, Out::Resnapshot { .. })), "{outs:?}");
    assert_eq!(e.book_of("e1-y0").unwrap().top().best_ask.map(|(x, _)| x), Some(p("0.45")));
}

#[test]
fn a_persistent_mismatch_is_reported_at_most_once_per_throttle_window() {
    let mut e = engine(); // common::ctx(): issue_throttle_ms = 5_000
    seed(&mut e); // e1-y2 ask 0.30
    // the server keeps saying 0.29: a level we LACK, so only a resnapshot could fix it
    let mut details = Vec::new();
    let mut resnaps = 0;
    for k in 0..10_i64 {
        let t = 2_000 + k * 1_100; // each later delta resolves the previous pending check
        let outs = e.on_frame(
            0,
            t + 100,
            &[MarketEvent::Delta {
                token: "e1-y2".into(),
                server_ts_ms: t,
                side: Side::Ask,
                price: p("0.50"),
                size: sh("5"),
                best_bid: None,
                best_ask: Some(p("0.29")),
            }],
        );
        for o in &outs {
            match o {
                Out::Integrity(i) if i.kind == "top_mismatch" => details.push(i.detail.clone().unwrap_or_default()),
                Out::Resnapshot { token } if token == "e1-y2" => resnaps += 1,
                _ => {}
            }
        }
    }
    // resolutions at ts 2000, 3100, …, 10800: emitted at 2000 and 7500 only (window 5000)
    assert_eq!(details.len(), 2, "{details:?}");
    assert_eq!(resnaps, 2);
    assert!(details[1].ends_with("(+4 suppressed)"), "{details:?}");
    assert!(!details[0].contains("repaired"));
}
```

- [ ] **Step 2: Run to verify they fail.**
Run: `cargo test -p arb-core --test book trim_to_top && cargo test -p scanner --test engine -- phantom persistent_mismatch`
Expected: compile errors (`trim_to_top` missing; `issue_throttle_ms` missing from `EngineCtx` once `common::ctx()` is updated) or assertion failures (a resnapshot is emitted; 9 rows instead of 2).

- [ ] **Step 3: Implement.**

`crates/arb-core/src/book.rs`, inside `impl Book`:
```rust
    /// Drop levels the server's authoritative top proves are gone: bids strictly above
    /// `best_bid`, asks strictly below `best_ask` (a removal we never applied). `None` leaves that
    /// side alone: an empty or unreported side is not proof. Returns true if anything was removed.
    pub fn trim_to_top(&mut self, best_bid: Option<Price>, best_ask: Option<Price>) -> bool {
        let before = self.bids.len() + self.asks.len();
        if let Some(b) = best_bid {
            self.bids.retain(|p, _| *p <= b);
        }
        if let Some(a) = best_ask {
            self.asks.retain(|p, _| *p >= a);
        }
        self.bids.len() + self.asks.len() != before
    }
```

`bins/scanner/src/engine.rs`:
- `EngineCtx` gets `pub issue_throttle_ms: i64,` after `drift_settle_ms`.
- New private struct and field:
```rust
/// Per-(kind, token) integrity-row rate limit (S3a Task 8), in event time.
#[derive(Debug, Clone, Copy)]
struct IssueThrottle {
    last_ms: i64,
    suppressed: u32,
}
```
  Add `issue_throttle: HashMap<(&'static str, String), IssueThrottle>,` to `Engine`, initialized empty in `Engine::new`.
- New methods in `impl Engine`:
```rust
    fn book_mut(&mut self, token: &str) -> Option<&mut Book> {
        if let Some((g, l)) = self.token_ix.get(token).copied() {
            return self.groups.get_mut(g).and_then(|gr| gr.legs.get_mut(l)).map(|leg| &mut leg.book);
        }
        self.canary.get_mut(token)
    }

    /// Some(suppressed since the last emitted row) when a row may be emitted at `ts`; None while
    /// still inside `issue_throttle_ms` of the last one. Pure: time is the event's ts.
    fn throttle(&mut self, kind: &'static str, token: &str, ts: i64) -> Option<u32> {
        let window = self.ctx.issue_throttle_ms;
        let e = self
            .issue_throttle
            .entry((kind, token.to_string()))
            .or_insert(IssueThrottle { last_ms: i64::MIN, suppressed: 0 });
        if e.last_ms != i64::MIN && ts.saturating_sub(e.last_ms) < window {
            e.suppressed = e.suppressed.saturating_add(1);
            return None;
        }
        let n = e.suppressed;
        e.last_ms = ts;
        e.suppressed = 0;
        Some(n)
    }
```
- In `check_book`, keep the early returns, then restructure the body to this order. Compute `mismatch` from the untouched book as today. If it is `Some`:
  1. Trim, and decide whether the book is now repaired:
     ```rust
     let (sb, sa) = server.unwrap_or((None, None));
     let repaired = self.book_mut(token).is_some_and(|b| {
         b.trim_to_top(sb, sa);
         let t = b.top();
         (sb.is_none() || sb == t.best_bid.map(|(x, _)| x)) && (sa.is_none() || sa == t.best_ask.map(|(x, _)| x))
     });
     ```
  2. If `repaired`, append `" → repaired from stream"` to `detail`.
  3. `if let Some(n) = self.throttle("top_mismatch", token, ts)`: append `format!(" (+{n} suppressed)")` when `n > 0`, push the integrity row (same fields as today), and push `Out::Resnapshot` only when `!repaired`.
  4. Always: `if let Some(g) = group { self.suspect(g, ts + self.ctx.suspect_window_ms, ts, out); }`.
  5. If `repaired` and `group` is `Some(g)`: `let eval_ts = ts.max(self.group_last_ts.get(g).copied().unwrap_or(i64::MIN)); self.evaluate(g, eval_ts, eval_ts, out);`.

  THEN compute `crossed` from the (possibly trimmed) book: `self.book_of(token).is_some_and(|b| b.is_crossed())`. If crossed, then `if let Some(n) = self.throttle("crossed_book", token, ts)`: push the row with `detail: (n > 0).then(|| format!("+{n} suppressed"))`. Suspect marking stays unconditional, as today.

`bins/scanner/src/config.rs`: `EngineCfg` gets `pub issue_throttle_ms: i64,`; `validate` rejects a negative value (follow the existing `anyhow::ensure!` style). `config/scanner.toml` `[engine]` gets
```toml
issue_throttle_ms = 5000         # one top_mismatch/crossed_book row per token per 5 s (VPS soak 2026-09-25: 20k rows/35 min from one book)
```
`bins/scanner/src/run.rs` `engine_ctx`: add `issue_throttle_ms: cfg.engine.issue_throttle_ms,`. `bins/scanner/tests/common/mod.rs` `ctx()`: add `issue_throttle_ms: 5_000,`. Fix any other `EngineCtx { .. }` literal the compiler points at (tests), with the same value.

`crates/store/src/universe.rs`: in `upsert_universe` and `mark_gone`, right after `let mut tx = self.pool().begin().await?;`, add
```rust
        // Background work: the scanner role's 5 s default (01-roles.sh) is for the hot path. The
        // ~89k-leg upsert hit it under load on the VPS soak (2026-09-25). This transaction only.
        sqlx::query("SET LOCAL statement_timeout = '60s'").execute(&mut *tx).await?;
```

- [ ] **Step 4: Run to verify they pass, then the full suites.**
Run: `cargo test -p arb-core && cargo test -p scanner && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Then run the store suite against Postgres (it must not print SKIPPED): `set -a; . ./.env; set +a; export TEST_DATABASE_URL="postgres://ct_owner:${POSTGRES_PASSWORD}@127.0.0.1:55432/postgres" && cargo test -p store`
Expected: all pass. The existing tests `server_top_that_disagrees_with_ours_is_an_integrity_event_and_a_resnapshot` (a missing level: still resnapshots) and `a_real_top_mismatch_is_reported_once_a_later_frame_resolves_it` (still reported) must keep passing unchanged. If one of them fails, re-read Rule 1 before changing the test.

- [ ] **Step 5: Commit.**
```bash
git add crates/arb-core/src/book.rs crates/arb-core/tests/book.rs bins/scanner/src/engine.rs bins/scanner/src/config.rs config/scanner.toml bins/scanner/src/run.rs bins/scanner/tests/common/mod.rs bins/scanner/tests/engine.rs crates/store/src/universe.rs
git commit -m "scanner: repair phantom levels from the stream's top, throttle integrity rows; universe save gets 60 s"
```

---

## Self-review notes (for the reviewer)

- **Coverage of the 8 deferred items:**
  1. engine_suspect timing: Task 2.
  2. Sticky plan: Task 3, wired in Task 4.
  3. Re-check cap and reserved REST share: Task 5.
  4. Stop all changed slots before any start: Task 4.
  5. Shutdown deadline: Task 7.
  6. Writer `VecDeque`: Task 6.
  7. Oversize units dropped and reported: Task 3 (plan), Task 4 (integrity row), Task 1 (DB kind).
  8. Migration 0003 and its role test: Task 1.
- **Replay stays byte-identical.** The only new engine input from live is the `now_ms` argument of `mark_suspect`. The capture already has it: the `Ctl::Suspect` line's `recv_ts_us` is the same `t` live passes.
- **Types used across tasks:**
  - `Plan.oversize: Vec<(String, usize)>`, `Plan::live_shards`, `Plan::new_oversize`, `shards::replan`, `shards::MAX_UNIT_TOKENS` (Task 3), used by Task 4;
  - `RestClass`, `Limiter::new(per_sec, reserved)` and the `run_rechecks(…, max_in_flight, writer)` argument order (Task 5), matching the `run.rs` call in the same task;
  - `mark_suspect(event_id, until_ms, now_ms) -> Vec<Out>` (Task 2), called in `run.rs` and `replay.rs` in the same task.
