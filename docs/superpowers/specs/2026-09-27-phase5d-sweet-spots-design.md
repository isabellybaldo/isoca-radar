# isoca-radar Phase 5d: sweet spots

**Date:** 2026-09-27. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.6 paper execution, §11 policies). **Builds on:** Phase 5a (`2026-09-26-phase5a-cards-design.md`: card framework, premises, `analyze`, Edge capture). **Comes before:** 5b (external storage).
**Status:** idea approved in conversation on 2026-09-27. The owner asked for spec → independent review → plan without per-section approval, so the design choices below were made without the owner and are listed in §12 for override.

## 1. Purpose, scope and success

Edge capture (5a §6) answers *how much* edge a latency keeps, summed over everything. It does not answer *where*. **Sweet spots** slices the **captured** edge, not the edge on the table, after fees and capital lockup. It cuts by category, days to resolution, set size S, hour of day and latency scenario, and asks one question per slice: **does anything stay positive once you arrive late, pay the fees and wait for your money?**

"No cell survives" is a valid, useful answer, and the page says it plainly: *"Nothing here pays at 300 ms. Don't trade this, or get closer."*

**In scope:**
- A per-attempt capture measure built from existing paper rows (§4).
- A category source: Gamma event tags, recorded by the scanner (§3).
- One new card family, `SWEET`, in the 5a framework: hourly merge-exact cells, windows, premise key, status, hash (§5, §6).
- Statistical guard rails: minimum n, confidence intervals, a multiple-comparisons correction, and labels that never call a slice "profitable" (§7).
- A **Sweet spots** box on the Paper page, below Edge capture (§8).

**Out of scope:**
- New paper accounts or edits to `policies.toml`. Any edit resets every paper account (S3c ruling 26). §12 D3 lists the one account worth adding later.
- Cross-dimension slices (category × days × S …). §7.3 explains why.
- Real orders. A candidate is a hypothesis for real small orders, which the paper-only rule does not allow. That decision belongs to the owner, not to this phase.
- Raw-book backtests of new strategies: those need 5b.

**Exit:** on the VPS, the Paper page shows the Sweet spots box for 7d / 30d / all, every cell carries n, a status and the card hash, and `analyze` stays inside its CPU cap (§10 benchmark, VPS `docker stats`).

## 2. Architecture

```
scanner ──(new) events.tags from Gamma──► events
        ──paper trader (unchanged)──────► paper_intents, paper_execs, paper_ledger
analyze tick (unchanged schedule) ──► card_cells('sweet', key, hour) ──merge──► card_results('SWEET', win)
api GET /api/cards?win=… (unchanged endpoint; one more card) ──► web Paper page: Sweet spots box
```

- **No new service, no new endpoint.** `SWEET` is one more card in the existing `analyze` tick and one more entry in `GET /api/cards`.
- **`analyze_rw` gains SELECT** on `events`, `episode_legs`, `paper_intents`, `paper_execs` and `paper_ledger` (migration 0007). It still writes only the card tables, and the startup write-refusal check is unchanged.
- **The scanner's only change is recording tags** (§3). Detection, episodes and the paper trader are untouched, so no premise area of theirs moves.

## 3. Category from Gamma tags

`episodes.category` exists but the scanner always writes NULL (`bins/scanner/src/engine.rs`, `writer.rs`). 5d leaves that column alone and records tags **per event** instead:

- **`pm-api`:** `GammaEvent` gains `tags: Option<Vec<GammaTag>>` with `GammaTag { slug: Option<String>, label: Option<String> }`. Parsing is lenient: a missing or malformed `tags` is `None`, never a parse error for the event.
- **Migration 0007:** `events.tags TEXT[] NULL`, holding the tag slugs as Gamma returned them, lower-cased, at most 16.
- **`store` universe upsert:** writes `tags` on insert and on conflict (`tags = EXCLUDED.tags`), so every event still listed at the first universe refresh after deploy gains its tags. **Past episodes join through `event_id`**, so their categories appear without a backfill as long as the event is still in `events`.
- **Buckets** live in `crates/cards/src/sweet.rs` as a const ordered list of `(bucket, &[tag slugs])`. The first bucket with a matching slug wins:
  `politics`, `sports`, `crypto`, `economy` (finance, business, economy, fed, rates), `world` (geopolitics, world, elections-international), `culture` (pop culture, entertainment, awards, music, movies), `tech` (tech, ai, science), then `other` for tagged events that match nothing, and `untagged` for NULL or empty tags.
- The exact slug lists are fixed in the plan from one live Gamma sample (§11). Bucket definitions are part of the new premise area `sweet` (§6), so editing them restarts the card instead of mixing definitions.

## 4. The unit: one attempt

An **attempt** is one (paper account, episode) pair with a `paper_intents` row, from the accounts below.

**Accounts used.** Only default-fee accounts on the arbitrage side they are built for:
- **NO side:** `P0-converter` with `fee_bips = 0`, one per scenario: `colo-idle`, `sp-idle`, `sp-burst`, `sp-idle-nodelay`.
- **YES side:** `P1-settlement`, one per scenario: `colo-idle`, `sp-idle`, `sp-burst` (there is no `sp-idle-nodelay` P1 account).
- **Not used:** the `fee_bips` 10 and 50 P0 accounts (shown only as a fee-sensitivity line, §8), P2 (dormant), and P3 (control group; ignores fees and completeness).

**Episodes used.** `source = 'live'`, kinds `no_basket` and `yes_basket` (`yes_basket_incomplete` is never arbitrage), runs whose `run_premises` match the card key. `engine_suspect` or `stale_transport` episodes are excluded and counted, as in 5a.

**States of an attempt** (exactly one):
- **`skipped`:** `paper_intents.skip_reason` is set. Captured 0, capital 0; counted by reason (`no_cash`, `below_min_size`, `consumed`, `too_long`, `annualized_too_low`, …).
- **`missed`:** executed, `paper_execs.q_eff = 0`. Captured is the sum of its ledger rows (normally 0).
- **`filled`:** executed with `q_eff > 0`.
- **`in_flight`:** executed less than 60 s before the tick's input cutoff. Left out of the tick and picked up by a later one, because the conversion or unwinds may not be in the ledger yet.

**Captured of a filled attempt**, exact, from `paper_ledger` rows with this `(run_id, account_id, episode_id)`. All figures include fees, because `cash_delta` does:
- `cash_out` = −Σ `cash_delta` of its `buy` rows;
- `cash_back` = Σ `cash_delta` of its `sell` and `convert` rows;
- **NO side:** `captured = cash_back − cash_out`. YES tokens returned by conversions are worth 0 here, the same convention as the paper accounts and the ★ rule (a bonus is never counted).
- **YES side:** `captured = cash_back + q_eff × 1 − cash_out`. The `q_eff` complete sets pay 1 each at resolution. Settlement ledger rows carry no `episode_id`, so the payout is taken as the complete-set guarantee (L3), not read from settlement. A non-binary resolution (Q19) is an existing integrity class, and the card notes it.
- **Leftovers:** if the attempt's buys minus its sells leave any leg with a positive quantity beyond `q_eff`, that leftover is valued at 0 (conservative), and the attempt is flagged `held_leftover` and counted in the notes. The plan checks the sim's unwind path to confirm this is rare or impossible.
- **Capital:** `cost = cash_out`.
- **Lock time:** YES side: `episodes.days_to_end` at open (NULL → the attempt is in the `unknown` days bucket and has no annualized figure). NO side: the capital returns within `settle_delay_ms` (2 s), so it is **not annualized**; the NO side reports return per $100 per trade instead.

**Edge on the table** for the same slice is the 5a definition (Σ `pnl_net_budget100_usd` over net-positive episodes), so every row can show "on the table vs captured" side by side. YES rows also carry **`on_table_eligible`**: the same sum over episodes P1 did not skip as `too_long` or `no_lockup_figures`. In a Category, S or Hour row, YES captured covers only P1-eligible episodes, so its capture rate is computed against `on_table_eligible`, never against the full figure.

## 5. Slices

**Always split by side** (YES and NO work differently) and **by scenario** (the latency axis, labelled with the scenario's `proc + net + match`: 52, 300, 450 ms, and `sp-idle-nodelay` 300 ms with secondsDelay ignored).

**One dimension at a time**, each with its own buckets:

| Dimension | Source | Buckets |
|---|---|---|
| Category | `events.tags` via `event_id` (§3) | the §3 buckets |
| Days to resolution | `episodes.days_to_end` at open | <1, 1–7, 7–30, 30–90, 90–365, >365, unknown |
| Set size S | count of `episode_legs.in_s` | 2, 3, 4–5, 6–10, >10 |
| Hour of day (UTC) | `episodes.opened_server_ms` | 00–04, 04–08, 08–12, 12–16, 16–20, 20–24 |
| All | — | one row: the whole side |

Bucket edges are half-open `[lo, hi)`. Days use exact decimals.

## 6. Cells, premises and windows

- **Premise areas:** `episode_def`, `detector_yes`, `detector_no`, `fees`, `depth`, `sim`, `host` (as Edge capture), plus a new area **`sweet`** covering `crates/cards/src/sweet.rs` (buckets, thresholds, the z table in §7.2). It is added to `premises.rs` and `premises.lock` in the usual way.
- **Cell family `sweet`**, one per UTC hour per premise key, keyed by the episode's open hour. For each (side, scenario, dimension, bucket) the cell holds, sparsely (empty entries are omitted):
  - counts: episodes on the table, attempts, skipped by reason, missed, filled, `held_leftover`, flagged-excluded;
  - exact decimal sums: on-the-table edge (all and P1-eligible), captured Σx, captured Σx² (for the interval), cost, and cost × days (YES);
  - the maximum single captured value and the minimum (merge by max and min).
  - A property test checks merge(cells) equals the direct computation, as in 5a.
- **Dirty hours:** the existing `updated_ms` cursor already marks an episode's hours dirty when it closes, which is after its attempts execute. In addition, `sweet` rebuilds the current and previous hour on every tick, which covers attempts that finish after the close.
- **Windows:** `24h`, `7d`, `30d`, `all`, as 5a. `today` does not include `SWEET`. The box defaults to **30d**, because shorter windows are mostly `thin`.
- **Status:** the 5a statuses (`live`, `warming`, `under_review`) apply to the card. `thin` applies per row (§7.1), not to the whole card.

## 7. Statistical honesty

### 7.1 Minimum sample
A row needs **n ≥ 30 filled attempts** to get any verdict. Below that its label is `thin`: the numbers are shown dimmed with n, and it can't be a candidate.

### 7.2 Interval and multiple comparisons
- For each row: mean captured per filled attempt, x̄ = Σx / n, and sample sd from Σx and Σx². The interval is x̄ ± z · sd / √n, with `rust_decimal` maths (no floats).
- **Bonferroni.** Every tick counts m = the number of non-`thin` rows in the window, across all dimensions, sides and scenarios. z is taken for a two-sided family error of 5% over m rows from a fixed table (`sweet.rs`), using the next larger m, which is conservative:

  | m ≤ | 1 | 5 | 10 | 20 | 50 | 100 | 200 | 500 | 1000 |
  |---|---|---|---|---|---|---|---|---|---|
  | z | 1.960 | 2.576 | 2.807 | 3.023 | 3.291 | 3.481 | 3.662 | 3.891 | 4.056 |

  For m above 1000, z = 4.056 and a note says the correction is capped.
- The box states m and z in a footnote: *"m rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20."*
- **Heavy tails:** each row also shows the share of Σ captured that came from its single largest attempt. Above 50% it is labelled *"one trade carries this row"*.

### 7.3 Why no cross slices
The 28 buckets (9 + 7 + 5 + 6 + 1) × 7 side-scenario pairs (4 NO, 3 YES) give ~200 one-way rows already. A full cross product would give thousands of rows, almost all `thin`, and Bonferroni would make the rest unreadable. One-way slices are the honest resolution for weeks of data. Cross slices can come back when 5b keeps more history.

### 7.4 Labels (per row; never "profitable")
- **`candidate`:** n ≥ 30, lower interval bound > 0, and, YES side only, the annualized return on locked capital (§7.5) ≥ the hurdle. Wording: *"survives here; worth a real small-order test."*
- **`negative`:** upper interval bound < 0. Wording: *"loses here. Don't trade this."*
- **`inconclusive`:** anything else with n ≥ 30.
- **`thin`:** n < 30.
- **`not_tested`:** no account trades this row, decided from the data: every YES attempt in the row was skipped as `too_long` or `no_lockup_figures` (P1 only takes days ≤ 30 with known lockup figures). In practice that is the 30–90 (except exactly 30 days), 90–365, >365 and `unknown` days rows. Such rows still show edge on the table and its **upper-bound** annualized figure from `episodes.annualized_return` (capital-weighted, §7.5). When even that upper bound is below the hurdle, the row reads **`dead_on_paper`**: *"even first in line, the money is locked too long to be worth it."*

### 7.5 Capital lockup (YES side)
- Annualized return on locked capital, per row, capital-weighted: `(Σ captured / Σ (cost × days)) × 365`. It is NULL when Σ (cost × days) = 0.
- **The hurdle** is `p1.min_annualized` read from the same `policies.toml` the scanner runs (10% today), so the card and the policy can't disagree. Changing it changes the `sim` premise, which restarts the card.
- For `not_tested` rows the upper bound uses `episodes.annualized_return` weighted by `peak_cost_usd` over net-positive episodes with a non-NULL figure.

### 7.6 The paper caveat, on every view
A fixed note, shown under the box title and never collapsed: *"Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer."*

## 8. API and web

- **API:** no change to endpoints. `GET /api/cards?win=24h|7d|30d|all` returns `SWEET` with the 5a envelope (`status`, `premise_key`, `hash`, `data_from_ms`, `data_to_ms`, `n`, `notes`). Its `series` holds:
  - `rows[]`: `{ side, scenario, latency_ms, dimension, bucket, label, n_filled, n_attempts, skipped: {reason: n}, n_missed, on_table, on_table_eligible, captured, mean, lo, hi, top_share, cost, annualized, annualized_upper }`, all money as decimal strings or null;
  - `m`, `z`;
  - `fee_sensitivity`: the NO side's Σ captured at `fee_bips` 0, 10, 50 for `sp-idle`.
- **Web: a Sweet spots box on the Paper page, below Edge capture.**
  - A headline sentence per side at the headline scenario (`sp-idle`): *"YES: 0 candidates in 34 tested rows."* or *"NO: 2 candidates: sports, 2 legs."*
  - A dimension switch (All · Category · Days · S · Hour) and a side switch (NO · YES). The table has rows = buckets and columns = scenarios, and each cell shows the label tag, the mean with its interval, and n. YES cells add the annualized figure.
  - The colo-idle column is titled *"a co-located trader"*, so the latency gap reads directly.
  - The window switch shares Edge capture's state.
  - (?) help: "What is a sweet spot?", "Why are most rows thin?", "What is Bonferroni, in one line?", "Why is YES > 30 days not tested?".
  - No horizontal page scroll at 360 px: the table scrolls inside its box.

## 9. Performance and disk

- **Reads per dirty hour:** the hour's episodes (existing), plus their attempts by `episode_id` through new indexes on `paper_intents (episode_id)` and `paper_execs (episode_id)` (migration 0007), plus ledger rows by the existing `paper_ledger_episode` index, plus `episode_legs` by its primary key, plus `events.tags` by primary key.
- **Budget:** the 5a benchmark (a tick over a synthetic 30-day history, < 200 ms CPU in a debug build) is re-run with `SWEET` included and must still pass. `SWEET` alone should add < 50 ms.
- **Disk:** cells are sparse. An hour with k episodes writes at most k × 5 dimensions × up to 4 scenarios entries. Expect well under 100 MB a year, inside the 50 GB VPS budget.

## 10. Testing

- **Captured (unit):** NO attempt with conversion and unwind; YES attempt with the complete-set payout; `missed`; `skipped`; `held_leftover` valued at 0; fees included; `in_flight` excluded until 60 s old.
- **Buckets:** tag → bucket order, `untagged`, `other`; half-open edges for days, S and hours.
- **Statistics:** the interval on a hand-computed set; the z table lookup (next larger m); m counting excludes `thin`; `top_share`; every label rule, including `dead_on_paper` and the NULL annualized case.
- **Merge property:** random attempts → merged hourly cells equal the direct computation for every window.
- **Premises:** a change to `sweet.rs` without a version bump fails the lock test.
- **Store (real Postgres):** migration 0007 is idempotent; `analyze_rw` can read the new tables and still can't write them; the universe upsert writes and refreshes `tags`.
- **pm-api:** an event with tags, without tags, and with malformed tags all parse.
- **Benchmark:** §9.
- **Web:** each label renders; the paper caveat is always visible; the headline sentences for zero and some candidates; 360 px.

## 11. Open items for the plan

- One live Gamma `/events` sample (read-only, one request) to confirm `tags` is present on keyset pages and to fix the slug lists of §3.
- Confirm in `crates/sim` whether an attempt can leave a leftover leg unsold (§4). If impossible, `held_leftover` stays as a guard with a test, not a live count.
- Confirm that `paper_ledger` rows for `sell` and `convert` always carry the attempt's `episode_id` (they do in `account.rs`; the plan cites the line).

## 12. Decisions made without the owner (override any)

- **D1. Captured = per-attempt ledger cash plus the YES complete-set payout,** not equity deltas. Equity can't be split by episode, and settlement rows have no `episode_id`.
- **D2. Only the default-fee P0 (NO) and P1 (YES) accounts count.** P3 is a control and P2 is dormant. The fee accounts appear only as a sensitivity line.
- **D3. No `policies.toml` edit.** YES rows ≥ 30 days are `not_tested` because P1 filters them out. Adding an unfiltered P1 account would test them, but it resets every paper account. Owner's call, later.
- **D4. One-way slices only;** no cross product (§7.3).
- **D5. n ≥ 30 filled attempts** for any verdict; **Bonferroni** over all non-thin rows; a normal-approximation interval with a heavy-tail warning.
- **D6. The hurdle is P1's `min_annualized` (10%),** shared with the policy.
- **D7. Category comes from Gamma tags on `events`,** joined by `event_id`; `episodes.category` stays unused. Events that left `events` before deploy read `untagged`.
- **D8. Hour of day in 4-hour UTC blocks** and days-to-resolution in 7 buckets, chosen for sample size, not elegance.
- **D9. The box lives on the Paper page below Edge capture,** with no new nav tab, and defaults to 30d.
- **D10. Default headline scenario `sp-idle`,** the same as Edge capture.
