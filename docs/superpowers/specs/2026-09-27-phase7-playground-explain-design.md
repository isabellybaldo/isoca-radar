# isoca-radar Phase 7: public math playground (WASM) and the explain panel

**Date:** 2026-09-27. **Parent spec:** `2026-09-22-arb-scanner-design.md` (§5.7 schematics, §14 frontend, §19 Phase 7). **Builds on:** Phase 4 (`2026-09-25-phase4-dashboard-design.md`), Phase 5a (`2026-09-26-phase5a-cards-design.md`).
**Superseded in part (2026-09-29):** §7 (the explain panel) is replaced by `2026-09-29-phase7a-latency-answer-design.md` (Phase 7a). The rest of this spec is **Phase 7b** (the public `/math` playground), unchanged; 7b reuses 7a's `src/schematics/`. §1's "no api writes" no longer holds for the dashboard: 7a adds `POST /api/paper/reset`.
**Superseded in part (2026-09-30):** Phase 7b is specified by `2026-09-30-phase7b-math-playground-design.md`, which keeps §1–6 and §8–12 here and adds section f (leg risk), the public snapshot and two plans.
**Status:** approved in conversation, section by section (2026-09-27). The owner asked for an independent subagent review of this file instead of their own review before the plan.

## 1. Purpose, scope and success

Two things that share one set of drawings:

- **The math playground** (`/math`, **public**): interactive schematics a–e from parent §5.7. Every number comes from `arb-core` compiled to WASM, so the explainer can't disagree with the scanner. Portfolio first, teaching second.
- **The explain panel** (dashboard, **login-only**): an expandable panel on every dashboard page that answers the owner's plain questions ("How can this make money?", "What did we generate today?", "How do I read this, and why?") with drawings filled with the page's live numbers. It's for the owner.

**In scope:**
- `crates/arb-wasm`: a wasm-bindgen facade over `arb-core` (and `sim::latency`), with JSON in and JSON out.
- Additions to `arb-core` so that no decision or money logic lives in the facade (§3): `payoff`, `book::d1_sums`, `detect::classify`, `fill::walk_steps`.
- `web/src/schematics/`: five pure SVG views, a–e.
- A second Vite build, `math`, and its nginx locations and Traefik router.
- A native-vs-WASM parity check in CI.
- The explain panel, with per-page questions, on all six dashboard pages.

**Out of scope:**
- Laya and the Review page (Phase 6, delayed by VPS memory).
- 5b storage, 5c Saguillo, 5d sweet spots: each has its own spec.
- Loading WASM on the dashboard. The panel draws numbers the scanner already stored (§7.3).
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

## 3. `arb-core` additions

The facade (§4) only converts JSON to `arb-core` calls and back. Anything that decides or computes money lives in `arb-core`, where the scanner uses it too.

### 3.1 `payoff` (new module)

This module makes the §5.7 (b) tables code rather than prose.

- `pub fn yes_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError>`, where rows are `YES_i` plus a `basket` row, and columns are each outcome plus `Other` when `!other_listed` (the uncovered column).
- `pub fn no_basket(s: usize, other_listed: bool) -> Result<Matrix, PayoffError>`.
- `pub fn implication() -> Matrix`, with **four** columns `A∧B`, `A∧¬B`, `¬A∧B`, `¬A∧¬B`. The `A∧¬B` column has `possible = false` and empty cells.
- `Matrix { rows, cols, cells: Vec<Vec<Option<Decimal>>>, basket: Vec<Option<Decimal>>, guaranteed: Decimal, broken: Vec<bool> }`:
  - `guaranteed` is 1 for the YES basket, `s − 1` for the NO basket and 1 for the implication;
  - `broken[c]` = the column is possible and `basket[c] < guaranteed`.
- **Errors:** `s` outside 2..=12 returns `PayoffError::Size`. Nothing is clamped.
- **Tests** (property, `s` in 2..=12):
  - L3: the YES basket is broken in some column iff `!other_listed`;
  - L4: the NO basket is never broken;
  - L10: the implication total is at least 1 in every possible column.

### 3.2 `book::d1_sums`

`pub fn d1_sums(top: &Top) -> Option<(Decimal, Decimal)>` returns (long, short) = (`A + (1 − B)`, `B + (1 − A)`), or `None` without both sides. A test checks that `canary` (crossed) holds iff long < 1 and short > 1, which is L2.

### 3.3 `detect::classify`

`pub fn classify(g: &Group) -> Classification { path: Vec<Node>, signal: Option<Signal> }`:

- **What `Node` is:** an enum naming each §5.7 (c) test in order: `Decided`, `NoLive`, `Crossed`, `NoBasketTried`, `NoBasketPositive`, `YesBasketTried`, `MissingAsk`, `YesBasketPositive`, `Complete`, `Incomplete`, and `NoSignal`.
- **Relation to the existing detectors:** `detect_yes_basket` and `detect_no_basket` keep their signatures and results. `classify` calls them and adds the path by re-evaluating the same guard functions (`decided`, the crossed check and the missing-ask check). Those guards move into named private functions that both use.
- **The test:** for every existing detector test and a proptest over random groups, `classify(g).signal` equals whichever of the two detectors is positive, and L7 means they are never both positive.
- **`post_end` is not in `arb-core`.** The scanner sets it from event dates (`bins/scanner/src/engine.rs`) and it never changes the kind. The playground shows it as a label the scanner would add, and says so in the caption. A decided event (any leg `ClosedYes`) yields no signal, as it does in the scanner today.

### 3.4 `fill::walk_steps`

`pub fn walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step>` returns every segment until some leg's depth runs out, **including the unprofitable segments after Q***. `Step { q_from, q_to, cost_per_basket, profitable }`.

**Property test:** the sum over the profitable prefix equals `walk(legs, rule, None)` (same Q*, same Π), and cost is non-decreasing (L8).

## 4. `arb-wasm` and the parity check

### 4.1 Crate

- `crates/arb-wasm`, a workspace member with `crate-type = ["cdylib", "rlib"]`.
  - **Dependencies:** `arb-core`, `sim`, `serde`, `serde_json` and `rust_decimal` come through the workspace as usual.
  - **Explicit features:** `serde_json` with `arbitrary_precision`, and `rust_decimal` with `serde-with-str`. They're declared in arb-wasm's own manifest, so the feature set is the same whether it's built alone for wasm32 or unified under `cargo test --workspace`.
  - **`wasm-bindgen`** is the only exact pin (`=x.y.z`), because its CLI must match.
- **Two layers:**
  - `src/api.rs` holds plain Rust, `pub fn <name>_json(input: &str) -> String`, and is fully testable natively;
  - `src/lib.rs` holds only `#[wasm_bindgen]` wrappers that call those functions. No logic lives in the wrapper.
- **Decimals cross as JSON strings** in both directions. A JSON number where a decimal is expected is rejected with an error.
  - **Output format:** every output is a `#[derive(Serialize)]` struct (fixed field order), with every `Decimal` written as `normalize().to_string()`.
  - **Invalid input** returns `{"error": "<plain sentence>"}` and never panics (`unwrap_used`/`expect_used` stay denied).
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
| `mirror_json` | YES bids and asks (levels) | NO asks and bids from `no_asks_best_first` / `no_bids_best_first`, `top`, D1 long and short, `crossed` | `book::Book`, `book::d1_sums`, `detect::canary` |
| `payoff_json` | kind (`yes`, `no`, `implication`), `s`, `other_listed`, optional price per leg | `Matrix`; with prices (YES ask and bid per leg, one level each, zero fees), the gross Π at top of book from `detect::classify`, shown next to the parent spec's formula (`1 − Σ c(A)` or `Σ_S v(B) − 1`, L5) | `payoff` (§3.1), `detect::classify` |
| `classify_json` | `augmented`; per leg: `status` (`LegStatus`), bid and ask levels, fee `{rate, exponent}` | `Classification`: path, `Kind` and `Realization` or none, net and gross Π | `detect::classify` (§3.3), `fee::FeeSchedule::new` |
| `depth_walk_json` | legs with ask levels, fee `{rate, exponent}` or none, `rule` (`yes` = `AllLegs{payoff: 1}`) | `walk_steps` output plus the `walk` totals (Q*, Π) | `fill::walk_steps` (§3.4), `fill::walk` |
| `episode_json` | `kind` (`yes` or `no`); `gap_ms` (default 30,000, as `config/scanner.toml`); 2–8 updates, each `{t_ms, legs}` with the same leg shape as `classify_json`; a full `Scenario` (`proc_ms`, `net_ms`, `match_ms`, `honor_seconds_delay`, `name` fixed to `"playground"`, `burst` empty) and `seconds_delay` | per update: `classify` result and net Π; `EpisodeEvent`s from `on_state(key = "e", t_ms, net Π)`; open, close, peak; `t_x = t_open + exec_delay_ms(scenario, seconds_delay, 0)`; `caught` and captured Π | `detect::classify`, `episode::EpisodeTracker`, `sim::latency::exec_delay_ms` |

**`episode_json` is a teaching model:**
- **What it simulates:** one taker at `t_x` who gets the net Π of the latest update with `t ≤ t_x`, or 0 if the episode has closed by then. Picking that update is the only thing arb-wasm decides, and the money figure is `classify`'s.
- **What it leaves out:** consumed liquidity, FAK per leg and unwinds. The caption says so and points to the Paper page's Edge capture box for the full simulation.
- **No burst:** `frames_per_s` is 0 (no burst multiplier); the burst scenario is explained in text.

### 4.2 Build

- **`rust-toolchain.toml` is unchanged.** The wasm32 target is added with `rustup target add` only where it's needed: the web Docker stage, the CI `web` job, and the owner's PC (already done 2026-09-27). The rust image and the rust CI job don't download it.
- **`scripts/build-wasm.sh`:**
  - runs `cargo build -p arb-wasm --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir web/src/wasm/pkg`;
  - installs `wasm-bindgen-cli` with `cargo install --locked --version <same as the crate>`;
  - uses no `wasm-pack`, and no `wasm-opt` in v1 (the size budget in §4.4 decides whether it's needed).
- **`web/src/wasm/pkg/` is gitignored.**
- **Only math needs Rust.** Only `src/math/**` and `src/wasm/**` import `pkg`.
  - `web/tsconfig.json` excludes them, and `web/tsconfig.math.json` includes them.
  - `npm run build:dashboard` and `npm test` work without Rust: `parity.test.ts` and the math page tests skip with a printed reason when `pkg` is missing, unless `WASM_REQUIRED=1` (same pattern as `STORE_TESTS_REQUIRED`). CI sets it.
  - `npm run build` does both builds and needs `pkg`. The README's dev section names the prerequisite: `rustup target add wasm32-unknown-unknown`, the pinned CLI, and `scripts/build-wasm.sh`.
- **Docker** (`docker/web.Dockerfile`):
  - a new first stage uses `FROM rust:1.98.1-bookworm@<same digest as rust.Dockerfile>` and the same `cargo-chef` planner/cook pattern (`--package arb-wasm --target wasm32-unknown-unknown`), so a web-only change doesn't rebuild dependencies;
  - it then installs the pinned `wasm-bindgen-cli` and runs the script;
  - the node stage copies `pkg/` in before `npm run build`;
  - the final nginx stage is unchanged apart from its config.

### 4.3 Parity (native = WASM)

- **Golden fixtures:** `crates/arb-wasm/fixtures/<function>.json` holds `[{ "name", "input", "output" }]`, where `input` and `output` are **JSON strings** (the exact text passed and returned), never re-serialized values. So "byte for byte" means string equality on both sides. The fixtures include at least:
  - the §5.7 examples;
  - a crossed book;
  - an uncovered `Other`;
  - zero depth on one leg;
  - "exactly 1.00";
  - fees off;
  - every limit ± 1 (the error outputs are part of parity).
- **Native test** (`cargo test -p arb-wasm`): runs each input through `api.rs` and compares the output string byte for byte. With `UPDATE_FIXTURES=1` it rewrites the files instead, and a reviewer sees the diff.
- **Web test** (`web/src/wasm/parity.test.ts`): loads the generated package with `initSync` from the `.wasm` bytes, runs the same fixture files, and compares byte for byte.
- **CI:**
  - **`web` job** (`ci.yml`): adds the toolchain from `rust-toolchain.toml`, `rustup target add wasm32-unknown-unknown`, and the pinned `wasm-bindgen-cli` (cached). It runs `scripts/build-wasm.sh` and `cargo test -p arb-wasm` (native side, built alone), then `WASM_REQUIRED=1 npm test` and `npm run build`.
  - **`rust` job:** it also runs the native side under workspace feature unification (`cargo test --workspace`).
  - Any of the three drifting from the checked-in fixtures turns CI red.

### 4.4 Size budget and public-bundle check

- **Size:** the public page's assets (JS + WASM + CSS) must stay at or under **350 KB gzip**, which is what users receive, because nginx gzips them (§5.2).
- **The check** is `scripts/check-math-bundle.mjs`, which runs after the math build and fails above the limit.
- **No dashboard code:** `vite.math.config.ts` includes a small plugin whose `generateBundle` hook walks every chunk's `moduleIds`. It fails the build if any module lives under `src/pages/`, `src/api/`, `src/explain/`, or is `src/help.ts`. A static import of dashboard code therefore fails at build time, not only on review.

## 5. The public route

### 5.1 Build

- `web/math.html` plus `web/src/math/main.tsx` is a separate entry with no router, api client, stream or `help.ts` imports. It gets its own short copy module.
- **`web/vite.math.config.ts`:**
  - sets `input: math.html`, `build.assetsDir: 'math-assets'`, `outDir: 'dist'` and `emptyOutDir: false`, plus the module guard plugin (§4.4);
  - runs after the dashboard build: `npm run build` = typecheck both tsconfigs, dashboard build, math build, size check;
  - because the builds are separate, no chunk is shared with the dashboard.
- **Styles:** the math page imports `theme.css` for the tokens and pixel classes and has its own layout CSS. It gets the same dark/light behaviour as the dashboard.

### 5.2 nginx (`docker/web/nginx.conf`)

```
location = /math       { add_header Cache-Control "no-cache"; try_files /math.html =404; }
location /math-assets/ {
    add_header Cache-Control "public, max-age=31536000, immutable";
    gzip on; gzip_types application/wasm application/javascript text/css;
    try_files $uri =404;
}
```

Gzip is computed per response, so it costs nginx CPU, capped at 0.1. Precompressed `.gz` files with `gzip_static` would avoid it. `gzip_static` is compiled into the official nginx image, so the plan uses it if the build can emit `.gz` files (for example with `vite-plugin-compression` or a `gzip -k` step); otherwise it uses `gzip on`.

- **Everything else** stays as it is.
- **WASM MIME type:** nginx's `mime.types` maps `.wasm` to `application/wasm` (needed for streaming compile). A test in the runbook checks the header.

### 5.3 Traefik (`compose.web.yml`, `web` service labels)

| Label | Value |
|---|---|
| router `ct-math.rule` | ``Host(`${RADAR_DOMAIN}`) && (Path(`/math`) \|\| PathPrefix(`/math-assets/`))`` |
| `ct-math.entrypoints` | `websecure` (so plain HTTP still hits `ct-http`'s redirect) |
| `ct-math.tls.certresolver` | `letsencrypt` |
| `ct-math.service` | `ct-web` (the same nginx) |
| `ct-math.priority` | `1000` (well above `ct-web`'s rule-length default for any domain length) |
| `ct-math.middlewares` | `ct-math-rl@docker, ct-math-if@docker, ct-sec@docker, ct-math-csp@docker`: **no `ct-auth`** |
| `ct-math-rl` | `ratelimit.average=5`, `burst=20`, `sourcecriterion.ipstrategy.depth=0` (per client IP) |
| `ct-math-if` | `inflightreq.amount=4`, `sourcecriterion.ipstrategy.depth=0` (per client IP; without it, InFlightReq groups by request host, so one limit would be shared by everyone) |
| `ct-math-csp` | `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` |

- **`connect-src 'self'`** exists only so `WebAssembly.instantiateStreaming(fetch(...))` can fetch the `.wasm` file. The page makes no other request, and the bundle check (§4.4) makes sure no `/api/` string ships.
- **The dashboard routers keep their CSP unchanged,** without `wasm-unsafe-eval`.
- **Any path not matched by `ct-math`** still falls to `ct-web`, which requires login. Even `/math/` (with a trailing slash) and `/math.html` go through login.
- **Client IP:** DNS is a plain A record to the VPS (phase 4 runbook §0), with no CDN, so `depth=0` (the TCP peer) is the client IP. If a CDN is ever put in front, both limiters need `depth=1` plus Traefik `forwardedHeaders.trustedIPs` set to the CDN's ranges; otherwise every visitor shares the CDN's IPs.

### 5.4 What a flood can reach

Traefik rejects over-limit requests itself. What passes reaches nginx, which serves at most three static files, and those are immutable except the no-cache HTML (about 1 KB). No database, api, analyze or scanner work sits behind any public path, so memory can't grow with traffic beyond nginx's fixed worker buffers, inside `mem_limit: 32m`. The dashboard shares this nginx, so a flood can slow it but not the capture.

**What this does not protect.** Each rejected request still costs Traefik a TLS handshake and some CPU, and Traefik is shared with the VPS's other sites. A distributed flood (many IPs) passes per-IP limits by design. That is true of every public site on this Traefik already. The playground adds no new cost per request beyond a static file, and nothing here makes it worse than the existing `/oauth2/` login page, which is also public.

**Fallback, documented and not built:** if the runbook's flood test shows the dashboard suffering, `math` moves to its own `nginx-unprivileged` container (16 MB, 0.05 CPU) that serves only the math files.

### 5.5 Flood test (runbook, from the PC, against the VPS)

- `oha -z 60s -c 50 https://<domain>/math-assets/<wasm file>` should show mostly `429`s.
- Meanwhile `docker stats` on the VPS should show `web` at or under 32 MB, and `api`, `scanner` and `postgres` unchanged.
- The dashboard should still load while the test runs.
- **A second run with `-c 50`** against `/math` checks that the in-flight limit is per IP: a phone on mobile data (another IP) must still load `/math` during the run.

This test uses a single source IP, so it shows the per-IP limits and the memory cap. It does not show behaviour under a distributed flood (§5.4). The results are recorded in the runbook.

## 6. The playground page

### 6.1 Layout

- **Page:** one column, the same pixel style, a header `isoca-radar · math` and no nav to the dashboard. At the bottom, a line says the live dashboard is private.
- **Sections:** a to e in order. Each has a title, the drawing, controls under it, one deadpan line, a "reset" button, and an honest caption naming the `arb-core` function that computed it.
- **Layout contract:** it works at 360 px with no horizontal scroll, and controls are usable by touch.

### 6.2 Sections

| | Starts from | Controls | Draws |
|---|---|---|---|
| a · one book, twice | the §5.7 Fed book | drag any level's price (0.01 ticks) or size; "cross the book" | both books side by side, reflected prices annotated `(1 − p)`, D1 long and short (`d1_sums`), and a crossed state in the alert colour: *"Crossed book. Somewhere a feed is lying."* |
| b · payoff tables | YES basket, S = 3, Other unlisted | YES/NO/implication, S 2–6, "Other listed" toggle, optional prices per leg | the matrix, the ✗ column, the basket row, and the formula (`1 − Σ c(A)` or `Σ_S v(B) − 1`) with its value from the prices entered |
| c · classify one update | 3 live legs, a small NO-basket edge | per leg: status (live, placeholder, Other, halted, closed NO/YES), best bid and ask; "augmented" toggle | the §5.7 (c) tree with `classify`'s path lit, ending in the kind or "nothing". A static note explains `post_end`: the scanner adds it from event dates and it never changes the kind |
| d · depth walk | 3 legs, 3 levels each, a positive edge | drag levels on up to 4 legs, fees on or off, "exactly 1.00" (sets Σ best asks to 1.00) | the cost staircase from `walk_steps` against payoff 1, including the unprofitable steps after Q*, Π shaded, Q* marked |
| e · episode and latency | the §5.7 (e) five updates, NO basket | scrub updates, `proc_ms`/`net_ms`/`match_ms` sliders (0–1,000 ms each) with presets matching `policies.toml`'s scenarios, `secondsDelay` toggle | Π per update, the episode bar, peak, `t_x` marker, and "caught" or "missed" with captured Π. Caption: teaching model (§4.1) |

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
- **Pages publish, the panel reads.** Pages publish what they already fetched through `useExplainData(page, data)`, and the panel only reads from context.
- **Fetch rule:** the panel issues no fetches of its own, with **one** exception. "Would we have caught it?" on Episode detail calls the existing `api.cards('24h')` (for `measured` and `decay`) through the existing `useApi` hook, and only when that question is opened.
- **Its test:** freeze ticks and SSE (the existing fake EventSource and fake timers), render each page, and record `fetch.mock.calls.length`. Open every question except that one, and the delta must be 0. Opening that one must add exactly one call, to `/api/cards?win=24h`.
- **Selection:**
  - clicking a Radar or Episodes row, an Episode detail page, a Paper position row, or a Papers card sets the selection;
  - rows get a small `explain` affordance (the row click keeps its current navigation behaviour);
  - "back to the page" clears the selection.
- **Honesty rule:**
  - every money figure is labelled *paper*, *on the table*, or *captured at N ms*;
  - panel copy never claims real money. The test renders every question on every page with fixture data and scans the **rendered text of the panel**, `help.ts` sentences included. It fails on the whole words `earned`, `earn`, `made` and `income`, case-insensitive. `profit` is allowed only inside the phrases `profit @ $100`, `net profit at the peak` and `profit after`/`before taker fees` (the existing `help.ts` wording). Any other `profit` fails, and the fix is to reword the copy, `help.ts` included, not to widen the list;
  - the owner's question "What did we generate today?" is kept as a question, and its answer says what kind of number each one is.

### 7.3 Questions per page

Each answer is one schematic plus 2–4 sentences from `src/explain/copy.ts`. Sentences that overlap `help.ts` import from it, so the two never disagree.

| Page | Question | Drawing and source |
|---|---|---|
| Radar | How can this make money? | (b) payoff structure for the selected episode, or else the top open one, or else a stored example labelled *example*. Built from `EpisodeSummary.kind` and `s_legs`. Money figures are the row's `net_budget100` and `net` |
| | What did we generate today? | two bars: edge on the table today (`/api/today` `edge_budget100`) vs captured at the headline latency (the figure the Radar teaser already shows); text says paper, not trades |
| | Why is this row not arbitrage? | shown when the selection's `kind` is `yes_basket_incomplete`: (b) with **one** generic column "an outcome not in the set" marked ✗. `EpisodeSummary` doesn't carry which legs are uncovered, so it doesn't pretend to know |
| | How do I read these columns? | an annotated copy of one row: S, profit @ $100, net and gross @ depth, Q*, age, edge bar |
| Episodes | What is an episode? | a duration bar from the selected row's `opened_ms` / `closed_ms` / `duration_ms`, next to the §5.7 (e) example timeline labelled *example*. Summaries carry no peak or observations, so no per-update Π is drawn here |
| | What does "censored", "stale" or "suspect" mean? | the row's three booleans with their `FLAG_WORDS` sentences (Episodes lists no close reason; none is stored) |
| | What do the flags mean? | the flag legend already built from the shared flag sentences |
| Episode detail | What would buying this have cost? | (d) simplified: one step per leg from its VWAP and Q*, Π at peak. Caption: *"simplified: per-level steps are not stored"* |
| | What happened after it opened? | the (e) timeline from `observations`, drawn as re-checks (+Δd, 1 s, 2 s, 5 s) as in §5.7 (e)'s re-check row, **not** as updates u1–u5 |
| | Would we have caught it? | its `duration_ms` against the measured latency (`CardSeries.measured`) and each scenario's point on the decay chart (`CardSeries.decay`). This is the one question that fetches (§7.2) |
| Paper | Are we making money? | equity vs bankroll per account, realized vs unrealized split, all labelled paper |
| | Why is money locked? | open positions (`returned = false`) as blocks sized by `basis` (their cost), with days to resolution from a **new field `Position.end_date`** (§7.4) |
| | Why do the policies differ? | P0–P3 one line each, with the account's current equity |
| Papers | What does this card test? | the card's claim in one sentence plus its PAPER / THEIR / OUR columns explained |
| | Do we agree with the paper, and why not? | status plus method differences from the card body |
| Health | Are we seeing everything? | hearts decomposed: shards, drops, lag, capture; integrity per million vs Gebele's 20 per 308M |

- **Schematic reuse:** the panel uses the same `src/schematics/` components as the playground, with a `compact` prop (smaller type, no controls).
- **Where drawings come from:** in the panel, drawing (b) comes from a panel-side model builder that reproduces only *labels and structure* (rows, columns, which column is ✗). Every money figure shown comes from the api. A unit test checks the panel's payoff structure against `crates/arb-wasm/fixtures/payoff.json` (the `s` 2–6 cases), so the two can't drift.

### 7.4 The one api change

- **The field:** `GET /api/paper/positions` items gain `end_date: string | null`. It's the event's `end_date` (`events.end_date`, TEXT, already stored), joined through the position's market. It's read-only, with no migration and no new endpoint.
- **Display:** the web shows days to resolution from it, or "unknown" when null.
- **Tests:** an api test with a fixture event, and a web fixture update.

Nothing else in §7.3 needs new data. Answers that would have needed more (uncovered legs on Radar, close reasons, per-update Π on Episodes) were cut or simplified above instead of growing the api.

## 8. Testing

- **Rust:**
  - `payoff`, `d1_sums`, `classify` and `walk_steps` property tests (§3), including "`classify` equals the existing detectors";
  - `arb-wasm` native fixtures, limits and "never panics" (proptest over random JSON-ish input up to 16 KiB returns either a model or `{"error"}`);
  - the existing lints and `cargo deny check bans advisories` (unchanged; `wasm-bindgen` needs no allow entry);
  - an api test for `Position.end_date` (§7.4).
- **Web, parity:** `parity.test.ts` (§4.3).
- **Web, schematics:** each renders from a fixture model, and a snapshot of key SVG attributes checks the ✗ column, Q* and the `t_x` marker.
- **Web, math page:**
  - renders all five sections;
  - a control change updates the drawing (using the real WASM, as in parity);
  - invalid input shows the error sentence;
  - 360 px with no horizontal scroll (the existing responsive test helper).
- **Web, panel:**
  - each question per page renders with `test/fixtures.ts` data;
  - the fetch rule (§7.2): delta 0 except the one named question;
  - selection switches answers and "back" clears them;
  - Esc closes and returns focus;
  - drawer at 360 px, docked at 1,500 px;
  - the banned-words check (§7.2).
- **Build:** the module guard plugin (no dashboard code) and `check-math-bundle.mjs` (size) (§4.4).
- **Deploy:**
  - the flood test (§5.5);
  - `curl -I` without a cookie of `/math` (200), `/math-assets/*.wasm` (200, `application/wasm`, `Content-Encoding: gzip` when requested), `/math/` and `/math.html` (redirect to login), and `/api/today` (redirect to login);
  - `curl -I http://<domain>/math` (plain HTTP) redirects to https.

## 9. Deploy

- **One image change (web)** and label changes in `compose.web.yml`. No new container, no new memory.
- **Runbook:** `docs/superpowers/runbooks/phase7-playground-vps.md` covers the build on the PC, pinning the new web digest, the no-cookie `curl` checks, the flood test and a rollback (the previous web digest).
- **Local (phase A):** `/math` works at `127.0.0.1:8080` like the other pages. There's no Traefik locally, so no rate limit; that's fine, because nothing public runs locally.

## 10. Order of work

1. The `arb-core` additions (§3), then `arb-wasm` with fixtures and native tests.
2. The WASM build script, tsconfig split, CI parity, and Docker stage.
3. `src/schematics/` a–e with fixture models.
4. The math page, its Vite build and the bundle check.
5. nginx, Traefik labels, and the runbook with the flood test.
6. The explain panel's shell (context, tab, drawer and dock).
7. `Position.end_date` (§7.4).
8. Questions page by page: Radar, Episode detail, Episodes, Paper, Papers, Health.

## 11. Open items for the plan

- Pick the exact `wasm-bindgen` version (the latest release at plan time) and pin crate and CLI together.
- Check how `/api/paper/positions` builds its rows (`store/src/read_paper.rs` reads `paper_state` JSON) and find the market → event join for `end_date`.
- Confirm `oha` (or `hey`) is available on the PC for the flood test; otherwise use a small `curl` loop in the runbook.
- Decide precompressed `gzip_static` vs `gzip on` (§5.2).

## 12. Changes after the independent review (2026-09-27)

- **Logic moved into `arb-core`** (`d1_sums`, `classify`, `walk_steps`), so arb-wasm stays a pure facade. `post_end` stays in the scanner and is only labelled in the playground.
- **`episode_json` fully specified:** `gap_ms`, a full `Scenario`, the detector per update, and a precise rule for the captured figure.
- **Traefik:** the router got entrypoints, TLS and service; the limiters are per IP (`sourcecriterion`), priority is 1000, and the no-CDN assumption is stated.
- **Bundle check:** a `moduleIds` build plugin replaced the manifest check.
- **Parity:** fixtures hold strings, decimals are normalized, JSON numbers are rejected, and arb-wasm declares its features explicitly.
- **Build:** a tsconfig split and `WASM_REQUIRED` keep dashboard work Rust-free, and cargo-chef was added to the web Docker Rust stage. The toolchain file is unchanged.
- **Size:** gzip in nginx, so the budget matches what's served.
- **The explain panel's data was checked against `types.ts`:**
  - answers the payloads can't support were cut or simplified;
  - one api field was added (`Position.end_date`);
  - one lazy fetch is allowed and tested.
- **Honesty test:** it scans rendered text, `help.ts` included.
- **Payoff:** `implication` has 4 columns (one impossible), `guaranteed` is defined, and a bad `s` returns an error instead of being clamped.
- **Flood test:** a statement of what it doesn't cover (Traefik TLS cost, distributed floods).
