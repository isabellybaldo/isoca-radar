# The /math page as a story, then a playground: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the public `/math` page into seven stacked story chapters (one drawing and one before/after switch each) followed by the existing six-section playground.

**Architecture:** The story is new code under `web/src/math/story/`: twelve preset engine inputs, a chapter shell, and seven small chapter components that call the same WASM engine and reuse the playground's own drawings. The page (`MathPage.tsx`) renders the hook, the story (prose always; drawings once the engine is ready; the live strip inside chapter 3 and the live ladder inside chapter 7), then `section#play` with a word list and sections a–f. Nothing outside `web/` changes except docs.

**Tech Stack:** React 18 + TypeScript, Vite, vitest + Testing Library, the `arb-wasm` engine (wasm-bindgen 0.2.129), plain CSS (`math.css`, `theme.css`).

**Spec:** `docs/superpowers/specs/2026-10-01-math-story-design.md` (read it first; it holds every sentence of the copy). Parent: `docs/superpowers/specs/2026-09-30-phase7b-math-playground-design.md`.

## Global Constraints

- Worktree: `C:\Users\Isabelly\PycharmProjects\cute-trading\.claude\worktrees\math-story` (branch `math-story`). All commands below run in `web/` unless they say otherwise. Git Bash.
- The real code wins over this plan: where a name, prop or line here differs from the file, follow the file and say so in the report.
- Every number shown comes from the engine (`call(engine, '<export>', input)`) or the snapshot. The page computes no money. Decimals stay strings; `Number()` only for drawing geometry; engine inputs are built from integer ticks (`fromTick`).
- Every `$` figure carries "on paper" or "on the table" in its row or caption (the page test's `unframedMoney`). Story prose has no `$` and no digit-figure of its own; a payout is "one dollar".
- A sentence that states an outcome is chosen from the engine's output. `checkHonesty` (`web/src/explain/honesty.ts`) must find nothing in any copy.
- No `<a>`, no `<nav>`, no inline `style=`, no `dangerouslySetInnerHTML`, no `/api/` string, no import from `src/pages`, `src/api`, `src/explain` (tests excepted), `src/help.ts` in anything the math entry reaches. No new dependency. No timer.
- 360 px without horizontal scroll; controls at least 44 px high; no `minmax(<N>px` in CSS; labels are HTML beside SVGs.
- Sections a–f keep ids `a`…`f`, controls and behaviour. No existing assertion is dropped; the ones that move are listed in the spec §8.
- Tests that run the real WASM use `describeWasm` and `loadTestEngine` from `web/src/wasm/testEngine.ts`; engine figures in tests are read from engine output, never typed.
- Check set per task: `npx vitest run <touched test files>`; then `WASM_REQUIRED=1 npm test`; `npx tsc --noEmit`; `npx tsc --noEmit -p tsconfig.math.json`. Test output without warnings or `act()` noise.
- Commits: explicit paths, message `web: <sentence>` (or `runbook: …`, `docs: …`), **never** a Co-Authored-By or any other trailer.
- Copy is the spec's §5 and §6, verbatim, unless a sentence is untrue of the engine's output or trips a check; then change it minimally and report it.

## Review Focus

1. **The engine answers `{error}` inside a chapter** (a preset the engine rejects after a future engine change): the chapter shows the engine's sentence and no takeaway, and the page stands. Test in Task 3.
2. **The snapshot arrives after the engine on a cold `/math#play` or `/math#d`:** the target is still at the top after the strip and the ladder appear. Test in Task 5.
3. **Switching one chapter must not move another:** each chapter owns its own state. Test in Task 3.
4. **Today has zero episodes, or the epoch has no rungs yet:** chapters 3 and 7 still read true (no sentence promises a number). Test in Task 5.
5. **The two longest switch labels at 360 px** ("every outcome is listed" / "one outcome is not listed"): the switch wraps onto two rows and nothing overflows. CSS pin in Task 5; the controller looks at it in Chrome.

---

## File map

| File | Responsibility | Task |
|---|---|---|
| `web/src/math/story/presets.ts` (new) | the twelve engine inputs, chapter 3's two labels | 1 |
| `web/src/math/story/presets.test.ts` (new) | every preset's "must hold" on the real engine | 1 |
| `web/src/math/sections/*.tsx` | export builders; `episodeInput(st, times)`; `PayoffView`, `EpisodeView` | 1, 3 |
| `web/src/math/story/copy.ts` (new) | hook, chapters, takeaway pickers, word list | 2 |
| `web/src/math/story/Chapter.tsx`, `Words.tsx` (new) | the chapter shell; the word list | 2 |
| `web/src/math/story/Story.tsx` (new) | the seven chapters | 3 |
| `web/src/schematics/MirrorBooks.tsx` | `headline` prop | 3 |
| `web/src/math/sections/Section.tsx`, `copy.ts` | h3, back-reference, new titles | 4 |
| `web/src/math/MathPage.tsx`, `math.css`, `web/math.html` | assembly, live parts, cold link, CSS, meta | 5 |
| `docs/superpowers/runbooks/math-story-web-vps.md` (new) | deploy | 6 |

---

### Task 1: Presets, proven on the real engine

**Files:**
- Create: `web/src/math/story/presets.ts`, `web/src/math/story/presets.test.ts`
- Modify: `web/src/math/sections/MirrorSection.tsx` (export `crossed`), `web/src/math/sections/EpisodeSection.tsx` (`episodeInput` takes the times)

**Interfaces:**
- Consumes: `MIRROR_START`, `crossed` (`sections/MirrorSection.tsx`); `PAYOFF_START`, `payoffInput` (`sections/PayoffSection.tsx`); `classifyInput`, `ClassifyState` (`sections/ClassifySection.tsx`); `DEPTH_START`, `depthInput` (`sections/DepthSection.tsx`); `EPISODE_START`, `episodeInput` (`sections/EpisodeSection.tsx`); `basketInput`, `drain` (`basket.ts`); `F_BOOKS`, `F_START`, `F_BURST`, `EPISODE_T`, `fromTick` (`defaults.ts`).
- Produces (later tasks import exactly these):

```ts
export type Pair<T> = readonly [T, T];
export const CH1: Pair<MirrorIn>;
export const CH2: Pair<PayoffIn>;
export const CH3: Pair<ClassifyIn>;
export const CH3_LABELS: Pair<string>;   // "prices add to 1.00", "prices add to 0.97"
export const CH4: Pair<DepthWalkIn>;
export const CH5: Pair<EpisodeIn>;
export const CH6: Pair<BasketIn>;
export const SHORT_T: number[];          // [0, 400, 450, 500, 550]
```
  and `episodeInput(st: EpisodeState, times: number[] = EPISODE_T): EpisodeIn`.

- [ ] **Step 1: Write the failing test** `web/src/math/story/presets.test.ts`

```ts
// @ts-ignore: node:fs exists at runtime under vitest
import { readFileSync } from 'node:fs';
import { beforeAll, expect, it } from 'vitest';
import { basketWord } from '../../schematics/BasketLegs';
import { LADDER } from '../../lib/ladder';
import { call } from '../../wasm/call';
import { describeWasm, loadTestEngine } from '../../wasm/testEngine';
import type { BasketModel, ClassifyModel, Engine, EpisodeModel, MirrorModel, PayoffMatrix, WalkModel } from '../../wasm/types';
import { isError } from '../../wasm/types';
import { CH1, CH2, CH3, CH3_LABELS, CH4, CH5, CH6 } from './presets';

declare const process: { cwd(): string };

/** The engine's model for an input; a test fails loudly on an engine error. */
function ok<T>(engine: Engine, fn: Parameters<typeof call>[1], input: unknown): T {
  const out = call<T>(engine, fn, input);
  if (isError(out)) throw new Error(`${fn}: ${out.error}`);
  return out;
}
const positive = (d: string) => !d.startsWith('-') && /[1-9]/.test(d);
const less = (a: string, b: string) => Number(a) < Number(b); // comparison only: nothing here is shown as text

describeWasm('story presets on the real engine (spec §4)', () => {
  let e: Engine;
  beforeAll(async () => { e = await loadTestEngine(); });

  it('chapter 1: a healthy book, then a crossed one', () => {
    expect(ok<MirrorModel>(e, 'mirror_json', CH1[0]).crossed).toBe(false);
    expect(ok<MirrorModel>(e, 'mirror_json', CH1[1]).crossed).toBe(true);
  });

  it('chapter 2: every outcome listed has no broken column; one not listed has one', () => {
    expect(ok<PayoffMatrix>(e, 'payoff_json', CH2[0]).broken.some(Boolean)).toBe(false);
    expect(ok<PayoffMatrix>(e, 'payoff_json', CH2[1]).broken.some(Boolean)).toBe(true);
  });

  it('chapter 3: at 1.00 nothing; below it a YES basket settled at resolution with a positive net', () => {
    expect(ok<ClassifyModel>(e, 'classify_json', CH3[0]).kind).toBeNull();
    const m = ok<ClassifyModel>(e, 'classify_json', CH3[1]);
    expect(m.kind).toBe('yes_basket');
    expect(m.realization).toBe('settlement');
    expect(m.net !== null && positive(m.net)).toBe(true);
  });

  it('chapter 3: each label is the sum of its own preset ticks, and has no dollar sign', () => {
    const sum = (i: 0 | 1) => {
      const ticks = CH3[i].legs.reduce((a, l) => a + Math.round(Number(l.asks[0].p) * 100), 0); // test-side check of the label
      return `${Math.floor(ticks / 100)}.${String(ticks % 100).padStart(2, '0')}`;
    };
    expect(CH3_LABELS).toEqual([`prices add to ${sum(0)}`, `prices add to ${sum(1)}`]);
    expect(CH3_LABELS.join(' ')).not.toContain('$');
  });

  it('chapter 4: a thin book ends the walk sooner and leaves less', () => {
    const deep = ok<WalkModel>(e, 'depth_walk_json', CH4[0]);
    const thin = ok<WalkModel>(e, 'depth_walk_json', CH4[1]);
    for (const m of [deep, thin]) {
      expect(positive(m.q_star)).toBe(true);
      expect(positive(m.pi)).toBe(true);
    }
    expect(less(thin.q_star, deep.q_star)).toBe(true);
    expect(less(thin.pi, deep.pi)).toBe(true);
    // the picture must change too: Q* sits at a different share of the walk's length
    const share = (m: WalkModel) => Number(m.q_star) / Number(m.steps[m.steps.length - 1].q_to);
    expect(share(thin)).not.toBeCloseTo(share(deep), 2);
  });

  it('chapter 5: the edge outlasts the order, then closes first', () => {
    const a = ok<EpisodeModel>(e, 'episode_json', CH5[0]);
    const b = ok<EpisodeModel>(e, 'episode_json', CH5[1]);
    expect([a.open_ms !== null, a.caught]).toEqual([true, true]);
    expect([b.open_ms !== null, b.caught]).toEqual([true, false]);
  });

  it('chapter 6: every leg fills, then one misses', () => {
    expect(basketWord(ok<BasketModel>(e, 'basket_json', CH6[0]))).toBe('filled');
    expect(basketWord(ok<BasketModel>(e, 'basket_json', CH6[1]))).toBe('caught');
  });

  it('chapter 7 says "six accounts": the ladder in config/policies.toml has six rungs', () => {
    const toml: string = readFileSync(`${process.cwd()}/../config/policies.toml`, 'utf8');
    expect(LADDER).toHaveLength(6);
    for (const l of LADDER) expect(toml).toContain(`"${l.scenario}"`);
  });
});
```

- [ ] **Step 2: Run it, see it fail**

Run: `WASM_REQUIRED=1 npx vitest run src/math/story/presets.test.ts`
Expected: FAIL, `Failed to resolve import "./presets"`.

- [ ] **Step 3: Export what the presets need**

In `web/src/math/sections/MirrorSection.tsx` change `function crossed(` to `export function crossed(`.

In `web/src/math/sections/EpisodeSection.tsx` give `episodeInput` its times:

```ts
/** episode_json's input: no basket (the engine has none to fill, 7b §6.2 e) and no gap_ms (it changes nothing).
 *  `times` are the updates' clock (the story's second state uses a shorter one). */
export function episodeInput(st: EpisodeState, times: number[] = EPISODE_T): EpisodeIn {
  return {
    kind: 'no',
    updates: times.slice(0, st.upto).map((t, i) => ({ t_ms: t, legs: EPISODE_BIDS[i].map(leg) })),
    scenario: { ...st.sc, honor_seconds_delay: true },
    seconds_delay: st.delay ? 1 : null,
  };
}
```

- [ ] **Step 4: Write `web/src/math/story/presets.ts`**

```ts
// The story's engine inputs (spec 2026-10-01 §4): two per chapter, built from the playground's own starting
// states and input builders, so a chapter and its playground section can never disagree. presets.test.ts runs
// each through the real engine and pins what the chapter's words rely on.
import type { BasketIn, ClassifyIn, DepthWalkIn, EpisodeIn, MirrorIn, PayoffIn } from '../../wasm/types';
import { basketInput, drain } from '../basket';
import { F_BOOKS, F_BURST, F_START, fromTick } from '../defaults';
import { type ClassifyState, classifyInput } from '../sections/ClassifySection';
import { DEPTH_START, depthInput } from '../sections/DepthSection';
import { EPISODE_START, episodeInput } from '../sections/EpisodeSection';
import { MIRROR_START, crossed } from '../sections/MirrorSection';
import { PAYOFF_START, payoffInput } from '../sections/PayoffSection';

export type Pair<T> = readonly [T, T];

export const CH1: Pair<MirrorIn> = [MIRROR_START, crossed(MIRROR_START)];

/** A YES basket over three outcomes: with "Other" listed every column pays; without it one column pays nothing. */
export const CH2: Pair<PayoffIn> = [
  payoffInput({ ...PAYOFF_START, kind: 'yes', s: 3, listed: true, priced: false }),
  payoffInput({ ...PAYOFF_START, kind: 'yes', s: 3, listed: false, priced: false }),
];

/** Best asks in ticks: Σ 1.00 (nothing to take), then Σ 0.97 (a YES-basket edge). Bids sit one tick below. */
const CH3_ASKS: Pair<number[]> = [[33, 33, 34], [30, 32, 35]];
const ch3State = (asks: number[]): ClassifyState => ({
  augmented: false,
  legs: asks.map((ask) => ({ status: 'live', bid: ask - 1, ask })),
});
export const CH3: Pair<ClassifyIn> = [classifyInput(ch3State(CH3_ASKS[0])), classifyInput(ch3State(CH3_ASKS[1]))];
const sumText = (ticks: number[]) => fromTickSum(ticks.reduce((a, t) => a + t, 0));
/** A tick count as a decimal string, integer maths only (100 → "1.00"). */
function fromTickSum(n: number): string {
  return `${Math.floor(n / 100)}.${String(n % 100).padStart(2, '0')}`;
}
export const CH3_LABELS: Pair<string> = [`prices add to ${sumText(CH3_ASKS[0])}`, `prices add to ${sumText(CH3_ASKS[1])}`];

/** The thin book: each leg's two cheapest levels at a quarter of their size, the third as it is. (Quartering
 *  every level would leave the stairs' picture unchanged: they scale to their last step.) */
export const CH4: Pair<DepthWalkIn> = [
  depthInput(DEPTH_START),
  depthInput({ ...DEPTH_START, legs: DEPTH_START.legs.map((l) => [...drain(l.slice(0, 2), 25), ...l.slice(2)]) }),
];

/** The second state's clock: the same five updates, the last four 50 ms apart, so the episode closes at 550 ms. */
export const SHORT_T = [0, 400, 450, 500, 550];
export const CH5: Pair<EpisodeIn> = [episodeInput(EPISODE_START), episodeInput(EPISODE_START, SHORT_T)];

export const CH6: Pair<BasketIn> = [basketInput(F_BOOKS, F_START), basketInput(F_BOOKS, F_BURST)];
```

Remove the unused `fromTick` import if `tsc` flags it (the labels use `fromTickSum`); or use `bestSum`-style code already in `defaults.ts` if it fits without change. One helper for "ticks → decimal string" is enough: if `defaults.ts` or `schematics/geo.ts` (`tickText`) already has it, use that and delete `fromTickSum`.

- [ ] **Step 5: Run the test, see it pass**

Run: `WASM_REQUIRED=1 npx vitest run src/math/story/presets.test.ts`
Expected: 8 passed. If a "must hold" fails, the preset is wrong, not the test: adjust the preset (never the assertion) and report what the engine returned.

- [ ] **Step 6: Check set and commit**

Run: `WASM_REQUIRED=1 npm test` (all pass), `npx tsc --noEmit`, `npx tsc --noEmit -p tsconfig.math.json` (silent).

```bash
git add web/src/math/story/presets.ts web/src/math/story/presets.test.ts
git commit web/src/math/story/presets.ts web/src/math/story/presets.test.ts web/src/math/sections/MirrorSection.tsx web/src/math/sections/EpisodeSection.tsx -m "web: the story's twelve engine inputs, each proven on the real engine"
```

---

### Task 2: Story copy, the chapter shell and the word list

**Files:**
- Create: `web/src/math/story/copy.ts`, `web/src/math/story/copy.test.ts`, `web/src/math/story/Chapter.tsx`, `web/src/math/story/Words.tsx`, `web/src/math/story/Chapter.test.tsx`

**Interfaces:**
- Consumes: `Seg` from `sections/Section.tsx`; `BasketWord` from `schematics/BasketLegs`; `checkHonesty` from `explain/honesty` (tests only).
- Produces:

```ts
// copy.ts
export const HOOK: string;
export interface ChapterCopy { n: number; id: string; question: string; lead: string; switchLabel: string; options: readonly [string, string] | null }
export const CHAPTERS: readonly ChapterCopy[];           // seven, ids 's1'…'s7'; chapter 3's options are null here (Story passes CH3_LABELS); chapter 7's are null (no switch)
export const TAKE: {
  ch1: (crossed: boolean) => string;
  ch2: (broken: boolean) => string;
  ch3: (signal: boolean) => string;
  ch4: string;
  ch5: (caught: boolean) => string;
  ch6: (word: BasketWord) => string;
  ch7: string;
};
export const CH3_COUNT: string;     // "The scanner counts each stretch an edge stays open as one episode. Today's count:"
export const CH7_EXTRAS: string;    // the dim "extras: …" line
export const PLAY: { title: string; line: string };
export const WORDS: readonly (readonly [string, string])[];   // twelve [term, definition]
// Chapter.tsx
export function Chapter(p: { copy: ChapterCopy; options?: readonly [string, string] | null; value?: 0 | 1; onPick?: (v: 0 | 1) => void; take?: string | null; children?: ReactNode }): JSX.Element;
// Words.tsx
export function Words(): JSX.Element;
```

- [ ] **Step 1: Write the failing tests**

`web/src/math/story/copy.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { checkHonesty } from '../../explain/honesty';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, HOOK, PLAY, TAKE, WORDS } from './copy';

const every: string[] = [
  HOOK, CH3_COUNT, CH7_EXTRAS, PLAY.title, PLAY.line,
  ...CHAPTERS.flatMap((c) => [c.question, c.lead, c.switchLabel, ...(c.options ?? [])]),
  TAKE.ch1(false), TAKE.ch1(true), TAKE.ch2(false), TAKE.ch2(true), TAKE.ch3(false), TAKE.ch3(true), TAKE.ch4,
  TAKE.ch5(false), TAKE.ch5(true), TAKE.ch6('filled'), TAKE.ch6('caught'), TAKE.ch6('nothing filled'), TAKE.ch7,
  ...WORDS.flat(),
];

describe('story copy (spec §5, §6)', () => {
  it('seven chapters, numbered 1 to 7, ids s1 to s7, each asking a question', () => {
    expect(CHAPTERS.map((c) => [c.n, c.id])).toEqual([1, 2, 3, 4, 5, 6, 7].map((n) => [n, `s${n}`]));
    for (const c of CHAPTERS) expect(c.question.endsWith('?')).toBe(true);
  });

  it('uses no word the honesty check bans, no dollar figure, no exclamation mark', () => {
    for (const s of every) {
      expect(checkHonesty(s), s).toEqual([]);
      expect(s, s).not.toMatch(/\$\s?\d/);
      expect(s, s).not.toContain('!');
    }
  });

  it('a lead is at most four sentences', () => {
    for (const c of CHAPTERS) expect(c.lead.split(/(?<=[.?])\s+/).length, c.id).toBeLessThanOrEqual(4);
  });

  it('each outcome sentence has a different line per outcome', () => {
    expect(TAKE.ch1(true)).not.toBe(TAKE.ch1(false));
    expect(TAKE.ch2(true)).not.toBe(TAKE.ch2(false));
    expect(TAKE.ch3(true)).not.toBe(TAKE.ch3(false));
    expect(TAKE.ch5(true)).not.toBe(TAKE.ch5(false));
    expect(new Set([TAKE.ch6('filled'), TAKE.ch6('caught'), TAKE.ch6('nothing filled')]).size).toBe(3);
  });

  it('the word list has the twelve terms of the spec, in its order', () => {
    expect(WORDS.map(([t]) => t)).toEqual([
      'YES / NO share', 'book, bid, ask', 'basket, leg', 'NO basket', 'edge (Π)', 'Q*', 'episode', 't_x', 'unwind', 'convert', 'on the table', 'on paper',
    ]);
  });
});
```

`web/src/math/story/Chapter.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Chapter } from './Chapter';
import { CHAPTERS, WORDS } from './copy';
import { Words } from './Words';

const C2 = CHAPTERS[1];

describe('Chapter (spec §3.1)', () => {
  it('is a section named by its question, with its number, lead, switch, drawing and takeaway in that order', () => {
    const { container } = render(
      <Chapter copy={C2} options={C2.options} value={0} onPick={() => {}} take="the takeaway">
        <figure aria-label="the drawing" />
      </Chapter>,
    );
    const sec = container.querySelector('section#s2') as HTMLElement;
    expect(sec).toHaveClass('m-ch');
    expect(within(sec).getByRole('heading', { level: 2 })).toHaveTextContent(`2 ${C2.question}`);
    expect(sec).toHaveAccessibleName(`2 ${C2.question}`);
    const kids = [...sec.children].map((k) => k.className || k.tagName.toLowerCase());
    expect(kids).toEqual(['', 'm-ch-lead', 'seg', 'figure', 'm-ch-take'].map((k, i) => (i === 0 ? kids[0] : k)));
    expect(within(sec).getByText('the takeaway')).toHaveClass('m-ch-take');
  });

  it('the switch is a named group of two buttons; the pressed one says so; picking the other reports 1', () => {
    const onPick = vi.fn();
    render(<Chapter copy={C2} options={C2.options} value={0} onPick={onPick} take={null} />);
    const group = screen.getByRole('group', { name: C2.switchLabel });
    const [a, b] = within(group).getAllByRole('button');
    expect([a.getAttribute('aria-pressed'), b.getAttribute('aria-pressed')]).toEqual(['true', 'false']);
    fireEvent.click(b);
    expect(onPick).toHaveBeenCalledWith(1);
  });

  it('with no options it is prose only: no group, no takeaway paragraph', () => {
    const { container } = render(<Chapter copy={C2} />);
    expect(screen.queryByRole('group')).toBeNull();
    expect(container.querySelector('.m-ch-take')).toBeNull();
    expect(screen.getByText(C2.lead)).toBeInTheDocument();
  });
});

describe('Words (spec §6)', () => {
  it('is one definition list with a term and a definition per word', () => {
    const { container } = render(<Words />);
    const dl = container.querySelector('dl.m-words') as HTMLElement;
    expect(dl.querySelectorAll('dt')).toHaveLength(WORDS.length);
    expect(dl.querySelectorAll('dd')).toHaveLength(WORDS.length);
    expect([...dl.querySelectorAll('dt')].map((d) => d.textContent)).toEqual(WORDS.map(([t]) => t));
  });
});
```

(The order assertion in the first test compares class names of the section's children after the heading; write it so it reads plainly: heading, `m-ch-lead`, `seg`, the child, `m-ch-take`.)

- [ ] **Step 2: Run them, see them fail**

Run: `npx vitest run src/math/story/copy.test.ts src/math/story/Chapter.test.tsx`
Expected: FAIL, the imports do not resolve.

- [ ] **Step 3: Write `web/src/math/story/copy.ts`**

Every string is the spec's §5.0, §5.1, §5.2 and §6, verbatim. Shape:

```ts
// The story's own copy (spec 2026-10-01 §5, §6). No figure lives here: payouts are "one dollar", every number
// a reader sees comes from the engine or the snapshot. A line that states an outcome is picked by the engine's
// own flag (TAKE), never fixed.
import type { BasketWord } from '../../schematics/BasketLegs';

export const HOOK =
  'Sometimes the prices of every outcome of an event add up to less than the one dollar the winner pays. In theory, that gap is free money. This page shows how a scanner finds it, sizes it and times it, and what can go wrong between seeing it and holding it. Every drawing runs the scanner’s own Rust code, compiled to WASM.';

export interface ChapterCopy {
  n: number;
  id: string;
  question: string;
  lead: string;
  /** The switch's accessible name. */
  switchLabel: string;
  /** null: chapter 3 (its labels are built from the preset's ticks) and chapter 7 (live data, no switch). */
  options: readonly [string, string] | null;
}

export const CHAPTERS: readonly ChapterCopy[] = [
  {
    n: 1, id: 's1', question: 'What is being traded?',
    lead: 'A market asks one yes-or-no question. A YES share pays one dollar if the answer is yes, a NO share pays one dollar if it is no. So an offer to sell YES at a price p (an ask) is the same offer as a bid for NO at 1 − p: one list of bids and asks, a book, drawn twice.',
    switchLabel: 'the book shown', options: ['a healthy book', 'a crossed book'],
  },
  {
    n: 2, id: 's2', question: 'What happens with several outcomes?',
    lead: 'Think of an election with candidates A, B and C, each with its own market. Hold one YES share of each: a basket, and each market in it is a leg. If one of the three must win, it pays one dollar whoever it is.',
    switchLabel: 'the outcomes listed', options: ['every outcome is listed', 'one outcome is not listed'],
  },
  {
    n: 3, id: 's3', question: 'When is that free money?',
    lead: 'A complete basket always pays one dollar. When its prices add up to less, the difference, on every basket the books can fill, is the edge, written Π: money on the table before anyone has traded. Buying one NO share of each works too: all but one of them pay, whoever wins, and the edge is there when the YES bids add up to more than one dollar.',
    switchLabel: 'what the prices add up to', options: null,
  },
  {
    n: 4, id: 's4', question: 'How much of it is there?',
    lead: 'Each price is good for a few shares only. Buy more and you reach the next, worse price. Walk down the books basket by basket and stop where the next one would no longer pay more than it costs: the size reached is Q*.',
    switchLabel: 'how deep the books are', options: ['a deep book', 'a thin book'],
  },
  {
    n: 5, id: 's5', question: 'How long does it last?',
    lead: 'An edge opens on one price update and closes on a later one: an episode. An order needs time to be decided, to travel and to be matched; it lands at t_x. If the episode has closed by then, there is nothing left to take.',
    switchLabel: 'how long the edge stays', options: ['it outlasts the order', 'it closes first'],
  },
  {
    n: 6, id: 's6', question: 'What if you get only part of it?',
    lead: 'A basket is several orders, one per leg. If one leg has no shares left at t_x, the others still fill, and those extra shares are a bet, not a basket. The way out is to sell them back at the bid: an unwind. The drawing uses a NO basket, which can be converted back into cash at once.',
    switchLabel: 'what fills', options: ['every leg fills', 'one leg misses'],
  },
  {
    n: 7, id: 's7', question: 'So does it pay?',
    lead: 'The live scanner keeps six paper accounts that differ only in latency, from the fastest rung to the slowest, and settles every trade by the rules of chapter 6.',
    switchLabel: '', options: null,
  },
];

export const TAKE = {
  ch1: (crossed: boolean) =>
    crossed ? 'A bid above the ask would be free money inside one market. The scanner distrusts its feed instead.' : 'One book, two views.',
  ch2: (broken: boolean) =>
    broken
      ? 'Someone outside the basket can win, and that column pays nothing. Not an arbitrage: see ✗.'
      : 'The basket pays the same in every column. That is what risk-free means here.',
  ch3: (signal: boolean) => (signal ? 'The scanner runs this check each time a price moves.' : 'At exactly one dollar there is nothing to take.'),
  ch4: 'The edge has a size. A thinner book reaches it sooner.',
  ch5: (caught: boolean) =>
    caught ? 'The order landed in time. Whether every leg fills is the next chapter.' : 'The edge was real. It just did not wait.',
  ch6: (word: BasketWord): string =>
    ({
      filled: 'Every leg filled the same amount, so nothing is left over.',
      caught: 'Some legs filled more than others. The extra shares are now a bet, and the exit is the bid.',
      'nothing filled': 'Nothing filled, so there is nothing to sell back.',
    })[word],
  ch7: 'Every figure here is on paper. No order was ever sent.',
} as const;

export const CH3_COUNT = 'The scanner counts each stretch an edge stays open as one episode. Today’s count:';
export const CH7_EXTRAS =
  'extras: one account whose matching is slower and slows further when the feed is busy, and one that ignores a market’s own order delay.';

export const PLAY = {
  title: 'Try it yourself',
  line: 'The same six drawings, with every control. Nothing you do here is recorded or sent anywhere.',
} as const;

export const WORDS: readonly (readonly [string, string])[] = [
  ['YES / NO share', 'pays one dollar if the answer is yes / no.'],
  ['book, bid, ask', 'the list of offers to buy (bids) and to sell (asks) one share type.'],
  ['basket, leg', 'one share of every outcome of an event; each market in it is a leg.'],
  ['NO basket', 'one NO share of each; all but one pay.'],
  ['edge (Π)', 'what a basket pays over what it costs, on every basket the books can fill.'],
  ['Q*', 'how many baskets are worth buying before the next costs as much as it pays.'],
  ['episode', 'one stretch during which an edge stays open.'],
  ['t_x', 'the moment an order lands at the exchange.'],
  ['unwind', 'selling extra shares back at the bid.'],
  ['convert', 'turning a NO basket back into cash at the exchange.'],
  ['on the table', 'an edge the books show right now, before any order.'],
  ['on paper', 'what a simulated account would have gained or lost; no order was sent.'],
];
```

Chapter 7's lead says "six": Task 1's test pins that `LADDER` has six rungs.

- [ ] **Step 4: Write `Chapter.tsx` and `Words.tsx`**

`web/src/math/story/Chapter.tsx`:

```tsx
import type { ReactNode } from 'react';
import { Seg } from '../sections/Section';
import type { ChapterCopy } from './copy';

/** Spec 2026-10-01 §3.1: a question, its lead, one two-state switch, the drawing, and the takeaway for the state
 *  shown. Without options (the engine is not there, or the chapter is live data) it is prose and children only. */
export function Chapter({ copy, options, value = 0, onPick, take, children }: {
  copy: ChapterCopy;
  options?: readonly [string, string] | null;
  value?: 0 | 1;
  onPick?: (v: 0 | 1) => void;
  take?: string | null;
  children?: ReactNode;
}) {
  const hid = `m-${copy.id}`;
  return (
    <section id={copy.id} className="m-ch" aria-labelledby={hid}>
      <h2 id={hid}>
        <span className="m-ch-n">{copy.n}</span> {copy.question}
      </h2>
      <p className="m-ch-lead">{copy.lead}</p>
      {options != null && onPick !== undefined && (
        <Seg<0 | 1> label={copy.switchLabel} options={[[0, options[0]], [1, options[1]]]} value={value} onPick={onPick} />
      )}
      {children}
      {take != null && <p className="m-ch-take">{take}</p>}
    </section>
  );
}
```

(In the tests above, pass `onPick` wherever a switch is expected.)

`web/src/math/story/Words.tsx`:

```tsx
import { WORDS } from './copy';

/** Spec 2026-10-01 §6: the story's terms, once, at the top of the playground. */
export function Words() {
  return (
    <dl className="m-words">
      {WORDS.map(([term, def]) => (
        <div key={term} className="m-word">
          <dt>{term}</dt>
          <dd>{def}</dd>
        </div>
      ))}
    </dl>
  );
}
```

- [ ] **Step 5: Run the tests, see them pass**

Run: `npx vitest run src/math/story/copy.test.ts src/math/story/Chapter.test.tsx`
Expected: all pass. If `checkHonesty` flags a word in the spec's copy, change that one word, keep the meaning, and report it.

- [ ] **Step 6: Check set and commit**

```bash
git add web/src/math/story/copy.ts web/src/math/story/copy.test.ts web/src/math/story/Chapter.tsx web/src/math/story/Words.tsx web/src/math/story/Chapter.test.tsx
git commit web/src/math/story -m "web: the story's copy, its chapter shell and the word list"
```

---

### Task 3: The seven chapters

**Files:**
- Create: `web/src/math/story/Story.tsx`, `web/src/math/story/Story.test.tsx`
- Modify: `web/src/schematics/MirrorBooks.tsx` (+ its test): prop `headline?: boolean` (default `true`) that renders the `p.mb-big` line only when true
- Modify: `web/src/math/sections/PayoffSection.tsx`: export `PayoffView({ m, kind }: { m: PayoffMatrix; kind: PayoffKind })`, the exact JSX the section renders inside `<Out>` today (the grid, and the formula line when `top_edge_gross` is not null); the section uses it
- Modify: `web/src/math/sections/EpisodeSection.tsx`: export `EpisodeView({ m }: { m: EpisodeModel })`, the exact JSX the section renders inside `<Out>` today (verdict word, captured line, times line, `UpdateBars`, `DurationBars`); the section uses it

**Interfaces:**
- Consumes: Task 1's presets; Task 2's `Chapter`, `CHAPTERS`, `TAKE`, `CH3_COUNT`, `CH7_EXTRAS`; `call`, `isError`; `Out` from `sections/Section.tsx`; the schematics `MirrorBooks`, `ClassifyTree`, `DepthStairs`, `BasketLegs` + `basketWord`; `LEGS`, `F_BOOKS` from `defaults.ts`.
- Produces:

```tsx
/** `engine` null: prose only (loading or failed). `strip` and `ladder` are what MathPage decided to show in
 *  chapters 3 and 7: the live component, the "unavailable" line, or null while the snapshot loads. */
export function Story(p: { engine: Engine | null; strip: ReactNode; ladder: ReactNode; ladderDrawn: boolean }): JSX.Element;
```

- [ ] **Step 1: Write the failing test** `web/src/math/story/Story.test.tsx`

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { CROSSED } from '../../schematics/MirrorBooks';
import { describeWasm, loadTestEngine } from '../../wasm/testEngine';
import type { Engine } from '../../wasm/types';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, TAKE } from './copy';
import { CH3_LABELS } from './presets';
import { Story } from './Story';

const sec = (c: HTMLElement, id: string) => c.querySelector(`section#${id}`) as HTMLElement;
const second = (s: HTMLElement) => fireEvent.click(within(s).getAllByRole('button').filter((b) => b.hasAttribute('aria-pressed'))[1]);
const take = (s: HTMLElement) => s.querySelector('.m-ch-take')?.textContent ?? null;

describe('Story without the engine (spec §3.2)', () => {
  it('reads as prose: seven headed chapters with their leads, no switch, no drawing, no takeaway except chapter 7', () => {
    const { container } = render(<Story engine={null} strip={null} ladder={null} ladderDrawn={false} />);
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(CHAPTERS.map((c) => `${c.n} ${c.question}`));
    for (const c of CHAPTERS) expect(within(sec(container, c.id)).getByText(c.lead)).toBeInTheDocument();
    expect(screen.queryByRole('group')).toBeNull();
    expect(container.querySelectorAll('figure, svg')).toHaveLength(0);
    expect([...container.querySelectorAll('.m-ch-take')].map((p) => p.textContent)).toEqual([TAKE.ch7]);
    expect(screen.queryByText(CH3_COUNT)).toBeNull();
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
  });
});

describeWasm('Story on the real engine (spec §3.1, §5.1)', () => {
  let engine: Engine;
  beforeAll(async () => { engine = await loadTestEngine(); });
  const draw = () => render(<Story engine={engine} strip={<p>STRIP</p>} ladder={<p>LADDER</p>} ladderDrawn />).container;

  it('chapters 1 to 6 each have a two-option switch on its first option, a drawing and a takeaway', () => {
    const c = draw();
    for (const ch of CHAPTERS.slice(0, 6)) {
      const s = sec(c, ch.id);
      const pressed = within(s).getAllByRole('button').filter((b) => b.hasAttribute('aria-pressed'));
      expect(pressed.map((b) => b.getAttribute('aria-pressed')), ch.id).toEqual(['true', 'false']);
      expect(s.querySelector('figure, svg'), ch.id).not.toBeNull();
      expect(take(s), ch.id).not.toBeNull();
    }
  });

  it('chapter 1: no D1 headline; healthy says two views; crossed shows the alert once and the feed line', () => {
    const c = draw();
    const s = sec(c, 's1');
    expect(s.querySelector('.mb-big')).toBeNull();
    expect(take(s)).toBe(TAKE.ch1(false));
    second(s);
    expect(within(s).getAllByText(CROSSED)).toHaveLength(1);
    expect(take(s)).toBe(TAKE.ch1(true));
  });

  it('chapter 2: listed says risk-free; not listed shows a ✗ column and says so', () => {
    const s = sec(draw(), 's2');
    expect(take(s)).toBe(TAKE.ch2(false));
    expect(s.textContent).not.toContain('✗');
    second(s);
    expect(s.textContent).toContain('✗');
    expect(take(s)).toBe(TAKE.ch2(true));
  });

  it('chapter 3: the labels are the tick sums; 1.00 has nothing, the second state has a framed edge; then the count line and the strip', () => {
    const s = sec(draw(), 's3');
    expect(within(s).getAllByRole('button').filter((b) => b.hasAttribute('aria-pressed')).map((b) => b.textContent)).toEqual([...CH3_LABELS]);
    expect(take(s)).toBe(TAKE.ch3(false));
    expect(s.textContent).not.toMatch(/\$\d/);
    second(s);
    expect(take(s)).toBe(TAKE.ch3(true));
    expect(s.textContent).toMatch(/\$\d[\d.,]* on the table/);
    const order = [...s.children].map((k) => k.textContent);
    expect(order.indexOf(CH3_COUNT)).toBeGreaterThan(order.indexOf(TAKE.ch3(true)));
    expect(order.indexOf('STRIP')).toBe(order.indexOf(CH3_COUNT) + 1);
  });

  it('chapter 4: the thin book changes the drawing', () => {
    const s = sec(draw(), 's4');
    const before = s.querySelector('svg')?.outerHTML;
    second(s);
    expect(s.querySelector('svg')?.outerHTML).not.toBe(before);
    expect(take(s)).toBe(TAKE.ch4);
  });

  it('chapter 5: caught, then missed, each with the matching line', () => {
    const s = sec(draw(), 's5');
    expect(within(s).getByText('caught')).toBeInTheDocument();
    expect(take(s)).toBe(TAKE.ch5(true));
    second(s);
    expect(within(s).getByText('missed')).toBeInTheDocument();
    expect(take(s)).toBe(TAKE.ch5(false));
  });

  it('chapter 6: filled, then caught, each with the matching line', () => {
    const s = sec(draw(), 's6');
    expect(take(s)).toBe(TAKE.ch6('filled'));
    second(s);
    expect(take(s)).toBe(TAKE.ch6('caught'));
  });

  it('chapter 7: the ladder, the extras line only when the ladder is drawn, and the fixed line', () => {
    const s = sec(draw(), 's7');
    expect(within(s).getByText('LADDER')).toBeInTheDocument();
    expect(within(s).getByText(CH7_EXTRAS)).toHaveClass('dim');
    expect(take(s)).toBe(TAKE.ch7);
    const { container } = render(<Story engine={engine} strip={null} ladder={<p>unavailable</p>} ladderDrawn={false} />);
    expect(within(sec(container, 's7')).queryByText(CH7_EXTRAS)).toBeNull();
    expect(within(sec(container, 's3')).queryByText(CH3_COUNT)).toBeNull();
  });

  it('review focus 3: switching one chapter moves no other', () => {
    const c = draw();
    const others = () => ['s1', 's2', 's4', 's5', 's6'].map((id) => sec(c, id).innerHTML);
    const before = others();
    second(sec(c, 's3'));
    expect(others()).toEqual(before);
  });

  it('review focus 1: an engine that answers with an error shows its sentence and no takeaway', () => {
    const broken: Engine = { ...engine, depth_walk_json: () => JSON.stringify({ error: 'levels must be cheapest first' }) };
    const { container } = render(<Story engine={broken} strip={null} ladder={null} ladderDrawn={false} />);
    const s = sec(container, 's4');
    expect(within(s).getByRole('status')).toHaveTextContent('levels must be cheapest first');
    expect(take(s)).toBeNull();
    expect(take(sec(container, 's1'))).toBe(TAKE.ch1(false));
  });
});
```

Also add to `web/src/schematics/MirrorBooks.test.tsx` (use that file's own way of building a model):

```tsx
it('headline={false} leaves the D1 line out and keeps the drawing', () => {
  // render <MirrorBooks … headline={false} /> with the file's fed-book model
  // expect(container.querySelector('.mb-big')).toBeNull();
  // expect(container.querySelector('svg')).not.toBeNull();
});
it('by default the D1 line is there', () => {
  // expect(container.querySelector('.mb-big')).not.toBeNull();
});
```

(Write both with real code, following the helpers already in that test file.)

- [ ] **Step 2: Run, see it fail**

Run: `WASM_REQUIRED=1 npx vitest run src/math/story/Story.test.tsx src/schematics/MirrorBooks.test.tsx`
Expected: FAIL, `./Story` does not resolve; the `headline` test fails.

- [ ] **Step 3: `MirrorBooks` headline prop; `PayoffView`; `EpisodeView`**

- `MirrorBooks({ bids, asks, model, headline = true })`: wrap the existing `<p className="mb-big">…</p>` in `{headline && (…)}`. Nothing else changes.
- `PayoffSection.tsx`: move the JSX inside `<Out out={out}>{(m) => (…)}</Out>` into `export function PayoffView({ m, kind }: { m: PayoffMatrix; kind: PayoffKind })` (it needs `FORMULA[kind]`, `edgeNote(m)`, `matrixToPayoff(m, kind)`); the section renders `<Out out={out}>{(m) => <PayoffView m={m} kind={input.kind} />}</Out>`.
- `EpisodeSection.tsx`: move the JSX inside its `<Out>` into `export function EpisodeView({ m }: { m: EpisodeModel })`; the section renders `<Out out={out}>{(m) => <EpisodeView m={m} />}</Out>`.

Run the sections' tests: `WASM_REQUIRED=1 npx vitest run src/math/sections-abc.test.tsx src/math/sections-def.test.tsx src/schematics/MirrorBooks.test.tsx`. Expected: all pass (pure moves).

- [ ] **Step 4: Write `web/src/math/story/Story.tsx`**

```tsx
import { type ReactNode, useMemo, useState } from 'react';
import { BasketLegs, basketWord } from '../../schematics/BasketLegs';
import { ClassifyTree } from '../../schematics/ClassifyTree';
import { DepthStairs } from '../../schematics/DepthStairs';
import { MirrorBooks } from '../../schematics/MirrorBooks';
import { call } from '../../wasm/call';
import { type BasketModel, type ClassifyModel, type Engine, type EngineFn, type EpisodeModel, type MirrorModel, type PayoffMatrix, type WalkModel, isError } from '../../wasm/types';
import { F_BOOKS, LEGS } from '../defaults';
import { EpisodeView } from '../sections/EpisodeSection';
import { PayoffView } from '../sections/PayoffSection';
import { Out } from '../sections/Section';
import { Chapter } from './Chapter';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, type ChapterCopy, TAKE } from './copy';
import { CH1, CH2, CH3, CH3_LABELS, CH4, CH5, CH6, type Pair } from './presets';

/** One engine-backed chapter: its own two-state switch, the engine's answer for the state shown, the drawing,
 *  and the takeaway picked from that answer (none when the engine answered with an error). */
function Preset<I, M>({ engine, copy, fn, inputs, options, draw, take }: {
  engine: Engine | null;
  copy: ChapterCopy;
  fn: EngineFn;
  inputs: Pair<I>;
  options: readonly [string, string];
  draw: (m: M, input: I) => ReactNode;
  take: (m: M) => string;
  children?: ReactNode;
}) {
  const [v, setV] = useState<0 | 1>(0);
  const out = useMemo(() => (engine === null ? null : call<M>(engine, fn, inputs[v])), [engine, fn, inputs, v]);
  if (out === null) return <Chapter copy={copy} />;
  return (
    <Chapter copy={copy} options={options} value={v} onPick={setV} take={isError(out) ? null : take(out)}>
      <Out out={out}>{(m) => draw(m, inputs[v])}</Out>
    </Chapter>
  );
}

const F_NAMES = LEGS.slice(0, F_BOOKS.length);
const [C1, C2, C3, C4, C5, C6, C7] = CHAPTERS;

/** Spec 2026-10-01 §3, §5.1. `engine` null: prose only. `strip` / `ladder`: what the page shows for the live
 *  parts (the component, the "unavailable" line, or null while the snapshot loads). */
export function Story({ engine, strip, ladder, ladderDrawn }: { engine: Engine | null; strip: ReactNode; ladder: ReactNode; ladderDrawn: boolean }) {
  return (
    <>
      <Preset<typeof CH1[0], MirrorModel> engine={engine} copy={C1} fn="mirror_json" inputs={CH1} options={C1.options!}
        draw={(m, i) => <MirrorBooks bids={i.bids} asks={i.asks} model={m} headline={false} />} take={(m) => TAKE.ch1(m.crossed)} />
      <Preset<typeof CH2[0], PayoffMatrix> engine={engine} copy={C2} fn="payoff_json" inputs={CH2} options={C2.options!}
        draw={(m, i) => <PayoffView m={m} kind={i.kind} />} take={(m) => TAKE.ch2(m.broken.some(Boolean))} />
      <Ch3 engine={engine} strip={strip} />
      <Preset<typeof CH4[0], WalkModel> engine={engine} copy={C4} fn="depth_walk_json" inputs={CH4} options={C4.options!}
        draw={(m) => <DepthStairs model={m} />} take={() => TAKE.ch4} />
      <Preset<typeof CH5[0], EpisodeModel> engine={engine} copy={C5} fn="episode_json" inputs={CH5} options={C5.options!}
        draw={(m) => <EpisodeView m={m} />} take={(m) => TAKE.ch5(m.caught)} />
      <Preset<typeof CH6[0], BasketModel> engine={engine} copy={C6} fn="basket_json" inputs={CH6} options={C6.options!}
        draw={(m) => <BasketLegs model={m} names={F_NAMES} />} take={(m) => TAKE.ch6(basketWord(m))} />
      <Chapter copy={C7} take={TAKE.ch7}>
        {ladder}
        {ladderDrawn && <p className="dim m-note">{CH7_EXTRAS}</p>}
      </Chapter>
    </>
  );
}
```

Chapter 3 needs the count line and the strip **after** the takeaway, so it is its own component (same pattern, with the extra children after `take`). `Chapter` renders `children` before the takeaway; give `Chapter` one more optional prop `after?: ReactNode`, rendered after the takeaway, and use it here:

```tsx
function Ch3({ engine, strip }: { engine: Engine | null; strip: ReactNode }) {
  const [v, setV] = useState<0 | 1>(0);
  const out = useMemo(() => (engine === null ? null : call<ClassifyModel>(engine, 'classify_json', CH3[v])), [engine, v]);
  const live = strip !== null && (
    <>
      <p className="m-ch-lead">{CH3_COUNT}</p>
      {strip}
    </>
  );
  if (out === null) return <Chapter copy={C3} after={live} />;
  return (
    <Chapter copy={C3} options={CH3_LABELS} value={v} onPick={setV} take={isError(out) ? null : TAKE.ch3(out.kind !== null)} after={live}>
      <Out out={out}>{(m) => <ClassifyTree model={m} />}</Out>
    </Chapter>
  );
}
```

Add `after` to `Chapter` (rendered last, after the takeaway) and one test for it in `Chapter.test.tsx`: "`after` comes after the takeaway". Without the engine and without a strip the first test above (`CH3_COUNT` absent) holds; with a strip and no engine the count line and the strip still show (prose plus live data): keep that, and add the assertion to the "without the engine" test with a second render (`strip={<p>STRIP</p>}`): `CH3_COUNT` and `STRIP` present, no group.

Avoid the non-null assertions (`C1.options!`) if the linter or `tsc` settings reject them: narrow with a small helper `const opts = (c: ChapterCopy) => { if (c.options === null) throw new Error(c.id); return c.options; }`.

- [ ] **Step 5: Run the tests, see them pass**

Run: `WASM_REQUIRED=1 npx vitest run src/math/story src/schematics/MirrorBooks.test.tsx src/math/sections-abc.test.tsx src/math/sections-def.test.tsx`
Expected: all pass, no `act()` warning.

- [ ] **Step 6: Check set and commit**

```bash
git add web/src/math/story/Story.tsx web/src/math/story/Story.test.tsx
git commit web/src/math/story web/src/schematics/MirrorBooks.tsx web/src/schematics/MirrorBooks.test.tsx web/src/math/sections/PayoffSection.tsx web/src/math/sections/EpisodeSection.tsx -m "web: the seven story chapters on the real engine, each with its own before/after switch"
```

---

### Task 4: The playground's framing (h3 sections, new titles, back-references)

**Files:**
- Modify: `web/src/math/sections/Section.tsx` (h3; a `chapter` prop rendered as dim text), `web/src/math/copy.ts` (titles; `chapter` per section), the six `sections/*.tsx` only if they pass props explicitly, `web/src/math/sections/LegMissSection.tsx` (drop the `children` slot)
- Modify tests: `web/src/math/sections-abc.test.tsx`, `web/src/math/sections-def.test.tsx`, `web/src/math/live.test.tsx`, `web/src/math/MathPage.test.tsx` only where a title string, a heading level or f's live part is asserted

**Interfaces:**
- Produces: `SECTIONS.<x>.title` = the spec §5.2 titles; `SECTIONS.<x>.chapter` = `'chapter 1'` … `'chapter 6'`; `Section` renders `<h3 id="m-<id>">` and, under it, `<p className="dim m-back">{chapter}</p>`; `LegMissSection({ engine })` with no children.

- [ ] **Step 1: Write the failing tests** (in `sections-abc.test.tsx`, beside the existing title checks; real engine as that file does)

```tsx
it('each section is an h3 with its new title and says which chapter introduced it, as text, not a link', () => {
  // render the six sections as the file already does
  const want: [string, string, string][] = [
    ['a', 'a · the book, twice', 'chapter 1'],
    ['b', 'b · the basket’s payoff', 'chapter 2'],
    ['c', 'c · is this update an edge?', 'chapter 3'],
    ['d', 'd · how much: the depth walk', 'chapter 4'],
    ['e', 'e · how long: the episode', 'chapter 5'],
    ['f', 'f · one leg misses', 'chapter 6'],
  ];
  for (const [id, title, chapter] of want) {
    const s = container.querySelector(`section#${id}`) as HTMLElement;
    expect(within(s).getByRole('heading', { level: 3 })).toHaveTextContent(title);
    expect(s).toHaveAccessibleName(title);
    expect(within(s).getByText(chapter)).toHaveClass('dim');
    expect(s.querySelector('a')).toBeNull();
    expect(s.querySelector('h2')).toBeNull();
  }
});
```

And in `sections-def.test.tsx`: "section f has no live part of its own" (`container.querySelector('#f .m-liveladder')` is null when rendering `LegMissSection`).

- [ ] **Step 2: Run, see them fail** (`h3` not found; old titles).

- [ ] **Step 3: Implement**

- `copy.ts`: titles per the spec §5.2 (typographic apostrophe `’` as the rest of the file), plus `chapter: 'chapter N'` in each `SECTIONS` entry.
- `Section.tsx`: `<h3 id={`m-${id}`}>{title}</h3>` then `<p className="dim m-back">{chapter}</p>`; add `chapter: string` to the props (sections spread `{...SECTIONS.x}`, so it flows).
- `LegMissSection`: remove `children` and `after`; if `Section`'s `after` prop has no other user, remove it too.
- Update every existing assertion that names an old title or an `h2` section heading to the new title / `h3`; update tests that looked for the ladder inside `#f` only if they live in the section tests (the page-level ones are Task 5). No assertion is dropped: where a test checked "f ends with the live ladder" at page level, leave it failing for Task 5 **only if** it lives in `MathPage.test.tsx`; then do Task 5's change for that one assertion here so the suite is green at this commit, and say so in the report.

- [ ] **Step 4: Run** `WASM_REQUIRED=1 npm test` — all pass. `npx tsc --noEmit` and `-p tsconfig.math.json` silent.

- [ ] **Step 5: Commit**

```bash
git commit web/src/math -m "web: the playground's sections are h3s named after the chapter that introduced them; f no longer carries the live ladder"
```

---

### Task 5: The page (assembly, live parts, cold link, CSS, meta)

**Files:**
- Modify: `web/src/math/MathPage.tsx`, `web/src/math/copy.ts` (remove `TAGLINE`, export nothing unused), `web/src/math/math.css`, `web/math.html` (meta description), `web/src/math/MathPage.test.tsx`, `web/src/math/math.css.test.ts`, `web/src/theme.test.ts` (only if a pin lives there), `web/src/math/checkBundle.test.ts` only if it pins the meta text

**Interfaces:**
- Consumes: `Story`, `Words`, `HOOK`, `PLAY`; `LiveStrip`, `LiveUnavailable`, `LiveLadder`, `LiveBoundary`; the six sections.

Page structure to produce (spec §3):

```tsx
<div className="app math">
  <header className="top m-top"><h1>{TITLE}</h1></header>
  <main className="body m-body">
    <p className="m-tag">{HOOK}</p>
    {eng.kind === 'loading' && <p className="dim" role="status">{LOADING}</p>}
    {eng.kind === 'failed' && <p className="error" role="alert">{ENGINE_FAILED}</p>}
    <Story engine={eng.kind === 'ok' ? eng.engine : null} strip={strip} ladder={ladder} ladderDrawn={snap !== null} />
    <section id="play" className="m-play" aria-labelledby="m-play">
      <h2 id="m-play">{PLAY.title}</h2>
      <p className="m-ch-lead">{PLAY.line}</p>
      <Words />
      {eng.kind === 'ok' && (<> …the six sections, in order a–f… </>)}
    </section>
  </main>
  <footer className="m-foot"><p>{FOOTER}</p></footer>
</div>
```

where, with `snap`, `stale`, `unavailable` computed exactly as today:

```tsx
const strip = snap !== null
  ? <LiveBoundary key={snap.written_ms} onError={() => setBrokeOn(snap.written_ms)}><LiveStrip today={snap.today} writtenMs={snap.written_ms} stale={stale} /></LiveBoundary>
  : unavailable ? <LiveUnavailable /> : null;
const ladder = snap !== null
  ? <LiveBoundary key={snap.written_ms} onError={() => setBrokeOn(snap.written_ms)}><LiveLadder snap={snap} stale={stale} /></LiveBoundary>
  : unavailable ? <LiveUnavailable /> : null;
```

Cold link (spec §3.2): `useSectionLink(ready)` where `ready = eng.kind === 'ok' && load.kind !== 'loading'`, accepting `/^#([a-f]|play)$/`; still once, still the browser's default `scrollIntoView()`.

- [ ] **Step 1: Rewrite the page tests that move, and add the new ones** (in `MathPage.test.tsx`, using the file's existing helpers for the engine promise, the fetch stub and the sample snapshot)

Assertions to change (spec §8; each keeps its intent):

| Today | Becomes |
|---|---|
| tagline text | `HOOK` |
| loading / failed: "no section" (`h2` count 0) | eight `h2` (seven chapters + "Try it yourself"), no `section#a`, no `figure`/`svg` inside `.m-ch` for chapters 1–6, the loading line / the failed sentence exactly once |
| "the live strip already draws" while loading | still true: the strip is inside `#s3` |
| `LIVE_UNAVAILABLE` exactly once (`unavailableShown`, lines ~82, 183, 239, 251, 389, 401) | exactly once inside `#s3` and exactly once inside `#s7` (rewrite `unavailableShown` to assert both) |
| "f ends with the live ladder" | the ladder is inside `#s7`; `#f` has no `.m-liveladder` |
| "draws a–f in order under the live strip" | order of `main`'s children: hook, story chapters `s1…s7`, `#play`; inside `#play`: h2, line, `dl.m-words`, then `a…f` |
| stale: "in the strip and above the ladder" | the same two places, now `#s3` and `#s7` |
| `/math#d` scrolls once the sections exist | once the engine is ready **and** the snapshot has settled |

New tests:

```tsx
it('one h1; chapters and the playground are h2; sections are h3', async () => {
  // full page, engine ok, sample snapshot
  expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
  expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(8);
  expect(screen.getAllByRole('heading', { level: 3 }).length).toBeGreaterThanOrEqual(6);
});

it('review focus 2: on a cold /math#play with a late snapshot, the scroll waits for the snapshot', async () => {
  // hash '#play'; engine resolves first; the fetch stub is held open
  // expect(scrollIntoView).not.toHaveBeenCalled() while the snapshot is loading
  // resolve the fetch; await the strip; expect(scrollIntoView).toHaveBeenCalledTimes(1) on #play
});

it('review focus 4: a day with no episodes and an epoch with no rungs still reads true', async () => {
  // snapshot built from the sample with today.arb/no/yes = 0, hourly all 0, ladder.rungs = [], break_even kind 'none'
  // the page renders; #s3 shows the count line and a strip with 0; #s7 shows the ladder's own "no rungs" line
  // expectHonest(container, 'an empty day');
});

it('the story is honest in both states of every chapter', async () => {
  // full page; expectHonest(container, 'story: first states');
  // click the second option of the switch in s1…s6; expectHonest(container, 'story: second states');
});
```

Keep every existing `expectHonest` state (crossed book, basket kinds, fees, the usual burst, a market cell): the market cell is now found inside `#s7`.

- [ ] **Step 2: Run, see the new and changed tests fail**

Run: `WASM_REQUIRED=1 npx vitest run src/math/MathPage.test.tsx`

- [ ] **Step 3: Implement `MathPage.tsx`** as above. Remove `TAGLINE` from `copy.ts` and its import. Update `web/math.html`'s `<meta name="description">` to: `How a scanner finds a basket arbitrage on Polymarket, sizes it and times it, and what can go wrong between seeing it and holding it. Drawn by the scanner's own Rust code.` (no `$`, no claim about the result). If a test pins the old description, update the pin.

- [ ] **Step 4: CSS** in `web/src/math/math.css` (and pins in `math.css.test.ts`)

```css
/* The story (spec 2026-10-01 §3): chapters stack; each is a question, a lead, one switch, a drawing, a takeaway. */
.m-ch, .m-play { display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; min-width: 0; padding-top: 14px; border-top: 3px solid var(--line); }
.m-ch h2, .m-play h2 { font-size: 20px; line-height: 1.25; margin: 0; overflow-wrap: anywhere; }
.m-ch-n { font-family: var(--big); font-size: 28px; line-height: 1; color: var(--teal); margin-right: 6px; }
.m-ch-lead, .m-ch-take { margin: 0; max-width: 78ch; overflow-wrap: anywhere; }
.m-ch-take { font-weight: 700; border-left: 3px solid var(--teal); padding-left: 10px; }
.m-ch .seg { display: flex; flex-wrap: wrap; gap: 6px; }
.m-ch .seg button { min-height: 44px; padding: 8px 10px; }
.m-ch .fold summary { min-height: 44px; display: flex; align-items: center; gap: 6px; }
.m-ch .ll-svg { border: 0; }
.m-ch .fold { border: 0; border-top: 1px dashed var(--line2); margin-top: 0; }
.m-ch .fold > summary { padding: 0; }
.m-ch .fold-body { padding: 0 0 8px; }
.m-ch[id], .m-play[id] { scroll-margin-top: 12px; }
.m-words { display: grid; gap: 4px; margin: 0; font-size: 13px; }
.m-word { display: grid; grid-template-columns: minmax(0, 12ch) minmax(0, 1fr); gap: 8px; }
.m-word dt { font-weight: 700; overflow-wrap: anywhere; }
.m-word dd { margin: 0; overflow-wrap: anywhere; }
.m-back { margin: 0; font-size: 13px; }
/* The live ladder's "the numbers": a rung's name stays on one line (seen breaking mid-word at 360 px). */
.m-liveladder table th:first-child, .m-liveladder table td:first-child { white-space: nowrap; overflow-wrap: normal; }
```

Check the real selectors first: the colour tokens (`--teal`, `--line`, `--line2`, `--big`) exist in `theme.css`; the "numbers" table's real class (read `LatencyLadder.tsx`) may give a tighter selector than `table`; the live strip keeps its box (`.m-live`) inside the chapter; `.m-liveladder`'s own top rule/padding (it was "under the section's caption") may now double the chapter's: remove what doubles. If `.m-sec .seg button` and `.m-ch .seg button` can share one rule, share it.

Pins to add in `math.css.test.ts` (follow the file's existing style of reading the CSS text):
- no `minmax(<N>px` anywhere (the file's existing rule covers the new lines);
- `.m-ch .seg` has `flex-wrap: wrap` (review focus 5);
- `.m-ch .seg button` and `.m-ch .fold summary` have `min-height: 44px`;
- the first-column `white-space: nowrap` rule for the live ladder's table;
- `.m-ch[id]` has `scroll-margin-top`.

- [ ] **Step 5: Run everything**

Run: `WASM_REQUIRED=1 npm test` — all pass, no warnings. `npx tsc --noEmit`, `npx tsc --noEmit -p tsconfig.math.json` — silent.
Then the release build and the size check, from the repo root: `sh scripts/build-wasm.sh && (cd web && npm run build)`; expected `math-assets total: <n> B gzip (limit 350000)` and no `check-math-bundle:` error; report `<n>`. Afterwards restore the dev package: `PROFILE=dev sh scripts/build-wasm.sh`.

- [ ] **Step 6: Commit**

```bash
git commit web/src/math web/math.html -m "web: /math tells the story first (seven chapters, the live strip in chapter 3, the live ladder in chapter 7), then the playground with its word list"
```

---

### Task 6: The deploy runbook

**Files:**
- Create: `docs/superpowers/runbooks/math-story-web-vps.md`

Owner's format (binding): flat numbered steps; each step ONE command block, then `→ expected: …`, then `if not: …`; `**STOP**` after any step whose failure must halt; one-line notes only; self-contained (never "see another runbook" for a command); rollback as commands; a Results block. Header notes: nothing is installed on the VPS; never print `.env`; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker; never `--remove-orphans`. VPS: `ssh -p 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" root@31.97.23.123`, deploy dir `/home/isoca/projects/cute-trading`, domain `radar.isoca.space`; the owner runs every VPS step. **Never execute a block that contains `ssh`, `scp` or the domain while writing or testing the runbook: `bash -n` on the extracted text only.**

Copy the proven steps from `docs/superpowers/runbooks/phase7b-web-vps.md` (same commands, adapted names), in this order:

1. (PC, the `math-story` worktree) branch clean and pushed, PR CI green (`gh pr checks`: compose, rust, wasm, web). STOP.
2. (PC, main checkout) the owner merges the PR; `git checkout -q main && git pull -q --ff-only`; clean; `git log -1` shows the merge. STOP.
3. (PC) `cargo clean` in the worktrees that exist (`phase7b`, `phase7b-B`, `math-story`), then `scripts/disk-budget.ps1`.
4. (PC) release engine, `npm ci`, `WASM_REQUIRED=1 npm test`, `npm run build` with the size line (7b-B's step 6, same greps). STOP.
5. (PC) build the web image (7b-B's step 7).
6. (PC) image contents (7b-B's step 8), plus: `grep -c 'Try it yourself' math-assets/*.js` ≥ 1 (the new page is in the image). STOP.
7. (PC) bundle `~/ct-story.tar` (image + `compose.yml`, `compose.web.yml`, `.env.example`).
8. (PC) `scp`.
9. (VPS) rollback tag `cute-trading/web:pre-story` from the running web container's image ID (only if absent) and `cp -p` of both compose files to `/root/*.pre-story` (never overwritten). STOP.
10. (VPS) load the bundle; `docker image ls cute-trading/web`; disk budget. STOP.
11. (VPS) `config -q` ok; `cmp` says `compose.yml` and `compose.web.yml` are both identical to the saved copies (this change touches neither). If they differ: print the diff, put the saved files back, STOP. STOP.
12. (VPS, advisory) quiet minute (7b-B's step 16).
13. (VPS) record the scanner's start time, recreate `web` only (`up -d --no-build --no-deps web`), check `running 0`, on the new image and not on `pre-story`, mount read-only, `ct-math 8081` (7b-B's step 17, with its `--force-recreate` if-not). STOP.
14. (VPS) scanner not restarted; reconnects since the recreate (recorded).
15. (PC) public paths (7b-B's step 19) plus `curl -s https://radar.isoca.space/math-assets/<js> | grep -c 'Try it yourself'` ≥ 1. STOP.
16. (PC) gated paths (7b-B's step 20). STOP.
17. (PC) traversal probes (7b-B's step 21). STOP.
18. (Chrome, private window, 1440 px) `/math`: the hook; seven numbered chapters, each with a drawing; each switch changes its drawing and its takeaway; chapter 3 shows today's counts; chapter 7 the ladder; "Try it yourself" with the word list and sections a–f working; console clean; no request to `/api/`.
19. (Chrome, 360 × 740) the console line `document.documentElement.scrollWidth <= window.innerWidth` → `true` with every fold open; switch labels wrap, nothing cut; in chapter 7's "the numbers" the rung names stay on one line.
20. (Chrome, logged in) the dashboard `/` and `/paper` still load. STOP.
21. Rollback: `docker tag cute-trading/web:pre-story cute-trading/web:dev`, recreate `web`, verify the image ID equals `pre-story` and `/math` answers 200 with the old page (`grep -c 'Try it yourself'` = 0). The compose files do not change in this deploy, so there is nothing else to restore.

Results block: rollback tag ID; compose `cmp` result; quiet minute; reconnects; the public / gated / traversal status lines; bundle size; Chrome notes (1440, 360).

- [ ] **Step 1:** Write the runbook with every command in full (no "as in 7b-B": the text above is the outline for you; the runbook must be self-contained). Take each command from `phase7b-web-vps.md` and adapt names (`pre7b-B` → `pre-story`, `ct-7bB` → `ct-story`, file names under `/root`).
- [ ] **Step 2:** `bash -n` on every extracted block; no CR bytes; counts of steps, `→ expected:`, `if not:` equal; `docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q` passes in the worktree.
- [ ] **Step 3:** Commit: `git add docs/superpowers/runbooks/math-story-web-vps.md && git commit docs/superpowers/runbooks/math-story-web-vps.md -m "runbook: the /math story: web image only, rollback tag pre-story, probes and Chrome checks"`

---

## After the tasks (controller)

- Build the web image from the branch, run it locally on the public port with a sample `public.json`, and look at every chapter in both states at 360 and 1440 px in headless Chrome (the session's `shoot.mjs`), plus the traversal probes on the local port.
- Whole-branch review, one fix wave, scoped re-review.
- Push, open the PR, hand the owner the runbook.

## Self-review notes

- Spec coverage: §3 → Tasks 2, 3, 5; §3.2 → Tasks 3 (engine null, engine error), 5 (snapshot states, cold link); §4 → Task 1; §5–§6 → Task 2 (copy), 3 (takeaways by engine flag), 4 (titles, back-references); §7 → Tasks 1–5; §8 → each task's tests; §9 → Task 6.
- Names used across tasks: `Pair`, `CH1…CH6`, `CH3_LABELS`, `SHORT_T`, `episodeInput(st, times)`, `crossed`, `HOOK`, `CHAPTERS`, `ChapterCopy`, `TAKE`, `CH3_COUNT`, `CH7_EXTRAS`, `PLAY`, `WORDS`, `Chapter` (props `copy, options, value, onPick, take, children, after`), `Words`, `Story` (props `engine, strip, ladder, ladderDrawn`), `PayoffView`, `EpisodeView`, `MirrorBooks` `headline`.
