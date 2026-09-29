# isoca-radar Phase 7a: the latency answer

**Date:** 2026-09-29. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.6 execution, §11 policies). **Replaces:** §7 (explain panel) of `2026-09-27-phase7-playground-explain-design.md`; that spec's §1–6 become **Phase 7b** (the public `/math` playground), unchanged and not urgent. **Builds on:** 5a (Edge capture, `LatencyAxis`), 5d (Sweet spots).
**Status:** designed in conversation with the owner on 2026-09-29, decision by decision (§13). The owner asked for an independent subagent review instead of their own review before the plan.

## 1. Purpose, scope and success

The owner's question: **at which latency would this pay?** The dashboard gives pieces (Edge capture's 3–4 scenario points, a latency axis, a Sweet spots box that is usually empty) and never answers it.

**Motivation (diagnosed 2026-09-29, prod queries + a replay probe).** P0 colo-idle (52 ms) lost $524 of $1,000 in its first two days:
- The edge was there at `t_x`: 77 clean trades still had Π > 0 at execution (+$39.57). Conversions realized +$12.18.
- **Leg risk ate it:** unwind sells realized −$500.94. 90 of 106 leg-risk trades had one leg fill 0, so `q_eff = 0` and every other leg was sold at the bid.
- 77 % of the loss came in 2026-09-27 09–10 UTC, on crypto price brackets (BTC "Oct 3" 8 times in 6 minutes, about −$17 each; ETH, XRP, SOL) and Seattle temperatures. Feed lag was normal in those minutes (p50 ≈ 74 ms). This supports the parent spec's H0: the Σ ≠ 1 windows are the adverse-selection windows.
- Ruled out: `secondsDelay` (3 trades). Phantom books from the duplicated-token bug showed 0 duplicates in a 3-hour replay, but the burst hours were past raw retention, so this is unproven for them.

So a single P&L number per latency is not enough. The answer must say **why** at each latency: basket gains vs leg-risk cost.

**In scope:**
- A **latency ladder**: four new P0-converter paper accounts (§3).
- **Paper epochs and a reset button**, the api's first write route (§4, §5).
- Epoch-aware readers and cards (§6).
- The **hero drawing**, "at what latency would this pay?", with its read endpoint (§7).
- The **Sweet spots grid** replacing the per-scenario interval chart on `/sweet` (§8).
- The **explain panel**: the hero plus four questions (§9).
- `Position.end_date` (unchanged from the 09-27 spec §7.4).

**Out of scope:**
- `/math`, `arb-wasm`, parity CI, Traefik public router, flood test (Phase 7b).
- Fixing the duplicated-token engine bug (its own fix, already queued).
- Changing the unwind rule (dump the whole basket at the bid with no floor). The spec records it as a model choice that makes burst losses as large as possible (§12).
- Real orders.

**Exit:**
- On the VPS, all 19 paper accounts trade in epoch ≥ 1.
- The reset button starts a new epoch within ~30 s (the request poll), at most once per UTC day.
- The hero shows a break-even figure, or an honest "none" or "too few trades yet", on every dashboard page.
- `/sweet` shows the grid.
- Edge capture shows no jump at an epoch start.

## 2. What the latency axis means

**Definition.** A scenario's latency `d = proc + net + match` is the time **from the exchange showing the edge until our order is matched.**

**Why the feed lag cancels.** The sim detects at our receive time `r_open = s + L`, where `s` is the exchange time and `L` is our feed lag (`crates/sim/src/latency.rs`). It fills against the books we see at `r_open + d`, and those show the exchange at `s + d`. Both ends pass through the same `L`, so it cancels.

**Limits:**
- **Resolution** is the frame spacing of the leg's token. Below it, a smaller `d` fills against the same book. Busy markets (crypto) resolve finest.
- **Jitter** and cross-shard lag differences put legs at slightly different exchange times. This is noise, not a shift.

**Points on the axis:**

| ms | account(s) | meaning |
|---|---|---|
| 0 | new `rung-0` | baseline, impossible: filled on the book that showed the edge |
| 25 | new `rung-25` | faster than the exchange's matching prior; theoretical |
| 52 | `colo-idle` (existing) | co-located: ≈1 ms network + 50 ms matching |
| 100 | new `rung-100` | a bot in the exchange's cloud region |
| 200 | new `rung-200` | us on a good day |
| 300 | `sp-idle` (existing) | us with the spec's 150 ms matching prior |
| ~450 | `sp-burst` (existing) | us in a burst (the multiplier applies only above the burst threshold, so 450 ms idle, 1,050 ms at ×3, 3,150 ms at ×10) |

**The ladder is an explicit list**, not "every P0 fee-0 account": `rung-0`, `rung-25`, `colo-idle`, `rung-100`, `rung-200`, `sp-idle`. It lives as one constant, `LADDER`, next to `HEADLINE` in `crates/cards`. `sp-burst` (variable latency) and `sp-idle-nodelay` (300 ms with `secondsDelay` ignored, a sensitivity check) are **extras**. They are drawn as annotated markers beside the ladder and are never part of the break-even interpolation.

**"Us".** Our own position is `lag_p50 + rest_rtt_p50 / 2` (the measured figure Edge capture already computes, `bins/analyze/src/tick.rs:575-581`), **plus matching**. The drawing shows it as a band from measured + 50 to measured + 150 ms, labelled *"us (measured ≈N + matching)"*. It is never a point.

## 3. The latency ladder

`config/policies.toml` gains four scenarios and four accounts. Every other line stays byte-identical.

| scenario | proc_ms | net_ms | match_ms | honor_seconds_delay |
|---|---|---|---|---|
| `rung-0` | 0 | 0 | 0 | true |
| `rung-25` | 0 | 0 | 25 | true |
| `rung-100` | 5 | 45 | 50 | true |
| `rung-200` | 5 | 95 | 100 | true |

- **Accounts:** `P0-converter` on each, bankroll `"1000"`, per_trade `"100"`, fee_bips 0. The account ids follow `AccountSpec::id()`, e.g. `P0-converter/rung-0`.
- **No burst steps** on the rungs, so each rung's latency is exact.
- **Consequences of the edit, all expected:**
  - The policies sha changes. At the next scanner start every account starts fresh (ruling 26). That is **epoch 1**, cause `policies` (§4.2).
  - The `sim` premise version changes. EDGE and SWEET open a new premise key and catch up (`under_review` until done).
  - Edge capture's decay points come from the policies file (`bins/analyze/src/scenarios.rs`), so the rungs appear on `LatencyAxis`. The only code change is the test `the_repo_policies_give_four_scenarios`, which becomes eight.
  - SWEET's `row_scenario` maps P0 at fee 0 to NO rows under the scenario, so the rungs appear as new columns with no code change. The row count grows from 28 × 9 to 28 × 13 = 364 buckets·scenarios, still under `Z_TABLE`'s saturation at 500. `m` grows, so z is stricter; the grid (§8) shows this in its footnote.
- **Cost gate (before deploy).** Replay one busy raw hour on the PC twice with `scanner replay --dry-run`: once with the current `policies.toml`, once with the ladder. Record the wall time and peak RSS of both. The scanner has no compose `mem_limit`, so the gate is absolute. Deploy only if peak RSS grows by **less than 10 % and less than 50 MB**, and wall time grows by less than 10 %. Otherwise STOP and bring it to the owner.
- **Validation:** confirm that `SimConfig::validate` accepts `proc = net = match = 0`. If it rejects it, `rung-0` becomes `0/0/1` (1 ms), and the table and drawing say 1 ms.

## 4. Paper epochs

An epoch is a stretch of time in which every paper account started at its bankroll. Epochs are defined by **time boundaries**. No primary key or foreign key changes, and account ids stay the same across epochs.

### 4.1 Tables (migration `0009_paper_epochs.sql`)

```sql
CREATE TABLE paper_reset_requests (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    requested_ms  BIGINT NOT NULL,
    utc_day       BIGINT GENERATED ALWAYS AS (requested_ms / 86400000) STORED UNIQUE  -- one per UTC day
);

CREATE TABLE paper_epochs (
    epoch        INTEGER PRIMARY KEY CHECK (epoch >= 1),
    start_ms     BIGINT  NOT NULL UNIQUE,
    cause        TEXT    NOT NULL CHECK (cause IN ('policies', 'reset')),
    request_id   BIGINT  UNIQUE REFERENCES paper_reset_requests (id),
    run_id       BIGINT  NOT NULL REFERENCES runs (run_id),
    policies_sha256 TEXT NOT NULL
);
CREATE INDEX paper_ledger_account_t ON paper_ledger (account_id, t_ms);
```

- **Epoch 0** is implicit: everything before epoch 1's `start_ms`.
- **Rows belong to epochs by time.** A paper row belongs to epoch `e` when its time is in `[start_ms(e), start_ms(e+1))`. The time is:
  - `decided_ms` for intents;
  - `exec_ms` for execs;
  - `t_ms` for fills, ledger and marks;
  - for `paper_markouts`, which has no time column, its exec's `exec_ms`, joined on (run_id, account_id, episode_id).
  - §4.2 guarantees that no row of the new trader carries a time below `start_ms`.
- **The new index** serves the hero's per-account sums since `start_ms` (§7.1). Building it locks inserts on `paper_ledger` briefly, so the runbook builds it in the stopped-scanner window that the migration already needs.

**Roles and grants** (explicit, as since 0003; the role pattern follows `0008_compact.sql:30-40`):
- `scanner_rw`: SELECT on `paper_reset_requests` (the default already); SELECT, INSERT on `paper_epochs`.
- `analyze_ro`, `analyze_rw`: SELECT on both.
- **New role `paper_reset_rw`:**
  - created by a `DO $$ CREATE ROLE … NOLOGIN … EXCEPTION WHEN duplicate_object` block;
  - `ALTER ROLE … IN DATABASE … SET statement_timeout`;
  - `GRANT USAGE ON SCHEMA public`;
  - SELECT, INSERT on `paper_reset_requests`; SELECT on `paper_epochs`; **nothing else**. The identity column needs no sequence grant.
  - The runbook sets its password and LOGIN (as for `compact_rw`). `docker/postgres/init/01-roles.sh` gains a `RESET_RW_DB_PASSWORD` block for fresh volumes.
- The migration gets its checksum pin in `crates/store/tests/migration_checksums.rs` after the deploy, as 0008 did.

### 4.2 The scanner applies requests

- **Polling off the hot loop.** The engine's `select!` loop has no database awaits today (`run.rs:919-1030`). So a spawned task, like `run_resolver`, polls `paper_reset_requests` every 30 s for `id > last_seen` and sends new request ids to the loop over an mpsc. At start, before the loop, the scanner reads `last_applied = max(request_id)` from `paper_epochs` once and seeds the task with it. The loop keeps **`last_applied` in memory** once it applies. It never re-reads that value from `paper_epochs` while running, because a backlogged writer would make it re-apply the same request. A failed poll logs one line; the save tick is unaffected.
- **Applying** (one or more pending requests collapse into one epoch; the highest id is recorded). A new `Engine::paper_reset(t, …)` does:
  1. Take `t = max(paper clock, wall clock)`. The paper clock (`last_recv_ms`) is `i64::MIN` before the first frame; in normal running the two are within milliseconds.
  2. Call `paper_close(t)`: conversions complete, pending intents release their reservations, unwinds and markouts are dropped, every account is marked at `t`.
  3. **Reset in place:** a new `PaperTrader::reset(&mut self)` (sim).
     - Every account goes back to bankroll: cash, `reserved`, realized, fees, positions, returned, `peak_equity` (`account.rs:85`; drawdown is computed from it), drawdown.
     - `rate` (FrameRate, a market measure), `cfg` and `scenario_ix` are kept.
     - Consumed liquidity, the due queue and `next_mark_ms` are cleared.
     - **Each account's ledger `seq` carries on.** A fresh `Account::new` restarts `seq` at 0, and `paper_ledger`'s `(run_id, account_id, seq)` key plus `ON CONFLICT DO NOTHING` would then silently drop the new epoch's ledger (review C1). `next_seq` only orders the now-empty due queue, so it carries on too; that's harmless.
     - **Two consequences:**
       - clearing consumed liquidity lets the new epoch retake depth the old epoch just used, as after a restart;
       - episodes already open stay open and are never offered to the new epoch, because `on_open` runs only at open (`engine.rs:1242`).
  4. Set `last_recv_ms = t + 1`. From then on **every trader entry point** is driven with its time clamped to `last_recv_ms`:
     - `on_frame`;
     - `on_resolution` and `paper_close`, which today pass a raw `t_ms` (`engine.rs:1640-1647, 1686-1697`);
     - opens, which already clamp in `open_ctx` (`engine.rs:1196`).
     
     So **no row of the new epoch carries a time below `start_ms = t + 1`**, even for frames in the same millisecond, the 0 ms rung, a REST-snapshot open stamped with an older server time, or the resolver's immediate poll at start (review C2). The clamp changes nothing outside a reset: everything due at or before `last_recv_ms` was already drained at the frame that set it.
  5. Mark every account at `t + 1` through a new `PaperTrader::mark_now(t + 1)`, so the dashboard shows $1,000 at once.
  6. Log one line: `paper: reset → epoch N (request R)`.
- **One message, one transaction, never split.** The close rows, the `paper_epochs` row, the `mark_now` rows and a `paper_state` save go to the writer as **one new `WriterMsg::PaperReset(Batch)`**. `paper_epochs` becomes a `Batch` field, written in `write_paper` after the marks.
  - The writer first flushes its pending batch, then writes the reset batch **alone** in one transaction, never merged with unrelated rows.
  - Transient errors retry it like any flush.
  - A data error (SQLSTATE 22/23) does **not** go through `quarantine`/`into_singles`, which would commit the epoch row and `paper_state` separately (`writer.rs:187-229`, `rows.rs:387-411`). The whole group is dropped and logged at error level. The next save tick then writes `paper_state` again, and the missing epoch is visible as `current` not advancing.
  - The cap never evicts paper rows (`writer.rs:95`); `paper_epochs` is treated the same.
  - The reset's `paper_state` rows are stamped `updated_ms = now_ms()` (wall clock, as the save tick does), never the paper clock `t`. The upsert guard `WHERE paper_state.updated_ms <= EXCLUDED.updated_ms` (`crates/store/src/paper.rs:190-192`) would otherwise reject it behind a later wall-clock save.

  A crash can then never leave an epoch row next to pre-reset `paper_state`.
- **A request pending at start.** If a request is pending when the scanner starts, it builds `PaperTrader::new` instead of `restore_trader` (review C3). There are no close rows, because the previous run's shutdown already closed. The epoch starts at `start_ms = now_ms()`, and `last_recv_ms` is seeded with it, so it never touches the `i64::MIN` clock. The epoch row and the first `paper_state` save go as one `WriterMsg::PaperReset`.
- **A policies epoch at start.** The trigger is: `paper_state` has **no rows** (a fresh DB), or a **sha mismatch** (ruling 26).
  - It writes `paper_epochs(cause = 'policies', request_id = NULL, start_ms = now_ms())` together with an immediate `paper_state` save, as one `WriterMsg::PaperReset`. `last_recv_ms` is seeded with `start_ms`, as in the request case. A crash loop therefore can't write a second policies epoch: the next start sees the new sha.
  - If a reset request is also pending, one epoch is written, with cause `reset` and that request id.
- **Replay reproduces resets.** At `t`, the live scanner writes a `Ctl::PaperReset { t }` capture line through `io.ctl`, like `Resolved` and `Shutdown` (`run.rs:989, 1049`).
  - **Replay** applies `Engine::paper_reset(t)` using the **payload `t`**, not the line's timestamp. It writes no epoch row, so the live and replay ledgers still agree (runbook `s3-paper.md`). A start-time epoch needs no line, because replay always starts fresh.
  - **Other readers of the line:** the variant goes into `tape::line::Ctl` (`crates/tape/src/line.rs:18-44`). It's a **no-op** in `tape::feed` (`feed.rs:320-355`, it touches no book) and so in compact's reducer; `replay.rs:83-131` gets its arm.
  - **Old binaries can't read it.** `for_each_line` hard-errors on an unknown line (`crates/tape/src/files.rs:135-136`). So **compact must run the new image before the first reset** (§11), and after any reset, pre-7a images can neither replay nor compact that hour.
  - **Capture is best-effort** (`run.rs:397`). A dropped `PaperReset` line voids live-vs-replay across it, as a dropped `Resolved` already does. The s3-paper runbook notes it.
- **Open positions** are dropped by the reset. Their YES holdings awaiting resolution are no longer settled for the old epoch. This is accepted: the epoch's final equity counts them at their last mark, and the ledger keeps them.
- **Stale `paper_state` rows** of removed accounts (no DELETE grant) are unaffected. `paper_positions` gains a filter to current account ids (§6).

## 5. The reset route (api)

### 5.1 `POST /api/paper/reset`

- **Body:** `{"confirm": "reset"}`. Any other body gives 400 `bad_request`.
- **Required header:** `X-CT-Intent: reset-paper`. A custom header forces a CORS preflight, which the api never answers, so a cross-site form or fetch can't send it. If `Sec-Fetch-Site` is present, it must be `same-origin`. Either check failing gives 403 `forbidden`.
- **Login:** Traefik's forward-auth (oauth2-proxy, owner only) already guards every `/api` route. The api adds no auth of its own, as today.
- **Effect:** `INSERT INTO paper_reset_requests (requested_ms) VALUES (now)` through a **second pool** as `paper_reset_rw`: `Option<Store>` in `AppState` (`lib.rs:42`), a writable `Store::connect` with 1 connection, `RESET_DATABASE_URL`. The crate doc and start-up log that say the api exposes no write endpoint (`lib.rs:1-2`, `:117`) are updated.
- **CSRF.** The oauth2-proxy cookie is `SameSite=lax` (`compose.web.yml:94`). A CORS preflight carries no cookie, so forward-auth rejects it before the api sees it. `Sec-Fetch-Site` is checked on top.
- **Races.** Two requests racing the 409 check can't both be inserted on the same day: the second hits the `utc_day` unique and gets 429. Across midnight two requests can both land. The scanner collapses them into one epoch (§4.2).
- **Responses:**
  - 202 `{request_id, requested_ms}`;
  - 409 `reset_pending` when a request is not yet applied (`id > max(paper_epochs.request_id)`);
  - 429 `reset_daily_limit` on the unique `utc_day` violation, with `detail` naming the next UTC midnight;
  - 503 `reset_disabled` when `RESET_DATABASE_URL` is unset (local dev without the role).
  - Error bodies keep the `{error, detail}` shape.
- **The read-only guard stays.** The main pool stays `analyze_ro` with `default_transaction_read_only=on`. At start, the api also refuses to run if the reset role holds INSERT, UPDATE or DELETE on **any** table in `public` other than `paper_reset_requests`, or UPDATE/DELETE on that one (a loop over `pg_tables`), mirroring `lib.rs:72-78`.

### 5.2 `GET /api/paper/epochs`

It returns `{epochs: [{epoch, start_ms, cause}], current, pending_request: {id, requested_ms} | null, next_allowed_ms}` (next UTC midnight after today's request, or now). There is no epoch 0 row: `current = 0` when the table is empty.

### 5.3 The button (web, Paper page)

- **Where:** a Fold `▸ reset paper money` at the bottom of the Paper page.
- **Contents:**
  - the current epoch and its start;
  - a text input that must read `reset`, then a button;
  - no browser dialogs.
- **States:** idle; sending; "queued: the scanner applies it within ~30 s" (while `pending_request` is set, polled every 10 s); "next reset allowed after ⟨UTC time⟩".
- **Copy** follows the honesty rule (§9.4): *"Starts every paper account again at its bankroll. Earlier epochs stay in the history."*

## 6. Epoch-aware readers

| Reader | Change |
|---|---|
| `paper_equity` (`read_paper.rs:112-132`), live runs only as today | marks from the requested epoch's `start_ms`, default the current epoch; `?epoch=N` for earlier ones |
| `paper_ledger_page` | an `epoch` filter, default current |
| `paper_accounts_now` | unchanged (latest mark; the `mark_now` rows make it $1,000 at once) |
| `paper_positions` | only account ids of the newest live run's `paper_accounts` (drops removed accounts' stale rows) |
| Edge capture `edge_inputs` (`tick.rs:561-633`) | the first mark of an account at or after an epoch `start_ms` is treated like the existing policies-change case, `fresh()` = equity − bankroll, not a delta from the previous mark. With the `mark_now` row this equals skipping; without it, real P&L after the reset is still counted. analyze reads `paper_epochs` **after** `marks_with_prev` in the same tick, so a reset committed between the two reads is never half-seen. Merge-exactness holds: the change is per mark, assigned by `hour_of(m.t_ms)` |
| SWEET | unchanged: per-attempt capture, not equity. A reset adds the same artifacts as a restart: intents dropped at close count as `DROPPED` (`sweet.rs:348-361`), and markouts dropped at close leave legs unpriced |

**Paper page:**
- The header shows `since reset ⟨date⟩ · epoch N` above ACCOUNTS.
- A Fold `▸ earlier epochs` lists each epoch: start, end, cause, and each P0 fee-0 account's final equity. That's the last mark before the next `start_ms`, read through `paper_equity`'s epoch filter.

## 7. The hero: "At what latency would this pay?"

### 7.1 `GET /api/paper/ladder`

It is read-only (analyze_ro). For the current epoch, for the `LADDER` accounts (§2) sorted by latency, plus the two extras marked `extra: true`:

```
{ epoch, start_ms,
  as_of_ms,                 // the latest mark's t_ms, common cut for every sum below
  rungs: [{ account_id, scenario, latency_ms, extra,
            n_exec, n_leg_risk, n_missed,   // n_missed: execs with q_eff = 0 (SWEET's meaning)
            gains,        // Σ realized_delta, kind 'convert'  (basket gains, fees included)
            leg_risk,     // Σ realized_delta, kind 'sell'     (unwinds, fees included)
            settle,       // Σ realized_delta, kind 'settle'   (usually positive for P0: returned YES pays out)
            unrealized,   // latest mark
            net }],       // latest mark equity − bankroll = gains + leg_risk + settle + unrealized
  markets: SweetRow[],     // NO-side SWEET rows (30d) for the LADDER scenarios, dimensions all + category
  measured: { latency_ms, host } | null,   // median over 24 h of lag_p50 + rest_rtt_p50/2
  markets_status: 'live' | 'warming' | 'under_review' | 'missing',
  break_even: { kind: 'at' | 'none' | 'all' | 'thin', latency_ms?, direction?: 'loses_beyond' | 'pays_beyond', not_monotone? } }
```

- **Money** is decimal strings, as elsewhere.
- **Reconciliation.** Buys carry `realized_delta` 0 with fees in the basis, and marks carry 0 (`account.rs:176-192`). So within an epoch, equity − bankroll = Σ realized_delta + unrealized, **provided every sum stops at the same mark**. `as_of_ms` is the latest instant at which **every** ladder account has a ledger `mark` row. Each account's sums, and its counts (`n_exec`, `n_leg_risk`, `n_missed`, through the execs whose buys fall inside the cut), stop at that account's own mark row, by `(run_id, seq)`. This stays exact when a trade lands in the mark's own millisecond, or when a resolution marks only one account. Rows are summed from `start_ms` up to that cut, **live runs only** (`runs.mode = 'live'`, as every paper reader already filters). The api test asserts the identity exactly.
- **`markets`** is read from `card_results` (SWEET, 30d) and carried in the same response, so the panel still makes one fetch (§9.3). It picks the premise key with `cards::list`'s current-premise logic. Right after deploy A, SWEET catches up under the new key, so `markets` is empty and `markets_status` is `under_review`. The fold then says *"recomputing after the ladder"*.
- **`latency_ms`** comes from the policies file the api is given (`POLICIES_PATH`, default `/etc/cute-trading/policies.toml`, the copy baked into the shared image as analyze uses), parsed with `scenarios::parse` moved to a shared crate. If the file is missing, the rung's latency is null and the drawing lists it without a position.
- **`measured`** reuses `Store::health_latency` and the same nearest-rank median as Edge capture (one shared function; the plan moves it out of `analyze`).
- **`break_even`**, computed in `arb-core` or `cards` (pure, unit-tested), not in the web:
  - `thin` if any rung has `n_exec < 30`. This is the SWEET minimum; the drawing says "too few trades yet" and shows each rung's n.
  - Otherwise `all` if every rung's net ≥ 0, and `none` if every rung's net < 0.
  - Otherwise `at`: the **first sign change** between adjacent `LADDER` rungs (by latency), linearly interpolated and rounded to 5 ms, with `direction`:
    - `loses_beyond` for ≥ 0 → < 0, the expected shape;
    - `pays_beyond` for < 0 → ≥ 0, a pattern where the faster rungs lose.
  - `not_monotone` is set when there is more than one sign change.
  - Extras never enter the computation.
  - One epoch of paper trades is one path, so there is **no interval**. The caption says so.

### 7.2 The drawing (`src/schematics/LatencyLadder.tsx`)

```
 $ (paper, since reset)
 +│  ▓ basket gains (converts)
  │  ▓   ▓   ▓   ▓
 0┼──●───●───●───────────── break-even ≈ N ms   (or: none · pays at every rung · too few trades yet)
  │  ░   ░   ░   ░   ░   ░
 −│  ░ leg-risk cost (unwinds)          ▭▭▭ us: measured ≈153 + matching
  └──0──25──52──100─200─300   ms from the exchange showing the edge to our order matching
```

- **Marks:**
  - one stacked bar per rung: gains up, leg-risk down, settle drawn by its own sign (usually up);
  - a hatched segment for unrealized;
  - a dot for net;
  - a zero line;
  - the "us" band;
  - `sp-burst` and `sp-idle-nodelay` as small annotated markers beside the 300 ms rung, outside the bars.
- **Numbers:** the one big number (VT323) is the break-even line. Everything else is a label in HTML beside the SVG, because of the `preserveAspectRatio="none"` distortion lesson.
- **Scale:** the x axis is **ordinal** (equal spacing per rung), labelled with each rung's ms. A linear axis would crush 0–100 ms. The break-even position is interpolated between its two neighbouring rungs' positions.
- **Fold `▸ which markets`:** the compact Sweet grid (§8) built from the response's `markets` rows. It is the per-category answer to "why".
- **Caption** names the unwind rule: *"a missed leg sells the rest at the bid"* (§12).
- **Props:** a typed `model`. It never fetches.

## 8. The Sweet spots grid (`/sweet`)

The `IntervalChart` + scenario chips at the top of `SweetBody` are replaced by a grid. The old table stays in the `all numbers` fold.

```
                 0   25   52  100  200  300 ms       ● candidate   ○ negative
 all             ●    ●    ○    ○    ○    ○          ◌ inconclusive
 crypto          ●    ◐    ○    ○    ○    ○          ◐ thin, mean > 0   ◑ thin, mean ≤ 0
 weather         ◐    ◐    ◐    ·    ·    ·          · no attempts  ✗ dead on paper  – not tested
```

- **Axes:**
  - Rows are the chosen dimension's buckets (chips: all · category · days · S · hour; `all` first).
  - Columns are the side's `LADDER` scenarios sorted by `latency_ms` and headed with ms: 6 on the NO side and 2 on YES (colo-idle and sp-idle, the `LADDER` ∩ P1 scenarios). P1's sp-burst YES rows are in the fold only.
  - The extras (`sp-burst`, `sp-idle-nodelay`) and the fee variants (`sp-idle@10`, `@50`) stay out of the grid and remain in the `all numbers` fold.
  - The side chip (NO/YES) stays.
  - It reuses `grid()` from `web/src/lib/sweet.ts:99`, which already builds buckets × scenarios sorted by latency, filtered to `LADDER`.
- **Cell glyph** from `label` and `mean`:
  - `candidate` ●, `negative` ○, `inconclusive` ◌;
  - `thin` ◐ when mean > 0, ◑ otherwise;
  - `dead_on_paper` ✗, `not_tested` –;
  - no row ·.
  - ● and ○ are the only cells whose interval clears zero: ● from above, ○ from below. ◐ and ◑ are thin, and nothing is concluded from them. The legend sits beside the grid in HTML.
- **Tap or focus a cell** to open a small detail line under the grid (not a popover): bucket, scenario, n, clusters, captured per $100 with its interval, and the label sentence (`LABEL_TEXT`).
- **Size:** it's a plain HTML table of buttons with `aria-label`s. At 360 px there are 6 columns × 36 px + an 88 px label column = 304 px, inside 328 px (360 − 2 × 16 gutters). Longer bucket labels wrap. There's no SVG, so no aspect-ratio issue.
- **Radar's `SweetCard`** is unchanged.

## 9. The explain panel

### 9.1 Shape

As in the 09-27 spec §7.1: a vertical `EXPLAIN ▸` tab on every dashboard page; docked at ≥ 1,500 px (app + 360 px panel centred as a pair); an overlay drawer below that; Esc closes and returns focus. Open or closed and the last question open are kept in `localStorage` (try/catch). It's an `<aside aria-label="explain">`.

### 9.2 Content

| Where | Question | Drawing and source |
|---|---|---|
| every page, first, open by default | **At what latency would this pay?** | the ladder (§7.2) from `/api/paper/ladder` |
| Radar | How can this make money? | payoff structure (b) for the selected, top open, or stored *example* episode, from `EpisodeSummary.kind` and `s_legs`; money figures from the row's `net_budget100` and `net` |
| Episode detail | Would we have caught it? | the episode's `duration_ms` as a bar on the ladder's ordinal axis: rungs left of its end are "in time" |
| Episodes | What is an episode? | a duration bar from the selected row's `opened_ms`/`closed_ms`/`duration_ms` next to the §5.7 (e) example labelled *example* |
| Paper | Why is money locked? | open positions (`returned = false`) as blocks sized by `basis`, with days to resolution from `Position.end_date` (09-27 §7.4, unchanged) |

Each answer is one drawing plus at most two sentences from `src/explain/copy.ts`. Sentences that overlap `help.ts` import from it.

**Cut from the 09-27 list** (owner decision): "What did we generate today?" and "Are we making money?" (both answered by the hero), "Why is this row not arbitrage?", "How do I read these columns?", "What would buying this have cost?", "What happened after it opened?", "What does censored/stale/suspect mean?", "What do the flags mean?", "Why do the policies differ?", and everything on Papers and Health.

### 9.3 Data and the fetch rule

- **Pages publish, the panel reads**, through `ExplainContext` (`page`, `selection`), as in 09-27 §7.2.
- **The panel fetches exactly one thing:** `/api/paper/ladder`, when the panel is open. It polls every 60 s while open, and never while closed. Episode detail's question and the hero's `which markets` fold reuse that same response (its `markets` rows), with no second fetch.
- **Test:** with ticks and SSE frozen, opening the panel on each page adds exactly one call, to `/api/paper/ladder`. Opening every question adds 0 more.
- **Selection:** clicking a Radar or Episodes row, an Episode detail page, or a Paper position sets it. "back to the page" clears it.

### 9.4 Honesty rule

The 09-27 §7.2 rule applies to the text the panel renders, including any `help.ts` sentence the panel imports. `help.ts:160` ("made or lost") is reworded if the panel imports it. The rule:
- every money figure is labelled *paper*, *on the table* or *captured at N ms*;
- the words `earned`, `earn`, `made` and `income` fail the test;
- `profit` is allowed only in the existing phrases.

The hero uses **"basket gains"** and **"leg-risk cost"**, never "profit". It never says a latency "is profitable". It says "pays on paper" or "loses on paper". The same scan also covers the Sweet grid legend and the reset fold.

## 10. Testing

**Rust:**
- `sim`:
  - `PaperTrader::reset` keeps each account's `seq` and `next_seq`: after close → reset → a trade, ledger seqs strictly increase per account within the run (review C1);
  - `PaperTrader::mark_now`;
  - a reset sequence test: close → `reset` → `mark_now` gives the close rows before `start_ms`, every account at bankroll with `peak_equity` reset at `start_ms`, and `rate` kept;
  - the 0 ms scenario is accepted (or the 1 ms fallback, §3).
- `scanner`:
  - a pending request is applied once, and a second tick with no new request does nothing; `last_applied` stays in memory even while the writer is backlogged;
  - frames in the reset's millisecond, and a 0 ms rung trade right after it, produce no new-epoch row with a time below `start_ms` (review C2);
  - a request pending at start builds a fresh trader with `start_ms = now_ms()`, with no close rows and no `i64::MIN` times (review C3);
  - the close rows, epoch row, marks and `paper_state` go as one `WriterMsg::PaperReset`; `extend_paper` and `restore` keep `paper_epochs`;
  - an empty `paper_state` at start writes a `policies` epoch; a crash before the first save doesn't write a second one;
  - `Ctl::PaperReset` is captured, and replay of it (using the payload `t`) gives the same ledger as live; `tape::feed` and compact ignore it;
  - writer: a reset message with a poison episode row pending is still written whole (the pending batch is flushed first), and a data error in the reset group drops it whole, never split;
  - the reset's `paper_state` is stamped with the wall clock and isn't rejected by the upsert guard;
  - a REST-snapshot open and a resolution right after a reset, and at start for the policies epoch, are stamped ≥ `start_ms`;
  - two pending requests collapse to one epoch with the highest id;
  - a policies-sha fresh start writes a `policies` epoch;
  - both at once write one `reset` epoch;
  - a failed poll logs one line and the save tick still saves `paper_state`;
  - replay writes no epoch row.
- `store`:
  - migration 0009 grants (`has_table_privilege` for all four roles);
  - `paper_reset_rw` can't INSERT anywhere else;
  - the `utc_day` unique violation;
  - the epoch filters in `paper_equity` and `paper_ledger_page`;
  - the `paper_positions` current-accounts filter.
- `api` (`bins/api/tests/paper.rs`):
  - POST without the header → 403;
  - with `Sec-Fetch-Site: cross-site` → 403;
  - a bad body → 400;
  - OK → 202;
  - a second one the same day → 429;
  - pending → 409;
  - no `RESET_DATABASE_URL` → 503;
  - the start-up refusal with an over-granted reset role;
  - `GET /api/paper/epochs`;
  - `GET /api/paper/ladder` sums per kind since `start_ms` across two runs of one epoch; `gains + leg_risk + settle + unrealized = net` exactly; replay rows are ignored; a row after `as_of_ms` is excluded.
- `break_even`:
  - `thin`, `all`, `none`;
  - `at` in both directions, with interpolation and rounding;
  - `not_monotone`;
  - extras are ignored.
  - A property test over arbitrary sign patterns: the result is always defined, and an `at` lies between the two rungs it names.
- `analyze`:
  - the first mark at or after an epoch start counts as `fresh()`, with and without a `mark_now` row;
  - deltas inside an epoch are unchanged;
  - `the_repo_policies_give_four_scenarios` becomes eight.
- `migration_checksums`: 0009 is pinned after deploy.

**Web:**
- `LatencyLadder`:
  - renders every `break_even` kind;
  - bars are signed correctly;
  - the us-band sits between the right rungs;
  - it renders with `measured = null`.
- The Sweet grid:
  - the glyph per label and sign;
  - fee variants and extras are excluded; the YES side has 2 columns;
  - columns are sorted by latency;
  - the detail line on focus;
  - a 360 px structure check (the `theme.test.ts` rules).
- The reset fold: the input gate, each response state, and polling only while pending.
- The panel:
  - each page's questions render from fixtures;
  - the fetch rule (§9.3);
  - selection and "back";
  - Esc and focus;
  - the banned-words scan (§9.4).
- `npx tsc --noEmit` alongside vitest.
- **Chrome check after deploy** at 1,440 px and ~360 px (the redesign lesson).

## 11. Deploy

**Two steps, two plans:** plan 7a-A (backend + api, ends at deploy A) and plan 7a-B (web, ends at deploy B). The backend goes as soon as it's ready, because every rung needs 1–2 days of trades before the hero means anything. The web follows.

1. **Backend (step A):**
   - migration 0009;
   - the `paper_reset_rw` password;
   - the scanner image with the ladder `policies.toml` (this starts epoch 1);
   - analyze;
   - the api with `RESET_DATABASE_URL` and `POLICIES_PATH`.
   - **compact recreated on the new image**, before any reset can happen. The first reset is gated in the runbook until compact runs the 7a image.
   - Gates, in order: the §3 cost gate on the PC, then the migration grants check, then "19 accounts at $1,000 in epoch 1" (`paper_marks` and `paper_epochs` queries).
2. **Web (step B):** the panel, the hero, the grid and the reset fold. `web` is recreated at a quiet hour (the 2026-09-28 websocket-drop lesson), and the Chrome check follows.

**Runbook:** `docs/superpowers/runbooks/phase7a-latency-vps.md`, in the owner's format (command → expected, self-contained, STOP after migrate). It covers:
- rollback tags (rust and web pre-7a);
- a DB dump before 0009;
- the checksum pin after deploy.

**Live-vs-replay across a reset** is kept by `Ctl::PaperReset` (§4.2). The s3-paper runbook notes it.

**Rollback:** the previous images, plus the old `policies.toml`. Pre-7a images never write `paper_epochs`. So a rollback's own epoch is recorded only at the next 7a start, through the sha mismatch. **After any reset, a pre-7a image can't replay or compact that hour** (§4.2). Migration 0009 is additive and stays.

## 12. Risks and notes

- **The unwind rule dominates the answer.** Selling the whole basket at the bid with no floor makes a one-leg miss cost about $17 in a burst. A different rule (hold, limit-sell, retry the missing leg) could move the break-even. That's a new policy and a later phase. The hero's caption names the rule: *"a missed leg sells the rest at the bid"*.
- **The phantom-book bug** could still contribute to leg risk. Its fix (queued) is a scanner change, so it doesn't touch premises and would show up as a change in the leg-risk bars.
- **Crypto dominates.** The busiest markets carry both the edge and the adverse selection. The hero keeps them in; the `which markets` fold shows the split.
- **Oddities from the diagnosis, not chased here:**
  - health `lag_p99` is 17–53 s in every hour;
  - msgs/hour dropped from ~14 M (run 8) to ~5.7 M (run 24).
- **One reset per UTC day.** It's per calendar day, not a rolling 24 h, so it's enforced by a unique index without a race.

## 13. Decisions (owner, 2026-09-29)

1. **The panel's job:** one hero question on every page, plus 1–2 questions per page in folds (option A). The final cut is 1 + 4 (§9.2).
2. **Reset semantics:** an epoch. Old history is kept, and pages show "since reset" (option A).
3. **Precision:** a paper-account latency ladder (option B), with fewer rungs. The 4 new rungs (0, 25, 100, 200) are combined with the existing 52 and 300.
4. **Latency is exchange-relative;** rungs below our feed lag are valid hypotheticals. 0 ms is the baseline, 52 ms is co-located, and "us" is measured + matching.
5. **Crypto stays in the main drawing;** the split is in a fold.
6. **The Sweet spots grid** replaces the per-scenario chart.
7. **The reset button** sits in a Paper page fold, typed confirmation, 1 per day. The scanner's request poll (~30 s) picks it up.
8. **Phase 7 split:** 7a (this spec), then 7b (`/math`, unchanged).
9. **Review:** an independent subagent, not the owner.

## 14. Order of work

**Plan 7a-A (backend):**

1. `sim`: `PaperTrader::reset`, `mark_now`, the entry-point clamp, and the 0 ms validation check; the `tape` `Ctl::PaperReset` variant (a no-op in `feed`). Then the ladder `policies.toml` and the §3 cost gate on a replay.
2. Migration 0009 and its grants tests; `store` epoch reads and filters.
3. The scanner's request apply (§4.2) and policies-epoch.
4. `break_even` (pure) and the shared `scenarios::parse` and median; analyze's epoch-aware deltas.
5. The api: `/paper/reset`, `/paper/epochs`, `/paper/ladder`, `Position.end_date`, the start-up refusal.
6. Runbook step A. **Deploy A.**

**Plan 7a-B (web):**

7. Web: `LatencyLadder`, the Sweet grid, the reset fold and the epoch header.
8. Web: the panel shell, then the four questions.
9. Runbook step B. **Deploy B** at a quiet hour; Chrome check.

## 15. Changes after the independent review (2026-09-29)

- **C1:** the reset is done in place (`PaperTrader::reset`) and keeps ledger `seq`. A fresh `Account::new` would have collided on `paper_ledger`'s key, and `ON CONFLICT DO NOTHING` would have dropped the new epoch's ledger without a sound.
- **C2:** `last_recv_ms = t + 1` after a reset, so no new-epoch row can be stamped before `start_ms`.
- **C3:** a request pending at start builds a fresh trader with `start_ms = now_ms()`. The policies epoch uses `now_ms()` too, and fires on empty `paper_state`.
- **Atomicity:** close rows, the epoch row, marks and `paper_state` go in one `Batch`, and `paper_epochs` is a `Batch` field. `last_applied` is held in memory.
- **Polling** moved to a spawned task (no DB await in the engine loop).
- **Replay** reproduces resets through `Ctl::PaperReset`.
- **Cost gate** is absolute (< 10 % and < 50 MB); the scanner has no `mem_limit`.
- **The ladder is an explicit list;** `sp-burst` and `sp-idle-nodelay` are extras.
- **The grid** has 6 NO columns, and the 360 px budget was recomputed.
- **`break_even`** is defined for every sign pattern.
- **`/api/paper/ladder`** sums stop at the latest mark, cover live runs only, reconcile exactly, and embed the `markets` rows. That keeps the one-fetch rule.
- **Edge capture** uses `fresh()` at an epoch's first mark instead of skipping a delta.
- **Migration:** follows 0008's role pattern; the index is built in the stopped window; markouts get their epoch through their exec.
- **Api:** a second pool as `Option<Store>`; the reset-role refusal covers every table; CSRF and race reasoning are written down; `POLICIES_PATH` defaults to the baked `/etc/cute-trading/policies.toml`.
- **Honesty scan** is scoped to the panel's rendered text, including imported `help.ts` sentences.
- **Scope:** two plans, 7a-A and 7a-B.

## 16. Changes after the second review pass (2026-09-29)

- **The reset is a writer message of its own** (`WriterMsg::PaperReset`): flushed alone, never quarantined row by row, and its `paper_state` is stamped with the wall clock.
- **`Ctl::PaperReset`:** a no-op in tape/compact. Compact moves to the 7a image before the first reset. Replay uses the payload `t`. Rollback limits are stated.
- **Both start cases** seed `last_recv_ms = start_ms`, and every trader entry point is clamped.
- **`reset` contents:** `reserved`, `peak_equity`; `rate` is kept. Its consequences are stated.
- **Leftovers:** poll timing wording; sp-burst's real latencies; `direction` and `markets_status` in the schema; 2 YES columns; test and order-of-work leftovers; rollback epoch wording.
