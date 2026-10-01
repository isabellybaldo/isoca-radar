# isoca-radar Phase 7b: the public math playground, with leg risk and a live snapshot

**Date:** 2026-09-30. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.6 execution, §5.7 schematics, §19 Phase 7). **Base:** `2026-09-27-phase7-playground-explain-design.md` §1–6 and §8–12 (the "09-27 spec"). This spec keeps that design and changes it where 7a's result and the owner's 2026-09-30 decisions require. Where a section below says "as 09-27", the 09-27 text applies word for word, except where §11 amends it. **Builds on:** 7a (`2026-09-29-phase7a-latency-answer-design.md`): the ladder, `src/schematics/`, and the live answer "no rung pays; leg risk is why".
**Status:** approved in conversation, section by section (2026-09-30). The independent review's findings are folded in (§12).

## 1. Purpose, scope and success

`/math` is **public**, needs no login, and is portfolio first, teaching second (as 09-27). The playground shows how a basket arbitrage is found, sized and timed, and **why it still loses**. 7a found that on live Polymarket books no latency from 0 to 300 ms pays, because a leg that misses sells the rest of the basket at the bid. The 09-27 section e left unwinds out, so on its own it would teach the opposite of the live result. This spec adds section **f, "one leg misses"**, driven by the paper sim's own planning, fill and unwind rules, and a **live snapshot** of the scanner's results on the page.

**Changes from 09-27:**
1. **Section f** (§6), computed by new pure functions in `sim` that are extracted from `PaperTrader::execute`/`unwind`, plus the policy's existing `settle`. The playground runs the paper sim's arithmetic (§3).
2. **A public snapshot file** (§5): the ladder (with its category grid) and today's counts. The api writes it every 5 minutes into a volume, and nginx serves it as a static file. No public request reaches the api.
3. **Schematics reuse:** `PayoffGrid` (b), `DurationBars` (e) and `LatencyLadder` (f's live part) from 7a move into the public bundle, after the fixes in §7.2.
4. **Two plans:** 7b-A (Rust and the api writer) and 7b-B (web, routes, deploy), like 7a.

**Out of scope:** as 09-27, plus market names, market ids, token ids, episode ids, account ids or host names on any public path (§5.2), and any public request that reaches the api, postgres, analyze or the scanner.

**Exit:**
- `/math` loads without login on the VPS, and sections a–f work at 360 px and on desktop.
- The page shows the live snapshot, and says so honestly when the snapshot is stale or missing.
- CI's parity check is green, including `basket_json`.
- The replay identity check (§3.3) passed before merging 7b-A.
- The flood test (§9) leaves `web` inside its 32 MB, with `api`, `scanner` and `postgres` untouched, including while `/math-data/public.json` is being flooded.

## 2. Architecture

```
arb-core ─┬─► arb-wasm (JSON in, JSON out; decimals as strings)          api (login-only routes unchanged)
sim ──────┘      │  (+ basket_json → sim::policy::settle, sim::venue::{basket_buy, unwind_sell, basket_cash})
                 ▼                                                          │ every 5 min: PublicSnapshot
web/src/wasm/pkg (generated)                                                ▼
                 │                                    volume public-data/public.json (tmp + fsync + rename)
web/src/schematics/ a..f (pure views) ◄── math.html → src/math/ ◄── fetch /math-data/public.json (nginx static)
```

- **The facade holds no logic** (as 09-27). Everything that decides or computes money is in `arb-core` or `sim`, where the scanner and the paper sim use it.
- **The public snapshot is data, not an endpoint.** The only public paths are `/math`, `/math-assets/` and the exact file `/math-data/public.json`. nginx serves all of them from disk.
- **`sim` and `arb-core` already build for `wasm32-unknown-unknown`:** `cargo check -p sim -p arb-core --target wasm32-unknown-unknown` passes today, and neither uses time, fs or threads. CI keeps it that way (§8).

## 3. Rust additions (plan 7b-A)

### 3.1 `arb-core` additions

As 09-27 §3: `payoff`, `book::d1_sums`, `classify` and `walk_steps`, with their property tests. **Placement (plan 7b-A):** `fill.rs` and `detect.rs` are premise-locked, and editing them would restart every card and break the 0006 backfill test. So `walk_steps` lives in a new `arb_core::steps` and `classify` in a new `arb_core::classify`. `classify` reads the same conditions as the detectors, and a proptest checks that they agree. The extraction in §3.2 bumps the `sim` premise from 2 to 3, refreshed once in `premises.lock`, so Edge capture and SWEET start counting anew from the first scanner run on the 7b image.

### 3.2 Extracting the sim's fill arithmetic

The fill arithmetic of `PaperTrader::execute` and `PaperTrader::unwind` (`crates/sim/src/trader.rs`) moves into pure functions in `crates/sim/src/venue.rs`. The `Books` trait moves from `trader.rs` to `venue.rs`, and `trader` re-exports it, so `venue` never refers back to `trader`.

- `pub fn basket_buy(books: &impl Books, legs: &[FillLeg], q_target: Decimal, consumed: &mut ConsumedLiquidity) -> BasketBuy`:
  - `FillLeg { key: LegKey, fee: FeeSchedule, limit: Price }`.
  - Returns `{ per_leg: Vec<OrderFill>, q_eff, excess: Vec<Decimal> }`. `q_eff` is the smallest `filled`, or 0 with no legs. `excess[i] = per_leg[i].filled − q_eff`.
  - A leg whose book is missing gets an empty fill (as today).
- `pub fn unwind_sell(book: Option<&Book>, leg: &LegKey, fee: FeeSchedule, qty: Decimal, held: Decimal, min_order: Decimal, consumed: &mut ConsumedLiquidity) -> (Decimal, OrderFill)`:
  - It rounds **both** `qty` and `held` to 2 dp toward zero and requests `min(qty, held)`. The caller passes the raw held quantity; today the sim rounds it at `trader.rs:719`, and that rounding moves in here.
  - It returns the requested quantity and the fill. The fill is empty when the book is missing or the request is below `min_order`.
- `pub fn basket_cash(buy: &BasketBuy, unwinds: &[OrderFill], convert: Option<ConvertTerms>) -> BasketCash` returns `{ spent, fees, unwind_back, convert_back, net_cash, held_unsold: Vec<{ qty, basis }> }`:
  - `net_cash` is the sum of the `cash_delta`s the ledger would book for these rows (buys, sells, and the conversion when `convert` is given). `ConvertTerms { s, q, fee_bips }` values the conversion as `Account::convert` does: `(s − 1) · q · (1 − fee_bips/10000)`, with returned complement YES valued at 0.
  - `held_unsold` is excess never sold (below the minimum, a partial unwind, rounding dust or a missing book), with its basis. It is cost still held, not cash.
- `PaperTrader` keeps everything else: `pnl_now` (still called before `basket_buy`), reservations, delays, the `due` queue, markouts, conversions, ledger rows and output rows. It calls `basket_buy` and then emits the Fill and Ledger rows per leg in the same order as today. `Account::buy` doesn't touch `consumed`, so that order is unchanged.
- `sim::policy::settle` (today private) becomes `pub`, unchanged. `basket_json` uses it to derive `q_target` and the per-leg limits from the open book exactly as the policy does.

### 3.3 The identity guard for the extraction

- Every existing `sim` test passes unchanged, and so does `cargo test -p scanner` (`tests/replay.rs`, `engine_paper.rs`).
- New unit tests:
  - all legs fill;
  - one leg empty (`q_eff = 0`; every other leg's whole fill is excess);
  - partial fills;
  - an unwind below the minimum (zero fill, requested qty kept);
  - a missing book;
  - `held` needing rounding.
- New property test (random books, 2–6 legs): `q_eff ≤` every `per_leg.filled`, every `excess ≥ 0`, and an unwind never sells more than `min(round2(qty), round2(held))`.
- **`basket_cash` against the ledger:** for random fills, `net_cash` equals the sum of `cash_delta` over the ledger rows that `Account::buy`/`sell`/`convert` produce for the same fills, and the ledger's realized total equals `net_cash` plus the open basis (for a NO basket, the open basis is Σ `held_unsold.basis`). *(Corrected by plan 7b-A: the first draft's identity failed on a two-leg example.)*
- **The replay identity check (runbook step, before merging 7b-A):** run `scanner::replay::replay_dir_paper_with` over one busy raw hour at `main` and at the branch, through `scripts/replay-identity.sh`. All six paper row kinds (intent, exec, fill, ledger, markout, account) must be byte-identical, in emission order and sorted, and the frame and parse-error counts must match. *(Measured 2026-09-30 on the 2026-09-28 17:00 hour: 6,721,810 frames, 7,552 paper rows, IDENTICAL.)*

### 3.4 `arb-wasm`

As 09-27 §4, with one more function. **All book inputs are YES-book levels**, as the sim takes them: a NO buy walks the YES bids (mirrored), and a NO unwind sells into the YES asks (mirrored).

| Function | Input | Output (model) | Uses |
|---|---|---|---|
| `basket_json` | `kind` (`yes`, `no`); 2–4 legs, each with its YES book at **open** (bids, asks), at **`t_x`** (bids, asks) and at **unwind** time (bids, asks); fee `{rate, exponent}` or none; `q_request`; `min_order` (default 5, as the sim config); `convert_fee_bips` (default 0, as the ladder accounts; NO only) | `q_target` and per-leg limits from `settle`; per leg: the buy fill (filled, vwap, fees), the excess and the unwind fill; `q_eff`; `basket_cash` (spent, fees, unwind back, convert back, `net_cash`, `held_unsold`); for YES, the value at resolution if the basket's winner is any listed leg (`q_eff · 1`), labelled as not yet cash | `sim::policy::settle`, `sim::venue::{basket_buy, unwind_sell, basket_cash}` |

- `settle` returning an error, or a `q_target` of 0, gives `{"error": …}` with a plain sentence ("the book at open is too thin for 5 shares"). Nothing is clamped.
- **Limits:** as 09-27 (≤ 8 levels, 16 KiB, prices on the 0.01 grid). `basket_json` accepts 2–4 legs, matching what the section draws.
- **Fixtures** (`crates/arb-wasm/fixtures/basket_json.json`):
  - all fill; one leg empty; one leg partial;
  - excess below the minimum; the unwind book empty;
  - YES and NO; convert fee 0 and 50;
  - fees off; each limit ± 1.
- **Parity from the web side:** `web/src/wasm/parity.test.ts` reads the fixture files with node `fs` (as `theme.test.ts` reads `theme.css`). `vite.config.ts`'s `fs.allow` stays as it is.

## 4. The api writer (plan 7b-A)

- **What:** a tokio task started in `bins/api/src/main.rs` beside the server. Every `PUBLIC_EVERY_S` seconds (default 300, first run 30 s after start) it builds a `PublicSnapshot`.
- **Where the data comes from:** the `ladder` handler (`ladder.rs`) and the `today` handler (`episodes.rs`, `st.store.today(now_ms())`) each split into a `pub(crate) async fn load_*(&AppState) -> Result<Dto, ApiError>`, called by both the handler and the task. Neither depends on request state, so the snapshot can't drift from the dashboard.
- **Cost:** each run is about 10 queries (the ladder's plus today's), 12 runs an hour, whatever the traffic. The runs share the api's pool of 4, one of which LISTEN holds, and run one at a time.
- **Where the file goes:** `PUBLIC_DIR` (compose: `/var/lib/ct-public`).
  - The task writes `public.json.tmp` with `std::fs` inside `spawn_blocking` (the api's tokio has no `fs` feature), fsyncs the file, renames it to `public.json` and fsyncs the directory.
  - The api stays `read_only: true`, and `public-data` is its only writable mount.
- **Ownership:** `docker/rust.Dockerfile` pre-creates `/var/lib/ct-public` owned by 65532, beside `/data` and `/archive` (`mkdir -p … && chown 65532:65532 …` and a `COPY --from=builder --chown=65532:65532` line). A new named volume copies its ownership from the first container that mounts it. So the **api must mount it first**: 7b-A creates the volume and 7b-B adds the web mount. The 7b-A runbook checks that the volume root's owner is 65532.
- **Failures:**
  - A failed query logs one line (`api: public snapshot skipped: <reason>`) and leaves the old file.
  - A write error logs one line too, and the old file stays because the rename never happened.
  - The task never panics and never stops the server.
  - Without `PUBLIC_DIR` it doesn't start (local dev and tests).

## 5. The public snapshot

### 5.1 Shape (`PublicSnapshot`, typed serialize structs, fixed field order)

```
{ "v": 1, "written_ms": …,
  "ladder": { "epoch", "start_ms", "as_of_ms", "break_even",
              "measured": { "latency_ms" } | null,
              "markets_status",
              "rungs":   [ PublicRung ],
              "markets": [ PublicSweetRow ] },
  "today": { "day_start_ms", "arb", "no", "yes", "edge_budget100", "hourly": [ { "hour", "arb" } ] } }
```

- **`PublicRung`** is the api's `LadderRung` minus `account_id`: `scenario, latency_ms, extra, n_exec, n_leg_risk, n_missed, gains, leg_risk, settle, unrealized, net`, plus any other `LadderRung` field `LatencyLadder` reads. The plan checks this against the component. `measured.host` is dropped because it names the VPS.
- **`PublicSweetRow`** is built with `serde_json::from_value` from the SWEET body rows that `ladder.rs` already selects (they're raw `Value` clones today). It has exactly the fields `lib/sweet.ts` and `SweetLadderGrid` read, which include `fee_bips` (the grid filters on 0) and `clusters` (the cell line). The plan lists them from the code. Only dimensions `all` and `category` are kept. A `bucket` is a category name such as `crypto` or `sports`.
- **The ladder's shape matches the api's `Ladder`**, minus the dropped fields, so `lib/ladder.ts`'s `ladderModel` reads it through a small `publicToLadder` mapper (§7.2).
- **Bounded size:** `markets` keeps at most `PUBLIC_MAX_CATEGORIES = 20` category buckets, ranked by `n_exec` summed over the row's scenarios, plus the `all` rows. Rows are trimmed **before** serializing, so a data-driven category count can't push the file over 64 KiB and freeze it. If the file is still over 64 KiB, the run is skipped with a log line, which is a bug to fix, not a steady state.
- Decimals are strings, as in the api.

### 5.2 What never ships (tests)

- **Whitelist test:** it serializes a fully populated snapshot and compares every key path to a fixed list in the test. A new field fails until the list names it.
- **Content test:** the api's store fixture is seeded with recognisable market ids, token ids, event titles and account ids. The snapshot built from it must contain none of those strings, and none of the fixture host name.
- **Size test:** a snapshot with the `all` rows plus 20 categories, for what the grid draws (NO side, fee 0, the six LADDER scenarios), and 24 hourly rows serializes under 64 KiB (about 32 KB). `markets` holds only those rows. *(Corrected by plan 7b-A: both sides × every scenario came to about 82 KB.)*

### 5.3 Serving

- **nginx** (`docker/web/nginx.conf`):

  ```
  location = /math-data/public.json {
      root /srv/ct-public-root;   # the volume is mounted read-only at /srv/ct-public-root/math-data
      add_header Cache-Control "public, max-age=60";
      gzip on; gzip_types application/json;
      try_files $uri =404;
  }
  ```

  - Only that exact path is served from the volume. `public.json.tmp` and every other `/math-data/…` path fall to the dashboard location and require login at Traefik.
  - The file changes every 5 minutes and is written by the api, so it's gzipped per response (`gzip_static` can't apply). At about 10–30 KB behind the rate limits, that cost is negligible.
- **Traefik:** the full `ct-math` rule is:

  ``Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`) || Path(`/math-data/public.json`))``

  Its middlewares (per-IP rate and in-flight limits, `ct-sec`, the math CSP) and priority 1000 apply as in 09-27. `connect-src 'self'` allows the fetch.
- **Compose:** a named volume `public-data`. `api` mounts it read-write at `/var/lib/ct-public` (7b-A). `web` mounts it read-only at `/srv/ct-public-root/math-data` (7b-B). There's no new container and no new memory.

## 6. The playground page (plan 7b-B)

### 6.1 Layout

As 09-27 §6.1, plus:
- a **live strip** under the header: today's episodes (NO / YES), edge on the table, and "as of HH:MM UTC", drawn from `today`;
- sections a–f in order.

### 6.2 Sections

a–e as 09-27 §6.2, plus section e's caption: "caught is not filled: see f".

| | Starts from | Controls | Draws |
|---|---|---|---|
| f · one leg misses | a 3-leg NO basket with a positive edge at open; the books at `t_x` equal the open ones and all legs are deep enough (the default result: all fill, a small positive `net_cash`) | per leg: drain the depth at `t_x` (slider to 0); move the unwind book against us by 0–20 ticks (for NO, the YES asks rise, which is the NO bids falling); `q_request`; fees on or off; "the usual burst" preset (one leg empty at `t_x`, the unwind book 5 ticks against us) | per leg: a bar for planned vs filled; `q_eff` as a line across the legs; each leg's excess as a block, split into sold (at the unwind price) and `held_unsold`; the big line built from `net_cash` and what happened (amended 2026-10-01, see below). Then the **live ladder** from the snapshot (`LatencyLadder`, read-only, with its own "which markets" fold holding the category grid), headed "on the live scanner, as of …" |

- Every control change calls `basket_json` synchronously, once per animation frame while dragging (as 09-27).
- **f's big line (amended 2026-10-01).** The first draft chose between "caught, and lost $x" and "filled, $x on paper" by the sign of `net_cash`. That is false whenever cash is out but shares are still held, such as a fully filled YES basket or an unsold excess. The line now has two halves, and the page does no arithmetic:
  - **Word:** "caught" if any leg has excess; otherwise "nothing filled" if `q_eff` is 0; otherwise "filled".
  - **Money phrase:**
    - "and lost $x on paper" when the net is negative and nothing is held;
    - "$x out on paper, shares still held" when the net is negative and shares or a resolution value remain;
    - "+$x on paper" otherwise.
- **f's unwind wording** tells the two zero-fill cases apart: "under the minimum, no order went out" and "no buyer in the book".
- **f's caption** names `sim::policy::settle` and `sim::venue::basket_buy` / `unwind_sell` and says: "the same rules the paper accounts on the live scanner use; here the unwind happens at one instant, the live sim waits the account's delay".

### 6.3 The snapshot on the page

- **Fetching:** one `fetch('/math-data/public.json')` on load. It re-fetches when the tab becomes visible and the last fetch is over 5 minutes old. There is no polling loop.
- **Fresh** (`written_ms` within 30 minutes): the live strip and f's live part are drawn.
- **Stale:** the same drawings, plus "as of <time>: the live writer is behind".
- **Missing** (404, a network error, bad JSON or `v ≠ 1`): the live strip and f's live part are left out, and one dim line says "live numbers unavailable". The rest of the page never depends on the file.

## 7. Web build (plan 7b-B)

### 7.1 As 09-27

§4.2 (the wasm build, tsconfig split, `WASM_REQUIRED`), §4.4 (the 350 KB gzip budget and the module guard) and §5.1 (the second Vite build).

### 7.2 Making the 7a schematics public-safe

The module guard fails a build that pulls `src/pages/`, `src/api/` (values), `src/explain/` or `src/help.ts` into the math bundle. The review traced the imports: `Fold`, `Money`, `SweetLadderGrid` and `lib/{ladder,sweet,money,time}` import only types from `api/types`, and none imports `help.ts`. Three changes are needed:

1. **`DurationBars` imports `durationText` (a value) from `src/explain/models`.** `durationText` and the `DurationRow` and `PayoffModel` types move to `src/schematics/models.ts`, and `src/explain/models.ts` re-exports them.
2. **`LatencyLadder` renders `EmptyState`, which imports `api/client`,** with three different texts. The plain empty-state view moves to `src/schematics/Empty.tsx`, which has no api import and takes text. `LatencyLadder` uses it. `components/EmptyState` keeps its `ApiError`/session-expired handling for the dashboard and renders `Empty` inside.
3. **`LatencyLadder` keys rows by `account_id`,** which the snapshot drops. Rows are keyed by `scenario` instead (unique per ladder), and the component's rung type becomes `Omit<LadderRung, 'account_id'>`. `publicToLadder(PublicSnapshot['ladder']) → Ladder`-shaped input lets `ladderModel` stay unchanged.

Type-only imports from `src/api/types` are erased and allowed. The guard checks built module ids, so it proves all of this.

## 8. Testing

As 09-27 §8 (minus the panel items, which 7a covered), plus:
- **Rust:**
  - the §3.3 tests, and `cargo check -p sim -p arb-core -p arb-wasm --target wasm32-unknown-unknown` in CI;
  - `basket_json` fixtures and parity;
  - the §5.2 whitelist, content and size tests;
  - the writer:
    - bytes are written only to `public.json.tmp`, and the reader-visible file changes only by the rename;
    - a failure injected before the rename leaves the old file byte-identical;
    - a failed query keeps the old file;
    - there's no start without `PUBLIC_DIR`.
- **Web:**
  - section f renders the default and the burst preset with the real WASM;
  - the live strip for a fresh, stale and missing snapshot (a mocked fetch);
  - `publicToLadder` feeding `LatencyLadder`;
  - the module guard with the reused schematics;
  - 360 px with no horizontal scroll, plus the Chrome check after deploy (drawings over text, labels in HTML beside the SVGs, per the 7a lessons).

## 9. Deploy

- **7b-A** (the rust image, the api writer and the volume): runbook `docs/superpowers/runbooks/phase7b-api-vps.md`.
  - The replay identity check runs on the PC before merge.
  - Then the rust image is built and shipped. Deploy order (owner decision 2026-09-30: sim premise 3 starts at a known minute):
    1. stop analyze and compact;
    2. recreate `api` with the new mount while the scanner still captures;
    3. recreate the scanner alone in one command, and record its start as the premise-3 start;
    4. start analyze, then compact.
  - Nothing is installed on the VPS: the whitelist check runs on the PC.
  - Then, within 60 s, the file must be readable without the api image, which is distroless with no shell:

    ```
    docker run --rm -v cute-trading_public-data:/v:ro alpine sh -c 'stat -c %u /v; cat /v/public.json' | …
    ```

    The expected output is `65532`, then JSON with `v: 1` whose key paths match the whitelist (checked on the PC with python against the 48 paths of the Rust whitelist test).
- **7b-B** (the web image, the web mount and the Traefik labels): runbook `docs/superpowers/runbooks/phase7b-web-vps.md`.
  - The `curl` checks without a cookie (09-27 §8), plus:
    - `/math-data/public.json` returns 200 with `application/json`;
    - `/math-data/`, `/math-data/x.json` and `/math-data/public.json.tmp` redirect to login;
    - `curl --path-as-is /math-data/../public.json` redirects to login or returns 400.
  - The flood test (09-27 §5.5) also runs against `/math-data/public.json`.
- **The quiet-minute check is advisory in both runbooks.** If there's no quiet minute within 15 minutes, recreate anyway and count the reconnects (2026-09-30 showed 0).
- **Rollback:** a `pre7b` tag per image, tagged from the running container's image ID (7a-B's lesson).

## 10. Order of work

**7b-A:**
1. The `Books` move and the `basket_buy` / `unwind_sell` / `basket_cash` extraction, with tests; `settle` made pub.
2. The `arb-core` additions (09-27 §3).
3. `arb-wasm` with fixtures, native tests and the wasm32 check in CI.
4. The load-function split in the api, the snapshot types and the writer.
5. The Dockerfile directory and compose volume.
6. The replay script and the runbook.

**7b-B:**
1. The schematics fixes (§7.2).
2. The wasm build, Docker stage and web parity (09-27 §4.2, §4.3).
3. Views a–f.
4. The math page, its build and the bundle check.
5. nginx, the web mount and Traefik.
6. The runbook with the flood test.

## 11. Amendments to 09-27 kept "as 09-27"

- **§5.3:** "The page makes no other request" becomes "the page makes one other request, `GET /math-data/public.json` (§6.3)". "No `/api/` string ships" stays.
- **§5.4:**
  - "At most three static files, immutable except the HTML" becomes four files: the no-cache HTML, the immutable assets, and `public.json`, which is replaced every 5 minutes and gzipped per response (§5.3).
  - The memory argument is unchanged: nginx serves a file of at most 64 KiB from disk.
- **§4.3:** the web parity test reads fixtures with node `fs` (§3.4).
- **§11:** the `oha` question and the `wasm-bindgen` pin stay open. The `Position.end_date` item was done in 7a.

## 12. Changes after the independent review (2026-09-30)

- **Volume ownership:** the api runs as 65532 in distroless, so `/var/lib/ct-public` is pre-created and chowned in the image, the api mounts the volume first, and the runbook checks the owner (§4).
- **The net is cash, not realized:** `basket_cash` sums the ledger's `cash_delta` and reports `held_unsold` separately. YES value at resolution is shown as not yet cash (§3.2, §3.4).
- **`basket_json` plans like the policy:** open books go in, and `settle` derives `q_target` and the limits (§3.4).
- **The convert fee is an input,** and returned complements are valued at 0.
- **Inputs are YES-book levels,** and f's "unwind book against us" is defined for NO (§3.4, §6.2).
- **Snapshot fields match the components:** `PublicSweetRow` keeps `fee_bips` and `clusters` and is typed, `PublicRung` drops only `account_id`, and the component keys by `scenario` (§5.1, §7.2).
- **One grid, not two:** the category grid stays inside `LatencyLadder`'s fold.
- **`EmptyState` is split,** not replaced by a prop.
- **Markets are trimmed before the size check,** so the file can't freeze (§5.1).
- **The runbook reads the volume with an alpine container.** The curl traversal check uses `--path-as-is` and also covers `.tmp`.
- **The full Traefik rule is written out.**
- **09-27 §5.3 and §5.4 are amended** (§11).
- **Parity fixtures are read with node `fs`.**
- **Names:** `PaperTrader`; `Books` moves to `venue`; `unwind_sell` does the held rounding.
- **The byte-identity path:** `pnl_now` stays first, the row order is kept, and `cargo test -p scanner` is part of the guard.
- **The replay uses `replay_dir_paper_with`.**
- **The writer runs `std::fs` in `spawn_blocking`,** and the pool use is stated.
- **The wasm32 check is in CI.**
- **The content test seeds ids;** the atomic-write test tests the tmp-then-rename sequence.
