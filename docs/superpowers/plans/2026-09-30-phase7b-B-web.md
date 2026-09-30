# Phase 7b-B (public /math page, routes, deploy) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the public, login-free `/math` page on radar.isoca.space: sections a–f computed by `arb-core`/`sim` compiled to WASM (f shows why a caught edge still loses when one leg misses), a live strip and the live latency ladder read from the static snapshot `/math-data/public.json`, served by the existing nginx behind a strict Traefik router, then deploy `web` alone.
**Architecture:** Web and deploy only; plan 7b-A (the `arb-wasm` crate with six `*_json` functions and fixtures, and the api writer that keeps `public.json` in the `public-data` volume) is merged and deployed first. Three 7a schematics become public-safe (§7.2). The WASM package is generated into `web/src/wasm/pkg` (gitignored) by `scripts/build-wasm.sh`; only `src/math/**` and `src/wasm/**` import it, split out by `tsconfig.math.json`. Pure views live in `web/src/schematics/` (a `MirrorBooks`, b `PayoffGrid`, c `ClassifyTree`, d `DepthStairs`, e `DurationBars` + `UpdateBars`, f `BasketLegs` + `LatencyLadder`); `web/src/math/` holds the page, its sections (inputs → `call(engine, …)` → a view), the snapshot fetch and a second Vite build (`vite.math.config.ts`, module guard, 350 KB gzip check). nginx serves `/math`, `/math-assets/` and the one file `/math-data/public.json` (read-only volume); Traefik's `ct-math` router sends only those paths there without login.
**Tech Stack:** React 19.2, TypeScript 5.9, Vite 8 (rolldown), Vitest 4 + Testing Library (jsdom), hand-made SVG and HTML, plain CSS; Rust 1.98.1 + `wasm-bindgen` CLI (the version `Cargo.lock` pins) for the package; Docker (rust cargo-chef stage → node 22 → nginx-unprivileged 1.29), Traefik labels in `compose.web.yml`.
**Spec:** docs/superpowers/specs/2026-09-30-phase7b-math-playground-design.md (§6, §7, §5.3, §8 web, §9 7b-B, §11), on docs/superpowers/specs/2026-09-27-phase7-playground-explain-design.md (§4.2–§6 "as 09-27", amended by 7b §11).

> **Ruling (controller, 2026-09-30, from 7b-A preflight P9):** `depth_walk_json`'s `pi`/`pi_gross` are the authoritative totals (conservative fees, as `fill::walk`); `steps[].cost` is for drawing (exact fees) and may differ in the 5th decimal. Never assert Σ steps == `pi`.

> **Ruling (controller, 2026-09-30, Task 10 review):** `depth_walk_json`'s `levels` are the **asks of the outcome being bought** (NO asks for `rule: "no"`), **cheapest first**; they are NOT YES-book levels. Non-ascending levels return `{"error":"levels must be cheapest first"}` (fixture `descending-levels`). Episode boundary fixtures `tx-at-close` and `tx-at-update` pin t_x == close_ms → captured "0" and t_x == an update's t_ms → that update's net.

> **Contract note (controller, 2026-09-30, Task 11 review):** `basket_json`'s `cash.fees` = buy fees + unwind fees + the conversion fee (e.g. `no-convert-fee-50` → fees "0.5"; `no-convert-fee-10000` → fees "100", convert_back "0"). The facade plans exactly as `sim::policy::decide` for open books the page can produce; a crossed open book or an open leg without the side it buys is outside section f's controls.

## Contract used from 7b-A

The contract is `docs/superpowers/plans/2026-09-30-phase7b-A-rust.md` § "Contract for 7b-B" (authoritative; its code was compiled and tested), and the fixture cases in its Tasks 9–11. `web/src/wasm/types.ts` and `web/src/math/snapshot.ts` copy it. What 7b-B reads or sends:

- **Package:** `crates/arb-wasm`, `wasm-bindgen =0.2.129` (CLI the same version, read from `Cargo.lock`); exports `mirror_json`, `payoff_json`, `classify_json`, `depth_walk_json`, `episode_json`, `basket_json`, `(input: string) => string`, never throwing (an `{"error"}` object instead); fixtures `crates/arb-wasm/fixtures/<export>.json` (`mirror_json.json` … `basket_json.json`), `[{name, input, output}]` compared byte for byte.
- **`mirror_json`:** sends `{bids, asks}`; reads `no_asks`, `no_bids`, `best_bid`, `best_ask`, `spread`, `d1_long`, `d1_short`, `crossed`. Section a starts from fixture `fed-book`.
- **`payoff_json`:** sends `{kind, s, other_listed, prices}` (`prices` length `s`, or 2 for `implication`, whose `s` is ignored; the page sends 2); reads `rows`, `cols`, `possible`, `cells`, `basket`, `guaranteed`, `broken`, `top_edge_gross`.
- **`classify_json`:** sends `{augmented, legs: LegIn[]}`; reads `path` (the 14 nodes `decided`, `no_basket_tried`, `no_basket_few_legs`, `no_basket_crossed`, `no_basket_not_positive`, `no_basket_positive`, `yes_basket_tried`, `yes_basket_no_live`, `yes_basket_crossed`, `yes_basket_missing_ask`, `yes_basket_not_positive`, `yes_basket_positive`, `complete`, `incomplete`), `kind`, `realization`, `net`, `gross`, `q_star`. Section c starts from fixture `no-edge-3`.
- **`depth_walk_json`:** sends `{rule: "yes", legs: [{levels, fee}]}`; reads `steps[] {q_from, q_to, cost, payoff, profitable}` (past Q*), `q_star`, `pi`, `pi_gross`. Payoff is read per step (the `no` rule's payoff changes as legs run out).
- **`episode_json`:** sends `{kind, gap_ms, updates: [{t_ms, legs}], scenario: {proc_ms, net_ms, match_ms, honor_seconds_delay}, seconds_delay: int|null}`; reads `updates[] {t_ms, net}`, `open_ms`, `close_ms`, `peak_ms`, `peak_net`, `t_x`, `captured`, `caught`, with `t_x = open_ms + proc + net + match (+ seconds_delay × 1000 when honored)`, `captured` = the net of the latest update at or before `t_x`, `"0"` if the episode closed at or before `t_x`, `caught = captured > 0`.
- **`basket_json`:** sends `{kind: "no", legs: [{open, t_x, unwind, fee}], q_request}` (fee per leg; `min_order` and `convert_fee_bips` omitted, so the defaults `"5"` and `"0"`); reads `kind`, `min_order`, `q_open`, `q_target`, `legs[] {limit, buy {requested, filled, vwap, fees, levels_used}, excess, unwind {…same}}`, `q_eff`, `cash {spent, fees, unwind_back, convert_back, net_cash, held_unsold[] {qty, basis}}`, `value_at_resolution`. Section f's start is fixture `no-all-fill` (`net_cash "2.5"`), "the usual burst" is `no-one-leg-empty-burst` (`q_eff "0"`, `net_cash "-6"`), leg C at 20 % is `no-one-leg-partial`.
- **Snapshot** `GET /math-data/public.json`: `{v: 1, written_ms, ladder {epoch, start_ms, as_of_ms, break_even, measured {latency_ms}|null, markets_status, rungs[], markets[]}, today {day_start_ms, arb, no, yes, edge_budget100, hourly[24]}}`; rungs are `LadderRung` minus `account_id` (keyed by `scenario`); markets rows are NO side, `fee_bips` 0, the six LADDER scenarios, dimensions `all`/`category`. `measured` has no `host`: `publicToLadder` supplies `host: ""`, which `usText` never draws.
- **Deploy:** `compose.web.yml` has the top-level `volumes: public-data: {}` and the api's `- public-data:/var/lib/ct-public` (7b-A Task 16); the api's image pre-creates the directory owned by 65532; `ci.yml` has 7b-A's `wasm` job (wasm32 check, `cargo test -p arb-wasm`); the rust image line in `docker/rust.Dockerfile` is unchanged. Web rollback tag: `web:pre7b-B` (`web:pre7b` is 7a-B's).

## Global Constraints

- **360 px:** the page has no horizontal scroll at 360 px; controls are usable by touch. jsdom cannot see layout: `theme.test.ts` pins the CSS rules (now over `src/math/math.css` too) and the runbook checks `document.documentElement.scrollWidth <= window.innerWidth` in Chrome.
- **SVG rules:** drawings use `preserveAspectRatio="none"`, so every label is HTML beside the SVG, never SVG `<text>`; every SVG part gets its own class name (`mb-*`, `ds-*`, `ub-*`, `bl-*`), never a shared class like `.dot` that global CSS could size; thin strokes carry `vector-effect: non-scaling-stroke`.
- **`web/src/theme.test.ts` rules:** no `width:`/`min-width:` above 328 px; no `minmax(<N>px` (write `minmax(min(100%, Npx), …)`); schematic CSS goes into `web/src/theme.css`, the page's own layout into `web/src/math/math.css`.
- **Drawings over text:** title → big number only where it is the answer → one drawing → controls → one deadpan line → a caption naming the function that computed it; method and tables in closed folds; no box inside a box; prose to a line.
- **Fonts:** Press Start 2P (`var(--px)`) for section titles, chips and buttons only; VT323 (`var(--big)`) for big numbers only; IBM Plex Mono (`var(--body)`) sentence case for labels; tokens from `theme.css` (`--teal`, `--mint`, `--salmon`, `--rose`, `--line2`, `--dim`…).
- **Exact decimals stay strings:** money text goes through `web/src/lib/money.ts` or `<Money>`; `Number(...)` on a decimal string only for SVG geometry, never for a number shown as text. Prices move on the 0.01 grid through `toTick`/`fromTick` (string math).
- **Honest copy:** every `$` figure is labelled `on paper`, `on the table` or `captured at N ms`; the rendered page never uses the whole words `earned`, `earn`, `made`, `income` or `profitable` (Task 12 scans it).
- **Public bundle:** at most **350 KB gzip** for everything under `dist/math-assets/` (JS + WASM + CSS; `check-math-bundle.mjs`); no module under `src/pages/`, `src/api/` (values), `src/explain/` or `src/help.ts` (the `noDashboardCode` guard; type-only imports are erased and allowed); no `/api/` string in any math JS, CSS or HTML; the page's only request besides its own assets is `GET /math-data/public.json`.
- **CSP (math, exact):** `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`. So: no inline `<script>`, no `style=` attribute anywhere in the math page (React `style` props are not used in public components either), fonts from Google Fonts only. The dashboard CSP (`ct-csp`) is unchanged.
- **Traefik `ct-math` (exact):** rule ``Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`) || Path(`/math-data/public.json`))``; `entrypoints=websecure`, `tls.certresolver=letsencrypt`, `service=ct-web`, `priority=1000`; middlewares `ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker` (never `ct-auth`); `ct-math-rl` `ratelimit.average=5`, `ratelimit.burst=20`, `ratelimit.sourcecriterion.ipstrategy.depth=0`; `ct-math-if` `inflightreq.amount=4`, `inflightreq.sourcecriterion.ipstrategy.depth=0`.
- **WASM on the PC:** `PROFILE=dev sh scripts/build-wasm.sh` for tests; `npm run build` (the 350 KB budget) needs the release package, so Task 13 and the runbook run `sh scripts/build-wasm.sh` (release, `arb-wasm` only). The lead approved that; the owner's OK is still pending, so Task 13 step 5 and runbook step 2 ask for it first. `wasm-bindgen-cli 0.2.129` is installed by the owner (7b-A); if `wasm-bindgen --version` differs from `Cargo.lock`, **stop and ask**.
- **Engine numbers come from 7b-A's fixtures:** a test that asserts an engine output either compares with the fixture's `output` string for the same input, or reads the value from the engine for the page's input (wiring); no hand-computed engine figures.
- **No new npm dependency** (runtime or dev); `package.json` changes only in `scripts`; `package-lock.json` untouched; no fresh `npm install` in the worktree (reuse `web/node_modules`).
- **UI tasks (6, 7, 8, 9, 10, 11, 12):** the implementer loads the `frontend-design:frontend-design` skill before writing components or CSS, within the constraints above.
- **Test commands (from the worktree root):** one file `cd web && npx vitest run <path>`; all `cd web && npm test`; types `cd web && npx tsc --noEmit` (dashboard) and, from Task 4 on, `cd web && npx tsc --noEmit -p tsconfig.math.json` (math); vitest does not catch unused imports (`noUnusedLocals` is on). `WASM_REQUIRED=1` in front makes a missing package a failure instead of a skip. Every task ends with `cd web && npm test && npx tsc --noEmit` green (plus the math tsc from Task 4 on).
- **Shell:** Git Bash; Rust commands with `export PATH="$HOME/.cargo/bin:$PATH" &&`; never print `.env`; `docker compose config` only as `docker compose --env-file .env.example … config -q`; never restart Docker; Docker builds and every VPS step are the owner's (runbook, Task 15).
- **Commits:** stage only the task's files (`git add <paths>`, never `-A` or `.`); message style `area: sentence`; commit messages NEVER carry a `Co-Authored-By` line or any other trailer.

## Review Focus

1. **The snapshot file in a shape the page did not expect:** a 404, a network error, an HTML page with 200, `v: 2`, a `v: 1` body missing `today` or with a rung whose `gains` is a JSON number, a `written_ms` in the future (clock skew), a failed re-fetch after a good load. Each must read "live numbers unavailable" (or fresh for the future stamp, or keep the last good snapshot), never throw, never draw `NaN` or a negative age. Pinned in Task 3 by `rejects every shape the page cannot draw` and `a written_ms in the future is fresh; exactly 30 min is fresh, one ms more is stale`, and in Task 11 by `404, a network error, an HTML page and v 2 all read missing` and `a failed re-fetch keeps the last good snapshot`.
2. **The engine misbehaving or fed junk by the UI:** a trap after a Rust panic (`RuntimeError: unreachable`), a non-JSON string, an array, `{"error"}`, and the UI's own invalid input (a size of 0, two levels dragged onto one price). The section shows one sentence in `role="alert"` and the other sections keep working. Pinned in Task 4 by `turns a trap, non-JSON and an array into one sentence` and in Task 9 by `a: a size of 0 is the engine's sentence, and the page keeps working`.
3. **Degenerate geometry (`NaN` in an SVG attribute):** an empty book, a zero-depth leg (no steps, Q* 0), every leg drained (nothing filled, `q_eff` 0), an episode that never opens (`open_ms` null) or never closes (`close_ms` null), `t_x` null. Pinned in Task 6 by `an empty book draws no bars and no NaN`, Task 7 by `no steps and Q* 0 draw no NaN` and `nothing filled says so and draws no NaN`, Task 8 by `an episode that never opens or never closes draws no NaN`.
4. **Contract drift with 7b-A:** a fixture file missing or extra, a section's start drifting from the fixture it copies (`fed-book`, `no-edge-3`, `no-all-fill`, `no-one-leg-empty-burst`), a classify node this page does not know, and the snapshot's rungs without `account_id` (every React key `undefined`, rows collapsing). Pinned in Task 4 by `has exactly one fixture file per engine export`, Tasks 9–10 by the `… is fixture …` tests, Task 6 by `shows a node the page does not know instead of dropping it`, Task 2 by `draws a ladder whose rungs carry no account_id, keyed by scenario`.
5. **Dashboard code reaching the public bundle** through a reused module (a value import of `explain/models`, `components/EmptyState` → `api/client`, `help.ts`), or a `/api/` string shipping. Pinned in Task 1 by `the reused schematics reach no dashboard module by a value import`, and in Task 13 by `bundles the reused schematics and nothing from the dashboard` and `fails on "/api/" in an asset`.

---

## File Structure

```
.gitignore, .dockerignore                              MODIFY: web/src/wasm/pkg
scripts/build-wasm.sh                                  CREATE: cargo → wasm-bindgen into web/src/wasm/pkg
.github/workflows/ci.yml                               MODIFY: web job builds the engine, WASM_REQUIRED=1, full build
web/package.json                                       MODIFY: scripts (build = both builds + size check; build:dashboard)
web/tsconfig.json                                      MODIFY: exclude src/math, src/wasm
web/tsconfig.math.json                                 CREATE
web/vite.math.config.ts                                CREATE: math.html → dist/math-assets, noDashboardCode guard
web/math.html                                          CREATE
web/scripts/check-math-bundle.mjs                      CREATE: 350 KB gzip, no "/api/", writes .gz for gzip_static
web/src/schematics/models.ts                           CREATE: PayoffModel, DurationRow, durationText (moved from explain)
web/src/schematics/Empty.tsx                           CREATE: the plain empty line (no api import)
web/src/schematics/{MirrorBooks,ClassifyTree,DepthStairs,BasketLegs,UpdateBars}.tsx   CREATE: a, c, d, f, e's Π per update
web/src/schematics/{LatencyLadder,DurationBars,PayoffGrid}.tsx                        MODIFY: public-safe, scenario keys, implication, order bar
web/src/explain/models.ts                              MODIFY: re-exports the moved models
web/src/components/EmptyState.tsx                      MODIFY: renders Empty
web/src/components/SweetLadderGrid.tsx                 MODIFY: rows: GridRow[]
web/src/lib/ladder.ts                                  MODIFY: Rung, LadderIn
web/src/lib/sweet.ts                                   MODIFY: GridRow, generic grid
web/src/wasm/{types,call,engine,testEngine}.ts         CREATE: contract types, safe call, loader, test loader
web/src/wasm/parity.test.ts                            CREATE
web/src/math/{snapshot,useSnapshot,adapt,basket,defaults,copy,useFrameThrottle,fixtures}.ts  CREATE
web/src/math/{LiveStrip,LiveLadder,MathPage,main}.tsx  CREATE
web/src/math/sections/{Section,MirrorSection,PayoffSection,ClassifySection,DepthSection,EpisodeSection,LegMissSection}.tsx  CREATE
web/src/math/math.css                                  CREATE
web/src/theme.css, web/src/theme.test.ts               MODIFY
web/src/deploy.test.ts                                 CREATE: ci.yml, Dockerfile, nginx, compose checks
docker/web.Dockerfile                                  MODIFY: wasm stages (cargo-chef), pkg into the node stage
docker/web/nginx.conf                                  MODIFY: /math, /math-assets/, /math-data/public.json
compose.web.yml                                        MODIFY: web volume mount, ct-math router and middlewares
docs/superpowers/runbooks/phase7b-web-vps.md           CREATE
```

---

### Task 1: the reused schematics become public-safe (models and Empty move out of dashboard modules)

**Files:**
- Create: `web/src/schematics/models.ts`, `web/src/schematics/Empty.tsx`
- Modify: `web/src/explain/models.ts` (line 3 import; delete `PayoffModel` lines 21-32, `DurationRow` lines 50-57, `durationText` at the end), `web/src/schematics/DurationBars.tsx:1`, `web/src/schematics/PayoffGrid.tsx:1`, `web/src/components/EmptyState.tsx:1-5`, `web/src/schematics/LatencyLadder.tsx` (line 3 import and the three `<EmptyState` uses)
- Test: `web/src/schematics/publicSafe.test.ts`, `web/src/schematics/models.test.ts`, `web/src/schematics/Empty.test.tsx` (new)

**Interfaces:**
- Consumes: `fmtDuration(ms: number): string` (`lib/time.ts`).
- Produces:

```ts
// web/src/schematics/models.ts
export interface PayoffModel { kind: 'yes' | 'no'; s: number; cols: string[]; rows: { name: string; cells: (0 | 1)[] }[]; basket: number[]; guaranteed: number; broken: boolean[] }
export interface DurationRow { kind: 'example' | 'row'; label: string; ms: number; open: boolean; ticks: number[] }
export function durationText(r: DurationRow): string
// web/src/schematics/Empty.tsx
export function Empty({ text }: { text: string }): JSX.Element   // <p className="quip empty">{text}</p>
// web/src/explain/models.ts keeps exporting durationText, DurationRow, PayoffModel (re-exports)
```

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/schematics/publicSafe.test.ts
// Spec 7b §7.2: the schematics the public /math bundle reuses must not reach dashboard code through a
// value import (type-only imports are erased). vite.math.config.ts's guard proves it on the built bundle
// (Task 13); this walks the source, so `npm test` shows it without a build.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const ROOT = `${process.cwd()}/`;
/** 09-27 §4.4: what the math bundle must never hold. */
const DASHBOARD = /^src\/(pages|api|explain)\/|^src\/help\.ts$/;
/** The public schematics; Tasks 6–8 add theirs. */
const ENTRIES = ['src/schematics/LatencyLadder.tsx', 'src/schematics/DurationBars.tsx', 'src/schematics/PayoffGrid.tsx'];
/** `import x from '…'`, `import { type A, b } from '…'`, `export … from '…'`, `import '…'`; never `import type` / `export type`. */
const IMPORT = /^\s*(?:import|export)\s+(?!type\b)[^'"]*?\s*from\s*'([^']+)'|^\s*import\s+'([^']+)'/gm;

function resolve(from: string, spec: string): string | null {
  if (!spec.startsWith('.')) return null; // a package (react, …)
  const parts: string[] = [];
  for (const p of `${from.slice(0, from.lastIndexOf('/') + 1)}${spec}`.split('/')) {
    if (p === '..') parts.pop();
    else if (p !== '.' && p !== '') parts.push(p);
  }
  const path = parts.join('/');
  for (const c of [path, `${path}.ts`, `${path}.tsx`]) {
    if (/\.(tsx?|css)$/.test(c) && existsSync(ROOT + c)) return c;
  }
  return null;
}

/** Every file reached from `entries` by value imports, sorted. */
function reach(entries: string[]): string[] {
  const seen = new Set<string>();
  const todo = [...entries];
  while (todo.length > 0) {
    const f = todo.pop() as string;
    if (seen.has(f)) continue;
    seen.add(f);
    if (!/\.tsx?$/.test(f)) continue;
    const src: string = readFileSync(ROOT + f, 'utf8');
    for (const m of src.matchAll(IMPORT)) {
      const r = resolve(f, (m[1] ?? m[2]).split('?')[0]);
      if (r !== null) todo.push(r);
    }
  }
  return [...seen].sort();
}

describe('public-safe schematics (spec 7b §7.2)', () => {
  it('the reused schematics reach no dashboard module by a value import (review focus 5)', () => {
    expect(reach(ENTRIES).filter((f) => DASHBOARD.test(f))).toEqual([]);
  });

  it('the walker follows value imports, so the check can fail', () => {
    expect(reach(['src/components/EmptyState.tsx'])).toContain('src/api/client.ts');
    expect(reach(['src/schematics/LockedBlocks.tsx'])).toContain('src/explain/models.ts');
  });
});
```

```ts
// web/src/schematics/models.test.ts
import { describe, expect, it } from 'vitest';
import { durationText as fromExplain, payoffStructure } from '../explain/models';
import { durationText } from './models';

describe('schematics/models (spec 7b §7.2)', () => {
  it('explain/models re-exports the moved durationText, so the panel is unchanged', () => {
    expect(fromExplain).toBe(durationText);
    expect(payoffStructure('yes', 3, true).broken).toEqual([false, false, false, true]);
  });

  it('reads the example and a still-open row', () => {
    expect(durationText({ kind: 'example', label: 'example', ms: 3800, open: false, ticks: [] })).toBe('example: 3.80 s, from u2 to u5, peak at u3');
    expect(durationText({ kind: 'row', label: 'Fed', ms: 125_000, open: true, ticks: [] })).toBe('Fed: still open, 2 min 5 s so far');
    expect(durationText({ kind: 'row', label: 'Fed', ms: 4190, open: false, ticks: [] })).toBe('Fed: 4.19 s');
  });
});
```

```tsx
// web/src/schematics/Empty.test.tsx
import { cleanup, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EmptyState } from '../components/EmptyState';
import { Empty } from './Empty';

describe('Empty (spec 7b §7.2)', () => {
  it('draws the plain line, and the dashboard EmptyState draws the same markup', () => {
    const a = render(<Empty text="nothing yet" />).container.innerHTML;
    cleanup();
    const b = render(<EmptyState text="nothing yet" />).container.innerHTML;
    expect(a).toBe('<p class="quip empty">nothing yet</p>');
    expect(b).toBe(a);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/schematics/publicSafe.test.ts src/schematics/models.test.ts src/schematics/Empty.test.tsx`
Expected: FAIL: `publicSafe` lists `src/api/client.ts` (LatencyLadder → components/EmptyState) and `src/explain/models.ts` (DurationBars → durationText); `models.test.ts` and `Empty.test.tsx`: `Failed to resolve import "./models"` / `"./Empty"`.

- [ ] **Step 3: Implement**

`web/src/schematics/models.ts`:

```ts
// Spec 7b §7.2: the view models the public /math page shares with the explain panel. They moved here from
// src/explain/models.ts (which re-exports them), so no public schematic imports dashboard code.
import { fmtDuration } from '../lib/time';

export interface PayoffModel {
  kind: 'yes' | 'no';
  /** The basket's real number of outcomes (may exceed the rows drawn). */
  s: number;
  cols: string[];
  rows: { name: string; cells: (0 | 1)[] }[];
  /** What the whole basket pays in each column, over all s outcomes. */
  basket: number[];
  /** Parent spec §5.7 (b): 1 for a YES basket, s − 1 for a NO basket. */
  guaranteed: number;
  broken: boolean[];
}

export interface DurationRow {
  kind: 'example' | 'row';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open (the example only). */
  ticks: number[];
}

export function durationText(r: DurationRow): string {
  if (r.kind === 'example') return `example: ${fmtDuration(r.ms)}, from u2 to u5, peak at u3`;
  return r.open ? `${r.label}: still open, ${fmtDuration(r.ms)} so far` : `${r.label}: ${fmtDuration(r.ms)}`;
}
```

`web/src/schematics/Empty.tsx`:

```tsx
/** Spec 7b §7.2: the plain empty-state line. No api import, so the public bundle can use it; the dashboard's
 *  EmptyState renders it and keeps its ApiError/session handling (errorText, ErrorState) beside it. */
export function Empty({ text }: { text: string }) {
  return <p className="quip empty">{text}</p>;
}
```

`web/src/explain/models.ts`: replace line 3 (`import { DAY_MS, fmtDuration } from '../lib/time';`) with

```ts
import { DAY_MS } from '../lib/time';
import type { DurationRow, PayoffModel } from '../schematics/models';

export { durationText } from '../schematics/models';
export type { DurationRow, PayoffModel } from '../schematics/models';
```

and delete the `export interface PayoffModel { … }` block, the `export interface DurationRow { … }` block and the `export function durationText(…) { … }` function (the last one in the file). `payoffStructure`, `durationRows`, `EXAMPLE_EPISODE`, `positionKey`, `daysText` stay as they are.

`web/src/schematics/DurationBars.tsx` line 1: `import { type DurationRow, durationText } from './models';`
`web/src/schematics/PayoffGrid.tsx` line 1: `import type { PayoffModel } from './models';`

`web/src/components/EmptyState.tsx` lines 1-5 become:

```tsx
import { ApiError, SESSION_EXPIRED_TEXT } from '../api/client';
import { Empty } from '../schematics/Empty';

/** The dashboard's empty line: the public view (spec 7b §7.2); the api error handling stays below. */
export function EmptyState({ text }: { text: string }) {
  return <Empty text={text} />;
}
```

`web/src/schematics/LatencyLadder.tsx`: replace line 3 (`import { EmptyState } from '../components/EmptyState';`) with `import { Empty } from './Empty';`, and the three `<EmptyState text={…} />` with `<Empty text={…} />` (in `Markets` twice and in `LadderTable` once).

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/schematics src/explain src/components/EmptyState.test.tsx`
Expected: PASS (the 7a schematic and panel tests unchanged).

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/schematics/models.ts web/src/schematics/Empty.tsx web/src/schematics/publicSafe.test.ts web/src/schematics/models.test.ts web/src/schematics/Empty.test.tsx web/src/explain/models.ts web/src/schematics/DurationBars.tsx web/src/schematics/PayoffGrid.tsx web/src/components/EmptyState.tsx web/src/schematics/LatencyLadder.tsx
git commit -m "web: public-safe schematics: durationText and the view models move to schematics/models, Empty split from EmptyState"
```

---

### Task 2: the ladder reads rungs without `account_id` (`Rung`, `LadderIn`, `GridRow`), keyed by scenario

**Files:**
- Modify: `web/src/lib/ladder.ts` (imports lines 1-2; `LadderColumn`, `LadderModel`, `ladderModel`, `stackRung`, `yDomain`, `extraText` signatures), `web/src/lib/sweet.ts` (`Grid`, `grid`, `interval`, `glyphKey`, `LadderGrid`, `ladderGrid`, `cellDetail` signatures; new `GridRow`), `web/src/components/SweetLadderGrid.tsx` (imports, `rows` prop), `web/src/schematics/LatencyLadder.tsx` (imports, `LadderTable`, three keys)
- Test: `web/src/schematics/LatencyLadder.test.tsx` (imports lines 1-6; one new `describe` at the end)

**Interfaces:**
- Consumes: `LadderRung`, `BreakEven`, `MarketsStatus`, `SweetRow` (`api/types`, type-only).
- Produces:

```ts
// lib/sweet.ts
export type GridRow = Pick<SweetRow, 'side' | 'scenario' | 'fee_bips' | 'latency_ms' | 'dimension' | 'bucket' | 'label' | 'n_exec' | 'clusters' | 'mean' | 'lo' | 'hi'>;
export interface Grid<R extends GridRow = SweetRow> { scenarios: { name: string; latency_ms: number | null }[]; buckets: string[]; cell: (bucket: string, scenario: string) => R | undefined }
export function grid<R extends GridRow = SweetRow>(s: { rows: R[] }, side: SweetSide, dim: SweetDim): Grid<R>
export function ladderGrid(rows: GridRow[], side: SweetSide, dim: SweetDim, only?: string[]): LadderGrid   // cell → GridRow | undefined
export function interval(r: Pick<SweetRow, 'mean' | 'lo' | 'hi'>): string
export function glyphKey(r: Pick<SweetRow, 'label' | 'mean'> | undefined): GlyphKey
export function cellDetail(row: LadderGridRow, col: { scenario: string; latency_ms: number }, r: GridRow | undefined): string
// lib/ladder.ts
export type Rung = Omit<LadderRung, 'account_id'>;
export interface LadderIn { epoch: number; start_ms: number | null; as_of_ms: number | null; rungs: Rung[]; markets: GridRow[]; measured: { latency_ms: number; host?: string } | null; markets_status: MarketsStatus; break_even: BreakEven }
export function ladderModel(l: LadderIn): LadderModel   // body unchanged; the api's Ladder is a LadderIn
// LadderModel: columns[].rung: Rung; extras: Rung[]; measured: { latency_ms: number } | null; markets.rows: GridRow[]
// SweetLadderGrid({ rows: GridRow[], … })
```

- [ ] **Step 1: Write the failing test**

In `web/src/schematics/LatencyLadder.test.tsx` replace the import lines 1-6 with:

```tsx
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Ladder, LadderRung, SweetRow } from '../api/types';
import { LADDER, type LadderIn, NOT_MONOTONE, type Rung, SLOTS, ladderModel, slotX } from '../lib/ladder';
import type { GridRow } from '../lib/sweet';
import { ladderFixture, ladderLiveFixture } from '../test/fixtures';
import { LADDER_CAPTION, LatencyLadder, MARKETS_CAPTION, MARKETS_MISSING, MARKETS_RECOMPUTING, MARKETS_WARMING } from './LatencyLadder';
```

and append at the end of the file:

```tsx
/** A rung as the public snapshot carries it (spec 7b §5.1): no account id. */
const noAccount = (r: LadderRung): Rung => {
  const { account_id, ...rest } = r;
  void account_id;
  return rest;
};
/** A Sweet row with only the fields the grid reads. */
const gridOnly = (r: SweetRow): GridRow => ({
  side: r.side, scenario: r.scenario, fee_bips: r.fee_bips, latency_ms: r.latency_ms, dimension: r.dimension, bucket: r.bucket,
  label: r.label, n_exec: r.n_exec, clusters: r.clusters, mean: r.mean, lo: r.lo, hi: r.hi,
});

describe('LatencyLadder on the public snapshot (spec 7b §7.2)', () => {
  it('draws a ladder whose rungs carry no account_id, keyed by scenario (review focus 4)', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const l = ladderFixture();
    const input: LadderIn = { ...l, measured: { latency_ms: 155 }, rungs: l.rungs.map(noAccount), markets: l.markets.map(gridOnly) };
    const { container } = render(<LatencyLadder model={ladderModel(input)} />);
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(container.querySelectorAll('circle.ll-net')).toHaveLength(6);
    expect(container.querySelectorAll('line.ll-extra')).toHaveLength(2);
    expect(within(screen.getByRole('list', { name: 'extras' })).getAllByRole('listitem')).toHaveLength(2);
    expect(within(screen.getByRole('table', { name: 'which markets' })).getAllByRole('button').length).toBeGreaterThan(0);
    expect(error).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/schematics/LatencyLadder.test.tsx`
Expected: FAIL on the new test: `expected "error" to not be called` (React: `Each child in a list should have a unique "key" prop`, the rows keyed by `account_id` are all `undefined`); the older tests pass.

- [ ] **Step 3: Implement**

`web/src/lib/sweet.ts`: after the `import` lines add

```ts
/** Spec 7b §5.1: the fields the grid reads, which is all the public snapshot's rows carry. */
export type GridRow = Pick<
  SweetRow,
  'side' | 'scenario' | 'fee_bips' | 'latency_ms' | 'dimension' | 'bucket' | 'label' | 'n_exec' | 'clusters' | 'mean' | 'lo' | 'hi'
>;
```

and replace the `Grid` interface and the `grid`, `interval`, `glyphKey` signatures, the `LadderGrid` interface and the `ladderGrid`, `cellDetail` signatures (bodies unchanged):

```ts
export interface Grid<R extends GridRow = SweetRow> {
  scenarios: { name: string; latency_ms: number | null }[];
  buckets: string[];
  cell: (bucket: string, scenario: string) => R | undefined;
}

export function grid<R extends GridRow = SweetRow>(s: { rows: R[] }, side: SweetSide, dim: SweetDim): Grid<R> {
export function interval(r: Pick<SweetRow, 'mean' | 'lo' | 'hi'>): string {
export function glyphKey(r: Pick<SweetRow, 'label' | 'mean'> | undefined): GlyphKey {

export interface LadderGrid {
  columns: { scenario: string; latency_ms: number }[];
  rows: LadderGridRow[];
  cell: (row: LadderGridRow, scenario: string) => GridRow | undefined;
}

export function ladderGrid(rows: GridRow[], side: SweetSide, dim: SweetDim, only?: string[]): LadderGrid {
export function cellDetail(row: LadderGridRow, col: { scenario: string; latency_ms: number }, r: GridRow | undefined): string {
```

`rowsFor`, `bestCandidate`, `rowName` and `pages/Sweet.tsx` keep `SweetRow` (`grid(s, …)` with a `SweetSeries` infers `R = SweetRow`).

`web/src/lib/ladder.ts`: replace the import lines 1-2 with

```ts
import type { BreakEven, LadderRung, MarketsStatus } from '../api/types';
import type { GridRow } from './sweet';
import { fmtUtcDayHm, fmtUtcHm } from './time';
```

insert before `export type LadderColumn`:

```ts
/** Spec 7b §7.2: a rung as the public snapshot carries it (no account id); the api's LadderRung is one too. */
export type Rung = Omit<LadderRung, 'account_id'>;

/** What ladderModel reads: the api's Ladder, or the public snapshot's ladder through publicToLadder. */
export interface LadderIn {
  epoch: number;
  start_ms: number | null;
  as_of_ms: number | null;
  rungs: Rung[];
  markets: GridRow[];
  /** The api's Ladder carries the host; the snapshot's publicToLadder passes "" (never drawn: usText reads latency_ms only). */
  measured: { latency_ms: number; host?: string } | null;
  markets_status: MarketsStatus;
  break_even: BreakEven;
}
```

and change these declarations (bodies unchanged):

```ts
  | { kind: 'rung'; scenario: string; latency_ms: number; latencyKnown: boolean; rung: Rung };
// in LadderModel:
  extras: Rung[];
  measured: { latency_ms: number } | null;
  markets: { status: MarketsStatus; rows: GridRow[] };
export function ladderModel(l: LadderIn): LadderModel {
export function stackRung(r: Pick<Rung, 'gains' | 'settle' | 'unrealized' | 'leg_risk'>): Segment[] {
export function yDomain(rungs: Rung[]): { lo: number; hi: number } {
export function extraText(r: Rung): string {
```

`web/src/components/SweetLadderGrid.tsx`: delete `import type { SweetRow } from '../api/types';`, add `type GridRow,` to the `../lib/sweet` import list, and change the prop to `rows: GridRow[];`.

`web/src/schematics/LatencyLadder.tsx`: delete `import type { LadderRung } from '../api/types';`, add `type Rung,` to the `../lib/ladder` import list, change `function LadderTable({ rungs }: { rungs: LadderRung[] })` to `function LadderTable({ rungs }: { rungs: Rung[] })`, and replace the three `key={r.account_id}` with `key={r.scenario}` (the table row, the extras `<line>` and the extras `<li>`). Scenario names are unique per ladder (rungs and extras together).

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/schematics src/lib src/components/SweetLadderGrid.test.tsx src/pages/Sweet.test.tsx src/explain`
Expected: PASS.

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/ladder.ts web/src/lib/sweet.ts web/src/components/SweetLadderGrid.tsx web/src/schematics/LatencyLadder.tsx web/src/schematics/LatencyLadder.test.tsx
git commit -m "web: LatencyLadder keys rows by scenario and reads rungs without account_id (Rung, LadderIn, GridRow)"
```

---
### Task 3: the public snapshot contract (types, parse, freshness, `publicToLadder`)

**Files:**
- Create: `web/src/math/snapshot.ts`, `web/src/math/fixtures.ts` (test helper)
- Test: `web/src/math/snapshot.test.tsx`

**Interfaces:**
- Consumes: `LadderIn` (Task 2), `BreakEven`, `MarketsStatus`, `SweetLabel` (`api/types`, type-only), `ladderFixture()` (`test/fixtures.ts`, tests only).
- Produces:

```ts
export const SNAPSHOT_URL = '/math-data/public.json';
export const FRESH_MS = 1_800_000;   // 30 min
export const REFETCH_MS = 300_000;   // 5 min
export interface PublicRung { scenario: string; latency_ms: number | null; extra: boolean; n_exec: number; n_leg_risk: number; n_missed: number; gains: string; leg_risk: string; settle: string; unrealized: string; net: string }
export interface PublicSweetRow { side: 'no'; scenario: string; fee_bips: 0; latency_ms: number | null; dimension: 'all' | 'category'; bucket: string; label: SweetLabel; n_exec: number; clusters: number; mean: string | null; lo: string | null; hi: string | null }
export interface PublicLadder { epoch: number; start_ms: number | null; as_of_ms: number | null; break_even: BreakEven; measured: { latency_ms: number } | null; markets_status: MarketsStatus; rungs: PublicRung[]; markets: PublicSweetRow[] }
export interface PublicToday { day_start_ms: number; arb: number; no: number; yes: number; edge_budget100: string | null; hourly: { hour: number; arb: number }[] }
export interface PublicSnapshot { v: 1; written_ms: number; ladder: PublicLadder; today: PublicToday }
export function parseSnapshot(raw: unknown): PublicSnapshot | null
export function freshness(s: Pick<PublicSnapshot, 'written_ms'>, nowMs: number): 'fresh' | 'stale'
export function publicToLadder(l: PublicLadder): LadderIn   // measured gets host ""
export function hourlyValues(t: Pick<PublicToday, 'hourly'>): number[]   // 24 slots, UTC hours
// fixtures.ts (tests only)
export function snapshotFixture(o?: Partial<PublicSnapshot>, writtenMs?: number): PublicSnapshot
```

- [ ] **Step 1: Write the failing test**

```ts
// web/src/math/fixtures.ts
// Test helper: the public snapshot 7b-A writes (spec 7b §5.1), built from the 7a ladder fixture minus the
// fields that never ship (account ids, the host), plus today's counts.
import { ladderFixture } from '../test/fixtures';
import type { PublicSnapshot } from './snapshot';

export function snapshotFixture(o: Partial<PublicSnapshot> = {}, writtenMs: number = Date.now()): PublicSnapshot {
  const l = ladderFixture();
  return {
    v: 1,
    written_ms: writtenMs,
    ladder: {
      epoch: l.epoch,
      start_ms: l.start_ms,
      as_of_ms: l.as_of_ms,
      break_even: l.break_even,
      measured: l.measured === null ? null : { latency_ms: l.measured.latency_ms },
      markets_status: l.markets_status,
      rungs: l.rungs.map((r) => ({
        scenario: r.scenario, latency_ms: r.latency_ms, extra: r.extra, n_exec: r.n_exec, n_leg_risk: r.n_leg_risk, n_missed: r.n_missed,
        gains: r.gains, leg_risk: r.leg_risk, settle: r.settle, unrealized: r.unrealized, net: r.net,
      })),
      // 7b-A: NO side, fee 0, the LADDER scenarios, `all` and `category` only.
      markets: l.markets.flatMap((r) =>
        r.side === 'no' && r.fee_bips === 0 && (r.dimension === 'all' || r.dimension === 'category')
          ? [{
              side: 'no' as const, scenario: r.scenario, fee_bips: 0 as const, latency_ms: r.latency_ms, dimension: r.dimension, bucket: r.bucket,
              label: r.label, n_exec: r.n_exec, clusters: r.clusters, mean: r.mean, lo: r.lo, hi: r.hi,
            }]
          : [],
      ),
    },
    today: {
      day_start_ms: Date.UTC(2026, 8, 30),
      arb: 15,
      no: 12,
      yes: 3,
      edge_budget100: '41.37',
      // 7b-A writes 24 entries, hours 0–23 UTC
      hourly: Array.from({ length: 24 }, (_, hour) => ({ hour, arb: [2, 0, 5, 8][hour] ?? 0 })),
    },
    ...o,
  };
}
```

```tsx
// web/src/math/snapshot.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { snapshotFixture } from './fixtures';
import { FRESH_MS, freshness, hourlyValues, parseSnapshot, publicToLadder } from './snapshot';

/** What fetch().json() hands over: plain JSON (undefined fields gone). */
const json = (x: unknown): unknown => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));

describe('parseSnapshot (spec 7b §5.1, §6.3)', () => {
  it('accepts the snapshot 7b-A writes', () => {
    const s = snapshotFixture({}, 1_790_000_000_000);
    expect(parseSnapshot(json(s))).toEqual(s);
    expect(JSON.stringify(s)).not.toMatch(/account_id|"host"|vps-kvm2/);
  });

  it('rejects every shape the page cannot draw (review focus 1)', () => {
    const b = snapshotFixture();
    const bad: [string, unknown][] = [
      ['null', null],
      ['a string', 'nope'],
      ['an array', []],
      ['v 2', { ...b, v: 2 }],
      ['no written_ms', { ...b, written_ms: undefined }],
      ['no today', { ...b, today: undefined }],
      ['no ladder', { ...b, ladder: undefined }],
      ['a rung whose gains is a JSON number', { ...b, ladder: { ...b.ladder, rungs: [{ ...b.ladder.rungs[0], gains: 3.8 }] } }],
      ['a rung without a scenario', { ...b, ladder: { ...b.ladder, rungs: [{ ...b.ladder.rungs[0], scenario: undefined }] } }],
      ['an unknown break_even', { ...b, ladder: { ...b.ladder, break_even: { kind: 'maybe' } } }],
      ['break_even at without a latency', { ...b, ladder: { ...b.ladder, break_even: { kind: 'at' } } }],
      ['an unknown markets_status', { ...b, ladder: { ...b.ladder, markets_status: 'busy' } }],
      ['a market row without a label', { ...b, ladder: { ...b.ladder, markets: [{ ...b.ladder.markets[0], label: undefined }] } }],
      ['a YES-side market row', { ...b, ladder: { ...b.ladder, markets: [{ ...b.ladder.markets[0], side: 'yes' }] } }],
      ['a market row with a fee', { ...b, ladder: { ...b.ladder, markets: [{ ...b.ladder.markets[0], fee_bips: 10 }] } }],
      ['hourly not a list', { ...b, today: { ...b.today, hourly: {} } }],
      ['edge as a JSON number', { ...b, today: { ...b.today, edge_budget100: 41.37 } }],
    ];
    for (const [name, x] of bad) expect(parseSnapshot(json(x)), name).toBeNull();
  });

  it('a written_ms in the future is fresh; exactly 30 min is fresh, one ms more is stale (review focus 1)', () => {
    const now = 1_790_000_000_000;
    expect(freshness({ written_ms: now + 60_000 }, now)).toBe('fresh');
    expect(freshness({ written_ms: now - FRESH_MS }, now)).toBe('fresh');
    expect(freshness({ written_ms: now - FRESH_MS - 1 }, now)).toBe('stale');
  });

  it('spreads the hourly counts over 24 UTC hours and ignores an hour out of range', () => {
    const v = hourlyValues({ hourly: [{ hour: 0, arb: 2 }, { hour: 3, arb: 5 }, { hour: 24, arb: 9 }, { hour: -1, arb: 9 }] });
    expect(v).toHaveLength(24);
    expect(v.slice(0, 5)).toEqual([2, 0, 0, 5, 0]);
    expect(v.reduce((a, x) => a + x, 0)).toBe(7);
  });
});

describe('publicToLadder (spec 7b §7.2)', () => {
  it('feeds ladderModel and LatencyLadder unchanged, and drops a market row of another dimension', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const s = snapshotFixture();
    const drift = { ...s.ladder, markets: [...s.ladder.markets, { ...s.ladder.markets[0], dimension: 'days' as 'all', bucket: '1-7' }] };
    const l = publicToLadder(drift);
    expect(l.markets).toHaveLength(s.ladder.markets.length);
    expect(l.measured).toEqual({ latency_ms: 155, host: '' });
    const { container } = render(<LatencyLadder model={ladderModel(l)} />);
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(container.querySelectorAll('circle.ll-net')).toHaveLength(6);
    expect(screen.getByText('us (measured ≈155 + matching)')).toBeInTheDocument();
    expect(error).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/math/snapshot.test.tsx`
Expected: FAIL: `Failed to resolve import "./snapshot"`.

- [ ] **Step 3: Implement**

```ts
// web/src/math/snapshot.ts
// Spec 7b §5.1 and §6.3: the public snapshot the api writes every 5 minutes (plan 7b-A) and nginx serves
// as a static file. The types copy 7b-A's "Contract for 7b-B" (the fields the page reads);
// parseSnapshot turns anything else (bad JSON, v ≠ 1, a missing or mistyped field) into null, which the
// page shows as "live numbers unavailable". Type-only imports from src/api are erased (spec 7b §7.2).
import type { BreakEven, MarketsStatus, SweetLabel } from '../api/types';
import type { LadderIn } from '../lib/ladder';

export const SNAPSHOT_URL = '/math-data/public.json';
/** §6.3: within 30 minutes of written_ms the snapshot is fresh. */
export const FRESH_MS = 30 * 60_000;
/** §6.3: a tab coming back re-fetches only when the last fetch is older than this. */
export const REFETCH_MS = 5 * 60_000;

export interface PublicRung {
  scenario: string;
  latency_ms: number | null;
  extra: boolean;
  n_exec: number;
  n_leg_risk: number;
  n_missed: number;
  gains: string;
  leg_risk: string;
  settle: string;
  unrealized: string;
  net: string;
}

/** 7b-A: the rows the "which markets" grid draws: NO side, fee 0, the six LADDER scenarios. */
export interface PublicSweetRow {
  side: 'no';
  scenario: string;
  fee_bips: 0;
  latency_ms: number | null;
  dimension: 'all' | 'category';
  bucket: string;
  label: SweetLabel;
  n_exec: number;
  clusters: number;
  mean: string | null;
  lo: string | null;
  hi: string | null;
}

export interface PublicLadder {
  epoch: number;
  start_ms: number | null;
  as_of_ms: number | null;
  break_even: BreakEven;
  measured: { latency_ms: number } | null;
  markets_status: MarketsStatus;
  rungs: PublicRung[];
  markets: PublicSweetRow[];
}

export interface PublicToday {
  day_start_ms: number;
  arb: number;
  no: number;
  yes: number;
  edge_budget100: string | null;
  hourly: { hour: number; arb: number }[];
}

export interface PublicSnapshot {
  v: 1;
  written_ms: number;
  ladder: PublicLadder;
  today: PublicToday;
}

type Obj = Record<string, unknown>;
const isObj = (x: unknown): x is Obj => typeof x === 'object' && x !== null && !Array.isArray(x);
const isInt = (x: unknown): x is number => typeof x === 'number' && Number.isFinite(x);
const isIntOrNull = (x: unknown): boolean => x === null || isInt(x);
const isDec = (x: unknown): x is string => typeof x === 'string' && /^-?\d+(\.\d+)?$/.test(x);
const isDecOrNull = (x: unknown): boolean => x === null || isDec(x);
const BREAK_EVEN = new Set(['thin', 'all', 'none', 'at']);
const MARKETS_STATUS = new Set(['live', 'warming', 'under_review', 'missing']);

function isRung(x: unknown): boolean {
  return isObj(x) && typeof x.scenario === 'string' && isIntOrNull(x.latency_ms) && typeof x.extra === 'boolean'
    && isInt(x.n_exec) && isInt(x.n_leg_risk) && isInt(x.n_missed)
    && isDec(x.gains) && isDec(x.leg_risk) && isDec(x.settle) && isDec(x.unrealized) && isDec(x.net);
}

function isRow(x: unknown): boolean {
  return isObj(x) && x.side === 'no' && typeof x.scenario === 'string' && x.fee_bips === 0
    && isIntOrNull(x.latency_ms) && typeof x.dimension === 'string' && typeof x.bucket === 'string' && typeof x.label === 'string'
    && isInt(x.n_exec) && isInt(x.clusters) && isDecOrNull(x.mean) && isDecOrNull(x.lo) && isDecOrNull(x.hi);
}

/** The snapshot, or null for anything the page cannot draw (§6.3 "missing"). */
export function parseSnapshot(raw: unknown): PublicSnapshot | null {
  if (!isObj(raw) || raw.v !== 1 || !isInt(raw.written_ms)) return null;
  const l = raw.ladder;
  const t = raw.today;
  if (!isObj(l) || !isObj(t)) return null;
  const be = l.break_even;
  const m = l.measured;
  const ladderOk = isInt(l.epoch) && isIntOrNull(l.start_ms) && isIntOrNull(l.as_of_ms)
    && isObj(be) && typeof be.kind === 'string' && BREAK_EVEN.has(be.kind) && (be.kind !== 'at' || isInt(be.latency_ms))
    && (m === null || (isObj(m) && isInt(m.latency_ms)))
    && typeof l.markets_status === 'string' && MARKETS_STATUS.has(l.markets_status)
    && Array.isArray(l.rungs) && l.rungs.every(isRung)
    && Array.isArray(l.markets) && l.markets.every(isRow);
  const todayOk = isInt(t.day_start_ms) && isInt(t.arb) && isInt(t.no) && isInt(t.yes) && isDecOrNull(t.edge_budget100)
    && Array.isArray(t.hourly) && t.hourly.every((h: unknown) => isObj(h) && isInt(h.hour) && isInt(h.arb));
  return ladderOk && todayOk ? (raw as unknown as PublicSnapshot) : null;
}

/** §6.3: fresh within 30 minutes of written_ms; a stamp in the future (clock skew) is fresh, never "behind". */
export function freshness(s: Pick<PublicSnapshot, 'written_ms'>, nowMs: number): 'fresh' | 'stale' {
  return nowMs - s.written_ms <= FRESH_MS ? 'fresh' : 'stale';
}

/** §7.2: the snapshot's ladder as ladderModel's input. Only `all` and `category` rows ship (§5.1); a row of
 *  another dimension is dropped here rather than trusted. The snapshot drops the measured host (it names
 *  the VPS); the Ladder shape gets "", which usText never draws (7b-A's contract). */
export function publicToLadder(l: PublicLadder): LadderIn {
  return {
    epoch: l.epoch,
    start_ms: l.start_ms,
    as_of_ms: l.as_of_ms,
    rungs: l.rungs,
    markets: l.markets.filter((r) => r.dimension === 'all' || r.dimension === 'category'),
    measured: l.measured === null ? null : { latency_ms: l.measured.latency_ms, host: '' },
    markets_status: l.markets_status,
    break_even: l.break_even,
  };
}

/** Today's arbitrage episodes per UTC hour, 24 slots (counts, never money). */
export function hourlyValues(t: Pick<PublicToday, 'hourly'>): number[] {
  const out = Array.from({ length: 24 }, () => 0);
  for (const h of t.hourly) if (Number.isInteger(h.hour) && h.hour >= 0 && h.hour < 24) out[h.hour] = Math.max(0, h.arb);
  return out;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/math/snapshot.test.tsx`
Expected: PASS (5 tests).

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/math/snapshot.ts web/src/math/fixtures.ts web/src/math/snapshot.test.tsx
git commit -m "web: the public snapshot contract: types, parse (anything else is missing), freshness, publicToLadder"
```

---

### Task 4: the WASM package: build script, tsconfig split, contract types, loader, safe call, parity

**Files:**
- Create: `scripts/build-wasm.sh`, `web/tsconfig.math.json`, `web/src/wasm/types.ts`, `web/src/wasm/call.ts`, `web/src/wasm/engine.ts`, `web/src/wasm/testEngine.ts`
- Modify: `.gitignore` (append), `.dockerignore` (append), `web/tsconfig.json` (add `exclude`)
- Test: `web/src/wasm/call.test.ts`, `web/src/wasm/parity.test.ts`

**Interfaces:**
- Consumes: 7b-A's crate (`wasm-bindgen =0.2.129`), its six exports and `crates/arb-wasm/fixtures/<export>.json` ("Contract for 7b-B").
- Produces:

```ts
// types.ts: the engine contract (7b-A "Contract for 7b-B"), plus
export const ENGINE_FNS: readonly ['mirror_json', 'payoff_json', 'classify_json', 'depth_walk_json', 'episode_json', 'basket_json'];
export type EngineFn = (typeof ENGINE_FNS)[number];
export type Engine = Record<EngineFn, (input: string) => string>;
export type EngineError = { error: string };
export function isError(x: unknown): x is EngineError
// call.ts
export const ENGINE_BROKE: string;
export function call<T>(engine: Engine, fn: EngineFn, input: unknown): T | EngineError   // never throws
// engine.ts (the only module importing ./pkg)
export async function loadEngine(): Promise<Engine>
// testEngine.ts (tests only)
export const HAVE_PKG: boolean; export const WASM_REQUIRED: boolean;
export function skipReason(what: string): string;
export const FIXTURES: string;                     // <web>/../crates/arb-wasm/fixtures
export interface FixtureCase { name: string; input: string; output: string }
export function fixtureCases(fn: EngineFn): FixtureCase[];            // crates/arb-wasm/fixtures/<fn>.json
export function fixtureCase(fn: EngineFn, name: string): FixtureCase; // throws when the case is missing
export function loadTestEngine(): Promise<Engine>;
```

- [ ] **Step 1: Check that 7b-A is in the tree and the CLI matches**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && grep -c 'crates/arb-wasm' Cargo.toml && ls crates/arb-wasm/fixtures && awk '/^name = "wasm-bindgen"/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock && wasm-bindgen --version && rustup target list --installed | grep wasm32`
Expected: `1`; `basket_json.json classify_json.json depth_walk_json.json episode_json.json mirror_json.json payoff_json.json`; `0.2.129`; `wasm-bindgen 0.2.129`; `wasm32-unknown-unknown`.
If not: no `crates/arb-wasm` = 7b-A is not merged into this branch: **STOP** and report. A missing or different CLI: **STOP** and ask the owner to run `cargo install --locked --version 0.2.129 wasm-bindgen-cli`.

- [ ] **Step 2: Write the failing tests**

```ts
// web/src/wasm/call.test.ts
import { describe, expect, it } from 'vitest';
import { ENGINE_BROKE, call } from './call';
import { ENGINE_FNS, type Engine, isError } from './types';

const engine = (f: (input: string) => string): Engine => Object.fromEntries(ENGINE_FNS.map((n) => [n, f])) as Engine;

describe('call (review focus 2)', () => {
  it('sends the input as JSON and hands back the parsed model', () => {
    let seen = '';
    const out = call<{ ok: number }>(engine((i) => { seen = i; return '{"ok":1}'; }), 'mirror_json', { bids: [], asks: [] });
    expect(seen).toBe('{"bids":[],"asks":[]}');
    expect(out).toEqual({ ok: 1 });
  });

  it("keeps the engine's own sentence", () => {
    const out = call(engine(() => '{"error":"two levels at the same price 0.50"}'), 'mirror_json', {});
    expect(isError(out)).toBe(true);
    expect(isError(out) ? out.error : '').toBe('two levels at the same price 0.50');
  });

  it('turns a trap, non-JSON and an array into one sentence', () => {
    const trap = (): string => {
      throw new Error('unreachable');
    };
    for (const f of [trap, () => 'not json', () => '[1]', () => 'null']) {
      expect(call(engine(f), 'basket_json', {})).toEqual({ error: ENGINE_BROKE });
    }
  });
});
```

```ts
// web/src/wasm/parity.test.ts
// 09-27 §4.3, 7b §3.4: the WASM build returns exactly the strings the native fixtures hold
// (crates/arb-wasm/fixtures/<export>.json, 7b-A). Read with node fs; vite's fs.allow stays as it is (7b §11).
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync, readdirSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { FIXTURES, HAVE_PKG, WASM_REQUIRED, fixtureCases, loadTestEngine, skipReason } from './testEngine';
import { ENGINE_FNS, type Engine } from './types';

const FILES: string[] = existsSync(FIXTURES) ? readdirSync(FIXTURES).filter((f: string) => f.endsWith('.json')).sort() : [];

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('parity'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('native = WASM (parity)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it('has exactly one fixture file per engine export (review focus 4)', () => {
    expect(FILES).toEqual(ENGINE_FNS.map((f) => `${f}.json`).sort());
  });

  for (const fn of ENGINE_FNS) {
    it(`${fn}.json: every case, byte for byte`, () => {
      const cases = fixtureCases(fn);
      expect(cases.length).toBeGreaterThan(0);
      for (const c of cases) expect(engine[fn](c.input), `${fn} ${c.name}`).toBe(c.output);
    });
  }
});
```

- [ ] **Step 3: Run to verify failure**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/wasm`
Expected: FAIL: `Failed to resolve import "./call"` / `"./testEngine"`.

- [ ] **Step 4: Implement the build script and ignore rules**

`scripts/build-wasm.sh`:

```sh
#!/bin/sh
# Phase 7b-B (09-27 §4.2): build crates/arb-wasm for the browser into web/src/wasm/pkg (gitignored).
# PROFILE=release (Docker, CI and `npm run build`; the default) or PROFILE=dev (the PC's test loop).
# No wasm-pack, no wasm-opt (09-27 §4.2); the release build is size-optimized through cargo's
# per-invocation profile variables, which touch only this wasm32 build (its own target directory).
set -eu
cd "$(dirname "$0")/.."
WANT=$(awk '/^name = "wasm-bindgen"/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock)
if [ -z "$WANT" ]; then
  echo "build-wasm: no wasm-bindgen in Cargo.lock (is 7b-A merged?)" >&2
  exit 1
fi
HAVE=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }')
if [ "$HAVE" != "$WANT" ]; then
  echo "build-wasm: the wasm-bindgen CLI is '${HAVE:-missing}', Cargo.lock pins $WANT: cargo install wasm-bindgen-cli --locked --version $WANT" >&2
  exit 1
fi
PROFILE="${PROFILE:-release}"
if [ "$PROFILE" = release ]; then
  export CARGO_PROFILE_RELEASE_OPT_LEVEL="${CARGO_PROFILE_RELEASE_OPT_LEVEL:-s}"
  export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-true}"
  export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
  cargo build -p arb-wasm --target wasm32-unknown-unknown --release
  dir=release
else
  cargo build -p arb-wasm --target wasm32-unknown-unknown
  dir=debug
fi
rm -rf web/src/wasm/pkg
wasm-bindgen --target web --out-dir web/src/wasm/pkg --out-name arb_wasm "target/wasm32-unknown-unknown/$dir/arb_wasm.wasm"
ls web/src/wasm/pkg
```

Append to `.gitignore`, after the `web/dist/` line: `web/src/wasm/pkg/`. Append to `.dockerignore`: `web/src/wasm/pkg` (the node stage takes the package from the wasm stage, never a PC's debug build).

Run: `export PATH="$HOME/.cargo/bin:$PATH" && PROFILE=dev sh scripts/build-wasm.sh && git status --short web/src/wasm`
Expected: the cargo build, then `arb_wasm.d.ts arb_wasm.js arb_wasm_bg.wasm arb_wasm_bg.wasm.d.ts`; `git status` prints nothing for `web/src/wasm` (ignored).

- [ ] **Step 5: Implement the web side**

```ts
// web/src/wasm/types.ts
// The JSON contract with crates/arb-wasm, copied from 7b-A's "Contract for 7b-B" (field order = serialized
// order). Dec is a JSON string holding a normalized decimal. The parity test compares whole strings.

export type Dec = string;
export interface Level { p: Dec; s: Dec }
export interface Book { bids: Level[]; asks: Level[] }
export interface Fee { rate: Dec; exponent: number }
export type LegStatus = 'live' | 'placeholder' | 'other' | 'closed_no' | 'closed_yes' | 'halted';
export interface LegIn { status: LegStatus; bids: Level[]; asks: Level[]; fee: Fee | null }

export interface MirrorIn { bids: Level[]; asks: Level[] }
export interface MirrorModel {
  no_asks: Level[];
  no_bids: Level[];
  best_bid: Level | null;
  best_ask: Level | null;
  spread: Dec | null;
  d1_long: Dec | null;
  d1_short: Dec | null;
  crossed: boolean;
}

export type PayoffKind = 'yes' | 'no' | 'implication';
/** prices: length s, or 2 for the implication (whose s is ignored). */
export interface PayoffIn { kind: PayoffKind; s: number; other_listed: boolean; prices: { bid: Dec; ask: Dec }[] | null }
export interface PayoffMatrix {
  rows: string[];
  cols: string[];
  possible: boolean[];
  cells: (Dec | null)[][];
  basket: (Dec | null)[];
  guaranteed: Dec;
  broken: boolean[];
  top_edge_gross: Dec | null;
}

export const CLASSIFY_NODES = [
  'decided', 'no_basket_tried', 'no_basket_few_legs', 'no_basket_crossed', 'no_basket_not_positive', 'no_basket_positive',
  'yes_basket_tried', 'yes_basket_no_live', 'yes_basket_crossed', 'yes_basket_missing_ask', 'yes_basket_not_positive',
  'yes_basket_positive', 'complete', 'incomplete',
] as const;
export type ClassifyNode = (typeof CLASSIFY_NODES)[number];
export type SignalKind = 'yes_basket' | 'yes_basket_incomplete' | 'no_basket' | 'implication';
export type Realization = 'settlement' | 'converter' | 'not_arbitrage';
export interface ClassifyIn { augmented: boolean; legs: LegIn[] }
/** path is typed string[]: a node this page does not know is still drawn (review focus 4). */
export interface ClassifyModel {
  path: string[];
  kind: SignalKind | null;
  realization: Realization | null;
  net: Dec | null;
  gross: Dec | null;
  q_star: Dec | null;
}

export interface DepthWalkIn { rule: 'yes' | 'no'; legs: { levels: Level[]; fee: Fee | null }[] }
/** payoff per step: 1 for the YES rule; for the nested NO rule, active legs − 1. */
export interface WalkStep { q_from: Dec; q_to: Dec; cost: Dec; payoff: Dec; profitable: boolean }
export interface WalkModel { steps: WalkStep[]; q_star: Dec; pi: Dec; pi_gross: Dec }

export interface ScenarioIn { proc_ms: number; net_ms: number; match_ms: number; honor_seconds_delay: boolean }
export interface EpisodeIn {
  kind: 'yes' | 'no';
  gap_ms: number;
  updates: { t_ms: number; legs: LegIn[] }[];
  scenario: ScenarioIn;
  /** 0–10 s, or null. */
  seconds_delay: number | null;
}
export interface EpisodeModel {
  updates: { t_ms: number; net: Dec | null }[];
  open_ms: number | null;
  close_ms: number | null;
  peak_ms: number | null;
  peak_net: Dec | null;
  t_x: number | null;
  captured: Dec | null;
  caught: boolean;
}

/** Every book is a YES book; for kind "no" the buy walks the mirrored bids, the unwind the mirrored asks. */
export interface BasketLegIn { open: Book; t_x: Book; unwind: Book; fee: Fee | null }
/** min_order ("5") and convert_fee_bips ("0") are left to their defaults by the page. */
export interface BasketIn { kind: 'yes' | 'no'; legs: BasketLegIn[]; q_request: Dec }
export interface Fill { requested: Dec; filled: Dec; vwap: Dec | null; fees: Dec; levels_used: number }
export interface BasketModel {
  kind: 'yes' | 'no';
  min_order: Dec;
  q_open: Dec;
  q_target: Dec;
  legs: { limit: Dec; buy: Fill; excess: Dec; unwind: Fill }[];
  q_eff: Dec;
  cash: { spent: Dec; fees: Dec; unwind_back: Dec; convert_back: Dec; net_cash: Dec; held_unsold: { qty: Dec; basis: Dec }[] };
  value_at_resolution: Dec | null;
}

export const ENGINE_FNS = ['mirror_json', 'payoff_json', 'classify_json', 'depth_walk_json', 'episode_json', 'basket_json'] as const;
export type EngineFn = (typeof ENGINE_FNS)[number];
/** The six exports of the generated package (C3). */
export type Engine = Record<EngineFn, (input: string) => string>;
export type EngineError = { error: string };

export function isError(x: unknown): x is EngineError {
  return typeof x === 'object' && x !== null && typeof (x as { error?: unknown }).error === 'string';
}
```

```ts
// web/src/wasm/call.ts
import type { Engine, EngineError, EngineFn } from './types';

/** A trap (a Rust panic aborts the instance) or an unreadable answer: one sentence, never a blank page. */
export const ENGINE_BROKE = 'The math engine stopped on this input. Reload the page to start it again.';

/** JSON in, JSON out (09-27 §4.1). Never throws: the engine's own {"error"} passes through; anything else
 *  that is not a JSON object becomes ENGINE_BROKE. */
export function call<T>(engine: Engine, fn: EngineFn, input: unknown): T | EngineError {
  let out: string;
  try {
    out = engine[fn](JSON.stringify(input));
  } catch {
    return { error: ENGINE_BROKE };
  }
  try {
    const v: unknown = JSON.parse(out);
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return { error: ENGINE_BROKE };
    return v as T | EngineError;
  } catch {
    return { error: ENGINE_BROKE };
  }
}
```

```ts
// web/src/wasm/engine.ts
// The only module that imports the generated package (09-27 §2); src/math/main.tsx calls loadEngine once.
import init, { basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json } from './pkg/arb_wasm.js';
import wasmUrl from './pkg/arb_wasm_bg.wasm?url';
import type { Engine } from './types';

/** Streams and instantiates the .wasm (CSP: 'wasm-unsafe-eval', connect-src 'self'); rejects if it fails. */
export async function loadEngine(): Promise<Engine> {
  await init({ module_or_path: wasmUrl });
  return { mirror_json, payoff_json, classify_json, depth_walk_json, episode_json, basket_json };
}
```

```ts
// web/src/wasm/testEngine.ts
// Tests only: the generated package through import.meta.glob (an empty map when pkg is missing, so
// `npm test` works without Rust, 09-27 §4.2) and the .wasm bytes from disk through initSync.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { ENGINE_FNS, type Engine, type EngineFn } from './types';

declare const process: { cwd(): string; env: Record<string, string | undefined> };

interface Pkg extends Engine {
  initSync(o: { module: unknown }): unknown;
}
const KEY = './pkg/arb_wasm.js';
const PKG = import.meta.glob<Pkg>('./pkg/arb_wasm.js');

export const HAVE_PKG = KEY in PKG;
/** CI sets WASM_REQUIRED=1: a missing package is then a failure, not a skip (like STORE_TESTS_REQUIRED). */
export const WASM_REQUIRED = process.env.WASM_REQUIRED === '1';

export function skipReason(what: string): string {
  return `${what}: web/src/wasm/pkg is missing, skipped (run scripts/build-wasm.sh; WASM_REQUIRED=1 makes this a failure)`;
}

/** 7b-A's golden fixtures: crates/arb-wasm/fixtures/<export>.json, [{name, input, output}] as exact strings. */
export const FIXTURES = `${process.cwd()}/../crates/arb-wasm/fixtures`;
export interface FixtureCase {
  name: string;
  input: string;
  output: string;
}

export function fixtureCases(fn: EngineFn): FixtureCase[] {
  return JSON.parse(readFileSync(`${FIXTURES}/${fn}.json`, 'utf8')) as FixtureCase[];
}

/** One named case (sections start from these: fed-book, no-edge-3, no-all-fill, …); throws when missing. */
export function fixtureCase(fn: EngineFn, name: string): FixtureCase {
  const c = fixtureCases(fn).find((x) => x.name === name);
  if (c === undefined) throw new Error(`no fixture case ${fn}/${name}`);
  return c;
}

let cached: Promise<Engine> | null = null;

export function loadTestEngine(): Promise<Engine> {
  const load = PKG[KEY];
  if (load === undefined) return Promise.reject(new Error('web/src/wasm/pkg is missing: run scripts/build-wasm.sh'));
  cached ??= load().then((m) => {
    m.initSync({ module: readFileSync(`${process.cwd()}/src/wasm/pkg/arb_wasm_bg.wasm`) });
    return Object.fromEntries(ENGINE_FNS.map((f) => [f, m[f]])) as Engine;
  });
  return cached;
}
```

`web/tsconfig.json`: after the `"include"` line add `"exclude": ["src/math", "src/wasm"]` (the dashboard typecheck never needs Rust; a file it imports from there, like `src/wasm/types.ts`, is still checked through the import). Put a comma after the `compilerOptions` block's closing brace as JSON requires.

`web/tsconfig.math.json`:

```json
{
  "extends": "./tsconfig.json",
  "include": ["src/math", "src/wasm", "vite.math.config.ts"],
  "exclude": []
}
```

- [ ] **Step 6: Run to verify pass**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/wasm`
Expected: PASS: `call.test.ts` 3 tests; `parity.test.ts` `has exactly one fixture file per engine export` plus one test per export (7 tests).
If a fixture case fails: the native and WASM outputs differ; report the case name and both strings (a 7b-A issue; do not edit fixtures here).

- [ ] **Step 7: The dashboard stays Rust-free**

Run: `mv web/src/wasm/pkg web/src/wasm/pkg-away && (cd web && npx vitest run src/wasm 2>&1 | grep -E 'skipped|Test Files'; npx tsc --noEmit && echo dashboard-tsc-ok); mv web/src/wasm/pkg-away web/src/wasm/pkg && cd web && npx tsc --noEmit -p tsconfig.math.json && echo math-tsc-ok`
Expected: the `parity: web/src/wasm/pkg is missing, skipped …` line, `Test Files  1 passed | 1 skipped (2)`, `dashboard-tsc-ok`, then `math-tsc-ok`.

- [ ] **Step 8: Full suite**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 9: Commit**

```bash
git add scripts/build-wasm.sh .gitignore .dockerignore web/tsconfig.json web/tsconfig.math.json web/src/wasm/types.ts web/src/wasm/call.ts web/src/wasm/engine.ts web/src/wasm/testEngine.ts web/src/wasm/call.test.ts web/src/wasm/parity.test.ts
git commit -m "wasm: build script into web/src/wasm/pkg, tsconfig split, engine contract types, safe call, native = WASM parity from the fixture files"
```

---

### Task 5: CI's web job builds the engine and requires it

**Files:**
- Modify: `.github/workflows/ci.yml` (the `web` job; 7b-A's `wasm` job, above it, stays as it is)
- Test: `web/src/deploy.test.ts` (new; Task 14 extends it)

**Interfaces:**
- Consumes: `scripts/build-wasm.sh` (Task 4); `npm run build` (the full build from Task 13 on; the dashboard build until then).
- Produces: `web/src/deploy.test.ts` with helpers `repo(path: string): string` (a repo file, CRLF → LF) and `job(yml: string, name: string): string`.

- [ ] **Step 1: Write the failing test**

```ts
// web/src/deploy.test.ts
// Spec 7b §7, §5.3, §9 (09-27 §4.2–§5.3): the checked-in build and deploy files carry what the public page
// needs. Plain text checks, read with node fs (as theme.test.ts reads theme.css); CRLF normalized.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const repo = (p: string): string => (readFileSync(`${process.cwd()}/../${p}`, 'utf8') as string).replace(/\r\n/g, '\n');

/** One top-level job of a workflow: from `  <name>:` up to the next `  <job>:` line. */
function job(yml: string, name: string): string {
  const start = yml.indexOf(`\n  ${name}:\n`);
  expect(start, `no job ${name}`).toBeGreaterThanOrEqual(0);
  const rest = yml.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[a-z][\w-]*:\n/);
  return next < 0 ? rest : rest.slice(0, next + 2);
}

describe('ci (09-27 §4.3)', () => {
  it('the web job builds the engine, runs the WASM side of parity and the full build, with WASM_REQUIRED', () => {
    const web = job(repo('.github/workflows/ci.yml'), 'web');
    expect(web).toContain('WASM_REQUIRED: "1"');
    expect(web).toContain('rustup target add wasm32-unknown-unknown');
    expect(web).toContain('cargo install wasm-bindgen-cli --locked --version');
    expect(web).not.toContain('cargo test -p arb-wasm'); // 7b-A's wasm job runs the native side
    expect(repo('.github/workflows/ci.yml')).toContain('\n  wasm:\n');
    const steps = ['sh scripts/build-wasm.sh', 'npm ci', 'npm test', 'npm run build'].map((s) => web.indexOf(s));
    expect(steps.every((i) => i >= 0)).toBe(true);
    expect([...steps].sort((a, b) => a - b)).toEqual(steps);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/deploy.test.ts`
Expected: FAIL: `expected '…' to contain 'WASM_REQUIRED: "1"'`.

- [ ] **Step 3: Implement** — replace the whole `web:` job in `.github/workflows/ci.yml` with:

```yaml
  web:
    runs-on: ubuntu-latest
    env:
      # 09-27 §4.2: parity and the math page tests fail instead of skipping when the package is missing.
      WASM_REQUIRED: "1"
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - run: rustup target add wasm32-unknown-unknown
      - id: wbg
        # the CLI must equal the wasm-bindgen crate Cargo.lock pins (7b-A)
        run: echo "version=$(awk '/^name = "wasm-bindgen"/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock)" >> "$GITHUB_OUTPUT"
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/wasm-bindgen
          key: wasm-bindgen-cli-${{ steps.wbg.outputs.version }}
      - run: command -v wasm-bindgen || cargo install wasm-bindgen-cli --locked --version "${{ steps.wbg.outputs.version }}"
      # the native side of parity is 7b-A's wasm job (cargo test -p arb-wasm)
      - run: sh scripts/build-wasm.sh
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: web/package-lock.json
      - run: npm ci
        working-directory: web
      - run: npm test
        working-directory: web
      - run: npm run build
        working-directory: web
```

(The job's old `defaults.run.working-directory: web` is gone; the node steps name it.)

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/deploy.test.ts`
Expected: PASS.

- [ ] **Step 5: Full suite, types**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml web/src/deploy.test.ts
git commit -m "ci: web job builds arb-wasm with the Cargo.lock wasm-bindgen and runs the WASM side of parity with WASM_REQUIRED, then the full build"
```

---

### Task 6: schematics a and c (`MirrorBooks`, `ClassifyTree`)

**Files:**
- Create: `web/src/schematics/MirrorBooks.tsx`, `web/src/schematics/ClassifyTree.tsx`
- Modify: `web/src/theme.css` (append), `web/src/schematics/publicSafe.test.ts` (`ENTRIES`)
- Test: `web/src/schematics/MirrorBooks.test.tsx`, `web/src/schematics/ClassifyTree.test.tsx`

**Interfaces:**
- Consumes: `Level`, `MirrorModel`, `ClassifyModel` (`wasm/types.ts`, type-only); `<Money>`.
- Produces:

```ts
export const CROSSED = 'Crossed book. Somewhere a feed is lying.';
export function MirrorBooks({ bids, asks, model }: { bids: Level[]; asks: Level[]; model: MirrorModel }): JSX.Element
export const NODES: [ClassifyNode, string][];        // 7b-A's 14 nodes in classify's order, with a label each
export function endText(m: ClassifyModel): string;   // 'NO basket · converted' | 'nothing' | …
export function ClassifyTree({ model }: { model: ClassifyModel }): JSX.Element
```

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/schematics/MirrorBooks.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { MirrorModel } from '../wasm/types';
import { CROSSED, MirrorBooks } from './MirrorBooks';

const BIDS = [{ p: '0.54', s: '5603.74' }, { p: '0.53', s: '85887.16' }];
const ASKS = [{ p: '0.55', s: '32356.03' }];
const MODEL: MirrorModel = {
  no_asks: [{ p: '0.46', s: '5603.74' }, { p: '0.47', s: '85887.16' }], no_bids: [{ p: '0.45', s: '32356.03' }],
  best_bid: { p: '0.54', s: '5603.74' }, best_ask: { p: '0.55', s: '32356.03' }, spread: '0.01',
  d1_long: '1.01', d1_short: '0.99', crossed: false,
};

describe('MirrorBooks (parent §5.7 a)', () => {
  it('draws both books on one price axis and joins each YES level to its NO mirror', () => {
    const { container } = render(<MirrorBooks bids={BIDS} asks={ASKS} model={MODEL} />);
    const svg = container.querySelector('svg.mb-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.mb-bid')).toHaveLength(3);
    expect(container.querySelectorAll('rect.mb-ask')).toHaveLength(3);
    const links = container.querySelectorAll('line.mb-link');
    expect(links).toHaveLength(3);
    expect(Number(links[0].getAttribute('x1'))).toBeCloseTo(0.54 * 300);
    expect(Number(links[0].getAttribute('x2'))).toBeCloseTo(0.46 * 300);
    expect(screen.getByText('D1 long 1.01')).toBeInTheDocument();
    expect(screen.getByText('short 0.99')).toBeInTheDocument();
    expect(screen.getByText('NO ask 0.46 (1 − 0.54)')).toBeInTheDocument();
    expect(screen.getByText('NO bid 0.45 (1 − 0.55)')).toBeInTheDocument();
    expect(screen.getByText('spread 0.01')).toBeInTheDocument();
    expect(screen.queryByText(CROSSED)).toBeNull();
  });

  it('names a crossed book in the alert colour', () => {
    render(<MirrorBooks bids={BIDS} asks={ASKS} model={{ ...MODEL, crossed: true, spread: '-0.01', d1_long: '0.99', d1_short: '1.01' }} />);
    expect(screen.getByText(CROSSED)).toHaveClass('mb-alert');
  });

  it('an empty book draws no bars and no NaN (review focus 3)', () => {
    const { container } = render(<MirrorBooks bids={[]} asks={[]} model={{ no_asks: [], no_bids: [], best_bid: null, best_ask: null, spread: null, d1_long: null, d1_short: null, crossed: false }} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(container.querySelectorAll('rect')).toHaveLength(0);
    expect(screen.getByText('D1 long —')).toBeInTheDocument();
  });
});
```

```tsx
// web/src/schematics/ClassifyTree.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ClassifyModel } from '../wasm/types';
import { ClassifyTree, NODES } from './ClassifyTree';

const lit = (c: HTMLElement) => [...c.querySelectorAll('li.cl-node.lit')].map((li) => li.getAttribute('data-node'));

describe('ClassifyTree (parent §5.7 c)', () => {
  it('lights the path classify took and ends in the kind', () => {
    // 7b-A fixture classify_json/no-edge-3's output
    const m: ClassifyModel = { path: ['no_basket_tried', 'no_basket_positive'], kind: 'no_basket', realization: 'converter', net: '0.5', gross: '0.5', q_star: '10' };
    const { container } = render(<ClassifyTree model={m} />);
    expect(container.querySelectorAll('li.cl-node')).toHaveLength(14);
    expect(lit(container)).toEqual(['no_basket_tried', 'no_basket_positive']);
    expect(screen.getByText('NO basket · converted')).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net')).toHaveTextContent('net Π +$0.50 on the table · gross +$0.50 · Q* 10');
  });

  it('ends in nothing for a decided event', () => {
    const { container } = render(<ClassifyTree model={{ path: ['decided'], kind: null, realization: null, net: null, gross: null, q_star: null }} />);
    expect(lit(container)).toEqual(['decided']);
    expect(screen.getByText('nothing')).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net')).toBeNull();
  });

  it('shows a node the page does not know instead of dropping it (review focus 4)', () => {
    const { container } = render(<ClassifyTree model={{ path: ['no_basket_tried', 'brand_new_node'], kind: null, realization: null, net: null, gross: null, q_star: null }} />);
    expect(lit(container)).toEqual(['no_basket_tried', 'brand_new_node']);
    expect(container.querySelectorAll('li.cl-node')).toHaveLength(15);
    expect(screen.getByText('brand_new_node')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/schematics/MirrorBooks.test.tsx src/schematics/ClassifyTree.test.tsx`
Expected: FAIL: `Failed to resolve import "./MirrorBooks"` / `"./ClassifyTree"`.

- [ ] **Step 3: Implement**

```tsx
// web/src/schematics/MirrorBooks.tsx
import type { Level, MirrorModel } from '../wasm/types';

export const CROSSED = 'Crossed book. Somewhere a feed is lying.';

const W = 300;
const H = 64;
const MID = H / 2;
const BAR = 28;

/** A decimal string as geometry only; malformed or not positive draws as 0. */
const geo = (v: string): number => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : 0;
};

/**
 * Parent §5.7 (a): one book, twice. The top track is the YES book, the bottom one the NO book arb-core
 * mirrored from it; x is the price from 0 to 1, a bar's height the square root of its size; a thin line
 * joins each YES price p to its NO price 1 − p. Every number is HTML beside the SVG (preserveAspectRatio="none").
 */
export function MirrorBooks({ bids, asks, model }: { bids: Level[]; asks: Level[]; model: MirrorModel }) {
  const max = Math.max(1, ...[...bids, ...asks].map((l) => geo(l.s)));
  const h = (l: Level) => Math.sqrt(geo(l.s) / max) * BAR;
  const cx = (p: string) => Math.min(W - 1, Math.max(1, geo(p) * W));
  const up = (l: Level, cls: string, key: string) => <rect key={key} className={cls} x={cx(l.p) - 1} y={MID - 2 - h(l)} width={2} height={h(l)} />;
  const down = (l: Level, cls: string, key: string) => <rect key={key} className={cls} x={cx(l.p) - 1} y={MID + 2} width={2} height={h(l)} />;
  return (
    <figure className="mirror" aria-label="one book, shown twice">
      <p className="mb-big">
        <span>{`D1 long ${model.d1_long ?? '—'}`}</span>
        <span>{`short ${model.d1_short ?? '—'}`}</span>
      </p>
      {model.crossed && <p className="mb-alert">{CROSSED}</p>}
      <div className="mb-row">
        <ul className="plain mb-lab" aria-hidden="true">
          <li>YES</li>
          <li>NO</li>
        </ul>
        <svg className="mb-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="the YES book above, its NO mirror below">
          <line className="mb-mid" x1={W / 2} x2={W / 2} y1={0} y2={H} />
          {bids.map((l, i) => up(l, 'mb-bid', `yb${i}`))}
          {asks.map((l, i) => up(l, 'mb-ask', `ya${i}`))}
          {model.no_bids.map((l, i) => down(l, 'mb-bid', `nb${i}`))}
          {model.no_asks.map((l, i) => down(l, 'mb-ask', `na${i}`))}
          {[...bids, ...asks].map((l, i) => (
            <line key={`k${i}`} className="mb-link" x1={cx(l.p)} y1={MID - 2} x2={W - cx(l.p)} y2={MID + 2} />
          ))}
        </svg>
      </div>
      <ol className="mb-axis" aria-hidden="true">
        <li>0</li>
        <li>0.50</li>
        <li>1</li>
      </ol>
      <ul className="plain mb-refl" aria-label="the mirror at the top of the book">
        {bids[0] && model.no_asks[0] && <li>{`NO ask ${model.no_asks[0].p} (1 − ${bids[0].p})`}</li>}
        {asks[0] && model.no_bids[0] && <li>{`NO bid ${model.no_bids[0].p} (1 − ${asks[0].p})`}</li>}
        {model.spread !== null && <li>{`spread ${model.spread}`}</li>}
      </ul>
      <p className="dim mb-note">bars: size at each price, teal bids and salmon asks; a line joins YES price p to NO price 1 − p</p>
    </figure>
  );
}
```

```tsx
// web/src/schematics/ClassifyTree.tsx
import { Money } from '../components/Money';
import type { ClassifyModel, ClassifyNode } from '../wasm/types';

/** 7b-A's classify path nodes, in the order classify runs them, with a label each. */
export const NODES: [ClassifyNode, string][] = [
  ['decided', 'is the event decided?'],
  ['no_basket_tried', 'NO basket over the legs with a YES bid'],
  ['no_basket_few_legs', 'fewer than two legs have a YES bid'],
  ['no_basket_crossed', 'a crossed book: our state is wrong'],
  ['no_basket_not_positive', 'Σ YES bids − 1 is not positive'],
  ['no_basket_positive', 'the NO basket pays'],
  ['yes_basket_tried', 'YES basket over every live leg'],
  ['yes_basket_no_live', 'no live leg'],
  ['yes_basket_crossed', 'a crossed book: our state is wrong'],
  ['yes_basket_missing_ask', 'a live leg has no ask'],
  ['yes_basket_not_positive', '1 − Σ asks is not positive'],
  ['yes_basket_positive', 'the YES basket pays'],
  ['complete', 'every outcome is in the set'],
  ['incomplete', 'an outcome is not in the set'],
];

const KIND: Record<string, string> = {
  yes_basket: 'YES basket', yes_basket_incomplete: 'YES basket, incomplete set', no_basket: 'NO basket', implication: 'implication',
};
const REALIZATION: Record<string, string> = { settlement: 'held to settlement', converter: 'converted', not_arbitrage: 'not arbitrage' };

/** Where the path ends: the kind and how it is realized, or "nothing". */
export function endText(m: ClassifyModel): string {
  if (m.kind === null) return 'nothing';
  const k = KIND[m.kind] ?? m.kind;
  return m.realization === null ? k : `${k} · ${REALIZATION[m.realization] ?? m.realization}`;
}

/**
 * Parent §5.7 (c): every test classify can run, top to bottom, the ones it ran lit. HTML only (no SVG to
 * stretch). A node the page does not know is shown under its own name, never dropped (contract drift shows).
 */
export function ClassifyTree({ model }: { model: ClassifyModel }) {
  const known = new Set<string>(NODES.map(([id]) => id));
  const extra = model.path.filter((n) => !known.has(n)).map((n): [string, string] => [n, n]);
  return (
    <figure className="ctree" aria-label="classify one update">
      <ol className="cl-path" aria-label="the tests classify runs">
        {[...(NODES as [string, string][]), ...extra].map(([id, text]) => {
          const on = model.path.includes(id);
          return (
            <li key={id} className={on ? 'cl-node lit' : 'cl-node'} data-node={id} aria-current={on ? 'step' : undefined}>
              <span className="cl-dot" aria-hidden="true" />
              <span>{text}</span>
            </li>
          );
        })}
      </ol>
      <p className="cl-end">{endText(model)}</p>
      {model.net !== null && (
        <p className="cl-net">
          net Π <Money value={model.net} signed cents /> on the table · gross <Money value={model.gross} signed cents />
          {model.q_star !== null && ` · Q* ${model.q_star}`}
        </p>
      )}
    </figure>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7b §6: the public schematics a and c. Every SVG part has its own mb-* class; labels are HTML; the drawings
   carry no border of their own (they sit in a section's box: no box inside a box). */
.mirror, .ctree { display: grid; gap: 4px; margin: 0; min-width: 0; }
.mb-big { font-family: var(--big); font-size: 32px; line-height: 1; display: flex; flex-wrap: wrap; gap: 4px 16px; }
.mb-alert { color: var(--salmon); font-weight: 600; }
.mb-row { display: grid; grid-template-columns: 4ch minmax(0, 1fr); gap: 6px; }
ul.plain.mb-lab { display: grid; grid-template-rows: 1fr 1fr; gap: 0; align-items: center; font-family: var(--px); font-size: 7px; }
.mb-svg { display: block; width: 100%; height: 96px; background: var(--bg); }
.mb-svg .mb-bid { fill: var(--teal); }
.mb-svg .mb-ask { fill: var(--salmon); }
.mb-svg .mb-link { stroke: var(--line2); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.mb-svg .mb-mid { stroke: var(--dim); stroke-width: 1px; stroke-dasharray: 2 3; vector-effect: non-scaling-stroke; }
.mb-axis { list-style: none; display: flex; justify-content: space-between; margin-left: calc(4ch + 6px); font-size: 13px; color: var(--dim); }
ul.plain.mb-refl { gap: 2px; }
.mb-note { font-size: 13px; }
.cl-path { list-style: none; display: grid; font-size: 13px; }
.cl-node { display: flex; gap: 8px; align-items: center; color: var(--dim); padding: 1px 0; }
.cl-node .cl-dot { flex: none; width: 10px; height: 10px; border: 2px solid var(--line2); }
.cl-node.lit { color: var(--fg); font-weight: 600; }
.cl-node.lit .cl-dot { background: var(--accent); border-color: var(--accent); }
.cl-end { font-family: var(--big); font-size: 32px; line-height: 1; }
```

In `web/src/schematics/publicSafe.test.ts`, `ENTRIES` gains `'src/schematics/MirrorBooks.tsx', 'src/schematics/ClassifyTree.tsx'`.

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/schematics src/theme.test.ts`
Expected: PASS.

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/schematics/MirrorBooks.tsx web/src/schematics/ClassifyTree.tsx web/src/schematics/MirrorBooks.test.tsx web/src/schematics/ClassifyTree.test.tsx web/src/schematics/publicSafe.test.ts web/src/theme.css
git commit -m "web: schematics a (one book twice, mirror lines, D1) and c (classify path lit, unknown nodes shown)"
```

---

### Task 7: schematics d and f (`DepthStairs`, `BasketLegs`)

**Files:**
- Create: `web/src/schematics/DepthStairs.tsx`, `web/src/schematics/BasketLegs.tsx`
- Modify: `web/src/theme.css` (append), `web/src/schematics/publicSafe.test.ts` (`ENTRIES`)
- Test: `web/src/schematics/DepthStairs.test.tsx`, `web/src/schematics/BasketLegs.test.tsx`

**Interfaces:**
- Consumes: `WalkModel`, `BasketModel` (`wasm/types.ts`); `formatDecimal`, `formatMoneyCents`, `signOf` (`lib/money.ts`); `<Money>`.
- Produces:

```ts
export function DepthStairs({ model }: { model: WalkModel }): JSX.Element   // payoff read per step
export interface Verdict { big: string; tone: 'pos' | 'neg' | null }
export function basketVerdict(m: BasketModel): Verdict
export function BasketLegs({ model, names }: { model: BasketModel; names: string[] }): JSX.Element   // reads 7b-A's basket_json output
```

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/schematics/DepthStairs.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { WalkModel } from '../wasm/types';
import { DepthStairs } from './DepthStairs';

/** 7b-A fixture depth_walk_json/yes-two-legs's output (its test depth_walk_draws_past_q_star, verbatim). */
const YES_TWO: WalkModel = JSON.parse(
  '{"steps":[{"q_from":"0","q_to":"10","cost":"0.9","payoff":"1","profitable":true},{"q_from":"10","q_to":"15","cost":"1","payoff":"1","profitable":false}],"q_star":"10","pi":"1","pi_gross":"1"}',
) as WalkModel;
/** The nested NO rule: the payoff falls as legs run out (a view model, not an engine figure). */
const NESTED: WalkModel = {
  steps: [
    { q_from: '0', q_to: '10', cost: '1.9', payoff: '2', profitable: true },
    { q_from: '10', q_to: '30', cost: '1.42', payoff: '1', profitable: false },
  ],
  q_star: '10',
  pi: '1',
  pi_gross: '1',
};

describe('DepthStairs (parent §5.7 d)', () => {
  it('shades Π up to Q*, draws the steps past it, marks Q*, and says Π and Q* in HTML', () => {
    const { container } = render(<DepthStairs model={YES_TWO} />);
    const svg = container.querySelector('svg.ds-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.ds-pi')).toHaveLength(1);
    expect(container.querySelectorAll('rect.ds-loss')).toHaveLength(0); // cost 1 = payoff 1: a zero-height step
    expect(container.querySelectorAll('line.ds-cost')).toHaveLength(2);
    expect(container.querySelectorAll('line.ds-pay')).toHaveLength(2);
    expect(Number(container.querySelector('line.ds-qstar')?.getAttribute('x1'))).toBeCloseTo(200); // 10 of 15 shares
    expect(container.querySelector('.ds-big')).toHaveTextContent(/^Π \+\$1\.00/);
    expect(screen.getByText('Q* 10')).toBeInTheDocument();
    expect(container.querySelector('.ds-gross')).toHaveTextContent('before fees +$1.00');
    expect(screen.getByText('15 shares')).toBeInTheDocument();
  });

  it('draws each step against its own payoff (the nested NO rule)', () => {
    const { container } = render(<DepthStairs model={NESTED} />);
    const pays = [...container.querySelectorAll('line.ds-pay')].map((l) => Number(l.getAttribute('y1')));
    expect(pays[0]).toBeLessThan(pays[1]); // payoff 2 sits above payoff 1
    const loss = container.querySelector('rect.ds-loss');
    expect(Number(loss?.getAttribute('y')) + Number(loss?.getAttribute('height'))).toBeCloseTo(pays[1]);
  });

  it('no steps and Q* 0 draw no NaN (review focus 3; fixture zero-depth-leg)', () => {
    const { container } = render(<DepthStairs model={{ steps: [], q_star: '0', pi: '0', pi_gross: '0' }} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(container.querySelectorAll('rect.ds-pi, rect.ds-loss')).toHaveLength(0);
    expect(screen.getByText('Q* 0')).toBeInTheDocument();
  });
});
```

```tsx
// web/src/schematics/BasketLegs.test.tsx
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { BasketModel, Fill } from '../wasm/types';
import { BasketLegs, basketVerdict } from './BasketLegs';

const fill = (requested: string, filled: string, vwap: string | null = null): Fill => ({ requested, filled, vwap, fees: '0', levels_used: filled === '0' ? 0 : 1 });
const none = { qty: '0', basis: '0' };
/** The shape and the checked figures of 7b-A fixture basket_json/no-all-fill (section f's start). */
const ALL: BasketModel = {
  kind: 'no',
  min_order: '5',
  q_open: '100',
  q_target: '50',
  legs: [
    { limit: '0.6', buy: fill('50', '50', '0.6'), excess: '0', unwind: fill('0', '0') },
    { limit: '0.65', buy: fill('50', '50', '0.65'), excess: '0', unwind: fill('0', '0') },
    { limit: '0.7', buy: fill('50', '50', '0.7'), excess: '0', unwind: fill('0', '0') },
  ],
  q_eff: '50',
  cash: { spent: '97.5', fees: '0', unwind_back: '0', convert_back: '100', net_cash: '2.5', held_unsold: [none, none, none] },
  value_at_resolution: null,
};
/** The shape and the checked figures of fixture no-one-leg-empty-burst: leg B empty at t_x, A and C sold
 *  back 5 ticks against us at 0.54 and 0.64. */
const BURST: BasketModel = {
  ...ALL,
  legs: [
    { limit: '0.6', buy: fill('50', '50', '0.6'), excess: '50', unwind: fill('50', '50', '0.54') },
    { limit: '0.65', buy: fill('50', '0'), excess: '0', unwind: fill('0', '0') },
    { limit: '0.7', buy: fill('50', '50', '0.7'), excess: '50', unwind: fill('50', '50', '0.64') },
  ],
  q_eff: '0',
  cash: { spent: '65', fees: '0', unwind_back: '59', convert_back: '0', net_cash: '-6', held_unsold: [none, none, none] },
};
const legs = () => within(screen.getByRole('list', { name: 'legs' })).getAllByRole('listitem');

describe('basketVerdict (spec 7b §6.2 f, 7b-A contract)', () => {
  it('says "caught, and lost $x" when net cash is below zero, "filled, $x on paper" otherwise', () => {
    expect(basketVerdict(ALL)).toEqual({ big: 'filled, +$2.50 on paper', tone: 'pos' });
    expect(basketVerdict(BURST)).toEqual({ big: 'caught, and lost $6.00', tone: 'neg' });
    expect(basketVerdict({ ...BURST, cash: { ...BURST.cash, net_cash: '0' } })).toEqual({ big: 'filled, $0.00 on paper', tone: null });
  });
});

describe('BasketLegs (spec 7b §6.2 f)', () => {
  it('draws planned vs filled per leg, q_eff as one line, and the excess split into sold and held', () => {
    const held: BasketModel = {
      ...BURST,
      legs: [{ ...BURST.legs[0], unwind: fill('50', '45', '0.54') }, BURST.legs[1], BURST.legs[2]],
      cash: { ...BURST.cash, held_unsold: [{ qty: '5', basis: '3' }, none, none] },
    };
    const { container } = render(<BasketLegs model={held} names={['A', 'B', 'C']} />);
    const svg = container.querySelector('svg.bl-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.bl-plan')).toHaveLength(3);
    expect(container.querySelectorAll('line.bl-qeff')).toHaveLength(1);
    expect(Number(container.querySelector('line.bl-qeff')?.getAttribute('y1'))).toBeCloseTo(120); // q_eff 0 sits on the floor
    const a = container.querySelector('g[data-leg="A"]');
    expect(Number(a?.querySelector('rect.bl-sold')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(Number(a?.querySelector('rect.bl-held')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(Number(container.querySelector('g[data-leg="B"] rect.bl-fill')?.getAttribute('height'))).toBe(0);
    expect(legs()[1]).toHaveTextContent(/^leg B0 of 50/);
    expect(legs()[0]).toHaveTextContent('sold 45 at 0.54');
    expect(legs()[0]).toHaveTextContent('held 5, $3.00 cost, not cash');
    expect(screen.getByText('caught, and lost $6.00')).toHaveClass('bl-big');
  });

  it('nothing filled draws no NaN (review focus 3)', () => {
    const zero: BasketModel = {
      ...ALL, q_target: '0', q_eff: '0',
      legs: ALL.legs.map((l) => ({ ...l, buy: fill('0', '0'), excess: '0', unwind: fill('0', '0') })),
      cash: { ...ALL.cash, spent: '0', convert_back: '0', net_cash: '0' },
    };
    const { container } = render(<BasketLegs model={zero} names={['A', 'B', 'C']} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(screen.getByText('filled, $0.00 on paper')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/schematics/DepthStairs.test.tsx src/schematics/BasketLegs.test.tsx`
Expected: FAIL: `Failed to resolve import "./DepthStairs"` / `"./BasketLegs"`.

- [ ] **Step 3: Implement**

```tsx
// web/src/schematics/DepthStairs.tsx
import { Money } from '../components/Money';
import type { WalkModel } from '../wasm/types';

const W = 300;
const H = 120;
const geo = (v: string): number => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : 0;
};

/**
 * Parent §5.7 (d): the cost of each next basket as a staircase against what that basket pays (1 for the
 * YES rule; active legs − 1 for the nested NO rule, so the payoff is read per step). Between cost and payoff
 * each step is shaded: teal where it pays (up to Q*), salmon past Q* where it costs more than it pays
 * (walk_steps keeps those). Π, Π before fees and Q* are HTML; numbers are geometry only in the SVG.
 */
export function DepthStairs({ model }: { model: WalkModel }) {
  const last = model.steps.length > 0 ? model.steps[model.steps.length - 1].q_to : '0';
  const qMax = Math.max(1, geo(last));
  const yMax = Math.max(0, ...model.steps.flatMap((s) => [geo(s.cost), geo(s.payoff)])) * 1.1 || 1;
  const sx = (q: string) => (geo(q) / qMax) * W;
  const sy = (v: number) => H - (v / yMax) * H;
  const qs = sx(model.q_star);
  return (
    <figure className="dstairs" aria-label="the cost of each next basket against what it pays">
      <p className="ds-big">
        <span>
          Π <Money value={model.pi} signed cents />
        </span>
        <span>{`Q* ${model.q_star}`}</span>
      </p>
      <p className="dim">
        on the table, if every level stayed put · <span className="ds-gross">before fees <Money value={model.pi_gross} signed cents /></span>
      </p>
      <svg className="ds-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="the cost staircase">
        {model.steps.map((s, i) => {
          const c = sy(geo(s.cost));
          const p = sy(geo(s.payoff));
          const x = sx(s.q_from);
          const w = Math.max(0, sx(s.q_to) - x);
          if (Math.abs(c - p) === 0) return null;
          return <rect key={i} className={s.profitable ? 'ds-pi' : 'ds-loss'} x={x} y={Math.min(c, p)} width={w} height={Math.abs(c - p)} />;
        })}
        {model.steps.map((s, i) => {
          const y = sy(geo(s.cost));
          return <line key={`c${i}`} className="ds-cost" x1={sx(s.q_from)} x2={sx(s.q_to)} y1={y} y2={y} />;
        })}
        {model.steps.map((s, i) => {
          const y = sy(geo(s.payoff));
          return <line key={`p${i}`} className="ds-pay" x1={sx(s.q_from)} x2={sx(s.q_to)} y1={y} y2={y} />;
        })}
        <line className="ds-qstar" x1={qs} x2={qs} y1={0} y2={H} />
      </svg>
      <ol className="ds-axis" aria-hidden="true">
        <li>0</li>
        <li>{`${last} shares`}</li>
      </ol>
      <ul className="plain ds-legend" aria-label="legend">
        <li><span className="ds-sw pi" aria-hidden="true" />the next basket pays more than it costs</li>
        <li><span className="ds-sw loss" aria-hidden="true" />past Q*: it costs more than it pays</li>
        <li><span className="ds-sw pay" aria-hidden="true" />what a basket pays</li>
      </ul>
    </figure>
  );
}
```

```tsx
// web/src/schematics/BasketLegs.tsx
import { Money } from '../components/Money';
import { formatDecimal, formatMoneyCents, signOf } from '../lib/money';
import type { BasketModel } from '../wasm/types';

const W = 300;
const H = 120;
const geo = (v: string): number => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : 0;
};

export interface Verdict {
  big: string;
  tone: 'pos' | 'neg' | null;
}

/** Spec 7b §6.2 f and 7b-A's contract: the big number is net_cash, "caught, and lost $x" when it is below
 *  zero, "filled, $x on paper" otherwise. Money through money.ts only. */
export function basketVerdict(m: BasketModel): Verdict {
  const net = m.cash.net_cash;
  const s = signOf(net);
  if (s < 0) return { big: `caught, and lost ${formatMoneyCents(net.replace(/^-/, ''))}`, tone: 'neg' };
  return { big: `filled, ${formatMoneyCents(net, { signed: true })} on paper`, tone: s > 0 ? 'pos' : null };
}

/**
 * Spec 7b §6.2 f: per leg, the planned shares (outline) and what filled; the full baskets (q_eff) as one line
 * across the legs; above it each leg's excess, split into the part sold back at the unwind price and the part
 * still held (cost, not cash). Labels and money are HTML under the drawing.
 */
export function BasketLegs({ model, names }: { model: BasketModel; names: string[] }) {
  const n = Math.max(1, model.legs.length);
  const slot = W / n;
  const bw = slot * 0.5;
  const plan = geo(model.q_target);
  const top = Math.max(1, plan, ...model.legs.map((l) => geo(l.buy.filled)));
  const sy = (q: number) => H - (q / top) * (H - 4);
  const qe = geo(model.q_eff);
  const v = basketVerdict(model);
  return (
    <figure className="blegs" aria-label="each leg: planned, filled, and the excess sold back or held">
      <p className={v.tone === null ? 'bl-big' : `bl-big ${v.tone}`}>{v.big}</p>
      <svg className="bl-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="planned and filled shares per leg">
        {model.legs.map((l, i) => {
          const x = i * slot + (slot - bw) / 2;
          const base = Math.min(geo(l.buy.filled), qe);
          const sold = geo(l.unwind.filled);
          const held = geo(model.cash.held_unsold[i]?.qty ?? '0');
          return (
            <g key={i} data-leg={names[i] ?? String(i + 1)}>
              <rect className="bl-plan" x={x} y={sy(plan)} width={bw} height={H - sy(plan)} />
              <rect className="bl-fill" x={x} y={sy(base)} width={bw} height={H - sy(base)} />
              <rect className="bl-sold" x={x} y={sy(base + sold)} width={bw} height={sy(base) - sy(base + sold)} />
              <rect className="bl-held" x={x} y={sy(base + sold + held)} width={bw} height={sy(base + sold) - sy(base + sold + held)} />
            </g>
          );
        })}
        <line className="bl-qeff" x1={0} x2={W} y1={sy(qe)} y2={sy(qe)} />
      </svg>
      <ol className="bl-axis" aria-label="legs">
        {model.legs.map((l, i) => {
          const h = model.cash.held_unsold[i];
          return (
            <li key={i}>
              <b>{`leg ${names[i] ?? i + 1}`}</b>
              <span>{`${formatDecimal(l.buy.filled)} of ${formatDecimal(l.buy.requested)}`}</span>
              {signOf(l.unwind.filled) > 0 && <span>{`sold ${formatDecimal(l.unwind.filled)}${l.unwind.vwap === null ? '' : ` at ${l.unwind.vwap}`}`}</span>}
              {signOf(l.unwind.requested) > 0 && signOf(l.unwind.filled) === 0 && <span>{`${formatDecimal(l.unwind.requested)} to sell, no order went out`}</span>}
              {h !== undefined && signOf(h.qty) > 0 && <span>{`held ${formatDecimal(h.qty)}, ${formatMoneyCents(h.basis)} cost, not cash`}</span>}
            </li>
          );
        })}
      </ol>
      <p className="dim">{`the line: ${formatDecimal(model.q_eff)} full baskets (q_eff) · planned ${formatDecimal(model.q_target)} of the ${formatDecimal(model.q_open)} the book at open could fill`}</p>
      <dl className="bl-cash">
        <div><dt>spent</dt><dd><Money value={model.cash.spent} cents /></dd></div>
        <div><dt>fees</dt><dd><Money value={model.cash.fees} cents /></dd></div>
        <div><dt>sold back</dt><dd><Money value={model.cash.unwind_back} cents /></dd></div>
        <div><dt>converted</dt><dd><Money value={model.cash.convert_back} cents /></dd></div>
        <div><dt>net cash, on paper</dt><dd><Money value={model.cash.net_cash} signed tone cents /></dd></div>
      </dl>
      {model.value_at_resolution !== null && (
        <p className="dim">
          if a listed leg wins: <Money value={model.value_at_resolution} cents /> at resolution, not yet cash
        </p>
      )}
      <ul className="plain bl-legend" aria-label="legend">
        <li><span className="bl-sw fill" aria-hidden="true" />in full baskets</li>
        <li><span className="bl-sw sold" aria-hidden="true" />excess sold back at the unwind price</li>
        <li><span className="bl-sw held" aria-hidden="true" />excess still held</li>
        <li><span className="bl-sw plan" aria-hidden="true" />planned</li>
      </ul>
    </figure>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7b §6: schematics d (ds-*) and f (bl-*). */
.dstairs, .blegs { display: grid; gap: 4px; margin: 0; min-width: 0; }
.ds-big, .bl-big { font-family: var(--big); font-size: 32px; line-height: 1; display: flex; flex-wrap: wrap; gap: 4px 16px; }
.bl-big.pos { color: var(--good); }
.bl-big.neg { color: var(--bad); }
.ds-svg, .bl-svg { display: block; width: 100%; height: 140px; background: var(--bg); }
.ds-svg .ds-pi { fill: var(--teal); opacity: 0.7; }
.ds-svg .ds-loss { fill: var(--salmon); opacity: 0.7; }
.ds-svg .ds-cost { stroke: var(--fg); stroke-width: 2px; vector-effect: non-scaling-stroke; }
.ds-svg .ds-pay { stroke: var(--mint); stroke-width: 1px; stroke-dasharray: 4 3; vector-effect: non-scaling-stroke; }
.ds-svg .ds-qstar { stroke: var(--accent); stroke-width: 2px; vector-effect: non-scaling-stroke; }
.ds-axis { list-style: none; display: flex; justify-content: space-between; font-size: 13px; color: var(--dim); }
ul.plain.ds-legend, ul.plain.bl-legend { gap: 2px; font-size: 13px; }
.ds-sw, .bl-sw { display: inline-block; width: 10px; height: 10px; margin-right: 4px; vertical-align: middle; }
.ds-sw.pi { background: var(--teal); }
.ds-sw.loss { background: var(--salmon); }
.ds-sw.pay { border-top: 2px dashed var(--mint); height: 0; }
.bl-svg .bl-plan { fill: none; stroke: var(--line2); stroke-width: 1px; stroke-dasharray: 4 3; vector-effect: non-scaling-stroke; }
.bl-svg .bl-fill { fill: var(--teal); }
.bl-svg .bl-sold { fill: var(--salmon); }
.bl-svg .bl-held { fill: var(--rose); }
.bl-svg .bl-qeff { stroke: var(--fg); stroke-width: 2px; vector-effect: non-scaling-stroke; }
.bl-axis { list-style: none; display: grid; grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); gap: 6px; text-align: center; font-size: 13px; line-height: 1.3; }
.bl-axis li { display: grid; min-width: 0; }
.bl-cash { display: flex; flex-wrap: wrap; gap: 4px 16px; }
.bl-cash dt { color: var(--dim); font-size: 13px; }
.bl-cash dd { font-family: var(--big); font-size: 22px; }
.bl-sw.fill { background: var(--teal); }
.bl-sw.sold { background: var(--salmon); }
.bl-sw.held { background: var(--rose); }
.bl-sw.plan { border: 1px dashed var(--line2); }
```

In `web/src/schematics/publicSafe.test.ts`, `ENTRIES` gains `'src/schematics/DepthStairs.tsx', 'src/schematics/BasketLegs.tsx'`.

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/schematics src/theme.test.ts`
Expected: PASS.

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/schematics/DepthStairs.tsx web/src/schematics/BasketLegs.tsx web/src/schematics/DepthStairs.test.tsx web/src/schematics/BasketLegs.test.tsx web/src/schematics/publicSafe.test.ts web/src/theme.css
git commit -m "web: schematics d (cost staircase past Q*) and f (planned vs filled per leg, q_eff line, excess sold or held, net cash verdict)"
```

---

### Task 8: b and e reuse `PayoffGrid` and `DurationBars` (implication, order bar), `UpdateBars`, adapters

**Files:**
- Create: `web/src/schematics/UpdateBars.tsx`, `web/src/math/adapt.ts`
- Modify: `web/src/schematics/models.ts` (`PayoffModel`, `DurationRow`), `web/src/schematics/PayoffGrid.tsx` (header, cells, basket), `web/src/schematics/DurationBars.tsx` (bar class), `web/src/theme.css` (append), `web/src/schematics/publicSafe.test.ts` (`ENTRIES`)
- Test: `web/src/math/adapt.test.tsx`

**Interfaces:**
- Consumes: `PayoffMatrix`, `PayoffKind`, `EpisodeModel` (`wasm/types.ts`); `payoffStructure` (`explain/models.ts`, test only); `FIXTURES`, `fixtureCases` (`wasm/testEngine.ts`, test only).
- Produces:

```ts
// schematics/models.ts (widened; the panel's payoffStructure still fits)
export interface PayoffModel { kind: 'yes' | 'no' | 'implication'; s: number; cols: string[]; rows: { name: string; cells: (0 | 1 | null)[] }[]; basket: (number | null)[]; guaranteed: number; broken: boolean[]; possible?: boolean[] }
export interface DurationRow { kind: 'example' | 'row' | 'order'; label: string; ms: number; open: boolean; ticks: number[] }
// schematics/UpdateBars.tsx
export function UpdateBars({ updates, peakMs }: { updates: { t_ms: number; net: string | null }[]; peakMs: number | null }): JSX.Element
// math/adapt.ts
export function matrixToPayoff(m: PayoffMatrix, kind: PayoffKind): PayoffModel
export function episodeRows(m: EpisodeModel): DurationRow[]
```

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/math/adapt.test.tsx
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { payoffStructure } from '../explain/models';
import { DurationBars } from '../schematics/DurationBars';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { UpdateBars } from '../schematics/UpdateBars';
import { FIXTURES, fixtureCases } from '../wasm/testEngine';
import { type EpisodeModel, type PayoffMatrix, isError } from '../wasm/types';
import { episodeRows, matrixToPayoff } from './adapt';

const IMPLICATION: PayoffMatrix = {
  rows: ['NO_A', 'YES_B'],
  cols: ['A∧B', 'A∧¬B', '¬A∧B', '¬A∧¬B'],
  possible: [true, false, true, true],
  cells: [['0', null, '1', '1'], ['1', null, '1', '0']],
  basket: ['1', null, '2', '1'],
  guaranteed: '1',
  broken: [false, false, false, false],
  top_edge_gross: null,
};
/** A view model with section e's shape: open at u2 (400 ms), peak u3, close u5 (1900 ms), t_x 700 ms. */
const EPISODE: EpisodeModel = {
  updates: [{ t_ms: 0, net: null }, { t_ms: 400, net: '1.9' }, { t_ms: 900, net: '5.8' }, { t_ms: 1400, net: '1.9' }, { t_ms: 1900, net: null }],
  open_ms: 400,
  close_ms: 1900,
  peak_ms: 900,
  peak_net: '5.8',
  t_x: 700,
  captured: '1.9',
  caught: true,
};

describe('b reuses PayoffGrid (spec 7b §1 change 3)', () => {
  it('draws the implication: the impossible column and its cells as a dash', () => {
    render(<PayoffGrid model={matrixToPayoff(IMPLICATION, 'implication')} label="payoff of the implication" />);
    expect(screen.getByRole('columnheader', { name: 'A∧¬B (impossible)' })).toBeInTheDocument();
    expect(screen.getAllByText('—')).toHaveLength(3);
    expect(screen.getByText('2')).toHaveClass('pay');
  });

  it('keeps the YES basket ✗ on the uncovered column', () => {
    const m: PayoffMatrix = {
      rows: ['YES_A', 'YES_B'], cols: ['A', 'B', 'Other'], possible: [true, true, true],
      cells: [['1', '0', '0'], ['0', '1', '0']], basket: ['1', '1', '0'], guaranteed: '1', broken: [false, false, true], top_edge_gross: null,
    };
    render(<PayoffGrid model={matrixToPayoff(m, 'yes')} label="payoff of the YES basket" />);
    expect(screen.getByRole('columnheader', { name: 'Other ✗' })).toBeInTheDocument();
  });
});

describe('e reuses DurationBars and adds UpdateBars (spec 7b §6.2 e)', () => {
  it('puts the episode on the table and our order on one time scale, and the edge per update', () => {
    const rows = episodeRows(EPISODE);
    expect(rows).toEqual([
      { kind: 'row', label: 'on the table', ms: 1500, open: false, ticks: [500, 1000, 1500] },
      { kind: 'order', label: 'our order lands', ms: 300, open: false, ticks: [] },
    ]);
    const { container } = render(<><UpdateBars updates={EPISODE.updates} peakMs={EPISODE.peak_ms} /><DurationBars rows={rows} /></>);
    expect(screen.getByText('on the table: 1.50 s')).toBeInTheDocument();
    expect(screen.getByText('our order lands: 0.30 s')).toBeInTheDocument();
    expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
    expect(container.querySelectorAll('rect.ub-bar')).toHaveLength(5);
    expect(container.querySelectorAll('rect.ub-bar.peak')).toHaveLength(1);
    expect(container.querySelector('svg.ub-svg')).toHaveAttribute('preserveAspectRatio', 'none');
    expect(screen.getAllByText('no edge')).toHaveLength(2);
  });

  it('an episode that never opens or never closes draws no NaN (review focus 3)', () => {
    const never: EpisodeModel = { ...EPISODE, updates: EPISODE.updates.map((u) => ({ ...u, net: null })), open_ms: null, close_ms: null, peak_ms: null, peak_net: null, t_x: null, caught: false, captured: null };
    expect(episodeRows(never)).toEqual([]);
    const open: EpisodeModel = { ...EPISODE, close_ms: null, t_x: null };
    expect(episodeRows(open)).toEqual([{ kind: 'row', label: 'on the table', ms: 1500, open: true, ticks: [500, 1000, 1500] }]);
    const { container } = render(<><UpdateBars updates={never.updates} peakMs={null} /><DurationBars rows={episodeRows(open)} /></>);
    expect(container.innerHTML).not.toContain('NaN');
    expect(screen.getByText('on the table: still open, 1.50 s so far')).toBeInTheDocument();
  });
});

const HAVE_PAYOFF: boolean = existsSync(`${FIXTURES}/payoff_json.json`);

describe.skipIf(!HAVE_PAYOFF)('the panel payoff structure against the payoff fixtures (09-27 §7.3, left open by 7a-B)', () => {
  it('agrees on every YES and NO case without prices, S 2–6', () => {
    const cases = fixtureCases('payoff_json');
    let checked = 0;
    for (const c of cases) {
      const i = JSON.parse(c.input) as { kind?: string; s?: number; other_listed?: boolean; prices?: unknown };
      const o: unknown = JSON.parse(c.output);
      if ((i.kind !== 'yes' && i.kind !== 'no') || typeof i.s !== 'number' || i.s < 2 || i.s > 6) continue;
      if ((i.prices !== null && i.prices !== undefined) || isError(o)) continue;
      const engine = matrixToPayoff(o as PayoffMatrix, i.kind);
      const panel = payoffStructure(i.kind, i.s, i.other_listed === false);
      if (i.kind === 'yes') {
        expect(engine.broken, c.name).toEqual(panel.broken);
        expect(engine.basket, c.name).toEqual(panel.basket);
      } else {
        expect(engine.broken.some(Boolean), c.name).toBe(false);
        expect(panel.broken.some(Boolean), c.name).toBe(false);
      }
      checked += 1;
    }
    expect(checked).toBeGreaterThan(0);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/math/adapt.test.tsx`
Expected: FAIL: `Failed to resolve import "./adapt"` (and `"../schematics/UpdateBars"`).

- [ ] **Step 3: Implement**

`web/src/schematics/models.ts`: replace `PayoffModel` and `DurationRow` with

```ts
export interface PayoffModel {
  kind: 'yes' | 'no' | 'implication';
  /** The basket's real number of outcomes (may exceed the rows drawn). */
  s: number;
  cols: string[];
  /** 1 where the row's token pays $1 in that column; null in a column that cannot happen. */
  rows: { name: string; cells: (0 | 1 | null)[] }[];
  /** What the whole basket pays in each column, over all s outcomes; null where the column cannot happen. */
  basket: (number | null)[];
  /** Parent spec §5.7 (b): 1 for a YES basket, s − 1 for a NO basket, 1 for the implication. */
  guaranteed: number;
  broken: boolean[];
  /** 09-27 §3.1: false for a column that cannot happen (the implication's A∧¬B); absent means all can. */
  possible?: boolean[];
}

export interface DurationRow {
  /** 'order' (section e): when our order lands, drawn in the accent colour. */
  kind: 'example' | 'row' | 'order';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open. */
  ticks: number[];
}
```

`web/src/schematics/PayoffGrid.tsx`: inside the component, before `return`, add `const possible = (i: number) => model.possible?.[i] !== false;`, and replace the three mapped cells:

```tsx
              {model.cols.map((c, i) => (
                <th key={c} scope="col" className={model.broken[i] ? 'broken' : possible(i) ? undefined : 'impossible'}>
                  {model.broken[i] ? `${c} ✗` : possible(i) ? c : `${c} (impossible)`}
                </th>
              ))}
```

```tsx
                {r.cells.map((v, i) => (
                  <td key={model.cols[i]} className={v === null ? 'impossible' : v === 1 ? 'pay' : 'nopay'}>
                    {v ?? '—'}
                  </td>
                ))}
```

```tsx
              {model.basket.map((b, i) => (
                <td key={model.cols[i]} className={model.broken[i] ? 'broken' : b === null ? 'impossible' : 'pay'}>
                  {b ?? '—'}
                </td>
              ))}
```

`web/src/schematics/DurationBars.tsx`: the bar's class list becomes

```tsx
              className={['db-bar', r.kind === 'example' ? 'example' : null, r.kind === 'order' ? 'order' : null, r.open ? 'open' : null].filter(Boolean).join(' ')}
```

```tsx
// web/src/schematics/UpdateBars.tsx
import { Money } from '../components/Money';

const W = 300;
const H = 48;
const geo = (v: string): number => {
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : 0;
};

/** Parent §5.7 (e): the net edge classify found at each update (none: no bar), the peak marked. The update
 *  names, times and amounts are HTML under the drawing. */
export function UpdateBars({ updates, peakMs }: { updates: { t_ms: number; net: string | null }[]; peakMs: number | null }) {
  const vals = updates.map((u) => geo(u.net ?? '0'));
  const max = Math.max(0, ...vals) || 1;
  const slot = W / Math.max(1, updates.length);
  const bw = slot * 0.6;
  return (
    <figure className="ubars" aria-label="the edge at each update">
      <svg className="ub-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="net edge per update">
        {updates.map((u, i) => {
          const h = (vals[i] / max) * (H - 2);
          return <rect key={u.t_ms} className={u.t_ms === peakMs ? 'ub-bar peak' : 'ub-bar'} x={i * slot + (slot - bw) / 2} y={H - h} width={bw} height={h} />;
        })}
        <line className="ub-zero" x1={0} x2={W} y1={H - 0.5} y2={H - 0.5} />
      </svg>
      <ol className="ub-axis" aria-label="updates">
        {updates.map((u, i) => (
          <li key={u.t_ms}>
            <b>{`u${i + 1}`}</b>
            <span>{`${u.t_ms} ms`}</span>
            <span>{u.net === null ? 'no edge' : <Money value={u.net} signed cents />}</span>
          </li>
        ))}
      </ol>
    </figure>
  );
}
```

```ts
// web/src/math/adapt.ts
// Engine outputs as the models of the reused 7a views (spec 7b §1 change 3).
import type { DurationRow, PayoffModel } from '../schematics/models';
import type { EpisodeModel, PayoffKind, PayoffMatrix } from '../wasm/types';

/** payoff_json's matrix as PayoffGrid's model. Cells and the basket row count $1 payoffs ("0", "1", "2"),
 *  never money amounts, so Number() is safe here. */
export function matrixToPayoff(m: PayoffMatrix, kind: PayoffKind): PayoffModel {
  const cell = (v: string | null): 0 | 1 | null => (v === null ? null : Number(v) === 0 ? 0 : 1);
  return {
    kind,
    s: m.rows.length,
    cols: m.cols,
    rows: m.rows.map((name, i) => ({ name, cells: (m.cells[i] ?? []).map(cell) })),
    basket: m.basket.map((v) => (v === null ? null : Number(v))),
    guaranteed: Number(m.guaranteed),
    broken: m.broken,
    possible: m.possible,
  };
}

/** Section e on DurationBars: the episode on the table from its open (update ticks after it), and our order
 *  landing at t_x, on one time scale. Nothing when the episode never opened. */
export function episodeRows(m: EpisodeModel): DurationRow[] {
  const open = m.open_ms;
  if (open === null) return [];
  const last = m.updates.length > 0 ? m.updates[m.updates.length - 1].t_ms : open;
  const end = m.close_ms ?? Math.max(open, last);
  const rows: DurationRow[] = [
    { kind: 'row', label: 'on the table', ms: Math.max(0, end - open), open: m.close_ms === null, ticks: m.updates.map((u) => u.t_ms - open).filter((t) => t > 0) },
  ];
  if (m.t_x !== null) rows.push({ kind: 'order', label: 'our order lands', ms: Math.max(0, m.t_x - open), open: false, ticks: [] });
  return rows;
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7b §6.2: b's impossible column, e's order bar and its edge per update (ub-*). */
.payoff .impossible { color: var(--dim); }
.dbars .db-bar.order { fill: var(--salmon); }
.ubars { display: grid; gap: 4px; margin: 0; min-width: 0; }
.ub-svg { display: block; width: 100%; height: 56px; background: var(--bg); }
.ub-svg .ub-bar { fill: var(--teal); }
.ub-svg .ub-bar.peak { fill: var(--mint); }
.ub-svg .ub-zero { stroke: var(--line2); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.ub-axis { list-style: none; display: grid; grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); gap: 4px; text-align: center; font-size: 13px; line-height: 1.3; }
.ub-axis li { display: grid; min-width: 0; }
```

In `web/src/schematics/publicSafe.test.ts`, `ENTRIES` gains `'src/schematics/UpdateBars.tsx'`.

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/math/adapt.test.tsx src/schematics src/explain`
Expected: PASS (the fixture check runs once 7b-A's fixtures are in the tree).

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/schematics/models.ts web/src/schematics/PayoffGrid.tsx web/src/schematics/DurationBars.tsx web/src/schematics/UpdateBars.tsx web/src/schematics/publicSafe.test.ts web/src/math/adapt.ts web/src/math/adapt.test.tsx web/src/theme.css
git commit -m "web: b and e reuse PayoffGrid (implication, impossible column) and DurationBars (order bar), UpdateBars, engine adapters, panel payoff checked against the fixtures"
```

---

### Task 9: the math sections a–c (section frame, frame throttle, copy, defaults)

**Files:**
- Create: `web/src/math/copy.ts`, `web/src/math/defaults.ts`, `web/src/math/useFrameThrottle.ts`, `web/src/math/sections/Section.tsx`, `web/src/math/sections/MirrorSection.tsx`, `web/src/math/sections/PayoffSection.tsx`, `web/src/math/sections/ClassifySection.tsx`
- Test: `web/src/math/useFrameThrottle.test.tsx`, `web/src/math/sections-abc.test.tsx`

**Interfaces:**
- Consumes: `call` (Task 4), `Engine` and the models (`wasm/types.ts`), `MirrorBooks`, `ClassifyTree` (Task 6), `PayoffGrid`, `matrixToPayoff` (Task 8), `Fold`, `formatDecimal`; `loadTestEngine`, `HAVE_PKG`, `WASM_REQUIRED`, `skipReason` (tests).
- Produces:

```ts
// copy.ts
export const TITLE, TAGLINE, FOOTER, ENGINE_FAILED, LOADING, LIVE_MISSING: string;
export const SECTIONS: Record<'a' | 'b' | 'c' | 'd' | 'e' | 'f', { title: string; quip: string; caption: string }>;
// defaults.ts
export function toTick(p: string): number;      // '0.55' → 55
export function fromTick(t: number): string;    // 55 → '0.55', clamped to 1–99
export const LEGS: string[];                    // ['A' … 'F'], one name per leg (S goes to 6)
export const FED_BIDS: Level[]; export const FED_ASKS: Level[]; export const PAYOFF_TICKS: number[];
export interface ClassifyLeg { status: LegStatus; bid: number; ask: number }
export const CLASSIFY_LEGS: ClassifyLeg[];
// useFrameThrottle.ts
export function useFrameThrottle<T>(value: T): T
// sections/Section.tsx
export function Section(p: { id: string; title: string; quip: string; caption: string; controls: ReactNode; onReset: () => void; children: ReactNode; after?: ReactNode }): JSX.Element
export function Out<T>(p: { out: T | EngineError; children: (m: T) => ReactNode }): JSX.Element
export function Range(p: { label: string; min: number; max: number; step?: number; value: number; show?: string; onChange: (v: number) => void }): JSX.Element
export function Seg<T extends string | number>(p: { label: string; options: [T, string][]; value: T; onPick: (v: T) => void }): JSX.Element
export function Check(p: { label: string; checked: boolean; onChange: (v: boolean) => void }): JSX.Element
// sections
export const MIRROR_START: MirrorIn;   // = fixture mirror_json/fed-book's input
export function MirrorSection({ engine }: { engine: Engine }): JSX.Element
export function payoffInput(st: PayoffState): PayoffIn; export const PAYOFF_START: PayoffState; export function PayoffSection({ engine }: { engine: Engine }): JSX.Element
export function classifyInput(st: ClassifyState): ClassifyIn; export const CLASSIFY_START: ClassifyState;   // = fixture classify_json/no-edge-3's input
export function ClassifySection({ engine }: { engine: Engine }): JSX.Element
```

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/math/useFrameThrottle.test.tsx
import { act, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { fromTick, toTick } from './defaults';
import { useFrameThrottle } from './useFrameThrottle';

describe('useFrameThrottle (09-27 §6.2: one engine call per frame while dragging)', () => {
  it('passes one value per frame, and it is the latest', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal('cancelAnimationFrame', () => {});
    const seen: number[] = [];
    function Probe({ v }: { v: number }) {
      seen.push(useFrameThrottle(v));
      return null;
    }
    const { rerender } = render(<Probe v={1} />);
    rerender(<Probe v={2} />);
    rerender(<Probe v={3} />);
    rerender(<Probe v={4} />);
    expect(frames).toHaveLength(1);
    act(() => {
      frames.shift()?.(0);
    });
    expect(seen.at(-1)).toBe(4);
    expect([...new Set(seen)]).toEqual([1, 4]);
    expect(frames).toHaveLength(0);
  });
});

describe('ticks', () => {
  it('moves prices on the 0.01 grid by string math, clamped to 0.01–0.99', () => {
    expect(toTick('0.55')).toBe(55);
    expect(toTick('0.5')).toBe(50);
    expect(fromTick(55)).toBe('0.55');
    expect(fromTick(5)).toBe('0.05');
    expect(fromTick(0)).toBe('0.01');
    expect(fromTick(140)).toBe('0.99');
  });
});
```

```tsx
// web/src/math/sections-abc.test.tsx
// Engine figures come from 7b-A's fixtures (the sections start from fixture inputs) or from the engine for
// the page's own input; none is computed by hand here.
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { formatDecimal } from '../lib/money';
import { CROSSED } from '../schematics/MirrorBooks';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { ClassifyModel, Engine, MirrorModel, PayoffMatrix } from '../wasm/types';
import { CLASSIFY_START, ClassifySection, classifyInput } from './sections/ClassifySection';
import { MIRROR_START, MirrorSection } from './sections/MirrorSection';
import { PAYOFF_START, PayoffSection, payoffInput } from './sections/PayoffSection';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('sections a–c'));

const lit = (c: HTMLElement) => [...c.querySelectorAll('li.cl-node.lit')].map((li) => li.getAttribute('data-node'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('sections a–c on the real engine (09-27 §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it('a: the start is fixture fed-book, and the page draws its output (review focus 4)', () => {
    const fx = fixtureCase('mirror_json', 'fed-book');
    expect(MIRROR_START).toEqual(JSON.parse(fx.input));
    expect(engine.mirror_json(JSON.stringify(MIRROR_START))).toBe(fx.output);
    const m = JSON.parse(fx.output) as MirrorModel;
    render(<MirrorSection engine={engine} />);
    expect(screen.getByRole('heading', { name: 'a · one book, twice' })).toBeInTheDocument();
    expect(screen.getByText(`D1 long ${m.d1_long}`)).toBeInTheDocument();
    expect(screen.getByText(`short ${m.d1_short}`)).toBeInTheDocument();
    expect(screen.getByText(`spread ${m.spread}`)).toBeInTheDocument();
  });

  it('a: crossing the book raises the alert, and reset clears it', async () => {
    render(<MirrorSection engine={engine} />);
    fireEvent.click(screen.getByRole('button', { name: 'cross the book' }));
    expect(await screen.findByText(CROSSED)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'reset' }));
    await waitFor(() => expect(screen.queryByText(CROSSED)).toBeNull());
  });

  it("a: a size of 0 is the engine's sentence, and the page keeps working (review focus 2)", async () => {
    const { container } = render(<MirrorSection engine={engine} />);
    fireEvent.change(screen.getByLabelText('YES bid 1 size'), { target: { value: '0' } });
    expect(await screen.findByRole('alert')).toHaveTextContent('size "0" must be above 0 and at most 1,000,000, with at most 6 decimals');
    expect(container.querySelector('svg.mb-svg')).toBeNull();
    fireEvent.change(screen.getByLabelText('YES bid 1 size'), { target: { value: '100' } });
    await waitFor(() => expect(container.querySelector('svg.mb-svg')).not.toBeNull());
  });

  it('b: the YES basket with Other unlisted breaks on Other; the NO basket never does; the implication has an impossible column', async () => {
    render(<PayoffSection engine={engine} />);
    expect(screen.getByRole('columnheader', { name: 'Other ✗' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'NO basket' }));
    await waitFor(() => expect(screen.queryByRole('columnheader', { name: /✗/ })).toBeNull());
    fireEvent.click(screen.getByRole('button', { name: 'implication A ⇒ B' }));
    expect(await screen.findByRole('columnheader', { name: 'A∧¬B (impossible)' })).toBeInTheDocument();
  });

  it("b: with prices, the formula line carries the engine's top_edge_gross for the page's input", async () => {
    render(<PayoffSection engine={engine} />);
    fireEvent.click(screen.getByLabelText('“Other” is listed'));
    fireEvent.click(screen.getByLabelText('with prices'));
    const m = JSON.parse(engine.payoff_json(JSON.stringify(payoffInput({ ...PAYOFF_START, listed: true, priced: true })))) as PayoffMatrix;
    expect(m.top_edge_gross).not.toBeNull();
    expect(await screen.findByText('1 − Σ asks =')).toBeInTheDocument();
    expect(screen.getByText(formatDecimal(m.top_edge_gross))).toHaveClass('m-big');
  });

  it('c: the start is fixture no-edge-3 and lights its path; a leg closed YES ends in nothing (review focus 4)', async () => {
    const fx = fixtureCase('classify_json', 'no-edge-3');
    expect(classifyInput(CLASSIFY_START)).toEqual(JSON.parse(fx.input));
    const m = JSON.parse(fx.output) as ClassifyModel;
    const { container } = render(<ClassifySection engine={engine} />);
    expect(lit(container)).toEqual(m.path);
    expect(container.querySelector('.cl-end')).toHaveTextContent('NO basket · converted');
    expect(container.querySelector('.cl-net')).toHaveTextContent(`Q* ${m.q_star}`);
    fireEvent.change(screen.getByLabelText('leg A status'), { target: { value: 'closed_yes' } });
    await waitFor(() => expect(lit(container)).toEqual(['decided']));
    expect(container.querySelector('.cl-end')).toHaveTextContent(/^nothing$/);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/useFrameThrottle.test.tsx src/math/sections-abc.test.tsx`
Expected: FAIL: `Failed to resolve import "./defaults"` / `"./useFrameThrottle"` / `"./sections/…"`.

- [ ] **Step 3: Implement copy, defaults and the throttle**

```ts
// web/src/math/copy.ts
// The public page's own copy (09-27 §5.1: no help.ts). Honest money words only: on paper, on the table,
// captured at N ms (Task 12 scans the rendered page).
export const TITLE = 'isoca-radar · math';
export const TAGLINE =
  'How a basket arbitrage on Polymarket is found, sized and timed, and why it still loses. Every number below comes from the scanner’s own Rust code, compiled to WASM.';
export const FOOTER = 'The live dashboard is private. The math is not.';
export const ENGINE_FAILED = 'The math engine did not load. Your browser may be too old, or something blocked it.';
export const LOADING = 'Loading the math engine. The pixels are warming up.';
export const LIVE_MISSING = 'live numbers unavailable';

export const SECTIONS = {
  a: {
    title: 'a · one book, twice',
    quip: 'Same book, twice. If both sides ever look like free money, the free money is a bug.',
    caption: 'Computed by arb_core::book::d1_sums and detect::canary, the scanner’s own code compiled to WASM.',
  },
  b: {
    title: 'b · payoff tables',
    quip: 'A basket is only risk-free if nothing outside it can win.',
    caption: 'Computed by arb_core::payoff; the edge at the top of the book by detect::classify.',
  },
  c: {
    title: 'c · classify one update',
    quip: 'Most updates end in “nothing”. That is the right answer most of the time.',
    caption: 'Computed by arb_core::detect::classify. post_end is not here: the scanner adds it from event dates, and it never changes the kind.',
  },
  d: {
    title: 'd · depth walk',
    quip: 'The first shares are the cheap ones. The walk stops where the next basket costs more than it pays.',
    caption: 'Computed by arb_core::fill::walk_steps and fill::walk.',
  },
  e: {
    title: 'e · episode and latency',
    quip: 'The edge was real. It just did not wait.',
    caption:
      'Teaching model: one taker at t_x gets the edge of the latest update, or nothing once the episode closed; no queue, no consumed liquidity, no unwinds, so caught is not filled: see f. Computed by detect::classify, episode::EpisodeTracker and sim::latency::exec_delay_ms.',
  },
  f: {
    title: 'f · one leg misses',
    quip: 'Two legs filled, one did not. The two you hold are now a bet, and the exit is the bid.',
    caption:
      'Computed by sim::policy::settle and sim::venue::basket_buy / unwind_sell: the same rules the paper accounts on the live scanner use; here the unwind happens at one instant, the live sim waits the account’s delay.',
  },
} as const;
```

```ts
// web/src/math/defaults.ts
// Starting states (09-27 §6.2, 7b §6.2). Prices move on the 0.01 grid as whole ticks (string math only).
import type { LegStatus, Level } from '../wasm/types';

/** '0.55' → 55 ('0.5' → 50). */
export function toTick(p: string): number {
  const [i = '0', f = ''] = p.split('.');
  return Number(i) * 100 + Number(f.padEnd(2, '0').slice(0, 2));
}

/** 55 → '0.55', clamped to 0.01–0.99. */
export function fromTick(t: number): string {
  const c = Math.min(99, Math.max(1, Math.round(t)));
  return `0.${String(c).padStart(2, '0')}`;
}

export const LEGS = ['A', 'B', 'C', 'D', 'E', 'F'];

/** Parent §5.7 (a): the Fed book, best first; the same levels as 7b-A's fixture mirror_json/fed-book. */
export const FED_BIDS: Level[] = [
  { p: '0.54', s: '5603.74' }, { p: '0.53', s: '85887.16' }, { p: '0.52', s: '42239.53' }, { p: '0.51', s: '28133.09' },
];
export const FED_ASKS: Level[] = [
  { p: '0.55', s: '32356.03' }, { p: '0.56', s: '252281.27' }, { p: '0.57', s: '23963.36' }, { p: '0.58', s: '5637.30' },
];

/** Section b's optional prices, one per leg (S up to 6): the YES asks or the NO bids, in ticks. */
export const PAYOFF_TICKS = [30, 31, 33, 30, 30, 30];

export interface ClassifyLeg {
  status: LegStatus;
  bid: number;
  ask: number;
}
/** Section c: three live legs, Σ YES bids 1.05, a small NO-basket edge (7b-A's fixture classify_json/no-edge-3). */
export const CLASSIFY_LEGS: ClassifyLeg[] = [
  { status: 'live', bid: 40, ask: 41 },
  { status: 'live', bid: 35, ask: 36 },
  { status: 'live', bid: 30, ask: 31 },
];
```

```ts
// web/src/math/useFrameThrottle.ts
import { useEffect, useRef, useState } from 'react';

const hasFrames = (): boolean => typeof globalThis.requestAnimationFrame === 'function';

/**
 * 09-27 §6.2: every control change recomputes synchronously, at most once per animation frame while
 * dragging. Returns `value`, updated at most once per frame and always ending on the latest one.
 */
export function useFrameThrottle<T>(value: T): T {
  const [out, setOut] = useState(value);
  const latest = useRef(value);
  latest.current = value;
  const pending = useRef<{ id: number; frame: boolean } | null>(null);
  useEffect(() => {
    if (Object.is(value, out) || pending.current !== null) return;
    const done = () => {
      pending.current = null;
      setOut(latest.current);
    };
    const frame = hasFrames();
    const id = frame ? globalThis.requestAnimationFrame(() => done()) : setTimeout(done, 16);
    pending.current = { id, frame };
  }, [value, out]);
  useEffect(
    () => () => {
      const p = pending.current;
      if (p === null) return;
      if (p.frame) globalThis.cancelAnimationFrame(p.id);
      else clearTimeout(p.id);
      pending.current = null;
    },
    [],
  );
  return out;
}
```

- [ ] **Step 4: Implement the section frame and sections a–c**

```tsx
// web/src/math/sections/Section.tsx
import type { ReactNode } from 'react';
import { type EngineError, isError } from '../../wasm/types';

/** 09-27 §6.1: title, the answer and its drawing, controls with a reset, one deadpan line, and a caption
 *  naming the function that computed it. `after` is f's live part (7b §6.2). */
export function Section({ id, title, quip, caption, controls, onReset, children, after }: {
  id: string;
  title: string;
  quip: string;
  caption: string;
  controls: ReactNode;
  onReset: () => void;
  children: ReactNode;
  after?: ReactNode;
}) {
  return (
    <section className="box m-sec" aria-labelledby={`m-${id}`}>
      <h2 id={`m-${id}`}>{title}</h2>
      {children}
      <div className="m-controls">
        {controls}
        <button type="button" className="m-btn" onClick={onReset}>reset</button>
      </div>
      <p className="quip">{quip}</p>
      <p className="dim m-cap">{caption}</p>
      {after}
    </section>
  );
}

/** The engine's model, or its one sentence in place of the drawing. */
export function Out<T>({ out, children }: { out: T | EngineError; children: (m: T) => ReactNode }) {
  return isError(out) ? <p className="error" role="alert">{out.error}</p> : <>{children(out)}</>;
}

export function Range({ label, min, max, step = 1, value, show, onChange }: {
  label: string;
  min: number;
  max: number;
  step?: number;
  value: number;
  show?: string;
  onChange: (v: number) => void;
}) {
  return (
    <label className="m-range">
      <span>{label}</span>
      <input type="range" aria-label={label} min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="num">{show ?? String(value)}</span>
    </label>
  );
}

export function Seg<T extends string | number>({ label, options, value, onPick }: {
  label: string;
  options: [T, string][];
  value: T;
  onPick: (v: T) => void;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={String(v)} type="button" className={v === value ? 'on' : undefined} aria-pressed={v === value} onClick={() => onPick(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

export function Check({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="m-check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} /> {label}
    </label>
  );
}
```

```tsx
// web/src/math/sections/MirrorSection.tsx
import { useMemo, useState } from 'react';
import { Fold } from '../../components/Fold';
import { MirrorBooks } from '../../schematics/MirrorBooks';
import { call } from '../../wasm/call';
import type { Engine, Level, MirrorIn, MirrorModel } from '../../wasm/types';
import { SECTIONS } from '../copy';
import { FED_ASKS, FED_BIDS, fromTick, toTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Out, Range, Section } from './Section';

export const MIRROR_START: MirrorIn = { bids: FED_BIDS, asks: FED_ASKS };
const START = MIRROR_START;
const SIDE = { bids: 'bid', asks: 'ask' } as const;

/** Best bid one tick above the best ask; the other bids at or above it go (no two levels at one price). */
function crossed(b: MirrorIn): MirrorIn {
  const p = fromTick(toTick(b.asks[0]?.p ?? '0.50') + 1);
  return { asks: b.asks, bids: [{ p, s: b.bids[0]?.s ?? '100' }, ...b.bids.slice(1).filter((l) => toTick(l.p) < toTick(p))] };
}

/** 09-27 §6.2 a: drag any level's price (0.01 ticks) or size; "cross the book". */
export function MirrorSection({ engine }: { engine: Engine }) {
  const [book, setBook] = useState<MirrorIn>(START);
  const input = useFrameThrottle(book);
  const out = useMemo(() => call<MirrorModel>(engine, 'mirror_json', input), [engine, input]);
  const setLevel = (side: 'bids' | 'asks', i: number, patch: Partial<Level>) =>
    setBook((b) => {
      const next = b[side].map((l, k) => (k === i ? { ...l, ...patch } : l));
      return side === 'bids' ? { ...b, bids: next } : { ...b, asks: next };
    });
  return (
    <Section
      id="a"
      {...SECTIONS.a}
      onReset={() => setBook(START)}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setBook(crossed)}>cross the book</button>
          <Fold id="math:a:levels" label="move the levels">
            {(['bids', 'asks'] as const).flatMap((side) =>
              book[side].map((l, i) => {
                const name = `YES ${SIDE[side]} ${i + 1}`;
                return (
                  <fieldset key={`${side}${i}`} className="m-level">
                    <legend>{name}</legend>
                    <Range label={`${name} price`} min={1} max={99} value={toTick(l.p)} show={l.p} onChange={(t) => setLevel(side, i, { p: fromTick(t) })} />
                    <label className="m-size">
                      size
                      <input type="number" aria-label={`${name} size`} min={0} step="0.01" value={l.s} onChange={(e) => setLevel(side, i, { s: e.target.value })} />
                    </label>
                  </fieldset>
                );
              }),
            )}
          </Fold>
        </>
      }
    >
      <Out out={out}>{(m) => <MirrorBooks bids={input.bids} asks={input.asks} model={m} />}</Out>
    </Section>
  );
}
```

```tsx
// web/src/math/sections/PayoffSection.tsx
import { useMemo, useState } from 'react';
import { formatDecimal } from '../../lib/money';
import { PayoffGrid } from '../../schematics/PayoffGrid';
import { call } from '../../wasm/call';
import type { Engine, PayoffIn, PayoffKind, PayoffMatrix } from '../../wasm/types';
import { matrixToPayoff } from '../adapt';
import { SECTIONS } from '../copy';
import { LEGS, PAYOFF_TICKS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section, Seg } from './Section';

export interface PayoffState {
  kind: PayoffKind;
  s: number;
  listed: boolean;
  priced: boolean;
  ticks: number[];
}
export const PAYOFF_START: PayoffState = { kind: 'yes', s: 3, listed: false, priced: false, ticks: PAYOFF_TICKS };
const START = PAYOFF_START;
const KINDS: [PayoffKind, string][] = [['yes', 'YES basket'], ['no', 'NO basket'], ['implication', 'implication A ⇒ B']];
const SIZES: [number, string][] = [2, 3, 4, 5, 6].map((k): [number, string] => [k, `S ${k}`]);
/** Parent §5.7 (b), L5: the gross edge at the top of the book; the value is the engine's. */
const FORMULA: Record<PayoffKind, string | null> = { yes: '1 − Σ asks', no: 'Σ bids − 1', implication: null };

/** The engine input: prices has length s, or 2 for the implication (whose s the engine ignores; the page
 *  sends 2); a price tick is the YES ask or the NO bid. */
export function payoffInput(st: PayoffState): PayoffIn {
  const n = st.kind === 'implication' ? 2 : st.s;
  const prices = st.priced
    ? st.ticks.slice(0, n).map((t) => (st.kind === 'no' ? { bid: fromTick(t), ask: fromTick(t + 1) } : { bid: fromTick(t - 1), ask: fromTick(t) }))
    : null;
  return { kind: st.kind, s: n, other_listed: st.kind === 'implication' ? false : st.listed, prices };
}

/** 09-27 §6.2 b: YES / NO / implication, S 2–6, "Other listed", optional prices per leg. */
export function PayoffSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<PayoffState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<PayoffMatrix>(engine, 'payoff_json', payoffInput(input)), [engine, input]);
  const set = (p: Partial<PayoffState>) => setSt((s) => ({ ...s, ...p }));
  const n = st.kind === 'implication' ? 2 : st.s;
  const side = st.kind === 'no' ? 'bid' : 'ask';
  const formula = FORMULA[input.kind];
  return (
    <Section
      id="b"
      {...SECTIONS.b}
      onReset={() => setSt(START)}
      controls={
        <>
          <Seg label="basket" options={KINDS} value={st.kind} onPick={(kind) => set({ kind })} />
          {st.kind !== 'implication' && <Seg label="outcomes in the set" options={SIZES} value={st.s} onPick={(s) => set({ s })} />}
          {st.kind !== 'implication' && <Check label="“Other” is listed" checked={st.listed} onChange={(listed) => set({ listed })} />}
          <Check label="with prices" checked={st.priced} onChange={(priced) => set({ priced })} />
          {st.priced &&
            st.ticks.slice(0, n).map((t, i) => (
              <Range
                key={LEGS[i] ?? i}
                label={`leg ${LEGS[i] ?? i + 1} ${side}`}
                min={2}
                max={98}
                value={t}
                show={fromTick(t)}
                onChange={(v) => set({ ticks: st.ticks.map((x, k) => (k === i ? v : x)) })}
              />
            ))}
        </>
      }
    >
      <Out out={out}>
        {(m) => (
          <>
            <PayoffGrid model={matrixToPayoff(m, input.kind)} label={input.kind === 'implication' ? 'payoff of the implication' : `payoff of the ${input.kind.toUpperCase()} basket`} />
            {formula !== null && m.top_edge_gross !== null && (
              <p className="m-formula">
                <span>{`${formula} =`}</span> <span className="m-big">{formatDecimal(m.top_edge_gross)}</span>{' '}
                <span className="dim">per basket at the top of the book, before fees, on the table</span>
              </p>
            )}
          </>
        )}
      </Out>
    </Section>
  );
}
```

```tsx
// web/src/math/sections/ClassifySection.tsx
import { useMemo, useState } from 'react';
import { ClassifyTree } from '../../schematics/ClassifyTree';
import { call } from '../../wasm/call';
import type { ClassifyIn, ClassifyModel, Engine, LegStatus } from '../../wasm/types';
import { SECTIONS } from '../copy';
import { CLASSIFY_LEGS, type ClassifyLeg, LEGS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

export interface ClassifyState {
  legs: ClassifyLeg[];
  augmented: boolean;
}
export const CLASSIFY_START: ClassifyState = { legs: CLASSIFY_LEGS, augmented: false };
const START = CLASSIFY_START;
const STATUSES: LegStatus[] = ['live', 'placeholder', 'other', 'halted', 'closed_no', 'closed_yes'];

/** One level a side per leg (10 shares, as 7b-A's classify fixtures), no fees: the update classify reads. */
export function classifyInput(st: ClassifyState): ClassifyIn {
  return {
    augmented: st.augmented,
    legs: st.legs.map((l) => ({ status: l.status, bids: [{ p: fromTick(l.bid), s: '10' }], asks: [{ p: fromTick(l.ask), s: '10' }], fee: null })),
  };
}

/** 09-27 §6.2 c: per leg its status and best bid and ask; "augmented". */
export function ClassifySection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<ClassifyState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<ClassifyModel>(engine, 'classify_json', classifyInput(input)), [engine, input]);
  const setLeg = (i: number, patch: Partial<ClassifyLeg>) => setSt((s) => ({ ...s, legs: s.legs.map((l, k) => (k === i ? { ...l, ...patch } : l)) }));
  return (
    <Section
      id="c"
      {...SECTIONS.c}
      onReset={() => setSt(START)}
      controls={
        <>
          {st.legs.map((l, i) => (
            <fieldset key={LEGS[i]} className="m-leg">
              <legend>{`leg ${LEGS[i]}`}</legend>
              <label className="m-select">
                status
                <select aria-label={`leg ${LEGS[i]} status`} value={l.status} onChange={(e) => setLeg(i, { status: e.target.value as LegStatus })}>
                  {STATUSES.map((s) => (
                    <option key={s} value={s}>{s.replace('_', ' ')}</option>
                  ))}
                </select>
              </label>
              <Range label={`leg ${LEGS[i]} best bid`} min={1} max={99} value={l.bid} show={fromTick(l.bid)} onChange={(bid) => setLeg(i, { bid })} />
              <Range label={`leg ${LEGS[i]} best ask`} min={1} max={99} value={l.ask} show={fromTick(l.ask)} onChange={(ask) => setLeg(i, { ask })} />
            </fieldset>
          ))}
          <Check label="augmented event (an “Other” leg can appear)" checked={st.augmented} onChange={(augmented) => setSt((s) => ({ ...s, augmented }))} />
        </>
      }
    >
      <Out out={out}>{(m) => <ClassifyTree model={m} />}</Out>
    </Section>
  );
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/useFrameThrottle.test.tsx src/math/sections-abc.test.tsx`
Expected: PASS (2 + 6 tests).
If a `… is fixture …` test fails on the input: the section's start drifted from 7b-A's fixture: fix `defaults.ts`, never the fixture.

- [ ] **Step 6: Types, full suite**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 7: Commit**

```bash
git add web/src/math/copy.ts web/src/math/defaults.ts web/src/math/useFrameThrottle.ts web/src/math/useFrameThrottle.test.tsx web/src/math/sections/Section.tsx web/src/math/sections/MirrorSection.tsx web/src/math/sections/PayoffSection.tsx web/src/math/sections/ClassifySection.tsx web/src/math/sections-abc.test.tsx
git commit -m "web: math sections a-c on the engine (one call per frame, the engine's sentence on bad input), copy and starting states"
```

---

### Task 10: the math sections d–f (depth walk, episode, one leg misses)

**Files:**
- Create: `web/src/math/basket.ts`, `web/src/math/sections/DepthSection.tsx`, `web/src/math/sections/EpisodeSection.tsx`, `web/src/math/sections/LegMissSection.tsx`
- Modify: `web/src/math/defaults.ts` (append)
- Test: `web/src/math/defaults.test.ts`, `web/src/math/basket.test.ts`, `web/src/math/sections-def.test.tsx`

**Interfaces:**
- Consumes: Task 9's frame, throttle, `toTick`/`fromTick`, `LEGS`; `DepthStairs`, `BasketLegs` (Task 7); `DurationBars`, `UpdateBars`, `episodeRows` (Task 8); `LADDER` (`lib/ladder.ts`, test).
- Produces:

```ts
// basket.ts
export interface LegBook { bids: Level[]; asks: Level[] }
export interface FState { depth: number[]; against: number[]; q: number; fees: boolean }
export const CRYPTO_FEE: Fee;                                     // { rate: '0.07', exponent: 1 }
export function drain(levels: Level[], pct: number): Level[];      // sizes cut to pct %, whole shares, empty levels gone
export function against(b: LegBook, ticks: number): LegBook;       // YES asks up `ticks` (for NO: its bids fall)
export function basketInput(books: LegBook[], st: FState): BasketIn // kind "no", fee per leg, min_order / convert_fee_bips left to their defaults
// defaults.ts (append)
export const WALK_LEGS: Level[][]; export const NEW_WALK_LEG: Level[];
export function exactlyOne(legs: Level[][]): Level[][];
export const EPISODE_T: number[]; export const EPISODE_BIDS: number[][];
export const PRESETS: readonly { name: string; proc_ms: number; net_ms: number; match_ms: number }[];
export const F_BOOKS: LegBook[]; export const F_START: FState; export const F_BURST: FState;   // = fixtures no-all-fill, no-one-leg-empty-burst
export function depthInput(st: DepthState): DepthWalkIn; export const DEPTH_START: DepthState;
// sections
export function DepthSection({ engine }: { engine: Engine }): JSX.Element
export function EpisodeSection({ engine }: { engine: Engine }): JSX.Element
export function LegMissSection({ engine, children }: { engine: Engine; children?: ReactNode }): JSX.Element   // children: the live part
```

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/math/defaults.test.ts
import policiesToml from '../../../config/policies.toml?raw';
import { describe, expect, it } from 'vitest';
import { LADDER } from '../lib/ladder';
import { PRESETS, WALK_LEGS, exactlyOne, toTick } from './defaults';

describe('starting states', () => {
  it("e's presets are the LADDER rungs of config/policies.toml, same proc, net and match", () => {
    expect(PRESETS.map((p) => p.name)).toEqual(LADDER.map((l) => l.scenario));
    const blocks = policiesToml.split('[[scenarios]]').slice(1);
    for (const p of PRESETS) {
      const block = blocks.find((b) => new RegExp(`name = "${p.name}"`).test(b));
      expect(block, p.name).toBeDefined();
      for (const k of ['proc_ms', 'net_ms', 'match_ms'] as const) {
        expect(Number(new RegExp(`${k} = (\\d+)`).exec(block ?? '')?.[1]), `${p.name} ${k}`).toBe(p[k]);
      }
    }
  });

  it('"exactly 1.00" shifts leg A so the best asks sum to 1.00, keeping its level order', () => {
    const one = exactlyOne(WALK_LEGS);
    expect(one.reduce((a, l) => a + toTick(l[0].p), 0)).toBe(100);
    expect(one[0].map((l) => l.p)).toEqual(['0.36', '0.38', '0.41']);
    expect(one.slice(1)).toEqual(WALK_LEGS.slice(1));
  });
});
```

```ts
// web/src/math/basket.test.ts
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { FIXTURES, fixtureCase } from '../wasm/testEngine';
import { CRYPTO_FEE, against, basketInput, drain } from './basket';
import { F_BOOKS, F_BURST, F_START } from './defaults';

/** What JSON.stringify would send (undefined fields gone), to compare with a fixture's parsed input. */
const wire = (x: unknown): unknown => JSON.parse(JSON.stringify(x));

describe('section f input (spec 7b §6.2 f, 7b-A basket_json)', () => {
  it('drains the t_x depth in whole shares and drops an emptied level', () => {
    expect(drain(F_BOOKS[0].bids, 50)).toEqual([{ p: '0.40', s: '50' }]);
    expect(drain(F_BOOKS[0].bids, 0)).toEqual([]);
    expect(drain(F_BOOKS[0].bids, 100)).toEqual(F_BOOKS[0].bids);
  });

  it('moves the unwind book against a NO holder: the YES asks rise, the bids stay', () => {
    const b = against(F_BOOKS[1], 5);
    expect(b.asks).toEqual([{ p: '0.41', s: '100' }]);
    expect(b.bids).toEqual(F_BOOKS[1].bids);
  });

  it('sends a fee on every leg, and no min_order or convert_fee_bips (the engine defaults "5" and "0")', () => {
    const start = basketInput(F_BOOKS, F_START);
    expect(Object.keys(start).sort()).toEqual(['kind', 'legs', 'q_request']);
    expect(start.legs.map((l) => l.fee)).toEqual([null, null, null]);
    expect(basketInput(F_BOOKS, { ...F_START, fees: true }).legs.map((l) => l.fee)).toEqual([CRYPTO_FEE, CRYPTO_FEE, CRYPTO_FEE]);
  });
});

describe.skipIf(!existsSync(`${FIXTURES}/basket_json.json`))('section f starts from 7b-A fixtures (review focus 4)', () => {
  it('the start is fixture no-all-fill, the usual burst is no-one-leg-empty-burst', () => {
    expect(wire(basketInput(F_BOOKS, F_START))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-all-fill').input));
    expect(wire(basketInput(F_BOOKS, F_BURST))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-one-leg-empty-burst').input));
  });

  it('leg C at 20 % is no-one-leg-partial, fees on is no-fees-on', () => {
    expect(wire(basketInput(F_BOOKS, { ...F_START, depth: [100, 100, 20] }))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-one-leg-partial').input));
    expect(wire(basketInput(F_BOOKS, { ...F_START, fees: true }))).toEqual(JSON.parse(fixtureCase('basket_json', 'no-fees-on').input));
  });
});
```

```tsx
// web/src/math/sections-def.test.tsx
// Engine figures come from 7b-A's fixtures, from the engine for the page's own input, or from the contract's
// stated rules (t_x = open_ms + proc + net + match; a YES step whose cost equals the payoff does not pay).
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { basketVerdict } from '../schematics/BasketLegs';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { BasketModel, Engine, WalkModel } from '../wasm/types';
import { basketInput } from './basket';
import { F_BOOKS, F_BURST, F_START } from './defaults';
import { DEPTH_START, DepthSection, depthInput } from './sections/DepthSection';
import { EpisodeSection } from './sections/EpisodeSection';
import { LegMissSection } from './sections/LegMissSection';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('sections d–f'));

const legItems = () => within(screen.getByRole('list', { name: 'legs' })).getAllByRole('listitem');

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('sections d–f on the real engine (7b §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it("d: the page draws the engine's walk for its start; \"exactly 1.00\" leaves nothing to take", async () => {
    const m = JSON.parse(engine.depth_walk_json(JSON.stringify(depthInput(DEPTH_START)))) as WalkModel;
    const { container } = render(<DepthSection engine={engine} />);
    expect(screen.getByText(`Q* ${m.q_star}`)).toBeInTheDocument();
    expect(container.querySelectorAll('rect.ds-pi').length).toBe(m.steps.filter((s) => s.profitable && s.cost !== s.payoff).length);
    fireEvent.click(screen.getByRole('button', { name: 'exactly 1.00' }));
    expect(await screen.findByText('Q* 0')).toBeInTheDocument();
  });

  it('e: sp-idle catches the example episode 300 ms after it opens; 1,000 ms of net and match misses it', async () => {
    const { container } = render(<EpisodeSection engine={engine} />);
    expect(screen.getByText('caught')).toHaveClass('m-verdict');
    expect(screen.getByText(/^captured at 300 ms:/)).toBeInTheDocument();
    expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
    fireEvent.change(screen.getByLabelText('net ms'), { target: { value: '1000' } });
    fireEvent.change(screen.getByLabelText('match ms'), { target: { value: '1000' } });
    expect(await screen.findByText('missed')).toBeInTheDocument();
  });

  it('f: the start is fixture no-all-fill and says its net cash; the usual burst is no-one-leg-empty-burst', async () => {
    const all = fixtureCase('basket_json', 'no-all-fill');
    const burst = fixtureCase('basket_json', 'no-one-leg-empty-burst');
    expect(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, F_START)))).toBe(all.output);
    expect(engine.basket_json(JSON.stringify(basketInput(F_BOOKS, F_BURST)))).toBe(burst.output);
    render(<LegMissSection engine={engine}><p>live part</p></LegMissSection>);
    expect(screen.getByText(basketVerdict(JSON.parse(all.output) as BasketModel).big)).toHaveClass('bl-big');
    expect(screen.getByText('filled, +$2.50 on paper')).toBeInTheDocument();
    expect(screen.getByText('live part')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'the usual burst' }));
    expect(await screen.findByText('caught, and lost $6.00')).toBeInTheDocument();
    expect(legItems()[1]).toHaveTextContent(/^leg B0 of 50/);
    expect(legItems()[0]).toHaveTextContent('sold 50 at 0.54');
    expect(legItems()[2]).toHaveTextContent('sold 50 at 0.64');
  });

  it('f: leg C at 20 % at t_x (fixture no-one-leg-partial) fills 20 of 50', async () => {
    render(<LegMissSection engine={engine} />);
    fireEvent.change(screen.getByLabelText('leg C depth left at t_x'), { target: { value: '20' } });
    await waitFor(() => expect(legItems()[2]).toHaveTextContent(/^leg C20 of 50/));
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/defaults.test.ts src/math/basket.test.ts src/math/sections-def.test.tsx`
Expected: FAIL: `does not provide an export named 'PRESETS'` / `Failed to resolve import "./basket"` / `"./sections/DepthSection"`.

- [ ] **Step 3: Implement `basket.ts` and the defaults**

```ts
// web/src/math/basket.ts
// Section f's engine input (spec 7b §3.4, §6.2 f): YES-book levels at open, at t_x and at unwind time.
// A NO buy walks the YES bids (mirrored); a NO unwind sells into the YES asks (mirrored).
import type { BasketIn, Fee, Level } from '../wasm/types';
import { fromTick, toTick } from './defaults';

export interface LegBook {
  bids: Level[];
  asks: Level[];
}
export interface FState {
  /** Per leg, the depth left at t_x in % of the open book. */
  depth: number[];
  /** Per leg, ticks the unwind book moved against us. */
  against: number[];
  q: number;
  fees: boolean;
}

/** fee = shares × 0.07 × p(1 − p): the crypto schedule (crates/arb-core/tests/fee.rs). */
export const CRYPTO_FEE: Fee = { rate: '0.07', exponent: 1 };

/** Sizes cut to `pct` % in whole shares (sizes are shares, not money); a level cut to 0 is gone. Applied to
 *  the side a NO buy walks (the YES bids); the t_x asks stay, as in 7b-A's fixtures. */
export function drain(levels: Level[], pct: number): Level[] {
  return levels.map((l) => ({ p: l.p, s: String(Math.floor((Number(l.s) * pct) / 100)) })).filter((l) => l.s !== '0');
}

/** For a NO holder the unwind book moves against us when the YES asks rise (the NO bids fall). */
export function against(b: LegBook, ticks: number): LegBook {
  return { bids: b.bids, asks: b.asks.map((l) => ({ p: fromTick(toTick(l.p) + ticks), s: l.s })) };
}

/** 7b-A's basket_json input: kind "no", the fee on every leg, min_order and convert_fee_bips left to the
 *  engine's defaults ("5", the sim's min_order_shares, and "0"). */
export function basketInput(books: LegBook[], st: FState): BasketIn {
  return {
    kind: 'no',
    legs: books.map((b, i) => ({
      open: b,
      t_x: { bids: drain(b.bids, st.depth[i] ?? 100), asks: b.asks },
      unwind: against(b, st.against[i] ?? 0),
      fee: st.fees ? CRYPTO_FEE : null,
    })),
    q_request: String(st.q),
  };
}
```

`drain(F_BOOKS[0].bids, 100)` must return the same string (`'100'`), which `String(Math.floor(100))` does.

Append to `web/src/math/defaults.ts`:

```ts
import type { FState, LegBook } from './basket';

/** Section d: three legs, three ask levels each; Σ best asks 0.94 (a positive edge). */
export const WALK_LEGS: Level[][] = [
  [{ p: '0.30', s: '40' }, { p: '0.32', s: '60' }, { p: '0.35', s: '100' }],
  [{ p: '0.31', s: '50' }, { p: '0.33', s: '50' }, { p: '0.36', s: '100' }],
  [{ p: '0.33', s: '30' }, { p: '0.34', s: '70' }, { p: '0.37', s: '100' }],
];
/** The fourth leg "add a leg" appends. */
export const NEW_WALK_LEG: Level[] = [{ p: '0.05', s: '50' }, { p: '0.06', s: '50' }, { p: '0.07', s: '100' }];

/** "exactly 1.00" (09-27 §6.2 d): shift leg A's levels so the best asks sum to 1.00. */
export function exactlyOne(legs: Level[][]): Level[][] {
  const sum = legs.reduce((a, l) => a + (l[0] ? toTick(l[0].p) : 0), 0);
  const shift = 100 - sum;
  return legs.map((l, i) => (i === 0 ? l.map((x) => ({ ...x, p: fromTick(toTick(x.p) + shift) })) : l));
}

/** Parent §5.7 (e): five updates of a 3-leg NO basket, YES bids in ticks (asks one tick above, 100 deep):
 *  u1 Σ 0.99, u2 1.02 (open), u3 1.06 (peak), u4 1.02, u5 0.99 (close). */
export const EPISODE_T = [0, 400, 900, 1400, 1900];
export const EPISODE_BIDS: number[][] = [[33, 33, 33], [35, 34, 33], [37, 35, 34], [35, 34, 33], [33, 33, 33]];

/** Section e's presets: the six LADDER rungs of config/policies.toml (defaults.test.ts checks them). */
export const PRESETS = [
  { name: 'rung-0', proc_ms: 0, net_ms: 0, match_ms: 0 },
  { name: 'rung-25', proc_ms: 0, net_ms: 0, match_ms: 25 },
  { name: 'colo-idle', proc_ms: 1, net_ms: 1, match_ms: 50 },
  { name: 'rung-100', proc_ms: 5, net_ms: 45, match_ms: 50 },
  { name: 'rung-200', proc_ms: 5, net_ms: 95, match_ms: 100 },
  { name: 'sp-idle', proc_ms: 5, net_ms: 145, match_ms: 150 },
] as const;

/** Section f (7b §6.2), exactly 7b-A's fixture basket_json/no-all-fill: a 3-leg NO basket, YES bids
 *  0.40 / 0.35 / 0.30 × 100 (NO asks 0.60 / 0.65 / 0.70, Σ 1.95 < 2), asks a tick up. */
export const F_BOOKS: LegBook[] = [
  { bids: [{ p: '0.40', s: '100' }], asks: [{ p: '0.41', s: '100' }] },
  { bids: [{ p: '0.35', s: '100' }], asks: [{ p: '0.36', s: '100' }] },
  { bids: [{ p: '0.30', s: '100' }], asks: [{ p: '0.31', s: '100' }] },
];
/** t_x and unwind books equal the open ones, 50 shares: fixture no-all-fill (net_cash "2.5"). */
export const F_START: FState = { depth: [100, 100, 100], against: [0, 0, 0], q: 50, fees: false };
/** "the usual burst" = fixture no-one-leg-empty-burst: leg B without bids at t_x, every unwind book 5 ticks
 *  against us (q_eff "0", net_cash "-6"). */
export const F_BURST: FState = { depth: [100, 0, 100], against: [5, 5, 5], q: 50, fees: false };
```

(Put the new `import type` line at the top of `defaults.ts`, beside the existing one; `basket.ts` imports values from `defaults.ts` and `defaults.ts` imports only types from `basket.ts`, so there is no runtime cycle.)

- [ ] **Step 4: Implement the sections**

```tsx
// web/src/math/sections/DepthSection.tsx
import { useMemo, useState } from 'react';
import { Fold } from '../../components/Fold';
import { DepthStairs } from '../../schematics/DepthStairs';
import { call } from '../../wasm/call';
import type { DepthWalkIn, Engine, Level, WalkModel } from '../../wasm/types';
import { CRYPTO_FEE } from '../basket';
import { SECTIONS } from '../copy';
import { LEGS, NEW_WALK_LEG, WALK_LEGS, exactlyOne, fromTick, toTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

export interface DepthState {
  legs: Level[][];
  fees: boolean;
}
export const DEPTH_START: DepthState = { legs: WALK_LEGS, fees: false };
const START = DEPTH_START;

export function depthInput(st: DepthState): DepthWalkIn {
  return { rule: 'yes', legs: st.legs.map((levels) => ({ levels, fee: st.fees ? CRYPTO_FEE : null })) };
}

/** 09-27 §6.2 d: drag levels on up to 4 legs, fees on or off, "exactly 1.00". */
export function DepthSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<DepthState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<WalkModel>(engine, 'depth_walk_json', depthInput(input)), [engine, input]);
  const setP = (li: number, i: number, t: number) =>
    setSt((s) => ({ ...s, legs: s.legs.map((l, k) => (k === li ? l.map((x, j) => (j === i ? { ...x, p: fromTick(t) } : x)) : l)) }));
  return (
    <Section
      id="d"
      {...SECTIONS.d}
      onReset={() => setSt(START)}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setSt((s) => ({ ...s, legs: exactlyOne(s.legs) }))}>exactly 1.00</button>
          <button type="button" className="m-btn" disabled={st.legs.length >= 4} onClick={() => setSt((s) => ({ ...s, legs: [...s.legs, NEW_WALK_LEG] }))}>add a leg</button>
          <button type="button" className="m-btn" disabled={st.legs.length <= 2} onClick={() => setSt((s) => ({ ...s, legs: s.legs.slice(0, -1) }))}>remove a leg</button>
          <Check label="taker fees on (crypto: rate 0.07, exponent 1)" checked={st.fees} onChange={(fees) => setSt((s) => ({ ...s, fees }))} />
          <Fold id="math:d:levels" label="move the levels">
            {st.legs.map((l, li) =>
              l.map((x, i) => (
                <Range key={`${li}-${i}`} label={`leg ${LEGS[li]} level ${i + 1}`} min={1} max={99} value={toTick(x.p)} show={x.p} onChange={(t) => setP(li, i, t)} />
              )),
            )}
          </Fold>
        </>
      }
    >
      <Out out={out}>{(m) => <DepthStairs model={m} />}</Out>
    </Section>
  );
}
```

```tsx
// web/src/math/sections/EpisodeSection.tsx
import { useMemo, useState } from 'react';
import { Money } from '../../components/Money';
import { DurationBars } from '../../schematics/DurationBars';
import { UpdateBars } from '../../schematics/UpdateBars';
import { call } from '../../wasm/call';
import type { Engine, EpisodeIn, EpisodeModel, LegIn } from '../../wasm/types';
import { episodeRows } from '../adapt';
import { SECTIONS } from '../copy';
import { EPISODE_BIDS, EPISODE_T, PRESETS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section, Seg } from './Section';

type Lat = { proc_ms: number; net_ms: number; match_ms: number };
interface EpisodeState {
  sc: Lat;
  delay: boolean;
  upto: number;
}
const SP_IDLE = PRESETS[PRESETS.length - 1];
const START: EpisodeState = { sc: { proc_ms: SP_IDLE.proc_ms, net_ms: SP_IDLE.net_ms, match_ms: SP_IDLE.match_ms }, delay: false, upto: EPISODE_T.length };
const OPTIONS: [string, string][] = PRESETS.map((p): [string, string] => [p.name, p.name]);
const KNOBS: [keyof Lat, string][] = [['proc_ms', 'proc ms'], ['net_ms', 'net ms'], ['match_ms', 'match ms']];

const leg = (bid: number): LegIn => ({ status: 'live', bids: [{ p: fromTick(bid), s: '100' }], asks: [{ p: fromTick(bid + 1), s: '100' }], fee: null });

export function episodeInput(st: EpisodeState): EpisodeIn {
  return {
    kind: 'no',
    gap_ms: 30_000,
    updates: EPISODE_T.slice(0, st.upto).map((t, i) => ({ t_ms: t, legs: EPISODE_BIDS[i].map(leg) })),
    scenario: { ...st.sc, honor_seconds_delay: true },
    seconds_delay: st.delay ? 1 : null,
  };
}

/** 09-27 §6.2 e: scrub updates, proc/net/match sliders with the LADDER presets, secondsDelay. */
export function EpisodeSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<EpisodeState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<EpisodeModel>(engine, 'episode_json', episodeInput(input)), [engine, input]);
  const preset = PRESETS.find((p) => p.proc_ms === st.sc.proc_ms && p.net_ms === st.sc.net_ms && p.match_ms === st.sc.match_ms)?.name ?? 'custom';
  return (
    <Section
      id="e"
      {...SECTIONS.e}
      onReset={() => setSt(START)}
      controls={
        <>
          <Seg
            label="latency presets from the ladder"
            options={OPTIONS}
            value={preset}
            onPick={(name) => {
              const p = PRESETS.find((x) => x.name === name);
              if (p) setSt((s) => ({ ...s, sc: { proc_ms: p.proc_ms, net_ms: p.net_ms, match_ms: p.match_ms } }));
            }}
          />
          {KNOBS.map(([k, label]) => (
            <Range key={k} label={label} min={0} max={1000} value={st.sc[k]} onChange={(v) => setSt((s) => ({ ...s, sc: { ...s.sc, [k]: v } }))} />
          ))}
          <Range label="updates so far" min={2} max={EPISODE_T.length} value={st.upto} show={`u1–u${st.upto}`} onChange={(upto) => setSt((s) => ({ ...s, upto }))} />
          <Check label="the market has a 1 s secondsDelay" checked={st.delay} onChange={(delay) => setSt((s) => ({ ...s, delay }))} />
        </>
      }
    >
      <Out out={out}>
        {(m) => {
          const lat = m.t_x !== null && m.open_ms !== null ? m.t_x - m.open_ms : null;
          const word = m.open_ms === null ? 'no episode yet' : m.caught ? 'caught' : 'missed';
          return (
            <>
              <p className={m.open_ms === null ? 'm-verdict' : m.caught ? 'm-verdict pos' : 'm-verdict neg'}>{word}</p>
              {m.caught && m.captured !== null && lat !== null && (
                <p>
                  {`captured at ${lat} ms: `}
                  <Money value={m.captured} signed cents /> on paper, in this teaching model
                </p>
              )}
              <UpdateBars updates={m.updates} peakMs={m.peak_ms} />
              <DurationBars rows={episodeRows(m)} />
            </>
          );
        }}
      </Out>
    </Section>
  );
}
```

`{ ...s.sc, [k]: v }` with `k: keyof Lat` (all three `number`) type-checks as `Lat`.

```tsx
// web/src/math/sections/LegMissSection.tsx
import { type ReactNode, useMemo, useState } from 'react';
import { BasketLegs } from '../../schematics/BasketLegs';
import { call } from '../../wasm/call';
import type { BasketModel, Engine } from '../../wasm/types';
import { type FState, basketInput } from '../basket';
import { SECTIONS } from '../copy';
import { F_BOOKS, F_BURST, F_START, LEGS } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

/** 7b §6.2 f: per leg the depth left at t_x and the unwind book against us; shares; fees; "the usual burst".
 *  `children` is the live part (the ladder from the snapshot), drawn after the caption. */
export function LegMissSection({ engine, children }: { engine: Engine; children?: ReactNode }) {
  const [st, setSt] = useState<FState>(F_START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<BasketModel>(engine, 'basket_json', basketInput(F_BOOKS, input)), [engine, input]);
  const upd = (xs: number[], i: number, v: number) => xs.map((x, j) => (j === i ? v : x));
  const names = LEGS.slice(0, F_BOOKS.length);
  return (
    <Section
      id="f"
      {...SECTIONS.f}
      onReset={() => setSt(F_START)}
      after={children}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setSt(F_BURST)}>the usual burst</button>
          {names.map((n, i) => (
            <fieldset key={n} className="m-leg">
              <legend>{`leg ${n}`}</legend>
              <Range label={`leg ${n} depth left at t_x`} min={0} max={100} step={5} value={st.depth[i]} show={`${st.depth[i]}%`} onChange={(v) => setSt((s) => ({ ...s, depth: upd(s.depth, i, v) }))} />
              <Range label={`leg ${n} unwind book against us`} min={0} max={20} value={st.against[i]} show={`${st.against[i]} ticks`} onChange={(v) => setSt((s) => ({ ...s, against: upd(s.against, i, v) }))} />
            </fieldset>
          ))}
          <Range label="shares to buy" min={5} max={200} step={5} value={st.q} onChange={(q) => setSt((s) => ({ ...s, q }))} />
          <Check label="taker fees on (crypto: rate 0.07, exponent 1)" checked={st.fees} onChange={(fees) => setSt((s) => ({ ...s, fees }))} />
        </>
      }
    >
      <Out out={out}>{(m) => <BasketLegs model={m} names={names} />}</Out>
    </Section>
  );
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/defaults.test.ts src/math/basket.test.ts src/math/sections-def.test.tsx`
Expected: PASS (2 + 5 + 4 tests).
If a fixture comparison fails: the section's input drifted from 7b-A's fixture (fix `defaults.ts` / `basket.ts`), or the engine differs from its own fixture (report it; a 7b-A issue). Never edit a fixture here.

- [ ] **Step 6: Types, full suite**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 7: Commit**

```bash
git add web/src/math/basket.ts web/src/math/basket.test.ts web/src/math/defaults.ts web/src/math/defaults.test.ts web/src/math/sections/DepthSection.tsx web/src/math/sections/EpisodeSection.tsx web/src/math/sections/LegMissSection.tsx web/src/math/sections-def.test.tsx
git commit -m "web: math sections d-f: depth walk past Q*, episode with the ladder presets, one leg misses (drain at t_x, unwind against us, the usual burst)"
```

---

### Task 11: the snapshot on the page (`useSnapshot`, `LiveStrip`, `LiveLadder`)

**Files:**
- Create: `web/src/math/useSnapshot.ts`, `web/src/math/LiveStrip.tsx`, `web/src/math/LiveLadder.tsx`
- Test: `web/src/math/live.test.tsx`

**Interfaces:**
- Consumes: `SNAPSHOT_URL`, `REFETCH_MS`, `parseSnapshot`, `publicToLadder`, `hourlyValues` (Task 3); `LatencyLadder`, `ladderModel`; `Spark`, `<Money>`; `fmtUtcHm`, `fmtUtcDayHm`; tests: `mockApi`, `jsonResponse`, `settle`, `snapshotFixture`.
- Produces:

```ts
export type SnapshotLoad = { kind: 'loading' } | { kind: 'missing' } | { kind: 'ok'; snap: PublicSnapshot };
export async function fetchSnapshot(): Promise<PublicSnapshot | null>   // any failure → null
export function useSnapshot(): SnapshotLoad
export const BEHIND = 'the live writer is behind';
export function asOfLine(writtenMs: number, stale: boolean): string     // 'as of 14:05 UTC' | 'as of 2026-09-30 11:00 UTC: the live writer is behind'
export function LiveStrip({ today, writtenMs, stale }: { today: PublicToday; writtenMs: number; stale: boolean }): JSX.Element
export function LiveLadder({ snap, stale }: { snap: PublicSnapshot; stale: boolean }): JSX.Element
```

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/math/live.test.tsx
import { render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { snapshotFixture } from './fixtures';
import { LiveLadder } from './LiveLadder';
import { BEHIND, LiveStrip } from './LiveStrip';
import { REFETCH_MS, SNAPSHOT_URL } from './snapshot';
import { type SnapshotLoad, useSnapshot } from './useSnapshot';

let last: SnapshotLoad = { kind: 'loading' };
function Probe() {
  last = useSnapshot();
  return null;
}
function setVisible(v: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => v });
  document.dispatchEvent(new Event('visibilitychange'));
}
afterEach(() => {
  Reflect.deleteProperty(document, 'visibilityState');
  last = { kind: 'loading' };
});

describe('useSnapshot (spec 7b §6.3)', () => {
  it('fetches the one public file once on load, and nothing else', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture() });
    render(<Probe />);
    expect(last.kind).toBe('loading');
    await settle(f);
    expect(last.kind).toBe('ok');
    expect(f.mock.calls.map((c) => String(c[0]))).toEqual(['/math-data/public.json']);
  });

  it('404, a network error, an HTML page and v 2 all read missing (review focus 1)', async () => {
    const answers = [
      () => mockApi({}),
      () => {
        const f = vi.fn(async () => {
          throw new TypeError('Failed to fetch');
        });
        vi.stubGlobal('fetch', f);
        return f;
      },
      () => mockApi({ [SNAPSHOT_URL]: () => new Response('<html>502</html>', { status: 200, headers: { 'Content-Type': 'text/html' } }) }),
      () => mockApi({ [SNAPSHOT_URL]: { ...snapshotFixture(), v: 2 } }),
    ];
    for (const answer of answers) {
      const f = answer();
      const { unmount } = render(<Probe />);
      await settle(f);
      expect(last.kind).toBe('missing');
      unmount();
    }
  });

  it('re-fetches only when the tab comes back and the last fetch is over 5 minutes old', async () => {
    const t0 = 1_790_000_000_000;
    const now = vi.spyOn(Date, 'now').mockReturnValue(t0);
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, t0) });
    render(<Probe />);
    await settle(f);
    now.mockReturnValue(t0 + REFETCH_MS - 1000);
    setVisible('hidden');
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    now.mockReturnValue(t0 + REFETCH_MS + 1000);
    setVisible('hidden');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
  });

  it('a failed re-fetch keeps the last good snapshot (review focus 1)', async () => {
    const t0 = 1_790_000_000_000;
    const now = vi.spyOn(Date, 'now').mockReturnValue(t0);
    let up = true;
    const f = mockApi({ [SNAPSHOT_URL]: () => (up ? snapshotFixture({}, t0) : jsonResponse(404, {})) });
    render(<Probe />);
    await settle(f);
    up = false;
    now.mockReturnValue(t0 + REFETCH_MS + 1);
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    expect(last.kind).toBe('ok');
  });
});

describe('LiveStrip (spec 7b §6.1)', () => {
  it('draws today: the NO and YES counts, the edge on the table, the hours, as of HH:MM UTC', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 14, 5));
    const { container } = render(<LiveStrip today={s.today} writtenMs={s.written_ms} stale={false} />);
    expect(screen.getByText('12')).toHaveClass('m-big');
    expect(screen.getByText('3')).toHaveClass('m-big');
    expect(screen.getByText('$41.37')).toBeInTheDocument();
    expect(screen.getByText('as of 14:05 UTC')).toHaveClass('dim');
    expect(container.querySelectorAll('rect.spark-bar')).toHaveLength(24);
    expect(container.querySelector('svg.spark')).toHaveAttribute('preserveAspectRatio', 'none');
  });

  it('says when the writer is behind, and never reads an unmeasured edge as $0.00', () => {
    const base = snapshotFixture();
    const s = snapshotFixture({ today: { ...base.today, edge_budget100: null } }, Date.UTC(2026, 8, 30, 11, 0));
    render(<LiveStrip today={s.today} writtenMs={s.written_ms} stale />);
    expect(screen.getByText(`as of 2026-09-30 11:00 UTC: ${BEHIND}`)).toHaveClass('stale');
    expect(screen.getByText('edge on the table: not measured')).toBeInTheDocument();
    expect(screen.queryByText('$0.00')).toBeNull();
  });
});

describe('LiveLadder (spec 7b §6.2 f)', () => {
  it('heads the live ladder with its time and draws it from the snapshot', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 14, 5));
    render(<LiveLadder snap={s} stale={false} />);
    expect(screen.getByRole('heading', { name: 'on the live scanner, as of 14:05 UTC' })).toBeInTheDocument();
    expect(screen.getByText('≈ 70 ms')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/math/live.test.tsx`
Expected: FAIL: `Failed to resolve import "./LiveLadder"` (and `./LiveStrip`, `./useSnapshot`).

- [ ] **Step 3: Implement**

```ts
// web/src/math/useSnapshot.ts
import { useEffect, useRef, useState } from 'react';
import { type PublicSnapshot, REFETCH_MS, SNAPSHOT_URL, parseSnapshot } from './snapshot';

export type SnapshotLoad = { kind: 'loading' } | { kind: 'missing' } | { kind: 'ok'; snap: PublicSnapshot };

/** One GET of the static file; a 404, a network error, bad JSON, v ≠ 1 or a missing field is null (7b §6.3). */
export async function fetchSnapshot(): Promise<PublicSnapshot | null> {
  try {
    const r = await fetch(SNAPSHOT_URL, { credentials: 'same-origin' });
    if (!r.ok) return null;
    return parseSnapshot(await r.json());
  } catch {
    return null;
  }
}

/**
 * 7b §6.3: one fetch on load, and again only when the tab becomes visible and the last fetch is over five
 * minutes old; no polling loop. A failed re-fetch keeps the last good snapshot, whose own written_ms then
 * says how old it is (the page shows it stale), rather than blanking numbers it already had.
 */
export function useSnapshot(): SnapshotLoad {
  const [load, setLoad] = useState<SnapshotLoad>({ kind: 'loading' });
  const last = useRef(0);
  useEffect(() => {
    let live = true;
    const get = () => {
      last.current = Date.now();
      void fetchSnapshot().then((s) => {
        if (!live) return;
        setLoad((prev) => (s !== null ? { kind: 'ok', snap: s } : prev.kind === 'ok' ? prev : { kind: 'missing' }));
      });
    };
    get();
    const onVisible = () => {
      if (document.visibilityState === 'visible' && Date.now() - last.current > REFETCH_MS) get();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      live = false;
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);
  return load;
}
```

```tsx
// web/src/math/LiveStrip.tsx
import { Money } from '../components/Money';
import { Spark } from '../components/Spark';
import { fmtUtcDayHm, fmtUtcHm } from '../lib/time';
import { type PublicToday, hourlyValues } from './snapshot';

export const BEHIND = 'the live writer is behind';

/** 7b §6.3: "as of HH:MM UTC"; stale: the day too, and that the writer is behind. */
export function asOfLine(writtenMs: number, stale: boolean): string {
  return stale ? `as of ${fmtUtcDayHm(writtenMs)}: ${BEHIND}` : `as of ${fmtUtcHm(writtenMs)} UTC`;
}

/** 7b §6.1: today's arbitrage episodes (NO / YES), the edge on the table, the day by hour, and when. */
export function LiveStrip({ today, writtenMs, stale }: { today: PublicToday; writtenMs: number; stale: boolean }) {
  return (
    <section className="m-live" aria-label="today on the live scanner">
      <p className="m-live-k">today on the live scanner</p>
      <div className="m-live-row">
        <p className="m-live-n">
          <span className="m-big">{today.no}</span> NO · <span className="m-big">{today.yes}</span> YES <span className="dim">arbitrage episodes</span>
        </p>
        <p className="m-live-n">
          {today.edge_budget100 === null ? (
            <span className="dim">edge on the table: not measured</span>
          ) : (
            <>
              <span className="m-big"><Money value={today.edge_budget100} cents /></span> <span className="dim">edge on the table, $100 per episode at most</span>
            </>
          )}
        </p>
        <div className="m-live-spark">
          <Spark values={hourlyValues(today)} label="arbitrage episodes per UTC hour today" />
        </div>
      </div>
      <p className={stale ? 'stale' : 'dim'}>{asOfLine(writtenMs, stale)}</p>
    </section>
  );
}
```

```tsx
// web/src/math/LiveLadder.tsx
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { asOfLine } from './LiveStrip';
import { type PublicSnapshot, publicToLadder } from './snapshot';

/** 7b §6.2 f: the live ladder from the snapshot, read-only, with its own "which markets" fold. */
export function LiveLadder({ snap, stale }: { snap: PublicSnapshot; stale: boolean }) {
  return (
    <div className="m-liveladder">
      <h3 className="m-h3">{`on the live scanner, ${asOfLine(snap.written_ms, stale)}`}</h3>
      <LatencyLadder model={ladderModel(publicToLadder(snap.ladder))} />
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && npx vitest run src/math/live.test.tsx`
Expected: PASS (7 tests).

- [ ] **Step 5: Types, full suite**

Run: `cd web && npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 6: Commit**

```bash
git add web/src/math/useSnapshot.ts web/src/math/LiveStrip.tsx web/src/math/LiveLadder.tsx web/src/math/live.test.tsx
git commit -m "web: the live snapshot on the math page: one fetch, again only on a visible tab after 5 min, strip with the day's counts, live ladder"
```

---

### Task 12: the math page, its entry, `math.html` and `math.css`

**Files:**
- Create: `web/src/math/MathPage.tsx`, `web/src/math/main.tsx`, `web/math.html`, `web/src/math/math.css`
- Modify: `web/src/theme.test.ts` (one new `it`)
- Test: `web/src/math/MathPage.test.tsx`

**Interfaces:**
- Consumes: every section (Tasks 9–10), `LiveStrip`, `LiveLadder`, `useSnapshot`, `freshness`, `useNow` (`lib/hooks.ts`), `loadEngine` (`main.tsx` only).
- Produces: `MathPage({ engine }: { engine: Promise<Engine> }): JSX.Element` (tests inject an engine promise; `main.tsx` passes `loadEngine()`); the `math.html` entry.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/math/MathPage.test.tsx
import { render, screen, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { HAVE_PKG, WASM_REQUIRED, loadTestEngine, skipReason } from '../wasm/testEngine';
import type { Engine } from '../wasm/types';
import { ENGINE_FAILED, FOOTER, LIVE_MISSING } from './copy';
import { snapshotFixture } from './fixtures';
import { BEHIND } from './LiveStrip';
import { MathPage } from './MathPage';
import { SNAPSHOT_URL } from './snapshot';

const never = new Promise<Engine>(() => {});
const TITLES = ['a · one book, twice', 'b · payoff tables', 'c · classify one update', 'd · depth walk', 'e · episode and latency', 'f · one leg misses'];

describe('MathPage, the engine aside (spec 7b §6.3, 09-27 §8)', () => {
  it('shows one sentence when the engine fails to load, and the live strip still draws', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture() });
    render(<MathPage engine={Promise.reject(new Error('blocked'))} />);
    expect(await screen.findByText(ENGINE_FAILED)).toHaveAttribute('role', 'alert');
    await settle(f);
    expect(screen.getByRole('region', { name: 'today on the live scanner' })).toBeInTheDocument();
    expect(screen.queryAllByRole('heading', { level: 2 })).toHaveLength(0);
  });

  it('a missing snapshot is one dim line, and the page asks for nothing but that file', async () => {
    const f = mockApi({});
    render(<MathPage engine={never} />);
    await settle(f);
    expect(screen.getAllByText(LIVE_MISSING)).toHaveLength(1);
    expect(screen.queryByRole('region', { name: 'today on the live scanner' })).toBeNull();
    const urls = f.mock.calls.map((c) => String(c[0]));
    expect(urls).toEqual(['/math-data/public.json']);
    expect(urls.some((u) => u.includes('/api/'))).toBe(false);
  });

  it('a snapshot older than 30 minutes says the writer is behind', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, Date.now() - 45 * 60_000) });
    render(<MathPage engine={never} />);
    await settle(f);
    expect(screen.getByText(new RegExp(`: ${BEHIND}$`))).toHaveClass('stale');
  });
});

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('math page'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('MathPage on the real engine (spec 7b §6, §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it('draws a–f in order under the live strip, f ends with the live ladder, and the footer', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture() });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} />);
    await settle(f);
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(TITLES);
    const sf = screen.getByRole('region', { name: 'f · one leg misses' });
    expect(within(sf).getByRole('heading', { level: 3 })).toHaveTextContent(/^on the live scanner, as of \d\d:\d\d UTC$/);
    expect(within(sf).getByText('≈ 70 ms')).toBeInTheDocument();
    expect(screen.getByText(FOOTER)).toBeInTheDocument();
    expect(container.querySelectorAll('[style]')).toHaveLength(0); // the math CSP has no 'unsafe-inline' for styles
    expect(container.querySelectorAll('svg text')).toHaveLength(0);
    expect(container.innerHTML).not.toContain('NaN');
  });

  it('says only honest money words (on paper, on the table, captured at N ms)', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture() });
    render(<MathPage engine={Promise.resolve(engine)} />);
    await settle(f);
    expect(document.body.textContent ?? '').not.toMatch(/\b(earned|earn|made|income|profitable)\b/i);
  });

  it('without the snapshot, f has no live part and every section still works', async () => {
    const f = mockApi({});
    render(<MathPage engine={Promise.resolve(engine)} />);
    await settle(f);
    expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(6);
    expect(screen.queryAllByRole('heading', { level: 3 })).toHaveLength(0);
    expect(screen.getAllByText(LIVE_MISSING)).toHaveLength(1);
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
  });
});
```

Append inside `describe('theme', …)` in `web/src/theme.test.ts`:

```ts
  it('keeps the public page inside 360 px, on the same fonts, with no inline script or style (spec 7b §6.1, the math CSP)', () => {
    const mathCss: string = readFileSync(`${process.cwd()}/src/math/math.css`, 'utf8');
    const mathHtml: string = readFileSync(`${process.cwd()}/math.html`, 'utf8');
    for (const m of mathCss.matchAll(/(?<![-\w])(min-width|width):\s*(\d+)px/g)) {
      expect(Number(m[2]), `${m[0]}`).toBeLessThanOrEqual(328);
    }
    expect(mathCss).not.toMatch(/minmax\(\s*\d+px/);
    expect(mathHtml).toContain('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap');
    expect(mathHtml).not.toMatch(/<script(?![^>]*type="module")/);
    expect(mathHtml).not.toMatch(/\sstyle=|<style/);
    expect(mathHtml).toContain('<script type="module" src="/src/math/main.tsx"></script>');
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/MathPage.test.tsx src/theme.test.ts`
Expected: FAIL: `Failed to resolve import "./MathPage"`; theme: `ENOENT … src/math/math.css`.

- [ ] **Step 3: Implement**

```tsx
// web/src/math/MathPage.tsx
import { useEffect, useState } from 'react';
import { useNow } from '../lib/hooks';
import type { Engine } from '../wasm/types';
import { ENGINE_FAILED, FOOTER, LIVE_MISSING, LOADING, TAGLINE, TITLE } from './copy';
import { LiveLadder } from './LiveLadder';
import { LiveStrip } from './LiveStrip';
import { ClassifySection } from './sections/ClassifySection';
import { DepthSection } from './sections/DepthSection';
import { EpisodeSection } from './sections/EpisodeSection';
import { LegMissSection } from './sections/LegMissSection';
import { MirrorSection } from './sections/MirrorSection';
import { PayoffSection } from './sections/PayoffSection';
import { freshness } from './snapshot';
import { useSnapshot } from './useSnapshot';

/**
 * 09-27 §6.1 + 7b §6.1: one column, the header, the live strip, sections a–f, and a footer saying the
 * dashboard is private. No router, no api client, no help.ts. The snapshot and the engine are independent:
 * each part says so on its own when it is missing, and the rest of the page never waits for it.
 */
export function MathPage({ engine }: { engine: Promise<Engine> }) {
  const [e, setE] = useState<Engine | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    void engine.then(
      (x) => {
        if (live) setE(x);
      },
      () => {
        if (live) setFailed(true);
      },
    );
    return () => {
      live = false;
    };
  }, [engine]);
  const load = useSnapshot();
  const now = useNow(60_000);
  const snap = load.kind === 'ok' ? load.snap : null;
  const stale = snap !== null && freshness(snap, now) === 'stale';
  return (
    <div className="app math">
      <header className="top m-top">
        <h1>{TITLE}</h1>
      </header>
      <main className="body m-body">
        <p className="m-tag">{TAGLINE}</p>
        {snap !== null && <LiveStrip today={snap.today} writtenMs={snap.written_ms} stale={stale} />}
        {load.kind === 'missing' && <p className="dim">{LIVE_MISSING}</p>}
        {failed && <p className="error" role="alert">{ENGINE_FAILED}</p>}
        {!failed && e === null && <p className="quip" role="status">{LOADING}</p>}
        {e !== null && (
          <>
            <MirrorSection engine={e} />
            <PayoffSection engine={e} />
            <ClassifySection engine={e} />
            <DepthSection engine={e} />
            <EpisodeSection engine={e} />
            <LegMissSection engine={e}>{snap !== null ? <LiveLadder snap={snap} stale={stale} /> : null}</LegMissSection>
          </>
        )}
        <p className="m-foot">{FOOTER}</p>
      </main>
    </div>
  );
}
```

```tsx
// web/src/math/main.tsx
// The public /math entry (09-27 §5.1): no router, no api client, no stream, no help.ts.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../theme.css';
import './math.css';
import { loadEngine } from '../wasm/engine';
import { MathPage } from './MathPage';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <MathPage engine={loadEngine()} />
    </StrictMode>,
  );
}
```

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="description" content="How a basket arbitrage on Polymarket is found, sized and timed, and why it still loses, computed by the scanner's own Rust code in WASM." />
    <title>isoca-radar · math</title>
    <link rel="icon" href="data:," />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/math/main.tsx"></script>
  </body>
</html>
```

(`web/math.html`. No `robots` meta: the page is public and meant to be found, unlike `index.html`.)

```css
/* web/src/math/math.css: spec 7b §6.1 (09-27 §5.1, §6.1), the public page's own layout. Tokens, pixel
   classes and the schematic styles come from theme.css. One column; nothing here widens 360 px. */
.app.math { max-width: 960px; margin: 0 auto; }
.m-top h1 { font-family: var(--px); font-size: 11px; line-height: 1.5; }
.m-body { gap: 14px; }
.m-tag { color: var(--dim); }
.m-live { display: grid; gap: 4px; border: 3px solid var(--line); background: var(--card); padding: 8px 12px; }
.m-live-k { font-family: var(--px); font-size: 8px; color: var(--dim); }
.m-live-row { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 200px), 1fr)); gap: 6px 16px; align-items: center; }
.m-big { font-family: var(--big); font-size: 30px; line-height: 1; }
.m-sec { display: grid; gap: 8px; }
.m-sec h2 { margin-bottom: 0; }
/* no box inside a box (7a lessons): the drawings and folds inside a section drop their frames */
.m-sec .ll-svg { border: 0; }
.m-sec .fold { border: 0; border-top: 1px dashed var(--line2); }
.m-sec .fold > summary { padding: 4px 0; }
.m-controls { display: flex; flex-wrap: wrap; gap: 8px 14px; align-items: center; }
.m-controls .fold { flex-basis: 100%; }
.m-range, .m-check, .m-select, .m-size { display: inline-flex; flex-wrap: wrap; gap: 6px; align-items: center; min-width: 0; }
.m-range input[type='range'] { width: min(180px, 60vw); min-height: 28px; }
.m-size input, .m-select select { font: inherit; background: var(--chip); color: var(--fg); border: 2px solid var(--line); padding: 0 4px; max-width: 12ch; }
.m-level, .m-leg { border: 0; border-top: 1px dashed var(--line2); padding: 6px 0 0; display: flex; flex-wrap: wrap; gap: 6px 12px; min-width: 0; flex: 1 1 100%; }
.m-level legend, .m-leg legend { font-family: var(--px); font-size: 7px; padding: 0 4px 0 0; }
.m-btn { font-family: var(--px); font-size: 7px; padding: 6px 8px; border: 2px solid var(--line); background: var(--chip); cursor: pointer; min-height: 32px; }
.m-btn:disabled { opacity: 0.4; cursor: default; }
.m-cap { font-size: 13px; }
.m-verdict { font-family: var(--big); font-size: 36px; line-height: 1; }
.m-verdict.pos { color: var(--good); }
.m-verdict.neg { color: var(--bad); }
.m-formula { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: baseline; }
.m-liveladder { display: grid; gap: 6px; border-top: 1px dashed var(--line2); padding-top: 8px; }
.m-h3 { font-family: var(--body); font-weight: 600; font-size: 15px; }
.m-foot { color: var(--dim); }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/MathPage.test.tsx src/theme.test.ts`
Expected: PASS (6 + the theme tests).

- [ ] **Step 5: Look at it (dev server, 360 px and desktop)**

Run: `cd web && npx vite --open /math.html` (Vite serves `math.html` as a second page; the snapshot fetch 404s locally, so the page shows `live numbers unavailable`, which is the missing state).
Expected, in Chrome at 1440 px and at 360 px (device toolbar): sections a–f with their drawings; `document.documentElement.scrollWidth <= window.innerWidth` is `true` in the console at 360 px; dragging a slider redraws while dragging; no label is drawn inside an SVG. Stop the server (Ctrl+C).

- [ ] **Step 6: Types, full suite**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 7: Commit**

```bash
git add web/src/math/MathPage.tsx web/src/math/main.tsx web/math.html web/src/math/math.css web/src/math/MathPage.test.tsx web/src/theme.test.ts
git commit -m "web: the public math page: header, live strip or one missing line, sections a-f with f's live ladder, footer; math.html and its layout CSS"
```

---

### Task 13: the math build: `vite.math.config.ts` with the module guard, the bundle check, scripts

**Files:**
- Create: `web/vite.math.config.ts`, `web/scripts/check-math-bundle.mjs`
- Modify: `web/package.json` (`scripts` only)
- Test: `web/src/math/guard.test.ts`, `web/src/math/checkBundle.test.ts`, `web/src/math/build.test.ts`

**Interfaces:**
- Consumes: `math.html` (Task 12); the release package (`sh scripts/build-wasm.sh`).
- Produces:

```ts
// vite.math.config.ts
export const DASHBOARD: RegExp;              // /\/src\/(pages|api|explain)\/|\/src\/help\.ts$/
export function noDashboardCode(): Plugin;   // generateBundle: this.error on a dashboard module id
export default defineConfig(…);              // math.html → dist/math.html + dist/math-assets/, emptyOutDir false
// scripts/check-math-bundle.mjs: exit 1 above 350,000 B gzip, on "/api/" in math JS/CSS/HTML, or without assets; writes <asset>.gz; MATH_DIST overrides dist/
// package.json scripts:
//   "build": "tsc --noEmit && tsc --noEmit -p tsconfig.math.json && vite build && vite build -c vite.math.config.ts && node scripts/check-math-bundle.mjs"
//   "build:dashboard": "tsc --noEmit && vite build"
```

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/math/guard.test.ts
// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { noDashboardCode } from '../../vite.math.config';

type Hook = (this: { error: (m: string) => never }, o: unknown, b: Record<string, unknown>, w: boolean) => void;
const run = (ids: string[]) => {
  const hook = noDashboardCode().generateBundle as unknown as Hook;
  const error = (m: string): never => {
    throw new Error(m);
  };
  hook.call({ error }, {}, { 'a.js': { type: 'chunk', moduleIds: ids }, 'a.css': { type: 'asset' } }, false);
};

describe('noDashboardCode (09-27 §4.4)', () => {
  it('accepts math, wasm, schematics, lib and components, with either path style', () => {
    expect(() =>
      run(['C:\\x\\web\\src\\math\\MathPage.tsx', '/x/web/src/schematics/LatencyLadder.tsx', '/x/web/src/wasm/pkg/arb_wasm_bg.wasm?url', '/x/web/src/lib/money.ts', '/x/web/src/components/Fold.tsx']),
    ).not.toThrow();
  });

  it('rejects pages, api, explain and help.ts (review focus 5)', () => {
    for (const bad of ['/x/web/src/pages/Radar.tsx', 'C:\\x\\web\\src\\api\\client.ts', '/x/web/src/explain/models.ts', '/x/web/src/help.ts']) {
      expect(() => run([bad]), bad).toThrow(/dashboard module/);
    }
  });
});
```

```ts
// web/src/math/checkBundle.test.ts
// @vitest-environment node
// @ts-ignore: node built-ins exist at runtime under vitest (no @types/node installed)
import { execFileSync } from 'node:child_process';
// @ts-ignore
import { randomBytes } from 'node:crypto';
// @ts-ignore
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
// @ts-ignore
import { tmpdir } from 'node:os';
import { describe, expect, it } from 'vitest';

declare const process: { execPath: string; env: Record<string, string | undefined> };

function dist(files: Record<string, string | Uint8Array>): string {
  const d: string = mkdtempSync(`${tmpdir()}/math-dist-`);
  mkdirSync(`${d}/math-assets`);
  writeFileSync(`${d}/math.html`, '<script type="module" src="/math-assets/m.js"></script>');
  for (const [name, body] of Object.entries(files)) writeFileSync(`${d}/math-assets/${name}`, body);
  return d;
}

function check(d: string): { ok: boolean; out: string } {
  try {
    const out: string = execFileSync(process.execPath, ['scripts/check-math-bundle.mjs'], { env: { ...process.env, MATH_DIST: d }, encoding: 'utf8', stdio: 'pipe' });
    return { ok: true, out };
  } catch (e) {
    const x = e as { stdout?: string; stderr?: string };
    return { ok: false, out: `${x.stdout ?? ''}${x.stderr ?? ''}` };
  }
}

describe('check-math-bundle.mjs (09-27 §4.4)', () => {
  it('passes a small bundle and writes a .gz beside each asset', () => {
    const d = dist({ 'm.js': 'console.log("math")', 'm.css': 'body{}' });
    const r = check(d);
    expect(r.ok, r.out).toBe(true);
    expect(r.out).toMatch(/math-assets total: \d+ B gzip \(limit 350000\)/);
    expect(existsSync(`${d}/math-assets/m.js.gz`)).toBe(true);
  });

  it('fails on "/api/" in an asset (review focus 5)', () => {
    const r = check(dist({ 'm.js': 'fetch("/api/today")' }));
    expect(r.ok).toBe(false);
    expect(r.out).toContain('"/api/" found in m.js');
  });

  it('fails over 350 KB gzip', () => {
    const r = check(dist({ 'm.js': 'x', 'm.wasm': randomBytes(400_000) }));
    expect(r.ok).toBe(false);
    expect(r.out).toContain('over the 350 KB gzip budget');
  });
});
```

```ts
// web/src/math/build.test.ts
// @vitest-environment node
import { build } from 'vite';
import { describe, expect, it } from 'vitest';
import { DASHBOARD } from '../../vite.math.config';
import { HAVE_PKG, WASM_REQUIRED, skipReason } from '../wasm/testEngine';

if (!HAVE_PKG && !WASM_REQUIRED) console.info(skipReason('math build'));

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('the math build (spec 7b §7.2, §8)', () => {
  it('bundles the reused schematics and nothing from the dashboard (review focus 5)', async () => {
    const out: unknown = await build({ configFile: 'vite.math.config.ts', mode: 'production', logLevel: 'silent', build: { write: false } });
    const outputs = (Array.isArray(out) ? out : [out]) as { output: { type: string; moduleIds?: string[] }[] }[];
    const ids = outputs
      .flatMap((o) => o.output.flatMap((c) => (c.type === 'chunk' ? (c.moduleIds ?? []) : [])))
      .map((id) => id.replace(/\\/g, '/').split('?')[0]);
    for (const f of ['src/schematics/LatencyLadder.tsx', 'src/schematics/PayoffGrid.tsx', 'src/schematics/DurationBars.tsx', 'src/components/SweetLadderGrid.tsx', 'src/math/MathPage.tsx', 'src/wasm/engine.ts']) {
      expect(ids.some((id) => id.endsWith(f)), f).toBe(true);
    }
    expect(ids.filter((id) => DASHBOARD.test(id))).toEqual([]);
  }, 120_000);
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/guard.test.ts src/math/checkBundle.test.ts src/math/build.test.ts`
Expected: FAIL: `Failed to resolve import "../../vite.math.config"`; checkBundle: `Cannot find module …/scripts/check-math-bundle.mjs` (every case `ok: false`).

- [ ] **Step 3: Implement**

```ts
// web/vite.math.config.ts
// 09-27 §5.1: the public /math page, a second build into dist/ (math.html + math-assets/) after the
// dashboard's; no chunk is shared with it. §4.4: the build fails if a dashboard module gets in.
import react from '@vitejs/plugin-react';
import { type Plugin, defineConfig } from 'vite';

/** What the public bundle must never hold (type-only imports are erased and never show up here). */
export const DASHBOARD = /\/src\/(pages|api|explain)\/|\/src\/help\.ts$/;

export function noDashboardCode(): Plugin {
  return {
    name: 'no-dashboard-code',
    generateBundle(_options, bundle) {
      for (const out of Object.values(bundle)) {
        if (out.type !== 'chunk') continue;
        for (const id of out.moduleIds) {
          const p = id.replace(/\\/g, '/').split('?')[0];
          if (DASHBOARD.test(p)) this.error(`math bundle includes dashboard module ${p}`);
        }
      }
    },
  };
}

export default defineConfig({
  plugins: [react(), noDashboardCode()],
  build: {
    outDir: 'dist',
    emptyOutDir: false,
    assetsDir: 'math-assets',
    // No inline scripts anywhere (the math CSP): no modulepreload polyfill.
    modulePreload: { polyfill: false },
    sourcemap: false,
    rolldownOptions: { input: 'math.html' },
  },
});
```

```js
// web/scripts/check-math-bundle.mjs
// 09-27 §4.4 + §5.2, 7b §11: the public page's assets stay at or under 350 KB gzip (what users receive),
// no "/api/" string ships in its JS, CSS or HTML, and every asset gets a .gz beside it for nginx
// gzip_static. MATH_DIST overrides the dist directory (checkBundle.test.ts).
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const raw = process.env.MATH_DIST ?? fileURLToPath(new URL('../dist/', import.meta.url));
const DIST = /[\\/]$/.test(raw) ? raw : `${raw}/`;
const ASSETS = `${DIST}math-assets/`;
const LIMIT = 350_000;
const TEXT = /\.(js|css|html)$/;

function fail(msg) {
  console.error(`check-math-bundle: ${msg}`);
  process.exit(1);
}

let total = 0;
const bad = [];
for (const name of readdirSync(ASSETS).sort()) {
  const path = ASSETS + name;
  if (name.endsWith('.gz') || !statSync(path).isFile()) continue;
  const bytes = readFileSync(path);
  const gz = gzipSync(bytes, { level: 9 });
  writeFileSync(`${path}.gz`, gz);
  total += gz.length;
  if (TEXT.test(name) && bytes.toString('utf8').includes('/api/')) bad.push(name);
  console.log(`${name}: ${gz.length} B gzip`);
}
const html = readFileSync(`${DIST}math.html`, 'utf8');
if (html.includes('/api/')) bad.push('math.html');
console.log(`math-assets total: ${total} B gzip (limit ${LIMIT})`);
if (total === 0) fail('no math assets found');
if (!html.includes('/math-assets/')) fail('math.html does not load from /math-assets/');
if (bad.length > 0) fail(`"/api/" found in ${bad.join(', ')}`);
if (total > LIMIT) fail('over the 350 KB gzip budget');
```

`web/package.json` `scripts` become (nothing else in the file changes):

```json
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && tsc --noEmit -p tsconfig.math.json && vite build && vite build -c vite.math.config.ts && node scripts/check-math-bundle.mjs",
    "build:dashboard": "tsc --noEmit && vite build",
    "test": "vitest run"
  },
```

- [ ] **Step 4: Run to verify pass**

Run: `cd web && WASM_REQUIRED=1 npx vitest run src/math/guard.test.ts src/math/checkBundle.test.ts src/math/build.test.ts`
Expected: PASS (2 + 3 + 1 tests).

- [ ] **Step 5: The full build on the release package (owner's OK first)**

A release build of `arb-wasm` on the PC is approved by the lead but still **pending the owner's OK** (it breaks the earlier "debug builds only on the PC" rule, for one crate). Ask before running this step; without the OK, skip it and leave the size check to CI and the Docker build.

Run: `export PATH="$HOME/.cargo/bin:$PATH" && sh scripts/build-wasm.sh >/dev/null && cd web && npm run build 2>&1 | tail -8 && ls dist/math.html dist/math-assets && (grep -l 'EventSource\|/api/' dist/math-assets/*.js || echo clean)`
Expected: the per-file gzip lines and `math-assets total: <n> B gzip (limit 350000)` with n ≤ 350000; `dist/math.html`; `dist/math-assets/` with one `.js`, one `.css`, one `.wasm` and a `.gz` beside each; `clean`.
If not: `over the 350 KB gzip budget` = **STOP** and report the per-file sizes (09-27 §4.4: whether `wasm-opt` is needed is the owner's call); a `math bundle includes dashboard module …` error names the import to fix (Task 1's walker should have caught it first).

- [ ] **Step 6: Put the PC back on the test package, full suite, types**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && PROFILE=dev sh scripts/build-wasm.sh >/dev/null && cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 7: Commit**

```bash
git add web/vite.math.config.ts web/scripts/check-math-bundle.mjs web/package.json web/src/math/guard.test.ts web/src/math/checkBundle.test.ts web/src/math/build.test.ts
git commit -m "web: second Vite build for /math with the no-dashboard-code guard, 350 KB gzip and no-/api/ check writing .gz files, build runs both"
```

---

### Task 14: deploy files: the web image's wasm stage, nginx locations, the web mount and the `ct-math` router

**Files:**
- Modify: `docker/web.Dockerfile` (new stages before the node stage; node stage copies `pkg`), `docker/web/nginx.conf` (three locations before `location /`), `compose.web.yml` (`web`: `volumes` after `tmpfs`, labels after the `ct-web` lines)
- Test: `web/src/deploy.test.ts` (three new `describe` blocks)

**Interfaces:**
- Consumes: `scripts/build-wasm.sh` (Task 4); `dist/math.html`, `dist/math-assets/*` and their `.gz` (Task 13); the `public-data` volume and the `ct-sec` middleware (D1; `oauth2-proxy` labels).
- Produces: the image `cute-trading/web:dev` serving `/math`, `/math-assets/`, `/math-data/public.json`; Traefik labels exactly as in Global Constraints.

- [ ] **Step 1: Write the failing tests** — append to `web/src/deploy.test.ts`:

```ts
describe('web image (09-27 §4.2)', () => {
  it('builds the engine on the rust image rust.Dockerfile pins, with cargo-chef, and hands pkg to the node stage', () => {
    const web = repo('docker/web.Dockerfile');
    const pin = /FROM (rust:\S+@sha256:[0-9a-f]{64})/.exec(repo('docker/rust.Dockerfile'))?.[1];
    expect(pin).toBeDefined();
    expect(web).toContain(`FROM ${pin} AS wasm-chef`);
    expect(web).toContain('cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json --package arb-wasm');
    expect(web).toContain('RUN sh scripts/build-wasm.sh');
    expect(web).toContain('COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg');
    expect(web.indexOf('COPY --from=wasm ')).toBeLessThan(web.indexOf('RUN npm run build'));
    expect(repo('.dockerignore')).toMatch(/^web\/src\/wasm\/pkg$/m);
  });
});

describe('nginx (09-27 §5.2, 7b §5.3)', () => {
  it('serves /math, /math-assets/ from .gz files, and from the volume only the exact public.json', () => {
    const n = repo('docker/web/nginx.conf');
    expect(n).toMatch(/location = \/math \{[^}]*add_header Cache-Control "no-cache";[^}]*try_files \/math\.html =404;/);
    expect(n).toMatch(/location \/math-assets\/ \{[^}]*immutable[^}]*gzip_static on;[^}]*try_files \$uri =404;/);
    expect(n).toMatch(/location = \/math-data\/public\.json \{[^}]*root \/srv\/ct-public-root;[^}]*max-age=60[^}]*gzip on;[^}]*gzip_types application\/json;[^}]*try_files \$uri =404;/);
    expect(n).not.toMatch(/location \/math-data/);
    expect(n.indexOf('location = /math-data/public.json')).toBeLessThan(n.indexOf('location / {'));
  });
});

describe('compose.web.yml (7b §5.3)', () => {
  const web = (): string => {
    const y = repo('compose.web.yml');
    return y.slice(y.indexOf('\n  web:\n'), y.indexOf('\n  oauth2-proxy:\n'));
  };

  it('mounts the snapshot volume read-only into web (7b-A mounts it into the api first)', () => {
    expect(web()).toContain('- public-data:/srv/ct-public-root/math-data:ro');
    expect(repo('compose.web.yml')).toMatch(/^volumes:\n(?: {2}\S.*\n)*? {2}public-data:/m);
  });

  it('routes only the three public paths to web without login, with the per-IP limits and the math CSP', () => {
    const w = web();
    const lines = [
      'routers.ct-math.rule=Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`) || Path(`/math-data/public.json`))',
      'routers.ct-math.entrypoints=websecure',
      'routers.ct-math.tls.certresolver=letsencrypt',
      'routers.ct-math.service=ct-web',
      'routers.ct-math.priority=1000',
      'routers.ct-math.middlewares=ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker',
      'middlewares.ct-math-rl.ratelimit.average=5',
      'middlewares.ct-math-rl.ratelimit.burst=20',
      'middlewares.ct-math-rl.ratelimit.sourcecriterion.ipstrategy.depth=0',
      'middlewares.ct-math-if.inflightreq.amount=4',
      'middlewares.ct-math-if.inflightreq.sourcecriterion.ipstrategy.depth=0',
      "middlewares.ct-math-csp.headers.contentSecurityPolicy=default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ];
    for (const l of lines) expect(w, l).toContain(`traefik.http.${l}`);
    expect(w).not.toMatch(/ct-math\.middlewares=[^\n]*ct-auth/);
    expect(w).toContain('traefik.http.routers.ct-web.middlewares=ct-rl@docker,ct-sec@docker,ct-csp@docker,ct-auth@docker');
    expect(w.match(/ct-math/g)).toHaveLength(15);
  });
});
```

(The single-quoted strings keep `${RADAR_DOMAIN}` and the backticks literal. The 15 `ct-math` occurrences: 12 label lines, the rule line has none extra, the middlewares line names `ct-math-rl`, `ct-math-if`, `ct-math-csp`; the comment block above the labels must not say `ct-math`.)

- [ ] **Step 2: Run to verify failure**

Run: `cd web && npx vitest run src/deploy.test.ts`
Expected: FAIL: `expected … to contain 'FROM rust:1.98.1-bookworm@sha256:… AS wasm-chef'`, the nginx regexes, the web mount and every `ct-math` line; the `ci` test still passes.

- [ ] **Step 3: Implement the Dockerfile** — replace everything in `docker/web.Dockerfile` above `# Unprivileged nginx` with:

```dockerfile
# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose -f compose.yml -f compose.web.yml --profile web build web
# Phase 7b (09-27 §4.2): arb-wasm first, on the same pinned Rust image as rust.Dockerfile, with cargo-chef so
# a web-only change reuses the cooked dependencies; then the node stage builds the dashboard and /math.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS wasm-chef
RUN rustup target add wasm32-unknown-unknown && cargo install cargo-chef --locked
WORKDIR /src
COPY Cargo.lock ./
# the CLI must equal the wasm-bindgen crate that Cargo.lock pins (7b-A)
RUN WBG=$(awk '/^name = "wasm-bindgen"/ { getline; gsub(/[^0-9.]/, ""); print; exit }' Cargo.lock) && test -n "$WBG" && cargo install wasm-bindgen-cli --locked --version "$WBG"

FROM wasm-chef AS wasm-planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM wasm-chef AS wasm
# the size-optimized release profile scripts/build-wasm.sh uses, so the cook and the build share artifacts
ENV CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
COPY --from=wasm-planner /src/recipe.json recipe.json
RUN cargo chef cook --release --target wasm32-unknown-unknown --recipe-path recipe.json --package arb-wasm
COPY . .
RUN sh scripts/build-wasm.sh

FROM node:22-alpine@sha256:0a7108bf6c7bf5de370ffb1a3ed6be93d405b43ff159f681a8d18c0e2bc2e402 AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
COPY --from=wasm /src/web/src/wasm/pkg ./src/wasm/pkg
RUN npm run build

```

The nginx stage below stays as it is (`COPY --from=build /web/dist /usr/share/nginx/html` now also carries `math.html`, `math-assets/` and the `.gz` files).

- [ ] **Step 4: Implement nginx** — in `docker/web/nginx.conf`, insert before `    location / {`:

```nginx
    # Phase 7b (09-27 §5.2, 7b §5.3): the public playground. Traefik's public router sends only these three
    # paths here without login; every other path (/math/, /math.html, /math-data/…) is ct-web's, behind login.
    location = /math {
        add_header Cache-Control "no-cache";
        try_files /math.html =404;
    }

    location /math-assets/ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        # check-math-bundle.mjs writes a .gz beside each asset: no gzip work per request
        gzip_static on;
        try_files $uri =404;
    }

    # 7b §5.3: the snapshot the api rewrites every 5 minutes; only this exact file comes from the volume,
    # which is mounted read-only at /srv/ct-public-root/math-data (public.json.tmp is never served).
    location = /math-data/public.json {
        root /srv/ct-public-root;
        add_header Cache-Control "public, max-age=60";
        gzip on;
        gzip_types application/json;
        try_files $uri =404;
    }

```

- [ ] **Step 5: Implement compose** — in `compose.web.yml`, in the `web` service, after `    tmpfs: [/tmp]` add:

```yaml
    # Phase 7b §5.3: the api's snapshot volume, read-only (nginx serves only public.json from it). The api
    # mounted it first (7b-A), so the volume root keeps the image's 65532 owner.
    volumes:
      - public-data:/srv/ct-public-root/math-data:ro
```

and after `      - traefik.http.services.ct-web.loadbalancer.server.port=8080` add:

```yaml
      # Phase 7b (09-27 §5.3, 7b §5.3): the public playground. Static files only, no ct-auth, per-IP rate and
      # in-flight limits (depth=0: DNS is a plain A record, no CDN), its own CSP with 'wasm-unsafe-eval'.
      # Any path it does not match still falls to ct-web (login).
      - "traefik.http.routers.ct-math.rule=Host(`${RADAR_DOMAIN}`) && (Path(`/math`) || PathPrefix(`/math-assets/`) || Path(`/math-data/public.json`))"
      - traefik.http.routers.ct-math.entrypoints=websecure
      - traefik.http.routers.ct-math.tls.certresolver=letsencrypt
      - traefik.http.routers.ct-math.service=ct-web
      - traefik.http.routers.ct-math.priority=1000
      - traefik.http.routers.ct-math.middlewares=ct-math-rl@docker,ct-math-if@docker,ct-sec@docker,ct-math-csp@docker
      - traefik.http.middlewares.ct-math-rl.ratelimit.average=5
      - traefik.http.middlewares.ct-math-rl.ratelimit.burst=20
      - traefik.http.middlewares.ct-math-rl.ratelimit.sourcecriterion.ipstrategy.depth=0
      - traefik.http.middlewares.ct-math-if.inflightreq.amount=4
      - traefik.http.middlewares.ct-math-if.inflightreq.sourcecriterion.ipstrategy.depth=0
      - "traefik.http.middlewares.ct-math-csp.headers.contentSecurityPolicy=default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
```

(The top-level `volumes: public-data:` is 7b-A's, D1. If `grep -n '^volumes:' -A3 compose.web.yml` shows no `public-data:` there, **STOP**: 7b-A is not merged; do not declare it here.)

- [ ] **Step 6: Run to verify pass, and compose parses**

Run: `cd web && npx vitest run src/deploy.test.ts && cd .. && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok`
Expected: PASS (5 tests), then `config-ok`.
If `expected … to have a length of 15` fails: count `grep -o 'ct-math' compose.web.yml | wc -l` and remove `ct-math` from any comment you added.

- [ ] **Step 7: Full suite, types**

Run: `cd web && WASM_REQUIRED=1 npm test && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json`
Expected: all green, no tsc output.

- [ ] **Step 8: Commit**

```bash
git add docker/web.Dockerfile docker/web/nginx.conf compose.web.yml web/src/deploy.test.ts
git commit -m "deploy: web image builds arb-wasm (cargo-chef, pinned rust image), nginx serves /math, /math-assets/ and public.json from the read-only volume, ct-math router with per-IP limits and its CSP"
```

---

### Task 15: runbook for 7b-B (web, the web mount and the router, at a quiet minute)

**Files:**
- Create: `docs/superpowers/runbooks/phase7b-web-vps.md`

**Interfaces:**
- Consumes: 7b-A deployed (the api writing `public.json` into `cute-trading_public-data`, D1–D2); the patterns of `docs/superpowers/runbooks/phase7a-web-vps.md` (steps 1–11, 19: the image ship, the rollback tag from the running container's image ID, the quiet minute, the websocket check).
- Produces: the owner's runbook; `web` recreated alone with the mount and labels; the no-cookie curl checks; the three floods; the Chrome check; a rollback to `web:pre7b-B` plus the saved `compose.web.yml`.

- [ ] **Step 1: Write the runbook** — create `docs/superpowers/runbooks/phase7b-web-vps.md` with exactly:

````markdown
# Phase 7b-B on the VPS: the public /math page (web image, web mount, ct-math router)

Note: one image change, `cute-trading/web:dev` (the /math page and its WASM engine), plus `compose.web.yml` (the `web` mount of `public-data`, read-only, and the `ct-math` router). The api, scanner, analyze, compact and postgres are not touched.
Note: 7b-A must be live first: the api mounts `cute-trading_public-data` and writes `public.json` every 5 minutes. `web` must never be the first container to mount that volume (a new volume takes its owner from the first container that mounts it); step 8 stops if the volume or the file is missing.
Note: rollback tag `cute-trading/web:pre7b-B` (`web:pre7b` is 7a-B's and stays untouched), tagged from the running container's image ID; `compose.web.yml.pre7b-B` is the compose file to restore with it.
Note: the quiet minute (step 12) is advisory: at most 15 minutes of waiting, then recreate anyway and count the reconnects (2026-09-30 showed 0).
Note: the flood tests (steps 18–20) use one source IP: they show the per-IP limits and web's 32 MB cap, not a distributed flood or Traefik's TLS cost (09-27 §5.4).
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker.
Note: PC steps in Git Bash from the main checkout on the merged commit; VPS steps as root over SSH (`ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`); browser steps in Chrome.

## On the PC

1. The repo on the commit being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s'
```
→ expected: no status lines, `main`, then `<sha> <subject>` of the merged 7b-B commit.
if not: commit or stash the listed changes, then run this step again.

2. Release engine, web tests with the engine required, both typechecks, both builds and the size check. PENDING OWNER OK: this is a release build of `arb-wasm` on the PC (lead-approved; the owner has not yet approved it). Without the OK, skip it: step 5's Docker build runs the same size check.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && export PATH="$HOME/.cargo/bin:$PATH" && sh scripts/build-wasm.sh >/dev/null && echo wasm-ok && cd web && npm ci --no-audit --no-fund >/dev/null && ( set -o pipefail; WASM_REQUIRED=1 npm test 2>&1 | grep -E '^ +(Test Files|Tests) ' ) && echo tests-ok && ( set -o pipefail; npm run build 2>&1 | grep -E 'math-assets total|check-math-bundle|error' ) && ls dist/math.html && echo build-ok
```
→ expected: `wasm-ok`, `Test Files  <n> passed (<n>)`, `Tests  <n> passed (<n>)`, `tests-ok`, `math-assets total: <n> B gzip (limit 350000)` with n ≤ 350000, `dist/math.html`, `build-ok`.
if not: `build-wasm: the wasm-bindgen CLI is …` = run the `cargo install …` it prints, then this step again; `check-math-bundle: over the 350 KB gzip budget` = **STOP**, report the per-file sizes (`npm run build 2>&1 | grep 'B gzip'`); any failed test or tsc error: fix on a branch, do not build the image.
**STOP**

3. Compose parses, with the router and the mount.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'ct-math' compose.web.yml && grep -c 'public-data:/srv/ct-public-root/math-data:ro' compose.web.yml && grep -c 'public-data:/var/lib/ct-public' compose.web.yml
```
→ expected: `config-ok`, `12`, `1`, `1`.
if not: the last `0` = 7b-A is not in this checkout: **STOP**.

4. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `docker builder prune -f --keep-storage 5GB`, then run this step again.

5. Build the web image only (the first build compiles the wasm dependencies; later web-only builds reuse them).
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | tail -3
```
→ expected: the build's last lines without `ERROR`.
if not: read the first `ERROR` line (`over the 350 KB gzip budget` = as step 2), fix, run again.

6. The image carries the math page, one wasm, the .gz files, no `/api/` in the math bundle, and a valid nginx config.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint sh cute-trading/web:dev -c 'cd /usr/share/nginx/html && ls math.html && ls math-assets | grep -c "\.wasm$" && ls math-assets | grep -c "\.gz$" && grep -l "/api/" math-assets/*.js math.html | wc -l && grep -l "math-data/public.json" math-assets/*.js | wc -l && nginx -t 2>&1 | tail -1'
```
→ expected: `math.html`, `1`, `3` (or more), `0`, `1`, `nginx: configuration file /etc/nginx/nginx.conf test is successful`.
if not: `grep -l "/api/"` above `0` = **STOP** (the public bundle must not name the api); nginx test failed = read its line, fix `docker/web/nginx.conf`.
**STOP**

7. Bundle the image and `compose.web.yml`, and ship them.
```bash
R=/c/Users/Isabelly/PycharmProjects/cute-trading && mkdir -p "$HOME/ct-7bB" && cd "$HOME/ct-7bB" && docker save cute-trading/web:dev | gzip -1 > image.tar.gz && cp "$R/compose.web.yml" . && tar -cf "$HOME/ct-7bB.tar" image.tar.gz compose.web.yml && cd "$HOME" && rm -rf "$HOME/ct-7bB" && ls -l "$HOME/ct-7bB.tar" && scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7bB.tar" root@31.97.23.123:/root/ && rm "$HOME/ct-7bB.tar"
```
→ expected: one `ls -l` line (roughly 15–45 MB), the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

8. 7b-A is live: the volume exists, the api mounts it, the file is fresh and readable by anyone.
```bash
cd /home/isoca/projects/cute-trading && docker volume inspect -f '{{.Name}}' cute-trading_public-data && docker inspect -f '{{range .Mounts}}{{.Destination}} {{.RW}}{{"\n"}}{{end}}' cute-trading-api-1 | grep ct-public && docker run --rm -v cute-trading_public-data:/v:ro alpine sh -c 'stat -c "%u %a" /v; stat -c "%u %a" /v/public.json; echo "age $(( $(date +%s) - $(stat -c %Y /v/public.json) )) s"; head -c 20 /v/public.json; echo'
```
→ expected: `cute-trading_public-data`; `/var/lib/ct-public true`; `65532 755`; `65532 644`; `age <n> s` with n ≤ 600; `{"v":1,"written_ms":`.
if not: no volume, no api mount or no file = 7b-A is not deployed: **STOP** (never let `web` create the volume); an owner other than 65532 or modes other than 755/644 = nginx (uid 101) may not read it: **STOP**, report (a 7b-A fix); age above 600 s = the writer is behind: `docker compose -f compose.yml -f compose.web.yml --profile web logs --since 15m api 2>&1 | grep 'public snapshot'`, write it in Results and go on (the page says "behind").
**STOP**

9. Rollback tag `web:pre7b-B` from the running web's image ID, and the compose file saved, each only once.
```bash
cd /home/isoca/projects/cute-trading && { docker image inspect cute-trading/web:pre7b-B >/dev/null 2>&1 || docker tag "$(docker inspect -f '{{.Image}}' cute-trading-web-1)" cute-trading/web:pre7b-B; } && { [ -f compose.web.yml.pre7b-B ] || cp compose.web.yml compose.web.yml.pre7b-B; } && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B && docker inspect -f '{{.Image}}' cute-trading-web-1 && grep -c 'ct-math' compose.web.yml.pre7b-B
```
→ expected: two identical `sha256:…` lines, then `0`.
if not (a re-run after step 13): the second line is the new web, so a different line is expected: leave `pre7b-B` as it is, and check the last line is still `0`.
**STOP**

10. Load the bundle (replaces `compose.web.yml`; moves `web:dev`; `web:pre7b-B`, `web:pre7b` and `web:pre7a` stay).
```bash
cd /home/isoca/projects/cute-trading && tar --no-same-owner -xf /root/ct-7bB.tar && rm /root/ct-7bB.tar && gunzip -c image.tar.gz | docker load && rm image.tar.gz && docker image ls 'cute-trading/web' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/web:dev`; `web:dev` on a new ID, `web:pre7b-B` on the old one; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: truncated tar = re-ship (step 7); `OVER BUDGET` = `docker image prune -f`, then `LIMIT_GB=75 sh scripts/disk-budget.sh` again.

11. Compose parses with the new web.
```bash
cd /home/isoca/projects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok && grep -c 'ct-math' compose.web.yml && grep -c 'public-data:/srv/ct-public-root/math-data:ro' compose.web.yml && grep -c 'public-data:/var/lib/ct-public' compose.web.yml
```
→ expected: `config-ok`, `12`, `1`, `1`.
if not: the bundle's compose file is old: re-bundle (step 7).
**STOP**

12. A quiet minute (advisory): the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`; run step 13 within 2 minutes.
if not: `f` = a busy minute: run this step again every few minutes; after 15 minutes without `t`, go on to step 13 anyway and write "no quiet minute in 15 min" in Results; an empty line = no health rows: **STOP**, report.

13. Baseline (scanner start, memory), then recreate `web` only.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7bB_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7bB_scanner_started && docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' cute-trading-web-1 cute-trading-api-1 cute-trading-scanner-1 cute-trading-postgres-1 && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker compose -f compose.yml -f compose.web.yml --profile web ps web --format '{{.Service}} {{.Status}}'
```
→ expected: the scanner's start time, four `<name> <used> / <limit>` lines (write them in Results), `Container cute-trading-web-1 …` lines only (no api, no oauth2-proxy, no volume created), then `web Up …`.
if not: `Volume "cute-trading_public-data" Created` = step 8 was skipped: roll back now (step 23) and report; `web` not `Up` = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then the rollback (step 23).

14. The web reads the volume as uid 101 and cannot write it; no websocket disconnect.
```bash
cd /home/isoca/projects/cute-trading && docker exec cute-trading-web-1 sh -c 'id -u; ls -ln /srv/ct-public-root/math-data; head -c 20 /srv/ct-public-root/math-data/public.json; echo; touch /srv/ct-public-root/math-data/x 2>&1 | tail -1'; sleep 180; T0=$(cat /root/7bB_web_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7bB_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `101`; a listing with `-rw-r--r-- … 65532 65532 … public.json`; `{"v":1,"written_ms":`; `touch: …: Read-only file system`; `scanner-not-restarted`; `0`.
if not: `Permission denied` on `head` = roll back (step 23) and report; reconnects above 0: do NOT roll back; write the count and the UTC time in Results.

## Checks (PC, no cookie)

15. The public page and its assets answer without login, with the math CSP, the caching and the wasm type.
```bash
D=https://radar.isoca.space; JS=$(curl -s "$D/math" | grep -o '/math-assets/[^"]*\.js' | head -1); WASM=$(curl -s --compressed "$D$JS" | grep -o '/math-assets/[^"]*\.wasm' | head -1); echo "$JS $WASM"; curl -sI "$D/math" | grep -iE '^(HTTP|content-type|cache-control|content-security-policy)'; curl -sI "$D$JS" | grep -iE '^(HTTP|cache-control)'; curl -sI -H 'Accept-Encoding: gzip' "$D$WASM" | grep -iE '^(HTTP|content-type|content-encoding)'
```
→ expected: `/math-assets/<name>.js /math-assets/<name>.wasm`; `/math`: `HTTP/2 200`, `content-type: text/html`, `cache-control: no-cache`, `content-security-policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; …`; the JS: `HTTP/2 200`, `cache-control: public, max-age=31536000, immutable`; the wasm: `HTTP/2 200`, `content-type: application/wasm`, `content-encoding: gzip`.
if not: `/math` answers `302` = the router is not picked up: on the VPS `docker inspect -f '{{json .Config.Labels}}' cute-trading-web-1 | grep -o 'ct-math[^,]*' | head -3`, report; the wasm without `application/wasm` = report (nginx mime types).
**STOP**

16. The snapshot answers without login, gzipped, short-cached, version 1, fresh, and with nothing that must not ship.
```bash
D=https://radar.isoca.space; curl -sI -H 'Accept-Encoding: gzip' "$D/math-data/public.json" | grep -iE '^(HTTP|content-type|content-encoding|cache-control)'; curl -s --compressed "$D/math-data/public.json" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);console.log("v",j.v,"age_min",Math.round((Date.now()-j.written_ms)/60000),"keys",Object.keys(j).join(","),"leak",/account_id|"host"/.test(s))})'
```
→ expected: `HTTP/2 200`, `content-type: application/json`, `content-encoding: gzip`, `cache-control: public, max-age=60`; then `v 1 age_min <0–10> keys v,written_ms,ladder,today leak false`.
if not: `404` = the mount or the file (step 14); `leak true` = on the VPS roll back (step 23) and report (a 7b-A whitelist bug).
**STOP**

17. Every other path still needs login, the traversal too, and plain HTTP redirects.
```bash
D=https://radar.isoca.space; for p in /math/ /math.html /math-data/ /math-data/x.json /math-data/public.json.tmp /api/today / /sweet; do curl -sS -o /dev/null -w "$p %{http_code}\n" "$D$p"; done; curl -sS --path-as-is -o /dev/null -w "/math-data/../public.json %{http_code}\n" "$D/math-data/../public.json"; curl -sS -o /dev/null -w "http /math %{http_code} %{redirect_url}\n" "http://radar.isoca.space/math"
```
→ expected: each path `302` (`/api/today` may be `401`); `/math-data/../public.json 302` or `400`; `http /math 301 https://radar.isoca.space/math` (or `308`).
if not: any `200` = a path is public that must not be: on the VPS roll back (step 23), report.
**STOP**

18. Flood the wasm (09-27 §5.5). First, in a VPS terminal:
```bash
cd /home/isoca/projects/cute-trading && for i in 1 2 3 4 5 6 7 8; do docker stats --no-stream --format '{{.Name}} {{.MemUsage}} {{.CPUPerc}}' cute-trading-web-1 cute-trading-api-1 cute-trading-scanner-1 cute-trading-postgres-1; echo; sleep 10; done
```
Then, within 10 s, on the PC:
```bash
D=https://radar.isoca.space; JS=$(curl -s "$D/math" | grep -o '/math-assets/[^"]*\.js' | head -1); U="$D$(curl -s --compressed "$D$JS" | grep -o '/math-assets/[^"]*\.wasm' | head -1)"; echo "$U"; if command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi
```
→ expected: on the PC mostly `429`, some `200`; on the VPS `web` at or under 32 MiB in every round, `api`, `scanner` and `postgres` within a few MiB of step 13's baseline; while it runs, the dashboard (Chrome, logged in) still loads `/`.
if not: `web` above 32 MiB, or `docker inspect -f '{{.State.OOMKilled}}' cute-trading-web-1` is `true` = roll back (step 23) and report (09-27 §5.4's fallback, a separate nginx for math, is the owner's call).

19. Flood the snapshot (7b §9): the same two commands as step 18 (VPS first), the PC one with this URL instead:
```bash
U=https://radar.isoca.space/math-data/public.json; if command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi
```
→ expected: as step 18; the api's CPU and memory do not move (the file never reaches it).
if not: as step 18.

20. Flood `/math` while a phone on mobile data (another IP) loads it (09-27 §5.5: the in-flight limit is per IP). The VPS command of step 18 first, then:
```bash
U=https://radar.isoca.space/math; if command -v oha >/dev/null; then oha -z 60s -c 50 --no-tui "$U" | grep -A8 'Status code distribution'; else seq 1 3000 | xargs -P 50 -I{} curl -s -o /dev/null -w '%{http_code}\n' "$U" | sort | uniq -c; fi
```
→ expected: as step 18; during the run the phone loads `https://radar.isoca.space/math` with its drawings.
if not: the phone gets `429` = the limiter is not per IP: report (the `sourcecriterion` labels).
**STOP**

21. (Chrome, window 1440 px wide, not logged in: a private window) open `https://radar.isoca.space/math` with DevTools on Network and Console.
→ expected: the header `isoca-radar · math`; the live strip (NO and YES counts in big digits, `$… edge on the table`, 24 hour bars, `as of HH:MM UTC`); sections a–f in order, each with its big answer, one drawing, controls, a reset, a deadpan line and a caption naming the Rust function; f's `the usual burst` turns its big line into `caught, and lost $…`; f ends with `on the live scanner, as of HH:MM UTC` and the ladder with its `which markets` fold; the footer `The live dashboard is private. The math is not.`; Network: `math`, one `.js`, one `.css`, one `.wasm` (`application/wasm`), exactly one `public.json`, no `/api/` request, no `429`; Console: no CSP error; every label sits in HTML beside a drawing, none stretched.
if not: a CSP error = report the directive; a `429` on load = report (in-flight limit 4); a stretched label = screenshot and report (a web fix; no rollback needed).

22. (Chrome) DevTools device toolbar 360 × 740, then a window 1600 px wide: `/math`; drag f's `leg A depth left at t_x` to 0 and back; in the console on each:
```js
document.documentElement.scrollWidth <= window.innerWidth
```
→ expected: `true` at both widths; the f drawing redraws while dragging; switching to another tab for 6 minutes and back adds exactly one `public.json` request.
if not: `false` = horizontal scroll: screenshot and report (a layout fix on a branch; no rollback needed).
**STOP**

## Rollback

23. Back to the web and the compose file that ran before this deploy (the api and the volume stay on 7b-A). At a quiet minute (step 12) when there is one.
```bash
cd /home/isoca/projects/cute-trading && cp compose.web.yml.pre7b-B compose.web.yml && docker tag cute-trading/web:pre7b-B cute-trading/web:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker image inspect -f '{{.Id}}' cute-trading/web:pre7b-B && docker inspect -f '{{.Image}}' cute-trading-web-1 && grep -c 'ct-math' compose.web.yml
```
→ expected: the same `sha256:…` twice, then `0`; from the PC `curl -sS -o /dev/null -w '%{http_code}\n' https://radar.isoca.space/math` prints `302`.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` and report.
Note: restore the compose file first: the old image under the new labels would answer `/math` without login with the dashboard's `index.html`.

## Results

- Snapshot at deploy (step 8): age, or the writer's log line:
- Rollback tag and compose backup (step 9):
- Quiet minute (step 12), UTC time, or "no quiet minute in 15 min":
- Baseline memory (step 13): web / api / scanner / postgres:
- Reconnects after the web recreate (step 14):
- Asset names and the gzip total (steps 2, 15):
- Flood wasm (step 18): status counts, web max MiB, api/scanner/postgres change, dashboard loaded:
- Flood public.json (step 19): status counts, web max MiB, api change:
- Flood /math (step 20): status counts, phone loaded:
- Chrome 1440 / 360 / 1600 (steps 21–22):
````

- [ ] **Step 2: Check the runbook's commands against the files**

Run: `grep -c 'ct-math' compose.web.yml && grep -n 'pre7b-B\|public-data' docs/superpowers/runbooks/phase7b-web-vps.md | head -5 && grep -c '^\*\*STOP\*\*$' docs/superpowers/runbooks/phase7b-web-vps.md`
Expected: `12` (the number steps 3 and 11 expect); the note and step 9 lines; `10`.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/runbooks/phase7b-web-vps.md
git commit -m "runbook: 7b-B web: release engine and size check, ship web + compose, 7b-A volume gate, pre7b-B tag and compose backup, no-cookie curls with --path-as-is, three floods, Chrome at 1440/360/1600, rollback"
```

---

## Spec coverage

| Spec | Requirement | Task(s) |
|---|---|---|
| 7b §6.1 | one column, header, live strip under it (NO / YES, edge on the table, as of HH:MM UTC), sections a–f in order, footer; 360 px and touch | 11, 12 |
| 7b §6.2 | a–e as 09-27 §6.2 (a mirror + D1 + crossed; b YES/NO/implication, S 2–6, Other listed, prices + formula; c status/bid/ask per leg, augmented, path lit, post_end note; d up to 4 legs, fees, exactly 1.00, steps past Q*, Π, Q*; e scrub, sliders 0–1,000 ms, presets = policies.toml, secondsDelay, caught/missed + captured) | 6, 7, 8, 9, 10 |
| 7b §6.2 | e's caption "caught is not filled: see f" | 9 (`copy.ts`) |
| 7b §6.2 | f: 3-leg NO start (all fill, small positive net), drain per leg at t_x, unwind against us 0–20 ticks, q_request, fees, "the usual burst"; planned vs filled, q_eff line, excess sold vs held_unsold, big net_cash "caught, and lost $x" / "filled, $x on paper"; live ladder headed "on the live scanner, as of …" with its "which markets" fold; the caption naming settle / basket_buy / unwind_sell | 7, 10, 11, 12 |
| 7b §6.2, 09-27 §6.2 | one engine call per animation frame while dragging | 9 (`useFrameThrottle`) |
| 7b §6.3 | one fetch on load; re-fetch on a visible tab after 5 min; fresh ≤ 30 min; stale "as of …: the live writer is behind"; missing (404, network, bad JSON, v ≠ 1) → one dim line, strip and f's live part left out | 3, 11, 12 |
| 7b §7.1, 09-27 §4.2 | `scripts/build-wasm.sh` (pinned CLI, no wasm-pack), pkg gitignored, tsconfig split, `WASM_REQUIRED`, `npm run build` needs pkg, dashboard build and tests Rust-free | 4, 13 |
| 09-27 §4.2 | web Docker stage on the rust.Dockerfile digest with cargo-chef `--package arb-wasm --target wasm32-unknown-unknown`, CLI installed, pkg into the node stage | 14 |
| 09-27 §4.3, 7b §3.4, §11 | web parity test reading the fixture files with node fs (vite fs.allow unchanged), byte for byte, all six functions; CI web job: toolchain, wasm32 target, cached CLI, build script, `cargo test -p arb-wasm`, `WASM_REQUIRED=1 npm test`, `npm run build` | 4, 5 |
| 09-27 §4.4 | 350 KB gzip budget (`check-math-bundle.mjs`), module guard in `vite.math.config.ts` (`generateBundle`, `moduleIds`) | 13 |
| 09-27 §5.1 | `math.html` + `src/math/main.tsx`, own copy module, no router/api/stream/help.ts; `vite.math.config.ts` (input, `math-assets`, `dist`, `emptyOutDir: false`); theme.css tokens + own layout CSS | 9, 12, 13 |
| 7b §7.2 | durationText, DurationRow, PayoffModel → `schematics/models.ts` (re-exported); `schematics/Empty.tsx` split from EmptyState; LatencyLadder keyed by scenario, `Omit<LadderRung,'account_id'>`; `publicToLadder` | 1, 2, 3 |
| 7b §5.3, 09-27 §5.2 | nginx `location = /math`, `/math-assets/` (gzip_static, the §11 open item decided), `location = /math-data/public.json` (root, max-age=60, gzip on) | 13, 14 |
| 7b §5.3 | compose: web mounts `public-data` read-only at `/srv/ct-public-root/math-data` | 14 |
| 7b §5.3, 09-27 §5.3 | full `ct-math` rule incl. `Path(/math-data/public.json)`, entrypoints, TLS, service, priority 1000, rl/if per IP, ct-sec, math CSP, no ct-auth | 14 |
| 7b §11 | §5.3: one other request (`public.json`), no `/api/` string ships; §5.4: four kinds of files | 11, 12, 13 |
| 7b §8 web | f default and burst with the real WASM; live strip fresh/stale/missing (mocked fetch); publicToLadder feeding LatencyLadder; the guard with the reused schematics; 360 px (CSS rules) + Chrome | 3, 10, 11, 12, 13, 15 |
| 09-27 §8 web | schematics from fixture models with key SVG attributes; all sections; a control change updates the drawing (real WASM); invalid input shows the sentence | 6, 7, 8, 9, 10, 12 |
| 7b §9 | runbook `phase7b-web-vps.md`: curl checks (public.json 200 JSON; `/math-data/`, `x.json`, `.tmp` to login; `--path-as-is` traversal), floods incl. public.json, quiet minute advisory 15 min, rollback tag from the running image ID | 15 |
| 09-27 §8 deploy | `/math` 200, wasm 200 `application/wasm` + gzip, `/math/`, `/math.html`, `/api/today` to login, plain HTTP → https; flood with `docker stats`, dashboard loads, phone on mobile data | 15 |

**Decisions and gaps:**
- **Contract:** every engine type, request and fixture name follows 7b-A's "Contract for 7b-B"; sections a, c and f start from 7b-A fixture inputs (`fed-book`, `no-edge-3`, `no-all-fill`; the burst is `no-one-leg-empty-burst`), and the tests compare with the fixture outputs instead of hand-computed figures.
- **Rollback tag `web:pre7b-B`, not `pre7b`** (the lead's decision): 7a-B's runbook already created `web:pre7b` on the VPS (its step 7), so the spec's "pre7b tag per image" is taken for the web image; 7b-B also saves `compose.web.yml.pre7b-B`, because the old image under the new labels would serve the dashboard shell at `/math` without login.
- **gzip_static for `/math-assets/`** (09-27 §5.2/§11 open item): the check script writes the `.gz` files, so nginx does no gzip work for the assets; `public.json` stays `gzip on` (7b §5.3).
- **`oha` is optional** (09-27 §11 open item): the runbook uses it when present, otherwise a 3,000-request `curl` loop with 50 in parallel.
- **A failed re-fetch keeps the last good snapshot, shown stale** (the lead's decision; 7b §6.3 names the missing cases for a load; for a later failure the page keeps what it had and shows it stale by its own `written_ms`).
- **The release wasm is size-optimized** with cargo's per-invocation profile variables (`opt-level=s`, LTO, one codegen unit) in `build-wasm.sh` and the Dockerfile; no `wasm-opt` (09-27 §4.2). If the budget still fails, Task 13 and the runbook stop for the owner.
- **The PC builds the release package** for `npm run build` (Task 13 step 5, runbook step 2): the 350 KB check cannot pass on a debug build. Lead-approved; the owner's OK is pending, and both steps say so.
- **jsdom has no layout:** the 360 px rule is pinned by `theme.test.ts` over `theme.css` and `math.css`, and checked in Chrome (runbook step 22).
- **The panel's payoff structure is checked against `payoff` fixtures** (Task 8), closing the gap 7a-B's plan left for 7b.
