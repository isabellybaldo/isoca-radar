# isoca-radar Phase 7b: the public math playground, with leg risk and a live snapshot

**Date:** 2026-09-30. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.6 execution, §5.7 schematics, §19 Phase 7). **Base:** `2026-09-27-phase7-playground-explain-design.md` §1–6 and §8–12 (the "09-27 spec"). This spec keeps that design and changes it where 7a's result and the owner's 2026-09-30 decisions require. Where a section below says "as 09-27", the 09-27 text applies word for word. **Builds on:** 7a (`2026-09-29-phase7a-latency-answer-design.md`): the ladder, `src/schematics/`, and the live answer "no rung pays; leg risk is why".
**Status:** approved in conversation, section by section (2026-09-30).

## 1. Purpose, scope and success

`/math` is **public**, needs no login, and is portfolio first, teaching second (as 09-27). The playground shows how a basket arbitrage is found, sized and timed, and **why it still loses**. 7a found that on live Polymarket books no latency from 0 to 300 ms pays, because a leg that misses sells the rest of the basket at the bid. The 09-27 section e left unwinds out, so on its own it would teach the opposite of the live result. This spec adds section **f, "one leg misses"**, driven by the paper sim's own unwind rule, and a **live snapshot** of the scanner's results on the page.

**Changes from 09-27:**
1. **Section f** (§6), computed by a new pure `sim::venue::basket_fill`, extracted from `Trader::execute`/`unwind` so the playground runs the paper sim's arithmetic (§3).
2. **A public snapshot file** (§5): the ladder, the category grid and today's counts. The api writes it every 5 minutes into a volume, and nginx serves it as a static file. No public request reaches the api.
3. **Schematics reuse:** `PayoffGrid` (b), `DurationBars` (e) and `LatencyLadder` (f's live line) from 7a move into the public bundle. This needs two import fixes (§7.2).
4. **Two plans:** 7b-A (Rust and the api writer) and 7b-B (web, routes, deploy), like 7a.

**Out of scope:** as 09-27, plus market names, episode ids or account ids on any public path (§5.2), and any public request that reaches the api, postgres, analyze or the scanner.

**Exit:**
- `/math` loads without login on the VPS, and sections a–f work at 360 px and on desktop.
- The page shows the live snapshot, and says so honestly when the snapshot is stale or missing.
- CI's parity check is green, including `basket_json`.
- The replay identity check (§3.3) passed before merging 7b-A.
- The flood test (§9) leaves `web` inside its 32 MB, with `api`, `scanner` and `postgres` untouched, including while `/math-data/public.json` is being flooded.

## 2. Architecture

```
arb-core ─┬─► arb-wasm (JSON in, JSON out; decimals as strings)          api (login-only routes unchanged)
sim ──────┘      │  (+ basket_json → sim::venue::basket_fill)                │ every 5 min: PublicSnapshot
                 ▼                                                          ▼
web/src/wasm/pkg (generated)                                  volume public-data/public.json (atomic rename)
                 │                                                          │ read-only mount
web/src/schematics/ a..f (pure views) ◄── math.html → src/math/ ◄── fetch /math-data/public.json (nginx static)
```

- **The facade holds no logic** (as 09-27). Everything that decides or computes money is in `arb-core` or `sim`, where the scanner and the paper sim use it.
- **The public snapshot is data, not an endpoint.** The only public paths are three routes: `/math`, `/math-assets/` and the exact file `/math-data/public.json`. nginx serves all of them from disk.

## 3. Rust additions (plan 7b-A)

### 3.1 `arb-core` additions

As 09-27 §3: `payoff`, `book::d1_sums`, `detect::classify`, `fill::walk_steps`, with their property tests.

### 3.2 `sim::venue::basket_fill` (new, extracted)

The fill arithmetic of `Trader::execute` (`crates/sim/src/trader.rs`, the FAK buys, `q_eff` and the excess per leg) and of `Trader::unwind` (the capped, rounded FAK sell below the venue minimum) moves into two pure functions in `venue.rs`:

- `pub fn basket_buy(books: &impl Books, legs: &[FillLeg], q_target: Decimal, consumed: &mut ConsumedLiquidity) -> BasketBuy`, returning `{ per_leg: Vec<OrderFill>, q_eff, excess: Vec<Decimal> }`. `FillLeg { key: LegKey, fee: FeeSchedule, limit: Price }`. `q_eff` is the smallest `filled`, or 0 with no legs. `excess[i] = per_leg[i].filled − q_eff`.
- `pub fn unwind_sell(book: Option<&Book>, leg: &LegKey, fee: FeeSchedule, qty: Decimal, held: Decimal, min_order: Decimal, consumed: &mut ConsumedLiquidity) -> (Decimal, OrderFill)`, returning the quantity requested after rounding to 2 dp toward zero and capping at `held`, and the fill. The fill is empty when the book is missing or the quantity is below `min_order`.

`Trader` keeps everything else: reservations, delays, the `due` queue, markouts, conversions, ledger rows and output rows. Its outputs must not change.

### 3.3 The identity guard for the extraction

- Every existing `sim` test passes unchanged.
- New unit tests: all legs fill; one leg empty (`q_eff = 0`, every other leg's whole fill is excess); partial fills; an unwind below the minimum (zero fill, requested qty kept); a missing book.
- New property test (random books, 2–6 legs): `q_eff ≤ every per_leg.filled`, `excess ≥ 0`, and an unwind never sells more than `min(qty, held)`.
- **The replay identity check (runbook step, before merging 7b-A):** replay one busy raw hour through the paper sim at `main` and at the branch. The `fills`, `exec` and ledger outputs must be byte-identical (compared with a sorted dump plus `sha256sum`).

### 3.4 `arb-wasm`

As 09-27 §4, with one more function:

| Function | Input | Output (model) | Uses |
|---|---|---|---|
| `basket_json` | `kind` (`yes`, `no`); 2–4 legs, each with ask levels and bid levels at `t_x`, and bid levels at unwind time; fee `{rate, exponent}` or none; `q_target`; `min_order` (default 5, as the sim config) | per leg: the buy fill (filled, vwap, fees), the excess and the unwind fill; `q_eff`; `leg_risk_qty`; cash spent, cash back from unwinds, the value of `q_eff` baskets at payoff (1 for YES; for NO, `s − 1` via conversion); **net** = payoff value + unwind cash − spent − fees | `sim::venue::basket_buy`, `sim::venue::unwind_sell`, `payoff` |

- **Net is computed in `sim` or `arb-core`, not in the facade.** A `sim::venue::basket_net(&BasketBuy, &[unwind fills], payoff)` helper sums it, and `Trader`'s ledger stays the source for the live sim. A test checks that `basket_net` equals the ledger's realized total for the same fills.
- **Limits:** as 09-27 (≤ 6 legs, ≤ 8 levels, 16 KiB, prices on the 0.01 grid, errors never clamped). `basket_json` accepts at most 4 legs, matching what the section draws.
- **Fixtures** (`crates/arb-wasm/fixtures/basket_json.json`): all fill; one leg empty; one leg partial; excess below the minimum; unwind book empty; fees off; each limit ± 1.

## 4. The api writer (plan 7b-A)

- **What:** a tokio task started in `bins/api/src/main.rs` beside the server. Every `PUBLIC_EVERY_S` seconds (default 300, first run 30 s after start) it builds a `PublicSnapshot` from the same functions the `ladder` (`ladder.rs`) and `today` (`episodes.rs`) handlers use. Each handler's body is split into a `pub(crate) async fn load_*(&AppState) -> Result<Dto, ApiError>` that both call, so the snapshot can't drift from the dashboard.
- **Where:** `PUBLIC_DIR` (compose: `/var/lib/ct-public`). The task writes `public.json.tmp`, fsyncs it, and renames it to `public.json`. The api stays `read_only: true`, and `public-data` is its only writable mount.
- **Failures:**
  - A failed query or a serialized size over 64 KiB logs one line (`api: public snapshot skipped: <reason>`) and leaves the old file in place.
  - The task never panics and never stops the server.
  - Without `PUBLIC_DIR` it doesn't start (local dev).
- **Cost:** 12 ladder and today queries an hour, whatever the traffic.

## 5. The public snapshot

### 5.1 Shape (`PublicSnapshot`, one serialize struct, fixed field order)

```
{ "v": 1, "written_ms": …,
  "ladder": { "epoch", "start_ms", "as_of_ms", "break_even",
              "measured": { "latency_ms" } | null,
              "rungs": [ { "scenario", "latency_ms", "extra", "n_exec", "n_leg_risk", "n_missed", "gains", "leg_risk", "settle", "unrealized", "net" } ] },
  "markets": { "status", "rows": [ { "side", "scenario", "latency_ms", "dimension", "bucket", "label", "few_trades", "n_exec", "mean", "lo", "hi" } ] },
  "today": { "day_start_ms", "arb", "no", "yes", "edge_budget100", "hourly": [ { "hour", "arb" } ] } }
```

- `markets.rows` holds only `dimension` `all` and `category`. Its field list above is the minimum. The plan sets it to exactly the fields `SweetLadderGrid` reads (checked against the component), and no others. A `bucket` is a category name such as `crypto` or `sports`.
- `measured.host` and each rung's `account_id` are dropped (the host names the VPS; account ids are internal). Rung fields otherwise keep the api's `LadderRung` names, so `LatencyLadder` reads them unchanged.
- Decimals are strings, as in the api.

### 5.2 What never ships (tests)

- **Whitelist test:** it serializes a fully populated snapshot and compares every key path to a fixed list in the test. A new field fails until the list names it.
- **Content test:** no string field matches a market id, token id, event title, account id or host name taken from the fixture data.
- **Size test:** the fixture snapshot with the maximum row counts stays under 64 KiB.

### 5.3 Serving

- **nginx** (`docker/web/nginx.conf`):

  ```
  location = /math-data/public.json {
      root /srv/ct-public-root;   # the volume mounted at /srv/ct-public-root/math-data (read-only)
      add_header Cache-Control "public, max-age=60";
      gzip on; gzip_types application/json;
      try_files $uri =404;
  }
  ```

  Any other `/math-data/…` path isn't matched there and falls to the dashboard location, which requires login at Traefik.
- **Traefik:** the 09-27 `ct-math` rule gains `|| Path(`/math-data/public.json`)`. Its middlewares (per-IP rate and in-flight limits, `ct-sec`, the math CSP) apply unchanged. `connect-src 'self'` already allows the fetch.
- **Compose:** a named volume `public-data`. `api` mounts it read-write at `/var/lib/ct-public`, and `web` mounts it read-only at `/srv/ct-public-root/math-data`. There's no new container and no new memory.

## 6. The playground page (plan 7b-B)

### 6.1 Layout

As 09-27 §6.1, plus:
- a **live strip** under the header: today's episodes (NO / YES), edge on the table, and "as of HH:MM UTC", drawn from `today`;
- sections a–f in order.

### 6.2 Sections

a–e as 09-27 §6.2, plus section e's caption: "caught is not filled: see f".

| | Starts from | Controls | Draws |
|---|---|---|---|
| f · one leg misses | a 3-leg NO basket with a positive edge at `t_x`, all legs deep enough (so the default result is "all fill") | per leg: drain the depth at `t_x` (slider to 0), shift the unwind bids down (0–20 ticks); `q_target`; fees on or off; "the usual burst" preset (one leg empty, bids down 5 ticks) | per leg: a bar for planned vs filled, `q_eff` as a line across the legs, each leg's excess as a block sold at the bid, and the net as the big number: "caught, and lost $x" or "filled, $x on paper". Then the **live ladder** from the snapshot (`LatencyLadder`, read-only) with "on the live scanner, as of …", and the **category grid** from `markets` |

- Every control change calls `basket_json` synchronously, once per animation frame while dragging (as 09-27).
- **f's caption** names `sim::venue::basket_buy` / `unwind_sell` and says: "the same rule the paper accounts on the live scanner use; unwinds here happen at one instant, the live sim waits the account's delay".

### 6.3 The snapshot on the page

- **Fetching:** one `fetch('/math-data/public.json')` on load. It re-fetches when the tab becomes visible and the last fetch is over 5 minutes old. There is no polling loop.
- **Fresh** (`written_ms` within 30 minutes): the live strip and f's live part are drawn.
- **Stale:** the same drawings, plus "as of <time>: the live writer is behind".
- **Missing** (404, a network error, bad JSON or `v ≠ 1`): the live strip and f's live part are left out, and one dim line says "live numbers unavailable". The rest of the page never depends on the file.

## 7. Web build (plan 7b-B)

### 7.1 As 09-27

§4.2 (the wasm build, tsconfig split, `WASM_REQUIRED`), §4.4 (the 350 KB gzip budget and the module guard) and §5.1 (the second Vite build).

### 7.2 Making the 7a schematics public-safe

The module guard fails a build that pulls `src/pages/`, `src/api/` (values), `src/explain/` or `src/help.ts` into the math bundle. Two 7a imports would trip it:
- `DurationBars` imports `durationText` (a value) from `src/explain/models`. `durationText` and the `DurationRow` and `PayoffModel` types move to `src/schematics/models.ts`, and `src/explain/models.ts` re-exports them.
- `LatencyLadder` renders `EmptyState`, which imports `api/client`. The ladder takes an `empty: ReactNode` prop instead. The dashboard passes `<EmptyState …/>` and the playground passes a dim line.

Type-only imports from `src/api/types` are erased and allowed. The guard checks built module ids, so it proves this.

## 8. Testing

As 09-27 §8 (minus the panel items, which 7a covered), plus:
- **Rust:**
  - `basket_buy`, `unwind_sell` and `basket_net` unit and property tests (§3.3), with the `basket_net` = ledger check;
  - `basket_json` fixtures and parity;
  - the snapshot whitelist, content and size tests (§5.2);
  - the writer: the atomic rename (no partial file is ever visible to a reader in the test), the old file kept on a failed query, and no start without `PUBLIC_DIR`.
- **Web:**
  - section f renders the default and the burst preset with the real WASM;
  - the live strip for a fresh, stale and missing snapshot (a mocked fetch);
  - the module guard with the reused schematics;
  - 360 px with no horizontal scroll, plus the Chrome check after deploy (drawings over text, labels in HTML beside the SVGs, per the 7a lessons).

## 9. Deploy

- **7b-A** (api image, compose volume and mounts): runbook `docs/superpowers/runbooks/phase7b-api-vps.md`.
  - The replay identity check runs on the PC before merge.
  - Then the rust image is built and shipped, and `api` recreated.
  - Then `public.json` must appear within 60 s with `v: 1`, and its keys must match the whitelist (a `jq` check).
  - The `web` mount lands in 7b-B.
- **7b-B** (web image and Traefik labels): runbook `docs/superpowers/runbooks/phase7b-web-vps.md`.
  - The `curl` checks without a cookie (09-27 §8) plus: `/math-data/public.json` returns 200 with `application/json`; `/math-data/`, `/math-data/x.json` and `/math-data/../public.json` redirect to login.
  - The flood test (09-27 §5.5) also runs against `/math-data/public.json`.
- **The quiet-minute check is advisory in both runbooks.** If there's no quiet minute within 15 minutes, recreate anyway and count the reconnects (2026-09-30 showed 0).
- **Rollback:** a `pre7b` tag per image, as in 7a's runbooks.

## 10. Order of work

**7b-A:**
1. `basket_buy` / `unwind_sell` / `basket_net` extraction and tests.
2. The `arb-core` additions (09-27 §3).
3. `arb-wasm` with fixtures, native tests and CI parity.
4. The load-function split in the api and the snapshot writer.
5. Compose.
6. The runbook.

**7b-B:**
1. The schematics import fixes.
2. The wasm build and Docker stage (09-27 §4.2).
3. Views a–f.
4. The math page, its build and the bundle check.
5. nginx and Traefik.
6. The runbook with the flood test.

## 11. Open items for the plans

- The exact `wasm-bindgen` version (the latest at plan time), with crate and CLI pinned together (09-27 §11).
- How to replay one raw hour through the paper sim on the PC for §3.3: find the existing replay path (the 09-29 diagnosis used a throwaway replay probe) and make it a script.
- Whether `today`'s load path depends on request state that a background task lacks. It shouldn't; check.
- `oha` on the PC, or a `curl` loop (09-27 §11).
