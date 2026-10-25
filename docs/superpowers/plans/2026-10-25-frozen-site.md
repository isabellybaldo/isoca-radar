# Frozen portfolio site Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build cute-trading's portfolio site: three static pages (`/` landing, `/math`, `/sweet`) that read
only the committed frozen JSON, never poll, never look stale, and build in GitHub Actions into one
artifact.

**Architecture:** A build-time *frozen mode* (`--mode frozen` → `VITE_FROZEN=1`) switches the shared
/math and Sweet code to the frozen data and frozen wording; without it every existing behavior and test
stays as is. A new Vite config builds the landing and Sweet pages (`web/portfolio/`), the existing math
config builds /math into the same folder, and a finishing script copies the frozen JSON and checks the
output. A new workflow builds that folder and uploads it as the `radar-site` artifact; plan 3 adds the
deploy job.

**Tech Stack:** React 19, Vite 8, Vitest 4, TypeScript 5.9, Rust→WASM (`arb-wasm`, wasm-bindgen
0.2.129), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-24-portfolio-port-design.md` (§2 goal 2, §4, §5.2–§5.4).
Plan 2 of 3. Plan 1 (done) committed `web/frozen/`; plan 3 (isoca-infra) serves the artifact.

## Global Constraints

- Only `web/frozen/public.json` and `web/frozen/cards-all.json` are read; never edited (`frozen.test.ts`).
- Frozen moment: `FROZEN_MS = 1792432680000` (2026-10-19 17:58 UTC). Capture stopped 2026-10-19.
  Sweet spots data: 2026-10-16 15:08 → 2026-10-19 17:23 UTC, 110 paper trades. Capture began 2026-09-26.
- Sweet spots ships the **ALL window only**, with no window switch.
- In frozen mode nothing may show "stale", "behind", "live", "warming", "recomputing", "unavailable"
  (for age), "N min ago", or a LIVE tag; no `setInterval`-driven refetch; no `EventSource`; no `/api/`.
- Every page shows: `Frozen snapshot from 2026-10-19 17:58 UTC. Capture stopped on 2026-10-19.`
- Without frozen mode (`npm run build`, `npm test` defaults) behavior is unchanged: the whole existing
  suite keeps passing untouched, except tests this plan explicitly edits.
- The math bundle may not import `src/pages|api|explain` or `src/help.ts` (`noDashboardCode()` in
  `vite.math.config.ts`); frozen-mode code it needs lives in `src/frozenMode.ts` and `src/lib/`.
- Output layout (plan 3's nginx contract: `try_files $uri $uri.html $uri/ =404`): `dist-portfolio/`
  holds `index.html`, `sweet.html`, `math.html`, `site-assets/`, `math-assets/`, `frozen/public.json`,
  `frozen/cards-all.json`. One CSP serves all of it (plan 3): `script-src 'self' 'wasm-unsafe-eval'`,
  `connect-src 'self'`, Google Fonts for style/font.
- No inline scripts or styles (CSP); `modulePreload.polyfill: false` in every config.
- Drawings over prose (owner): the cost section is an SVG schematic, theme tokens only, readable in
  light and dark.
- No `ci.yml` change (`deploy.test.ts` pins it); the new workflow is a separate file.
- Owner does every push; commits are local; no `Co-Authored-By` trailer.

## Review Focus

1. **A visitor in 2027**: the data is over a year old; no age warning appears anywhere → Task 2 and
   Task 3 tests run with `Date.now` mocked to 2027-01-01 and the real frozen files.
2. **A deep link or reload on `/sweet` or `/math`** on a plain static server (no SPA fallback) →
   Task 4's finishing script fails unless `sweet.html` and `math.html` exist at the output root, and
   every asset URL in them is absolute under `/site-assets/` or `/math-assets/`.
3. **The frozen JSON fails to load** (404, offline): Sweet shows the normal error panel once, with no
   reload loop and no retry timer; /math shows its engine drawings without the live strip → Task 3 test
   (fetch rejects), Task 2 test (fetch 404).
4. **A viewer in another time zone** sees the same UTC dates → Task 1 test runs the frozen line under
   `TZ=America/Sao_Paulo` semantics (formatters are `fmtUtc*`; assert the exact string).
5. **A phone (360 px wide)**: the landing and Sweet pages have no horizontal page scroll → Task 4's
   manual check at 360 px in Chrome DevTools, done by the owner before plan 3's deploy.

---

### Task 1: Frozen mode

**Files:**
- Create: `web/src/frozenMode.ts`, `web/src/vite-env.d.ts`, `web/.env.frozen`
- Test: `web/src/frozenMode.test.ts`

**Interfaces:**
- Produces: `isFrozen(): boolean` (reads `import.meta.env.VITE_FROZEN === '1'` on every call, so
  `vi.stubEnv` works), `FROZEN_MS: number = 1792432680000`, `FROZEN_LINE: string`,
  `frozenOn(): string` = `` `frozen on ${fmtUtcDateTime(FROZEN_MS)} UTC` ``.

- [ ] **Step 1: Write the failing test**

```ts
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FROZEN_LINE, FROZEN_MS, frozenOn, isFrozen } from './frozenMode';

afterEach(() => vi.unstubAllEnvs());
describe('frozen mode', () => {
  it('is off unless VITE_FROZEN is 1', () => {
    expect(isFrozen()).toBe(false);
    vi.stubEnv('VITE_FROZEN', '1');
    expect(isFrozen()).toBe(true);
  });
  it('names the frozen moment in UTC', () => {
    expect(FROZEN_MS).toBe(1792432680000);
    expect(FROZEN_LINE).toBe('Frozen snapshot from 2026-10-19 17:58 UTC. Capture stopped on 2026-10-19.');
    expect(frozenOn()).toContain('2026-10-19');
    expect(frozenOn()).toContain('17:58');
  });
});
```

- [ ] **Step 2:** `cd web && npx vitest run src/frozenMode.test.ts` → FAIL (module not found).
- [ ] **Step 3:** Create `src/frozenMode.ts` (imports only `./lib/time`), `.env.frozen` with
  `VITE_FROZEN=1`, and `src/vite-env.d.ts` (`/// <reference types="vite/client" />` plus
  `interface ImportMetaEnv { readonly VITE_FROZEN?: string }`). Check `tsconfig.math.json` sees the
  `.d.ts` (it includes `src/math` and `src/wasm`; add the file to its `include` if `tsc -p` complains).
- [ ] **Step 4:** `npx vitest run src/frozenMode.test.ts && npx tsc --noEmit && npx tsc --noEmit -p tsconfig.math.json` → PASS, no errors.
- [ ] **Step 5: Commit** `web: a build-time frozen mode (VITE_FROZEN) and the frozen moment, 2026-10-19 17:58 UTC`

### Task 2: /math in frozen mode

**Files:**
- Modify: `web/src/math/snapshot.ts` (add `snapshotUrl()`), `web/src/math/useSnapshot.ts`,
  `web/src/math/MathPage.tsx`, `web/src/math/LiveStrip.tsx`, `web/src/math/LiveLadder.tsx`,
  `web/src/schematics/LatencyLadder.tsx`, `web/src/math/copy.ts`, `web/src/math/story/copy.ts` and
  where chapter 6/7 copy is read, `web/math.html` (description)
- Test: `web/src/math/frozen.test.tsx`

**Interfaces:**
- Consumes: Task 1's `isFrozen`, `FROZEN_LINE`, `frozenOn`.
- Produces: `snapshotUrl(): string` = `'/frozen/public.json'` when frozen, else `SNAPSHOT_URL` (which
  stays `'/math-data/public.json'` for the existing tests).

Frozen wording (exact; live wording unchanged):

| Where | Frozen text |
|---|---|
| `asOfLine` (strip and ladder) | `frozenOn()`; never `BEHIND`, never the `stale` class |
| `STRIP_TITLE` | `the last captured day (UTC)` |
| LiveLadder heading | `` `on the scanner, ${frozenOn()}` `` |
| `MARKETS_WARMING` | `the 30-day window never filled: capture stopped on 2026-10-19.` |
| `FOOTER` | `The scanner is switched off. The math still runs.` followed by a link `← isoca-radar` to `/` |
| story `HOOK` last clause | `chapter 7 shows its paper accounts as they stood when capture stopped on 2026-10-19.` |
| chapter 7 `lead` | `The scanner kept six paper accounts that differed only in latency, from the fastest rung to the slowest, and settled every trade by the rules of chapter 6. Here they are as they stood on 2026-10-19.` |
| chapter 6 caption tail | `…the same rules the scanner’s paper accounts used; here the unwind happens at one instant, the sim waited the account’s delay.` |
| page top, under the title | `FROZEN_LINE` |
| after chapter 7 | a link `What it took to run live →` to `/#cost` (the landing's cost section has `id="cost"`) |
| `math.html` description (both modes) | `How a scanner finds a basket arbitrage on Polymarket, sizes it and times it, and what can go wrong between seeing it and holding it. Drawn with the scanner's own Rust code and its paper accounts.` |

In frozen mode `MathPage` computes `stale = false`, and `useSnapshot` skips the `visibilitychange`
refetch (one fetch per page load).

- [ ] **Step 1: Write `src/math/frozen.test.tsx`** (render `MathPage` like `MathPage.test.tsx` does, with
  its engine fixture; `vi.stubEnv('VITE_FROZEN', '1')`; `Date.now` mocked to `Date.UTC(2027,0,1)`;
  `fetch` stubbed to return `web/frozen/public.json` imported `?raw`):
  - `fetch` was called once, with `'/frozen/public.json'`;
  - the page text contains `FROZEN_LINE` and `frozenOn()`;
  - it contains none of `BEHIND`, `LIVE_UNAVAILABLE`, `'warming: part of'`, `'on the live scanner'`,
    `'live paper accounts'`, `'The live dashboard is private'`;
  - firing `visibilitychange` (visible) does not call `fetch` again;
  - with `fetch` returning 404: no `BEHIND`, `FROZEN_LINE` still shown.
- [ ] **Step 2:** `npx vitest run src/math/frozen.test.tsx` → FAIL.
- [ ] **Step 3:** Implement the table above; each frozen string is a named export next to its live one
  (e.g. `FOOTER_FROZEN`), picked with `isFrozen()` at render.
- [ ] **Step 4:** `npx vitest run src/math && npx vitest run src/schematics` → all PASS (the existing
  live tests unchanged; only a test pinning the old `math.html` description is updated, if one exists).
- [ ] **Step 5: Commit** `web: /math in frozen mode: reads /frozen/public.json once, says when it froze, never says behind or live`

### Task 3: Sweet spots, frozen

**Files:**
- Modify: `web/src/pages/Sweet.tsx` (export `SweetBody`; it takes `computed: string` instead of calling
  `useNow` itself; `Sweet` computes `computedText(c.computed_ms, nowMs)` with `useNow(30_000)` as before)
- Create: `web/src/portfolio/FrozenSweet.tsx`
- Test: `web/src/portfolio/FrozenSweet.test.tsx`

**Interfaces:**
- Produces: `FrozenSweet(): JSX.Element`, rendering `Box title="SWEET SPOTS" help="sweet" hero` with
  actions `<Tag tone="plain">FROZEN · ALL</Tag>`, the `ONE_LINE_CAVEAT` quip, then
  `Loadable` over `useApi((s) => getJson<CardsResponse>('/frozen/cards-all.json', s), [])` (no refresh
  deps) → the SWEET card's `SweetBody` with `computed = \`computed ${fmtUtcDateTime(c.computed_ms)} UTC\``,
  or `EmptyState text={SWEET_EMPTY}` if it has no `series.sweet`.
- `SweetBody`'s props: today's props plus `computed: string`.

- [ ] **Step 1: Write the failing tests** (`FrozenSweet.test.tsx`; `fetch` stubbed with
  `web/frozen/cards-all.json` `?raw`; fake timers; `Date.now` = 2027-01-01):
  - `fetch` called once with `'/frozen/cards-all.json'`; after `vi.advanceTimersByTime(10 * 60_000)`
    still once;
  - shows `FROZEN · ALL`, the side chips (`NO`, `YES`) and the grid;
  - shows no `7D`, `30D`, `24H` buttons, no `LIVE`, `WARMING`, `UNDER REVIEW`, no `min ago`;
  - the "how it's computed" fold text contains `computed 2026-10-19`;
  - `fetch` rejecting → the error panel shows once, `fetch` still called once after 10 minutes.
- [ ] **Step 2:** `npx vitest run src/portfolio/FrozenSweet.test.tsx` → FAIL.
- [ ] **Step 3:** Implement. `FrozenSweet` imports `getJson`, not `api` (so no `/api/` path is bundled).
- [ ] **Step 4:** `npx vitest run src/portfolio src/pages/Sweet.test.tsx` → PASS (`Sweet.test.tsx` unchanged).
- [ ] **Step 5: Commit** `web: Sweet spots frozen: the ALL window from /frozen/cards-all.json, one fetch, no window switch`

### Task 4: The portfolio pages, the cost drawing, the build

**Files:**
- Create: `web/portfolio/index.html`, `web/portfolio/sweet.html` (copy `index.html`'s head: fonts,
  `data:,` favicon, no inline script; titles `isoca-radar` and `isoca-radar · sweet spots`; drop
  `noindex`), `web/src/portfolio/Shell.tsx`, `web/src/portfolio/Landing.tsx`,
  `web/src/portfolio/landing-main.tsx`, `web/src/portfolio/sweet-main.tsx`,
  `web/src/portfolio/portfolio.css` (only if `theme.css` lacks what the landing needs),
  `web/src/schematics/RunCost.tsx`, `web/vite.portfolio.config.ts`, `web/scripts/finish-portfolio.mjs`
- Modify: `web/package.json` (script `build:portfolio`), `.gitignore` (`web/dist-portfolio/`),
  `README.md`, `PROJECT.md`
- Test: `web/src/portfolio/Landing.test.tsx`, `web/src/schematics/RunCost.test.tsx`,
  `web/src/portfolio/build.test.ts`

**Interfaces:**
- Consumes: `FrozenSweet` (Task 3), `FROZEN_LINE` (Task 1).
- Produces: `npm run build:portfolio` → `web/dist-portfolio/` in the layout of Global Constraints.
- `Shell({ children })`: header `♥ isoca-radar` → `/`, nav `MATH` → `/math`, `SWEET SPOTS` → `/sweet`
  (plain `<a>`, no router: three separate pages); footer `FROZEN_LINE`.

Landing copy (exact):
- h1 `isoca-radar`
- lead: `A read-only lab for Polymarket arbitrage, run from 2026-09-26 to 2026-10-19. It watched the order books, found baskets whose prices added up to less than a dollar, and paper-traded them at six latencies. Paper only: it never placed an order.`
- card `/math`, **The math**: `Seven chapters that run the scanner’s own Rust code in your browser: how an arbitrage is found, sized and timed, and what goes wrong between seeing it and holding it.`
- card `/sweet`, **Sweet spots**: `Where the edge survived fees and money tied up, over the last three days of paper trading (110 trades).`
- section **What it took to run live**: `<RunCost/>`, then `It was switched off on 2026-10-19, when the host started limiting its CPU. The code is still in the repo; this page is what is left running.`

`RunCost` (SVG, `role="img"`, an `aria-label` summarising it; geometry helpers from
`src/schematics/geo.ts` like the other schematics): Polymarket feed → scanner → raw capture → compact →
Parquet archive, with these labels, each verified before it ships:

| Label | Source to verify |
|---|---|
| `123 GB/day in · 1.4 MB/s, all day` | `spikes/probe/RESULTS.md` (Q7c) |
| `33 GB/day after zstd` | same |
| `scanner ≈ 65 % of a core, all day` | 2026-10-19 Hostinger panel (owner); keep only if the owner confirms |
| `compact: 2 cores while compacting` | same |
| `kept: 24 h raw ≤ 30 GB · archive ≤ 40 GB` | `PROJECT.md` "Where it runs" |
| `7 services · 1.1 GB of memory limits` | sum the `mem_limit`s in `compose.yml` + `compose.web.yml` (recompute; drop if Postgres/scanner have none) |
| `left behind: 28 GB of data` | plan 1's backup size |

`vite.portfolio.config.ts`: `root: 'portfolio'`, `envDir` = `web/`, `build.outDir: '../dist-portfolio'`,
`emptyOutDir: true`, `assetsDir: 'site-assets'`, inputs `portfolio/index.html` and
`portfolio/sweet.html`, `modulePreload: { polyfill: false }`, `base: '/'`.

`build:portfolio` = `tsc --noEmit && tsc --noEmit -p tsconfig.math.json && vite build -c vite.portfolio.config.ts --mode frozen && vite build -c vite.math.config.ts --mode frozen --outDir dist-portfolio && node scripts/finish-portfolio.mjs`
(no inline env vars: npm runs scripts in `cmd` on Windows).

`finish-portfolio.mjs`: copies the two frozen files to `dist-portfolio/frozen/`; runs
`scripts/check-math-bundle.mjs` with `MATH_DIST=dist-portfolio/` (child process env); then fails
unless `index.html`, `sweet.html`, `math.html`, `frozen/public.json`, `frozen/cards-all.json` exist,
every `src`/`href` in the two site pages is under `/site-assets/`, `data:,` or the two font hosts, no
`site-assets/*.js` contains `/api/` or `EventSource`, and no site page has an inline script or style.
Prints `portfolio ok` last.

- [ ] **Step 1: Write the failing tests**
  - `Landing.test.tsx`: renders `<Shell><Landing/></Shell>`; finds the h1, both card links (`/math`,
    `/sweet`), the lead's `Paper only: it never placed an order.`, the cost heading and `FROZEN_LINE`;
    with the cost heading's text removed, the page text has no `/\blive\b/i`.
  - `RunCost.test.tsx`: renders an element with `role="img"` and every label string that survived
    verification.
  - `build.test.ts`: `vite.portfolio.config.ts` text has `assetsDir: 'site-assets'`,
    `polyfill: false`, both inputs; `package.json`'s `build:portfolio` equals the string above.
- [ ] **Step 2:** `npx vitest run src/portfolio src/schematics/RunCost.test.tsx` → FAIL.
- [ ] **Step 3:** Verify the cost numbers (table above); ask the owner about the two Hostinger numbers
  before writing them; then implement the files.
- [ ] **Step 4:** Tests and the build:
  Run (repo root, Git Bash): `sh scripts/build-wasm.sh && cd web && npx vitest run && npm run build:portfolio`
  Expected: all test files pass; the build ends with the math check's lines and `portfolio ok`.
- [ ] **Step 5: Local look** — `python -m http.server -d dist-portfolio 8099`, open
  `http://localhost:8099/index.html`, `/sweet.html`, `/math.html` in Chrome; DevTools at 360 px wide: no
  horizontal scroll; light and dark both readable; console shows no CSP-unrelated errors. (The plain
  server doesn't map `/math` → `math.html`; plan 3's nginx does.) Owner repeats this check.
- [ ] **Step 6:** `README.md` and `PROJECT.md`: one short section each: frozen since 2026-10-19,
  served at radar.isoca.space as static files, `npm run build:portfolio` builds it, the live code
  is kept but not deployed.
- [ ] **Step 7: Commit** `web: the portfolio pages: a landing with the cost of running live, Sweet spots and /math, built into one static folder`

### Task 5: Build it in GitHub Actions

**Files:**
- Create: `.github/workflows/portfolio.yml`
- Test: `web/src/portfolio/workflow.test.ts`

**Interfaces:**
- Produces: on push to `main`, on pull requests and on `workflow_dispatch`, a job `build` that leaves an
  artifact named `radar-site` holding `web/dist-portfolio/` (retention 7 days). Plan 3 adds a `deploy`
  job that downloads `radar-site` and calls isoca-infra's reusable workflow, only on `main`.

The `build` job repeats `ci.yml`'s web-job toolchain steps in the same order (rustup show, wasm target,
`wasm-bindgen-version.sh` output, cache of `~/.cargo/bin/wasm-bindgen`, install if missing,
`sh scripts/build-wasm.sh`, setup-node 22 with the npm cache), then in `web/`: `npm ci`,
`npm run build:portfolio`, then `actions/upload-artifact`. Every `uses:` is pinned to a full commit SHA
with the tag in a comment (resolve with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>`).
`permissions: contents: read`.

- [ ] **Step 1: Write `workflow.test.ts`** (reads the YAML as text, like `deploy.test.ts`): contains
  `npm run build:portfolio`, `name: radar-site`, `path: web/dist-portfolio`, `workflow_dispatch`,
  `permissions:` with `contents: read`; every `uses:` line matches `/@[0-9a-f]{40}\b/`; the
  wasm-bindgen steps appear in `ci.yml`'s order.
- [ ] **Step 2:** `npx vitest run src/portfolio/workflow.test.ts` → FAIL.
- [ ] **Step 3:** Write the workflow.
- [ ] **Step 4:** `npx vitest run` → all pass. (The workflow itself runs once the owner pushes the
  branch; its first green run is checked in the PR.)
- [ ] **Step 5: Commit** `ci: build the portfolio site in GitHub Actions and keep it as the radar-site artifact`
