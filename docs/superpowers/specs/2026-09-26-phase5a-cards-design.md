# isoca-radar Phase 5a: live paper cards (Gebele), Edge capture, readability, design

**Date:** 2026-09-26. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§4, §5.5, §11, §12, §14, §19 Phase 5). **Builds on:** Phase 4 (`2026-09-25-phase4-dashboard-design.md`), S3c paper trading.
**Status:** approved in conversation, section by section. Mockups: `mockup-papers.html` (Papers page) and `mockup-fonts-fantasy.html` (the font choice was **C, IBM Plex Mono**; the box shown there is renamed **Edge capture**, see §6).

## 1. Purpose, scope and success

A **live** Papers page and an **Edge capture** box, on the existing login-only dashboard, for portfolio, monitoring and research in equal measure. The owner is a senior engineer and wants something that reads as professional work: precise labels, honest methods, a deadpan voice kept to subtitles.

**In scope (5a):**
- A card framework: live rolling figures, premise versions (§4), status, reproducibility hashes.
- Gebele et al. 2026 cards G1–G4 (§5), computed from the `episodes` table.
- Edge capture: edge on the table versus edge captured by latency (§6), on the Paper page and as a teaser in the Radar's today box.
- A measured-latency marker from where the scanner runs (§6.3), plus the scanner host as a premise.
- (?) help on every section of every page (§8.3).
- A readability pass: body text in **IBM Plex Mono**, and a responsive pass with no horizontal scroll at 360 px (§8.4). It applies to all existing pages.

**Out of scope:**
- **External storage**, the next sub-project (**5b**), which comes *soon*: it archives raw capture older than 2–3 days and holds the Saguillo dataset.
- **Saguillo S-cards** (5c, needs 5b).
- **Cheng C-cards:** dropped at the owner's call; not a data problem.
- **Laya and card S4:** Phase 6, deferred.

**Exit:** on the VPS, the Papers page shows G1–G4 and the Edge capture box shows live figures for 24 h / 7 d / 30 d / all. Every card carries a status and a hash. The `analyze` job stays inside its CPU cap, as the §9 benchmark and the VPS `docker stats` show.

**Changes from the parent spec:**
- §12 planned *snapshot* cards run "local"; they are now **live** and run on the VPS.
- G3 is redefined after reading the paper (§5.3).
- The Papers page arrives in 5a, not in Phase 7.

## 2. Architecture

```
scanner ──upsert (sets updated_ms)──► episodes, runs, run_premises, health (+ REST RTT)
                                          │ indexed reads: changed rows only
                                          ▼
analyze (every 15 min; 0.25 CPU, 128 MB) ──► card_cells (hourly, merge-able) ──merge──► card_results
                                                                                          │ SELECT only
api (GET /api/cards, /api/cards/meta) ◄──────────────────────────────────────────────────┘
web: Papers page, Edge capture box (Paper), teaser (Radar today)
```

- **`analyze`** is a new binary, `bins/analyze`, in the existing Rust image. It runs as its own compose service in the `core` profile with `cpus: 0.25` and `mem_limit: 128m`. It is read-only filesystem, non-root, no-new-privileges and cap_drop ALL, like `api`.
  - It has no network client. The arch test bans `reqwest`, `pm-api` and `tokio-tungstenite`, the same as for `api`.
  - It reaches Postgres only through the `store` crate (§9.3 rule unchanged).
- **The dashboard `api` stays read-only.** It gains SELECT on `card_results` and `card_meta` through the existing `analyze_ro` role.
- **A new role, `analyze_rw`:**
  - SELECT on `episodes`, `runs`, `run_premises`, `health`, `paper_marks` and `paper_accounts`;
  - INSERT, UPDATE and DELETE on `card_cells`, `card_results` and `card_meta` only;
  - a statement timeout of 10 s.
  - A startup check refuses to run if the role can write any other table, in the pattern of the api's refusal.

## 3. Data (migration 0006)

- **`episodes.updated_ms BIGINT NOT NULL DEFAULT 0`**, with index `episodes_updated (updated_ms)`.
  - The store's episode upsert sets it to the writer's wall clock on every insert and update.
  - Existing rows get `opened_server_ms`, so the first `analyze` start sees them.
- **`run_premises(run_id BIGINT REFERENCES runs, area TEXT, version TEXT, PRIMARY KEY (run_id, area))`.** The scanner writes one row per area at run start (§4).
- **`health.rest_rtt_p50_ms INTEGER NULL` and `health.rest_rtt_p99_ms INTEGER NULL`.** Per-minute percentiles of the scanner's own REST request durations. `pm-api` times requests it already makes, so no new traffic is added.
- **`card_cells(card TEXT, premise_key TEXT, hour_ms BIGINT, data JSONB NOT NULL, computed_ms BIGINT NOT NULL, PRIMARY KEY (card, premise_key, hour_ms))`.**
  - One merge-able summary per card per UTC hour (§7).
  - `data` holds exact decimals as strings and integer counts, never floats.
- **`card_results(card TEXT, win TEXT, premise_key TEXT, status TEXT, body JSONB, computed_ms BIGINT, PRIMARY KEY (card, win))`.**
  - `win` is one of `24h`, `7d`, `30d`, `all`, plus `today` for Edge capture only.
  - `body` is the full API payload for that card and window (§8.1).
- **`card_meta(key TEXT PRIMARY KEY, value JSONB)`:** the last tick time, the analyze version, catch-up progress, and any last error.
- **Backfill:** `run_premises` rows for past runs that match the current logic.
  - The candidate list is soak run 3 (`run_id = 7`, image `2c5b84d`) and any later live runs.
  - The migration takes this list as literal run ids, and **the owner approves the list** when the plan is written.
  - Runs 1–6 are never backfilled (the phantom-level loop and drops), so no card counts them.

**Disk:** a cell is a few KB, so a year of history is about a few hundred MB. That is well inside the VPS's **50 GB** project budget (raised from 30 GB, 2026-09-26). The PC's budget stays at 30 GB.

## 4. Premise versions (per-card choice, falling back to restart)

- **Areas and versions** live in one file, `crates/arb-core/src/premises.rs`, as `const PREMISES: &[(area, version, files)]`. The first areas:
  - `episode_def`: frame-level Π>0 runs, open and close;
  - `censoring`: the left, right and gap rules;
  - `detector_yes` and `detector_no`;
  - `fees`;
  - `depth`: the depth walk and Q*;
  - `sim`: the paper trader, for Edge capture;
  - `host`: §6.3.
- **Config enters premises.** An area's effective version is `"{code_version}+{sha256_8(config_section)}"`. A changed fee config therefore behaves like a code change.
- **Enforcement.** A `premises.lock` file records a sha256 of each area's source files, taken with `\r` stripped so Windows and CI agree. A test recomputes the hashes and **fails if any file changed while the area's version didn't**. The fix is to bump the version and refresh the lock with a small command. Reviewers check that a bump is justified.
- **Runs record premises.** At start, the scanner writes `run_premises` for every area. `host` is the label from the `SCANNER_HOST` env var (e.g. `vps-kvm2`, `pc-dev`), plus the measured-latency band (§6.3).
- **Cards declare areas.** A card's premise key is its sorted `area=version` list. Cells and results are keyed by it, and only runs whose `run_premises` match the key count. **Different premises never mix.**
- **Status**, one per card per window:
  - `live`: the window is fully covered on the current key.
  - `warming`: the current key's data starts inside the window. The card shows "since ⟨date⟩ · ⟨x⟩ days of data".
  - `thin`: the sample is too small. The thresholds are fewer than 30 uncensored durations for a median, and fewer than 10 episodes on a side for a ratio. The number is shown dimmed, with the reason.
  - `under_review`: the key changed and catch-up is still building cells.
- **Reproducibility hash:** sha256 of the premise key, the analyze version, the window's data range (first and last hour), and the card's parameter set. It is shown shortened, with the full value on hover.

## 5. The Gebele cards

Source: Gebele, Mutzel, Matthes, *Executable Arbitrage and Market Efficiency in Prediction Markets*, arXiv 2608.00666 v1. It was read on 2026-09-26, with quotes kept in the plan.

The CLOB sample was hour-stratified, 14 Apr 2026 00:00 UTC to 19 May 2026 06:00 UTC. An episode is a maximal run of event-time states with the **fee-adjusted edge strictly > 0**, assuming **taker execution**, with no flicker merge. The paper keeps the largest edge across subsets per event, direction and update.

**Every card shows three columns:**
- **PAPER:** their number.
- **THEIR METHOD:** their definitions on our data.
- **OUR METHOD:** our headline definitions.

It also shows n, a method note, its status and its hash.

**Episodes counted:**
- live source only;
- kinds `yes_basket` (YES side) and `no_basket` (NO side), the scanner's `kind_str`; `yes_basket_incomplete` is excluded (realization `not_arbitrage`), and `implication` belongs to no side;
- excluding `engine_suspect` and `stale_transport` under OUR METHOD, and noting the count excluded.

### 5.1 G1 — side asymmetry
- **PAPER:** 2,098 positive YES-side vs 36 NO-side episodes.
- **THEIR METHOD:** our raw episodes (already net of taker fees, >0, no merge), counted per side.
- **OUR METHOD:** the same, plus the merge-gap sensitivity from the parent spec §5.5 (0, 250 ms, 1 s) and the gross counts as context.
- **Chart:** two pixel bars and the ratio.
- **Areas:** `episode_def`, `detector_yes`, `detector_no`, `fees`.

### 5.2 G2 — durations and survival
- **PAPER:**
  - medians of 16.15 s (YES, n=253) and 7.99 s (NO, n=5; the paper says this "does not support a reliable distributional comparison");
  - window-spanning episodes excluded;
  - Ŝ(τ) = K⁻¹ Σ 𝟙{D_k > τ}.
- **THEIR METHOD:** the empirical Ŝ over **uncensored** episodes; the median per side.
- **OUR METHOD:**
  - Kaplan–Meier with right-censored episodes;
  - left-censored episodes excluded and counted;
  - gap-censored episodes reported separately with bounds (parent §5.5).
- **Chart:** step curves of both estimators per side, with the medians marked.
- **Areas:** `episode_def`, `censoring`, `detector_yes`, `detector_no`, `fees`.

### 5.3 G3 — long-lived share (redefined)
- **PAPER:** "Of the 624 observed episodes, 366 (58.7%)" were observed during both the first and the final five minutes of their one-hour window (≥ 50 minutes). The paper does not state how its hours were chosen.
- **THEIR METHOD:** cut our continuous record into clock hours, and count the episodes present in both the first and the last 5 minutes of an hour, over all episodes observed in that hour.
- **OUR METHOD:** the share of episodes with duration ≥ 50 min, using KM for censored ones, and the duration bias that one-hour windows introduce, measured as the median of window-clipped durations minus the true median.
- **Areas:** as G2.

### 5.4 G4 — net edge by side
- **PAPER:** Figure 2 shows the distributions with insets of positive and negative counts. The text states no units, bins or percentiles, so the paper column shows **the counts only** and is marked "figure only".
- **OUR METHOD:**
  - histogram of peak net edge per side in bps (fixed 25 bps bins, with an open-ended top bin), gross as an overlay;
  - median and p90 per side.
- **Areas:** `episode_def`, `detector_yes`, `detector_no`, `fees`, `depth`.

### 5.5 Not replicated
The converter-profit figures come from on-chain conversions, which we don't observe. So do the same-condition findings of the paper's WebSocket validation appendix (20 inconsistencies in 308,416,666 messages). A "Not replicated" box on the Papers page says so. The Health page already reports our own integrity rate.

## 6. Edge capture

**Idea (implementation shortfall).** Opportunities are **measured live**. The only hypothetical is execution: being first in line at the best moment. The box shows the edge that was there, how much each latency would capture, and where the gap goes.

### 6.1 Figures (all exact decimals; windows 24h / 7d / 30d / all, plus `today` UTC)
- **Edge on the table:**
  - the sum of `pnl_net_budget100_usd` over net-positive NegRisk episodes, counted at the peak with a $100 cap per trade;
  - split into **NO side, cashable before settlement** and **YES side, locked until resolution**;
  - labelled "upper bound: assumes first-in-line execution; CLOB queue ignored".
- **Captured by latency:** each latency scenario's P0-converter paper account, with its equity change over the window from `paper_marks`. The scenarios from `policies.toml` today:
  - `colo-idle` (1 ms network, 50 ms match);
  - `sp-idle` (145 ms, 150 ms);
  - `sp-burst` (145 ms, 300 ms);
  - `sp-idle-nodelay` (the secondsDelay sensitivity).
- **Capture rate:** captured ÷ on the table, per scenario. The headline is `sp-idle`. It shows as "—" when on the table is 0.
- **Capital needed at once:** the peak sum of the capped costs of overlapping episodes. Each hourly cell stores the maximum over the episodes overlapping that hour, including ones opened earlier, so the window maximum is exact.
- **Biggest single opportunity:** the largest capped peak in the window, linked to its episode.
- **Areas:** `episode_def`, `detector_yes`, `detector_no`, `fees`, `depth`, `sim`, `host`.

### 6.2 Where it shows
- **Paper page:** an **Edge capture** box at the top. It contains:
  - the edge-decay chart: latency on the x axis, capture rate on the y axis, one point per scenario;
  - the measured marker (§6.3);
  - the split and the capital figure;
  - the subtitle *"The difference is physics and other people."*
- **Radar today box:** *"Edge on the table today: +$X · captured at 145 ms: Y%."*

### 6.3 Measured latency and the host premise
- **Measured marker.** The measured latency is the median over the window of `health.lag_p50_ms` (feed lag) plus `health.rest_rtt_p50_ms / 2`. It is shown on the decay chart as **"measured: ⟨host⟩"** and is labelled an estimate.
- **Host premise.** `host` = `SCANNER_HOST` plus a band of measured latency: <25, 25–100, 100–250, or >250 ms, taken from the first 10 minutes of the run.
  - Moving the scanner, or a band change at restart, restarts only the latency-dependent cards (Edge capture).
  - The Gebele cards don't depend on `host`.
- **Paper scenarios stay unchanged in 5a.** Editing `policies.toml` resets all paper accounts (S3c ruling 26). A measured scenario can be added later, on purpose.

## 7. The analyze job

- **Tick every 15 min** (`ANALYZE_EVERY_SECS`, default 900, range 60..=3600). Each tick:
  1. Reads the episodes with `updated_ms > cursor` (the cursor lives in `card_meta`). It marks their hours, `opened_server_ms` floored to the hour plus every hour they span, as dirty.
  2. Rebuilds the dirty cells and the current partial hour, per card and per premise key.
  3. Merges cells into windows: 24 cells for 24h, 168 for 7d, 720 for 30d, every cell of the key for all, and UTC midnight→now for today. It then upserts `card_results`.
  4. Writes the `card_meta` tick time.
- **The cell maths is merge-exact:**
  - durations as `duration_ms → (events, censored)` count maps, so KM is exact;
  - side counts;
  - edge bins;
  - decimal sums;
  - the maximum overlap;
  - the long-lived counts.
  - A property test (§9) checks that merge(cells) equals the direct computation.
- **Catch-up** runs on first start and after a new premise key. It builds cells from history oldest first, 24 hours per batch, sleeping 2 s between batches. The status is `under_review` until done.
- **Late closes** (an episode closing hours after it opened) are picked up by `updated_ms`, never missed.
- **Budget.** A normal tick is expected to take milliseconds. Hard limits: the §9 benchmark, the container's CPU cap, and a statement timeout on every query.
- **Failure.** A failed tick logs one line, records the error in `card_meta`, and retries next tick. Results keep their last values, and the page shows "updated ⟨n⟩ min ago" in `thin` style if older than 45 min. Nothing crash-loops: `restart: on-failure:5` applies, as for the scanner.

## 8. API and web

### 8.1 API (read-only; same login; decimal strings; ms times)
- **`GET /api/cards?win=24h|7d|30d|all|today`:** every card for that window. Each card carries:
  - `card`, `paper` and `title`;
  - `status`;
  - `premise_key` and `hash`;
  - `data_from_ms` and `data_to_ms`;
  - `n`;
  - `columns`: paper, their method, our method, as decimal strings or null;
  - `series`: survival steps, bins, decay points;
  - `notes`.
  - `win=today` returns Edge capture only. Any other value is a 400.
- **`GET /api/cards/meta`:** the last tick, the analyze version, catch-up progress, the paper list with each paper's status (`live`, `coming_later`, `dropped`), and the not-replicated items.

Both endpoints read `card_results` and `card_meta` only: one small query each.

### 8.2 Pages
- **Papers:**
  - a new tab between Paper and Health;
  - a window switch, 7D by default;
  - one section per paper: Gebele live; Saguillo "coming later: needs the on-chain dataset and somewhere to put it that is not your laptop";
  - the cards in a two-column grid, one column on phones;
  - a "Not replicated" box.
  - It follows the approved mockup.
- **Paper:** the Edge capture box at the top (§6.2).
- **Radar:** the teaser line in the today box.

### 8.3 (?) help
- Every section of every page gets a **(?)** button that opens a short plain-language explanation. Examples:
  - "What is an episode?"
  - "Why is the NO side cashable and the YES side locked?"
  - "What is Kaplan–Meier?"
  - "What does warming mean?"
- The button toggles on click, Enter or Space, and closes on Escape.
- The popover is announced to screen readers (`aria-expanded`, `aria-controls`).
- The copy lives in one file, `web/src/help.ts`, in the deadpan-but-clear voice.

### 8.4 Readability and responsive pass (all pages)
- **Fonts:**
  - **IBM Plex Mono** for body text and tables, 15–16 px with line-height ≥ 1.5;
  - Press Start 2P for headings and tags;
  - VT323 only for big numbers.
  - The fonts load from Google Fonts, so the existing CSP needs no change.
- **Responsive:**
  - no horizontal page scroll at 360 px on any page;
  - wide tables scroll inside their own box;
  - charts scale to the container.

## 9. Testing

- **Statistics (unit, Rust):**
  - KM on a textbook dataset with a known answer, including ties and censoring;
  - the empirical Ŝ;
  - medians, bins and p90;
  - the long-lived share;
  - Edge capture sums, the split, capture rates (including a zero denominator), and the maximum overlap across hour boundaries.
- **Merge property:** for random episode sets, merged hourly cells equal the direct computation for every card and window.
- **Premises:**
  - the lock test fails on an unbumped change;
  - a new key gives `warming` or `under_review` and never mixes;
  - runs without `run_premises` are never counted.
- **Store and roles** (real Postgres, `TEST_DATABASE_URL`):
  - `analyze_rw` can't write `episodes`, `runs`, `health` or paper tables;
  - `analyze_ro` (the api) still can't write at all;
  - `updated_ms` advances on update;
  - migration 0006 is idempotent.
- **analyze:** a tick over seeded episodes produces the expected `card_results`; late closes are picked up; catch-up batches and pauses.
- **Benchmark:** one tick over a synthetic 30-day history (~250k episodes, about 350/hour) must use **< 200 ms CPU** in a debug build on the PC, with the first-start catch-up measured separately.
- **API:** the shape of each window, 400 on a bad `win`, decimals as strings, and null distinct from "0".
- **Web (Vitest):**
  - each card renders each status;
  - Edge capture and the Radar teaser copy;
  - (?) works by keyboard and is announced;
  - no float formatting of money;
  - the existing tests stay green after the font and responsive changes.
- **CI:** the existing jobs cover `bins/analyze`; the `premises.lock` test runs in CI.

## 10. Deploy

- **One bundle after the soak** carries:
  - migration 0006;
  - the new image, with scanner (host label, REST RTT), analyze and api;
  - the web image;
  - the compose changes: the `analyze` service in `core`, and the `SCANNER_HOST` env.
  - It also carries the capture housekeeping fix (`cc51942`).
- **Runbook additions:**
  - set `SCANNER_HOST=vps-kvm2` and a new `ANALYZE_RW_DB_PASSWORD` key in the VPS `.env`;
  - activate the role once. Migration 0006 creates `analyze_rw` as NOLOGIN, because a migration can't carry a secret. Then run `docker compose exec postgres psql -U ct_owner -d cute_trading -c "ALTER ROLE analyze_rw LOGIN PASSWORD '…'"`, with the password from the VPS `.env`. The runbook reads it into a variable and never prints it. `docker/postgres/init/01-roles.sh` also gains the role for fresh volumes;
  - approve the backfill run list;
  - check `docker stats` for `analyze` after the first catch-up.

## 11. Open items for the plan

- **Backfill list, approved 2026-09-26:** soak run 3 onward, i.e. live scanner run `run_id = 7` (image `2c5b84d`) and every later **live** scanner run that exists when 0006 is applied. Replay, safe, census and breaker_reset runs are never backfilled. The plan's migration selects `run_id >= 7 AND service = 'scanner' AND mode = 'live'`.
- The KM textbook dataset used as the test oracle (e.g. a small published example with ties).
- The exact thresholds for `thin`, checked against real 7-day volumes after the first deploy.
