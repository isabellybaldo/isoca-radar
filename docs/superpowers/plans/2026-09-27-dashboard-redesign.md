# Dashboard redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the non-Radar pages of radar.isoca.space short and drawing-first, give Sweet spots its own `/sweet` page plus a Radar card, and make the layout work at 1440 px and at phone width.

**Architecture:** Web-only (`web/`), no api or Rust change. A few shared pieces (`Tiles`, `Fold`, `SubHead`, `PageHead`, `Box` `hero`/`actions`) define one section pattern: title row → tiles (only where the number is the answer) → one drawing → closed folds. New drawings (`IntervalChart`, `LatencyAxis`, `SplitBar`, `EpisodeStrip`, `CountBars`) are pure functions of payloads the pages already fetch.

**Tech Stack:** React 19, react-router 7, TypeScript, Vite, Vitest + Testing Library (jsdom), hand-made SVG (no chart library), plain CSS in `web/src/theme.css`.

**Spec:** `docs/superpowers/specs/2026-09-27-dashboard-redesign-design.md`

## Global Constraints

- Working copy: worktree `C:\Users\Isabelly\PycharmProjects\cute-trading-redesign`, branch `dashboard-redesign`. All commands below run from `web/` in that worktree.
- No api, Rust, nginx or compose change. Every figure comes from existing payloads; the web never recomputes a figure the api returns.
- Money and decimals shown as text go through `lib/money.ts` (`formatMoney`, `formatMoneyCents`, `formatDecimal`, `formatPercent`) or `<Money>`; `Number(...)` is for SVG geometry only.
- Keep the identity: `theme.css` palette tokens, Press Start 2P (`--px`) for section titles and chips only, VT323 (`--big`) for big numbers, IBM Plex Mono (`--body`) for everything else. The light theme stays a joke (`FakeToggle`).
- Tile labels, fold labels and chart annotations: sentence case, Plex Mono.
- Every section keeps a `(?)` (`Box` requires `help`).
- `localStorage` access always inside try/catch; the page renders correctly without it.
- `.app` max-width 1440 px. Phone breakpoint: 600 px.
- Nav order: `RADAR · SWEET · EPISODES · PAPER · PAPERS · HEALTH`.
- Commit after every task, message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A fold's content is still in the DOM when closed** (native `<details>`). Tests that use `getByText` on a value now shown twice (tile + folded table) throw "multiple elements". Each task says which assertions switch to `getAllByText` — a reviewer should grep for new duplicates.
2. **SWEET card present but `series.sweet` missing, or status `under_review`/`warming` with rows still attached** must show the state panel, never an empty chart with live-looking chips (Task 4 test "state panels").
3. **Scenario chosen on `/sweet` that doesn't exist for the chosen side/slice** (e.g. switch side after picking `colo-idle`) must fall back to the first available scenario, not render an empty chart (Task 4 test "falls back").
4. **Edge capture with thin edge** (`withThinEdge` nulls every rate) must render "—" rates, hollow dots, and no crash in tiles (Task 6 test "thin edge").
5. **Episodes page with one episode, or all episodes at the same `opened_ms`** must still draw the strip (x-span padded), not divide by zero (Task 8 test "single episode").

---

## File map

| File | Status | Responsibility |
|---|---|---|
| `web/src/components/Tiles.tsx` | new | big-number tiles |
| `web/src/components/Fold.tsx` | new | remembered `<details>` fold |
| `web/src/components/SubHead.tsx` | new | flat sub-heading (replaces inner boxes) |
| `web/src/components/PageHead.tsx` | new | page title row (h1, actions, `(?)`) |
| `web/src/components/Box.tsx` | modify | `hero`, `actions` |
| `web/src/components/IntervalChart.tsx` | new | Sweet spots interval rows |
| `web/src/components/LatencyAxis.tsx` | new | scenarios on one latency axis (replaces `DecayChart`) |
| `web/src/components/SplitBar.tsx` | new | two-part NO/YES bar |
| `web/src/components/EpisodeStrip.tsx` | new | Episodes timeline strip |
| `web/src/components/CountBars.tsx` | new | integrity bars |
| `web/src/components/SweetCard.tsx` | new | Radar's Sweet spots card |
| `web/src/pages/Sweet.tsx` | new | `/sweet` page (logic moved from `components/SweetSpots.tsx`) |
| `web/src/components/SweetSpots.tsx` (+ test) | delete | moved to `pages/Sweet.tsx` |
| `web/src/components/DecayChart.tsx` (+ test) | delete | replaced by `LatencyAxis` |
| `web/src/components/CardView.tsx` | modify | split into `CardTable`, `CardMeta`, `Claim`; folds |
| `web/src/components/CardCharts.tsx` | modify | decay → `LatencyAxis` |
| `web/src/components/EdgeCapture.tsx` | modify | tiles + `LatencyAxis` + `SplitBar` + folds |
| `web/src/components/LineChart.tsx` | modify | `legend` switch, `markerLabel` |
| `web/src/lib/sweet.ts` | modify | `rowsFor`, `candidateCount`, `bestCandidate`, `rowName`, copy |
| `web/src/lib/time.ts` | modify | `fmtUtcHms`, `fmtUtcDayMonth` |
| `web/src/pages/{Episodes,Paper,Papers,Health,Radar}.tsx` | modify | per spec §4, §5.2 |
| `web/src/App.tsx` | modify | nav + `/sweet` route |
| `web/src/help.ts` | modify | Papers subtitle into its `(?)` |
| `web/src/theme.css` | modify | new classes, width, phone |
| `web/src/test/setup.ts` | modify | clear `localStorage` after each test |

---

### Task 1: Shared section pieces and layout CSS

**Files:**
- Create: `web/src/components/Tiles.tsx`, `web/src/components/Fold.tsx`, `web/src/components/SubHead.tsx`, `web/src/components/PageHead.tsx`
- Modify: `web/src/components/Box.tsx`, `web/src/theme.css`, `web/src/test/setup.ts`
- Test: `web/src/components/Tiles.test.tsx`, `web/src/components/Fold.test.tsx`, `web/src/components/Box.test.tsx`

**Interfaces:**
- Produces:
  - `Tiles({ items }: { items: Tile[] })`, `interface Tile { label: string; value: ReactNode; title?: string; tone?: 'pos' | 'neg' }`
  - `Fold({ id, label, children }: { id: string; label: string; children: ReactNode })` — storage key `fold:<id>`
  - `SubHead({ children, help, right }: { children: ReactNode; help?: HelpId; right?: ReactNode })` renders `<h3 class="subhead">`
  - `PageHead({ title, help, children }: { title: string; help: HelpId; children?: ReactNode })` renders `<h1>`
  - `Box({ title, help, className, hero, actions, children })` — `hero?: boolean`, `actions?: ReactNode`

- [ ] **Step 1: Install dependencies in the worktree**

Run (from `web/`): `npm ci`
Expected: completes; then `npx vitest run` → `Tests  131 passed (131)`.

- [ ] **Step 2: Write the failing tests**

`web/src/components/Tiles.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Tiles } from './Tiles';

describe('Tiles', () => {
  it('renders each value with its label, tone and title', () => {
    render(<Tiles items={[{ label: 'on the table', value: '$258.01' }, { label: 'captured', value: '−$462.87', tone: 'neg', title: '−$462.8712' }]} />);
    const list = screen.getByRole('list');
    expect(list.tagName).toBe('DL');
    expect(screen.getByText('on the table')).toBeInTheDocument();
    expect(screen.getByText('$258.01')).toBeInTheDocument();
    const neg = screen.getByText('−$462.87');
    expect(neg).toHaveClass('neg');
    expect(neg).toHaveAttribute('title', '−$462.8712');
  });
});
```

`web/src/components/Fold.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { Fold } from './Fold';

const details = () => screen.getByText('all numbers').closest('details') as HTMLDetailsElement;

describe('Fold', () => {
  it('starts closed, opens on click, and remembers it', async () => {
    const { unmount } = render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(false);
    expect(screen.getByText('inside')).toBeInTheDocument(); // closed content stays in the DOM
    await userEvent.click(screen.getByText('all numbers'));
    expect(details().open).toBe(true);
    expect(localStorage.getItem('fold:t')).toBe('1');
    unmount();
    render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(true);
    await userEvent.click(screen.getByText('all numbers'));
    expect(localStorage.getItem('fold:t')).toBeNull();
  });

  it('renders closed and still toggles when storage throws', async () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    render(<Fold id="t" label="all numbers"><p>inside</p></Fold>);
    expect(details().open).toBe(false);
    await userEvent.click(screen.getByText('all numbers'));
    expect(details().open).toBe(true);
  });
});
```

`web/src/components/Box.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Box } from './Box';

describe('Box', () => {
  it('puts actions in the title row and marks the hero section', () => {
    const { container } = render(<Box title="EDGE CAPTURE" help="edge" hero actions={<button type="button">24H</button>}><p>body</p></Box>);
    const head = container.querySelector('.box-head') as HTMLElement;
    expect(head).toContainElement(screen.getByRole('button', { name: '24H' }));
    expect(container.querySelector('section')).toHaveClass('box', 'hero');
  });

  it('is a plain box without hero', () => {
    const { container } = render(<Box title="X" help="edge"><p>b</p></Box>);
    expect(container.querySelector('section')).not.toHaveClass('hero');
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run src/components/Tiles.test.tsx src/components/Fold.test.tsx src/components/Box.test.tsx`
Expected: FAIL — `Failed to resolve import "./Tiles"` / `"./Fold"`; Box test fails on missing `hero` class.

- [ ] **Step 4: Implement**

`web/src/components/Tiles.tsx`:
```tsx
import type { ReactNode } from 'react';

export interface Tile {
  label: string;
  value: ReactNode;
  title?: string;
  tone?: 'pos' | 'neg';
}

/** Big numbers with a small sentence-case label (spec §3.1): only where the number is the answer (§3.3). */
export function Tiles({ items }: { items: Tile[] }) {
  return (
    <dl className="tiles" role="list">
      {items.map((t) => (
        <div key={t.label} className="tile" role="listitem">
          <dt>{t.label}</dt>
          <dd className={t.tone} title={t.title}>
            {t.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
```

`web/src/components/Fold.tsx`:
```tsx
import { type ReactNode, useState } from 'react';

const key = (id: string) => `fold:${id}`;

function readOpen(id: string): boolean {
  try {
    return localStorage.getItem(key(id)) === '1';
  } catch {
    return false;
  }
}

function writeOpen(id: string, open: boolean): void {
  try {
    if (open) localStorage.setItem(key(id), '1');
    else localStorage.removeItem(key(id));
  } catch {
    // storage blocked (private window, preview): the fold still works, it just forgets
  }
}

/** Spec §3.1: closed by default, remembered per id. Controlled so the state never depends on the
 *  browser's own <details> toggle (jsdom and old browsers differ); the summary stays keyboard-native. */
export function Fold({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  const [open, setOpen] = useState(() => readOpen(id));
  return (
    <details className="fold" open={open}>
      <summary
        onClick={(e) => {
          e.preventDefault();
          const next = !open;
          setOpen(next);
          writeOpen(id, next);
        }}
      >
        {label}
      </summary>
      <div className="fold-body">{children}</div>
    </details>
  );
}
```

`web/src/components/SubHead.tsx`:
```tsx
import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** Spec §3.2: a former inner box becomes a flat sub-heading with a rule above it. */
export function SubHead({ children, help, right }: { children: ReactNode; help?: HelpId; right?: ReactNode }) {
  return (
    <div className="subhead-row">
      <h3 className="subhead">{children}</h3>
      {right}
      {help && <Help id={help} />}
    </div>
  );
}
```

`web/src/components/PageHead.tsx`:
```tsx
import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/** A page's own title row (spec §4.3, §4.4): the window switch and page-level status live here. */
export function PageHead({ title, help, children }: { title: string; help: HelpId; children?: ReactNode }) {
  return (
    <div className="pagehead">
      <h1>{title}</h1>
      <div className="pagehead-actions">{children}</div>
      <Help id={help} />
    </div>
  );
}
```

`web/src/components/Box.tsx` (replace the function):
```tsx
import type { ReactNode } from 'react';
import type { HelpId } from '../help';
import { Help } from './Help';

/**
 * A page section. `help` is required: every section of every page has a (?) (spec §8.3).
 * `hero` marks the page's one drawing section (the only one with the offset shadow, redesign §3.2);
 * `actions` sits in the title row (window switch, status tag, scenario chips).
 */
export function Box({ title, help, className, hero = false, actions, children }: {
  title?: ReactNode;
  help: HelpId;
  className?: string;
  hero?: boolean;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const cls = ['box', hero ? 'hero' : null, className ?? null].filter(Boolean).join(' ');
  return (
    <section className={cls}>
      <div className="box-head">
        {title !== undefined && <h2>{title}</h2>}
        {actions !== undefined && <div className="box-actions">{actions}</div>}
        <Help id={help} />
      </div>
      {children}
    </section>
  );
}
```

`web/src/test/setup.ts` — add storage cleanup:
```ts
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
  try {
    localStorage.clear();
  } catch {
    // a test may have stubbed Storage to throw
  }
});
```

`web/src/theme.css`:
1. Change `.app { … max-width: 1100px; … }` to `max-width: 1440px`.
2. Change `.box { border: 3px solid var(--line); box-shadow: 4px 4px 0 var(--line); … }` to drop the shadow: `.box { border: 3px solid var(--line); background: var(--card); padding: 10px 12px; min-width: 0; }` and add `.box.hero { box-shadow: 4px 4px 0 var(--line); }`.
3. Replace the final `@media (max-width: 640px) { … }` block with the 600 px block below, and append the rest:
```css
.box-actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.pagehead { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; padding: 2px 0 4px; }
.pagehead h1 { font-family: var(--px); font-size: 11px; line-height: 1.5; }
.pagehead-actions { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; flex: 1; min-width: 0; }
.tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 8px; margin: 6px 0 10px; }
.tile { display: flex; flex-direction: column-reverse; gap: 2px; border: 2px solid var(--line2); background: var(--chip); padding: 6px 8px; min-width: 0; }
.tile dt { color: var(--dim); font-size: 13px; }
.tile dd { font-family: var(--big); font-size: 34px; line-height: 1; white-space: nowrap; }
.fold { border: 2px solid var(--line2); margin-top: 8px; }
.fold > summary { cursor: pointer; padding: 4px 8px; color: var(--dim); list-style: none; }
.fold > summary::-webkit-details-marker { display: none; }
.fold > summary::before { content: '▸ '; }
.fold[open] > summary::before { content: '▾ '; }
.fold-body { padding: 4px 8px 8px; display: grid; gap: 6px; }
.subhead-row { display: flex; align-items: center; gap: 8px; border-top: 1px solid var(--line2); padding-top: 8px; margin-top: 10px; }
.subhead { font-family: var(--body); font-weight: 700; font-size: 15px; flex: 1; min-width: 0; }
td.num, .num { white-space: nowrap; font-variant-numeric: tabular-nums; }
@media (max-width: 600px) {
  body { padding: 0; }
  .app { border: 0; box-shadow: none; }
  .body { padding: 8px; }
  .top { padding: 8px; gap: 8px; flex-wrap: nowrap; overflow-x: auto; }
  .logo { font-size: 9px; white-space: nowrap; }
  .nav { margin-left: 0; flex-wrap: nowrap; gap: 4px; }
  .nav a { font-size: 7px; padding: 4px; white-space: nowrap; }
  .hearts { font-size: 20px; }
  .box { padding: 8px; }
  .tag { white-space: normal; }
  .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  svg.chart { min-height: 140px; }
}
```

- [ ] **Step 5: Run the tests**

Run: `npx vitest run`
Expected: all pass (131 + 5 new).

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Tiles.tsx web/src/components/Fold.tsx web/src/components/SubHead.tsx web/src/components/PageHead.tsx web/src/components/Box.tsx web/src/components/*.test.tsx web/src/theme.css web/src/test/setup.ts
git commit -m "web: shared section pieces (tiles, folds, sub-heads, page head), 1440 px layout, phone CSS

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Sweet spots helpers

**Files:**
- Modify: `web/src/lib/sweet.ts`
- Test: `web/src/lib/sweet.test.ts` (append)

**Interfaces:**
- Consumes: existing `grid`, `bucketText`, `HEADLINE_SCENARIO`, `interval` in `lib/sweet.ts`; `compareDecimal` from `lib/money.ts`; `fmtUtcHm` from `lib/time.ts`.
- Produces:
  - `rowsFor(s: SweetSeries, side: SweetSide, dim: SweetDim, scenario: string): SweetRow[]` — fee 0, canonical bucket order
  - `candidateCount(s: SweetSeries, side: SweetSide): number` — at `HEADLINE_SCENARIO`, fee 0
  - `bestCandidate(s: SweetSeries): SweetRow | null` — highest `lo` among candidates at `HEADLINE_SCENARIO`, fee 0; ties keep the api's order
  - `rowName(r: SweetRow): string` — `"whole NO side"` for `dimension === 'all'`, else `bucketText`
  - `recomputingText(computedMs: number): string`
  - `ONE_LINE_CAVEAT`, and `NOTHING_PAYS` becomes `"Nothing pays at sp-idle yet. Don't trade this, or get closer."`

- [ ] **Step 1: Write the failing tests** (append to `web/src/lib/sweet.test.ts`; add missing names to its import from `./sweet` and `sweetFixture, sweetRow` from `../test/fixtures` if not already imported)

```ts
describe('redesign helpers', () => {
  const s = sweetFixture([
    sweetRow({ dimension: 'hour', bucket: '12-16', scenario: 'sp-idle', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58' }),
    sweetRow({ dimension: 'hour', bucket: '00-04', scenario: 'sp-idle', label: 'negative', lo: '-0.3', mean: '-0.2', hi: '-0.1' }),
    sweetRow({ dimension: 'hour', bucket: '00-04', scenario: 'colo-idle', latency_ms: 52, label: 'candidate', lo: '0.5', mean: '0.6', hi: '0.7' }),
    sweetRow({ dimension: 'days', bucket: '1-7', scenario: 'sp-idle', label: 'candidate', lo: '0.09', mean: '0.2', hi: '0.3' }),
    sweetRow({ dimension: 'days', bucket: '1-7', scenario: 'sp-idle', fee_bips: 10, label: 'candidate', lo: '0.9', mean: '1', hi: '1.1' }),
    sweetRow({ side: 'yes', dimension: 'all', bucket: 'all', scenario: 'sp-idle', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2' }),
  ]).series.sweet!;

  it('rowsFor keeps one scenario, fee 0, in canonical bucket order', () => {
    expect(rowsFor(s, 'no', 'hour', 'sp-idle').map((r) => r.bucket)).toEqual(['00-04', '12-16']);
    expect(rowsFor(s, 'no', 'hour', 'colo-idle').map((r) => r.bucket)).toEqual(['00-04']);
    expect(rowsFor(s, 'no', 'days', 'sp-idle')).toHaveLength(1);
  });

  it('counts candidates per side at the headline scenario and fee 0 only', () => {
    expect(candidateCount(s, 'no')).toBe(2);
    expect(candidateCount(s, 'yes')).toBe(1);
  });

  it('picks the candidate with the highest lower bound, ignoring other scenarios and fees', () => {
    expect(bestCandidate(s)?.bucket).toBe('1-7');
    expect(bestCandidate(sweetFixture([sweetRow({ label: 'negative', lo: '-1', mean: '-0.5', hi: '0' })]).series.sweet!)).toBeNull();
  });

  it('names a row for people', () => {
    expect(rowName(sweetRow({ side: 'yes', dimension: 'all', bucket: 'all' }))).toBe('whole YES side');
    expect(rowName(sweetRow({ dimension: 'hour', bucket: '12-16' }))).toBe('12–16 UTC');
  });

  it('says what recomputing means and when it last had a result', () => {
    expect(recomputingText(Date.UTC(2026, 8, 27, 17, 14))).toBe(
      'Recomputing after a premise change. The last result was 17:14 UTC; new numbers appear when it finishes.',
    );
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/lib/sweet.test.ts`
Expected: FAIL — `rowsFor is not a function` (or import error).

- [ ] **Step 3: Implement** (append to `web/src/lib/sweet.ts`; add `import { compareDecimal } from './money';` next to the existing `formatMoney` import and `import { fmtUtcHm } from './time';`; change the `NOTHING_PAYS` value)

```ts
export const NOTHING_PAYS = "Nothing pays at sp-idle yet. Don't trade this, or get closer.";
/** Redesign §5.1: the caveat in one line on the page; the full SWEET_CAVEAT lives in the fold. */
export const ONE_LINE_CAVEAT = 'Paper, first in line. A candidate is a question for real small orders, not an answer.';

/** One scenario's rows for a side and slice, fee 0, canonical bucket order (redesign §5.1). */
export function rowsFor(s: SweetSeries, side: SweetSide, dim: SweetDim, scenario: string): SweetRow[] {
  const g = grid(s, side, dim);
  return g.buckets.map((b) => g.cell(b, scenario)).filter((r): r is SweetRow => r !== undefined);
}

const headlineCandidates = (s: SweetSeries) =>
  s.rows.filter((r) => r.label === 'candidate' && r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);

export function candidateCount(s: SweetSeries, side: SweetSide): number {
  return headlineCandidates(s).filter((r) => r.side === side).length;
}

/** Radar card (redesign §5.2): the candidate whose interval sits furthest above zero. */
export function bestCandidate(s: SweetSeries): SweetRow | null {
  let best: SweetRow | null = null;
  for (const r of headlineCandidates(s)) {
    if (r.lo === null) continue;
    if (best === null || best.lo === null || compareDecimal(r.lo, best.lo) > 0) best = r;
  }
  return best;
}

export function rowName(r: SweetRow): string {
  return r.dimension === 'all' ? `whole ${r.side.toUpperCase()} side` : bucketText(r.dimension, r.bucket);
}

export function recomputingText(computedMs: number): string {
  return `Recomputing after a premise change. The last result was ${fmtUtcHm(computedMs)} UTC; new numbers appear when it finishes.`;
}
```
Remove the old `NOTHING_PAYS` line at the top of the file (the new one replaces it).

- [ ] **Step 4: Run the tests**

Run: `npx vitest run src/lib/sweet.test.ts src/components/SweetSpots.test.tsx`
Expected: PASS (SweetSpots tests reference `NOTHING_PAYS` by name, if at all, so the new value is fine).

- [ ] **Step 5: Commit**

```bash
git add web/src/lib/sweet.ts web/src/lib/sweet.test.ts
git commit -m "web: sweet helpers for one-scenario rows, candidate counts, best candidate, recomputing copy

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: IntervalChart

**Files:**
- Create: `web/src/components/IntervalChart.tsx`
- Modify: `web/src/theme.css` (append)
- Test: `web/src/components/IntervalChart.test.tsx`

**Interfaces:**
- Consumes: `SweetRow` (`api/types.ts`); `LABEL_TAG`, `LABEL_TEXT`, `FEW_TRADES`, `interval`, `rowName` (`lib/sweet.ts`); `formatPercent`; `Tag`.
- Produces: `IntervalChart({ rows, label }: { rows: SweetRow[]; label: string })`; `export const NO_ROWS = 'No rows in this slice yet. Try ALL, or a wider window.'`; each row is `<li data-label={row.label}>`, the SVG has `rect.zero`, `line.bar` (class `dashed` for thin/not_tested), `rect.dot` (class `hollow` for negative/dead_on_paper).

- [ ] **Step 1: Write the failing test**

```tsx
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { sweetRow } from '../test/fixtures';
import { IntervalChart, NO_ROWS } from './IntervalChart';

const num = (el: Element | null, a: string) => Number(el?.getAttribute(a));

describe('IntervalChart', () => {
  const rows = [
    sweetRow({ dimension: 'hour', bucket: '12-16', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58', n_exec: 40, clusters: 33 }),
    sweetRow({ dimension: 'hour', bucket: '00-04', label: 'negative', lo: '-0.3', mean: '-0.2', hi: '-0.1', few_trades: true }),
    sweetRow({ dimension: 'hour', bucket: '04-08', label: 'thin', lo: '-0.5', mean: '0.1', hi: '0.9' }),
    sweetRow({ dimension: 'hour', bucket: '08-12', label: 'not_tested' }),
  ];

  it('draws candidates right of zero, filled, and negatives left of zero, hollow', () => {
    const { container } = render(<IntervalChart rows={rows} label="NO by hour at sp-idle" />);
    expect(screen.getByRole('list', { name: 'NO by hour at sp-idle' })).toBeInTheDocument();
    const cand = container.querySelector('li[data-label="candidate"]')!;
    const neg = container.querySelector('li[data-label="negative"]')!;
    const zeroX = num(cand.querySelector('rect.zero'), 'x');
    expect(num(cand.querySelector('line.bar'), 'x1')).toBeGreaterThan(zeroX);
    expect(num(neg.querySelector('line.bar'), 'x2')).toBeLessThan(zeroX);
    expect(cand.querySelector('rect.dot')).not.toHaveClass('hollow');
    expect(neg.querySelector('rect.dot')).toHaveClass('hollow');
    expect(within(cand as HTMLElement).getByText('CANDIDATE')).toHaveAttribute('title', 'survives here; worth a real small-order test.');
    expect(within(cand as HTMLElement).getByText('12–16 UTC')).toBeInTheDocument();
    expect(within(cand as HTMLElement).getByText('n 40 · C 33')).toBeInTheDocument();
    expect(within(neg as HTMLElement).getByText('few trades carry this row')).toBeInTheDocument();
  });

  it('dashes thin rows and draws no bar without a mean', () => {
    const { container } = render(<IntervalChart rows={rows} label="x" />);
    expect(container.querySelector('li[data-label="thin"] line.bar')).toHaveClass('dashed');
    const nt = container.querySelector('li[data-label="not_tested"]')!;
    expect(nt.querySelector('line.bar')).toBeNull();
    expect(nt.querySelector('rect.dot')).toBeNull();
    expect(within(nt as HTMLElement).getByText('—')).toBeInTheDocument();
  });

  it('shows the YES annualized rate as a percent', () => {
    render(<IntervalChart rows={[sweetRow({ side: 'yes', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2', annualized: '0.12' })]} label="x" />);
    expect(screen.getByText(/12%\/yr/)).toBeInTheDocument();
  });

  it('says so when the slice is empty', () => {
    render(<IntervalChart rows={[]} label="x" />);
    expect(screen.getByText(NO_ROWS)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/IntervalChart.test.tsx`
Expected: FAIL — cannot resolve `./IntervalChart`.

- [ ] **Step 3: Implement**

`web/src/components/IntervalChart.tsx`:
```tsx
import type { SweetLabel, SweetRow } from '../api/types';
import { formatPercent } from '../lib/money';
import { FEW_TRADES, LABEL_TAG, LABEL_TEXT, interval, rowName } from '../lib/sweet';
import { EmptyState } from './EmptyState';
import { Tag, type TagTone } from './Tag';

export const NO_ROWS = 'No rows in this slice yet. Try ALL, or a wider window.';
const W = 200;
const H = 10;

/** Fill, dash and tag carry the label as well as colour (redesign §5.1, §9 item 4). */
const LOOK: Record<SweetLabel, { tone: 'good' | 'bad' | 'dim'; hollow: boolean; dashed: boolean; tag: TagTone }> = {
  candidate: { tone: 'good', hollow: false, dashed: false, tag: 'no' },
  negative: { tone: 'bad', hollow: true, dashed: false, tag: 'flag' },
  dead_on_paper: { tone: 'bad', hollow: true, dashed: false, tag: 'flag' },
  inconclusive: { tone: 'dim', hollow: false, dashed: false, tag: 'plain' },
  thin: { tone: 'dim', hollow: false, dashed: true, tag: 'plain' },
  not_tested: { tone: 'dim', hollow: false, dashed: true, tag: 'plain' },
};

/** One shared $/attempt axis for every row, always including zero. Numbers are geometry only. */
function domain(rows: SweetRow[]): { lo: number; hi: number } {
  const vs = rows.flatMap((r) => [r.lo, r.mean, r.hi]).filter((v): v is string => v !== null).map(Number);
  const lo = Math.min(0, ...vs);
  const hi = Math.max(0, ...vs);
  const pad = (hi - lo || 1) * 0.05;
  return { lo: lo - pad, hi: hi + pad };
}

export function IntervalChart({ rows, label }: { rows: SweetRow[]; label: string }) {
  if (rows.length === 0) return <EmptyState text={NO_ROWS} />;
  const d = domain(rows);
  const sx = (v: number) => Math.round(((v - d.lo) / (d.hi - d.lo)) * (W - 2)) + 1;
  const zero = sx(0);
  return (
    <ul className="plain ivchart" aria-label={label}>
      {rows.map((r) => {
        const look = LOOK[r.label];
        const hasBar = r.mean !== null;
        const x1 = sx(Number(r.lo ?? r.mean ?? 0));
        const x2 = sx(Number(r.hi ?? r.mean ?? 0));
        return (
          <li key={`${r.side}|${r.dimension}|${r.bucket}|${r.scenario}`} className={`iv ${look.tone}`} data-label={r.label}>
            <span className="iv-name">{rowName(r)}</span>
            <svg className="iv-track" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
              <rect className="zero" x={zero} y={0} width={1} height={H} />
              {hasBar && (
                <>
                  <line className={look.dashed ? 'bar dashed' : 'bar'} x1={x1} x2={Math.max(x2, x1 + 1)} y1={H / 2} y2={H / 2} />
                  <rect className={look.hollow ? 'dot hollow' : 'dot'} x={sx(Number(r.mean)) - 1} y={1} width={3} height={H - 2} />
                </>
              )}
            </svg>
            <span className="iv-num num">{interval(r)}</span>
            <span className="iv-meta">
              <Tag tone={look.tag} title={LABEL_TEXT[r.label]}>{LABEL_TAG[r.label]}</Tag>
              <span className="dim">{`n ${r.n_exec} · C ${r.clusters}`}</span>
              {r.side === 'yes' && r.annualized !== null && <span className="dim">{` · ${formatPercent(r.annualized)}/yr`}</span>}
              {r.few_trades && <span className="dim"> {FEW_TRADES}</span>}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
```

Append to `web/src/theme.css`:
```css
.ivchart { gap: 4px; margin: 8px 0; }
.iv { display: grid; grid-template-columns: minmax(90px, 140px) minmax(120px, 1fr) auto; grid-template-areas: 'name track num' 'meta meta meta'; gap: 2px 10px; align-items: center; }
.iv-name { grid-area: name; color: var(--dim); }
.iv-track { grid-area: track; display: block; width: 100%; height: 14px; background: var(--bg); }
.iv-num { grid-area: num; }
.iv-meta { grid-area: meta; font-size: 13px; }
.iv .zero { fill: var(--fg); }
.iv .bar { stroke-width: 4px; vector-effect: non-scaling-stroke; }
.iv .bar.dashed { stroke-dasharray: 3 3; }
.iv.good .bar { stroke: var(--good); } .iv.good .dot { fill: var(--good); }
.iv.bad .bar { stroke: var(--bad); } .iv.bad .dot { fill: var(--bad); }
.iv.dim .bar { stroke: var(--line2); } .iv.dim .dot { fill: var(--dim); }
.iv .dot.hollow { fill: var(--bg); stroke: var(--bad); stroke-width: 1px; vector-effect: non-scaling-stroke; }
@media (max-width: 600px) { .iv { grid-template-columns: 1fr auto; grid-template-areas: 'name num' 'track track' 'meta meta'; } }
```

- [ ] **Step 4: Run the tests**

Run: `npx vitest run src/components/IntervalChart.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/IntervalChart.tsx web/src/components/IntervalChart.test.tsx web/src/theme.css
git commit -m "web: interval chart for sweet spots (fill, dash and tag carry the label, not colour alone)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `/sweet` page, nav, and Sweet spots off `/paper`

**Files:**
- Create: `web/src/pages/Sweet.tsx`, `web/src/pages/Sweet.test.tsx`
- Delete: `web/src/components/SweetSpots.tsx`, `web/src/components/SweetSpots.test.tsx`
- Modify: `web/src/App.tsx`, `web/src/App.test.tsx`, `web/src/pages/Paper.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`Box` `hero`/`actions`, `Tiles`, `Fold`), Task 2 (`rowsFor`, `candidateCount`, `recomputingText`, `ONE_LINE_CAVEAT`, `NOTHING_PAYS`), Task 3 (`IntervalChart`); existing `grid`, `nothingPays`, `footnote`, `bucketText`, `DIMS`, `COLO_TITLE`, `HEADLINE_SCENARIO`, `LABEL_TAG`, `LABEL_TEXT`, `FEW_TRADES`, `SWEET_CAVEAT`, `SWEET_EMPTY`, `interval` (`lib/sweet.ts`); `statusLabel`, `statusTone`, `warmingText`, `shortHash`, `computedText` (`lib/cards.ts`).
- Produces: `Sweet()` page component; route `/sweet`; `NAV` entry `{ to: '/sweet', label: 'SWEET', end: false }` second.

- [ ] **Step 1: Write the failing tests**

`web/src/pages/Sweet.test.tsx`:
```tsx
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { CardBody } from '../api/types';
import { warmingText } from '../lib/cards';
import { NOTHING_PAYS, ONE_LINE_CAVEAT, SWEET_CAVEAT, SWEET_EMPTY, footnote, recomputingText } from '../lib/sweet';
import { CARD_NOW, sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Sweet } from './Sweet';

const RECONCILE_NOTE =
  'Flagged episodes (engine_suspect, stale_transport) are excluded and counted, so SWEET does not reconcile with account equity.';
const renderPage = () => render(<MemoryRouter><Sweet /></MemoryRouter>);
const card = (rows: Parameters<typeof sweetFixture>[0], o: Partial<CardBody> = {}, s: Parameters<typeof sweetFixture>[1] = {}): CardBody => ({ ...sweetFixture(rows, s), ...o });

const rows = [
  sweetRow({ scenario: 'colo-idle', latency_ms: 52, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
  sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1', lo: '-0.2', hi: '-0.01', n_exec: 41, clusters: 35, few_trades: true }),
];

describe('Sweet page', () => {
  it('asks for 30D, shows the one-line caveat, tiles, and the sp-idle chart', async () => {
    const fetch = mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards: [card(rows)] }) });
    renderPage();
    expect(await screen.findByText(ONE_LINE_CAVEAT)).toBeInTheDocument();
    expect(String(fetch.mock.calls[0][0])).toBe('/api/cards?win=30d');
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles).toEqual(['NO candidates0', 'YES candidates0', 'rows tested2', 'scenariosp-idle']); // dt comes before dd in the DOM
    const chart = screen.getByRole('list', { name: 'NO · ALL · sp-idle' });
    expect(within(chart).getByText('NEGATIVE')).toBeInTheDocument();
    expect(screen.getByText(NOTHING_PAYS)).toBeInTheDocument();
  });

  it('switches scenario, side and slice, hides CATEGORY without tags', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [card([...rows, sweetRow({ side: 'yes', label: 'dead_on_paper' })], {}, { category_available: false })] } });
    renderPage();
    const scen = await screen.findByRole('group', { name: 'scenario' });
    expect(within(scen).getByRole('button', { name: /co-located trader/i })).toBeInTheDocument();
    await userEvent.click(within(scen).getByRole('button', { name: /co-located trader/i }));
    expect(within(screen.getByRole('list', { name: 'NO · ALL · colo-idle' })).getByText('CANDIDATE')).toBeInTheDocument();
    expect(within(screen.getByRole('group', { name: 'dimension' })).queryByRole('button', { name: 'CATEGORY' })).toBeNull();
    await userEvent.click(within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    // Review focus 3: colo-idle has no YES rows, so the chart falls back to the first YES scenario
    expect(within(screen.getByRole('list', { name: 'YES · ALL · sp-idle' })).getByText('DEAD ON PAPER')).toBeInTheDocument();
  });

  it('keeps every number and the method in folds', async () => {
    const c = card(rows, { hash: '3f2a9c1b7d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4', notes: [SWEET_CAVEAT, footnote(1), RECONCILE_NOTE] });
    mockApi({ '/api/cards': { win: '30d', cards: [c] } });
    renderPage();
    const numbers = (await screen.findByText('all numbers')).closest('details')!;
    expect(within(numbers).getByRole('columnheader', { name: /a co-located trader/ })).toBeInTheDocument();
    const how = screen.getByText("how it's computed").closest('details')!;
    expect(within(how).getByText(SWEET_CAVEAT)).toBeInTheDocument();
    expect(within(how).getByText(RECONCILE_NOTE)).toBeInTheDocument();
    expect(within(how).getByText('#3f2a9c1b7d4e')).toBeInTheDocument();
    expect(screen.getAllByText(SWEET_CAVEAT)).toHaveLength(1);
    expect(screen.getAllByText(footnote(1), { exact: false })).toHaveLength(1);
    expect(numbers.open).toBe(false);
  });

  it('state panels replace chips and chart: missing, warming, under review (review focus 2)', async () => {
    mockApi({ '/api/cards': { win: '30d', cards: [] } });
    const a = renderPage();
    expect(await screen.findByText(SWEET_EMPTY)).toBeInTheDocument();
    a.unmount();

    const warm = card(rows, { status: 'warming', since_ms: CARD_NOW - 3 * 86_400_000 });
    mockApi({ '/api/cards': { win: '30d', cards: [warm] } });
    const b = renderPage();
    expect(await screen.findByText(warmingText(warm))).toBeInTheDocument();
    expect(screen.getByText('WARMING')).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'side' })).toBeNull();
    b.unmount();

    const review = card(rows, { status: 'under_review' });
    mockApi({ '/api/cards': { win: '30d', cards: [review] } });
    renderPage();
    expect(await screen.findByText(recomputingText(review.computed_ms))).toBeInTheDocument();
    expect(screen.getByText('UNDER REVIEW')).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: /sp-idle/ })).toBeNull();
  });
});
```

In `web/src/App.test.tsx` change the nav list to `['RADAR', 'SWEET', 'EPISODES', 'PAPER', 'PAPERS', 'HEALTH']`.

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/pages/Sweet.test.tsx src/App.test.tsx`
Expected: FAIL — cannot resolve `./Sweet`; App test can't find link `SWEET`.

- [ ] **Step 3: Implement**

Create `web/src/pages/Sweet.tsx` (the old `SweetSpots` table moves in as `SweetGrid`, unchanged apart from being inside a fold):
```tsx
import { useState } from 'react';
import { api } from '../api/client';
import type { CardBody, SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { Help } from '../components/Help';
import { IntervalChart } from '../components/IntervalChart';
import { Loadable } from '../components/Loadable';
import { Tag } from '../components/Tag';
import { Tiles } from '../components/Tiles';
import { type Win, WindowSwitch } from '../components/WindowSwitch';
import { computedText, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { formatMoney, formatPercent } from '../lib/money';
import {
  COLO_TITLE, DIMS, FEW_TRADES, HEADLINE_SCENARIO, LABEL_TAG, LABEL_TEXT, NOTHING_PAYS, ONE_LINE_CAVEAT, SWEET_CAVEAT,
  SWEET_EMPTY, type SweetDim, type SweetSide, bucketText, candidateCount, footnote, grid, interval, nothingPays,
  recomputingText, rowsFor,
} from '../lib/sweet';
import { fmtLocal, fmtUtcDateTime } from '../lib/time';

const scenarioTitle = (name: string) => (name === 'colo-idle' ? `${COLO_TITLE} · ${name}` : name);

function Chips<T extends string>({ label, options, value, onChange }: {
  label: string; options: [T, string][]; value: T; onChange: (v: T) => void;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={v} type="button" className={v === value ? 'on' : undefined} aria-pressed={v === value} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

/** The old Sweet spots table: every scenario side by side (redesign §5.1 "all numbers"). */
function SweetGrid({ s, side, dim }: { s: SweetSeries; side: SweetSide; dim: SweetDim }) {
  const g = grid(s, side, dim);
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            <th scope="col">{dim === 'all' ? '' : dim.toUpperCase()}</th>
            {g.scenarios.map((sc) => (
              <th key={sc.name} scope="col">
                {scenarioTitle(sc.name)}
                {sc.latency_ms === null ? '' : ` · ${sc.latency_ms} ms`}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {g.buckets.map((b) => (
            <tr key={b}>
              <th scope="row">{bucketText(dim, b)}</th>
              {g.scenarios.map((sc) => {
                const r = g.cell(b, sc.name);
                if (!r) return <td key={sc.name}>—</td>;
                const dimmed = r.label === 'thin' || r.label === 'not_tested';
                const alt = r.captured_leftover0 !== r.captured ? `leftovers at 0, mean: ${formatMoney(r.mean_leftover0, { signed: true })}` : undefined;
                return (
                  <td key={sc.name} className={dimmed ? 'dim num' : 'num'} title={alt}>
                    <Tag tone={r.label === 'candidate' ? 'no' : r.label === 'negative' || r.label === 'dead_on_paper' ? 'flag' : 'plain'} title={LABEL_TEXT[r.label]}>
                      {LABEL_TAG[r.label]}
                    </Tag>{' '}
                    {interval(r)}
                    <br />
                    <span className="dim">n {r.n_exec} · C {r.clusters}</span>
                    {r.side === 'yes' && r.annualized !== null && <span className="dim"> · {formatPercent(r.annualized)}/yr</span>}
                    {r.few_trades && (<><br /><span className="dim">{FEW_TRADES}</span></>)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SweetBody({ card, s }: { card: CardBody; s: SweetSeries }) {
  const nowMs = useNow(30_000);
  const [side, setSide] = useState<SweetSide>('no');
  const [dimAsked, setDim] = useState<SweetDim>('all');
  const [scenAsked, setScen] = useState<string>(HEADLINE_SCENARIO);
  const dims = DIMS.filter(([d]) => d !== 'category' || s.category_available);
  const dim = dimAsked === 'category' && !s.category_available ? 'all' : dimAsked;
  const scenarios = grid(s, side, dim).scenarios;
  // Review focus 3: a scenario with no rows for this side/slice falls back to the first one that has some.
  const scenario = scenarios.some((sc) => sc.name === scenAsked) ? scenAsked : (scenarios[0]?.name ?? HEADLINE_SCENARIO);
  const extraNotes = card.notes.filter((n) => n !== SWEET_CAVEAT && n !== footnote(s.m));
  return (
    <>
      <Tiles
        items={[
          { label: 'NO candidates', value: String(candidateCount(s, 'no')) },
          { label: 'YES candidates', value: String(candidateCount(s, 'yes')) },
          { label: 'rows tested', value: String(s.m) },
          { label: 'scenario', value: HEADLINE_SCENARIO },
        ]}
      />
      {nothingPays(s) && <p className="quip">{NOTHING_PAYS}</p>}
      <div className="filters">
        <Chips label="side" options={[['no', 'NO'], ['yes', 'YES']]} value={side} onChange={setSide} />
        {side === 'no' ? <Help id="sweetNo" /> : <Help id="sweetYes" />}
        <Chips label="dimension" options={dims} value={dim} onChange={setDim} />
        <Chips label="scenario" options={scenarios.map((sc) => [sc.name, scenarioTitle(sc.name).toUpperCase()] as [string, string])} value={scenario} onChange={setScen} />
      </div>
      <IntervalChart rows={rowsFor(s, side, dim, scenario)} label={`${side.toUpperCase()} · ${dim.toUpperCase()} · ${scenario}`} />
      <Fold id="sweet:numbers" label="all numbers">
        <SweetGrid s={s} side={side} dim={dim} />
      </Fold>
      <Fold id="sweet:how" label="how it's computed">
        <p>{SWEET_CAVEAT}</p>
        <p className="dim">
          {footnote(s.m)} z = {s.z}. <Help id="sweetBonferroni" /> <Help id="sweetThin" /> <Help id="sweetClusters" />
        </p>
        {extraNotes.map((n) => (
          <p key={n} className="note">{n}</p>
        ))}
        <p className="meta">
          <span title={card.hash}>#{shortHash(card.hash)}</span> ·{' '}
          <span title={fmtLocal(card.computed_ms)}>
            {computedText(card.computed_ms, nowMs)} ({fmtUtcDateTime(card.computed_ms)})
          </span>
        </p>
      </Fold>
    </>
  );
}

/** Redesign §5.1: Sweet spots on its own page, 30D by default. */
export function Sweet() {
  const [win, setWin] = useState<Win>('30d');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  const card = st.data?.cards.find((c) => c.card === 'SWEET');
  return (
    <Box
      title="SWEET SPOTS"
      help="sweet"
      hero
      actions={
        <>
          <WindowSwitch win={win} onChange={setWin} label="sweet spots window" />
          {card && <Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>}
        </>
      }
    >
      <p className="quip">{ONE_LINE_CAVEAT}</p>
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'SWEET');
          if (!c?.series.sweet) return <EmptyState text={SWEET_EMPTY} />;
          if (c.status === 'under_review') return <EmptyState text={recomputingText(c.computed_ms)} />;
          if (c.status === 'warming') return <EmptyState text={warmingText(c)} />;
          return <SweetBody card={c} s={c.series.sweet} />;
        }}
      </Loadable>
    </Box>
  );
}
```

Delete the old component and its test:
```bash
git rm web/src/components/SweetSpots.tsx web/src/components/SweetSpots.test.tsx
```

`web/src/pages/Paper.tsx`: remove `import { SweetSpots } from '../components/SweetSpots';` and the `<SweetSpots />` line in `Paper()`.

`web/src/App.tsx`: add `import { Sweet } from './pages/Sweet';`, insert `{ to: '/sweet', label: 'SWEET', end: false },` as the second `NAV` entry, and add `<Route path="/sweet" element={<Sweet />} />` after the `/` route.

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS. (If a Paper test asserted a Sweet spots heading, delete that assertion — Sweet spots no longer lives on `/paper`.)

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: /sweet page with interval chart, state panels and folds; Sweet spots leaves /paper

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Sweet spots card on Radar

**Files:**
- Create: `web/src/components/SweetCard.tsx`, `web/src/components/SweetCard.test.tsx`
- Modify: `web/src/pages/Radar.tsx`, `web/src/pages/Radar.test.tsx`

**Interfaces:**
- Consumes: Task 2 (`bestCandidate`, `candidateCount`, `rowName`), Task 3 (`IntervalChart`); `nothingPays`, `SWEET_EMPTY`, `interval`; `warmingText`.
- Produces: `SweetCard()`; exported copy `SWEET_CARD_TITLE = 'SWEET SPOTS · LAST 30 DAYS'`, `NOTHING_YET = 'Nothing pays on paper yet.'`, `RECOMPUTING = 'Recomputing…'`, `TOO_FEW = 'Too few trades to say anything yet.'`.

Note: the spec's "whole box is a link" is implemented as the title being the link: a `(?)` button inside a link would be invalid HTML.

- [ ] **Step 1: Write the failing tests**

`web/src/components/SweetCard.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { CardBody } from '../api/types';
import { warmingText } from '../lib/cards';
import { SWEET_EMPTY } from '../lib/sweet';
import { CARD_NOW, sweetFixture, sweetRow } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { NOTHING_YET, RECOMPUTING, SWEET_CARD_TITLE, SweetCard, TOO_FEW } from './SweetCard';

const renderCard = () => render(<MemoryRouter><SweetCard /></MemoryRouter>);
const serve = (cards: CardBody[]) => mockApi({ '/api/cards': (u: URL) => ({ win: u.searchParams.get('win'), cards }) });

describe('SweetCard', () => {
  it('links its title to /sweet and asks for 30d', async () => {
    const f = serve([]);
    renderCard();
    expect(await screen.findByRole('link', { name: SWEET_CARD_TITLE })).toHaveAttribute('href', '/sweet');
    expect(String(f.mock.calls[0][0])).toBe('/api/cards?win=30d');
  });

  it('shows counts and the best candidate as one interval row', async () => {
    serve([sweetFixture([
      sweetRow({ dimension: 'hour', bucket: '12-16', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58' }),
      sweetRow({ dimension: 'days', bucket: '1-7', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2' }),
    ])]);
    renderCard();
    expect(await screen.findByText('NO 2 · YES 0 candidates')).toBeInTheDocument();
    expect(screen.getByText('12–16 UTC')).toBeInTheDocument();
    expect(screen.getByText('+$0.31 [+$0.04, +$0.58]')).toBeInTheDocument();
  });

  it('says nothing pays, too few, recomputing, warming, or missing', async () => {
    const cases: [CardBody[], string][] = [
      [[sweetFixture([sweetRow({ label: 'negative', lo: '-1', mean: '-0.5', hi: '-0.1' })])], NOTHING_YET],
      [[sweetFixture([sweetRow({ label: 'thin' })])], TOO_FEW],
      [[{ ...sweetFixture([sweetRow()]), status: 'under_review' }], RECOMPUTING],
      [[], SWEET_EMPTY],
    ];
    for (const [cards, text] of cases) {
      serve(cards);
      const r = renderCard();
      expect(await screen.findByText(text)).toBeInTheDocument();
      r.unmount();
    }
    const warm = { ...sweetFixture([sweetRow()]), status: 'warming' as const, since_ms: CARD_NOW - 2 * 86_400_000 };
    serve([warm]);
    renderCard();
    expect(await screen.findByText(warmingText(warm))).toBeInTheDocument();
  });
});
```

In `web/src/pages/Radar.test.tsx`, test "puts TODAY above the open episodes": change the expectation to
`expect(h2s()).toEqual(['TODAY (UTC)', 'SWEET SPOTS · LAST 30 DAYS', 'ARBITRAGE (YES / NO) · 0', 'NOT ARBITRAGE: INCOMPLETE SETS · 0']);`

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/SweetCard.test.tsx src/pages/Radar.test.tsx`
Expected: FAIL — cannot resolve `./SweetCard`; Radar heading list mismatch.

- [ ] **Step 3: Implement**

`web/src/components/SweetCard.tsx`:
```tsx
import { Link } from 'react-router';
import { api } from '../api/client';
import type { SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { warmingText } from '../lib/cards';
import { useTicker } from '../lib/hooks';
import { SWEET_EMPTY, bestCandidate, candidateCount, nothingPays } from '../lib/sweet';
import { Box } from './Box';
import { EmptyState } from './EmptyState';
import { IntervalChart } from './IntervalChart';
import { Loadable } from './Loadable';

export const SWEET_CARD_TITLE = 'SWEET SPOTS · LAST 30 DAYS';
export const NOTHING_YET = 'Nothing pays on paper yet.';
export const RECOMPUTING = 'Recomputing…';
export const TOO_FEW = 'Too few trades to say anything yet.';

function Candidates({ s }: { s: SweetSeries }) {
  const best = bestCandidate(s);
  if (best === null) return <EmptyState text={nothingPays(s) ? NOTHING_YET : TOO_FEW} />;
  return (
    <>
      <p className="quip">{`NO ${candidateCount(s, 'no')} · YES ${candidateCount(s, 'yes')} candidates`}</p>
      <IntervalChart rows={[best]} label="best candidate" />
    </>
  );
}

/** Redesign §5.2: Sweet spots in one glance on Radar; the title opens /sweet. */
export function SweetCard() {
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards('30d', s), [], [slow]);
  return (
    <Box title={<Link to="/sweet">{SWEET_CARD_TITLE}</Link>} help="sweet">
      <Loadable state={st} what="sweet spots">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'SWEET');
          if (!c?.series.sweet) return <EmptyState text={SWEET_EMPTY} />;
          if (c.status === 'under_review') return <EmptyState text={RECOMPUTING} />;
          if (c.status === 'warming') return <EmptyState text={warmingText(c)} />;
          return <Candidates s={c.series.sweet} />;
        }}
      </Loadable>
    </Box>
  );
}
```

`web/src/pages/Radar.tsx`: `import { SweetCard } from '../components/SweetCard';` and render `<SweetCard />` directly after the `TODAY (UTC)` `</Box>`.

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/SweetCard.tsx web/src/components/SweetCard.test.tsx web/src/pages/Radar.tsx web/src/pages/Radar.test.tsx
git commit -m "web: Sweet spots card on Radar, linking to /sweet

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: LatencyAxis and SplitBar (replace DecayChart)

**Files:**
- Create: `web/src/components/LatencyAxis.tsx`, `web/src/components/LatencyAxis.test.tsx`, `web/src/components/SplitBar.tsx`, `web/src/components/SplitBar.test.tsx`
- Delete: `web/src/components/DecayChart.tsx`, `web/src/components/DecayChart.test.tsx`
- Modify: `web/src/components/CardCharts.tsx`, `web/src/theme.css`, `web/src/pages/Paper.test.tsx`, `web/src/components/EdgeCapture.test.tsx`

**Interfaces:**
- Consumes: `DecayPoint` (`api/types.ts`); `formatMoneyCents`.
- Produces:
  - `LatencyAxis({ points, measured }: { points: DecayPoint[]; measured: { latency_ms: number; host: string } | null })` — a `<figure aria-label="capture rate by latency">`; one `<li data-scenario>` per point with text `` `${scenario} · ${latency_ms} ms → ${rate}` `` (rate `${rate_pct}%` or `—`); `rect.dot` (class `head` for the headline point, `hollow` when rate is null); `rect.measured` in every row at the same x; a caption `measured: ${host} ≈ ${ms} ms (an estimate)`.
  - `SplitBar({ parts, label }: { parts: { name: string; value: string | null; tone: 'p0' | 'p3' }[]; label: string })`

- [ ] **Step 1: Write the failing tests**

`web/src/components/LatencyAxis.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { edgeFixture } from '../test/fixtures';
import { LatencyAxis } from './LatencyAxis';

const pts = edgeFixture().series.decay!;
const measured = { latency_ms: 200, host: 'vps-kvm2' };
const x = (el: Element | null) => Number(el?.getAttribute('x'));

describe('LatencyAxis', () => {
  it('puts each scenario on its own row at its latency, with its rate', () => {
    const { container } = render(<LatencyAxis points={pts} measured={measured} />);
    expect(screen.getByRole('figure', { name: 'capture rate by latency' })).toBeInTheDocument();
    expect(screen.getByText('colo-idle · 52 ms → 24.8%')).toBeInTheDocument();
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-burst · 450 ms → —')).toBeInTheDocument();
    const dot = (s: string) => container.querySelector(`li[data-scenario="${s}"] rect.dot`);
    expect(x(dot('colo-idle'))).toBeLessThan(x(dot('sp-idle')));
    expect(x(dot('sp-idle'))).toBeLessThan(x(dot('sp-burst')));
    expect(dot('sp-idle')).toHaveClass('head');
    expect(dot('sp-burst')).toHaveClass('hollow');
  });

  it('labels the measured latency apart from the scenarios, at one x on every row', () => {
    const { container } = render(<LatencyAxis points={pts} measured={measured} />);
    expect(screen.getByText('measured: vps-kvm2 ≈ 200 ms (an estimate)')).toBeInTheDocument();
    const xs = [...container.querySelectorAll('rect.measured')].map(x);
    expect(xs).toHaveLength(pts.length);
    expect(new Set(xs).size).toBe(1);
  });

  it('stacks scenarios that share a latency on separate rows (sp-idle and sp-idle-nodelay)', () => {
    const both = [...pts, { ...pts[1], scenario: 'sp-idle-nodelay', headline: false }];
    render(<LatencyAxis points={both} measured={null} />);
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-idle-nodelay · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.queryByText(/^measured:/)).toBeNull();
  });
});
```

`web/src/components/SplitBar.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SplitBar } from './SplitBar';

describe('SplitBar', () => {
  it('sizes each part by its share and writes its value', () => {
    const { container } = render(
      <SplitBar label="on the table, by side" parts={[{ name: 'NO cashable', value: '51.49', tone: 'p0' }, { name: 'YES locked', value: '206.52', tone: 'p3' }]} />,
    );
    expect(screen.getByRole('figure', { name: 'on the table, by side' })).toBeInTheDocument();
    expect(screen.getByText('NO cashable $51.49')).toBeInTheDocument();
    expect(screen.getByText('YES locked $206.52')).toBeInTheDocument();
    const [a, b] = [...container.querySelectorAll('.split-part')] as HTMLElement[];
    expect(a.style.flexGrow).toBe('51.49');
    expect(b.style.flexGrow).toBe('206.52');
  });

  it('draws nothing but the labels when both parts are empty or missing', () => {
    const { container } = render(<SplitBar label="x" parts={[{ name: 'NO', value: null, tone: 'p0' }, { name: 'YES', value: '0', tone: 'p3' }]} />);
    expect(container.querySelectorAll('.split-part')).toHaveLength(0);
    expect(screen.getByText('NO —')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/LatencyAxis.test.tsx src/components/SplitBar.test.tsx`
Expected: FAIL — unresolved imports.

- [ ] **Step 3: Implement**

`web/src/components/LatencyAxis.tsx`:
```tsx
import type { DecayPoint } from '../api/types';

const W = 300;
const H = 10;

const rateText = (p: DecayPoint) => (p.rate_pct === null ? '—' : `${p.rate_pct}%`);

/**
 * Redesign §4.2: one latency axis, one row per scenario (so equal latencies never overlap), and the
 * measured latency as a separate marker with its own caption. Scenario latency and measured latency
 * are different things and are labelled so. Numbers are geometry only; text is the string sent.
 */
export function LatencyAxis({ points, measured }: { points: DecayPoint[]; measured: { latency_ms: number; host: string } | null }) {
  const rows = [...points].sort((a, b) => a.latency_ms - b.latency_ms || a.scenario.localeCompare(b.scenario));
  const xMax = Math.max(1, ...rows.map((p) => p.latency_ms), measured?.latency_ms ?? 0) * 1.15;
  const sx = (ms: number) => 1 + Math.round((ms / xMax) * (W - 4));
  return (
    <figure className="latency" aria-label="capture rate by latency">
      <ul className="plain">
        {rows.map((p) => (
          <li key={p.scenario} className="lat-row" data-scenario={p.scenario}>
            <svg className="lat-track" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
              <rect className="axis" x={0} y={H / 2} width={W} height={1} />
              {measured && <rect className="measured" x={sx(measured.latency_ms)} y={0} width={1} height={H} />}
              <rect
                className={['dot', p.headline ? 'head' : null, p.rate_pct === null ? 'hollow' : null].filter(Boolean).join(' ')}
                x={sx(p.latency_ms) - 2}
                y={1}
                width={5}
                height={H - 2}
              />
            </svg>
            <span className={p.headline ? 'lat-label head' : 'lat-label'}>{`${p.scenario} · ${p.latency_ms} ms → ${rateText(p)}`}</span>
          </li>
        ))}
      </ul>
      <div className="chart-axis">
        <span>0 ms</span>
        <span>{`${Math.round(xMax)} ms`}</span>
      </div>
      {measured && <figcaption className="dim">{`measured: ${measured.host} ≈ ${measured.latency_ms} ms (an estimate)`}</figcaption>}
    </figure>
  );
}
```

`web/src/components/SplitBar.tsx`:
```tsx
import { formatMoneyCents } from '../lib/money';

export interface SplitPart {
  name: string;
  value: string | null;
  tone: 'p0' | 'p3';
}

/** Redesign §4.2: the edge on the table split by side, values written on the bar's ends. */
export function SplitBar({ parts, label }: { parts: SplitPart[]; label: string }) {
  const sized = parts.filter((p) => p.value !== null && Number(p.value) > 0);
  return (
    <figure className="split" aria-label={label}>
      <div className="split-labels">
        {parts.map((p) => (
          <span key={p.name} className="num">{`${p.name} ${p.value === null ? '—' : formatMoneyCents(p.value)}`}</span>
        ))}
      </div>
      {sized.length > 0 && (
        <div className="split-bar">
          {sized.map((p) => (
            <span key={p.name} className={`split-part sw ${p.tone}`} style={{ flexGrow: Number(p.value) }} />
          ))}
        </div>
      )}
    </figure>
  );
}
```

`web/src/components/CardCharts.tsx`: replace `import { DecayChart } from './DecayChart';` with `import { LatencyAxis } from './LatencyAxis';` and the decay line with
`if (s.decay && s.decay.length > 0) return <LatencyAxis points={s.decay} measured={s.measured ?? null} />;`

Delete: `git rm web/src/components/DecayChart.tsx web/src/components/DecayChart.test.tsx`

Update tests that looked for the old SVG role:
- `web/src/pages/Paper.test.tsx` ("puts the Edge capture box at the top"): `findByRole('img', { name: 'capture rate by latency' })` → `findByRole('figure', { name: 'capture rate by latency' })`.
- `web/src/components/EdgeCapture.test.tsx`: same replacement wherever it appears.

In `web/src/theme.css` remove the `svg.decay`, `.decay .pt`, `.decay .pt.head` rules and append:
```css
.latency { display: grid; gap: 2px; margin: 8px 0; }
.lat-row { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 1.2fr); gap: 10px; align-items: center; }
.lat-track { display: block; width: 100%; height: 16px; }
.lat-track .axis { fill: var(--line2); }
.lat-track .measured { fill: var(--salmon); }
.lat-track .dot { fill: var(--teal); }
.lat-track .dot.head { fill: var(--mint); }
.lat-track .dot.hollow { fill: var(--bg); stroke: var(--teal); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.lat-label.head { font-weight: 700; }
.split { display: grid; gap: 4px; margin: 8px 0; }
.split-labels { display: flex; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
.split-bar { display: flex; height: 14px; border: 2px solid var(--line2); }
.split-part { display: block; height: 100%; width: auto; margin: 0; }
@media (max-width: 600px) { .lat-row { grid-template-columns: 1fr; } }
```

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: latency axis (one row per scenario, measured marker labelled apart) and split bar; drop DecayChart

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: CardView and Edge capture in the section pattern

**Files:**
- Modify: `web/src/components/CardView.tsx`, `web/src/components/EdgeCapture.tsx`, `web/src/components/CardView.test.tsx`, `web/src/components/EdgeCapture.test.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`Box` `hero`/`actions`, `Tiles`, `Fold`), Task 6 (`LatencyAxis`, `SplitBar`); `ON_TABLE`, `fmtValue`, `withThinEdge`, `statusLabel`, `statusTone`, `warmingText`, `UNDER_REVIEW` (`lib/cards.ts`); `signOf`, `formatMoney`, `formatMoneyCents`.
- Produces (exported from `CardView.tsx`):
  - `CardTable({ card, stale }: { card: CardBody; stale: boolean })` — the full table (unchanged markup, value cells get class `num`)
  - `CardMeta({ card, nowMs }: { card: CardBody; nowMs: number })` — n/hash/computed line, then premise (`p.meta.premise`) immediately after it, thin reasons, notes
  - `Claim({ card }: { card: CardBody })` — the first row in big type as `<dl className="claim">` with `paper` / `theirs` / `ours` labels
  - `CardView` = header, status text, chart, `Claim`, `Fold "all numbers"` (`CardTable`), episode link, `Fold "how it's computed"` (`CardMeta`). Fold ids: `card:<card.card>:numbers`, `card:<card.card>:how`.

- [ ] **Step 1: Update the tests first**

`web/src/components/CardView.test.tsx`:
- first test: `within(card).getByText('2,098')` → `expect(within(card).getAllByText('2,098').length).toBe(2);` (claim + table). Add:
```tsx
    const claim = card.querySelector('dl.claim') as HTMLElement;
    expect(within(claim).getByText('paper')).toBeInTheDocument();
    expect(within(claim).getByText('ours')).toBeInTheDocument();
    expect(within(card).getByText('all numbers').closest('details')).not.toHaveAttribute('open');
    expect(within(card).getByText("how it's computed").closest('details')).not.toHaveAttribute('open');
```
- thin test: the `'—'` and `'0'` counts include the claim row. The first row of that fixture is `NO-side episodes | 36 | 0 | 0`, so change `expect(screen.getAllByText('0')).toHaveLength(2)` to `toHaveLength(4)` and keep `'—'` at 2 (the claim shows row 1 only, which has no nulls).

`web/src/components/EdgeCapture.test.tsx`, first test — the split bar also prints `$9.25`, and the tiles print the headline amounts. Replace the money assertions with:
```tsx
    // drop the old `getByText('$9.25')` line: the table and the split bar both print it now
    expect(screen.getByText('NO cashable $9.25')).toBeInTheDocument();
    expect(screen.getAllByText('$180.00')).toHaveLength(1); // capital needed: table only
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles).toEqual(['edge on the table$12.50', 'captured at sp-idle (145 ms)$0.40', 'capture rate, sp-idle3.2%', 'measured latency (estimate)200 ms']);
```
Add a thin-edge test (review focus 4):
```tsx
  it('shows — for every rate and hollow dots when the edge on the table is thin', async () => {
    const thin = edgeFixture({ win: '24h' });
    thin.columns = thin.columns.map((r) => (r.label === 'edge on the table' ? { ...r, ours: '0.5' } : r));
    mockApi({ '/api/cards': { win: '24h', cards: [thin] }, '/api/cards/meta': metaFixture() });
    const { container } = renderBox();
    expect(await screen.findByText('sp-idle · 300 ms → —')).toBeInTheDocument();
    expect(container.querySelectorAll('rect.dot.hollow')).toHaveLength(3);
    expect(screen.getAllByRole('listitem').find((li) => li.textContent?.startsWith('capture rate, sp-idle'))?.textContent).toBe('capture rate, sp-idle—');
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/CardView.test.tsx src/components/EdgeCapture.test.tsx`
Expected: FAIL (no `dl.claim`, no tiles, no split labels).

- [ ] **Step 3: Implement**

`web/src/components/CardView.tsx` — keep `CARD_HELP`, `COLS`, `ValueCell`; replace `CardView` with:
```tsx
const CLAIM_LABEL: Record<Col, string> = { paper: 'paper', their: 'theirs', ours: 'ours' };

function usedCols(card: CardBody) {
  const used = COLS.filter((c) => card.columns.some((r: CardRow) => r[c.key] !== null));
  return used.length > 0 ? used : COLS.slice(2);
}

/** Redesign §4.3: the card's headline claim (its first row) in big type, PAPER / THEIRS / OURS. */
export function Claim({ card }: { card: CardBody }) {
  const r = card.columns[0];
  if (!r) return null;
  return (
    <div className="claim-wrap">
      <p className="dim">{r.label}</p>
      <dl className="claim">
        {usedCols(card).map((c) => (
          <div key={c.key}>
            <dt>{CLAIM_LABEL[c.key]}</dt>
            <dd>
              <ValueCell value={r[c.key]} unit={r.unit} />
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export function CardTable({ card, stale }: { card: CardBody; stale: boolean }) {
  const dim = card.status === 'thin' || card.status === 'under_review' || stale;
  const cols = usedCols(card);
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            <th scope="col" aria-label="measure" />
            {cols.map((c) => (
              <th key={c.key} scope="col">
                {cols.length === 1 ? 'VALUE' : c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {card.columns.map((r) => (
            <tr key={r.label} className={dim ? 'dim' : undefined}>
              <td className="rowlabel">{r.label}</td>
              {cols.map((c) => (
                <td key={c.key} className="num">
                  <ValueCell value={r[c.key]} unit={r.unit} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function CardMeta({ card, nowMs }: { card: CardBody; nowMs: number }) {
  return (
    <>
      <p className="meta">
        n = {card.n} · <span title={card.hash}>#{shortHash(card.hash)}</span> ·{' '}
        <span title={fmtLocal(card.computed_ms)}>
          {computedText(card.computed_ms, nowMs)} ({fmtUtcDateTime(card.computed_ms)})
        </span>
      </p>
      <p className="meta premise">{card.premise_key}</p>
      {card.thin_reasons.map((t) => (
        <p key={t} className="dim">thin: {t}</p>
      ))}
      {card.notes.map((n) => (
        <p key={n} className="note">{n}</p>
      ))}
    </>
  );
}

/** One card (redesign §4.3): header, status, drawing, claim; the table and the method folded. */
export function CardView({ card, stale = false, now }: { card: CardBody; stale?: boolean; now?: number }) {
  const ticking = useNow(30_000);
  const nowMs = now ?? ticking;
  return (
    <article className="cardv" aria-label={card.title}>
      <div className="box-head">
        <h3>{card.title}</h3>
        <Tag tone={statusTone(card.status)}>{statusLabel(card.status)}</Tag>
        <Help id={CARD_HELP[card.card] ?? 'status'} />
      </div>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <CardChart card={card} />
      <Claim card={card} />
      <Fold id={`card:${card.card}:numbers`} label="all numbers">
        <CardTable card={card} stale={stale} />
      </Fold>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      <Fold id={`card:${card.card}:how`} label="how it's computed">
        <CardMeta card={card} nowMs={nowMs} />
      </Fold>
    </article>
  );
}
```
Add `import { Fold } from './Fold';` to its imports. (The `CardView.test` "premise next to n" test still holds: `CardMeta` keeps the two paragraphs adjacent.)

`web/src/components/EdgeCapture.tsx` — replace the component body:
```tsx
import { useState } from 'react';
import { Link } from 'react-router';
import { api } from '../api/client';
import type { CardBody, CardRow } from '../api/types';
import { useApi } from '../api/useApi';
import { ON_TABLE, UNDER_REVIEW, fmtValue, statusLabel, statusTone, updatedText, warmingText, withThinEdge } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { formatMoney, formatMoneyCents, signOf } from '../lib/money';
import { Box } from './Box';
import { CardMeta, CardTable } from './CardView';
import { EmptyState } from './EmptyState';
import { Fold } from './Fold';
import { LatencyAxis } from './LatencyAxis';
import { Loadable } from './Loadable';
import { SplitBar } from './SplitBar';
import { Tag } from './Tag';
import { type Tile, Tiles } from './Tiles';
import { type Win, WindowSwitch } from './WindowSwitch';

export const EDGE_SUBTITLE = 'The difference is physics and other people.';
export const EDGE_EMPTY = 'No Edge capture yet. analyze has not finished a tick under these premises.';

const row = (c: CardBody, starts: string) => c.columns.find((r) => r.label.startsWith(starts));

function tile(r: CardRow | undefined): Tile[] {
  if (!r) return [];
  const s = signOf(r.ours);
  const money = r.unit === 'USD';
  return [{
    label: r.label,
    value: money ? formatMoneyCents(r.ours) : fmtValue(r.ours, r.unit),
    title: money && r.ours !== null ? formatMoney(r.ours) : undefined,
    tone: s < 0 ? 'neg' : undefined,
  }];
}

function EdgeBody({ card, stale }: { card: CardBody; stale: boolean }) {
  const nowMs = useNow(30_000);
  return (
    <>
      {card.status === 'warming' && <p className="dim">{warmingText(card)}</p>}
      {card.status === 'under_review' && <p className="dim">{UNDER_REVIEW}</p>}
      <Tiles items={[...tile(row(card, ON_TABLE)), ...tile(row(card, 'captured at')), ...tile(row(card, 'capture rate')), ...tile(row(card, 'measured latency'))]} />
      {card.series.decay && card.series.decay.length > 0 && <LatencyAxis points={card.series.decay} measured={card.series.measured ?? null} />}
      <SplitBar
        label="on the table, by side"
        parts={[
          { name: 'NO cashable', value: row(card, 'NO side')?.ours ?? null, tone: 'p0' },
          { name: 'YES locked', value: row(card, 'YES side')?.ours ?? null, tone: 'p3' },
        ]}
      />
      <Fold id="edge:numbers" label="all numbers">
        <CardTable card={card} stale={stale} />
      </Fold>
      {card.episode_link && (
        <p>
          <Link to={`/episodes/${encodeURIComponent(card.episode_link)}`}>biggest opportunity →</Link>
        </p>
      )}
      <Fold id="edge:how" label="how it's computed">
        <CardMeta card={card} nowMs={nowMs} />
      </Fold>
    </>
  );
}

/** Redesign §4.2: Edge capture as tiles, the latency axis and the NO/YES split; details folded. */
export function EdgeCapture() {
  const [win, setWin] = useState<Win>('24h');
  const slow = useTicker(60_000);
  const st = useApi((s) => api.cards(win, s), [win], [slow]);
  const meta = useApi((s) => api.cardsMeta(s), [], [slow]);
  const now = useNow(30_000);
  const stale = meta.data !== undefined && updatedText(meta.data.tick?.last_tick_ms ?? null, now).stale;
  const edge = st.data?.cards.find((x) => x.card === 'EDGE');
  return (
    <Box
      title="EDGE CAPTURE"
      help="edge"
      hero
      actions={
        <>
          <WindowSwitch win={win} onChange={setWin} />
          {edge && <Tag tone={statusTone(edge.status)}>{statusLabel(edge.status)}</Tag>}
        </>
      }
    >
      <p className="quip">{EDGE_SUBTITLE}</p>
      <Loadable state={st} what="edge capture">
        {(d) => {
          const c = d.cards.find((x) => x.card === 'EDGE');
          return c ? <EdgeBody card={withThinEdge(c)} stale={stale} /> : <EmptyState text={EDGE_EMPTY} />;
        }}
      </Loadable>
    </Box>
  );
}
```
If `EdgeCapture.test.tsx` asserted the `(?)` "Why is the NO side cashable and the YES side locked?", check where that Help was rendered (grep `help.ts` for that title): if it came from a note/help inside `CardView` for EDGE, add `<Help id="<that id>" />` next to the `SplitBar` so the assertion keeps passing.

Append to `web/src/theme.css`:
```css
.claim-wrap { display: grid; gap: 2px; }
.claim { display: flex; gap: 18px; flex-wrap: wrap; }
.claim dt { color: var(--dim); font-size: 13px; }
.claim dd { font-family: var(--big); font-size: 30px; line-height: 1; white-space: nowrap; }
.cardv { border: 2px solid var(--line2); }
```

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS. Any remaining failure is a `getByText` that now matches a tile *and* a folded table cell — switch it to `getAllByText(...)` and assert the count.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: cards show chart and claim with table and method folded; Edge capture as tiles, latency axis and split bar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Paper › Accounts section

**Files:**
- Modify: `web/src/pages/Paper.tsx`, `web/src/components/LineChart.tsx`, `web/src/pages/Paper.test.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`Box` `actions`, `Fold`); existing `policyLabel`, `policyCardTitle`, `equityAxis`, `extremes`, `Ledger`, `positionColumns`.
- Produces: `LineChart` gains `legend?: boolean` (default `true`) and `markerLabel?: string`; Paper renders `ACCOUNTS · $1,000 EACH · LAST 24 H` (`help="equity"`) with scenario chips in `actions` (group label `latency scenario`), the chart with an end legend (`ul.endlegend`, one `li` per line: swatch, label, last equity via `formatMoneyCents`), then folds `paper:accounts` "all numbers", `paper:ledger` "ledger", `paper:awaiting` "awaiting resolution". The separate SCENARIO box, EQUITY box and `Cards` are removed.

- [ ] **Step 1: Update the tests first** (`web/src/pages/Paper.test.tsx`)

- "switches scenario…": `expect(screen.getByText('$1,002.56'))` stays (end legend only; the accounts table has no equity column). `'−$286.60'` stays (accounts table, `<Money tone>`). Add:
```tsx
    const legend = screen.getByRole('list', { name: 'equity now' });
    expect(within(legend).getByText('P3 naive')).toBeInTheDocument();
    expect(within(legend).getByText('$713.40')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: /^ACCOUNTS/ })).toBeInTheDocument();
    for (const f of ['all numbers', 'ledger', 'awaiting resolution']) expect(screen.getByText(f).closest('details')).not.toHaveAttribute('open');
```
(add `within` to the testing-library import).
- "can hide the P3 control line": the last line becomes `expect(screen.getAllByText('P3 · NAIVE').length).toBeGreaterThan(0); // its accounts row and ledger button stay`.
- The ledger test clicks `OLDER →` inside a closed fold. Before that click add `await userEvent.click(screen.getByText('ledger'));`.

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/pages/Paper.test.tsx`
Expected: FAIL — no `equity now` list, no `ACCOUNTS` heading.

- [ ] **Step 3: Implement**

`web/src/components/LineChart.tsx`: add the props `legend = true` and `markerLabel` (type `legend?: boolean; markerLabel?: string;`) and replace the legend `<div>` with:
```tsx
      {(legend || markerLabel) && (
        <div className="legend">
          {legend &&
            series.map((s) => (
              <span key={s.label}>
                <span className={`sw ${s.tone}`} aria-hidden="true" />
                {s.label}
              </span>
            ))}
          {markerLabel && (
            <span>
              <span className="sw mk" aria-hidden="true" />
              {markerLabel}
            </span>
          )}
        </div>
      )}
```

`web/src/pages/Paper.tsx`:
1. Delete `Cards`. Replace `EquityChart` with an `AccountsChart` that returns the chart content (no `Box`):
```tsx
function AccountsChart({ accounts }: { accounts: PaperAccount[] }) {
  const ids = accounts.map((a) => a.account_id);
  const eq = useApi(
    (s) => Promise.all(accounts.map((a) => api.paperEquity(a.account_id, 24, s).then((e) => ({ a, e })))),
    [ids.join('|')],
  );
  const [hideControl, setHideControl] = useState(false);
  const hasControl = accounts.some((a) => a.policy === CONTROL);
  return (
    <>
      {hasControl && (
        <label className="check">
          <input type="checkbox" checked={hideControl} onChange={(e) => setHideControl(e.target.checked)} />
          hide P3 (control)
        </label>
      )}
      <Loadable state={eq} what="equity">
        {(loaded) => {
          const all = loaded.filter(({ a }) => ids.includes(a.account_id) && !(hideControl && a.policy === CONTROL));
          const series: Series[] = all.map(({ a, e }) => ({
            label: policyLabel(a),
            tone: TONES[a.policy] ?? 'p0',
            points: e.points.map((p) => ({ x: p.t_ms, y: Number(p.equity) })),
          }));
          const ext = extremes(all.flatMap(({ e }) => e.points.map((p) => p.equity)));
          const axis = equityAxis(
            all.flatMap(({ e }) => e.points.map((p) => p.t_ms)),
            Math.min(...loaded.map(({ e }) => e.from_ms)),
            Math.max(...loaded.map(({ e }) => e.to_ms)),
          );
          const ends = all.filter(({ e }) => e.points.length > 0);
          return (
            <>
              <div className="eqwrap">
                <LineChart label="equity per account" xMin={axis.xMin} xMax={axis.xMax} xLabels={axis.xLabels} series={series} legend={false}
                  yLabels={ext ? { bottom: formatMoney(ext.min), top: formatMoney(ext.max) } : undefined} />
                {ends.length > 0 && (
                  <ul className="plain endlegend" aria-label="equity now">
                    {ends.map(({ a, e }) => (
                      <li key={a.account_id}>
                        <span className={`sw ${TONES[a.policy] ?? 'p0'}`} aria-hidden="true" />
                        <span>{policyLabel(a)}</span> <b className="num">{formatMoneyCents(e.points[e.points.length - 1].equity)}</b>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              {hasControl && <p className="quip">{PAPER_QUIP}</p>}
              <p className="quip">{PAPER_LOSS_QUIP}</p>
            </>
          );
        }}
      </Loadable>
    </>
  );
}

function AccountsTable({ accounts }: { accounts: PaperAccount[] }) {
  return (
    <div className="scroll">
      <table>
        <thead>
          <tr>
            {['ACCOUNT', 'REALIZED', 'UNREALIZED', 'LOCKED', 'MAX DD', 'OPEN'].map((h) => <th key={h} scope="col">{h}</th>)}
          </tr>
        </thead>
        <tbody>
          {accounts.map((a) => (
            <tr key={a.account_id}>
              <td>{policyCardTitle(a.policy)}{!isZero(a.fee_bips) && ` · FEE ${a.fee_bips} BP`}</td>
              {a.marked_ms === null ? (
                <td colSpan={5} className="dim">No marks yet. Nothing ventured, nothing lost.</td>
              ) : (
                <>
                  <td className="num"><Money value={a.realized} signed tone cents /></td>
                  <td className="num"><Money value={a.unrealized} signed tone cents /></td>
                  <td className="num"><Money value={a.locked} cents /></td>
                  <td className="num"><Money value={a.max_drawdown} cents /></td>
                  <td className="num">{a.n_positions === null ? '—' : a.n_positions}</td>
                </>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
```
Add `formatMoneyCents` to the `../lib/money` import and `Fold` from `../components/Fold`.
2. `Ledger`: replace its outer `<Box title=… help="ledger">…</Box>` with a fragment `<>…</>` (keep the account chips, the marks checkbox, the table and the pager).
3. `PaperBody` returns:
```tsx
    <Box
      title={`ACCOUNTS · ${formatMoney(inScenario[0]?.bankroll ?? null)} EACH · LAST 24 H`}
      help="equity"
      actions={
        <div className="seg" role="group" aria-label="latency scenario">
          {scenarios.map((s) => (
            <button key={s} type="button" className={s === scenario ? 'on' : undefined} aria-pressed={s === scenario} onClick={() => set({ scenario: s, account: null, lcursor: null })}>
              {s.toUpperCase()}
            </button>
          ))}
        </div>
      }
    >
      <AccountsChart key={scenario} accounts={inScenario} />
      <Fold id="paper:accounts" label="all numbers">
        <AccountsTable accounts={inScenario} />
      </Fold>
      <Fold id="paper:ledger" label="ledger">
        {account !== undefined && (
          <Ledger accounts={inScenario} account={account} cursor={sp.get('lcursor')} marks={sp.get('marks') === '1'}
            onAccount={(id) => set({ account: id, lcursor: null })}
            onCursor={(c) => set({ lcursor: c })}
            onMarks={(show) => set({ marks: show ? '1' : null, lcursor: null })} />
        )}
      </Fold>
      <Fold id="paper:awaiting" label="awaiting resolution">
        <Loadable state={positions} what="positions">
          {(d) => {
            const rows = d.items.filter((p) => ids.includes(p.account_id));
            return rows.length === 0 ? <EmptyState text={NOTHING_AWAITING} /> : (
              <Table columns={positionColumns} rows={rows} rowKey={(p) => `${p.account_id}|${p.market_id}|${p.outcome}|${p.returned}`} />
            );
          }}
        </Loadable>
      </Fold>
    </Box>
```

Append to `web/src/theme.css`:
```css
.eqwrap { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; align-items: center; }
.endlegend { gap: 4px; }
.endlegend li { white-space: nowrap; }
.sw.mk { background: var(--line2); }
@media (max-width: 600px) { .eqwrap { grid-template-columns: 1fr; } }
```

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: Paper accounts as one section (scenario chips in the title, equity with end values, ledger and positions folded)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Episodes

**Files:**
- Create: `web/src/components/EpisodeStrip.tsx`, `web/src/components/EpisodeStrip.test.tsx`
- Modify: `web/src/pages/Episodes.tsx`, `web/src/pages/Episodes.test.tsx`, `web/src/lib/time.ts`, `web/src/lib/time.test.ts`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`Box` `hero`, `Fold`); `EpisodeSummary`; `kindTag`, `FLAG_WORDS`, `CENSORED_WORDS`; `formatMoneyCents`.
- Produces:
  - `fmtUtcHms(ms): string` → `"17:32:11"`; `fmtUtcDayMonth(ms): string` → `"27 SEP"`
  - `EpisodeStrip({ rows }: { rows: EpisodeSummary[] })` — `<figure aria-label="N episodes in view, K net-positive">`, one `rect.tick` per row (`hollow` when suspect/stale/censored; tone class `no` / `yes` / `other`), caption `N in view · K net-positive`
  - Episodes: one `EPISODES` hero box; filters line with a `flags` disclosure; strip; grouped table (`tr.day > th[colSpan]` per UTC day); flag chips `!` (suspect), `S` (stale), `C` (censored) with the same `title`s as before; legend in `Fold "what the flags mean"`.

- [ ] **Step 1: Write the failing tests**

Append to `web/src/lib/time.test.ts` (import the two new names):
```ts
it('formats a UTC clock time and a day heading', () => {
  const t = Date.UTC(2026, 8, 27, 17, 32, 11, 450);
  expect(fmtUtcHms(t)).toBe('17:32:11');
  expect(fmtUtcDayMonth(t)).toBe('27 SEP');
});
```

`web/src/components/EpisodeStrip.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture } from '../test/fixtures';
import { EpisodeStrip } from './EpisodeStrip';

describe('EpisodeStrip', () => {
  it('draws one tick per episode, hollow when flagged, and counts net-positive', () => {
    const rows = [
      episodeSummaryFixture({ id: 'a', opened_ms: 1000, net: '0.41' }),
      episodeSummaryFixture({ id: 'b', opened_ms: 5000, net: '0', kind: 'yes_basket', stale: true }),
      episodeSummaryFixture({ id: 'c', opened_ms: 9000, net: '-0.2', kind: 'implication' }),
    ];
    const { container } = render(<EpisodeStrip rows={rows} />);
    expect(screen.getByRole('figure', { name: '3 episodes in view, 1 net-positive' })).toBeInTheDocument();
    expect(screen.getByText('3 in view · 1 net-positive')).toBeInTheDocument();
    const ticks = container.querySelectorAll('rect.tick');
    expect(ticks).toHaveLength(3);
    expect(container.querySelectorAll('rect.tick.hollow')).toHaveLength(1);
    expect(ticks[0]).toHaveClass('no');
    expect(ticks[1]).toHaveClass('yes');
    expect(ticks[2]).toHaveClass('other');
    expect(Number(ticks[0].getAttribute('x'))).toBeLessThan(Number(ticks[2].getAttribute('x')));
  });

  it('still draws a single episode, or several at the same instant (review focus 5)', () => {
    const { container } = render(<EpisodeStrip rows={[episodeSummaryFixture(), episodeSummaryFixture({ id: 'x' })]} />);
    const xs = [...container.querySelectorAll('rect.tick')].map((r) => Number(r.getAttribute('x')));
    expect(xs.every((x) => Number.isFinite(x))).toBe(true);
  });
});
```

`web/src/pages/Episodes.test.tsx`:
- first test: `expect(screen.getByText('SUSPECT'))` → `expect(screen.getByTitle(FLAG_WORDS.suspect)).toHaveTextContent('!');`
- "gives each flag tag a tooltip": replace the three lookups with
```tsx
    expect(within(row).getByText('!')).toHaveAttribute('title', FLAG_WORDS.suspect);
    expect(within(row).getByText('S', { selector: '.tag' })).toHaveAttribute('title', FLAG_WORDS.stale);
    const censored = within(row).getByText('C', { selector: '.tag' }).getAttribute('title') ?? '';
```
- Add:
```tsx
  it('groups rows under a UTC day and shows only the time in each row', async () => {
    mockApi({ '/api/episodes': { items: [
      episodeSummaryFixture({ id: 'a', opened_ms: Date.UTC(2026, 8, 27, 17, 32, 11) }),
      episodeSummaryFixture({ id: 'b', opened_ms: Date.UTC(2026, 8, 26, 23, 59, 1), event_title: 'Older' }),
    ], next_cursor: null } });
    renderAt('/episodes');
    expect(await screen.findByRole('columnheader', { name: '27 SEP' })).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: '26 SEP' })).toBeInTheDocument();
    expect(screen.getByText('17:32:11')).toBeInTheDocument();
    expect(screen.getByRole('figure', { name: '2 episodes in view, 2 net-positive' })).toBeInTheDocument();
  });
```
- The flag checkboxes now sit inside a `flags` disclosure; they stay in the DOM, so `getByRole('checkbox', …)` still works (no change).

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/lib/time.test.ts src/components/EpisodeStrip.test.tsx src/pages/Episodes.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement**

`web/src/lib/time.ts` (append):
```ts
const MONTHS = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

/** 17:32:11 (UTC), for rows already grouped under a day heading. */
export function fmtUtcHms(ms: number): string {
  return new Date(ms).toISOString().slice(11, 19);
}

/** "27 SEP" (UTC). */
export function fmtUtcDayMonth(ms: number): string {
  const d = new Date(ms);
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]}`;
}
```

`web/src/components/EpisodeStrip.tsx`:
```tsx
import type { EpisodeSummary } from '../api/types';
import { formatMoneyCents, signOf } from '../lib/money';

const W = 600;
const H = 40;

const tone = (kind: string) => (kind === 'no_basket' ? 'no' : kind === 'yes_basket' ? 'yes' : 'other');

/** 95th percentile of |net|, so one outlier doesn't flatten every other tick. Geometry only. */
function cap(rows: EpisodeSummary[]): number {
  const v = rows.map((r) => Math.abs(Number(r.net))).sort((a, b) => a - b);
  return Math.max(0.01, v[Math.min(v.length - 1, Math.floor(v.length * 0.95))] ?? 0.01);
}

/** Redesign §4.1: every episode in view on one strip; colour = kind, height = |NET|, hollow = flagged. */
export function EpisodeStrip({ rows }: { rows: EpisodeSummary[] }) {
  if (rows.length === 0) return null;
  const ts = rows.map((r) => r.opened_ms);
  let lo = Math.min(...ts);
  let hi = Math.max(...ts);
  if (hi === lo) {
    lo -= 60_000;
    hi += 60_000;
  }
  const c = cap(rows);
  const sx = (t: number) => 1 + Math.round(((t - lo) / (hi - lo)) * (W - 4));
  const pos = rows.filter((r) => signOf(r.net) > 0).length;
  return (
    <figure className="strip" aria-label={`${rows.length} episodes in view, ${pos} net-positive`}>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
        <rect className="axis" x={0} y={H - 1} width={W} height={1} />
        {rows.map((r) => {
          const h = Math.max(2, Math.round((Math.min(Math.abs(Number(r.net)), c) / c) * (H - 4)));
          const flagged = r.suspect || r.stale || r.censored;
          return (
            <rect key={r.id} className={`tick ${tone(r.kind)}${flagged ? ' hollow' : ''}`} x={sx(r.opened_ms)} y={H - 1 - h} width={2} height={h}>
              <title>{`${r.event_title ?? r.id} ${formatMoneyCents(r.net, { signed: true })}`}</title>
            </rect>
          );
        })}
      </svg>
      <figcaption className="dim">{`${rows.length} in view · ${pos} net-positive`}</figcaption>
    </figure>
  );
}
```

`web/src/pages/Episodes.tsx`:
1. `FlagTags` becomes:
```tsx
function FlagTags({ e }: { e: EpisodeSummary }) {
  return (
    <>
      {e.suspect && <Tag tone="flag" title={FLAG_WORDS.suspect}>!</Tag>}
      {e.stale && <Tag tone="flag" title={FLAG_WORDS.stale}>S</Tag>}
      {e.censored && <Tag tone="flag" title={CENSORED_WORDS}>C</Tag>}
    </>
  );
}
```
2. Replace `columns` and the `<Table>` usage with a grouped table:
```tsx
const HEADS = ['OPENED', 'KIND', 'EVENT', 'S', 'NET', 'GROSS', 'Q*', 'DURATION', 'FLAGS'];

function EpisodeRow({ r }: { r: EpisodeSummary }) {
  const t = kindTag(r.kind);
  const title = r.event_title ?? r.id;
  return (
    <tr className="ep">
      <td className="num c-time" title={fmtLocal(r.opened_ms)}>{fmtUtcHms(r.opened_ms)}</td>
      <td className="c-kind"><Tag tone={t.tone}>{t.text}</Tag></td>
      <td className="c-event"><Link to={`/episodes/${encodeURIComponent(r.id)}`} title={title}>{title}</Link></td>
      <td className="num c-s">{`${r.s_legs}/${r.n_legs}`}</td>
      <td className="num c-net"><Money value={r.net} signed tone cents /></td>
      <td className="num c-gross"><Money value={r.gross} signed cents /></td>
      <td className="num c-q">{formatDecimal(r.q_star)}</td>
      <td className="num c-dur">{r.duration_ms === null ? 'open' : fmtDuration(r.duration_ms)}</td>
      <td className="c-flags"><FlagTags e={r} /></td>
    </tr>
  );
}

/** Redesign §4.1: one line per episode, grouped under its UTC day. */
function EpisodeTable({ rows }: { rows: EpisodeSummary[] }) {
  const groups: { day: string; rows: EpisodeSummary[] }[] = [];
  for (const r of rows) {
    const day = fmtUtcDayMonth(r.opened_ms);
    if (groups.at(-1)?.day !== day) groups.push({ day, rows: [] });
    groups[groups.length - 1].rows.push(r);
  }
  return (
    <div className="scroll">
      <table className="eptable">
        <thead>
          <tr>{HEADS.map((h) => <th key={h} scope="col">{h}</th>)}</tr>
        </thead>
        {groups.map((g) => (
          <tbody key={g.day}>
            <tr className="day"><th colSpan={HEADS.length} scope="colgroup">{g.day}</th></tr>
            {g.rows.map((r) => <EpisodeRow key={r.id} r={r} />)}
          </tbody>
        ))}
      </table>
    </div>
  );
}
```
3. `Episodes()` returns one hero box:
```tsx
    <Box title="EPISODES" help="episodes" hero>
      <div className="filters">
        <div className="seg" role="group" aria-label="kind">
          {KINDS.map(([value, label]) => (
            <button key={label} type="button" className={kind === value ? 'on' : undefined} aria-pressed={kind === value} onClick={() => update('kind', value || null)}>
              {label}
            </button>
          ))}
        </div>
        <label>from <input type="date" value={sp.get('from') ?? ''} onChange={(e) => update('from', e.target.value || null)} /></label>
        <label>to <input type="date" value={sp.get('to') ?? ''} onChange={(e) => update('to', e.target.value || null)} /></label>
        <details className="menu">
          <summary>{`flags${(q.net ? 1 : 0) + (q.flags?.length ?? 0) > 0 ? ` (${(q.net ? 1 : 0) + (q.flags?.length ?? 0)})` : ''}`}</summary>
          <div className="menu-body">
            <label className="check">
              <input type="checkbox" checked={q.net ?? false} onChange={(e) => update('net', e.target.checked ? '1' : null)} />
              net-positive only
            </label>
            {FLAGS.map((f) => (
              <label key={f} className="check">
                <input type="checkbox" checked={q.flags?.includes(f) ?? false} onChange={() => toggleFlag(f)} />
                {f}
              </label>
            ))}
          </div>
        </details>
        <Help id="filters" />
      </div>
      <Loadable state={page} what="episodes">
        {(d) =>
          d.items.length === 0 ? (
            <EmptyState text={NO_EPISODES} />
          ) : (
            <>
              <EpisodeStrip rows={d.items} />
              <EpisodeTable rows={d.items} />
              <div className="pager">
                <button type="button" disabled={!q.cursor} onClick={() => update('cursor', null)}>NEWEST</button>
                <button type="button" disabled={!d.next_cursor} onClick={() => d.next_cursor && update('cursor', d.next_cursor)}>OLDER →</button>
              </div>
              <Fold id="episodes:flags" label="what the flags mean">
                <FlagLegend />
              </Fold>
            </>
          )
        }
      </Loadable>
    </Box>
```
Imports: add `EpisodeStrip`, `Fold`, `fmtUtcDayMonth`, `fmtUtcHms`; remove `Table`, `Column`, `fmtUtcDateTime`, and the `Box title="FILTERS"`. Append " UTC days. Seven days at a time." to the end of `HELP.filters.text` in `web/src/help.ts` if it isn't already there, and delete the `<p className="dim">UTC days…</p>` line.

Append to `web/src/theme.css`:
```css
.strip svg { display: block; width: 100%; height: 48px; background: var(--bg); border: 2px solid var(--line2); }
.strip .axis { fill: var(--line2); }
.strip .tick.no { fill: var(--teal); } .strip .tick.yes { fill: var(--salmon); } .strip .tick.other { fill: var(--dim); }
.strip .tick.hollow { fill: var(--bg); stroke: var(--dim); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.menu { position: relative; }
.menu > summary { cursor: pointer; border: 2px solid var(--line); padding: 0 6px; background: var(--chip); list-style: none; }
.menu > summary::-webkit-details-marker { display: none; }
.menu-body { position: absolute; z-index: 3; top: 110%; left: 0; background: var(--bar); border: 2px solid var(--line); padding: 6px 8px; display: grid; gap: 2px; white-space: nowrap; }
.eptable tr.day th { font-family: var(--px); font-size: 8px; padding-top: 10px; color: var(--dim); border-bottom: 1px solid var(--line2); }
.eptable .c-event { max-width: 0; width: 45%; }
.eptable .c-event a { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
@media (max-width: 600px) {
  .eptable thead { display: none; }
  .eptable tr.ep { display: grid; grid-template-columns: auto auto 1fr; grid-template-areas: 'time kind net' 'event event event' 's dur flags'; gap: 2px 8px; padding: 6px 0; border-bottom: 1px dashed var(--line2); }
  .eptable tr.ep td { border: 0; padding: 0; }
  .eptable .c-time { grid-area: time; } .eptable .c-kind { grid-area: kind; } .eptable .c-net { grid-area: net; justify-self: end; }
  .eptable .c-event { grid-area: event; max-width: none; width: auto; min-width: 0; }
  .eptable .c-s { grid-area: s; } .eptable .c-dur { grid-area: dur; } .eptable .c-flags { grid-area: flags; justify-self: end; }
  .eptable .c-gross, .eptable .c-q { display: none; }
}
```

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: Episodes as one section: one-line filters, timeline strip, day-grouped one-line rows, phone cards

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Papers

**Files:**
- Modify: `web/src/pages/Papers.tsx`, `web/src/pages/Papers.test.tsx`, `web/src/help.ts`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`PageHead`, `Fold`), Task 7 (`CardView` with claim and folds).
- Produces: `PAPERS` page head (h1) with window switch and `updated n min ago`; `PAPERS_SUBTITLE` removed from `Papers.tsx` — its sentence now opens `HELP.papers.text`; each live paper is a `Box className="paper"` whose title is the citation; `NOT REPLICATED` becomes `Fold id="papers:notrep" label="not replicated (N)"`.

- [ ] **Step 1: Update the tests first** (`web/src/pages/Papers.test.tsx`)
- Remove `PAPERS_SUBTITLE` from the import; add `import { HELP } from '../help';`.
- Replace `expect(screen.getByText(PAPERS_SUBTITLE))…` with:
```tsx
    expect(screen.getByRole('heading', { level: 1, name: 'PAPERS' })).toBeInTheDocument();
    expect(HELP.papers.text.startsWith('Their number, their method on our data, our method. Same market, different weeks.')).toBe(true);
```
- Replace `expect(screen.getByText('NOT REPLICATED'))…` with `expect(screen.getByText('not replicated (2)').closest('details')).not.toHaveAttribute('open');`

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/pages/Papers.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement**

`web/src/help.ts`: prefix `papers.text` with `'Their number, their method on our data, our method. Same market, different weeks. '`.

`web/src/pages/Papers.tsx`: delete `PAPERS_SUBTITLE`; import `PageHead` and `Fold`; `PaperSection`'s Box becomes `<Box title={paper.cite} help={…} className="paper">` (unchanged otherwise); `Papers()` returns:
```tsx
    <>
      <PageHead title="PAPERS" help="papers">
        <WindowSwitch win={win} onChange={(w) => setSp(new URLSearchParams({ win: w }))} />
        <span className={upd.stale ? 'dim stale' : 'dim'}>{upd.text}</span>
        {meta.data?.last_error && <span className="error">last tick failed: {meta.data.last_error.text}</span>}
      </PageHead>
      <Loadable state={meta} what="the papers">
        {(m) => (
          <>
            {m.papers.map((p) => (
              <PaperSection key={p.id} paper={p} cards={cards} stale={upd.stale} />
            ))}
            <Fold id="papers:notrep" label={`not replicated (${m.not_replicated.length})`}>
              <ul className="plain">
                {m.not_replicated.map((n) => (
                  <li key={n.item}><b>{n.item}.</b> {n.why}</li>
                ))}
              </ul>
            </Fold>
          </>
        )}
      </Loadable>
    </>
```
(The `notReplicated` help id stays in `help.ts`; it is no longer rendered. Leave it — `HelpId` is a type union and unused ids are harmless.)

Append to `web/src/theme.css`:
```css
.box.paper > .box-head h2 { font-family: var(--body); font-size: 13px; font-weight: 400; color: var(--dim); }
@media (max-width: 900px) { .cardgrid { grid-template-columns: minmax(0, 1fr); } }
```
and delete the old `@media (max-width: 760px) { .cardgrid … }` line.

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: Papers page head, citation as a quiet line, not-replicated folded

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Health

**Files:**
- Create: `web/src/components/CountBars.tsx`, `web/src/components/CountBars.test.tsx`
- Modify: `web/src/pages/Health.tsx`, `web/src/pages/Health.test.tsx`, `web/src/theme.css`

**Interfaces:**
- Consumes: Task 1 (`PageHead`, `Tiles`, `Fold`, `SubHead`, `Box` `hero`); `LineChart` `markerLabel` (Task 8); `groupThousands`.
- Produces: `CountBars({ items, label }: { items: { label: string; count: number }[]; label: string })` — `<ul aria-label={label}>`, one `li` per item with the label, a bar `span.cb-bar` whose `style.width` is `${round(count / max * 100)}%`, and the count grouped by thousands.
- Deviation from spec §4.4 (called out for review): the integrity table fold is dropped because `CountBars` already prints every kind with its exact count; the fold would repeat it verbatim.

- [ ] **Step 1: Write the failing tests**

`web/src/components/CountBars.test.tsx`:
```tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { CountBars } from './CountBars';

describe('CountBars', () => {
  it('draws one bar per kind, longest first as given, with the count written out', () => {
    const { container } = render(<CountBars label="integrity by kind" items={[{ label: 'top_mismatch', count: 4256 }, { label: 'drift', count: 961 }]} />);
    expect(screen.getByRole('list', { name: 'integrity by kind' })).toBeInTheDocument();
    expect(screen.getByText('4,256')).toBeInTheDocument();
    const bars = [...container.querySelectorAll('.cb-bar')] as HTMLElement[];
    expect(bars[0].style.width).toBe('100%');
    expect(bars[1].style.width).toBe('23%');
  });
});
```

`web/src/pages/Health.test.tsx`, test "shows capture status…": add
```tsx
    expect(screen.getByRole('heading', { level: 1, name: 'HEALTH' })).toBeInTheDocument();
    const tiles = screen.getAllByRole('listitem').filter((li) => li.classList.contains('tile')).map((li) => li.textContent);
    expect(tiles[1]).toBe('shards40/40');
    expect(screen.getByRole('list', { name: 'integrity by kind' })).toBeInTheDocument();
```
(`'PAUSED'`, `'top_mismatch'` and the per-million line stay single matches.)

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/CountBars.test.tsx src/pages/Health.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Implement**

`web/src/components/CountBars.tsx`:
```tsx
import { groupThousands } from '../lib/money';

/** Redesign §4.4: counts as bars with the number written at the end (integrity by kind). */
export function CountBars({ items, label }: { items: { label: string; count: number }[]; label: string }) {
  const max = Math.max(1, ...items.map((i) => i.count));
  return (
    <ul className="plain countbars" aria-label={label}>
      {items.map((i) => (
        <li key={i.label}>
          <span className="cb-label">{i.label}</span>
          <span className="cb-track">
            <span className="cb-bar" style={{ width: `${Math.round((i.count / max) * 100)}%` }} />
          </span>
          <span className="num">{groupThousands(String(i.count))}</span>
        </li>
      ))}
    </ul>
  );
}
```

`web/src/pages/Health.tsx`:
1. `CaptureStatus` becomes tiles plus a fold:
```tsx
function CaptureStatus({ h }: { h: HealthNow }) {
  return (
    <>
      <Tiles
        items={[
          { label: 'capture', value: h.capture_paused ? <Tag tone="flag">PAUSED</Tag> : 'recording ✓' },
          { label: 'shards', value: `${h.shards - h.parked_shards}/${h.shards}` },
          { label: 'tokens', value: groupThousands(String(h.tokens)) },
          { label: 'last minute', value: `${fmtDuration(h.age_ms)} ago`, title: fmtLocal(h.minute_ms) },
        ]}
      />
      <Fold id="health:capture" label="all numbers">
        <dl className="kv">
          <dt>/ok</dt>
          <dd>{h.ok === null ? '—' : h.ok ? '✓ ok' : '✗ not ok'}</dd>
          <dt>clock offset</dt>
          <dd>{h.clock_offset_ms === null ? '—' : `${h.clock_offset_ms} ms`}</dd>
          <dt>last minute</dt>
          <dd title={fmtLocal(h.minute_ms)}>{fmtUtcDateTime(h.minute_ms)}</dd>
        </dl>
      </Fold>
    </>
  );
}
```
2. `SeriesCharts` returns one hero box with three sub-headed charts:
```tsx
  return (
    <Box title="PER MINUTE" help="perMinute" hero>
      <div className="grid3">
        <div>
          <SubHead help="msgs">msgs/s</SubHead>
          <LineChart label="messages per second" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'msgs/s', tone: 'p0', points: s.points.map((p) => ({ x: p.minute_ms, y: p.msgs / 60 })) }]} />
        </div>
        <div>
          <SubHead help="lag">lag (ms)</SubHead>
          <LineChart label="feed lag p50 and p99" xMin={s.from_ms} xMax={s.to_ms} series={[{ label: 'p50', tone: 'p0', points: lag('lag_p50_ms') }, { label: 'p99', tone: 'p3', points: lag('lag_p99_ms') }]} />
        </div>
        <div>
          <SubHead help="drops">drops and reconnects</SubHead>
          <LineChart
            label="drops and reconnects per minute"
            xMin={s.from_ms}
            xMax={s.to_ms}
            markers={s.points.filter((p) => p.reconnects > 0).map((p) => p.minute_ms)}
            markerLabel="minutes with reconnects"
            series={[
              { label: 'drops', tone: 'p3', points: s.points.map((p) => ({ x: p.minute_ms, y: p.drops })) },
              { label: 'reconnects', tone: 'p1', points: s.points.map((p) => ({ x: p.minute_ms, y: p.reconnects })) },
            ]}
          />
        </div>
      </div>
    </Box>
  );
```
3. `IntegrityView` uses bars instead of the table:
```tsx
function IntegrityView({ i }: { i: Integrity }) {
  if (i.total === 0) return <EmptyState text={NO_INTEGRITY} />;
  return (
    <>
      <CountBars label="integrity by kind" items={i.by_kind.map((k) => ({ label: k.kind, count: k.count }))} />
      <p className="quip">{`${i.total} events in ${i.msgs} messages: ${i.per_million === null ? '—' : formatDecimal(i.per_million)} per million.`}</p>
    </>
  );
}
```
4. `Health()`: replace the `WINDOW` box with
```tsx
      <PageHead title="HEALTH" help="window">
        <div className="seg" role="group" aria-label="window">
          {WINDOWS.map((w) => (
            <button key={w} type="button" className={w === hours ? 'on' : undefined} aria-pressed={w === hours} onClick={() => setSp(new URLSearchParams({ hours: String(w) }))}>
              {w === 168 ? '7 D' : `${w} H`}
            </button>
          ))}
        </div>
      </PageHead>
```
Imports: add `PageHead`, `Tiles`, `Fold`, `SubHead`, `CountBars`, `groupThousands`; remove `Table`.

Append to `web/src/theme.css`:
```css
.grid3 { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; }
.grid3 .subhead-row { margin-top: 0; border-top: 0; padding-top: 0; }
@media (max-width: 900px) { .grid3 { grid-template-columns: minmax(0, 1fr); } }
.countbars { gap: 4px; }
.countbars li { display: grid; grid-template-columns: minmax(120px, 200px) minmax(0, 1fr) auto; gap: 10px; align-items: center; }
.cb-track { display: block; height: 12px; background: var(--bg); }
.cb-bar { display: block; height: 100%; background: var(--accent); }
```

- [ ] **Step 4: Run all tests**

Run: `npx vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "web: Health page head, capture tiles, three charts in one section, integrity as bars

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Build, visual check, handoff

**Files:** none new (fixes only, if the checks find something).

- [ ] **Step 1: Full test and build**

Run: `npx vitest run && npm run build`
Expected: all tests pass; `tsc --noEmit` clean; vite build writes `dist/`.

- [ ] **Step 2: Grep for leftovers**

Run: `grep -rn "SweetSpots\|DecayChart\|PAPERS_SUBTITLE\|max-width: 1100px" src/ || echo clean`
Expected: `clean`.

- [ ] **Step 3: Local visual check against the live api (read-only)**

The dev server proxies `/api` to `127.0.0.1:8081`. Ask the owner whether to open an SSH tunnel to the VPS api (`ssh -L 8081:127.0.0.1:8081 <vps>`, per `docs/superpowers/runbooks/phase4-dashboard-vps.md`) or to skip to the post-deploy check. With a tunnel: `npm run dev`, then in Chrome at 1200 px and 400 px open `/`, `/sweet`, `/episodes`, `/paper`, `/papers`, `/health` and check the spec §1 exit list: drawing above the fold, no number wrapped mid-value, no box inside a box, `/sweet` and the Radar card show the live SWEET state. Fix and commit anything found (one commit per fix, same message style).

- [ ] **Step 4: Hand off**

Do not deploy or open a PR without the owner's go-ahead. Report: test count, build result, screenshots from Step 3, and the two spec deviations (Radar card: title is the link, not the whole box; Health: no integrity table fold).
