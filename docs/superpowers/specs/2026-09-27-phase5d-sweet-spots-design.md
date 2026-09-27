# isoca-radar Phase 5d: sweet spots

**Date:** 2026-09-27. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.6 paper execution, §11 policies). **Builds on:** Phase 5a (`2026-09-26-phase5a-cards-design.md`: card framework, premises, `analyze`, Edge capture). **Comes before:** 5b (external storage).
**Status:** idea approved in conversation on 2026-09-27. The owner asked for spec → independent review → plan without per-section approval, so the design choices below were made without the owner and are listed in §12 for override. Revised the same day after an independent review (§13).

## 1. Purpose, scope and success

Edge capture (5a §6) answers *how much* edge a latency keeps, summed over everything. It does not answer *where*. **Sweet spots** slices the **captured** edge, not the edge on the table, after fees and capital lockup. It cuts by category, days to resolution, set size S, hour of day and latency scenario, and asks one question per slice: **does anything stay positive once you arrive late, pay the fees and wait for your money?**

"No cell survives" is a valid, useful answer, and the page says it plainly: *"Nothing here pays at 300 ms. Don't trade this, or get closer."*

**Where it is useful.** Mostly on the **NO side**. The YES side can only be measured where P1 trades, which is complete baskets resolving within 30 days with at least a 10% annualized edge on the table. There, YES rows measure latency decay on short-dated baskets. Everything else on the YES side is `not_tested`, or `dead_on_paper` when even the first-in-line upper bound fails the hurdle (§7.4).

**In scope:**
- A per-attempt capture measure built from existing paper rows (§4).
- A category source: Gamma event tags, recorded by the scanner, behind a go/no-go check (§3).
- One new card family, `SWEET`, in the 5a framework: hourly merge-exact cells, windows, premise key, status, hash (§5, §6).
- Statistical guard rails: minimum clusters, a cluster-robust interval, a multiple-comparisons correction, and labels that never call a slice "profitable" (§7).
- A **Sweet spots** box on the Paper page, below Edge capture (§8).

**Out of scope:**
- New paper accounts or edits to `policies.toml`. Any edit resets every paper account (S3c ruling 26). §12 D3 lists the one account worth adding later.
- Cross-dimension slices (category × days × S …). §7.3 explains why.
- Real orders. A candidate is a hypothesis for real small orders, which the paper-only rule does not allow. That decision belongs to the owner, not to this phase.
- Raw-book backtests of new strategies: those need 5b.

**Exit:** on the VPS, the Paper page shows the Sweet spots box for 7d / 30d / all, every row carries n, clusters, a label and the card hash, and `analyze` stays inside its CPU cap and memory cap (§9 benchmark, VPS `docker stats`).

## 2. Architecture

```
scanner ──(new, gated §3) events.tags from Gamma──► events
        ──paper trader (unchanged)────────────────► paper_intents, paper_execs, paper_ledger, paper_markouts
analyze tick (every 900 s, unchanged) ──► card_cells('sweet', key, hour) ──merge──► card_results('SWEET', win)
api GET /api/cards?win=… (unchanged endpoint; one more card) ──► web Paper page: Sweet spots box
```

- **No new service, no new endpoint.** `SWEET` is one more card in the existing `analyze` tick and one more entry in `GET /api/cards`.
- **`analyze_rw` gains SELECT** on `events`, `episode_legs`, `paper_intents`, `paper_execs`, `paper_ledger` and `paper_markouts` (migration 0007). It still writes only the card tables, and the startup write-refusal check is unchanged.
- **The scanner's only change is recording tags** (§3). Detection, episodes and the paper trader are untouched, so no scanner premise area moves.
- **No new indexes.** Every paper table's primary key starts with `(run_id, account_id, episode_id)` (`0004_paper.sql`), and `analyze` knows both the run ids and the account ids, so it looks attempts up by full key prefix. That also avoids a non-concurrent `CREATE INDEX` blocking the scanner's inserts during the migration.

## 3. Category from Gamma tags (go/no-go first)

`episodes.category` exists but the scanner always writes NULL (`bins/scanner/src/engine.rs:1328`, `writer.rs:265`), and `GammaEvent` has no `tags` field today (`crates/pm-api/src/gamma.rs:48-59`).

**Go/no-go, the plan's first task.** One read-only request to the route the scanner actually uses, `GAMMA_EVENTS_KEYSET` (`/events/keyset`), checks whether events carry `tags` with slugs. The sample is saved as a test fixture.
- **Go:** everything below ships.
- **No-go:** the Category dimension is dropped from the card, and §3's pm-api, scanner and store write changes are not made. Migration 0007 still adds the (then unused) nullable column, so the migration never depends on the answer. Everything else ships unchanged. The spec's buckets (§5) then have four dimensions, and the (?) help says why category is missing.

**If go:**
- **`pm-api`:** `GammaEvent` gains `tags: Option<Vec<GammaTag>>` with `GammaTag { slug: Option<String>, label: Option<String> }`. Parsing is lenient: a missing or malformed `tags` is `None`, never a parse error for the event.
- **Migration 0007:** `events.tags TEXT[] NULL`, holding the tag slugs as Gamma returned them, lower-cased, at most 16.
- **`store` universe upsert:** writes `tags` on insert. On conflict, **`tags = COALESCE(events.tags, EXCLUDED.tags)`**: an event's tags are frozen the first time they are seen, so an event never changes bucket halfway through a window. Rows are never deleted from `events`, so every event still listed at the first universe refresh after deploy gains its tags, and past episodes join through `event_id` without a backfill.
- **`untagged`** is an event the scanner never re-upserted after deploy (it left the listing before the first refresh), or one Gamma lists with no tags.
- **Buckets** live in `crates/cards/src/sweet.rs` as a const ordered list of `(bucket, &[tag slugs])`. The first bucket with a matching slug wins:
  `politics`, `sports`, `crypto`, `economy`, `world`, `culture`, `tech`, then `other` for tagged events that match nothing, and `untagged`.
- The exact slug lists are fixed from the go/no-go sample. They are card parameters (§6), so editing them rebuilds the card instead of mixing definitions.

## 4. The unit: one attempt

An **attempt** is one (paper account, episode) pair with a `paper_intents` row, from the accounts and episodes below.

**Accounts used.** Only default-fee accounts on the arbitrage side they are built for:
- **NO side:** `P0-converter` with `fee_bips = 0`, one per scenario: `colo-idle`, `sp-idle`, `sp-burst`, `sp-idle-nodelay`.
- **YES side:** `P1-settlement`, one per scenario: `colo-idle`, `sp-idle`, `sp-burst` (there is no `sp-idle-nodelay` P1 account).
- **Also read:** the `sp-idle` P0 accounts at `fee_bips` 10 and 50, for the fee column (§8) only.
- **Not used:** P2 (dormant) and P3 (control group; ignores fees and completeness).

**Episodes used.** `source = 'live'`, kinds `no_basket` and `yes_basket` (`yes_basket_incomplete` is never arbitrage), **net-positive only** (intents skipped as `not_net_positive`, `policy.rs:248-250`, are not attempts), runs whose `run_premises` match the card key. `engine_suspect` or `stale_transport` episodes are excluded and counted, as in 5a. Because of that exclusion, **SWEET does not reconcile with account equity**, and a note says so.

**States of an attempt** (exactly one):
- **`skipped`:** `paper_intents.skip_reason` is set. Captured 0, capital 0; counted by reason (`no_cash`, `below_min_size`, `consumed`, `too_long`, `no_lockup_figures`, `annualized_too_low`, `bad_input`).
- **`missed`:** executed with `paper_execs.q_eff = 0`. A missed attempt can still have partial buys and their unwinds (`trader.rs:605-647`), so its captured value is computed like any executed attempt, and is usually negative.
- **`filled`:** executed with `q_eff > 0`.
- **`dropped`:** no skip reason and no exec row, with its planned execution more than 60 s old (a restart drops pending intents, `trader.rs:94-95`). Counted as a skipped attempt with reason `dropped`; while under 60 s old it is `in_flight`.
- **`in_flight`:** executed less than 60 s before the tick's input cutoff. It is left out and picked up by a later tick, because the conversion, unwinds and 30 s markout may not be written yet.

**The statistic's unit is the executed attempt** (`missed` + `filled`). n and the mean count both; `filled` stays a separate count.

**Captured of an executed attempt**, exact, from the `paper_ledger` rows that carry this `(run_id, account_id, episode_id)`. `buy`, `sell` and `convert` rows carry it (`account.rs:185`, `:214`, `:264`); `settle` rows don't. All figures include fees, because `cash_delta` does.
- `cash_out` = −Σ `cash_delta` of its `buy` rows; `cash_back` = Σ `cash_delta` of its `sell` and `convert` rows.
- **Net quantity per leg** (market, outcome) = its `buy` qty − its `sell` qty − (for each S leg of a NO attempt) its `convert` qty.
- **Leftovers are common, not rare.** A sub-minimum or bookless unwind sells nothing (`trader.rs:699-708`). A restart drops pending unwinds (`trader.rs:94-95`, `close`). A FAK unwind can fill partly. A NO conversion is capped by what the account holds, pooled across episodes (`account.rs:236-241`).
- **YES side:** `sets` = min over S legs of the net quantity. `captured = cash_back − cash_out + sets × 1 + Σ leftover value`, where each leg's leftover is its net quantity − `sets`. The complete sets pay 1 each at resolution (L3). A non-binary resolution (Q19) is an existing integrity class, and a note says so.
- **NO side:** `captured = cash_back − cash_out + Σ leftover value`, where each leg's leftover is its net quantity. YES tokens returned by conversions are worth 0, the same convention as the paper accounts and the ★ rule.
- **Leftover value** = qty × the leg's **30 s markout mid** (`paper_markouts`, `offset_ms = 30000`, same outcome). The headline uses this. A **sensitivity at 0** (leftovers worth nothing) is carried alongside, and rows show both when they differ. With no 30 s markout (e.g. dropped by a restart), the leftover is valued at 0 in both and counted as `leftover_unpriced`.
- **Pooling.** Positions are pooled per leg across episodes in one account, so another episode's unwind or conversion can consume this episode's shares. Attribution follows the `episode_id` on each ledger row; a negative net quantity is clamped at 0 for valuation and counted as `pooled`. The account-level total is unaffected, and a test covers two episodes on one leg.
- **Capital:** `cost = cash_out`.
- **Lock time (YES only):** `episodes.days_to_end` at open. The NO side gets its money back within `settle_delay_ms` (2 s), so it is **not annualized** and reports return per $100 per trade instead.

**Edge on the table** for the same slice is the 5a definition (Σ `pnl_net_budget100_usd` over net-positive episodes), so every row can show "on the table vs captured". YES rows also carry **`on_table_eligible`**, the same sum over episodes P1 did not skip for one of its three filter reasons (`too_long`, `no_lockup_figures`, `annualized_too_low`; `policy.rs:255-263`). The YES capture rate is computed against `on_table_eligible`, never against the full figure.

## 5. Slices

**Always split by side** (YES and NO work differently) and **by scenario** (the latency axis, labelled with the scenario's `proc + net + match`: 52, 300, 450 ms, and `sp-idle-nodelay` 300 ms with secondsDelay ignored).

**One dimension at a time**, each with its own buckets:

| Dimension | Source | Buckets |
|---|---|---|
| Category | `events.tags` via `event_id` (§3; dropped on no-go) | the §3 buckets (9) |
| Days to resolution | `episodes.days_to_end` at open | [0, 1), [1, 7), [7, 30], (30, 90), [90, 365), ≥ 365, unknown |
| Set size S | count of `episode_legs.in_s` | 2, 3, 4–5, 6–10, > 10 |
| Hour of day (UTC) | `episodes.opened_server_ms` | 00–04, 04–08, 08–12, 12–16, 16–20, 20–24 |
| All | — | one row: the whole side |

The `[7, 30]` bucket is closed on the right to match P1's `days > max_days_to_end` test (`policy.rs:258`). Other edges are half-open. Days use exact decimals, and negative days fall in `unknown`.

## 6. Cells, premises and windows

- **Premise areas:** `episode_def`, `detector_yes`, `detector_no`, `fees`, `depth`, `sim`, `host`, the same as Edge capture. **No new area.** The scanner writes a `run_premises` row for every `PREMISES` entry (`bins/scanner/src/premises.rs:38-50`), and a family counts only runs with every one of its areas (`crates/cards/src/family.rs:24-37`). A `sweet` area would therefore match no past run, and bucket edits would need a scanner restart.
- **Card parameters instead.** `sweet.rs` renders its parameters (the bucket lists and edges, the minimums, the z table, the 60 s in-flight rule, the leftover valuation offset) into one canonical text, `SWEET_PARAMS`, next to a `SWEET_VERSION` const.
  - The family's key is the run-derived premise key plus the suffix **`;sweet=<SWEET_VERSION>+<sha8(SWEET_PARAMS)>`** (a `;`-separated segment like an area's, so 5a's key parsing, e.g. `host_from_key`, is unaffected). Any parameter edit gives a new key, which triggers the existing 5a catch-up rebuild from stored history, and different parameters never mix.
  - `SWEET_PARAMS` also goes into the reproducibility hash through the 5a `params` field (`crates/cards/src/body.rs:195-199`).
  - The hurdle is read from `policies.toml`, whose sha256 is already in the `sim` area.
- **Cell family `sweet`**, one per UTC hour per key, keyed by the episode's open hour. For each (side, scenario, dimension, bucket), the cell holds, sparsely (empty entries are omitted), only **additive** fields:
  - counts: episodes on the table, attempts, skipped by reason, missed, filled, `leftover_unpriced`, `pooled`, flagged-excluded;
  - exact decimal sums: on-the-table edge (all and P1-eligible), captured Σx, Σx² and Σ|x|, captured with leftovers at 0 (Σx₀), cost, and cost × days (YES);
  - **cluster sums** (§7.2): number of clusters C, Σcₖ, Σcₖ², Σcₖnₖ and Σnₖ², where a cluster is one (event_id, open hour) within the entry. Because a cluster lives in exactly one hour, all five merge by addition.
  - **No max or min.** 5a's `Windows.maxes` keeps a per-hour maximum for every hour forever (`crates/cards/src/window.rs:62-64`, `:226`), which is fine for Edge capture's two numbers but not for a few hundred entries an hour.
  - A property test checks merge(cells) equals the direct computation, as in 5a.
- **Dirty hours.** The tick runs every 900 s. The existing `updated_ms` cursor already re-dirties an episode's hours from its open to now whenever it changes, including at close (`bins/analyze/src/tick.rs:271-277`). Every tick also rebuilds the current hour and every hour since the last tick. On top of that, `sweet` rebuilds every hour touched by **[last tick − 10 min, now]**: an attempt still `in_flight` at one tick (its episode may have opened in an hour no rule above re-dirties) is always inside that range at the next.
- **Windows:** `24h`, `7d`, `30d`, `all`, as 5a. `today` does not include `SWEET`. The box defaults to **30d**, because shorter windows are mostly `thin`.
- **Memory.** A sweet cell is far larger than a 5a cell: up to ~160 entries an hour (each side-scenario pair touches at most 9 + 7 + 5 + 1 + 1 buckets in one hour), each about 30 numbers. The 5a `Windows` already bounds this: it keeps only `RECENT_HOURS = 48` cells in memory (`crates/cards/src/window.rs:37`), the 24h/7d/30d spans and `all` are running sums, and the cells leaving a span are loaded from `card_cells` when needed (`needed_ranges`, `advance`). `Max` for SWEET is the unit type, so the per-hour `maxes` map costs next to nothing. Peak memory is therefore about 48 cells plus 4 sums, a few MB. The §9 benchmark checks peak RSS anyway.
- **Status:** the 5a statuses (`live`, `warming`, `under_review`) apply to the card. `thin` applies per row (§7.1), not to the whole card.

## 7. Statistical honesty

### 7.1 Minimum sample
A row needs **n ≥ 30 executed attempts and C ≥ 30 clusters** to get any verdict. Below either, its label is `thin`: the numbers are shown dimmed with n and C, and it can't be a candidate.

### 7.2 Interval: clusters and multiple comparisons
- **Why clusters.** One event can open many episodes in one hour, and the attempts on them are not independent: the same books and the same competitors. Treating them as independent would make intervals too narrow.
- **Mean:** x̄ = Σcₖ / Σnₖ, the ratio estimator (equal to Σx / n).
- **Cluster-robust standard error**, float-free from the additive sums:
  SSR = Σcₖ² − 2x̄·Σcₖnₖ + x̄²·Σnₖ², and se = √(C / (C − 1) · SSR) / n.
  Computed with `rust_decimal` maths; `cards` declares the `maths` feature explicitly.
- **Interval:** x̄ ± z · se.
- **Bonferroni.** Every render counts m = the number of non-`thin` rows in the window, across all dimensions, sides and scenarios. z is taken for a two-sided family error of 5% over m rows from a fixed table (`sweet.rs`), using the next larger m, which is conservative:

  | m ≤ | 1 | 5 | 10 | 20 | 50 | 100 | 200 | 500 |
  |---|---|---|---|---|---|---|---|---|
  | z | 1.960 | 2.576 | 2.807 | 3.023 | 3.291 | 3.481 | 3.662 | 3.891 |

  The row count is bounded below 500 (at most 28 buckets × 7 side-scenario pairs = 196), so the table always covers m.
- The box states m and z in a footnote: *"m rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20."*
- **Few trades carry it.** Effective n = (Σ|x|)² / Σx². When it is below 5, the row is labelled *"few trades carry this row"* next to its verdict.

### 7.3 Why no cross slices
The 28 buckets (9 + 7 + 5 + 6 + 1) × 7 side-scenario pairs (4 NO, 3 YES) give ~200 one-way rows already. A full cross product would give thousands of rows, almost all `thin`, and Bonferroni would make the rest unreadable. One-way slices are the honest resolution for weeks of data. Cross slices can come back when 5b keeps more history.

### 7.4 Labels (per row; never "profitable")
- **`candidate`:** not `thin`, lower bound lo > 0, and, YES side only, the lower bound clears the hurdle once annualized: `lo × n / Σ(cost × days) × 365 ≥ hurdle`. Wording: *"survives here; worth a real small-order test."*
- **`negative`:** upper bound hi < 0. Wording: *"loses here. Don't trade this."*
- **`inconclusive`:** anything else that is not `thin`.
- **`thin`:** n < 30 or C < 30.
- **`not_tested`:** YES rows where P1 took nothing: every YES intent in the row was skipped as `too_long`, `no_lockup_figures` or `annualized_too_low`. In practice this means the (30, 90), [90, 365), ≥ 365 and `unknown` days rows, and low-edge slices. They still show edge on the table and its **upper-bound** annualized figure (§7.5).
- **`dead_on_paper`:** a `not_tested` row whose upper bound is below the hurdle. Wording: *"even first in line, the money is locked too long to be worth it."*

### 7.5 Capital lockup (YES side)
- Annualized return on locked capital, per row, capital-weighted: `(Σ captured / Σ (cost × days)) × 365`. It is NULL when Σ (cost × days) = 0.
- **The hurdle** is `p1.min_annualized` read from the same `policies.toml` the scanner runs (10% today), so the card and the policy can't disagree. Changing it changes the `sim` premise, which restarts the card.
- For `not_tested` rows, the upper bound is `episodes.annualized_return` weighted by `peak_cost_usd` over net-positive episodes with a non-NULL figure.

### 7.6 The paper caveat, on every view
A fixed note, shown under the box title and never collapsed: *"Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer."*

## 8. API and web

- **API:** no change to endpoints. `GET /api/cards?win=24h|7d|30d|all` returns `SWEET` with the 5a envelope (`status`, `premise_key`, `hash`, `data_from_ms`, `data_to_ms`, `n`, `notes`). Its `series` holds:
  - `rows[]`: `{ side, scenario, latency_ms, dimension, bucket, label, few_trades, n_exec, n_filled, n_missed, clusters, n_attempts, skipped: {reason: n}, on_table, on_table_eligible, captured, captured_leftover0, mean, lo, hi, eff_n, cost, annualized, annualized_lo, annualized_upper, leftover_unpriced, pooled }`, with all money as decimal strings or null;
  - `m`, `z`, `category_available` (false on a no-go);
  - `fees`: the NO side's Σ captured and mean at `sp-idle` for `fee_bips` 0, 10 and 50.
- **Web: a Sweet spots box on the Paper page, below Edge capture.**
  - A headline sentence per side at the headline scenario (`sp-idle`), with the **10 bips figure beside it**: *"NO: 0 candidates in 34 tested rows (at 10 bips: 0)."* or *"NO: 2 candidates: sports, 2 legs."*
  - A dimension switch (All · Category · Days · S · Hour) and a side switch (NO · YES). The table has rows = buckets and columns = scenarios, and each cell shows the label tag, the mean with its interval, n and C. YES cells add the annualized figure. The leftovers-at-0 figure shows on hover when it differs.
  - The colo-idle column is titled *"a co-located trader"*, so the latency gap reads directly.
  - Its own window switch, **30D by default** (Edge capture's switch keeps its 24H default; the two are independent).
  - (?) help: "What is a sweet spot?", "Why are most rows thin?", "What is Bonferroni, in one line?", "Why count events, not trades?", "Why is YES > 30 days not tested?".
  - No horizontal page scroll at 360 px: the table scrolls inside its box.

## 9. Performance, memory and disk

- **Reads per dirty hour:** the hour's episodes (existing), then for its episodes and the known (run, account) pairs: `paper_intents`, `paper_execs`, `paper_ledger` and 30 s `paper_markouts` by primary-key prefix; `episode_legs` by primary key; `events.tags` by primary key.
- **CPU:** the 5a benchmark (a tick over a synthetic 30-day history, < 200 ms CPU in a debug build) is re-run with `SWEET` included and must still pass. `SWEET` alone should add < 50 ms.
- **Memory:** the benchmark measures the serialized size of what `analyze` holds for SWEET (48 synthetic cells plus the four sums) and requires **< 16 MB**; the deploy checks `docker stats` for `analyze` stays **< 96 MB** (75% of the 128 MB cap) after catch-up.
- **Disk:** at most ~160 entries an hour, each a few hundred bytes of JSON: under ~60 KB an hour at the cap, so well under 1 GB a year even at the cap, and far less in practice. That is inside the 50 GB VPS budget.

## 10. Testing

- **Captured (unit):**
  - a NO attempt with conversion and a full unwind;
  - a YES attempt with the complete-set payout;
  - a **missed attempt with a partial buy and a partial unwind** (negative captured);
  - `skipped` and `not_net_positive` exclusion;
  - leftovers valued at the 30 s mid, the at-0 sensitivity, and `leftover_unpriced` without a markout;
  - **pooled leftovers: two episodes on the same leg in one account**, where one episode's conversion consumes the other's shares;
  - fees included;
  - `in_flight` excluded until 60 s old.
- **Buckets:** tag → bucket order, `untagged`, `other`; the closed `[7, 30]` edge and the other half-open edges for days, S and hours; negative days → `unknown`.
- **Statistics:**
  - the cluster SE against a hand-computed set, including equal to the plain SE when every cluster has one attempt;
  - the z table lookup (next larger m);
  - m counting excludes `thin`;
  - effective n and the "few trades" label;
  - every label rule, including all three P1 skip reasons → `not_tested`, `dead_on_paper`, the annualized lower-bound hurdle, and the NULL annualized case.
- **Merge property:** random attempts → merged hourly cells equal the direct computation for every window, including hours expiring out of the 30d span.
- **Parameters:** changing `SWEET_PARAMS` changes the family key and triggers catch-up; the hash includes it.
- **Store (real Postgres):**
  - migration 0007 is idempotent;
  - `analyze_rw` can read the six new tables and still can't write them;
  - the universe upsert writes `tags`, and **a second upsert with different tags leaves them frozen**.
- **pm-api:** an event with tags, without tags, and with malformed tags all parse; the go/no-go fixture parses.
- **Benchmark:** §9, CPU and the memory proxy.
- **Web:** each label renders; the paper caveat is always visible; the headline sentences for zero and some candidates, with the 10 bips figure; the no-go state without the Category switch; 360 px.

## 11. Open items for the plan

- The go/no-go request (§3) and the slug lists it fixes.
- Where exactly the `sweet` suffix joins the key in `family.rs` / `tick.rs` without changing the other families' keys (a test checks G1–G4 and EDGE keys are unchanged).

## 12. Decisions made without the owner (override any)

- **D1. Captured = per-attempt ledger cash plus the YES complete-set payout plus leftovers at the 30 s mid,** not equity deltas. Equity can't be split by episode, and settlement rows have no `episode_id`. Leftovers at 0 are shown as a sensitivity.
- **D2. Only the default-fee P0 (NO) and P1 (YES) accounts count.** P3 is a control and P2 is dormant. The 10 and 50 bips accounts appear only as the fee column.
- **D3. No `policies.toml` edit.** YES rows beyond P1's filters are `not_tested`. Adding an unfiltered P1 account would test them, but it resets every paper account. **Owner decided 2026-09-27: they stay untested.**
- **D4. One-way slices only;** no cross product (§7.3).
- **D5. n ≥ 30 executed attempts and ≥ 30 event-hour clusters** for any verdict; a cluster-robust interval; **Bonferroni** over all non-thin rows; the "few trades" label at effective n < 5. The review suggested considering 50 clusters; 30 is kept until real volumes are known, and the plan's deploy step re-checks it against the first 30 days.
- **D6. The hurdle is P1's `min_annualized` (10%),** shared with the policy, applied to the interval's lower bound.
- **D7. Category comes from Gamma tags on `events`, frozen at first sight,** joined by `event_id`; `episodes.category` stays unused. `untagged` means the event left the listing before the first refresh after deploy, or Gamma lists it without tags. On a no-go, the Category dimension is dropped.
- **D8. Hour of day in 4-hour UTC blocks** and days to resolution in 7 buckets, split at P1's 30 days, chosen for sample size, not elegance.
- **D9. The box lives on the Paper page below Edge capture,** with no new nav tab and its own window switch, defaulting to 30d.
- **D10. Default headline scenario `sp-idle`,** the same as Edge capture, with the 10 bips figure beside it.
- **D11. Parameters are versioned card-side** (key suffix plus hash), not as a premise area.

## 13. Review record (2026-09-27)

An independent review raised 3 blockers, 8 should-fix items and nits. All were checked against the cited code and accepted:
- **B1:** no `sweet` premise area (§6).
- **B2:** leftovers valued at the 30 s markout mid, with pooling (§4).
- **B3:** missed attempts count in the mean (§4).
- **S1–S8 and the nits:** cluster SE (§7.2), no max or min and effective n (§6, §7.2), the lower-bound hurdle (§7.4), days split at 30 and all three P1 skips (§5, §7.4), frozen tags (§3), the go/no-go (§3), no new indexes (§2), net-positive attempts only (§4), plus the nits (the z table ends at 500, the `maths` feature, D7's wording, the 900 s tick and re-dirty, the 10 bips figure, the equity note, the disk estimate, the extra tests).
- **One point was only partly taken:** the minimum-cluster threshold stays at 30, not 50 (D5).
- **One addition of our own:** a memory check for `analyze` (§9), because sweet cells are larger than 5a's. The existing 48-hour `recent` bound already keeps memory small (§6).
