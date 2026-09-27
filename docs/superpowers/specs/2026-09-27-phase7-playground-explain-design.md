# isoca-radar Phase 7: public math playground (WASM) and the explain panel

**Date:** 2026-09-27. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.7 schematics, §14 frontend, §19 Phase 7). **Builds on:** Phase 4 (`2026-09-25-phase4-dashboard-design.md`), Phase 5a (`2026-09-26-phase5a-cards-design.md`).
**Status:** approved in conversation, section by section (2026-09-27). The owner asked for an independent subagent review of this file instead of their own review before the plan.

## 1. Purpose, scope and success

Two things that share one set of drawings:

- **The math playground** (`/math`, **public**): interactive schematics a–e from parent §5.7. Every number comes from `arb-core` compiled to WASM, so the explainer can't disagree with the scanner. Portfolio first, teaching second.
- **The explain panel** (dashboard, **login-only**): an expandable panel on every dashboard page that answers the owner's plain questions ("How can this make money?", "What did we generate today?", "How do I read this, and why?") with drawings filled with the page's live numbers. It's for the owner.

**In scope:**
- `crates/arb-wasm`: a wasm-bindgen facade over `arb-core` (and `sim::latency`), with JSON in and JSON out.
- One addition to `arb-core`: a `payoff` module (the §5.7 (b) tables as code, with lemma tests).
- `web/src/schematics/`: five pure SVG views, a–e.
- A second Vite build, `math`, and its nginx locations and Traefik router.
- A native-vs-WASM parity check in CI.
- The explain panel, with per-page questions, on all six dashboard pages.

**Out of scope:**
- Laya and the Review page (Phase 6, delayed by VPS memory).
- 5b storage, 5c Saguillo, 5d sweet spots: each has its own spec.
- Loading WASM on the dashboard. The panel draws numbers the scanner already stored (§6.4).
- Any LLM-written text. Every answer is a fixed template plus live numbers.

**Exit:**
- `/math` loads without login on the VPS, all five sections work, and the public bundle holds no dashboard code.
- The explain panel answers every question in §6.3 on every page, with live numbers.
- CI's parity check is green.
- A flood test against `/math` (§5.5) leaves the web container inside its 32 MB and api/scanner/postgres untouched.

## 2. Architecture

```
arb-core ─┬─► arb-wasm (new; wasm-bindgen; JSON strings in and out; decimals as strings)
sim ──────┘        │  (sim::latency::exec_delay_ms only)
                   ▼
web/src/wasm/pkg  (generated, gitignored)
                   │
web/src/schematics/  a..e: pure React + SVG, props = a computed model, no fetching, no WASM import
        ▲                                   ▲
math.html → src/math/ (public)          dashboard → src/explain/ (login-only)
model = arb-wasm(dragged inputs)        model = api data the page already holds
```

- **Schematics are views.** Each takes a typed `model` prop and draws it. They never compute money, never import the WASM package, and never fetch.
- **The playground adapts inputs to models** through `arb-wasm`. It is the only code that imports `src/wasm/pkg`.
- **The panel adapts api payloads to models** in `src/explain/models.ts`. It never recomputes a figure the api already returns. Where a drawing would need data the api doesn't return, it draws a simplified shape and says so in the caption (§6.3).

## 3. `arb-core` addition: `payoff`

`arb_core::payoff` makes the §5.7 (b) tables code rather than prose:

- `pub fn yes_basket(s: usize, other_listed: bool) -> Matrix`, where rows are `YES_i` plus a `basket` row, and columns are each outcome plus `Other` when `!other_listed` (the uncovered column).
- `pub fn no_basket(s: usize, other_listed: bool) -> Matrix`.
- `pub fn implication() -> Matrix`, with columns `A∧B`, `¬A∧B`, `¬A∧¬B`. `A∧¬B` is marked impossible, not omitted.
- `Matrix { rows: Vec<String>, cols: Vec<String>, cells: Vec<Vec<Decimal>>, basket: Vec<Decimal>, broken: Vec<bool> }`, where `broken[c]` is true when the basket row pays less than the basket's guaranteed payout in column `c`.
- **Tests** (property, `s` in 2..=12):
  - L3: the YES basket pays 1 in every column iff `other_listed`;
  - L4: the NO basket pays at least `s − 1` in every column;
  - L10: the implication total is at least 1 in every possible state.
- **Guard rails:** no floats (the existing lint), and `s` above 12 is clamped by the caller, not here: `arb-core` stays total.

## 4. `arb-wasm` and the parity check

### 4.1 Crate

- `crates/arb-wasm`, a workspace member with `crate-type = ["cdylib", "rlib"]`. Its dependencies are `arb-core`, `sim`, `serde`, `serde_json`, `rust_decimal` (`serde-with-str`) and `wasm-bindgen`, pinned with `=` to one exact version.
- **Two layers:**
  - `src/api.rs` holds plain Rust, `pub fn <name>_json(input: &str) -> String`, and is fully testable natively;
  - `src/lib.rs` holds only `#[wasm_bindgen]` wrappers that call those functions. No logic lives in the wrapper.
- **Decimals cross as strings** in both directions. Invalid input returns `{"error": "<plain sentence>"}` and never panics (`unwrap_used`/`expect_used` stay denied).
- **Limits, enforced in `api.rs` before any work:**
  - at most 6 legs and 8 levels per leg;
  - prices on the 0.01 grid within 0.01–0.99;
  - sizes up to 1,000,000 shares;
  - latency 0–5,000 ms;
  - `s` from 2 to 6;
  - input JSON up to 16 KiB.

  Anything outside these returns an error; nothing is silently clamped.

| Function | Input | Output (model) | Uses |
|---|---|---|---|
| `mirror_json` | YES bids and asks | NO asks and bids (`1 − p`, same sizes), best bid and ask, spread, D1 long and short sums, `crossed` | `book::Book`, `detect::canary` |
| `payoff_json` | kind (`yes`, `no`, `implication`), `s`, `other_listed` | `Matrix` plus the profit formula text and its value for given prices | `payoff` (§3) |
| `classify_json` | legs: status plus best ask, best bid and fee per leg, `post_end` | the decision path taken (list of node ids), resulting `Kind` and `Realization` or none, net and gross Π at top of book | `detect::detect_yes_basket`, `detect_no_basket` |
| `depth_walk_json` | legs with ask levels, fees on or off, rule | cost steps `(q_from, q_to, cost_per_basket)`, Q*, Π, and where it stops | `fill::walk` |
| `episode_json` | 5 updates (t, per-leg top of book), scenario (net/match ms, `honor_seconds_delay`, `seconds_delay`) | per-update Π, episode open and close, peak, `t_x`, the state in force at `t_x`, captured Π at `t_x` (0 if closed) | `episode::EpisodeTracker`, `sim::latency::exec_delay_ms`, `fill::walk` |

**`episode_json` is a teaching model:** one taker at `t_x`, with no consumed liquidity, no FAK per leg and no unwinds. The playground caption says so and points to the Paper page's Edge capture box for the full simulation. `frames_per_s` is fixed at 0 (no burst multiplier); the burst scenario is explained in text.

### 4.2 Build

- `rust-toolchain.toml` gains `targets = ["wasm32-unknown-unknown"]`.
- `scripts/build-wasm.sh` runs `cargo build -p arb-wasm --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir web/src/wasm/pkg`. `wasm-bindgen-cli` is installed with `cargo install --locked --version <same as the crate>`. No `wasm-pack`, and no `wasm-opt` in v1 (the size budget in §4.4 decides whether it's needed).
- `web/src/wasm/pkg/` is gitignored.
- **Docker** (`docker/web.Dockerfile`):
  - a new first stage `FROM rust:1.98.1-bookworm@<same digest as rust.Dockerfile>` installs the target and the pinned `wasm-bindgen-cli`, then runs the script;
  - the node stage copies `pkg/` in before `npm run build`;
  - the final nginx stage is unchanged apart from its config.

### 4.3 Parity (native = WASM)

- **Golden fixtures:** `crates/arb-wasm/fixtures/<function>.json` holds `[{ "name", "input", "output" }]`, with at least:
  - the §5.7 examples;
  - a crossed book;
  - an uncovered `Other`;
  - zero depth on one leg;
  - "exactly 1.00";
  - fees off;
  - every limit ± 1 (the error outputs are part of parity).
- **Native test** (`cargo test -p arb-wasm`): runs each input through `api.rs` and compares the output string byte for byte. With `UPDATE_FIXTURES=1` it rewrites the files instead, and a reviewer sees the diff.
- **Web test** (`web/src/wasm/parity.test.ts`): loads the generated package with `initSync` from the `.wasm` bytes, runs the same fixture files, and compares byte for byte.
- **CI** (`ci.yml`, `web` job): adds the Rust toolchain from `rust-toolchain.toml` and the pinned `wasm-bindgen-cli` (cached), then runs `scripts/build-wasm.sh` before `npm test`. The `rust` job already runs the native side through `cargo test --workspace`. Either side drifting from the checked-in fixtures turns CI red.

### 4.4 Size budget

The public page's assets (JS + WASM + CSS, gzip) must stay at or under 350 KB. `scripts/check-math-bundle.mjs` runs after the math build in CI and fails above that. The same script fails if any file under `dist/math-assets/` contains the string `/api/`, `EventSource`, or a source path from the dashboard (`src/pages/`, `src/api/`). The math build keeps module paths in its chunk names only for this check (`build.rollupOptions.output.sourcemapPathTransform` isn't needed: the check reads the build's `manifest.json`, which `vite.math.config.ts` enables, and fails if any listed module lives under `src/pages/` or `src/api/`).

## 5. The public route

### 5.1 Build

- `web/math.html` plus `web/src/math/main.tsx` is a separate entry with no router, api client, stream or `help.ts` imports. It gets its own short copy module.
- `web/vite.math.config.ts` sets `input: math.html`, `build.assetsDir: 'math-assets'`, `outDir: 'dist'` and `emptyOutDir: false`, and runs after the dashboard build (`npm run build` = typecheck, dashboard build, math build, bundle check). Because the builds are separate, no chunk is shared with the dashboard.
- **Styles:** the math page imports `theme.css` for the tokens and pixel classes and has its own layout CSS. It gets the same dark/light behaviour as the dashboard.

### 5.2 nginx (`docker/web/nginx.conf`)

```
location = /math           { add_header Cache-Control "no-cache"; try_files /math.html =404; }
location /math-assets/     { add_header Cache-Control "public, max-age=31536000, immutable"; try_files $uri =404; }
```

- **Everything else** stays as it is.
- **WASM MIME type:** nginx's `mime.types` maps `.wasm` to `application/wasm` (needed for streaming compile). A test in the runbook checks the header.

### 5.3 Traefik (`compose.web.yml`, `web` service labels)

| Label | Value |
|---|---|
| router `ct-math.rule` | ``Host(`${RADAR_DOMAIN}`) && (Path(`/math`) \|\| PathPrefix(`/math-assets/`))`` |
| `ct-math.priority` | `100` (above `ct-web`'s rule-length default) |
| `ct-math.middlewares` | `ct-math-rl, ct-math-if, ct-sec, ct-math-csp`: **no `ct-auth`** |
| `ct-math-rl` | `ratelimit.average=5`, `burst=20` (per source IP) |
| `ct-math-if` | `inflightreq.amount=4` (per source IP) |
| `ct-math-csp` | `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` |

- **`connect-src 'self'`** exists only so `WebAssembly.instantiateStreaming(fetch(...))` can fetch the `.wasm` file. The page makes no other request, and the bundle check (§4.4) makes sure no `/api/` string ships.
- **The dashboard routers keep their CSP unchanged,** without `wasm-unsafe-eval`.
- **Any path not matched by `ct-math`** still falls to `ct-web`, which requires login. Even `/math/` (with a trailing slash) and `/math.html` go through login.

### 5.4 What a flood can reach

Traefik rejects over-limit requests itself. What passes reaches nginx, which serves at most three static files, and those are immutable except the no-cache HTML (about 1 KB). No database, api, analyze or scanner work sits behind any public path, so memory can't grow with traffic beyond nginx's fixed worker buffers, inside `mem_limit: 32m`. The dashboard shares this nginx, so a flood can slow it but not the capture.

**Fallback, documented and not built:** if the runbook's flood test shows the dashboard suffering, `math` moves to its own `nginx-unprivileged` container (16 MB, 0.05 CPU) that serves only the math files.

### 5.5 Flood test (runbook, from the PC, against the VPS)

- `oha -z 60s -c 50 https://<domain>/math-assets/<wasm file>` should show mostly `429`s.
- Meanwhile `docker stats` on the VPS should show `web` at or under 32 MB, and `api`, `scanner` and `postgres` unchanged.
- The dashboard should still load while the test runs.

The results are recorded in the runbook.

## 6. The playground page

### 6.1 Layout

- **Page:** one column, the same pixel style, a header `isoca-radar · math` and no nav to the dashboard. At the bottom, a line says the live dashboard is private.
- **Sections:** a to e in order. Each has a title, the drawing, controls under it, one deadpan line, a "reset" button, and an honest caption naming the `arb-core` function that computed it.
- **Layout contract:** it works at 360 px with no horizontal scroll, and controls are usable by touch.

### 6.2 Sections

| | Starts from | Controls | Draws |
|---|---|---|---|
| a · one book, twice | the §5.7 Fed book | drag any level's price (0.01 ticks) or size; "cross the book" | both books side by side, reflected prices annotated `(1 − p)`, D1 sums, and a crossed state in the alert colour: *"Crossed book. Somewhere a feed is lying."* |
| b · payoff tables | YES basket, S = 3, Other unlisted | YES/NO/implication, S 2–6, "Other listed" toggle, optional prices per leg | the matrix, the ✗ column, the basket row, and profit `1 − Σc` or `Σv − 1` with the prices entered |
| c · classify one update | 3 live legs, a small NO-basket edge | per leg: status (live, placeholder, Other, halted), best bid and ask, `post_end` toggle | the §5.7 (c) tree with the taken path lit, ending in the kind or "nothing" |
| d · depth walk | 3 legs, 3 levels each, a positive edge | drag levels on up to 4 legs, fees on or off, "exactly 1.00" (sets Σ best asks to 1.00) | the cost staircase against payoff 1, Π shaded, Q* marked, the step where it stops |
| e · episode and latency | the §5.7 (e) five updates | scrub updates, latency slider 0–1,000 ms, `secondsDelay` toggle | Π per update, the episode bar, peak, `t_x` marker, and "caught" or "missed" with captured Π. Caption: teaching model (§4.1) |

- **Section d** is limited to 4 legs, which is below the crate's 6, so the drawing stays readable.
- **Recomputing:** every control change calls the WASM function synchronously. Changes are debounced to one call per animation frame while dragging.

## 7. The explain panel

### 7.1 Shape

- **Tab:** a vertical `EXPLAIN ▸` tab on the right edge of the viewport on every dashboard page.
- **Wide viewports (1,500 px or more):** when open, the app and the panel (360 px, 16 px gap) centre together as one pair, so the panel sits in the empty margin and covers nothing.
- **Narrower viewports:** it opens as an overlay drawer from the right, at `min(360px, 100vw)` wide. Esc or the tab closes it, and focus returns to the tab.
- **Memory:** open or closed and the last question open are kept in `localStorage` (wrapped in try/catch; the default is closed). The selection is not kept.
- **Accessibility:** it's a `<aside aria-label="explain">`, the tab is a button with `aria-expanded`, and the questions are a disclosure list.

### 7.2 Data

- **Selection state:** a `ExplainContext` (React context) holds `page` and `selection: { kind: 'episode' | 'position' | 'card', id } | null`.
- **Pages publish, the panel reads.** Pages publish what they already fetched through `useExplainData(page, data)`, and the panel only reads from context. The panel issues **no fetches**; a test enforces this with the mock api's call counter.
- **Selection:**
  - clicking a Radar or Episodes row, an Episode detail page, a Paper position row, or a Papers card sets the selection;
  - rows get a small `explain` affordance (the row click keeps its current navigation behaviour);
  - "back to the page" clears the selection.
- **Honesty rule:**
  - every money figure is labelled *paper*, *on the table*, or *captured at N ms*;
  - panel copy never claims real money: a test scans `src/explain/copy.ts` and fails on the whole words `earned`, `earn`, `made` and `income`, case-insensitive. "Profit" is allowed only as the existing column names use it (`profit @ $100`), and the test fails on any other `profit`;
  - the owner's question "What did we generate today?" is kept as a question, and its answer says what kind of number each one is.

### 7.3 Questions per page

Each answer is one schematic plus 2–4 sentences from `src/explain/copy.ts`. Sentences that overlap `help.ts` import from it, so the two never disagree.

| Page | Question | Drawing and source |
|---|---|---|
| Radar | How can this make money? | (b) payoff table for the selected episode, or else the top open one, or else a stored example labelled *example*. Built from its kind, S and `complete` flag |
| | What did we generate today? | two bars: edge on the table today vs captured at the headline latency (`/api/today`, the Edge capture teaser figures); text says paper, not trades |
| | Why is this row not arbitrage? | (b) with the uncovered column ✗ (shown when the selection is an incomplete set) |
| | How do I read these columns? | an annotated copy of one row: S, profit @ $100, net and gross @ depth, Q*, age, edge bar |
| Episodes | What is an episode? | (e) timeline from the selected episode's open, peak and close (`observations`), or else an example |
| | Why did it close? | its close reason and flags, via `FLAG_WORDS` |
| | What do the flags mean? | the flag legend already built from the shared flag sentences |
| Episode detail | What would buying this have cost? | (d) simplified: one step per leg from its VWAP and Q*, Π at peak. Caption: *"simplified: per-level steps are not stored"* |
| | Would we have caught it? | (e) its duration against the measured latency marker and each scenario's delay |
| Paper | Are we making money? | equity vs bankroll per account, realized vs unrealized split, all labelled paper |
| | Why is money locked? | positions awaiting resolution as blocks sized by locked capital, with days to resolution |
| | Why do the policies differ? | P0–P3 one line each, with the account's current equity |
| Papers | What does this card test? | the card's claim in one sentence plus its PAPER / THEIR / OUR columns explained |
| | Do we agree with the paper, and why not? | status plus method differences from the card body |
| Health | Are we seeing everything? | hearts decomposed: shards, drops, lag, capture; integrity per million vs Gebele's 20 per 308M |

- **Schematic reuse:** the panel uses the same `src/schematics/` components as the playground, with a `compact` prop (smaller type, no controls).
- **Where drawings come from:** in the panel, drawings (b) and (c) come from panel-side model builders that reproduce only *labels and structure* (rows, columns, which column is ✗). Every money figure shown comes from the api. A unit test checks the panel's payoff structure against `crates/arb-wasm/fixtures/payoff.json`, so the two can't drift.

## 8. Testing

- **Rust:**
  - `payoff` property tests (§3);
  - `arb-wasm` native fixtures, limits and "never panics" (proptest over random JSON-ish input up to 16 KiB returns either a model or `{"error"}`);
  - the existing lints and `cargo deny`, with `wasm-bindgen` added to the allowed list.
- **Web, parity:** `parity.test.ts` (§4.3).
- **Web, schematics:** each renders from a fixture model, and a snapshot of key SVG attributes checks the ✗ column, Q* and the `t_x` marker.
- **Web, math page:**
  - renders all five sections;
  - a control change updates the drawing (using the real WASM, as in parity);
  - invalid input shows the error sentence;
  - 360 px with no horizontal scroll (the existing responsive test helper).
- **Web, panel:**
  - each question per page renders with `test/fixtures.ts` data;
  - zero api calls from the panel;
  - selection switches answers and "back" clears them;
  - Esc closes and returns focus;
  - drawer at 360 px, docked at 1,500 px;
  - the banned-words check (§7.2).
- **Build:** `check-math-bundle.mjs` covers size and the no-dashboard-code check (§4.4).
- **Deploy:** the flood test (§5.5), and a `curl -I` of `/math`, `/math-assets/*.wasm` (`application/wasm`), `/math/` (302 to login) and `/api/today` (302 to login) without a cookie.

## 9. Deploy

- **One image change (web)** and label changes in `compose.web.yml`. No new container, no new memory.
- **Runbook:** `docs/superpowers/runbooks/phase7-playground-vps.md` covers the build on the PC, pinning the new web digest, the no-cookie `curl` checks, the flood test and a rollback (the previous web digest).
- **Local (phase A):** `/math` works at `127.0.0.1:8080` like the other pages. There's no Traefik locally, so no rate limit; that's fine, because nothing public runs locally.

## 10. Order of work

1. `payoff` in `arb-core`, then `arb-wasm` with fixtures and native tests.
2. The WASM build script, toolchain target, CI parity, and Docker stage.
3. `src/schematics/` a–e with fixture models.
4. The math page, its Vite build and the bundle check.
5. nginx, Traefik labels, and the runbook with the flood test.
6. The explain panel's shell (context, tab, drawer and dock).
7. Questions page by page: Radar, Episode detail, Episodes, Paper, Papers, Health.

## 11. Open items for the plan

- Pick the exact `wasm-bindgen` version (the latest release at plan time) and pin crate and CLI together.
- Confirm that the Radar, Episodes and Paper payloads already carry everything §7.3 names: for example, days to resolution for locked positions, and the `complete` flag for Radar rows. Anything missing becomes a read-only api field, listed in the plan, with no new endpoint.
- Confirm `oha` (or `hey`) is available on the PC for the flood test; otherwise use a small `curl` loop in the runbook.
