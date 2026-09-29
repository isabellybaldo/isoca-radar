# Phase 7a-B: the latency answer on the dashboard (web) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the answer to "at which latency would this pay?" on the dashboard: the latency-ladder hero in an explain panel on every page (plus four page questions), the Sweet spots grid on `/sweet`, and epochs with a once-a-day reset on the Paper page, then deploy `web` alone (step B).
**Architecture:** Web only; the api is live since deploy A. Pure models live in `web/src/lib/ladder.ts`, `web/src/lib/epochs.ts`, `web/src/lib/sweet.ts` and `web/src/explain/models.ts`; drawings live in `web/src/schematics/` and take a typed `model` (they never fetch). The explain panel (`web/src/explain/`) reads what pages already fetched through `ExplainContext` and makes exactly one fetch of its own, `/api/paper/ladder`, only while open (60 s poll). The Paper page gets `useEpochs` (10 s poll only while a reset is pending), an epoch line, an `earlier epochs` fold that fetches only when opened, and the `reset paper money` fold (the api's one write).
**Tech Stack:** React 19.2, react-router 7, TypeScript 5.9, Vite 8, Vitest 4 + Testing Library (jsdom), hand-made SVG and HTML (no chart library), plain CSS in `web/src/theme.css`; Docker (node 22 build → nginx-unprivileged) for deploy B.
**Spec:** docs/superpowers/specs/2026-09-29-phase7a-latency-answer-design.md

## Global Constraints

- **Paper only, honest copy (spec §9.4, 09-27 §7.2):** the panel, the Sweet grid legend/detail line and the reset fold never use the whole words `earned`, `earn`, `made`, `income` or `profitable` (any case); `profit` only inside `profit @ $100`, `net profit at the peak`, `profit after taker fees`, `profit before taker fees`; every `$` figure sits in a line/cell/table labelled `paper`, `on the table` or `captured at N ms`; the hero says **"basket gains"**, **"leg-risk cost"**, **"pays on paper"** / **"loses on paper"**, never "profit" and never that a latency "is profitable". Fix the copy, never widen the rule.
- **No new npm dependency** (runtime or dev) without owner approval; `package.json` and `package-lock.json` are not touched.
- **Exact decimals stay strings:** money text goes through `web/src/lib/money.ts` (`formatMoney`, `formatMoneyCents`, `signOf`, `isZero`, `compareDecimal`) or `<Money>`; `Number(...)` on a money string only for SVG/flex geometry (as `IntervalChart`, `LineChart`, Radar's `EdgeBar` already do), never for a number shown as text.
- **Identity:** Press Start 2P (`var(--px)`) for section titles, chips, tab and table heads only; VT323 (`var(--big)`) for big numbers only; IBM Plex Mono (`var(--body)`) sentence case for labels; night palette tokens from `theme.css` (`--teal`, `--mint`, `--salmon`, `--rose`, `--line2`, `--dim`…); the light theme stays a joke (`FakeToggle`, untouched).
- **Drawings over text:** title row → big number only where it is the answer → one drawing → closed folds for method/tables; one hero per page; no box inside a box; prose to a line; each explain answer = one drawing + at most two sentences.
- **SVG rules:** drawings use `preserveAspectRatio="none"`, so every label is HTML beside the SVG, never SVG `<text>`; every SVG part gets its own class (`ll-*`, `cb-*`, `db-*`), never a shared class like `.dot` that global CSS could size; thin strokes carry `vector-effect: non-scaling-stroke`.
- **`web/src/theme.test.ts` rules:** no `width:`/`min-width:` above 328 px; no `minmax(<N>px` (write `minmax(min(100%, Npx), …)`); new CSS goes into `web/src/theme.css` only.
- **Folds keep content in the DOM:** a value shown twice (drawing label + folded table) needs `getAllByText(...)` with the exact count.
- **UI tasks (3, 4, 5, 6, 7, 8, 10, 11):** the implementer loads the `frontend-design:frontend-design` skill before writing components or CSS, within the constraints above.
- **Test commands (from `web/`):** one file `npx vitest run <path>`; all `npm test` (= `vitest run`); types `npx tsc --noEmit` (vitest does not catch unused imports; `noUnusedLocals` is on); build `npm run build` (= `tsc --noEmit && vite build`). Every task ends with `npm test && npx tsc --noEmit` green.
- **Shell prefix for every command (Git Bash):** `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web &&`.
- **Commits:** stage only the task's files (`git add <paths>`, never `-A` or `.`); commit messages NEVER carry a `Co-Authored-By` line or any other trailer.
- **Runbook (Task 13):** owner format: command → expected (+ `if not:`), self-contained, no prose paragraphs, **STOP** after every gate; deploy B recreates `web` only, at a quiet hour; never print `.env`.

## Review Focus

1. **A LADDER rung missing from `rungs`** (no account yet, or epoch 0 with `rungs: []`). The api then answers `break_even: thin` with every present rung above 30 trades; the drawing must still have six columns, draw the absent one as an empty column labelled `no account`, and must not say "too few trades yet" beside rungs that all have enough (spec §17). Pinned in Task 2 by `draws a rung the api left out as missing, and says why the answer is thin` and in Task 5 by `a rung with no account is an empty column labelled so, and the answer says why`.
2. **The us-band or break-even outside the rung range, or a rung of unknown latency** (measured 155 → band 205–305 ms, past the last rung at 300; measured 5,000; `latency_ms: null` when the api has no policies file). Positions must clamp/extrapolate on the ordinal axis, never `NaN`, and the unknown rung keeps its slot and reads `?`. Pinned in Task 2 by `places the us-band between the right rungs, even past the last one` and `a rung the api could not place (no policies file) keeps its slot and reads "?"`.
3. **A fresh epoch (every figure `"0"`, the live state minutes after a reset) and epoch 0 (`start_ms`, `as_of_ms` null).** The y-domain must not divide by zero and no SVG attribute may be `NaN`. Pinned in Task 2 by `a fresh epoch (every figure "0") draws nothing and the y domain stays finite` and in Task 5 by `a fresh epoch and epoch 0 draw no NaN`.
4. **Reset double submit and 409/429 answers racing the poll.** A double click must send one POST; 409 (someone queued first) and 429 (today's reset used) must settle into the server's state (queued / next allowed) after a refetch, and the 10 s poll must stop once the request is applied. Pinned in Task 7 by `keeps RESET off until the box reads reset, posts once for a double click, then says queued`, the `it.each` answers test and `asks again every 10 s only while a request is pending`.
5. **Panel memory out of context.** A remembered question id from another page must fall back to the hero; a selected row that leaves the page's list stays as a snapshot (`as selected`); leaving the page clears the selection. Pinned in Task 8 by `opens the hero by default and falls back to it when the remembered question is not on this page` and in Task 9 by `a selection is a snapshot: it stays when its row leaves the list` and `leaving the page clears the selection`.

---

## File Structure

```
web/src/api/types.ts                    MODIFY: Position.end_date; LadderRung, BreakEven, MarketsStatus, Ladder, PaperEpoch, Epochs, ResetQueued
web/src/api/client.ts                   MODIFY: requestJson (POST), paperEpochs, paperLadder, paperReset, paperEquity(epoch)
web/src/api/useEpochs.ts                CREATE: useEpochs (10 s poll while pending), useEpochBump
web/src/test/mockApi.ts                 MODIFY: route functions also get the RequestInit
web/src/test/fixtures.ts                MODIFY: positionFixture.end_date, ladderRung, ladderFixture, ladderLiveFixture, epochsFixture
web/src/test/appRoutes.ts               CREATE: every page route answered from fixtures
web/src/test/settle.ts                  CREATE: flush until fetch stops growing
web/src/lib/time.ts                     MODIFY: fmtUtcDayHm
web/src/lib/ladder.ts                   CREATE: LADDER, ladderModel, axis positions, stacks, words
web/src/lib/epochs.ts                   CREATE: epoch line, earlier epochs, P0 fee-0 accounts, reset words
web/src/lib/sweet.ts                    MODIFY: grid() takes {rows}; ladderGrid, glyphs, cellDetail, gridNote
web/src/components/SweetLadderGrid.tsx  CREATE: the 0/25/52/100/200/300 grid of buttons
web/src/components/ResetFold.tsx        CREATE: the reset fold
web/src/components/Fold.tsx             MODIFY: children may be (open) => ReactNode
web/src/components/Table.tsx            MODIFY: onRowClick
web/src/schematics/LatencyLadder.tsx    CREATE: the hero
web/src/schematics/PayoffGrid.tsx       CREATE: payoff structure (b)
web/src/schematics/DurationBars.tsx     CREATE: episode bar next to the example
web/src/schematics/CaughtBar.tsx        CREATE: duration on the ladder's ordinal axis
web/src/schematics/LockedBlocks.tsx     CREATE: open positions sized by basis
web/src/explain/{ExplainContext,Explain,questions}.tsx, {storage,useMedia,copy,models,honesty}.ts   CREATE
web/src/help.ts                         MODIFY: EPISODE_DEF shared with the panel
web/src/pages/{Sweet,Paper,Radar,Episodes,EpisodeDetail}.tsx, web/src/App.tsx   MODIFY
web/src/theme.css, web/src/theme.test.ts                                        MODIFY
docs/superpowers/runbooks/phase7a-web-vps.md                                    CREATE
```

---

### Task 1: api types, client and fixtures for the ladder, epochs and reset

**Files:**
- Modify: `web/src/api/types.ts:201-209` (`Position` gains `end_date`), append after line 336 (end of file)
- Modify: `web/src/api/client.ts:1-4` (imports), `:40-62` (`getJson` → `requestJson` + `getJson`), `:107-108` (`paperEquity`), `:113-115` (three new methods before `};`)
- Modify: `web/src/test/mockApi.ts:7-22`
- Modify: `web/src/test/fixtures.ts:1-4` (imports), `:74-80` (`positionFixture`), append after line 181
- Test: `web/src/api/client.test.ts` (append inside the `describe`, before its closing `});` at line 61)

**Interfaces:**
- Consumes: the api JSON of `bins/api/src/ladder.rs` (`LadderDto`, `RungDto`, `cards::ladder::BreakEven` serialized `#[serde(tag = "kind")]` as `thin` | `all` | `none` | `at` with `latency_ms`, `direction`, `not_monotone` only when true), `bins/api/src/reset.rs` (`EpochsDto`, `QueuedDto`, errors `{error, detail}`: 403 `forbidden`, 400 `bad_request`, 409 `reset_pending`, 429 `reset_daily_limit`, 503 `reset_disabled`), `bins/api/src/paper.rs` (`PositionDto.end_date`, `?epoch=N` on equity/ledger).
- Produces:

```ts
// types.ts
export interface Position { /* …existing… */ end_date: string | null }
export interface LadderRung { account_id: string; scenario: string; latency_ms: number | null; extra: boolean; n_exec: number; n_leg_risk: number; n_missed: number; gains: Dec; leg_risk: Dec; settle: Dec; unrealized: Dec; net: Dec }
export type BreakEven = { kind: 'thin' } | { kind: 'all' } | { kind: 'none' } | { kind: 'at'; latency_ms: number; direction: 'loses_beyond' | 'pays_beyond'; not_monotone?: boolean };
export type MarketsStatus = 'live' | 'warming' | 'under_review' | 'missing';
export interface Ladder { epoch: number; start_ms: number | null; as_of_ms: number | null; rungs: LadderRung[]; markets: SweetRow[]; measured: { latency_ms: number; host: string } | null; markets_status: MarketsStatus; break_even: BreakEven }
export interface PaperEpoch { epoch: number; start_ms: number; cause: 'policies' | 'reset' }
export interface Epochs { epochs: PaperEpoch[]; current: number; pending_request: { id: number; requested_ms: number } | null; next_allowed_ms: number }
export interface ResetQueued { request_id: number; requested_ms: number }
// client.ts
export const RESET_INTENT_HEADER = 'X-CT-Intent'; export const RESET_INTENT = 'reset-paper';
api.paperEquity(account: string, hours: number, signal?: AbortSignal, epoch?: number): Promise<EquitySeries>
api.paperEpochs(signal?: AbortSignal): Promise<Epochs>
api.paperLadder(signal?: AbortSignal): Promise<Ladder>
api.paperReset(): Promise<ResetQueued>   // rejects with ApiError(status, code, detail)
// mockApi.ts
export type Route = unknown | ((url: URL, init?: RequestInit) => unknown);
// fixtures.ts
ladderRung(o?: Partial<LadderRung>): LadderRung; ladderFixture(o?: Partial<Ladder>): Ladder; ladderLiveFixture(): Ladder; epochsFixture(o?: Partial<Epochs>): Epochs; export const EPOCH1_START: number
```

- [ ] **Step 1: Write the failing tests** — append to `web/src/api/client.test.ts` inside `describe('api client', …)`:

```ts
  it('asks for a reset with the intent header, a JSON body and same-origin credentials', async () => {
    const f = mockApi({ '/api/paper/reset': () => jsonResponse(202, { request_id: 7, requested_ms: 1_790_000_000_000 }) });
    await expect(api.paperReset()).resolves.toEqual({ request_id: 7, requested_ms: 1_790_000_000_000 });
    const [url, init] = f.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/paper/reset');
    expect(init.method).toBe('POST');
    expect(init.credentials).toBe('same-origin');
    expect(init.headers).toEqual({ Accept: 'application/json', 'Content-Type': 'application/json', 'X-CT-Intent': 'reset-paper' });
    expect(init.body).toBe('{"confirm":"reset"}');
  });

  it('turns every reset refusal into an ApiError with its code', async () => {
    for (const [status, code] of [[409, 'reset_pending'], [429, 'reset_daily_limit'], [503, 'reset_disabled'], [403, 'forbidden']] as const) {
      mockApi({ '/api/paper/reset': () => jsonResponse(status, { error: code, detail: 'x' }) });
      const err = await api.paperReset().catch((e: unknown) => e);
      expect(err).toBeInstanceOf(ApiError);
      expect(err).toMatchObject({ status, code });
    }
  });

  it('sends ?epoch= for an earlier epoch, 0 included, and nothing otherwise', async () => {
    const f = mockApi({ '/api/paper/equity': { account_id: 'a', from_ms: 0, to_ms: 1, points: [] } });
    await api.paperEquity('P0-converter/rung-0', 6, undefined, 0);
    await api.paperEquity('P0-converter/rung-0', 24);
    expect(f.mock.calls.map((c) => String(c[0]))).toEqual([
      '/api/paper/equity?account=P0-converter%2Frung-0&hours=6&epoch=0',
      '/api/paper/equity?account=P0-converter%2Frung-0&hours=24',
    ]);
  });

  it('reads the ladder and the epochs with money kept as strings', async () => {
    const f = mockApi({ '/api/paper/ladder': ladderFixture(), '/api/paper/epochs': epochsFixture() });
    const l = await api.paperLadder();
    const e = await api.paperEpochs();
    expect(typeof l.rungs[0].net).toBe('string');
    expect(l.break_even).toEqual({ kind: 'at', latency_ms: 70, direction: 'loses_beyond' });
    expect(e.current).toBe(1);
    expect(f.mock.calls.map((c) => String(c[0]))).toEqual(['/api/paper/ladder', '/api/paper/epochs']);
  });
```

and change the first import line of the file to:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { epochsFixture, ladderFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { ApiError, SESSION_EXPIRED_TEXT, api } from './client';
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/api/client.test.ts`
Expected: FAIL — `does not provide an export named 'epochsFixture'` (and `api.paperReset is not a function` once fixtures exist).

- [ ] **Step 3: Implement**

`web/src/api/types.ts` — replace lines 201-209 (`Position`) with:

```ts
export interface Position {
  account_id: string;
  market_id: string;
  outcome: string;
  qty: Dec;
  basis: Dec;
  returned: boolean;
  event_title: string | null;
  /** The event's end date as stored (`events.end_date`, text); null when unknown (spec 7a §9.2). */
  end_date: string | null;
}
```

Append at the end of `web/src/api/types.ts`:

```ts
/** Phase 7a §7.1: one paper account of the latency ladder since the epoch start, cut at `as_of_ms`. */
export interface LadderRung {
  account_id: string;
  scenario: string;
  /** proc + net + match from the api's policies file; null when the api has none. */
  latency_ms: number | null;
  /** sp-burst / sp-idle-nodelay: drawn beside the ladder, never in the break-even. */
  extra: boolean;
  n_exec: number;
  n_leg_risk: number;
  /** Execs with q_eff = 0 (SWEET's meaning). */
  n_missed: number;
  /** Σ realized_delta of converts: basket gains, fees included. */
  gains: Dec;
  /** Σ realized_delta of unwind sells: leg-risk cost, fees included. */
  leg_risk: Dec;
  /** Σ realized_delta of settlements. */
  settle: Dec;
  unrealized: Dec;
  /** Latest common mark's equity − bankroll = gains + leg_risk + settle + unrealized, exactly. */
  net: Dec;
}
/** `cards::ladder::BreakEven`: internally tagged by `kind`; `not_monotone` is sent only when true. */
export type BreakEven =
  | { kind: 'thin' }
  | { kind: 'all' }
  | { kind: 'none' }
  | { kind: 'at'; latency_ms: number; direction: 'loses_beyond' | 'pays_beyond'; not_monotone?: boolean };
export type MarketsStatus = 'live' | 'warming' | 'under_review' | 'missing';
/** GET /api/paper/ladder. */
export interface Ladder {
  epoch: number;
  /** null in epoch 0 (no epoch row yet). */
  start_ms: number | null;
  /** The latest instant every ladder account has a mark; every sum stops there. */
  as_of_ms: number | null;
  /** LADDER rungs by latency, then the extras; a LADDER rung with no account is left out. */
  rungs: LadderRung[];
  /** SWEET's 30d NO rows at fee 0 for the LADDER scenarios, dimensions all + category. */
  markets: SweetRow[];
  measured: { latency_ms: number; host: string } | null;
  markets_status: MarketsStatus;
  break_even: BreakEven;
}
export interface PaperEpoch {
  epoch: number;
  start_ms: number;
  cause: 'policies' | 'reset';
}
/** GET /api/paper/epochs; `current` is 0 before the first epoch row. */
export interface Epochs {
  epochs: PaperEpoch[];
  current: number;
  pending_request: { id: number; requested_ms: number } | null;
  /** The next UTC midnight after today's request, or now. */
  next_allowed_ms: number;
}
/** POST /api/paper/reset → 202. */
export interface ResetQueued {
  request_id: number;
  requested_ms: number;
}
```

`web/src/api/client.ts` (line numbers are the original ones) — replace lines 1-4 with:

```ts
import type {
  CardsMeta, CardsResponse, EpisodeDetail, EpisodeSummary, Epochs, EquitySeries, HealthEnvelope, HealthSeries,
  Integrity, Ladder, LedgerEntry, OpenEpisodes, Page, PaperAccount, Position, ResetQueued, Today,
} from './types';
```

replace lines 40-62 (`async function getJson…` through its closing `}`) with:

```ts
interface RequestOpts {
  signal?: AbortSignal;
  method?: 'GET' | 'POST';
  headers?: Record<string, string>;
  body?: string;
}

async function requestJson<T>(path: string, opts: RequestOpts = {}): Promise<T> {
  const res = await fetch(path, {
    method: opts.method ?? 'GET',
    signal: opts.signal,
    credentials: 'same-origin',
    headers: { Accept: 'application/json', ...opts.headers },
    body: opts.body,
  });
  if (res.status === 401) {
    if (markReauthReload()) {
      location.reload();
      return new Promise<T>(() => {}); // navigating away: never resolve, never render an error flash
    }
    throw new ApiError(401, 'unauthorized', SESSION_EXPIRED_TEXT);
  }
  if (!res.ok) {
    let code = `http_${res.status}`;
    let detail = 'Something broke. Not your fault. Probably.';
    try {
      const body = (await res.json()) as { error?: string; detail?: string };
      if (body.error) code = body.error;
      if (body.detail) detail = body.detail;
    } catch {
      // not JSON (a proxy page): keep the generic text
    }
    throw new ApiError(res.status, code, detail);
  }
  return (await res.json()) as T;
}

function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  return requestJson<T>(path, { signal });
}

/** Spec 7a §5.1: a custom header forces a CORS preflight, which the api never answers. */
export const RESET_INTENT_HEADER = 'X-CT-Intent';
export const RESET_INTENT = 'reset-paper';
```

replace lines 107-108 (`paperEquity`) with:

```ts
  /** `epoch` (0 or more) reads an earlier epoch; absent = the current one (spec 7a §6). */
  paperEquity: (account: string, hours: number, signal?: AbortSignal, epoch?: number) =>
    getJson<EquitySeries>(`/api/paper/equity${qs({ account, hours, epoch })}`, signal),
```

and replace lines 113-115 (`cards`, `cardsMeta`, `};`) with:

```ts
  cards: (win: string, signal?: AbortSignal) => getJson<CardsResponse>(`/api/cards${qs({ win })}`, signal),
  cardsMeta: (signal?: AbortSignal) => getJson<CardsMeta>('/api/cards/meta', signal),
  paperEpochs: (signal?: AbortSignal) => getJson<Epochs>('/api/paper/epochs', signal),
  paperLadder: (signal?: AbortSignal) => getJson<Ladder>('/api/paper/ladder', signal),
  /** The api's one write (spec 7a §5): queues a reset request; the scanner applies it within ~30 s. */
  paperReset: () =>
    requestJson<ResetQueued>('/api/paper/reset', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', [RESET_INTENT_HEADER]: RESET_INTENT },
      body: JSON.stringify({ confirm: 'reset' }),
    }),
};
```

`web/src/test/mockApi.ts` — replace lines 7-22 with:

```ts
/** A route value is a JSON body, or a function of the URL (and the request init) returning a body or a Response. */
export type Route = unknown | ((url: URL, init?: RequestInit) => unknown);

/** Stub global fetch with a path → response table. Unknown paths answer a JSON 404. */
export function mockApi(routes: Record<string, Route>) {
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), 'http://localhost');
    if (!(url.pathname in routes)) {
      return jsonResponse(404, { error: 'not_found', detail: 'Nothing here. It may have been a dream.' });
    }
    const route = routes[url.pathname];
    const body = typeof route === 'function' ? (route as (u: URL, i?: RequestInit) => unknown)(url, init) : route;
    return body instanceof Response ? body : jsonResponse(200, body);
  });
  vi.stubGlobal('fetch', fn);
  return fn;
}
```

`web/src/test/fixtures.ts` (original line numbers) — replace lines 1-4 with:

```ts
import type {
  CardBody, CardsMeta, EpisodeDetail, EpisodeSummary, Epochs, HealthNow, HealthPoint, Ladder, LadderRung, LedgerEntry,
  PaperAccount, Position, SweetRow, SweetSeries, Today,
} from '../api/types';
```

replace lines 74-80 (`positionFixture`) with:

```ts
export function positionFixture(o: Partial<Position> = {}): Position {
  return {
    account_id: 'P3-naive/sp-idle', market_id: 'm1', outcome: 'yes', qty: '40', basis: '18', returned: false,
    event_title: 'Fed decision in October?', end_date: '2026-10-31',
    ...o,
  };
}
```

and append at the end of `web/src/test/fixtures.ts`:

```ts
/** Epoch 1 started with deploy A (spec §17): the policies epoch. */
export const EPOCH1_START = Date.UTC(2026, 8, 29, 19, 58, 12);

/** A ladder rung (or extra) with every field filled; the four parts add up to `net` exactly. */
export function ladderRung(o: Partial<LadderRung> = {}): LadderRung {
  return {
    account_id: 'P0-converter/sp-idle', scenario: 'sp-idle', latency_ms: 300, extra: false, n_exec: 40, n_leg_risk: 6,
    n_missed: 5, gains: '3.8', leg_risk: '-66', settle: '1', unrealized: '0', net: '-61.2',
    ...o,
  };
}

const rung = (scenario: string, latency_ms: number, gains: string, leg_risk: string, settle: string, unrealized: string, net: string, extra = false) =>
  ladderRung({ account_id: `P0-converter/${scenario}`, scenario, latency_ms, extra, gains, leg_risk, settle, unrealized, net });

/** An answered ladder: every rung has 40 trades; the sign changes once, between colo-idle (+3.10) and
 *  rung-100 (−4.40): 52 + 48 × 3.1 / 7.5 = 71.84 → 70 ms, loses beyond. */
export function ladderFixture(o: Partial<Ladder> = {}): Ladder {
  return {
    epoch: 1,
    start_ms: EPOCH1_START,
    as_of_ms: Date.UTC(2026, 8, 30, 21, 14),
    rungs: [
      rung('rung-0', 0, '41.5', '-12', '1', '0', '30.5'),
      rung('rung-25', 25, '30.25', '-19', '1', '0', '12.25'),
      rung('colo-idle', 52, '22.1', '-20', '1.5', '-0.5', '3.1'),
      rung('rung-100', 100, '15.6', '-21', '1', '0', '-4.4'),
      rung('rung-200', 200, '8.25', '-30', '1', '0', '-20.75'),
      rung('sp-idle', 300, '3.8', '-66', '1', '0', '-61.2'),
      rung('sp-idle-nodelay', 300, '4', '-63', '1', '0', '-58', true),
      rung('sp-burst', 450, '1.9', '-82', '0', '0', '-80.1', true),
    ],
    markets: [
      sweetRow({ scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.4', lo: '0.1', hi: '0.7', n_exec: 60, clusters: 41 }),
      sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.6', lo: '-0.9', hi: '-0.3', n_exec: 70, clusters: 50 }),
      sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-0', latency_ms: 0, label: 'thin', mean: '0.3', n_exec: 12, clusters: 9 }),
      sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'sp-idle', latency_ms: 300, label: 'thin', mean: '-0.8', n_exec: 20, clusters: 11 }),
    ],
    measured: { latency_ms: 155, host: 'vps-kvm2' },
    markets_status: 'live',
    break_even: { kind: 'at', latency_ms: 70, direction: 'loses_beyond' },
    ...o,
  };
}

/** The live /api/paper/ladder of 2026-09-29, minutes into epoch 1 (spec §17; runbook 7a step 31): the six
 *  LADDER rungs at 0, 25, 52, 100, 200, 300 ms, then the two extras; every figure "0", thin; SWEET warming. */
export function ladderLiveFixture(): Ladder {
  const fresh = (scenario: string, latency_ms: number, n_exec: number, extra = false) =>
    ladderRung({
      account_id: `P0-converter/${scenario}`, scenario, latency_ms, extra, n_exec, n_leg_risk: 0, n_missed: 0,
      gains: '0', leg_risk: '0', settle: '0', unrealized: '0', net: '0',
    });
  return {
    epoch: 1,
    start_ms: EPOCH1_START,
    as_of_ms: Date.UTC(2026, 8, 29, 20, 9),
    rungs: [
      fresh('rung-0', 0, 2), fresh('rung-25', 25, 2), fresh('colo-idle', 52, 1), fresh('rung-100', 100, 1),
      fresh('rung-200', 200, 0), fresh('sp-idle', 300, 0), fresh('sp-idle-nodelay', 300, 0, true), fresh('sp-burst', 450, 0, true),
    ],
    markets: [],
    measured: { latency_ms: 155, host: 'vps-kvm2' },
    markets_status: 'warming',
    break_even: { kind: 'thin' },
  };
}

/** Epoch 1 only (cause policies), nothing pending, a reset allowed now. */
export function epochsFixture(o: Partial<Epochs> = {}): Epochs {
  return {
    epochs: [{ epoch: 1, start_ms: EPOCH1_START, cause: 'policies' }],
    current: 1,
    pending_request: null,
    next_allowed_ms: 0,
    ...o,
  };
}
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/api/client.test.ts && npm test && npx tsc --noEmit`
Expected: PASS (client tests 9 passed; the whole suite green; tsc prints nothing).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/api/types.ts src/api/client.ts src/api/client.test.ts src/test/mockApi.ts src/test/fixtures.ts && git commit -m "web: api types and client for the ladder, epochs and the reset POST; ladder and epochs fixtures"
```

---

### Task 2: the ladder model (pure): LADDER, ordinal axis, stacks and words

**Files:**
- Create: `web/src/lib/ladder.ts`
- Modify: `web/src/lib/time.ts` (append after line 61)
- Test: `web/src/lib/ladder.test.ts` (create), `web/src/lib/time.test.ts` (append a test inside its `describe`)

**Interfaces:**
- Consumes: `Ladder`, `LadderRung`, `BreakEven`, `MarketsStatus`, `SweetRow` (Task 1); `fmtUtcHm`, `isoDay` (`lib/time.ts`).
- Produces:

```ts
// time.ts
export function fmtUtcDayHm(ms: number): string            // "2026-09-29 19:58 UTC"
// ladder.ts
export const LADDER: readonly { scenario: string; latency_ms: number }[]   // rung-0 0, rung-25 25, colo-idle 52, rung-100 100, rung-200 200, sp-idle 300
export const LADDER_YES: readonly string[]                 // ['colo-idle', 'sp-idle']
export const MIN_EXEC = 30; export const SLOTS = 7;        // six rungs + the extras slot
export type LadderColumn = { kind: 'missing'; scenario: string; latency_ms: number } | { kind: 'rung'; scenario: string; latency_ms: number; latencyKnown: boolean; rung: LadderRung };
export type ThinReason = 'few' | 'missing' | 'unknown' | 'no_epoch';
export interface LadderModel { epoch: number; startMs: number | null; asOfMs: number | null; columns: LadderColumn[]; extras: LadderRung[]; breakEven: BreakEven; thinReason: ThinReason | null; measured: { latency_ms: number; host: string } | null; markets: { status: MarketsStatus; rows: SweetRow[] } }
export function ladderModel(l: Ladder): LadderModel
export function slotX(i: number, slots?: number): number                  // fraction of the width, 0..1
export function latencyX(ms: number, slots?: number): number
export function usBand(m: { latency_ms: number }, slots?: number): { x0: number; x1: number }
export interface Segment { part: 'gains' | 'settle' | 'unrealized' | 'leg_risk'; from: number; to: number }
export function stackRung(r: Pick<LadderRung, 'gains' | 'settle' | 'unrealized' | 'leg_risk'>): Segment[]
export function yDomain(rungs: LadderRung[]): { lo: number; hi: number }
export interface BreakEvenText { big: string; line: string }
export function breakEvenText(m: LadderModel): BreakEvenText
export const NOT_MONOTONE: string
export function rungMsText(c: LadderColumn): string; export function rungCountText(c: LadderColumn, thin: boolean): string
export function asOfText(ms: number | null): string; export function epochText(m: Pick<LadderModel, 'epoch' | 'startMs'>): string
export function usText(m: { latency_ms: number } | null): string; export function extraText(r: LadderRung): string
```

- [ ] **Step 1: Write the failing tests**

Append inside `describe(…)` of `web/src/lib/time.test.ts` (and add `fmtUtcDayHm` to its import from `./time`):

```ts
  it('writes a UTC day and minute for epoch starts', () => {
    expect(fmtUtcDayHm(Date.UTC(2026, 8, 29, 19, 58, 12))).toBe('2026-09-29 19:58 UTC');
  });
```

Create `web/src/lib/ladder.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { BreakEven } from '../api/types';
import { ladderFixture, ladderLiveFixture, ladderRung } from '../test/fixtures';
import {
  LADDER, MIN_EXEC, SLOTS, asOfText, breakEvenText, epochText, extraText, ladderModel, latencyX, rungCountText, rungMsText,
  slotX, stackRung, usBand, usText, yDomain,
} from './ladder';

describe('the web LADDER', () => {
  it('names the same six rungs and latencies as the live api (spec §17)', () => {
    const live = ladderLiveFixture().rungs.filter((r) => !r.extra).map((r) => ({ scenario: r.scenario, latency_ms: r.latency_ms }));
    expect(LADDER).toEqual(live);
    expect(MIN_EXEC).toBe(30);
    expect(SLOTS).toBe(7);
  });

  it('the answered fixture reconciles: the four parts add up to net on every rung', () => {
    const cents = (v: string) => Math.round(Number(v) * 100); // test-only check of small fixture values
    for (const r of ladderFixture().rungs) {
      expect(cents(r.gains) + cents(r.leg_risk) + cents(r.settle) + cents(r.unrealized), r.scenario).toBe(cents(r.net));
    }
  });
});

describe('ladderModel', () => {
  it('keeps all six LADDER columns in order, the extras apart, and passes the api break-even through', () => {
    const m = ladderModel(ladderFixture());
    expect(m.columns.map((c) => [c.scenario, c.kind])).toEqual(LADDER.map((l) => [l.scenario, 'rung']));
    expect(m.extras.map((r) => r.scenario)).toEqual(['sp-idle-nodelay', 'sp-burst']);
    expect(m.breakEven).toEqual({ kind: 'at', latency_ms: 70, direction: 'loses_beyond' });
    expect(m.thinReason).toBeNull();
    expect(m.markets.status).toBe('live');
  });

  it('draws a rung the api left out as missing, and says why the answer is thin (review focus 1)', () => {
    const without25 = ladderFixture().rungs.filter((r) => r.scenario !== 'rung-25');
    const m = ladderModel(ladderFixture({ rungs: without25, break_even: { kind: 'thin' } }));
    expect(m.columns[1]).toEqual({ kind: 'missing', scenario: 'rung-25', latency_ms: 25 });
    expect(m.thinReason).toBe('missing');
    expect(breakEvenText(m).big).toBe('a rung has no account yet');
    const few = ladderModel(
      ladderFixture({ rungs: without25.map((r) => (r.scenario === 'rung-0' ? { ...r, n_exec: 29 } : r)), break_even: { kind: 'thin' } }),
    );
    expect(few.thinReason).toBe('few');
    expect(breakEvenText(few).big).toBe('too few trades yet');
  });

  it('epoch 0: no rungs is six missing columns and no epoch yet', () => {
    const m = ladderModel(ladderFixture({ epoch: 0, start_ms: null, as_of_ms: null, rungs: [], break_even: { kind: 'thin' } }));
    expect(m.columns.every((c) => c.kind === 'missing')).toBe(true);
    expect(m.extras).toEqual([]);
    expect(m.thinReason).toBe('no_epoch');
    expect(breakEvenText(m).big).toBe('no epoch yet');
    expect(epochText(m)).toBe('$ on paper · no epoch has started yet');
    expect(asOfText(m.asOfMs)).toBe('no common mark yet');
  });

  it('a rung the api could not place (no policies file) keeps its slot and reads "?" (review focus 2)', () => {
    const m = ladderModel(ladderFixture({ rungs: ladderFixture().rungs.map((r) => ({ ...r, latency_ms: null })), break_even: { kind: 'thin' } }));
    expect(m.thinReason).toBe('unknown');
    expect(m.columns.map(rungMsText)).toEqual(['?', '?', '?', '?', '?', '?']);
    expect(breakEvenText(m).big).toBe('rung latencies unknown');
    expect(extraText(m.extras[1])).toBe('sp-burst (? ms idle, more in bursts)');
  });
});

describe('the ordinal axis', () => {
  it('puts each rung at its slot centre and interpolates between neighbours', () => {
    LADDER.forEach((l, i) => expect(latencyX(l.latency_ms)).toBeCloseTo(slotX(i)));
    expect(latencyX(150)).toBeCloseTo((slotX(3) + slotX(4)) / 2);
    expect(latencyX(70)).toBeCloseTo(slotX(2) + (18 / 48) * (slotX(3) - slotX(2)));
    expect(slotX(0, 6)).toBeCloseTo(1 / 12);
  });

  it('places the us-band between the right rungs, even past the last one (review focus 2)', () => {
    const b = usBand({ latency_ms: 155 }); // 205..305 ms
    expect(b.x0).toBeGreaterThan(slotX(4));
    expect(b.x0).toBeLessThan(slotX(5));
    expect(b.x1).toBeGreaterThan(slotX(5));
    expect(b.x1).toBeLessThan(slotX(SLOTS - 1));
    const low = usBand({ latency_ms: 0 }); // 50..150 ms
    expect(low.x0).toBeGreaterThan(slotX(1));
    expect(low.x0).toBeLessThan(slotX(2));
    expect(low.x1).toBeGreaterThan(slotX(3));
    expect(low.x1).toBeLessThan(slotX(4));
    expect(usBand({ latency_ms: 5000 })).toEqual({ x0: 1, x1: 1 });
    expect(latencyX(-40)).toBe(slotX(0));
    expect(Number.isFinite(latencyX(Number.MAX_SAFE_INTEGER))).toBe(true);
  });
});

describe('bars', () => {
  it('stacks each part by its own sign: gains and settle up, unrealized by its sign, leg-risk down', () => {
    const s = stackRung(ladderRung({ gains: '22.1', leg_risk: '-20', settle: '1.5', unrealized: '-0.5' }));
    expect(s.map((x) => x.part)).toEqual(['gains', 'settle', 'unrealized', 'leg_risk']);
    expect(s[0]).toEqual({ part: 'gains', from: 0, to: 22.1 });
    expect(s[1].from).toBeCloseTo(22.1);
    expect(s[1].to).toBeCloseTo(23.6);
    expect(s[2]).toEqual({ part: 'unrealized', from: 0, to: -0.5 });
    expect(s[3].from).toBeCloseTo(-0.5);
    expect(s[3].to).toBeCloseTo(-20.5);
  });

  it('a fresh epoch (every figure "0") draws nothing and the y domain stays finite (review focus 3)', () => {
    const r = ladderLiveFixture().rungs;
    expect(r.flatMap((x) => stackRung(x))).toEqual([]);
    expect(yDomain(r)).toEqual({ lo: -1, hi: 1 });
    expect(yDomain([])).toEqual({ lo: -1, hi: 1 });
  });

  it('the y domain holds every stack end and every net, with zero inside', () => {
    const d = yDomain(ladderFixture().rungs);
    expect(d.hi).toBeGreaterThan(42.5); // rung-0: 41.5 + 1 settled
    expect(d.lo).toBeLessThan(-82); // sp-burst's leg-risk cost
  });
});

describe('words', () => {
  it('says the break-even in every kind, with "pays on paper" / "loses on paper"', () => {
    const m = (be: BreakEven) => ladderModel(ladderFixture({ break_even: be }));
    expect(breakEvenText(m({ kind: 'at', latency_ms: 70, direction: 'loses_beyond' }))).toEqual({
      big: '≈ 70 ms', line: 'Pays on paper below it, loses on paper beyond it.',
    });
    expect(breakEvenText(m({ kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true }))).toEqual({
      big: '≈ 40 ms', line: 'Loses on paper below it, pays on paper beyond it.',
    });
    expect(breakEvenText(m({ kind: 'all' }))).toEqual({ big: 'pays at every rung', line: 'Pays on paper at every latency on the ladder.' });
    expect(breakEvenText(m({ kind: 'none' }))).toEqual({ big: 'none', line: 'Loses on paper at every latency on the ladder.' });
    expect(breakEvenText(ladderModel(ladderLiveFixture()))).toEqual({
      big: 'too few trades yet', line: 'Each rung needs 30 trades; the counts are under the drawing.',
    });
  });

  it('labels counts n/30 only while thin, and the rest of the drawing in words', () => {
    const live = ladderModel(ladderLiveFixture());
    expect(live.columns.map((c) => rungCountText(c, true))).toEqual(['2/30', '2/30', '1/30', '1/30', '0/30', '0/30']);
    expect(rungCountText(ladderModel(ladderFixture()).columns[0], false)).toBe('40');
    expect(rungCountText({ kind: 'missing', scenario: 'rung-25', latency_ms: 25 }, true)).toBe('no account');
    expect(rungMsText(live.columns[2])).toBe('52');
    expect(asOfText(Date.UTC(2026, 8, 29, 20, 9))).toBe('as of 20:09 UTC');
    expect(epochText(live)).toBe('$ on paper since epoch 1 began, 2026-09-29 19:58 UTC');
    expect(usText({ latency_ms: 155 })).toBe('us: measured ≈155 + matching');
    expect(usText(null)).toBe('us: not measured yet');
    expect(extraText(ladderRung({ scenario: 'sp-burst', latency_ms: 450, extra: true }))).toBe('sp-burst (450 ms idle, more in bursts)');
    expect(extraText(ladderRung({ scenario: 'sp-idle-nodelay', latency_ms: 300, extra: true }))).toBe('sp-idle-nodelay (300 ms, secondsDelay ignored)');
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/ladder.test.ts src/lib/time.test.ts`
Expected: FAIL — `Failed to resolve import "./ladder"`, and `fmtUtcDayHm is not a function`.

- [ ] **Step 3: Implement**

Append to `web/src/lib/time.ts`:

```ts
/** "2026-09-29 19:58 UTC": an epoch start, a next-allowed time. */
export function fmtUtcDayHm(ms: number): string {
  return `${isoDay(ms)} ${fmtUtcHm(ms)} UTC`;
}
```

Create `web/src/lib/ladder.ts`:

```ts
import type { BreakEven, Ladder, LadderRung, MarketsStatus, SweetRow } from '../api/types';
import { fmtUtcDayHm, fmtUtcHm } from './time';

/**
 * Spec 7a §2: the ladder, fastest first. The same six names and latencies as `crates/cards`
 * `edge::LADDER` and `config/policies.toml`; ladder.test.ts checks them against the live response.
 * The web keeps its own copy so a rung the api leaves out (no account yet) is still drawn (spec §17).
 */
export const LADDER: readonly { scenario: string; latency_ms: number }[] = [
  { scenario: 'rung-0', latency_ms: 0 },
  { scenario: 'rung-25', latency_ms: 25 },
  { scenario: 'colo-idle', latency_ms: 52 },
  { scenario: 'rung-100', latency_ms: 100 },
  { scenario: 'rung-200', latency_ms: 200 },
  { scenario: 'sp-idle', latency_ms: 300 },
];
/** The YES side's LADDER columns (spec §8): the LADDER scenarios with a P1-settlement account. */
export const LADDER_YES: readonly string[] = ['colo-idle', 'sp-idle'];
/** SWEET's minimum executed attempts (`cards::sweet::MIN_EXEC`): below it a rung is thin. */
export const MIN_EXEC = 30;
/** Six rungs plus one slot beside sp-idle for the extras (sp-idle-nodelay, sp-burst). */
export const SLOTS = LADDER.length + 1;

export type LadderColumn =
  | { kind: 'missing'; scenario: string; latency_ms: number }
  | { kind: 'rung'; scenario: string; latency_ms: number; latencyKnown: boolean; rung: LadderRung };

/** Why a `thin` answer is thin, so the big line never says "too few trades" beside rungs that all have enough. */
export type ThinReason = 'few' | 'missing' | 'unknown' | 'no_epoch';

export interface LadderModel {
  epoch: number;
  startMs: number | null;
  asOfMs: number | null;
  /** Always the six LADDER rungs, in order. */
  columns: LadderColumn[];
  extras: LadderRung[];
  breakEven: BreakEven;
  thinReason: ThinReason | null;
  measured: { latency_ms: number; host: string } | null;
  markets: { status: MarketsStatus; rows: SweetRow[] };
}

export function ladderModel(l: Ladder): LadderModel {
  const columns = LADDER.map(({ scenario, latency_ms }): LadderColumn => {
    const rung = l.rungs.find((r) => !r.extra && r.scenario === scenario);
    return rung
      ? { kind: 'rung', scenario, latency_ms, latencyKnown: rung.latency_ms !== null, rung }
      : { kind: 'missing', scenario, latency_ms };
  });
  const present = columns.flatMap((c) => (c.kind === 'rung' ? [c] : []));
  let thinReason: ThinReason | null = null;
  if (l.break_even.kind === 'thin') {
    if (l.start_ms === null) thinReason = 'no_epoch';
    else if (present.some((c) => c.rung.n_exec < MIN_EXEC)) thinReason = 'few';
    else if (present.length < LADDER.length) thinReason = 'missing';
    else if (present.some((c) => !c.latencyKnown)) thinReason = 'unknown';
    else thinReason = 'few';
  }
  return {
    epoch: l.epoch,
    startMs: l.start_ms,
    asOfMs: l.as_of_ms,
    columns,
    extras: l.rungs.filter((r) => r.extra),
    breakEven: l.break_even,
    thinReason,
    measured: l.measured,
    markets: { status: l.markets_status, rows: l.markets },
  };
}

/** Centre of slot `i` as a fraction of the width: the x axis is ordinal, equal spacing per rung (spec §7.2). */
export function slotX(i: number, slots: number = SLOTS): number {
  return (i + 0.5) / slots;
}

/**
 * A latency on the ordinal axis: linear between its two neighbouring rungs' slot centres; below 0 ms it
 * sits on rung-0; beyond the last rung it follows the last segment's slope; always within [slotX(0), 1].
 */
export function latencyX(ms: number, slots: number = SLOTS): number {
  let i = 0;
  while (i < LADDER.length - 2 && ms > LADDER[i + 1].latency_ms) i++;
  const a = LADDER[i];
  const b = LADDER[i + 1];
  const x = slotX(i, slots) + ((ms - a.latency_ms) / (b.latency_ms - a.latency_ms)) * (slotX(i + 1, slots) - slotX(i, slots));
  return Math.min(1, Math.max(slotX(0, slots), x));
}

/** Spec §2: "us" is a band from measured + 50 to measured + 150 ms (matching), never a point. */
export function usBand(m: { latency_ms: number }, slots: number = SLOTS): { x0: number; x1: number } {
  return { x0: latencyX(m.latency_ms + 50, slots), x1: latencyX(m.latency_ms + 150, slots) };
}

export interface Segment {
  part: 'gains' | 'settle' | 'unrealized' | 'leg_risk';
  from: number;
  to: number;
}

const PARTS = ['gains', 'settle', 'unrealized', 'leg_risk'] as const;

/** One rung's stacked bar (spec §7.2): each part by its own sign, positives up from 0 and negatives down
 *  from 0, in the order gains, settle, unrealized, leg-risk. Numbers are geometry only. */
export function stackRung(r: Pick<LadderRung, 'gains' | 'settle' | 'unrealized' | 'leg_risk'>): Segment[] {
  let up = 0;
  let down = 0;
  const out: Segment[] = [];
  for (const part of PARTS) {
    const v = Number(r[part]);
    if (!Number.isFinite(v) || v === 0) continue;
    if (v > 0) {
      out.push({ part, from: up, to: up + v });
      up += v;
    } else {
      out.push({ part, from: down, to: down + v });
      down += v;
    }
  }
  return out;
}

/** The y range holding every stack end and every net, zero always inside, 8 % padding; [−1, 1] when all is zero. */
export function yDomain(rungs: LadderRung[]): { lo: number; hi: number } {
  let hi = 0;
  let lo = 0;
  for (const r of rungs) {
    for (const s of stackRung(r)) {
      hi = Math.max(hi, s.from, s.to);
      lo = Math.min(lo, s.from, s.to);
    }
    const n = Number(r.net);
    if (Number.isFinite(n)) {
      hi = Math.max(hi, n);
      lo = Math.min(lo, n);
    }
  }
  if (hi === lo) return { lo: -1, hi: 1 };
  const pad = (hi - lo) * 0.08;
  return { lo: lo - pad, hi: hi + pad };
}

export interface BreakEvenText {
  big: string;
  line: string;
}

const THIN: Record<ThinReason, BreakEvenText> = {
  few: { big: 'too few trades yet', line: `Each rung needs ${MIN_EXEC} trades; the counts are under the drawing.` },
  missing: { big: 'a rung has no account yet', line: 'Every rung needs its own paper account before the ladder can answer.' },
  unknown: { big: 'rung latencies unknown', line: 'The api has no policies file, so it cannot place the rungs.' },
  no_epoch: { big: 'no epoch yet', line: 'The ladder counts from the start of the first epoch.' },
};

export const NOT_MONOTONE = 'The sign changes more than once along the ladder; this is the first change.';

/** The one big number (spec §7.2) and its line: "pays on paper" / "loses on paper", never "profitable". */
export function breakEvenText(m: LadderModel): BreakEvenText {
  const be = m.breakEven;
  if (be.kind === 'at') {
    return {
      big: `≈ ${be.latency_ms} ms`,
      line: be.direction === 'loses_beyond' ? 'Pays on paper below it, loses on paper beyond it.' : 'Loses on paper below it, pays on paper beyond it.',
    };
  }
  if (be.kind === 'all') return { big: 'pays at every rung', line: 'Pays on paper at every latency on the ladder.' };
  if (be.kind === 'none') return { big: 'none', line: 'Loses on paper at every latency on the ladder.' };
  return THIN[m.thinReason ?? 'few'];
}

/** A column's ms under the drawing; "?" when the api could not place it. */
export function rungMsText(c: LadderColumn): string {
  return c.kind === 'rung' && !c.latencyKnown ? '?' : String(c.latency_ms);
}

/** Spec §17: while thin, each rung shows n/30 so the owner sees how far it is; a missing rung says so. */
export function rungCountText(c: LadderColumn, thin: boolean): string {
  if (c.kind === 'missing') return 'no account';
  return thin ? `${c.rung.n_exec}/${MIN_EXEC}` : String(c.rung.n_exec);
}

/** Spec §17: the sums stop at the latest common mark, not at "now". */
export function asOfText(ms: number | null): string {
  return ms === null ? 'no common mark yet' : `as of ${fmtUtcHm(ms)} UTC`;
}

/** The y-axis label: what the bars are, and since when (the hero's time frame is the epoch). */
export function epochText(m: Pick<LadderModel, 'epoch' | 'startMs'>): string {
  return m.startMs === null ? '$ on paper · no epoch has started yet' : `$ on paper since epoch ${m.epoch} began, ${fmtUtcDayHm(m.startMs)}`;
}

export function usText(m: { latency_ms: number } | null): string {
  return m === null ? 'us: not measured yet' : `us: measured ≈${m.latency_ms} + matching`;
}

/** The extras are annotated markers, never rungs (spec §2): name what each one is. */
export function extraText(r: LadderRung): string {
  const ms = r.latency_ms === null ? '?' : String(r.latency_ms);
  if (r.scenario === 'sp-burst') return `sp-burst (${ms} ms idle, more in bursts)`;
  if (r.scenario === 'sp-idle-nodelay') return `sp-idle-nodelay (${ms} ms, secondsDelay ignored)`;
  return `${r.scenario} (${ms} ms)`;
}
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/ladder.test.ts src/lib/time.test.ts && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/lib/ladder.ts src/lib/ladder.test.ts src/lib/time.ts src/lib/time.test.ts && git commit -m "web: the ladder model: six LADDER rungs always, ordinal axis, signed stacks, the break-even in words"
```

---

### Task 3: the Sweet spots grid (lib + component)

Load `frontend-design:frontend-design` before writing the component and CSS.

**Files:**
- Modify: `web/src/lib/sweet.ts:1-3` (imports), `:99` (`grid` takes `{ rows }`), append after line 155
- Create: `web/src/components/SweetLadderGrid.tsx`
- Modify: `web/src/theme.css` (append), `web/src/theme.test.ts` (append a test before the closing `});` at line 45)
- Test: `web/src/lib/sweet.test.ts` (append a `describe`), `web/src/components/SweetLadderGrid.test.tsx` (create)

**Interfaces:**
- Consumes: `SweetRow` (types), `grid`, `bucketText`, `interval`, `LABEL_TEXT`, `SweetSide`, `SweetDim` (`lib/sweet.ts`), `LADDER`, `LADDER_YES` (Task 2), `signOf` (`lib/money.ts`).
- Produces:

```ts
// lib/sweet.ts
export function grid(s: Pick<SweetSeries, 'rows'>, side: SweetSide, dim: SweetDim): Grid        // signature widened, behaviour unchanged
export const GLYPHS: Record<GlyphKey, string>; export type GlyphKey = 'candidate' | 'negative' | 'inconclusive' | 'thinUp' | 'thinDown' | 'dead_on_paper' | 'not_tested' | 'none'
export const GLYPH_NAME: Record<GlyphKey, string>; export const GLYPH_ORDER: GlyphKey[]
export function glyphKey(r: SweetRow | undefined): GlyphKey
export interface LadderGridRow { dim: SweetDim; bucket: string }
export interface LadderGrid { columns: { scenario: string; latency_ms: number }[]; rows: LadderGridRow[]; cell: (row: LadderGridRow, scenario: string) => SweetRow | undefined }
export function ladderGrid(rows: SweetRow[], side: SweetSide, dim: SweetDim, only?: string[]): LadderGrid
export function cellDetail(row: LadderGridRow, col: { scenario: string; latency_ms: number }, r: SweetRow | undefined): string
export function gridNote(s: Pick<SweetSeries, 'm' | 'z'>): string
export const GRID_HINT = 'Tap a cell for its numbers.'
// components/SweetLadderGrid.tsx
export function SweetLadderGrid(props: { rows: SweetRow[]; side: SweetSide; dim: SweetDim; label: string; only?: string[] }): ReactElement
//   <table class="lgrid" aria-label={label}>; column heads "ms", then each rung's ms; row heads bucketText;
//   each cell <button class="lg-cell g-<GlyphKey>" aria-label="<bucket> at <scenario> (<ms> ms): <GLYPH_NAME>">;
//   <ul aria-label="grid legend">; <p class="lg-detail" aria-live="polite">
```

- [ ] **Step 1: Write the failing tests**

Append to `web/src/lib/sweet.test.ts` (and change its imports to):

```ts
import { describe, expect, it } from 'vitest';
import { sweetFixture, sweetRow } from '../test/fixtures';
import type { SweetRow, SweetSeries } from '../api/types';
import {
  FEW_TRADES, GLYPHS, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, bestCandidate, candidateCount, cellDetail, feeLine, footnote, glyphKey, grid,
  gridNote, interval, ladderGrid, nothingPays, recomputingText, rowName, rowsFor,
} from './sweet';
```

```ts
describe('ladder grid (spec 7a §8)', () => {
  const rows = [
    sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1' }),
    sweetRow({ scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2' }),
    sweetRow({ scenario: 'colo-idle', latency_ms: 52, label: 'thin', mean: '0.05' }),
    sweetRow({ scenario: 'sp-idle', latency_ms: 300, fee_bips: 10, label: 'candidate' }),
    sweetRow({ scenario: 'sp-burst', latency_ms: 450, label: 'candidate' }),
    sweetRow({ scenario: 'sp-idle-nodelay', latency_ms: 300, label: 'candidate' }),
    sweetRow({ dimension: 'category', bucket: 'weather', scenario: 'rung-0', latency_ms: 0, label: 'thin', mean: '-0.3' }),
    sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-0', latency_ms: 0, label: 'inconclusive' }),
    sweetRow({ side: 'yes', scenario: 'sp-idle', latency_ms: 300, label: 'dead_on_paper' }),
    sweetRow({ side: 'yes', scenario: 'sp-burst', latency_ms: 450, label: 'candidate' }),
  ];

  it('has six NO columns and two YES columns, sorted by latency', () => {
    expect(ladderGrid(rows, 'no', 'all').columns.map((c) => c.scenario)).toEqual(['rung-0', 'rung-25', 'colo-idle', 'rung-100', 'rung-200', 'sp-idle']);
    expect(ladderGrid(rows, 'yes', 'all').columns).toEqual([{ scenario: 'colo-idle', latency_ms: 52 }, { scenario: 'sp-idle', latency_ms: 300 }]);
  });

  it('never shows a fee variant or an extra: sp-idle is the fee-0 row', () => {
    const g = ladderGrid(rows, 'no', 'all');
    expect(g.cell({ dim: 'all', bucket: 'all' }, 'sp-idle')?.label).toBe('negative');
    expect(g.columns.some((c) => c.scenario === 'sp-burst' || c.scenario === 'sp-idle-nodelay')).toBe(false);
    expect(ladderGrid(rows, 'yes', 'all').cell({ dim: 'all', bucket: 'all' }, 'sp-idle')?.label).toBe('dead_on_paper');
  });

  it('puts the all row first, then the slice in its own order; `only` keeps the named buckets', () => {
    expect(ladderGrid(rows, 'no', 'category').rows).toEqual([
      { dim: 'all', bucket: 'all' }, { dim: 'category', bucket: 'weather' }, { dim: 'category', bucket: 'crypto' },
    ]);
    expect(ladderGrid(rows, 'no', 'category', ['crypto']).rows).toEqual([{ dim: 'all', bucket: 'all' }, { dim: 'category', bucket: 'crypto' }]);
    expect(ladderGrid(rows, 'no', 'all').rows).toEqual([{ dim: 'all', bucket: 'all' }]);
  });

  it('gives one glyph per label and sign', () => {
    const k = (o: Partial<SweetRow>) => GLYPHS[glyphKey(sweetRow(o))];
    expect(k({ label: 'candidate' })).toBe('●');
    expect(k({ label: 'negative' })).toBe('○');
    expect(k({ label: 'inconclusive' })).toBe('◌');
    expect(k({ label: 'thin', mean: '0.1' })).toBe('◐');
    expect(k({ label: 'thin', mean: '-0.1' })).toBe('◑');
    expect(k({ label: 'thin', mean: '0' })).toBe('◑');
    expect(k({ label: 'thin', mean: null })).toBe('◑');
    expect(k({ label: 'dead_on_paper' })).toBe('✗');
    expect(k({ label: 'not_tested' })).toBe('–');
    expect(GLYPHS[glyphKey(undefined)]).toBe('·');
  });

  it('writes the detail line with the capture at that latency, and the footnote', () => {
    const r = sweetRow({
      dimension: 'category', bucket: 'crypto', scenario: 'rung-25', latency_ms: 25, label: 'thin', mean: '0.3', lo: '-0.1', hi: '0.7', n_exec: 12, clusters: 9,
    });
    expect(cellDetail({ dim: 'category', bucket: 'crypto' }, { scenario: 'rung-25', latency_ms: 25 }, r)).toBe(
      'crypto · rung-25 · n 12 · C 9 · captured at 25 ms, per $100: +$0.30 [−$0.10, +$0.70] · too few trades or events to say anything.',
    );
    expect(cellDetail({ dim: 'all', bucket: 'all' }, { scenario: 'rung-0', latency_ms: 0 }, undefined)).toBe('all · rung-0 · no attempts in this slice');
    expect(gridNote({ m: 364, z: '3.62' })).toBe('z = 3.62 across 364 rows tested: the more rows, the wider each interval.');
  });
});
```

Create `web/src/components/SweetLadderGrid.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { GRID_HINT } from '../lib/sweet';
import { sweetRow } from '../test/fixtures';
import { SweetLadderGrid } from './SweetLadderGrid';

const rows = [
  sweetRow({ scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
  sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2', lo: '0.05', hi: '0.35', n_exec: 40, clusters: 33 }),
];

describe('SweetLadderGrid', () => {
  it('is a table of buttons named by bucket, rung and verdict; focus shows the detail line under it', () => {
    render(<SweetLadderGrid rows={rows} side="no" dim="category" label="NO · CATEGORY" />);
    const t = screen.getByRole('table', { name: 'NO · CATEGORY' });
    expect(within(t).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '0', '25', '52', '100', '200', '300']);
    expect(within(t).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['all', 'crypto']);
    expect(within(t).getAllByRole('button')).toHaveLength(12);
    const b = within(t).getByRole('button', { name: 'crypto at rung-0 (0 ms): candidate' });
    expect(b).toHaveTextContent('●');
    expect(within(t).getByRole('button', { name: 'crypto at rung-25 (25 ms): no attempts' })).toHaveTextContent('·');
    expect(screen.getByText(GRID_HINT)).toBeInTheDocument();
    fireEvent.focus(b);
    expect(b).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('crypto · rung-0 · n 40 · C 33 · captured at 0 ms, per $100: +$0.20 [+$0.05, +$0.35] · survives here; worth a real small-order test.')).toBeInTheDocument();
  });

  it('shows the legend beside the grid, in words', () => {
    render(<SweetLadderGrid rows={rows} side="no" dim="all" label="NO · ALL" />);
    const legend = screen.getByRole('list', { name: 'grid legend' });
    expect(within(legend).getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      '● candidate', '○ negative', '◌ inconclusive', '◐ thin, mean > 0', '◑ thin, mean ≤ 0', '· no attempts', '✗ dead on paper', '– not tested',
    ]);
  });
});
```

Append to `web/src/theme.test.ts` inside `describe('theme', …)`:

```ts
  it('fits the Sweet spots grid in 360 px: an 88 px label column and six 36 px cells, no cell padding (spec 7a §8)', () => {
    const px = (sel: string) => Number(/(?<![-\w])width:\s*(\d+)px/.exec(rule(sel))?.[1]);
    expect(px('.lgrid .lg-row')).toBe(88);
    expect(px('.lgrid .lg-cell')).toBe(36);
    expect(px('.lgrid .lg-row') + 6 * px('.lgrid .lg-cell')).toBeLessThanOrEqual(328);
    expect(rule('.lgrid th, .lgrid td')).toMatch(/padding:\s*0/);
    expect(rule('.lgrid')).toMatch(/width:\s*auto/);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/sweet.test.ts src/components/SweetLadderGrid.test.tsx src/theme.test.ts`
Expected: FAIL — `ladderGrid is not a function` / `Failed to resolve import "./SweetLadderGrid"` / `no rule for .lgrid .lg-row`.

- [ ] **Step 3: Implement**

`web/src/lib/sweet.ts` — replace lines 1-3 with:

```ts
import type { SweetLabel, SweetRow, SweetSeries } from '../api/types';
import { LADDER, LADDER_YES } from './ladder';
import { compareDecimal, formatMoney, signOf } from './money';
import { fmtUtcHm } from './time';
```

change line 99 (now line 100 after the import) from `export function grid(s: SweetSeries, side: SweetSide, dim: SweetDim): Grid {` to:

```ts
export function grid(s: Pick<SweetSeries, 'rows'>, side: SweetSide, dim: SweetDim): Grid {
```

and append at the end of the file:

```ts
/** Spec 7a §8: one glyph per cell. ● and ○ are the only cells whose interval clears zero (from above,
 *  from below); ◐ and ◑ are thin, and nothing is concluded from them. */
export type GlyphKey = 'candidate' | 'negative' | 'inconclusive' | 'thinUp' | 'thinDown' | 'dead_on_paper' | 'not_tested' | 'none';
export const GLYPHS: Record<GlyphKey, string> = {
  candidate: '●', negative: '○', inconclusive: '◌', thinUp: '◐', thinDown: '◑', dead_on_paper: '✗', not_tested: '–', none: '·',
};
export const GLYPH_NAME: Record<GlyphKey, string> = {
  candidate: 'candidate', negative: 'negative', inconclusive: 'inconclusive', thinUp: 'thin, mean > 0', thinDown: 'thin, mean ≤ 0',
  dead_on_paper: 'dead on paper', not_tested: 'not tested', none: 'no attempts',
};
export const GLYPH_ORDER: GlyphKey[] = ['candidate', 'negative', 'inconclusive', 'thinUp', 'thinDown', 'none', 'dead_on_paper', 'not_tested'];
export const GRID_HINT = 'Tap a cell for its numbers.';

export function glyphKey(r: SweetRow | undefined): GlyphKey {
  if (!r) return 'none';
  if (r.label === 'thin') return signOf(r.mean) > 0 ? 'thinUp' : 'thinDown';
  return r.label;
}

export interface LadderGridRow {
  dim: SweetDim;
  bucket: string;
}

export interface LadderGrid {
  columns: { scenario: string; latency_ms: number }[];
  rows: LadderGridRow[];
  cell: (row: LadderGridRow, scenario: string) => SweetRow | undefined;
}

/** Spec 7a §8: rows = the `all` row first, then the chosen dimension's buckets in canonical order (only
 *  the `only` ones when given); columns = the side's LADDER scenarios by latency (6 NO, 2 YES). Fee
 *  variants and the extras never get a column; they stay in the "all numbers" fold. */
export function ladderGrid(rows: SweetRow[], side: SweetSide, dim: SweetDim, only?: string[]): LadderGrid {
  const names = side === 'no' ? LADDER.map((l) => l.scenario) : LADDER_YES;
  const columns = LADDER.filter((l) => names.includes(l.scenario)).map((l) => ({ scenario: l.scenario, latency_ms: l.latency_ms }));
  const all = grid({ rows }, side, 'all');
  const sub = dim === 'all' ? null : grid({ rows }, side, dim);
  const buckets = (sub?.buckets ?? []).filter((b) => only === undefined || only.includes(b));
  return {
    columns,
    rows: [{ dim: 'all', bucket: 'all' }, ...buckets.map((bucket) => ({ dim, bucket }))],
    cell: (row, scenario) => (row.dim === 'all' ? all.cell('all', scenario) : sub?.cell(row.bucket, scenario)),
  };
}

/** The line under the grid for one cell (spec §8): bucket, scenario, n, clusters, captured at that
 *  latency per $100 with its interval, and the label sentence. */
export function cellDetail(row: LadderGridRow, col: { scenario: string; latency_ms: number }, r: SweetRow | undefined): string {
  const head = `${bucketText(row.dim, row.bucket)} · ${col.scenario}`;
  if (!r) return `${head} · no attempts in this slice`;
  return `${head} · n ${r.n_exec} · C ${r.clusters} · captured at ${col.latency_ms} ms, per $100: ${interval(r)} · ${LABEL_TEXT[r.label]}`;
}

/** Spec 7a §3: the ladder grows m, so z is stricter; the grid's footnote says so. */
export function gridNote(s: Pick<SweetSeries, 'm' | 'z'>): string {
  return `z = ${s.z} across ${s.m} rows tested: the more rows, the wider each interval.`;
}
```

Create `web/src/components/SweetLadderGrid.tsx`:

```tsx
import { useState } from 'react';
import type { SweetRow } from '../api/types';
import {
  GLYPHS, GLYPH_NAME, GLYPH_ORDER, GRID_HINT, type LadderGridRow, type SweetDim, type SweetSide, bucketText, cellDetail, glyphKey, ladderGrid,
} from '../lib/sweet';

const cellId = (row: LadderGridRow, scenario: string) => `${row.dim}|${row.bucket}|${scenario}`;

/**
 * Spec 7a §8: buckets × LADDER rungs as a plain HTML table of buttons (no SVG, so no aspect-ratio
 * trouble). At 360 px: 88 px labels + 6 × 36 px cells = 304 px inside 328. Tap or focus a cell for one
 * detail line under the grid (not a popover); the legend sits beside it, in words.
 */
export function SweetLadderGrid({ rows, side, dim, label, only }: {
  rows: SweetRow[];
  side: SweetSide;
  dim: SweetDim;
  label: string;
  only?: string[];
}) {
  const g = ladderGrid(rows, side, dim, only);
  const [pick, setPick] = useState<string | null>(null);
  let detail = GRID_HINT;
  for (const row of g.rows) {
    for (const col of g.columns) {
      if (cellId(row, col.scenario) === pick) detail = cellDetail(row, col, g.cell(row, col.scenario));
    }
  }
  return (
    <div className="lgrid-wrap">
      <table className="lgrid" aria-label={label}>
        <thead>
          <tr>
            <th scope="col" className="lg-row">ms</th>
            {g.columns.map((c) => (
              <th key={c.scenario} scope="col" className="lg-col" title={c.scenario}>
                {c.latency_ms}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {g.rows.map((row) => (
            <tr key={`${row.dim}|${row.bucket}`}>
              <th scope="row" className="lg-row">{bucketText(row.dim, row.bucket)}</th>
              {g.columns.map((c) => {
                const k = glyphKey(g.cell(row, c.scenario));
                const id = cellId(row, c.scenario);
                return (
                  <td key={c.scenario}>
                    <button
                      type="button"
                      className={`lg-cell g-${k}`}
                      aria-label={`${bucketText(row.dim, row.bucket)} at ${c.scenario} (${c.latency_ms} ms): ${GLYPH_NAME[k]}`}
                      aria-pressed={pick === id}
                      onClick={() => setPick(id)}
                      onFocus={() => setPick(id)}
                    >
                      {GLYPHS[k]}
                    </button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <ul className="plain lg-legend" aria-label="grid legend">
        {GLYPH_ORDER.map((k) => (
          <li key={k}>
            <span aria-hidden="true">{GLYPHS[k]}</span> {GLYPH_NAME[k]}
          </li>
        ))}
      </ul>
      <p className="lg-detail" aria-live="polite">{detail}</p>
    </div>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §8: the Sweet spots grid. 88 + 6 × 36 = 304 px ≤ 328 (theme.test pins it). */
.lgrid-wrap { display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: flex-start; margin: 8px 0; }
.lgrid { width: auto; table-layout: fixed; border-collapse: collapse; }
.lgrid th, .lgrid td { padding: 0; border: 0; text-align: center; }
.lgrid .lg-row { width: 88px; text-align: left; font-family: var(--body); font-size: 13px; white-space: normal; color: var(--dim); padding-right: 4px; }
.lgrid .lg-col { width: 36px; font-family: var(--body); font-size: 13px; color: var(--dim); }
.lgrid .lg-cell { width: 36px; height: 32px; border: 1px solid var(--line2); background: var(--bg); font-family: var(--body); font-size: 18px; line-height: 1; cursor: pointer; }
.lgrid .lg-cell[aria-pressed='true'] { outline: 2px solid var(--accent); outline-offset: -2px; }
.lg-cell.g-candidate { color: var(--good); }
.lg-cell.g-negative, .lg-cell.g-dead_on_paper { color: var(--bad); }
.lg-cell.g-inconclusive, .lg-cell.g-thinUp, .lg-cell.g-thinDown, .lg-cell.g-none, .lg-cell.g-not_tested { color: var(--dim); }
.lg-legend { gap: 2px; font-size: 13px; color: var(--dim); }
.lg-detail { flex-basis: 100%; min-height: 1.5em; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/sweet.test.ts src/components/SweetLadderGrid.test.tsx src/theme.test.ts && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/lib/sweet.ts src/lib/sweet.test.ts src/components/SweetLadderGrid.tsx src/components/SweetLadderGrid.test.tsx src/theme.css src/theme.test.ts && git commit -m "web: the Sweet spots grid: buckets by the six ladder rungs, one glyph per verdict, a detail line on focus"
```

---

### Task 4: `/sweet` shows the grid instead of the per-scenario chart

Load `frontend-design:frontend-design` before editing the page.

**Files:**
- Modify: `web/src/pages/Sweet.tsx:1-22` (imports), `:87-141` (`SweetBody`), `:143-187` (`Sweet`)
- Test: `web/src/pages/Sweet.test.tsx:1` (imports), `:30-31`, `:48-59`, `:106`, append one test before the closing `});` at line 108

**Interfaces:**
- Consumes: `SweetLadderGrid` (Task 3), `gridNote` (Task 3).
- Produces: `SweetBody` props `{ card; s; side; setSide; dimAsked; setDim }` (the scenario props are gone); `/sweet` renders `<table aria-label="<SIDE> · <DIM>">` from `SweetLadderGrid`; no `group` named `scenario`, no `IntervalChart` on the page (Radar's `SweetCard` keeps it).

- [ ] **Step 1: Write the failing tests** — in `web/src/pages/Sweet.test.tsx`:

change line 1 to:

```tsx
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
```

replace lines 30-31 (the chart assertions) with:

```tsx
    const g = screen.getByRole('table', { name: 'NO · ALL' });
    expect(within(g).getByRole('button', { name: 'all at colo-idle (52 ms): candidate' })).toHaveTextContent('●');
    expect(within(g).getByRole('button', { name: 'all at sp-idle (300 ms): negative' })).toHaveTextContent('○');
    expect(within(g).getByRole('button', { name: 'all at rung-0 (0 ms): no attempts' })).toHaveTextContent('·');
    expect(screen.getByText('z = 1.96 across 2 rows tested: the more rows, the wider each interval.')).toBeInTheDocument();
```

replace lines 48-59 (the `switches scenario, side and slice…` test) with:

```tsx
  it('has no scenario chips: six NO columns, two YES columns, CATEGORY hidden without tags', async () => {
    mockApi({
      '/api/cards': {
        win: '30d',
        cards: [card([...rows, sweetRow({ side: 'yes', label: 'dead_on_paper' }), sweetRow({ side: 'yes', scenario: 'sp-burst', latency_ms: 450, label: 'candidate' })], {}, { category_available: false })],
      },
    });
    renderPage();
    const g = await screen.findByRole('table', { name: 'NO · ALL' });
    expect(within(g).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '0', '25', '52', '100', '200', '300']);
    expect(screen.queryByRole('group', { name: 'scenario' })).toBeNull();
    expect(within(screen.getByRole('group', { name: 'dimension' })).queryByRole('button', { name: 'CATEGORY' })).toBeNull();
    await userEvent.click(within(screen.getByRole('group', { name: 'side' })).getByRole('button', { name: 'YES' }));
    const yes = screen.getByRole('table', { name: 'YES · ALL' });
    expect(within(yes).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['ms', '52', '300']);
    expect(within(yes).getByRole('button', { name: 'all at sp-idle (300 ms): dead on paper' })).toHaveTextContent('✗');
  });
```

change line 106 from `expect(screen.queryByRole('list', { name: /sp-idle/ })).toBeNull();` to:

```tsx
    expect(screen.queryByRole('table', { name: /NO · ALL/ })).toBeNull();
```

and append before the file's last `});`:

```tsx
  it('slices by category: the all row first, then its buckets; focus shows one detail line', async () => {
    const withCrypto = [
      ...rows,
      sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-25', latency_ms: 25, label: 'thin', mean: '0.3', n_exec: 12, clusters: 9 }),
    ];
    mockApi({ '/api/cards': { win: '30d', cards: [card(withCrypto)] } });
    renderPage();
    await userEvent.click(within(await screen.findByRole('group', { name: 'dimension' })).getByRole('button', { name: 'CATEGORY' }));
    const g = screen.getByRole('table', { name: 'NO · CATEGORY' });
    expect(within(g).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['all', 'crypto']);
    const cell = within(g).getByRole('button', { name: 'crypto at rung-25 (25 ms): thin, mean > 0' });
    expect(cell).toHaveTextContent('◐');
    fireEvent.focus(cell);
    expect(screen.getByText('crypto · rung-25 · n 12 · C 9 · captured at 25 ms, per $100: +$0.30 · too few trades or events to say anything.')).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/pages/Sweet.test.tsx`
Expected: FAIL — `Unable to find role="table" and name "NO · ALL"`.

- [ ] **Step 3: Implement** — in `web/src/pages/Sweet.tsx`:

replace lines 1-22 (imports) with:

```tsx
import { useState } from 'react';
import { api } from '../api/client';
import type { CardBody, SweetSeries } from '../api/types';
import { useApi } from '../api/useApi';
import { Box } from '../components/Box';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { Help } from '../components/Help';
import { Loadable } from '../components/Loadable';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import { Tag } from '../components/Tag';
import { Tiles } from '../components/Tiles';
import { type Win, WindowSwitch } from '../components/WindowSwitch';
import { computedText, shortHash, statusLabel, statusTone, warmingText } from '../lib/cards';
import { useNow, useTicker } from '../lib/hooks';
import { formatMoney, formatPercent } from '../lib/money';
import {
  COLO_TITLE, DIMS, FEW_TRADES, HEADLINE_SCENARIO, LABEL_TAG, LABEL_TEXT, NOTHING_PAYS, ONE_LINE_CAVEAT, SWEET_CAVEAT,
  SWEET_EMPTY, type SweetDim, type SweetSide, bucketText, candidateCount, feeLine, footnote, grid, gridNote, interval, nothingPays,
  recomputingText,
} from '../lib/sweet';
import { fmtLocal, fmtUtcDateTime } from '../lib/time';
```

replace lines 87-141 (the comment above `SweetBody` through its closing `}`) with:

```tsx
/** M1: side/dim live in Sweet() so a window switch (which replaces this card by identity in
 *  Loadable's render) never resets the chips the owner just picked. Spec 7a §8: the grid replaces
 *  the per-scenario chart and its chips; every scenario's numbers stay in "all numbers". */
function SweetBody({ card, s, side, setSide, dimAsked, setDim }: {
  card: CardBody; s: SweetSeries;
  side: SweetSide; setSide: (v: SweetSide) => void;
  dimAsked: SweetDim; setDim: (v: SweetDim) => void;
}) {
  const nowMs = useNow(30_000);
  const dims = DIMS.filter(([d]) => d !== 'category' || s.category_available);
  const dim = dimAsked === 'category' && !s.category_available ? 'all' : dimAsked;
  const extraNotes = card.notes.filter((n) => n !== SWEET_CAVEAT && n !== footnote(s.m));
  return (
    <>
      <Tiles
        items={[
          { label: 'NO candidates', value: String(candidateCount(s, 'no')) },
          { label: 'YES candidates', value: String(candidateCount(s, 'yes')) },
          { label: 'rows tested', value: String(s.m) },
          { label: 'headline scenario', value: HEADLINE_SCENARIO },
        ]}
      />
      {feeLine(s) && <p className="dim">{feeLine(s)}</p>}
      {nothingPays(s) && <p className="quip">{NOTHING_PAYS}</p>}
      <div className="filters">
        <Chips label="side" options={[['no', 'NO'], ['yes', 'YES']]} value={side} onChange={setSide} />
        {side === 'no' ? <Help id="sweetNo" /> : <Help id="sweetYes" />}
        <Chips label="dimension" options={dims} value={dim} onChange={setDim} />
      </div>
      <SweetLadderGrid rows={s.rows} side={side} dim={dim} label={`${side.toUpperCase()} · ${dim.toUpperCase()}`} />
      <p className="dim">{gridNote(s)}</p>
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
```

and in `Sweet()` (originally lines 143-187) delete the line `const [scenAsked, setScen] = useState<string>(HEADLINE_SCENARIO);` (originally 151) and the two props `scenAsked={scenAsked}` / `setScen={setScen}` (originally 179-180) from the `<SweetBody …/>` element.

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/pages/Sweet.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS; tsc reports no unused `IntervalChart`/`rowsFor` import (both were removed from the import block).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/pages/Sweet.tsx src/pages/Sweet.test.tsx && git commit -m "web: /sweet shows the ladder grid (6 NO, 2 YES columns); the per-scenario chart and its chips are gone"
```

---
### Task 5: the hero drawing `LatencyLadder`

Load `frontend-design:frontend-design` before writing the component and CSS.

**Files:**
- Create: `web/src/schematics/LatencyLadder.tsx`
- Modify: `web/src/theme.css` (append)
- Test: `web/src/schematics/LatencyLadder.test.tsx` (create)

**Interfaces:**
- Consumes: `LadderModel`, `NOT_MONOTONE`, `MIN_EXEC`, `SLOTS`, `asOfText`, `breakEvenText`, `epochText`, `extraText`, `latencyX`, `rungCountText`, `rungMsText`, `slotX`, `stackRung`, `usBand`, `usText`, `yDomain` (Task 2); `SweetLadderGrid` (Task 3); `Fold`, `Money`, `EmptyState`.
- Produces:

```ts
export function LatencyLadder(props: { model: LadderModel }): ReactElement   // never fetches
export const LADDER_CAPTION, AXIS_TITLE, MARKETS_CAPTION, MARKETS_WARMING, MARKETS_RECOMPUTING, MARKETS_MISSING, NO_RUNGS, TABLE_CAPTION: string
// DOM contract: <figure class="ladder">; p.ll-big (VT323, the one big number); svg.ll-svg (preserveAspectRatio none) with
//   g[data-scenario] > rect.ll-seg.ll-{gains|settle|unrealized|leg_risk}, rect.ll-net; rect.ll-missing[data-scenario];
//   rect.ll-extra[data-scenario]; rect.ll-us; line.ll-zero; line.ll-be; pattern#ll-hatch-*;
//   <ol aria-label="rungs"> 7 li (6 rungs: span ms, span count; then "extras"); folds "which markets" and "the numbers".
```

- [ ] **Step 1: Write the failing test** — create `web/src/schematics/LatencyLadder.test.tsx`:

```tsx
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { Ladder } from '../api/types';
import { NOT_MONOTONE, ladderModel, slotX } from '../lib/ladder';
import { ladderFixture, ladderLiveFixture } from '../test/fixtures';
import { LADDER_CAPTION, LatencyLadder, MARKETS_CAPTION, MARKETS_MISSING, MARKETS_RECOMPUTING, MARKETS_WARMING } from './LatencyLadder';

const W = 700;
const draw = (o: Partial<Ladder> = {}) => render(<LatencyLadder model={ladderModel(ladderFixture(o))} />);
const num = (el: Element | null, a: string) => Number(el?.getAttribute(a));
const rungItems = () => within(screen.getByRole('list', { name: 'rungs' })).getAllByRole('listitem');

describe('LatencyLadder', () => {
  it('says the break-even big, marks it between its two rungs, and labels every rung in HTML', () => {
    const { container } = draw();
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(screen.getByText('Pays on paper below it, loses on paper beyond it.')).toBeInTheDocument();
    const be = container.querySelector('line.ll-be');
    expect(num(be, 'x1')).toBeGreaterThan(slotX(2) * W);
    expect(num(be, 'x1')).toBeLessThan(slotX(3) * W);
    expect(rungItems().map((li) => li.firstChild?.textContent)).toEqual(['0', '25', '52', '100', '200', '300', 'extras']);
    expect(rungItems().slice(0, 6).map((li) => li.lastChild?.textContent)).toEqual(['40', '40', '40', '40', '40', '40']);
    expect(screen.getByText('as of 21:14 UTC')).toBeInTheDocument();
    expect(screen.getByText('$ on paper since epoch 1 began, 2026-09-29 19:58 UTC')).toBeInTheDocument();
    expect(container.querySelector('svg.ll-svg')).toHaveAttribute('preserveAspectRatio', 'none');
    expect(container.querySelector('svg.ll-svg text')).toBeNull();
  });

  it('signs the bars: basket gains above zero, leg-risk cost below, unrealized hatched', () => {
    const { container } = draw();
    const zero = num(container.querySelector('line.ll-zero'), 'y1');
    const g = container.querySelector('g[data-scenario="colo-idle"]');
    const gains = g?.querySelector('rect.ll-gains') ?? null;
    expect(num(gains, 'y') + num(gains, 'height')).toBeCloseTo(zero);
    expect(num(g?.querySelector('rect.ll-leg_risk') ?? null, 'y')).toBeGreaterThanOrEqual(zero);
    const fill = g?.querySelector('rect.ll-unrealized')?.getAttribute('fill') ?? '';
    const id = /^url\(#(.+)\)$/.exec(fill)?.[1];
    expect(id).toMatch(/^ll-hatch-/);
    expect(container.querySelector(`pattern[id="${id}"]`)).not.toBeNull();
    expect(container.querySelectorAll('rect.ll-extra')).toHaveLength(2);
    expect(screen.getByText(/^sp-burst \(450 ms idle, more in bursts\):/)).toHaveTextContent('sp-burst (450 ms idle, more in bursts): −$80.10 on paper · n 40');
  });

  it('puts the us-band between the right rungs and names it; without a measurement there is no band', () => {
    const { container, unmount } = draw();
    const us = container.querySelector('rect.ll-us');
    expect(num(us, 'x')).toBeGreaterThan(slotX(4) * W);
    expect(num(us, 'x')).toBeLessThan(slotX(5) * W);
    expect(num(us, 'x') + num(us, 'width')).toBeGreaterThan(slotX(5) * W);
    expect(screen.getByText('us: measured ≈155 + matching')).toBeInTheDocument();
    unmount();
    const r = draw({ measured: null });
    expect(r.container.querySelector('rect.ll-us')).toBeNull();
    expect(screen.getByText('us: not measured yet')).toBeInTheDocument();
  });

  it('renders every break_even kind', () => {
    for (const [be, big] of [
      [{ kind: 'all' }, 'pays at every rung'],
      [{ kind: 'none' }, 'none'],
      [{ kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true }, '≈ 40 ms'],
    ] as const) {
      const r = draw({ break_even: be });
      expect(screen.getByText(big)).toHaveClass('ll-big');
      r.unmount();
    }
    const { container } = draw({ break_even: { kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true } });
    expect(screen.getByText(NOT_MONOTONE)).toBeInTheDocument();
    expect(screen.getByText('Loses on paper below it, pays on paper beyond it.')).toBeInTheDocument();
    expect(container.querySelector('line.ll-be')).not.toBeNull();
  });

  it('thin: "too few trades yet", each rung n/30, extras still drawn', () => {
    const { container } = render(<LatencyLadder model={ladderModel(ladderLiveFixture())} />);
    expect(screen.getByText('too few trades yet')).toHaveClass('ll-big');
    expect(rungItems().slice(0, 6).map((li) => li.lastChild?.textContent)).toEqual(['2/30', '2/30', '1/30', '1/30', '0/30', '0/30']);
    expect(screen.getByText('as of 20:09 UTC')).toBeInTheDocument();
    expect(container.querySelector('line.ll-be')).toBeNull();
    expect(container.querySelectorAll('rect.ll-extra')).toHaveLength(2);
  });

  it('a rung with no account is an empty column labelled so, and the answer says why (review focus 1)', () => {
    const { container } = draw({ rungs: ladderFixture().rungs.filter((r) => r.scenario !== 'rung-100'), break_even: { kind: 'thin' } });
    expect(container.querySelector('rect.ll-missing[data-scenario="rung-100"]')).not.toBeNull();
    expect(container.querySelector('g[data-scenario="rung-100"]')).toBeNull();
    expect(screen.getByText('no account')).toBeInTheDocument();
    expect(screen.getByText('a rung has no account yet')).toHaveClass('ll-big');
    expect(screen.queryByText('too few trades yet')).toBeNull();
    expect(rungItems()).toHaveLength(7);
  });

  it('a fresh epoch and epoch 0 draw no NaN (review focus 3)', () => {
    const epoch0 = ladderFixture({ epoch: 0, start_ms: null, as_of_ms: null, rungs: [], measured: null, break_even: { kind: 'thin' } });
    for (const l of [ladderLiveFixture(), epoch0]) {
      const { container, unmount } = render(<LatencyLadder model={ladderModel(l)} />);
      for (const el of container.querySelectorAll('svg rect, svg line')) {
        for (const a of ['x', 'y', 'width', 'height', 'x1', 'x2', 'y1', 'y2']) {
          const v = el.getAttribute(a);
          if (v !== null) expect(Number.isFinite(Number(v)), `${el.getAttribute('class')} ${a}=${v}`).toBe(true);
        }
      }
      unmount();
    }
    render(<LatencyLadder model={ladderModel(epoch0)} />);
    expect(screen.getByText('no epoch yet')).toHaveClass('ll-big');
    expect(screen.getAllByText('no account')).toHaveLength(6);
  });

  it('which markets: the compact grid for live and warming, words for under_review and missing; the time frame is named', () => {
    const a = draw();
    const fold = screen.getByText('which markets').closest('details') as HTMLDetailsElement;
    expect(fold.open).toBe(false);
    expect(within(fold).getByText(MARKETS_CAPTION)).toBeInTheDocument();
    expect(within(fold).getByRole('table', { name: 'which markets' })).toBeInTheDocument();
    expect(within(fold).getByRole('button', { name: 'crypto at rung-0 (0 ms): thin, mean > 0' })).toHaveTextContent('◐');
    a.unmount();
    const b = draw({ markets_status: 'warming' });
    expect(screen.getByText(MARKETS_WARMING)).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'which markets' })).toBeInTheDocument();
    b.unmount();
    const c = draw({ markets_status: 'under_review', markets: [] });
    expect(screen.getByText(MARKETS_RECOMPUTING)).toBeInTheDocument();
    expect(screen.queryByRole('table', { name: 'which markets' })).toBeNull();
    c.unmount();
    draw({ markets_status: 'missing', markets: [] });
    expect(screen.getByText(MARKETS_MISSING)).toBeInTheDocument();
  });

  it('names the unwind rule and keeps the numbers in a closed fold, labelled paper', () => {
    draw();
    expect(screen.getByText(LADDER_CAPTION)).toBeInTheDocument();
    const nums = screen.getByText('the numbers').closest('details') as HTMLDetailsElement;
    expect(nums.open).toBe(false);
    expect(within(nums).getAllByRole('row')).toHaveLength(9); // header + 6 rungs + 2 extras
    expect(within(nums).getByText(/^paper, since the epoch start/)).toBeInTheDocument();
    expect(screen.getAllByText('−$80.10')).toHaveLength(2); // extras line + the numbers fold
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/schematics/LatencyLadder.test.tsx`
Expected: FAIL — `Failed to resolve import "./LatencyLadder"`.

- [ ] **Step 3: Implement** — create `web/src/schematics/LatencyLadder.tsx`:

```tsx
import { useId } from 'react';
import type { LadderRung } from '../api/types';
import { EmptyState } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { Money } from '../components/Money';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import {
  type LadderModel, MIN_EXEC, NOT_MONOTONE, SLOTS, asOfText, breakEvenText, epochText, extraText, latencyX, rungCountText, rungMsText,
  slotX, stackRung, usBand, usText, yDomain,
} from '../lib/ladder';

/** Spec 7a §7.2 / §12: one epoch is one path (no interval), and the unwind rule is named. */
export const LADDER_CAPTION = 'One epoch is one path, so there is no interval. A missed leg sells the rest at the bid.';
export const AXIS_TITLE = 'ms from the exchange showing the edge to our order matching';
/** Spec §17: the bars are since the epoch start; the fold is per trade attempt over 30 days, across epochs. */
export const MARKETS_CAPTION = 'Sweet spots: per trade attempt over the last 30 days, across epochs. Not the same number as the bars above.';
export const MARKETS_WARMING = 'warming: part of the 30 days is still being filled.';
export const MARKETS_RECOMPUTING = 'recomputing after the ladder';
export const MARKETS_MISSING = 'no markets data';
export const NO_RUNGS = 'No ladder account has a mark in this epoch yet.';
export const TABLE_CAPTION = 'paper, since the epoch start, every sum cut at the same mark';

const W = 700;
const H = 200;
const BAR = 40;
const LEGEND: [string, string][] = [
  ['gains', 'basket gains (converts)'],
  ['leg_risk', 'leg-risk cost (unwinds)'],
  ['settle', 'settled at resolution'],
  ['unrealized', 'unrealized, marked now'],
  ['net', 'net on paper'],
];
const HEADS = ['RUNG', 'N', 'LEG RISK', 'MISSED', 'BASKET GAINS', 'LEG-RISK COST', 'SETTLED', 'UNREALIZED', 'NET'];

/** A money string as geometry only; a malformed value draws at zero instead of NaN. */
const geo = (v: string) => {
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
};

function Markets({ model }: { model: LadderModel }) {
  const { status, rows } = model.markets;
  if (status === 'under_review') return <EmptyState text={MARKETS_RECOMPUTING} />;
  if (status === 'missing') return <EmptyState text={MARKETS_MISSING} />;
  return (
    <>
      <p className="dim">{MARKETS_CAPTION}</p>
      {status === 'warming' && <p className="dim">{MARKETS_WARMING}</p>}
      <SweetLadderGrid rows={rows} side="no" dim="category" label="which markets" />
    </>
  );
}

function LadderTable({ rungs }: { rungs: LadderRung[] }) {
  if (rungs.length === 0) return <EmptyState text={NO_RUNGS} />;
  return (
    <div className="scroll">
      <table>
        <caption className="dim">{TABLE_CAPTION}</caption>
        <thead>
          <tr>
            {HEADS.map((h) => (
              <th key={h} scope="col">{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rungs.map((r) => (
            <tr key={r.account_id}>
              <td>{r.scenario}</td>
              <td className="num">{r.n_exec}</td>
              <td className="num">{r.n_leg_risk}</td>
              <td className="num">{r.n_missed}</td>
              <td className="num"><Money value={r.gains} signed cents /></td>
              <td className="num"><Money value={r.leg_risk} signed cents /></td>
              <td className="num"><Money value={r.settle} signed cents /></td>
              <td className="num"><Money value={r.unrealized} signed cents /></td>
              <td className="num"><Money value={r.net} signed tone cents /></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Spec 7a §7.2, "at what latency would this pay?": one stacked bar per rung on an ordinal axis (basket
 * gains up, leg-risk cost down, settle and unrealized by their sign, unrealized hatched), a net marker,
 * the zero line, the "us" band, the extras beside the 300 ms rung, and the break-even line. The one big
 * number is the break-even; every other label is HTML beside the SVG (preserveAspectRatio="none").
 * Pure: it draws `model` and never fetches.
 */
export function LatencyLadder({ model }: { model: LadderModel }) {
  const hatch = `ll-hatch-${useId().replace(/[^A-Za-z0-9_-]/g, '')}`;
  const present = model.columns.flatMap((c) => (c.kind === 'rung' ? [c.rung] : []));
  const d = yDomain([...present, ...model.extras]);
  const sy = (v: number) => ((d.hi - v) / (d.hi - d.lo)) * H;
  const zero = sy(0);
  const be = breakEvenText(model);
  const thin = model.breakEven.kind === 'thin';
  const band = model.measured ? usBand(model.measured) : null;
  const beX = model.breakEven.kind === 'at' ? latencyX(model.breakEven.latency_ms) * W : null;
  return (
    <figure className="ladder" aria-label="at what latency would this pay">
      <p className="ll-kicker">break-even</p>
      <p className="ll-big">{be.big}</p>
      <p className="ll-line">{be.line}</p>
      {model.breakEven.kind === 'at' && model.breakEven.not_monotone === true && <p className="dim">{NOT_MONOTONE}</p>}
      <p className="dim ll-ylabel">{epochText(model)}</p>
      <svg className="ll-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="basket gains and leg-risk cost per rung">
        <defs>
          <pattern id={hatch} width={6} height={6} patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect className="ll-hatch-line" x={0} y={0} width={2} height={6} />
          </pattern>
        </defs>
        {band && <rect className="ll-us" x={band.x0 * W} y={0} width={Math.max(1, (band.x1 - band.x0) * W)} height={H} />}
        {model.columns.map((c, i) => {
          const cx = slotX(i) * W;
          if (c.kind === 'missing') {
            return <rect key={c.scenario} className="ll-missing" data-scenario={c.scenario} x={cx - BAR / 2} y={1} width={BAR} height={H - 2} />;
          }
          return (
            <g key={c.scenario} data-scenario={c.scenario}>
              {stackRung(c.rung).map((s) => (
                <rect
                  key={s.part}
                  className={`ll-seg ll-${s.part}`}
                  x={cx - BAR / 2}
                  y={Math.min(sy(s.from), sy(s.to))}
                  width={BAR}
                  height={Math.abs(sy(s.from) - sy(s.to))}
                  fill={s.part === 'unrealized' ? `url(#${hatch})` : undefined}
                />
              ))}
              <rect className="ll-net" x={cx - BAR / 2 - 4} y={sy(geo(c.rung.net)) - 2} width={BAR + 8} height={4} />
            </g>
          );
        })}
        {model.extras.map((r, j) => {
          const cx = slotX(SLOTS - 1) * W + (j % 2 === 0 ? -14 : 14);
          return <rect key={r.account_id} className="ll-extra" data-scenario={r.scenario} x={cx - 5} y={sy(geo(r.net)) - 5} width={10} height={10} />;
        })}
        <line className="ll-zero" x1={0} x2={W} y1={zero} y2={zero} />
        {beX !== null && <line className="ll-be" x1={beX} x2={beX} y1={0} y2={H} />}
      </svg>
      <ol className="ll-axis" aria-label="rungs">
        {model.columns.map((c) => (
          <li key={c.scenario} className={c.kind === 'missing' ? 'missing' : undefined} title={c.scenario}>
            <span className="num">{rungMsText(c)}</span>
            <span>{rungCountText(c, thin)}</span>
          </li>
        ))}
        <li>
          <span>extras</span>
        </li>
      </ol>
      <p className="dim ll-xlabel">{`${AXIS_TITLE} · counts: trades per rung${thin ? `, of the ${MIN_EXEC} needed` : ''}`}</p>
      <p className="dim">{asOfText(model.asOfMs)}</p>
      <ul className="plain ll-legend" aria-label="legend">
        {LEGEND.map(([k, text]) => (
          <li key={k}>
            <span className={`ll-sw ${k}`} aria-hidden="true" />
            {text}
          </li>
        ))}
        <li>
          <span className="ll-sw us" aria-hidden="true" />
          {usText(model.measured)}
        </li>
      </ul>
      {model.extras.length > 0 && (
        <ul className="plain ll-extras" aria-label="extras">
          {model.extras.map((r) => (
            <li key={r.account_id}>
              {extraText(r)}: <Money value={r.net} signed tone cents /> on paper · n {r.n_exec}
            </li>
          ))}
        </ul>
      )}
      <p className="quip">{LADDER_CAPTION}</p>
      <Fold id="ladder:markets" label="which markets">
        <Markets model={model} />
      </Fold>
      <Fold id="ladder:numbers" label="the numbers">
        <LadderTable rungs={[...present, ...model.extras]} />
      </Fold>
    </figure>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §7.2: the hero. Every SVG part has its own ll-* class (no shared .dot sizing); labels are HTML. */
.ladder { display: grid; gap: 4px; margin: 0; }
.ll-kicker { color: var(--dim); font-size: 13px; }
.ll-big { font-family: var(--big); font-size: 40px; line-height: 1; }
.ll-svg { display: block; width: 100%; height: 170px; background: var(--bg); border: 2px solid var(--line2); }
.ll-svg .ll-gains { fill: var(--teal); }
.ll-svg .ll-settle { fill: var(--mint); }
.ll-svg .ll-leg_risk { fill: var(--salmon); }
.ll-svg .ll-unrealized { stroke: var(--teal); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.ll-svg .ll-hatch-line { fill: var(--teal); }
.ll-svg .ll-net { fill: var(--fg); }
.ll-svg .ll-extra { fill: var(--bg); stroke: var(--fg); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.ll-svg .ll-missing { fill: none; stroke: var(--line2); stroke-width: 1px; stroke-dasharray: 4 3; vector-effect: non-scaling-stroke; }
.ll-svg .ll-us { fill: var(--rose); opacity: 0.35; }
.ll-svg .ll-zero { stroke: var(--fg); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.ll-svg .ll-be { stroke: var(--accent); stroke-width: 2px; stroke-dasharray: 4 3; vector-effect: non-scaling-stroke; }
.ll-axis { list-style: none; display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); text-align: center; font-size: 13px; line-height: 1.3; }
.ll-axis li { display: grid; min-width: 0; }
.ll-axis li.missing { color: var(--dim); }
.ll-legend { grid-template-columns: repeat(auto-fit, minmax(min(100%, 150px), 1fr)); gap: 2px 10px; font-size: 13px; }
.ll-sw { display: inline-block; width: 10px; height: 10px; margin-right: 4px; vertical-align: middle; }
.ll-sw.gains { background: var(--teal); }
.ll-sw.leg_risk { background: var(--salmon); }
.ll-sw.settle { background: var(--mint); }
.ll-sw.unrealized { background: repeating-linear-gradient(45deg, var(--teal) 0 2px, transparent 2px 5px); border: 1px solid var(--teal); }
.ll-sw.net { background: var(--fg); height: 3px; }
.ll-sw.us { background: var(--rose); opacity: 0.6; }
.ll-extras { gap: 2px; font-size: 13px; }
```

(`.ll-unrealized` has no `fill` rule on purpose: a CSS `fill` would override the `url(#ll-hatch-…)` attribute.)

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/schematics/LatencyLadder.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/schematics/LatencyLadder.tsx src/schematics/LatencyLadder.test.tsx src/theme.css && git commit -m "web: the latency ladder hero: signed stacks per rung, the us band, missing rungs drawn, the break-even as the one big number"
```

---

### Task 6: Paper page: the epoch line, `earlier epochs` (lazy), P3 labelled

Load `frontend-design:frontend-design` before editing the page.

**Files:**
- Create: `web/src/lib/epochs.ts`, `web/src/api/useEpochs.ts`
- Modify: `web/src/components/Fold.tsx:25-41` (children may be a function of `open`)
- Modify: `web/src/pages/Paper.tsx:1-16` (imports), `:21-22` (constants after `NOTHING_AWAITING`), `:114` (P3 checkbox text), `:154` (P3 in the legend), above `:269` (two new components), `:269` (`PaperBody` signature), after `:328` (the new fold), `:333-352` (`Paper`)
- Modify: `web/src/theme.css` (append)
- Test: `web/src/lib/epochs.test.ts` (create), `web/src/components/Fold.test.tsx` (append), `web/src/pages/Paper.test.tsx:5` (imports), `:255` (checkbox name), append tests before the closing `});` at line 260

**Interfaces:**
- Consumes: `Epochs`, `PaperAccount` (Task 1), `api.paperEpochs`, `api.paperEquity(…, epoch)` (Task 1), `LADDER`, `fmtUtcDayHm` (Task 2), `isZero`.
- Produces:

```ts
// lib/epochs.ts
export const EPOCH_HOURS = 6; export const P3_NOTE = 'control: ignores fees';
export function epochLine(e: Epochs): string            // "since reset 2026-09-30 08:00 UTC · epoch 2" | "since new policies … · epoch 1" | "epoch 0 · no reset yet"
export interface EpochSpan { epoch: number; start_ms: number | null; end_ms: number; cause: string }
export function earlierEpochs(e: Epochs): EpochSpan[]   // newest first; epoch 0 = { start_ms: null, cause: 'before epochs' }
export function epochAccounts(accounts: PaperAccount[]): PaperAccount[]   // P0-converter at fee 0, LADDER order then the rest by name
export function currentEpochText(e: Epochs): string     // "epoch 1, started 2026-09-29 19:58 UTC (policies)"
export function nextAllowedText(ms: number): string     // "next reset allowed after 2026-10-01 00:00 UTC"
// api/useEpochs.ts
export const RESET_POLL_MS = 10_000;
export function useEpochs(): ApiState<Epochs>           // polls every 10 s only while pending_request is set
export function useEpochBump(current: number | undefined): number   // +1 each time a known current epoch changes
// components/Fold.tsx
export function Fold(props: { id: string; label: string; children: ReactNode | ((open: boolean) => ReactNode) }): ReactElement
// pages/Paper.tsx
export const NO_EPOCHS, NO_EARLIER, EPOCHS_NOTE: string
// PaperBody props: { accounts: PaperAccount[]; positions: ApiState<{ items: Position[] }>; epochs: ApiState<Epochs> }
```

- [ ] **Step 1: Write the failing tests**

Create `web/src/lib/epochs.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { EPOCH1_START, accountFixture, epochsFixture } from '../test/fixtures';
import { currentEpochText, earlierEpochs, epochAccounts, epochLine, nextAllowedText } from './epochs';

const S2 = Date.UTC(2026, 8, 30, 8, 0);
const two = epochsFixture({
  epochs: [{ epoch: 1, start_ms: EPOCH1_START, cause: 'policies' }, { epoch: 2, start_ms: S2, cause: 'reset' }],
  current: 2,
});

describe('epochs', () => {
  it('says since when and which epoch, by cause', () => {
    expect(epochLine(epochsFixture())).toBe('since new policies 2026-09-29 19:58 UTC · epoch 1');
    expect(epochLine(two)).toBe('since reset 2026-09-30 08:00 UTC · epoch 2');
    expect(epochLine(epochsFixture({ epochs: [], current: 0 }))).toBe('epoch 0 · no reset yet');
  });

  it('lists every earlier epoch newest first, epoch 0 without a start', () => {
    expect(earlierEpochs(two)).toEqual([
      { epoch: 1, start_ms: EPOCH1_START, end_ms: S2, cause: 'policies' },
      { epoch: 0, start_ms: null, end_ms: EPOCH1_START, cause: 'before epochs' },
    ]);
    expect(earlierEpochs(epochsFixture({ epochs: [], current: 0 }))).toEqual([]);
  });

  it('keeps the P0 accounts at fee 0, ladder order first, then the extras', () => {
    const p0 = (scenario: string, o = {}) => accountFixture({ account_id: `P0-converter/${scenario}`, scenario, ...o });
    const got = epochAccounts([
      p0('sp-idle'), p0('sp-burst'), p0('rung-0'), p0('sp-idle', { account_id: 'P0-converter/sp-idle@10', fee_bips: '10' }),
      accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive' }), p0('colo-idle'), p0('sp-idle-nodelay'),
    ]);
    expect(got.map((a) => a.account_id)).toEqual([
      'P0-converter/rung-0', 'P0-converter/colo-idle', 'P0-converter/sp-idle', 'P0-converter/sp-burst', 'P0-converter/sp-idle-nodelay',
    ]);
  });

  it('writes the reset fold lines', () => {
    expect(currentEpochText(epochsFixture())).toBe('epoch 1, started 2026-09-29 19:58 UTC (policies)');
    expect(currentEpochText(epochsFixture({ epochs: [], current: 0 }))).toBe('epoch 0: no epoch has started yet');
    expect(nextAllowedText(Date.UTC(2026, 9, 1))).toBe('next reset allowed after 2026-10-01 00:00 UTC');
  });
});
```

Append to `web/src/components/Fold.test.tsx` inside `describe('Fold', …)`:

```tsx
  it('renders function children only while open (a lazy fold fetches nothing until opened)', async () => {
    render(<Fold id="lazy" label="earlier epochs">{(open) => (open ? <p>fetched</p> : null)}</Fold>);
    expect(screen.queryByText('fetched')).toBeNull();
    await userEvent.click(screen.getByText('earlier epochs'));
    expect(screen.getByText('fetched')).toBeInTheDocument();
  });
```

In `web/src/pages/Paper.test.tsx` change line 5 to:

```tsx
import { EPOCH1_START, accountFixture, edgeFixture, epochsFixture, ledgerFixture, positionFixture } from '../test/fixtures';
```

change line 255 (`'hide P3 (control)'`) to `'hide P3 (control: ignores fees)'`, and append before the file's last `});`:

```tsx
  const S2 = Date.UTC(2026, 8, 30, 8, 0);
  const twoEpochs = epochsFixture({
    epochs: [{ epoch: 1, start_ms: EPOCH1_START, cause: 'policies' }, { epoch: 2, start_ms: S2, cause: 'reset' }],
    current: 2,
  });

  it('says since when and which epoch, above ACCOUNTS', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
      '/api/paper/epochs': twoEpochs,
    });
    renderAt('/paper');
    const line = await screen.findByText('since reset 2026-09-30 08:00 UTC · epoch 2');
    const head = await screen.findByRole('heading', { level: 2, name: /^ACCOUNTS/ });
    expect(line.compareDocumentPosition(head) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('earlier epochs: nothing fetched until the fold opens, then each epoch with its P0 fee-0 final equity', async () => {
    const f = mockApi({
      '/api/paper/accounts': { items: [...accounts, accountFixture({ account_id: 'P0-converter/rung-0', scenario: 'rung-0' })] },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': (url: URL) =>
        url.searchParams.has('epoch')
          ? {
              account_id: url.searchParams.get('account'), from_ms: 0, to_ms: 1,
              points: [{ t_ms: 0, equity: '1000' }, { t_ms: 1, equity: url.searchParams.get('epoch') === '0' ? '476.2' : '1003.5' }],
            }
          : equityRoute(url),
      '/api/paper/ledger': { items: [], next_cursor: null },
      '/api/paper/epochs': twoEpochs,
    });
    renderAt('/paper');
    await screen.findByText('since reset 2026-09-30 08:00 UTC · epoch 2');
    const epochCalls = () => f.mock.calls.map((c) => String(c[0])).filter((u) => u.includes('epoch='));
    expect(epochCalls()).toEqual([]);
    await userEvent.click(screen.getByText('earlier epochs'));
    const fold = screen.getByText('earlier epochs').closest('details') as HTMLDetailsElement;
    await waitFor(() => expect(within(fold).getAllByText('$476.20')).toHaveLength(3));
    expect(within(fold).getAllByText('$1,003.50')).toHaveLength(3);
    expect(within(fold).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['EPOCH', 'FROM', 'TO', 'CAUSE', 'RUNG-0', 'COLO-IDLE', 'SP-IDLE']);
    expect(within(fold).getByText('before epochs')).toBeInTheDocument();
    expect(epochCalls().every((u) => u.includes('hours=6'))).toBe(true);
    expect(new Set(epochCalls().map((u) => new URL(u, 'http://x').searchParams.get('account')))).toEqual(
      new Set(['P0-converter/rung-0', 'P0-converter/colo-idle', 'P0-converter/sp-idle']),
    );
  });

  it('labels P3 as the control that ignores fees', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
    });
    renderAt('/paper?scenario=sp-idle');
    expect(await screen.findByRole('checkbox', { name: 'hide P3 (control: ignores fees)' })).toBeInTheDocument();
    const legend = await screen.findByRole('list', { name: 'equity now' });
    expect(within(legend).getByText('· control: ignores fees')).toBeInTheDocument();
    expect(within(legend).getByText('P3 naive')).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/epochs.test.ts src/components/Fold.test.tsx src/pages/Paper.test.tsx`
Expected: FAIL — `Failed to resolve import "./epochs"`; the Fold test hits React's "Functions are not valid as a React child"; the Paper tests find no epoch line and no `earlier epochs` fold.

- [ ] **Step 3: Implement**

Create `web/src/lib/epochs.ts`:

```ts
import type { Epochs, PaperAccount } from '../api/types';
import { LADDER } from './ladder';
import { isZero } from './money';
import { fmtUtcDayHm } from './time';

/** An earlier epoch's final equity is its last mark; marks are per minute, so its last 6 h are plenty. */
export const EPOCH_HOURS = 6;
/** Spec §17: P3-naive loses the same amount at every latency; it is the fee-blind control. */
export const P3_NOTE = 'control: ignores fees';

/** Spec 7a §6: the line above ACCOUNTS. An epoch started by new policies is not called a reset. */
export function epochLine(e: Epochs): string {
  const cur = e.epochs.find((x) => x.epoch === e.current);
  if (!cur) return 'epoch 0 · no reset yet';
  return `${cur.cause === 'reset' ? 'since reset' : 'since new policies'} ${fmtUtcDayHm(cur.start_ms)} · epoch ${cur.epoch}`;
}

export interface EpochSpan {
  epoch: number;
  /** null for epoch 0: it is everything before epoch 1. */
  start_ms: number | null;
  end_ms: number;
  cause: string;
}

/** Every epoch before the current one, newest first; each ends where the next one starts. */
export function earlierEpochs(e: Epochs): EpochSpan[] {
  const list = [...e.epochs].sort((a, b) => a.epoch - b.epoch);
  return list
    .map((x, i): EpochSpan =>
      i === 0
        ? { epoch: 0, start_ms: null, end_ms: x.start_ms, cause: 'before epochs' }
        : { epoch: list[i - 1].epoch, start_ms: list[i - 1].start_ms, end_ms: x.start_ms, cause: list[i - 1].cause },
    )
    .reverse();
}

/** Spec 7a §6: the P0 accounts at fee 0 (the ladder and its extras), LADDER order, then by name. */
export function epochAccounts(accounts: PaperAccount[]): PaperAccount[] {
  const rank = (a: PaperAccount) => {
    const i = LADDER.findIndex((l) => l.scenario === a.scenario);
    return i === -1 ? LADDER.length : i;
  };
  return accounts
    .filter((a) => a.policy === 'P0-converter' && isZero(a.fee_bips))
    .sort((x, y) => rank(x) - rank(y) || x.scenario.localeCompare(y.scenario));
}

/** Spec 7a §5.3: the reset fold's first line, the current epoch and its start. */
export function currentEpochText(e: Epochs): string {
  const cur = e.epochs.find((x) => x.epoch === e.current);
  return cur ? `epoch ${cur.epoch}, started ${fmtUtcDayHm(cur.start_ms)} (${cur.cause})` : 'epoch 0: no epoch has started yet';
}

export function nextAllowedText(ms: number): string {
  return `next reset allowed after ${fmtUtcDayHm(ms)}`;
}
```

Create `web/src/api/useEpochs.ts`:

```ts
import { useEffect, useRef, useState } from 'react';
import { api } from './client';
import type { Epochs } from './types';
import { useApi, type ApiState } from './useApi';

/** Spec 7a §5.3: while a reset request is pending, ask again every 10 s; never otherwise. */
export const RESET_POLL_MS = 10_000;

export function useEpochs(): ApiState<Epochs> {
  const [poll, setPoll] = useState(0);
  const st = useApi((s) => api.paperEpochs(s), [], [poll]);
  const pending = st.data?.pending_request != null;
  useEffect(() => {
    if (!pending) return;
    const id = setInterval(() => setPoll((n) => n + 1), RESET_POLL_MS);
    return () => clearInterval(id);
  }, [pending]);
  return st;
}

/** Increments each time the current epoch changes after it was first seen (a reset was applied), so a
 *  page refetches its accounts once; the first answer never counts as a change. */
export function useEpochBump(current: number | undefined): number {
  const [bump, setBump] = useState(0);
  const seen = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (current === undefined) return;
    if (seen.current !== undefined && seen.current !== current) setBump((b) => b + 1);
    seen.current = current;
  }, [current]);
  return bump;
}
```

`web/src/components/Fold.tsx` — replace lines 25-41 (`export function Fold` through the end) with:

```tsx
export function Fold({ id, label, children }: { id: string; label: string; children: ReactNode | ((open: boolean) => ReactNode) }) {
  const [open, setOpen] = useState(() => readOpen(id));
  return (
    <details
      className="fold"
      open={open}
      onToggle={(e) => {
        const next = e.currentTarget.open;
        setOpen(next);
        writeOpen(id, next);
      }}
    >
      <summary>{label}</summary>
      {/* Spec 7a §6: a function child renders only while open, so a costly fold fetches nothing closed. */}
      <div className="fold-body">{typeof children === 'function' ? children(open) : children}</div>
    </details>
  );
}
```

`web/src/pages/Paper.tsx` — replace lines 1-16 (imports) with:

```tsx
import { useState } from 'react';
import { useSearchParams } from 'react-router';
import { api } from '../api/client';
import type { Epochs, LedgerEntry, PaperAccount, Position } from '../api/types';
import { useApi, type ApiState } from '../api/useApi';
import { useEpochBump, useEpochs } from '../api/useEpochs';
import { Box } from '../components/Box';
import { EdgeCapture } from '../components/EdgeCapture';
import { EmptyState, errorText } from '../components/EmptyState';
import { Fold } from '../components/Fold';
import { LineChart, type Series } from '../components/LineChart';
import { Loadable } from '../components/Loadable';
import { Money } from '../components/Money';
import { Table, type Column } from '../components/Table';
import { EPOCH_HOURS, type EpochSpan, P3_NOTE, earlierEpochs, epochAccounts, epochLine } from '../lib/epochs';
import { shortId } from '../lib/labels';
import { compareDecimal, formatDecimal, formatMoney, formatMoneyCents, isZero } from '../lib/money';
import { fmtUtcDateTime, fmtUtcDayHm, fmtUtcHm } from '../lib/time';
```

after the `NOTHING_AWAITING` constant (original line 22) add:

```tsx
export const NO_EPOCHS = 'No epoch data yet.';
export const NO_EARLIER = 'No earlier epochs. This is the first one.';
export const EPOCHS_NOTE = 'Final paper equity of each P0 account at fee 0: its last mark before the next epoch began.';
```

replace the checkbox text line `hide P3 (control)` (original line 114) with:

```tsx
          {`hide P3 (${P3_NOTE})`}
```

replace the legend line (original line 154) with:

```tsx
                        <span>{policyLabel(a)}</span>
                        {a.policy === CONTROL && <span className="dim"> · {P3_NOTE}</span>}{' '}
                        <b className="num">{formatMoneyCents(e.points[e.points.length - 1].equity)}</b>
```

insert above `function PaperBody` (original line 269):

```tsx
function EpochRow({ span, accounts }: { span: EpochSpan; accounts: PaperAccount[] }) {
  const ids = accounts.map((a) => a.account_id);
  const finals = useApi(
    (s) => Promise.all(ids.map((id) => api.paperEquity(id, EPOCH_HOURS, s, span.epoch))),
    [span.epoch, ids.join('|')],
  );
  return (
    <tr>
      <td className="num">{span.epoch}</td>
      <td>{span.start_ms === null ? '—' : fmtUtcDayHm(span.start_ms)}</td>
      <td>{fmtUtcDayHm(span.end_ms)}</td>
      <td>{span.cause}</td>
      {finals.error !== undefined && finals.data === undefined ? (
        <td colSpan={ids.length} className="error">{errorText(finals.error)}</td>
      ) : (
        ids.map((id, i) => {
          const pts = finals.data?.[i]?.points ?? [];
          return (
            <td key={id} className="num">
              {finals.data === undefined ? '…' : <Money value={pts.length > 0 ? pts[pts.length - 1].equity : null} cents />}
            </td>
          );
        })
      )}
    </tr>
  );
}

/** Spec 7a §6: each earlier epoch with start, end, cause and each P0 fee-0 account's final equity. */
function EarlierEpochs({ epochs, accounts }: { epochs: Epochs; accounts: PaperAccount[] }) {
  const spans = earlierEpochs(epochs);
  const cols = epochAccounts(accounts);
  if (spans.length === 0) return <EmptyState text={NO_EARLIER} />;
  return (
    <>
      <p className="dim">{EPOCHS_NOTE}</p>
      <div className="scroll">
        <table>
          <thead>
            <tr>
              {['EPOCH', 'FROM', 'TO', 'CAUSE', ...cols.map((a) => a.scenario.toUpperCase())].map((h) => (
                <th key={h} scope="col">{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {spans.map((s) => (
              <EpochRow key={s.epoch} span={s} accounts={cols} />
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

```

replace the `PaperBody` signature line (original line 269) with:

```tsx
function PaperBody({ accounts, positions, epochs }: {
  accounts: PaperAccount[];
  positions: ApiState<{ items: Position[] }>;
  epochs: ApiState<Epochs>;
}) {
```

insert after the `awaiting resolution` fold's closing `</Fold>` (original line 328), before `</Box>`:

```tsx
      <Fold id="paper:epochs" label="earlier epochs">
        {(open) => (!open ? null : epochs.data ? <EarlierEpochs epochs={epochs.data} accounts={accounts} /> : <EmptyState text={NO_EPOCHS} />)}
      </Fold>
```

and replace `export function Paper()` (original lines 333-352) with:

```tsx
export function Paper() {
  const epochs = useEpochs();
  // A reset applied while the page is open: refetch the accounts once, so they show the bankroll again.
  const bump = useEpochBump(epochs.data?.current);
  const accounts = useApi((s) => api.paperAccounts(s), [], [bump]);
  const positions = useApi((s) => api.paperPositions(s), [], [bump]);
  return (
    <>
      <EdgeCapture />
      {epochs.data && <p className="epochline">{epochLine(epochs.data)}</p>}
      <Loadable state={accounts} what="paper accounts">
        {(d) =>
          d.items.length === 0 ? (
            <Box title="PAPER" help="paper">
              <EmptyState text={PAPER_EMPTY} />
            </Box>
          ) : (
            <PaperBody accounts={d.items} positions={positions} epochs={epochs} />
          )
        }
      </Loadable>
    </>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §6: since when the paper accounts count. */
.epochline { color: var(--dim); padding: 0 2px; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/lib/epochs.test.ts src/components/Fold.test.tsx src/pages/Paper.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS (the older Paper tests have no `/api/paper/epochs` route: the epoch line is simply absent there).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/lib/epochs.ts src/lib/epochs.test.ts src/api/useEpochs.ts src/components/Fold.tsx src/components/Fold.test.tsx src/pages/Paper.tsx src/pages/Paper.test.tsx src/theme.css && git commit -m "web: Paper says since when and which epoch; earlier epochs fold (fetches only when opened); P3 labelled the fee-blind control"
```

---

### Task 7: the reset fold

Load `frontend-design:frontend-design` before writing the component and CSS.

**Files:**
- Create: `web/src/components/ResetFold.tsx`
- Modify: `web/src/pages/Paper.tsx` (two imports; `PaperBody` gets `now` and renders `<ResetFold>` after the `earlier epochs` fold)
- Modify: `web/src/theme.css` (append)
- Test: `web/src/components/ResetFold.test.tsx` (create), `web/src/pages/Paper.test.tsx` (append one test)

**Interfaces:**
- Consumes: `api.paperReset`, `ApiError` (Task 1), `useEpochs` (Task 6), `currentEpochText`, `nextAllowedText` (Task 6), `Fold`, `Loadable`, `ErrorState`, `useNow`.
- Produces:

```ts
export const RESET_COPY = 'Starts every paper account again at its bankroll. Earlier epochs stay in the history.';
export const RESET_QUEUED = 'queued: the scanner applies it within ~30 s';
export const RESET_OFF = 'reset is off on this server';
export const RESET_WORD = 'reset';
export function ResetFold(props: { epochs: ApiState<Epochs>; now: number }): ReactElement
// <details> "reset paper money": the current epoch line, the copy, then ONE of: queued (pending, or just sent) ·
//   reset is off (503) · next reset allowed after … (next_allowed_ms > now) · the form
//   <input aria-label="type reset to confirm"> + <button type="submit">RESET</button> (disabled until the box reads "reset");
//   409/429 refetch the epochs and show the server's state; any other failure keeps the form and shows the api detail.
```

- [ ] **Step 1: Write the failing tests** — create `web/src/components/ResetFold.test.tsx`:

```tsx
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useEpochs } from '../api/useEpochs';
import { nextAllowedText } from '../lib/epochs';
import { epochsFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { RESET_COPY, RESET_OFF, RESET_QUEUED, ResetFold } from './ResetFold';

const NOW = Date.UTC(2026, 8, 30, 12, 0);
const TOMORROW = Date.UTC(2026, 9, 1);
const pending = epochsFixture({ pending_request: { id: 3, requested_ms: NOW - 5_000 }, next_allowed_ms: TOMORROW });

function Harness() {
  const e = useEpochs();
  return <ResetFold epochs={e} now={NOW} />;
}

const posts = (f: ReturnType<typeof mockApi>) => f.mock.calls.filter((c) => c[1]?.method === 'POST');

describe('ResetFold', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps RESET off until the box reads reset, posts once for a double click, then says queued (review focus 4)', async () => {
    let queued = false;
    const f = mockApi({
      '/api/paper/epochs': () => (queued ? pending : epochsFixture()),
      '/api/paper/reset': (_u: URL, init?: RequestInit) => {
        if (init?.method !== 'POST') return jsonResponse(405, { error: 'method_not_allowed', detail: 'x' });
        queued = true;
        return jsonResponse(202, { request_id: 3, requested_ms: NOW });
      },
    });
    render(<Harness />);
    expect(await screen.findByText('epoch 1, started 2026-09-29 19:58 UTC (policies)')).toBeInTheDocument();
    expect(screen.getByText(RESET_COPY)).toBeInTheDocument();
    const box = screen.getByRole('textbox', { name: 'type reset to confirm' });
    const btn = screen.getByRole('button', { name: 'RESET' });
    await userEvent.type(box, 'rese');
    expect(btn).toBeDisabled();
    await userEvent.type(box, 't');
    expect(btn).toBeEnabled();
    fireEvent.click(btn);
    fireEvent.click(btn);
    expect(await screen.findByText(RESET_QUEUED)).toBeInTheDocument();
    expect(posts(f)).toHaveLength(1);
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it.each([
    ['409 reset_pending → queued, from the server', 409, 'reset_pending', 'x', () => pending, RESET_QUEUED],
    ['429 reset_daily_limit → next allowed', 429, 'reset_daily_limit', 'x', () => epochsFixture({ next_allowed_ms: TOMORROW }), nextAllowedText(TOMORROW)],
    ['503 reset_disabled → off', 503, 'reset_disabled', 'x', () => epochsFixture(), RESET_OFF],
    ['500 → the api detail, form kept', 500, 'internal', 'Something broke on our side. It has been written down.', () => epochsFixture(), 'Something broke on our side. It has been written down.'],
  ])('%s (review focus 4)', async (_name, status, code, detail, after, text) => {
    let posted = false;
    mockApi({
      '/api/paper/epochs': () => (posted ? after() : epochsFixture()),
      '/api/paper/reset': () => {
        posted = true;
        return jsonResponse(status, { error: code, detail });
      },
    });
    render(<Harness />);
    await userEvent.type(await screen.findByRole('textbox', { name: 'type reset to confirm' }), 'reset');
    await userEvent.click(screen.getByRole('button', { name: 'RESET' }));
    expect(await screen.findByText(text)).toBeInTheDocument();
  });

  it('asks again every 10 s only while a request is pending (review focus 4)', async () => {
    vi.useFakeTimers();
    let n = 0;
    const applied = epochsFixture({
      epochs: [...epochsFixture().epochs, { epoch: 2, start_ms: NOW, cause: 'reset' }],
      current: 2,
      next_allowed_ms: TOMORROW,
    });
    const f = mockApi({ '/api/paper/epochs': () => (++n <= 2 ? pending : applied) });
    render(<Harness />);
    const calls = () => f.mock.calls.filter((c) => String(c[0]) === '/api/paper/epochs').length;
    const tick = async (ms: number) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    };
    await tick(0);
    await tick(0);
    expect(calls()).toBe(1);
    expect(screen.getByText(RESET_QUEUED)).toBeInTheDocument();
    await tick(10_000);
    expect(calls()).toBe(2);
    await tick(10_000);
    expect(calls()).toBe(3);
    expect(screen.getByText('epoch 2, started 2026-09-30 12:00 UTC (reset)')).toBeInTheDocument();
    expect(screen.getByText(nextAllowedText(TOMORROW))).toBeInTheDocument();
    await tick(60_000);
    expect(calls()).toBe(3);
  });

  it("says when the next reset is allowed once today's is used, and offers no box", async () => {
    mockApi({ '/api/paper/epochs': epochsFixture({ next_allowed_ms: TOMORROW }) });
    render(<Harness />);
    expect(await screen.findByText(nextAllowedText(TOMORROW))).toBeInTheDocument();
    expect(screen.queryByRole('textbox')).toBeNull();
  });
});
```

Append to `web/src/pages/Paper.test.tsx` (before the last `});`):

```tsx
  it('puts the reset fold last on the page, closed', async () => {
    mockApi({
      '/api/paper/accounts': { items: accounts },
      '/api/paper/positions': { items: [] },
      '/api/paper/equity': equityRoute,
      '/api/paper/ledger': { items: [], next_cursor: null },
      '/api/paper/epochs': epochsFixture(),
    });
    const { container } = renderAt('/paper');
    await screen.findByText('since new policies 2026-09-29 19:58 UTC · epoch 1');
    const summaries = [...container.querySelectorAll('details.fold > summary')].map((s) => s.textContent);
    expect(summaries.at(-1)).toBe('reset paper money');
    expect(summaries.at(-2)).toBe('earlier epochs');
    expect((screen.getByText('reset paper money').closest('details') as HTMLDetailsElement).open).toBe(false);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/components/ResetFold.test.tsx src/pages/Paper.test.tsx`
Expected: FAIL — `Failed to resolve import "./ResetFold"`; the Paper test finds no `reset paper money` fold.

- [ ] **Step 3: Implement** — create `web/src/components/ResetFold.tsx`:

```tsx
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { ApiError, api } from '../api/client';
import type { Epochs } from '../api/types';
import type { ApiState } from '../api/useApi';
import { currentEpochText, nextAllowedText } from '../lib/epochs';
import { ErrorState } from './EmptyState';
import { Fold } from './Fold';
import { Loadable } from './Loadable';

export const RESET_COPY = 'Starts every paper account again at its bankroll. Earlier epochs stay in the history.';
export const RESET_QUEUED = 'queued: the scanner applies it within ~30 s';
export const RESET_OFF = 'reset is off on this server';
export const RESET_WORD = 'reset';

type Phase = 'idle' | 'sending' | 'sent' | 'off' | 'failed';

function ResetBody({ e, now, reload }: { e: Epochs; now: number; reload: () => void }) {
  const [typed, setTyped] = useState('');
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<unknown>(undefined);
  const busy = useRef(false);
  // A fresh answer from the server replaces "sent": from then on its pending_request speaks.
  useEffect(() => {
    setPhase((p) => (p === 'sent' ? 'idle' : p));
  }, [e]);
  const send = async () => {
    if (busy.current || typed !== RESET_WORD) return;
    busy.current = true;
    setPhase('sending');
    try {
      await api.paperReset();
      setTyped('');
      setPhase('sent');
      reload();
    } catch (err) {
      if (err instanceof ApiError && (err.code === 'reset_pending' || err.code === 'reset_daily_limit')) {
        // Someone queued first, or today's reset is used: the refetched epochs say which.
        setTyped('');
        setPhase('idle');
        reload();
      } else if (err instanceof ApiError && err.code === 'reset_disabled') {
        setPhase('off');
      } else {
        setError(err);
        setPhase('failed');
      }
    } finally {
      busy.current = false;
    }
  };
  let state: ReactNode = null;
  if (e.pending_request !== null || phase === 'sent') state = <p className="quip" role="status">{RESET_QUEUED}</p>;
  else if (phase === 'off') state = <p className="quip" role="status">{RESET_OFF}</p>;
  else if (e.next_allowed_ms > now) state = <p className="quip" role="status">{nextAllowedText(e.next_allowed_ms)}</p>;
  return (
    <>
      <p>{currentEpochText(e)}</p>
      <p className="dim">{RESET_COPY}</p>
      {state ?? (
        <form
          className="resetform"
          onSubmit={(ev) => {
            ev.preventDefault();
            void send();
          }}
        >
          <label>
            type <b>{RESET_WORD}</b> to confirm{' '}
            <input
              type="text"
              aria-label="type reset to confirm"
              value={typed}
              onChange={(ev) => setTyped(ev.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <button type="submit" disabled={typed !== RESET_WORD || phase === 'sending'}>
            {phase === 'sending' ? 'SENDING…' : 'RESET'}
          </button>
        </form>
      )}
      {phase === 'failed' && <ErrorState error={error} />}
    </>
  );
}

/** Spec 7a §5.3: at the bottom of the Paper page, typed confirmation, once per UTC day, no dialogs. */
export function ResetFold({ epochs, now }: { epochs: ApiState<Epochs>; now: number }) {
  return (
    <Fold id="paper:reset" label="reset paper money">
      <Loadable state={epochs} what="epochs">
        {(e) => <ResetBody e={e} now={now} reload={epochs.reload} />}
      </Loadable>
    </Fold>
  );
}
```

In `web/src/pages/Paper.tsx`: add `import { ResetFold } from '../components/ResetFold';` after the `Money` import, and `import { useNow } from '../lib/hooks';` after the `lib/epochs` import; at the top of `PaperBody`'s body (before `const [sp, setSp] = useSearchParams();`) add `const now = useNow(30_000);`; and after the `earlier epochs` `</Fold>` (Task 6), before `</Box>`, add:

```tsx
      <ResetFold epochs={epochs} now={now} />
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §5.3: the reset form. */
.resetform { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
.resetform label { display: inline-flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.resetform input { font: inherit; background: var(--chip); color: var(--fg); border: 2px solid var(--line); padding: 0 4px; width: 9ch; }
.resetform button { font-family: var(--px); font-size: 7px; padding: 5px 8px; border: 2px solid var(--line); background: var(--chip); cursor: pointer; }
.resetform button:disabled { opacity: 0.4; cursor: default; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/components/ResetFold.test.tsx src/pages/Paper.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/components/ResetFold.tsx src/components/ResetFold.test.tsx src/pages/Paper.tsx src/pages/Paper.test.tsx src/theme.css && git commit -m "web: reset paper money fold: typed confirmation, one POST, queued / next allowed / off states, 10 s poll only while pending"
```

---

### Task 8: the explain panel shell, with the hero and the fetch rule

Load `frontend-design:frontend-design` before writing the panel and its CSS.

**Files:**
- Create: `web/src/explain/ExplainContext.tsx`, `web/src/explain/storage.ts`, `web/src/explain/useMedia.ts`, `web/src/explain/questions.tsx`, `web/src/explain/Explain.tsx`
- Modify: `web/src/App.tsx:1-12` (imports), `:44-63` (`App`)
- Modify: `web/src/theme.css` (append), `web/src/theme.test.ts` (append one assertion test)
- Test: `web/src/explain/Explain.test.tsx` (create), `web/src/App.test.tsx:26` (one assertion added)

**Interfaces:**
- Consumes: `api.paperLadder` (Task 1), `useApi`, `useTicker`, `Loadable`, `ladderModel` (Task 2), `LatencyLadder` (Task 5).
- Produces:

```ts
// ExplainContext.tsx
export type ExplainPage = 'radar' | 'episodes' | 'episode' | 'paper';
export type Selection = { kind: 'episode'; episode: EpisodeSummary } | { kind: 'position'; position: Position } | null;
export interface PageData { radar?: { open?: EpisodeSummary[] }; episodes?: { items?: EpisodeSummary[] }; episode?: { detail?: EpisodeDetail }; paper?: { accounts?: PaperAccount[]; positions?: Position[] } }
export interface ExplainState { page: ExplainPage | null; data: PageData; selection: Selection }
export function ExplainProvider(props: { children: ReactNode }): ReactElement
export function useExplainCtx(): ExplainState & { isOpen: boolean; setOpen(v: boolean): void; select(s: Selection): void; publish<P extends ExplainPage>(p: P, d: PageData[P] | null): void }
export function useExplainSelect(): (s: Selection) => void          // a no-op outside the provider (page tests)
export function useExplainPage<P extends ExplainPage>(page: P, data: NonNullable<PageData[P]>): void
//   pages pass api data identities (useApi's data), never fresh literals; unmount clears page and selection
// storage.ts
export function readOpen(): boolean; export function writeOpen(v: boolean): void; export function readQuestion(): string | null; export function writeQuestion(id: string): void
//   keys "explain:open" ("1"/"0") and "explain:q"; every access in try/catch
// useMedia.ts
export function useMedia(query: string): boolean                    // false without matchMedia
// questions.tsx
export interface AnswerProps { s: ExplainState; ladder: ApiState<Ladder> }
export interface Question { id: string; q: string; pages: 'all' | ExplainPage[]; Answer: (p: AnswerProps) => ReactElement }
export const HERO_ID = 'latency'; export const QUESTIONS: Question[]; export function questionsFor(page: ExplainPage | null): Question[]
// Explain.tsx
export const LADDER_POLL_MS = 60_000; export const DOCK_QUERY = '(min-width: 1500px)';
export function Explain(): ReactElement
//   <button class="explain-tab" aria-expanded>EXPLAIN ▸</button>; when open <aside id="explain-panel" aria-label="explain"
//   class="explain" data-mode="dock"|"drawer" tabIndex=-1> with h2 EXPLAIN, a "close" button, "as selected: … · back to the page",
//   and a disclosure list of <button class="explain-q" aria-expanded> + answer. The Panel (mounted only while open) owns
//   the ONLY panel fetch: useApi(api.paperLadder) refreshed by useTicker(60 s).
```

- [ ] **Step 1: Write the failing tests**

Create `web/src/explain/Explain.test.tsx`:

```tsx
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { EpisodeSummary } from '../api/types';
import { episodeSummaryFixture, ladderFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Explain, LADDER_POLL_MS } from './Explain';
import { ExplainProvider, useExplainPage, useExplainSelect } from './ExplainContext';

const ROW = episodeSummaryFixture();
// Stable identities, as a page's useApi data: a fresh literal each render would republish forever.
const OPEN: EpisodeSummary[] = [ROW];

function RadarLike() {
  useExplainPage('radar', { open: OPEN });
  const select = useExplainSelect();
  return (
    <button type="button" onClick={() => select({ kind: 'episode', episode: ROW })}>
      pick
    </button>
  );
}

const mount = (node: ReactElement = <RadarLike />) =>
  render(
    <MemoryRouter>
      <ExplainProvider>
        {node}
        <Explain />
      </ExplainProvider>
    </MemoryRouter>,
  );
const tab = () => screen.getByRole('button', { name: /EXPLAIN/ });
const panel = () => screen.getByRole('complementary', { name: 'explain' });
const stubMedia = (wide: boolean) =>
  vi.stubGlobal('matchMedia', (q: string) => ({ matches: wide && q === '(min-width: 1500px)', media: q, addEventListener: () => {}, removeEventListener: () => {} }));
const ladderCalls = (f: ReturnType<typeof mockApi>) => f.mock.calls.filter((c) => String(c[0]) === '/api/paper/ladder').length;

describe('the explain panel shell (spec 7a §9.1)', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('is closed by default; the tab opens it as a labelled aside, remembers it, moves focus in, and shows the hero', async () => {
    mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    expect(tab()).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('complementary', { name: 'explain' })).toBeNull();
    fireEvent.click(tab());
    expect(tab()).toHaveAttribute('aria-expanded', 'true');
    expect(panel()).toHaveFocus();
    expect(localStorage.getItem('explain:open')).toBe('1');
    expect(await within(panel()).findByText('≈ 70 ms')).toBeInTheDocument();
  });

  it('Escape closes it and returns focus to the tab; so does the close button', () => {
    mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    fireEvent.click(tab());
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(tab()).toHaveAttribute('aria-expanded', 'false');
    expect(tab()).toHaveFocus();
    expect(localStorage.getItem('explain:open')).toBe('0');
    fireEvent.click(tab());
    fireEvent.click(within(panel()).getByRole('button', { name: 'close' }));
    expect(screen.queryByRole('complementary', { name: 'explain' })).toBeNull();
    expect(tab()).toHaveFocus();
  });

  it('docks at 1500 px and is a drawer below', () => {
    mockApi({ '/api/paper/ladder': ladderFixture() });
    localStorage.setItem('explain:open', '1');
    stubMedia(true);
    const a = mount();
    expect(panel()).toHaveAttribute('data-mode', 'dock');
    a.unmount();
    stubMedia(false);
    mount();
    expect(panel()).toHaveAttribute('data-mode', 'drawer');
  });

  it('fetches /api/paper/ladder only while open: once on open, every 60 s, never while closed (spec §9.3)', async () => {
    vi.useFakeTimers();
    const f = mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    const flush = async (ms = 0) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    };
    await flush(120_000);
    expect(ladderCalls(f)).toBe(0);
    fireEvent.click(tab());
    await flush();
    expect(ladderCalls(f)).toBe(1);
    await flush(LADDER_POLL_MS);
    expect(ladderCalls(f)).toBe(2);
    fireEvent.click(tab());
    await flush(3 * LADDER_POLL_MS);
    expect(ladderCalls(f)).toBe(2);
    expect(f.mock.calls.every((c) => String(c[0]) === '/api/paper/ladder')).toBe(true);
  });

  it('opens the hero by default and falls back to it when the remembered question is not on this page (review focus 5)', () => {
    localStorage.setItem('explain:q', 'locked'); // a Paper question; this page is Radar
    mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    fireEvent.click(tab());
    const qs = [...panel().querySelectorAll<HTMLButtonElement>('button.explain-q')];
    expect(qs[0]).toHaveTextContent('At what latency would this pay?');
    expect(qs[0]).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(qs[0]);
    expect(qs[0]).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(qs[0]);
    expect(localStorage.getItem('explain:q')).toBe('latency');
  });

  it('works with storage blocked: closed by default, still opens', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    expect(tab()).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(tab());
    expect(panel()).toBeInTheDocument();
  });

  it('shows the selection and "back to the page" clears it', () => {
    mockApi({ '/api/paper/ladder': ladderFixture() });
    mount();
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(tab());
    expect(within(panel()).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
    fireEvent.click(within(panel()).getByRole('button', { name: 'back to the page' }));
    expect(within(panel()).queryByText(/as selected/)).toBeNull();
  });
});
```

In `web/src/App.test.tsx`, after line 26 (`expect(screen.getByRole('button', { name: 'Toggle theme' })).toBeInTheDocument();`) add:

```tsx
    expect(screen.getByRole('button', { name: /EXPLAIN/ })).toHaveAttribute('aria-expanded', 'false');
```

Append to `web/src/theme.test.ts` inside `describe('theme', …)`:

```ts
  it('keeps the explain drawer inside the viewport and beside its tab (spec 7a §9.1)', () => {
    expect(rule(".explain[data-mode='drawer']")).toMatch(/width:\s*min\(360px,\s*calc\(100vw - 28px\)\)/);
    expect(rule(".explain[data-mode='dock']")).toMatch(/flex:\s*0 0 360px/);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/Explain.test.tsx src/App.test.tsx src/theme.test.ts`
Expected: FAIL — `Failed to resolve import "./Explain"`; App test finds no EXPLAIN button; theme test `no rule for .explain[data-mode='drawer']`.

- [ ] **Step 3: Implement**

Create `web/src/explain/storage.ts`:

```ts
// Spec 7a §9.1: open/closed and the last question open, in localStorage. Every access is guarded: a
// private window or blocked storage falls back to the defaults (closed, the hero).
const OPEN = 'explain:open';
const QUESTION = 'explain:q';

function get(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}

function set(k: string, v: string): void {
  try {
    localStorage.setItem(k, v);
  } catch {
    // storage blocked: the panel still works, it just forgets
  }
}

export const readOpen = (): boolean => get(OPEN) === '1';
export const writeOpen = (v: boolean): void => set(OPEN, v ? '1' : '0');
export const readQuestion = (): string | null => get(QUESTION);
export const writeQuestion = (id: string): void => set(QUESTION, id);
```

Create `web/src/explain/useMedia.ts`:

```ts
import { useEffect, useState } from 'react';

const query = (q: string): MediaQueryList | null => (typeof globalThis.matchMedia === 'function' ? globalThis.matchMedia(q) : null);

/** A CSS media query as state; false where matchMedia is missing (jsdom). */
export function useMedia(q: string): boolean {
  const [on, setOn] = useState(() => query(q)?.matches ?? false);
  useEffect(() => {
    const m = query(q);
    if (!m) return;
    setOn(m.matches);
    const f = () => setOn(m.matches);
    m.addEventListener('change', f);
    return () => m.removeEventListener('change', f);
  }, [q]);
  return on;
}
```

Create `web/src/explain/ExplainContext.tsx`:

```tsx
import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import type { EpisodeDetail, EpisodeSummary, PaperAccount, Position } from '../api/types';
import { readOpen, writeOpen } from './storage';

/** The pages that publish data to the panel (spec 7a §9.2); every other page gets the hero only. */
export type ExplainPage = 'radar' | 'episodes' | 'episode' | 'paper';
export type Selection = { kind: 'episode'; episode: EpisodeSummary } | { kind: 'position'; position: Position } | null;
export interface PageData {
  radar?: { open?: EpisodeSummary[] };
  episodes?: { items?: EpisodeSummary[] };
  episode?: { detail?: EpisodeDetail };
  paper?: { accounts?: PaperAccount[]; positions?: Position[] };
}
export interface ExplainState {
  page: ExplainPage | null;
  data: PageData;
  selection: Selection;
}

interface Ctx extends ExplainState {
  isOpen: boolean;
  setOpen: (v: boolean) => void;
  select: (s: Selection) => void;
  publish: <P extends ExplainPage>(page: P, d: PageData[P] | null) => void;
}

// Outside a provider (page tests render pages alone) everything is a no-op.
const NOOP: Ctx = { page: null, data: {}, selection: null, isOpen: false, setOpen: () => {}, select: () => {}, publish: () => {} };
const C = createContext<Ctx>(NOOP);

export function ExplainProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<ExplainPage | null>(null);
  const [data, setData] = useState<PageData>({});
  const [selection, setSelection] = useState<Selection>(null);
  const [isOpen, setIsOpen] = useState(readOpen);
  const setOpen = useCallback((v: boolean) => {
    setIsOpen(v);
    writeOpen(v);
  }, []);
  const publish = useCallback(<P extends ExplainPage>(p: P, d: PageData[P] | null) => {
    if (d === null) {
      setPage((cur) => (cur === p ? null : cur));
      setSelection(null);
      setData((cur) => ({ ...cur, [p]: undefined }) as PageData);
      return;
    }
    setPage(p);
    setData((cur) => ({ ...cur, [p]: d }) as PageData);
  }, []);
  const value = useMemo<Ctx>(
    () => ({ page, data, selection, isOpen, setOpen, select: setSelection, publish }),
    [page, data, selection, isOpen, setOpen, publish],
  );
  return <C.Provider value={value}>{children}</C.Provider>;
}

export function useExplainCtx(): Ctx {
  return useContext(C);
}

export function useExplainSelect(): (s: Selection) => void {
  return useContext(C).select;
}

/**
 * Spec 7a §9.3: pages publish what they already fetched; the panel only reads it. Pass api data
 * identities (useApi's `data`), never fresh literals: each new identity republishes. Unmounting (leaving
 * the page) clears the page and the selection.
 */
export function useExplainPage<P extends ExplainPage>(page: P, d: NonNullable<PageData[P]>): void {
  const { publish } = useContext(C);
  const deps = Object.values(d);
  useEffect(() => {
    publish(page, d);
    // `d` is a fresh object each render; its values say when it really changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, publish, ...deps]);
  useEffect(() => () => publish(page, null), [page, publish]);
}
```

Create `web/src/explain/questions.tsx`:

```tsx
import type { ReactElement } from 'react';
import type { Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import type { ExplainPage, ExplainState } from './ExplainContext';

export interface AnswerProps {
  s: ExplainState;
  /** The panel's one fetch (spec §9.3); every answer reuses it, none fetches. */
  ladder: ApiState<Ladder>;
}

export interface Question {
  id: string;
  q: string;
  pages: 'all' | ExplainPage[];
  Answer: (p: AnswerProps) => ReactElement;
}

export const HERO_ID = 'latency';

function Hero({ ladder }: AnswerProps) {
  return <Loadable state={ladder} what="the ladder">{(l) => <LatencyLadder model={ladderModel(l)} />}</Loadable>;
}

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [{ id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero }];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
```

Create `web/src/explain/Explain.tsx`:

```tsx
import { type RefObject, useEffect, useRef, useState } from 'react';
import { api } from '../api/client';
import { useApi } from '../api/useApi';
import { useTicker } from '../lib/hooks';
import { type Selection, useExplainCtx } from './ExplainContext';
import { HERO_ID, questionsFor } from './questions';
import { readQuestion, writeQuestion } from './storage';
import { useMedia } from './useMedia';

/** Spec 7a §9.3: the panel's one fetch is polled every 60 s while open, never while closed. */
export const LADDER_POLL_MS = 60_000;
/** Spec 7a §9.1: docked beside the app at ≥ 1,500 px, an overlay drawer below. */
export const DOCK_QUERY = '(min-width: 1500px)';

function selectionLabel(s: NonNullable<Selection>): string {
  return s.kind === 'episode' ? (s.episode.event_title ?? s.episode.id) : (s.position.event_title ?? s.position.market_id);
}

/** Mounted only while open, so its ticker and its fetch stop when the panel closes. */
function Panel({ mode, focusOnMount, onClose }: { mode: 'dock' | 'drawer'; focusOnMount: RefObject<boolean>; onClose: () => void }) {
  const { page, data, selection, select } = useExplainCtx();
  const tick = useTicker(LADDER_POLL_MS);
  const ladder = useApi((s) => api.paperLadder(s), [], [tick]);
  const aside = useRef<HTMLElement>(null);
  const [cur, setCur] = useState<string | null>(() => readQuestion() ?? HERO_ID);
  useEffect(() => {
    if (focusOnMount.current) {
      aside.current?.focus();
      focusOnMount.current = false;
    }
  }, [focusOnMount]);
  const qs = questionsFor(page);
  // A remembered question that is not on this page falls back to the hero; null = all closed.
  const active = cur === null ? null : qs.some((q) => q.id === cur) ? cur : HERO_ID;
  const s = { page, data, selection };
  return (
    <aside id="explain-panel" ref={aside} tabIndex={-1} aria-label="explain" className="explain" data-mode={mode}>
      <div className="explain-head">
        <h2>EXPLAIN</h2>
        <button type="button" className="explain-close" onClick={onClose}>
          close
        </button>
      </div>
      {selection && (
        <p className="explain-sel">
          {`as selected: ${selectionLabel(selection)} · `}
          <button type="button" onClick={() => select(null)}>
            back to the page
          </button>
        </p>
      )}
      <ul className="explain-qs">
        {qs.map((q) => {
          const on = q.id === active;
          return (
            <li key={q.id}>
              <button
                type="button"
                className="explain-q"
                aria-expanded={on}
                onClick={() => {
                  const next = on ? null : q.id;
                  setCur(next);
                  if (next !== null) writeQuestion(next);
                }}
              >
                {q.q}
              </button>
              {on && (
                <div className="explain-a">
                  <q.Answer s={s} ladder={ladder} />
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

/** Spec 7a §9.1: a vertical EXPLAIN ▸ tab on every page; Esc closes and returns focus to it. */
export function Explain() {
  const { isOpen, setOpen } = useExplainCtx();
  const docked = useMedia(DOCK_QUERY);
  const tab = useRef<HTMLButtonElement>(null);
  const focusOnMount = useRef(false); // focus the panel when the owner opens it, not on a remembered open
  const close = () => {
    setOpen(false);
    tab.current?.focus();
  };
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        tab.current?.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [isOpen, setOpen]);
  return (
    <>
      <button
        ref={tab}
        type="button"
        className="explain-tab"
        aria-expanded={isOpen}
        aria-controls="explain-panel"
        onClick={() => {
          focusOnMount.current = !isOpen;
          setOpen(!isOpen);
        }}
      >
        EXPLAIN ▸
      </button>
      {isOpen && <Panel mode={docked ? 'dock' : 'drawer'} focusOnMount={focusOnMount} onClose={close} />}
    </>
  );
}
```

`web/src/App.tsx` — add after line 5 (`import { LiveDot } …`):

```tsx
import { Explain } from './explain/Explain';
import { ExplainProvider } from './explain/ExplainContext';
```

and replace `export function App()` (lines 44-63) with:

```tsx
export function App() {
  return (
    <ExplainProvider>
      <div className="shell">
        <div className="app">
          <Header />
          <main className="body">
            <Routes>
              <Route path="/" element={<Radar />} />
              <Route path="/sweet" element={<Sweet />} />
              <Route path="/episodes" element={<Episodes />} />
              <Route path="/episodes/:id" element={<EpisodeDetail />} />
              <Route path="/paper" element={<Paper />} />
              <Route path="/papers" element={<Papers />} />
              <Route path="/health" element={<Health />} />
              <Route path="*" element={<NotFound />} />
            </Routes>
          </main>
        </div>
        <Explain />
      </div>
    </ExplainProvider>
  );
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §9.1: the explain panel. Docked at ≥ 1,500 px (app + 360 px panel centred as a pair); below,
   a drawer that stops 28 px short of the right edge so the tab stays visible. */
.shell { display: flex; justify-content: center; align-items: flex-start; gap: 16px; }
.shell > .app { flex: 0 1 1440px; min-width: 0; margin: 0; }
.explain-tab { position: fixed; right: 0; top: 50%; transform: translateY(-50%); writing-mode: vertical-rl; font-family: var(--px); font-size: 8px; padding: 10px 5px; border: 2px solid var(--line); border-right: 0; background: var(--chip); color: var(--fg); z-index: 11; cursor: pointer; }
.explain-tab[aria-expanded='true'] { background: var(--accent); color: var(--accentfg); }
.explain { background: var(--card); border: 3px solid var(--line); padding: 10px 12px; display: grid; gap: 8px; align-content: start; overflow-y: auto; }
.explain:focus { outline: none; }
.explain[data-mode='dock'] { flex: 0 0 360px; position: sticky; top: 16px; max-height: calc(100vh - 32px); box-shadow: 6px 6px 0 var(--line); }
.explain[data-mode='drawer'] { position: fixed; top: 0; right: 28px; bottom: 0; width: min(360px, calc(100vw - 28px)); z-index: 10; box-shadow: -6px 0 0 var(--line); }
.explain-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.explain-head h2 { font-family: var(--px); font-size: 9px; }
.explain-close, .explain-sel button { font: inherit; background: none; border: 0; color: var(--dim); text-decoration: underline; cursor: pointer; padding: 0; }
.explain-sel { color: var(--dim); }
.explain-qs { list-style: none; display: grid; gap: 6px; }
.explain-q { width: 100%; text-align: left; font-family: var(--body); font-weight: 600; font-size: 15px; border: 2px solid var(--line2); background: var(--chip); padding: 6px 8px; cursor: pointer; }
.explain-q[aria-expanded='true'] { border-color: var(--line); }
.explain-q::before { content: '▸ '; }
.explain-q[aria-expanded='true']::before { content: '▾ '; }
.explain-a { padding: 8px 2px; display: grid; gap: 6px; min-width: 0; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/Explain.test.tsx src/App.test.tsx src/theme.test.ts && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/explain/ExplainContext.tsx src/explain/storage.ts src/explain/useMedia.ts src/explain/questions.tsx src/explain/Explain.tsx src/explain/Explain.test.tsx src/App.tsx src/App.test.tsx src/theme.css src/theme.test.ts && git commit -m "web: explain panel shell: tab on every page, dock at 1500 px, drawer below, Esc; its one fetch (the ladder) only while open; the hero"
```

---

### Task 9: pages publish their data; rows set the selection

**Files:**
- Create: `web/src/test/appRoutes.ts`
- Modify: `web/src/components/Table.tsx:16-57` (`onRowClick`)
- Modify: `web/src/pages/Radar.tsx:1-21` (imports), `:140` + `:153` (`IncompleteSets`), `:160-172` (`Radar` hooks), `:203`, `:210`
- Modify: `web/src/pages/Episodes.tsx:1-16` (imports), `:77-81` (`EpisodeRow`), `:96`, `:112` (`EpisodeTable`), `:120-124` (`Episodes` hooks), `:175` (`<EpisodeTable …>`)
- Modify: `web/src/pages/EpisodeDetail.tsx:1-12` (imports), `:185`
- Modify: `web/src/pages/Paper.tsx` (imports; `Paper()` publishes; `PaperBody`'s positions table selects)
- Modify: `web/src/theme.css` (append)
- Test: `web/src/explain/pages.test.tsx` (create)

**Interfaces:**
- Consumes: `useExplainPage`, `useExplainSelect`, `ExplainProvider`, `Explain` (Task 8); every fixture of Task 1.
- Produces:

```ts
// Table.tsx: new optional prop
onRowClick?: (row: T) => void        // the <tr> gets class "pick" and an onClick; links inside still navigate
// appRoutes.ts
export function appRoutes(o?: Record<string, Route>): Record<string, Route>   // every page route answered from fixtures
// pages publish: Radar → ('radar', { open: open.data?.items }); Episodes → ('episodes', { items: page.data?.items });
//   EpisodeDetail → ('episode', { detail: d.data }); Paper → ('paper', { accounts: accounts.data?.items, positions: positions.data?.items })
// selection: Radar rows (both tables) and Episodes rows → { kind: 'episode', episode }; Paper awaiting rows → { kind: 'position', position }
```

- [ ] **Step 1: Write the failing tests** — create `web/src/test/appRoutes.ts`:

```ts
import {
  accountFixture, edgeFixture, episodeDetailFixture, episodeSummaryFixture, epochsFixture, healthFixture, ladderFixture, metaFixture,
  positionFixture, sweetFixture, sweetRow, todayFixture,
} from './fixtures';
import type { Route } from './mockApi';

/** Every route the dashboard pages ask for, answered from fixtures (the panel tests render the whole App). */
export function appRoutes(o: Record<string, Route> = {}): Record<string, Route> {
  return {
    '/api/health/now': { health: healthFixture() },
    '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
    '/api/integrity/summary': { from_ms: 0, to_ms: 1, by_kind: [], total: 0, msgs: 258_000, per_million: '0' },
    '/api/episodes/open': {
      now_ms: 0,
      items: [
        episodeSummaryFixture(),
        episodeSummaryFixture({ id: 'live-1-00000002', kind: 'yes_basket_incomplete', realization: 'not_arbitrage', event_title: 'Who wins the cup?', s_legs: 3 }),
      ],
    },
    '/api/today': todayFixture(1, 0, '0.33'),
    '/api/cards': (u: URL) => ({
      win: u.searchParams.get('win'),
      cards: [
        edgeFixture({ win: u.searchParams.get('win') ?? '24h' }),
        sweetFixture([sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1', lo: '-0.2', hi: '-0.01' })]),
      ],
    }),
    '/api/cards/meta': metaFixture(),
    '/api/episodes': { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 })], next_cursor: null },
    '/api/episodes/live-1-00000001': episodeDetailFixture({ category: 'crypto' }),
    '/api/paper/accounts': { items: [accountFixture(), accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive' })] },
    '/api/paper/positions': { items: [positionFixture(), positionFixture({ market_id: 'm2', end_date: null, event_title: 'Who wins the cup?' })] },
    '/api/paper/equity': (u: URL) => ({ account_id: u.searchParams.get('account'), from_ms: 0, to_ms: 1, points: [{ t_ms: 0, equity: '1000' }] }),
    '/api/paper/ledger': { items: [], next_cursor: null },
    '/api/paper/epochs': epochsFixture(),
    '/api/paper/ladder': ladderFixture(),
    ...o,
  };
}
```

Create `web/src/explain/pages.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { App } from '../App';
import type { EpisodeSummary } from '../api/types';
import { appRoutes } from '../test/appRoutes';
import { episodeSummaryFixture, ladderFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { Explain } from './Explain';
import { ExplainProvider, useExplainPage, useExplainSelect } from './ExplainContext';

const renderApp = (path: string) =>
  render(
    <MemoryRouter initialEntries={[path]}>
      <App />
    </MemoryRouter>,
  );
const openPanel = () => {
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
  return screen.getByRole('complementary', { name: 'explain' });
};

const ROW = episodeSummaryFixture({ id: 'gone', event_title: 'A row that leaves' });
const ONE: EpisodeSummary[] = [ROW];
const NONE: EpisodeSummary[] = [];

function Flip() {
  const [rows, setRows] = useState(ONE);
  useExplainPage('radar', { open: rows });
  const select = useExplainSelect();
  return (
    <>
      <button type="button" onClick={() => select({ kind: 'episode', episode: ROW })}>pick</button>
      <button type="button" onClick={() => setRows(NONE)}>drop</button>
    </>
  );
}

describe('pages publish, rows select (spec 7a §9.3)', () => {
  it('a Radar row click selects it; the panel says so; "back to the page" clears it', async () => {
    mockApi(appRoutes());
    renderApp('/');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    const panel = openPanel();
    expect(within(panel).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole('button', { name: 'back to the page' }));
    expect(within(panel).queryByText(/as selected/)).toBeNull();
  });

  it('leaving the page clears the selection (review focus 5)', async () => {
    mockApi(appRoutes());
    renderApp('/');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    const panel = openPanel();
    expect(within(panel).getByText(/^as selected:/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('link', { name: 'EPISODES' }));
    expect(await screen.findByRole('link', { name: 'Bitcoin price on September 26?' })).toBeInTheDocument();
    expect(within(screen.getByRole('complementary', { name: 'explain' })).queryByText(/as selected/)).toBeNull();
  });

  it('a selection is a snapshot: it stays when its row leaves the list (review focus 5)', () => {
    mockApi({ '/api/paper/ladder': ladderFixture() });
    render(
      <MemoryRouter>
        <ExplainProvider>
          <Flip />
          <Explain />
        </ExplainProvider>
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByText('pick'));
    fireEvent.click(screen.getByText('drop'));
    const panel = openPanel();
    expect(within(panel).getByText(/^as selected: A row that leaves/)).toBeInTheDocument();
  });

  it('an Episodes row and a Paper position select too', async () => {
    mockApi(appRoutes());
    const a = renderApp('/episodes');
    const link = await screen.findByRole('link', { name: 'Bitcoin price on September 26?' });
    fireEvent.click(link.closest('tr') as HTMLTableRowElement);
    expect(within(openPanel()).getByText(/^as selected: Bitcoin price on September 26\?/)).toBeInTheDocument();
    a.unmount();
    renderApp('/paper?scenario=sp-idle');
    const cell = await screen.findByText('Fed decision in October?');
    fireEvent.click(cell.closest('tr') as HTMLTableRowElement);
    expect(within(screen.getByRole('complementary', { name: 'explain' })).getByText(/^as selected: Fed decision in October\?/)).toBeInTheDocument();
  });
});
```

(The second render in the last test reuses the open state the first one stored in `localStorage`, so the panel is already open.)

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/pages.test.tsx`
Expected: FAIL — the snapshot test passes already (Task 8), the others fail: `Unable to find … /^as selected:/` (rows have no click handler yet).

- [ ] **Step 3: Implement** — line numbers below are the files' original ones (before this task adds its import lines).

`web/src/components/Table.tsx` — replace lines 16-57 (`export function Table` through the end) with:

```tsx
export function Table<T>({
  columns,
  rows,
  rowKey,
  rowClass,
  className,
  onRowClick,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  rowClass?: (row: T) => string | undefined;
  /** Hook for a page's own layout of this table (e.g. Radar's phone cards). */
  className?: string;
  /** Spec 7a §9.3: a row click sets the explain panel's selection; links inside still navigate. */
  onRowClick?: (row: T) => void;
}) {
  return (
    <div className="scroll">
      <table className={className}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} scope="col">
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr
              key={rowKey(r)}
              className={[rowClass?.(r), onRowClick ? 'pick' : null].filter(Boolean).join(' ') || undefined}
              onClick={onRowClick ? () => onRowClick(r) : undefined}
            >
              {columns.map((c) => (
                <td key={c.key} className={tdClass(c, r)}>
                  {c.render(r)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
```

`web/src/pages/Radar.tsx` — add after line 6 (`import { useApi } …`):

```tsx
import { useExplainPage, useExplainSelect } from '../explain/ExplainContext';
```

replace line 140 (`function IncompleteSets({ rows, now }: …) {`) with:

```tsx
function IncompleteSets({ rows, now, onPick }: { rows: EpisodeSummary[]; now: number; onPick: (r: EpisodeSummary) => void }) {
```

replace line 153 with:

```tsx
          {open && <Table className="arbtable" columns={episodeColumns(rows, now)} rows={rows} rowKey={(r) => r.id} rowClass={() => 'dim'} onRowClick={onPick} />}
```

after line 164 (`const open = useApi(…)`) add:

```tsx
  useExplainPage('radar', { open: open.data?.items });
  const select = useExplainSelect();
  const pick = (r: EpisodeSummary) => select({ kind: 'episode', episode: r });
```

replace line 203 with:

```tsx
                <Table className="arbtable" columns={episodeColumns(arb, now)} rows={arb} rowKey={(r) => r.id} onRowClick={pick} />
```

and line 210 with:

```tsx
      <IncompleteSets rows={rest} now={now} onPick={pick} />
```

`web/src/pages/Episodes.tsx` — add after line 4 (`import { useApi } …`):

```tsx
import { useExplainPage, useExplainSelect } from '../explain/ExplainContext';
```

replace lines 77-81 (`function EpisodeRow …` through `<tr className="ep">`) with:

```tsx
function EpisodeRow({ r, onPick }: { r: EpisodeSummary; onPick: (r: EpisodeSummary) => void }) {
  const t = kindTag(r.kind);
  const title = r.event_title ?? r.id;
  return (
    <tr className="ep pick" onClick={() => onPick(r)}>
```

replace line 96 (`function EpisodeTable({ rows }: { rows: EpisodeSummary[] }) {`) with:

```tsx
function EpisodeTable({ rows, onPick }: { rows: EpisodeSummary[]; onPick: (r: EpisodeSummary) => void }) {
```

replace line 112 with:

```tsx
            {g.rows.map((r) => <EpisodeRow key={r.id} r={r} onPick={onPick} />)}
```

after line 124 (`const page = useApi(…)`) add:

```tsx
  useExplainPage('episodes', { items: page.data?.items });
  const select = useExplainSelect();
```

and replace line 175 (`<EpisodeTable rows={d.items} />`) with:

```tsx
              <EpisodeTable rows={d.items} onPick={(r) => select({ kind: 'episode', episode: r })} />
```

`web/src/pages/EpisodeDetail.tsx` — add after line 4 (`import { useApi } …`):

```tsx
import { useExplainPage } from '../explain/ExplainContext';
```

and after line 185 (`const d = useApi(…)`) add:

```tsx
  useExplainPage('episode', { detail: d.data });
```

`web/src/pages/Paper.tsx` — add after the `useEpochs` import:

```tsx
import { useExplainPage, useExplainSelect } from '../explain/ExplainContext';
```

in `PaperBody`, after `const now = useNow(30_000);` add `const select = useExplainSelect();`, and in the `awaiting resolution` fold change the positions table to:

```tsx
              <Table
                columns={positionColumns}
                rows={rows}
                rowKey={(p) => `${p.account_id}|${p.market_id}|${p.outcome}|${p.returned}`}
                onRowClick={(p) => select({ kind: 'position', position: p })}
              />
```

and in `Paper()`, after the `positions` line, add:

```tsx
  useExplainPage('paper', { accounts: accounts.data?.items, positions: positions.data?.items });
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §9.3: rows that set the explain selection. */
tr.pick { cursor: pointer; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/pages.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS (the page tests still render pages without a provider: publishing and selecting are no-ops there).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/test/appRoutes.ts src/explain/pages.test.tsx src/components/Table.tsx src/pages/Radar.tsx src/pages/Episodes.tsx src/pages/EpisodeDetail.tsx src/pages/Paper.tsx src/theme.css && git commit -m "web: pages publish what they fetched to the explain panel; Radar, Episodes and Paper rows set its selection"
```

---

### Task 10: the Radar and Episodes questions

Load `frontend-design:frontend-design` before writing the two drawings and their CSS.

**Files:**
- Create: `web/src/explain/models.ts`, `web/src/explain/copy.ts`, `web/src/schematics/PayoffGrid.tsx`, `web/src/schematics/DurationBars.tsx`
- Modify: `web/src/help.ts:1` (add `EPISODE_DEF` after the import), `:16` (`open.text` built from it)
- Modify: `web/src/explain/questions.tsx` (whole file, below)
- Modify: `web/src/theme.css` (append)
- Test: `web/src/explain/models.test.ts` (create), `web/src/explain/questions.test.tsx` (create)

**Interfaces:**
- Consumes: `ExplainState`, `useExplainPage`, `useExplainSelect` (Task 8); `EpisodeSummary`; `formatMoneyCents`, `fmtDuration`, `useNow`; `HELP.open` text.
- Produces:

```ts
// help.ts
export const EPISODE_DEF: string      // the first sentence of HELP.open.text, shared with the panel (spec §9.2)
// explain/models.ts
export const OTHER = 'an outcome not in the set'; export const PAYOFF_MAX = 6;
export type PayoffKind = 'yes' | 'no' | 'incomplete'; export function payoffKind(kind: string): PayoffKind | null
export interface PayoffModel { kind: 'yes' | 'no'; s: number; cols: string[]; rows: { name: string; cells: (0 | 1)[] }[]; basket: number[]; guaranteed: number; broken: boolean[] }
export function payoffStructure(kind: 'yes' | 'no', s: number, uncovered: boolean): PayoffModel
export const EXAMPLE_EPISODE: { s_legs: 3; updates_ms: readonly [0, 400, 1400, 2600, 4200]; open_ms: 400; peak_ms: 1400; close_ms: 4200 }
export interface DurationRow { kind: 'example' | 'row'; label: string; ms: number; open: boolean; ticks: number[] }
export function durationRows(row: EpisodeSummary | null, nowMs: number): DurationRow[]; export function durationText(r: DurationRow): string
// explain/copy.ts
export const money: (v: string | null) => string; export const COPY: { example; makeMoneyYes; makeMoneyNo; makeMoneyIncomplete; whatEpisode; episodeBar: string }
export const onTable100: (v: string | null) => string; export const onTableDepth: (v: string) => string
// schematics
export function PayoffGrid(props: { model: PayoffModel; label: string }): ReactElement    // HTML <table aria-label={label}>, ✗ on broken columns
export function DurationBars(props: { rows: DurationRow[] }): ReactElement                // svg per row (rect.db-bar, rect.db-tick) + HTML label
// questions.tsx: QUESTIONS = hero, then { id: 'make-money', pages: ['radar'] }, { id: 'what-episode', pages: ['episodes'] }
```

- [ ] **Step 1: Write the failing tests**

Create `web/src/explain/models.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture } from '../test/fixtures';
import { OTHER, durationRows, durationText, payoffKind, payoffStructure } from './models';

describe('payoff structure (parent spec §5.7 (b))', () => {
  it('a NO basket of three: each NO pays unless its own outcome wins; the basket pays S − 1 = 2 whoever wins', () => {
    const m = payoffStructure('no', 3, false);
    expect(m.cols).toEqual(['A', 'B', 'C']);
    expect(m.rows).toEqual([
      { name: 'NO A', cells: [0, 1, 1] },
      { name: 'NO B', cells: [1, 0, 1] },
      { name: 'NO C', cells: [1, 1, 0] },
    ]);
    expect(m.basket).toEqual([2, 2, 2]);
    expect(m.guaranteed).toBe(2);
    expect(m.broken).toEqual([false, false, false]);
  });

  it('an incomplete YES set: the uncovered column pays nothing and is broken', () => {
    const m = payoffStructure('yes', 3, true);
    expect(m.cols).toEqual(['A', 'B', 'C', OTHER]);
    expect(m.rows[0]).toEqual({ name: 'YES A', cells: [1, 0, 0, 0] });
    expect(m.basket).toEqual([1, 1, 1, 0]);
    expect(m.broken).toEqual([false, false, false, true]);
  });

  it('draws at most six outcomes but keeps the real S in the basket', () => {
    const yes = payoffStructure('yes', 12, false);
    expect([yes.rows.length, yes.cols.length, yes.s]).toEqual([6, 6, 12]);
    expect(yes.basket).toEqual([1, 1, 1, 1, 1, 1]);
    const no = payoffStructure('no', 12, false);
    expect([no.guaranteed, no.basket[0]]).toEqual([11, 11]);
  });

  it('maps episode kinds; implications have no basket drawing', () => {
    expect(['no_basket', 'yes_basket', 'yes_basket_incomplete', 'implication'].map(payoffKind)).toEqual(['no', 'yes', 'incomplete', null]);
  });
});

describe('duration bars', () => {
  it('the §5.7 (e) example runs from u2 to u5 with a tick per update; a closed row has its duration', () => {
    const [ex, row] = durationRows(episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 }), 0);
    expect(ex).toEqual({ kind: 'example', label: 'example', ms: 3800, open: false, ticks: [0, 1000, 2200, 3800] });
    expect(durationText(ex)).toBe('example: 3.80 s, from u2 to u5, peak at u3');
    expect(durationText(row)).toBe('Bitcoin price on September 26?: 4.19 s');
  });

  it('an open row runs to now; no row is the example alone', () => {
    const [, row] = durationRows(episodeSummaryFixture({ opened_ms: 1000, closed_ms: null, duration_ms: null }), 126_000);
    expect(row).toMatchObject({ ms: 125_000, open: true });
    expect(durationText(row)).toBe('Bitcoin price on September 26?: still open, 2 min 5 s so far');
    expect(durationRows(null, 0)).toHaveLength(1);
  });
});
```

Create `web/src/explain/questions.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ReactElement } from 'react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import type { EpisodeSummary } from '../api/types';
import { EPISODE_DEF } from '../help';
import { episodeSummaryFixture, ladderFixture } from '../test/fixtures';
import { mockApi } from '../test/mockApi';
import { COPY } from './copy';
import { Explain } from './Explain';
import { type ExplainPage, ExplainProvider, type PageData, type Selection, useExplainPage, useExplainSelect } from './ExplainContext';
import { OTHER } from './models';

function Pub<P extends ExplainPage>({ page, data, sel }: { page: P; data: NonNullable<PageData[P]>; sel?: NonNullable<Selection> }) {
  useExplainPage(page, data);
  const select = useExplainSelect();
  return sel ? (
    <button type="button" onClick={() => select(sel)}>
      pick
    </button>
  ) : null;
}

/** Render a page stand-in with the panel, select (when asked), open the panel and one question. */
function ask(node: ReactElement, q: string): HTMLElement {
  mockApi({ '/api/paper/ladder': ladderFixture() });
  render(
    <MemoryRouter>
      <ExplainProvider>
        {node}
        <Explain />
      </ExplainProvider>
    </MemoryRouter>,
  );
  const pick = screen.queryByText('pick');
  if (pick) fireEvent.click(pick);
  fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
  fireEvent.click(screen.getByRole('button', { name: q }));
  return screen.getByRole('complementary', { name: 'explain' });
}

// Stable identities, as a page's useApi data.
const TOP = { open: [episodeSummaryFixture()] };
const NONE = { open: [] as EpisodeSummary[] };
const UNMEASURED = { open: [episodeSummaryFixture({ net_budget100: null })] };
const LIST = { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 })] };
const YES_ROW = episodeSummaryFixture({ id: 'y', kind: 'yes_basket', s_legs: 3, event_title: 'Who wins the cup?', net_budget100: '1.2', net: '4.5' });
const INCOMPLETE = episodeSummaryFixture({ id: 'i', kind: 'yes_basket_incomplete', s_legs: 2, event_title: 'Best AI model?' });

describe('Radar: How can this make money?', () => {
  it('draws the top open row (a NO basket, S = 5) with its figures on the table', () => {
    const p = ask(<Pub page="radar" data={TOP} />, 'How can this make money?');
    const t = within(p).getByRole('table', { name: 'payoff of Bitcoin price on September 26?' });
    expect(within(t).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['pays $ if →', 'A', 'B', 'C', 'D', 'E']);
    expect(within(t).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['NO A', 'NO B', 'NO C', 'NO D', 'NO E', 'basket']);
    expect(within(p).getByText(COPY.makeMoneyNo)).toBeInTheDocument();
    expect(within(p).getByText('On the table at $100 spent: +$0.33, at the peak.')).toBeInTheDocument();
    expect(within(p).getByText('On the table at full depth: +$0.41.')).toBeInTheDocument();
  });

  it('uses the selected row: a YES basket', () => {
    const p = ask(<Pub page="radar" data={TOP} sel={{ kind: 'episode', episode: YES_ROW }} />, 'How can this make money?');
    expect(within(p).getByRole('table', { name: 'payoff of Who wins the cup?' })).toBeInTheDocument();
    expect(within(p).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['YES A', 'YES B', 'YES C', 'basket']);
    expect(within(p).getByText(COPY.makeMoneyYes)).toBeInTheDocument();
    expect(within(p).getByText('On the table at $100 spent: +$1.20, at the peak.')).toBeInTheDocument();
  });

  it('an incomplete set gets the ✗ column and no money figure', () => {
    const p = ask(<Pub page="radar" data={TOP} sel={{ kind: 'episode', episode: INCOMPLETE }} />, 'How can this make money?');
    expect(within(p).getByRole('columnheader', { name: `${OTHER} ✗` })).toBeInTheDocument();
    expect(within(p).getByText(COPY.makeMoneyIncomplete)).toBeInTheDocument();
    expect(within(p).queryByText(/On the table at/)).toBeNull();
  });

  it('an unmeasured figure reads "not measured", never $0.00', () => {
    const p = ask(<Pub page="radar" data={UNMEASURED} />, 'How can this make money?');
    expect(within(p).getByText('On the table at $100 spent: not measured, at the peak.')).toBeInTheDocument();
    expect(p).not.toHaveTextContent('$0.00');
  });

  it('with nothing open, a labelled example and no money figure', () => {
    const p = ask(<Pub page="radar" data={NONE} />, 'How can this make money?');
    expect(within(p).getByText(COPY.example)).toBeInTheDocument();
    expect(within(p).getByRole('table', { name: 'payoff of the example basket' })).toBeInTheDocument();
    expect(within(p).queryByText(/On the table at/)).toBeNull();
  });

  it('is asked on Radar only', () => {
    ask(<Pub page="episodes" data={LIST} />, 'What is an episode?');
    expect(screen.queryByRole('button', { name: 'How can this make money?' })).toBeNull();
  });
});

describe('Episodes: What is an episode?', () => {
  it('draws the first row next to the labelled §5.7 (e) example', () => {
    const p = ask(<Pub page="episodes" data={LIST} />, 'What is an episode?');
    expect(within(p).getByText('example: 3.80 s, from u2 to u5, peak at u3')).toBeInTheDocument();
    expect(within(p).getByText('Bitcoin price on September 26?: 4.19 s')).toBeInTheDocument();
    expect(within(p).getByText(EPISODE_DEF)).toBeInTheDocument();
    expect(p.querySelectorAll('rect.db-tick')).toHaveLength(4);
    expect(p.querySelectorAll('rect.db-bar')).toHaveLength(2);
  });

  it('a selected open row says it is still open', () => {
    const open = episodeSummaryFixture({ id: 'o', event_title: 'Still open one', opened_ms: Date.now() - 125_000, closed_ms: null, duration_ms: null });
    const p = ask(<Pub page="episodes" data={LIST} sel={{ kind: 'episode', episode: open }} />, 'What is an episode?');
    expect(within(p).getByText(/^Still open one: still open, 2 min \d+ s so far$/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/models.test.ts src/explain/questions.test.tsx`
Expected: FAIL — `Failed to resolve import "./models"` and `"./copy"`.

- [ ] **Step 3: Implement**

`web/src/help.ts` — after line 1 (`import { FLAG_WORDS } …`) add:

```ts

/** What an episode is, in one sentence: the Radar (?) and the explain panel say it the same way. */
export const EPISODE_DEF =
  'One stretch of time in which buying a full basket of one event (every YES, or every NO) cost less than it pays out, after taker fees.';
```

and replace the `text:` line of `open` (original line 16) with:

```ts
    text: `${EPISODE_DEF} It opens on the first order-book update where that is true and closes on the first where it is not. Open ones sit here while they last, which is usually not long.`,
```

Create `web/src/explain/models.ts`:

```ts
// Panel-side models (09-27 §2): structure only; every money figure shown comes from the api.
import type { EpisodeSummary } from '../api/types';
import { fmtDuration } from '../lib/time';

/** The ✗ column of an incomplete YES set: EpisodeSummary does not say which outcome is uncovered. */
export const OTHER = 'an outcome not in the set';
/** The panel draws at most six outcomes; a caption says when the basket has more. */
export const PAYOFF_MAX = 6;
const NAMES = ['A', 'B', 'C', 'D', 'E', 'F'];

export type PayoffKind = 'yes' | 'no' | 'incomplete';

export function payoffKind(kind: string): PayoffKind | null {
  if (kind === 'yes_basket') return 'yes';
  if (kind === 'no_basket') return 'no';
  if (kind === 'yes_basket_incomplete') return 'incomplete';
  return null;
}

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

/** Parent spec §5.7 (b): who pays in which outcome. Counts, not money. */
export function payoffStructure(kind: 'yes' | 'no', s: number, uncovered: boolean): PayoffModel {
  const all = Math.max(2, s);
  const outcomes = NAMES.slice(0, Math.min(PAYOFF_MAX, all));
  const cols = kind === 'yes' && uncovered ? [...outcomes, OTHER] : outcomes;
  const rows = outcomes.map((o, i) => ({
    name: `${kind.toUpperCase()} ${o}`,
    cells: cols.map((_, c): 0 | 1 => (kind === 'yes' ? (c === i ? 1 : 0) : c === i ? 0 : 1)),
  }));
  const guaranteed = kind === 'yes' ? 1 : all - 1;
  const basket = cols.map((c) => (c === OTHER ? 0 : guaranteed));
  return { kind, s: all, cols, rows, basket, guaranteed, broken: basket.map((b) => b < guaranteed) };
}

/** Parent spec §5.7 (e): five updates, the episode from u2 to u5, the peak at u3. The times are illustrative. */
export const EXAMPLE_EPISODE = { s_legs: 3, updates_ms: [0, 400, 1400, 2600, 4200], open_ms: 400, peak_ms: 1400, close_ms: 4200 } as const;

export interface DurationRow {
  kind: 'example' | 'row';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open (the example only). */
  ticks: number[];
}

/** Spec 7a §9.2: a duration bar for the row, next to the example labelled example. */
export function durationRows(row: EpisodeSummary | null, nowMs: number): DurationRow[] {
  const e = EXAMPLE_EPISODE;
  const example: DurationRow = {
    kind: 'example',
    label: 'example',
    ms: e.close_ms - e.open_ms,
    open: false,
    ticks: e.updates_ms.filter((t) => t >= e.open_ms).map((t) => t - e.open_ms),
  };
  if (row === null) return [example];
  return [
    example,
    { kind: 'row', label: row.event_title ?? row.id, ms: row.duration_ms ?? Math.max(0, nowMs - row.opened_ms), open: row.duration_ms === null, ticks: [] },
  ];
}

export function durationText(r: DurationRow): string {
  if (r.kind === 'example') return `example: ${fmtDuration(r.ms)}, from u2 to u5, peak at u3`;
  return r.open ? `${r.label}: still open, ${fmtDuration(r.ms)} so far` : `${r.label}: ${fmtDuration(r.ms)}`;
}
```

Create `web/src/explain/copy.ts`:

```ts
// Spec 7a §9.2/§9.4: every answer is one drawing plus at most two sentences; every money figure is
// labelled "on the table", "paper" or "captured at N ms"; no real-money words.
import { EPISODE_DEF } from '../help';
import { formatMoneyCents } from '../lib/money';

/** A money figure inside a sentence, or "not measured" (a missing figure never reads $0.00). */
export const money = (v: string | null): string => (v === null ? 'not measured' : formatMoneyCents(v, { signed: true }));

export const COPY = {
  example: 'example: a NO basket with three outcomes, no money figure',
  makeMoneyYes: 'Exactly one outcome wins, so one YES of each pays $1 whoever wins. Bought for less than $1, the gap is on the table until resolution.',
  makeMoneyNo: 'One NO of each outcome converts back to S − 1 dollars at once, whoever wins. Bought for less than that, the gap is on the table now.',
  makeMoneyIncomplete: 'The set does not cover every result: if an outcome outside it wins (✗), the basket pays nothing. The gap is real, the guarantee is not.',
  whatEpisode: EPISODE_DEF,
  episodeBar: 'The bar is how long the edge stayed on the table.',
};

export const onTable100 = (v: string | null): string => `On the table at $100 spent: ${money(v)}, at the peak.`;
export const onTableDepth = (v: string): string => `On the table at full depth: ${money(v)}.`;
```

Create `web/src/schematics/PayoffGrid.tsx`:

```tsx
import type { PayoffModel } from '../explain/models';

/** Parent spec §5.7 (b) as an HTML table (no SVG, nothing to distort): 1 where a token pays $1, the
 *  basket row, and ✗ on a column where the basket pays less than it guarantees. */
export function PayoffGrid({ model, label }: { model: PayoffModel; label: string }) {
  return (
    <figure className="payoff">
      <div className="scroll">
        <table aria-label={label}>
          <thead>
            <tr>
              <th scope="col">pays $ if →</th>
              {model.cols.map((c, i) => (
                <th key={c} scope="col" className={model.broken[i] ? 'broken' : undefined}>
                  {model.broken[i] ? `${c} ✗` : c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {model.rows.map((r) => (
              <tr key={r.name}>
                <th scope="row">{r.name}</th>
                {r.cells.map((v, i) => (
                  <td key={model.cols[i]} className={v === 1 ? 'pay' : 'nopay'}>{v}</td>
                ))}
              </tr>
            ))}
            <tr className="basket">
              <th scope="row">basket</th>
              {model.basket.map((b, i) => (
                <td key={model.cols[i]} className={model.broken[i] ? 'broken' : 'pay'}>{b}</td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
      {model.s > model.rows.length && <figcaption className="dim">{`first ${model.rows.length} of ${model.s} outcomes shown`}</figcaption>}
    </figure>
  );
}
```

Create `web/src/schematics/DurationBars.tsx`:

```tsx
import { type DurationRow, durationText } from '../explain/models';

const W = 300;
const H = 10;

/** Spec 7a §9.2: each episode as a bar on one shared time scale, the example's updates as ticks.
 *  Labels are HTML under each track (the SVG stretches: preserveAspectRatio="none"). */
export function DurationBars({ rows }: { rows: DurationRow[] }) {
  const max = Math.max(1, ...rows.map((r) => r.ms));
  const sx = (ms: number) => (ms / max) * W;
  return (
    <figure className="dbars" aria-label="how long each stayed on the table">
      {rows.map((r) => (
        <div key={`${r.kind}|${r.label}`} className="db-row">
          <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
            <rect
              className={['db-bar', r.kind === 'example' ? 'example' : null, r.open ? 'open' : null].filter(Boolean).join(' ')}
              x={0}
              y={2}
              width={Math.max(1, sx(r.ms))}
              height={H - 4}
            />
            {r.ticks.map((t) => (
              <rect key={t} className="db-tick" x={Math.min(W - 1, sx(t))} y={0} width={1} height={H} />
            ))}
          </svg>
          <p className="db-label">{durationText(r)}</p>
        </div>
      ))}
    </figure>
  );
}
```

Replace `web/src/explain/questions.tsx` with:

```tsx
import type { ReactElement } from 'react';
import type { EpisodeSummary, Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { useNow } from '../lib/hooks';
import { ladderModel } from '../lib/ladder';
import { DurationBars } from '../schematics/DurationBars';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { COPY, onTable100, onTableDepth } from './copy';
import type { ExplainPage, ExplainState } from './ExplainContext';
import { EXAMPLE_EPISODE, durationRows, payoffKind, payoffStructure } from './models';

export interface AnswerProps {
  s: ExplainState;
  /** The panel's one fetch (spec §9.3); every answer reuses it, none fetches. */
  ladder: ApiState<Ladder>;
}

export interface Question {
  id: string;
  q: string;
  pages: 'all' | ExplainPage[];
  Answer: (p: AnswerProps) => ReactElement;
}

export const HERO_ID = 'latency';

function Hero({ ladder }: AnswerProps) {
  return <Loadable state={ladder} what="the ladder">{(l) => <LatencyLadder model={ladderModel(l)} />}</Loadable>;
}

/** The selected row, or else the top open arbitrage row (complete YES or NO basket). */
function radarEpisode(s: ExplainState): EpisodeSummary | null {
  if (s.selection?.kind === 'episode') return s.selection.episode;
  return (
    s.data.radar?.open?.find((e) => {
      const k = payoffKind(e.kind);
      return k === 'yes' || k === 'no';
    }) ?? null
  );
}

function MakeMoney({ s }: AnswerProps) {
  const e = radarEpisode(s);
  const kind = e === null ? null : payoffKind(e.kind);
  if (e === null || kind === null) {
    return (
      <>
        <p className="dim">{COPY.example}</p>
        <PayoffGrid model={payoffStructure('no', EXAMPLE_EPISODE.s_legs, false)} label="payoff of the example basket" />
        <p>{COPY.makeMoneyNo}</p>
      </>
    );
  }
  return (
    <>
      <PayoffGrid model={payoffStructure(kind === 'no' ? 'no' : 'yes', e.s_legs, kind === 'incomplete')} label={`payoff of ${e.event_title ?? e.id}`} />
      <p>{kind === 'no' ? COPY.makeMoneyNo : kind === 'yes' ? COPY.makeMoneyYes : COPY.makeMoneyIncomplete}</p>
      {kind !== 'incomplete' && <p>{onTable100(e.net_budget100)}</p>}
      {kind !== 'incomplete' && <p>{onTableDepth(e.net)}</p>}
    </>
  );
}

function WhatEpisode({ s }: AnswerProps) {
  const now = useNow(1_000);
  const row = s.selection?.kind === 'episode' ? s.selection.episode : (s.data.episodes?.items?.[0] ?? null);
  return (
    <>
      <DurationBars rows={durationRows(row, now)} />
      <p>{COPY.whatEpisode}</p>
      <p>{COPY.episodeBar}</p>
    </>
  );
}

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [
  { id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero },
  { id: 'make-money', q: 'How can this make money?', pages: ['radar'], Answer: MakeMoney },
  { id: 'what-episode', q: 'What is an episode?', pages: ['episodes'], Answer: WhatEpisode },
];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §9.2: payoff structure (b) and the episode duration bars. */
.payoff { margin: 0; }
.payoff table { width: auto; }
.payoff td { text-align: center; min-width: 2ch; }
.payoff td.nopay { color: var(--dim); }
.payoff .broken { color: var(--salmon); }
.payoff tr.basket th, .payoff tr.basket td { border-top: 2px solid var(--line); font-weight: 600; }
.dbars { display: grid; gap: 4px; margin: 0; }
.dbars svg { display: block; width: 100%; height: 14px; background: var(--bg); }
.dbars .db-bar { fill: var(--teal); }
.dbars .db-bar.example { fill: var(--line2); }
.dbars .db-bar.open { fill: var(--rose); }
.dbars .db-tick { fill: var(--fg); }
.db-label { font-size: 13px; color: var(--dim); }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/models.test.ts src/explain/questions.test.tsx src/pages/Episodes.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS (Episodes.test still finds `HELP.open.text`, unchanged in content).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/help.ts src/explain/models.ts src/explain/models.test.ts src/explain/copy.ts src/explain/questions.tsx src/explain/questions.test.tsx src/schematics/PayoffGrid.tsx src/schematics/DurationBars.tsx src/theme.css && git commit -m "web: explain Radar (payoff structure of the selected or top row, figures on the table) and Episodes (the row's bar next to the example)"
```

---

### Task 11: the Episode detail and Paper questions

Load `frontend-design:frontend-design` before writing the two drawings and their CSS.

**Files:**
- Create: `web/src/schematics/CaughtBar.tsx`, `web/src/schematics/LockedBlocks.tsx`
- Modify: `web/src/explain/models.ts` (append), `web/src/explain/copy.ts` (four `COPY` keys, two functions), `web/src/explain/questions.tsx` (whole file, below)
- Modify: `web/src/theme.css` (append)
- Test: `web/src/explain/models.test.ts` (append), `web/src/explain/questions.test.tsx` (append; two imports)

**Interfaces:**
- Consumes: `LADDER`, `latencyX`, `slotX`, `usBand` (Task 2); `SweetLadderGrid` (Task 3); `Position.end_date` (Task 1); `shortId`, `fmtDuration`, `DAY_MS`, `Money`, `useNow`.
- Produces:

```ts
// explain/models.ts
export const positionKey: (p: Position) => string                   // `${account_id}|${market_id}|${outcome}`
export function daysText(endDate: string | null, nowMs: number): string
//   'resolution date unknown' | 'past its end date, awaiting resolution' | 'resolves in 1 day' | 'resolves in N days'
// explain/copy.ts
COPY.caughtNotFilled, COPY.caughtWaiting, COPY.locked, COPY.nothingLocked: string
export function caughtText(durationMs: number | null): string; export function caughtCategory(category: string): string
// schematics
export function inTime(latencyMs: number, durationMs: number | null): boolean
export function CaughtBar(props: { durationMs: number | null; measured: { latency_ms: number } | null }): ReactElement
//   svg: rect.cb-dur, rect.cb-us, rect.cb-rung.in|.late[data-scenario]; <ol aria-label="rungs in time"> (span ms, span "in time"|"too late")
export function LockedBlocks(props: { positions: Position[]; selected: string | null; nowMs: number }): ReactElement
//   .lk-row of span.lk-block (flex-grow ∝ basis, .sel), <ul aria-label="locked positions">
// questions.tsx: + { id: 'caught', pages: ['episode'] }, { id: 'locked', pages: ['paper'] }
```

- [ ] **Step 1: Write the failing tests**

Append to `web/src/explain/models.test.ts` (and add `daysText, positionKey` to its import from `./models`, `positionFixture` to its fixtures import):

```ts
describe('days to resolution (spec 7a §9.2, Position.end_date)', () => {
  it('counts whole days up, and says when it is unknown or past', () => {
    expect(daysText('2026-10-31', Date.UTC(2026, 9, 29))).toBe('resolves in 2 days');
    expect(daysText('2026-10-30T00:00:00Z', Date.UTC(2026, 9, 29, 12))).toBe('resolves in 1 day');
    expect(daysText('2026-10-01', Date.UTC(2026, 9, 2))).toBe('past its end date, awaiting resolution');
    expect(daysText(null, 0)).toBe('resolution date unknown');
    expect(daysText('soon', 0)).toBe('resolution date unknown');
    expect(positionKey(positionFixture())).toBe('P3-naive/sp-idle|m1|yes');
  });
});
```

In `web/src/explain/questions.test.tsx` change the fixtures import to `import { accountFixture, episodeDetailFixture, episodeSummaryFixture, ladderFixture, positionFixture } from '../test/fixtures';` and append:

```tsx
const DETAIL = { detail: episodeDetailFixture({ category: 'crypto' }) }; // a NO basket that lasted 4.19 s
const FAST = { detail: episodeDetailFixture({ duration_ms: 60, closed_ms: Date.UTC(2026, 8, 25, 1, 12, 4, 270) }) };
const POSITIONS = {
  accounts: [accountFixture()],
  positions: [
    positionFixture(),
    positionFixture({ market_id: 'm2', end_date: null, event_title: 'Who wins the cup?' }),
    positionFixture({ market_id: 'm3', returned: true, event_title: 'Returned one' }),
  ],
};
const NOTHING = { accounts: [accountFixture()], positions: [] };

describe('Episode detail: Would we have caught it?', () => {
  it('puts its duration on the ladder axis: every rung faster than it arrives in time; its category by rung', async () => {
    const p = ask(<Pub page="episode" data={DETAIL} />, 'Would we have caught it?');
    const axis = within(p).getByRole('list', { name: 'rungs in time' });
    expect(within(axis).getAllByRole('listitem').map((li) => li.lastChild?.textContent)).toEqual(Array(6).fill('in time'));
    expect(within(p).getByText('It stayed on the table for 4.19 s: rungs faster than that arrive in time.')).toBeInTheDocument();
    expect(within(p).getByText(COPY.caughtNotFilled)).toBeInTheDocument();
    expect(await within(p).findByRole('table', { name: 'crypto by rung' })).toBeInTheDocument();
    expect(within(p).getAllByRole('rowheader').map((h) => h.textContent)).toEqual(['all', 'crypto']);
  });

  it('a 60 ms episode: the rungs at 100 ms and beyond are too late', () => {
    const p = ask(<Pub page="episode" data={FAST} />, 'Would we have caught it?');
    const axis = within(p).getByRole('list', { name: 'rungs in time' });
    expect(within(axis).getAllByRole('listitem').map((li) => li.lastChild?.textContent)).toEqual([
      'in time', 'in time', 'in time', 'too late', 'too late', 'too late',
    ]);
    expect(p.querySelectorAll('rect.cb-rung.late')).toHaveLength(3);
    expect(within(p).getByText('It stayed on the table for 0.06 s: rungs faster than that arrive in time.')).toBeInTheDocument();
  });
});

describe('Paper: Why is money locked?', () => {
  it('draws open positions as blocks sized by basis, each with days to resolution; returned ones are not locked', () => {
    const p = ask(<Pub page="paper" data={POSITIONS} />, 'Why is money locked?');
    const items = within(within(p).getByRole('list', { name: 'locked positions' })).getAllByRole('listitem').map((li) => li.textContent);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatch(/^Fed decision in October\? · P3 · \$18\.00 locked on paper · /);
    expect(items[1]).toBe('Who wins the cup? · P3 · $18.00 locked on paper · resolution date unknown');
    expect(p.querySelectorAll('.lk-block')).toHaveLength(2);
    expect(within(p).queryByText(/Returned one/)).toBeNull();
    expect(within(p).getByText(COPY.locked)).toBeInTheDocument();
  });

  it('highlights the selected position', () => {
    const p = ask(<Pub page="paper" data={POSITIONS} sel={{ kind: 'position', position: POSITIONS.positions[1] }} />, 'Why is money locked?');
    expect(p.querySelectorAll('.lk-block.sel')).toHaveLength(1);
    expect(within(p).getByText(/^Who wins the cup\?/).closest('li')).toHaveClass('sel');
  });

  it('says so when nothing is locked', () => {
    const p = ask(<Pub page="paper" data={NOTHING} />, 'Why is money locked?');
    expect(within(p).getByText(COPY.nothingLocked)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/models.test.ts src/explain/questions.test.tsx`
Expected: FAIL — `daysText is not a function`; `Unable to find role="button" and name "Would we have caught it?"`.

- [ ] **Step 3: Implement**

Append to `web/src/explain/models.ts` (and change its type import to `import type { EpisodeSummary, Position } from '../api/types';` and its time import to `import { DAY_MS, fmtDuration } from '../lib/time';`):

```ts
export const positionKey = (p: Position): string => `${p.account_id}|${p.market_id}|${p.outcome}`;

/** 09-27 §7.4: days to resolution from `Position.end_date` (stored text), or "unknown". */
export function daysText(endDate: string | null, nowMs: number): string {
  if (endDate === null) return 'resolution date unknown';
  const t = Date.parse(endDate);
  if (Number.isNaN(t)) return 'resolution date unknown';
  const d = Math.ceil((t - nowMs) / DAY_MS);
  if (d <= 0) return 'past its end date, awaiting resolution';
  return d === 1 ? 'resolves in 1 day' : `resolves in ${d} days`;
}
```

In `web/src/explain/copy.ts` add to the `COPY` object (after `episodeBar`):

```ts
  caughtNotFilled: 'In time is not filled: the first question shows what arriving in time was worth on paper.',
  caughtWaiting: 'Waiting for the episode to load.',
  locked: 'A YES basket pays only at resolution, so its cost stays locked on paper until the event ends. NO baskets convert at once and lock nothing.',
  nothingLocked: 'Nothing is locked: no paper position is waiting for a resolution.',
```

and append (with `import { fmtDuration } from '../lib/time';` added to its imports):

```ts
export function caughtText(durationMs: number | null): string {
  return durationMs === null
    ? 'Still open: every rung would have arrived while it was on the table so far.'
    : `It stayed on the table for ${fmtDuration(durationMs)}: rungs faster than that arrive in time.`;
}

/** Spec §17: the markets rows are Sweet spots' time frame, not the epoch's. */
export const caughtCategory = (category: string): string => `${category}, per trade attempt over the last 30 days (NO side, across epochs):`;
```

Create `web/src/schematics/CaughtBar.tsx`:

```tsx
import { LADDER, latencyX, slotX, usBand } from '../lib/ladder';
import { fmtDuration } from '../lib/time';

const W = 600;
const H = 30;
const N = LADDER.length;

/** A rung arrives in time when its latency is below the episode's duration; an open episode, so far, always. */
export function inTime(latencyMs: number, durationMs: number | null): boolean {
  return durationMs === null || latencyMs < durationMs;
}

/** Spec 7a §9.2: the episode's duration as a bar on the ladder's ordinal axis (six rungs, no extras slot);
 *  rungs left of its end are in time. Labels are HTML under the drawing. */
export function CaughtBar({ durationMs, measured }: { durationMs: number | null; measured: { latency_ms: number } | null }) {
  const end = durationMs === null ? 1 : latencyX(durationMs, N);
  const band = measured ? usBand(measured, N) : null;
  const last = LADDER[N - 1].latency_ms;
  return (
    <figure className="cbar" aria-label="would we have caught it">
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
        {band && <rect className="cb-us" x={band.x0 * W} y={0} width={Math.max(1, (band.x1 - band.x0) * W)} height={H} />}
        <rect className="cb-dur" x={0} y={H / 2 - 4} width={Math.max(1, end * W)} height={8} />
        {LADDER.map((l, i) => (
          <rect
            key={l.scenario}
            className={inTime(l.latency_ms, durationMs) ? 'cb-rung in' : 'cb-rung late'}
            data-scenario={l.scenario}
            x={slotX(i, N) * W - 4}
            y={4}
            width={8}
            height={H - 8}
          />
        ))}
      </svg>
      <ol className="cb-axis" aria-label="rungs in time">
        {LADDER.map((l) => {
          const ok = inTime(l.latency_ms, durationMs);
          return (
            <li key={l.scenario} className={ok ? undefined : 'late'} title={l.scenario}>
              <span className="num">{l.latency_ms}</span>
              <span>{ok ? 'in time' : 'too late'}</span>
            </li>
          );
        })}
      </ol>
      {durationMs !== null && durationMs > last && (
        <figcaption className="dim">{`ms on the axis; the bar runs past the last rung: ${fmtDuration(durationMs)}`}</figcaption>
      )}
    </figure>
  );
}
```

Create `web/src/schematics/LockedBlocks.tsx`:

```tsx
import type { Position } from '../api/types';
import { Money } from '../components/Money';
import { daysText, positionKey } from '../explain/models';
import { shortId } from '../lib/labels';

/** Spec 7a §9.2: open positions as blocks sized by basis (their cost; HTML flex, geometry only), each
 *  listed with its paper basis and days to resolution. */
export function LockedBlocks({ positions, selected, nowMs }: { positions: Position[]; selected: string | null; nowMs: number }) {
  const sizes = positions.map((p) => {
    const n = Number(p.basis);
    return Number.isFinite(n) && n > 0 ? n : 0;
  });
  const total = sizes.reduce((a, b) => a + b, 0);
  return (
    <figure className="locked" aria-label="money locked until resolution">
      <div className="lk-row">
        {positions.map((p, i) => (
          <span
            key={positionKey(p)}
            className={positionKey(p) === selected ? 'lk-block sel' : 'lk-block'}
            style={{ flexGrow: total > 0 ? sizes[i] / total : 1 }}
            title={p.event_title ?? p.market_id}
          />
        ))}
      </div>
      <ul className="plain lk-list" aria-label="locked positions">
        {positions.map((p) => (
          <li key={positionKey(p)} className={positionKey(p) === selected ? 'sel' : undefined}>
            {p.event_title ?? shortId(p.market_id)} · {p.account_id.split('-')[0]} · <Money value={p.basis} cents /> locked on paper · {daysText(p.end_date, nowMs)}
          </li>
        ))}
      </ul>
    </figure>
  );
}
```

Replace `web/src/explain/questions.tsx` with:

```tsx
import type { ReactElement } from 'react';
import type { EpisodeSummary, Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import { useNow } from '../lib/hooks';
import { ladderModel } from '../lib/ladder';
import { CaughtBar } from '../schematics/CaughtBar';
import { DurationBars } from '../schematics/DurationBars';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { LockedBlocks } from '../schematics/LockedBlocks';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { COPY, caughtCategory, caughtText, onTable100, onTableDepth } from './copy';
import type { ExplainPage, ExplainState } from './ExplainContext';
import { EXAMPLE_EPISODE, durationRows, payoffKind, payoffStructure, positionKey } from './models';

export interface AnswerProps {
  s: ExplainState;
  /** The panel's one fetch (spec §9.3); every answer reuses it, none fetches. */
  ladder: ApiState<Ladder>;
}

export interface Question {
  id: string;
  q: string;
  pages: 'all' | ExplainPage[];
  Answer: (p: AnswerProps) => ReactElement;
}

export const HERO_ID = 'latency';

function Hero({ ladder }: AnswerProps) {
  return <Loadable state={ladder} what="the ladder">{(l) => <LatencyLadder model={ladderModel(l)} />}</Loadable>;
}

/** The selected row, or else the top open arbitrage row (complete YES or NO basket). */
function radarEpisode(s: ExplainState): EpisodeSummary | null {
  if (s.selection?.kind === 'episode') return s.selection.episode;
  return (
    s.data.radar?.open?.find((e) => {
      const k = payoffKind(e.kind);
      return k === 'yes' || k === 'no';
    }) ?? null
  );
}

function MakeMoney({ s }: AnswerProps) {
  const e = radarEpisode(s);
  const kind = e === null ? null : payoffKind(e.kind);
  if (e === null || kind === null) {
    return (
      <>
        <p className="dim">{COPY.example}</p>
        <PayoffGrid model={payoffStructure('no', EXAMPLE_EPISODE.s_legs, false)} label="payoff of the example basket" />
        <p>{COPY.makeMoneyNo}</p>
      </>
    );
  }
  return (
    <>
      <PayoffGrid model={payoffStructure(kind === 'no' ? 'no' : 'yes', e.s_legs, kind === 'incomplete')} label={`payoff of ${e.event_title ?? e.id}`} />
      <p>{kind === 'no' ? COPY.makeMoneyNo : kind === 'yes' ? COPY.makeMoneyYes : COPY.makeMoneyIncomplete}</p>
      {kind !== 'incomplete' && <p>{onTable100(e.net_budget100)}</p>}
      {kind !== 'incomplete' && <p>{onTableDepth(e.net)}</p>}
    </>
  );
}

function WhatEpisode({ s }: AnswerProps) {
  const now = useNow(1_000);
  const row = s.selection?.kind === 'episode' ? s.selection.episode : (s.data.episodes?.items?.[0] ?? null);
  return (
    <>
      <DurationBars rows={durationRows(row, now)} />
      <p>{COPY.whatEpisode}</p>
      <p>{COPY.episodeBar}</p>
    </>
  );
}

/** Spec 7a §9.2/§9.3: the episode's duration on the ladder axis, plus its category's row from the SAME
 *  ladder response (`markets`), never a second fetch. */
function Caught({ s, ladder }: AnswerProps) {
  const e = s.data.episode?.detail?.episode;
  if (!e) return <p className="dim">{COPY.caughtWaiting}</p>;
  const l = ladder.data;
  const markets =
    l && e.category !== null && payoffKind(e.kind) === 'no' && (l.markets_status === 'live' || l.markets_status === 'warming')
      ? { rows: l.markets, category: e.category }
      : null;
  return (
    <>
      <CaughtBar durationMs={e.duration_ms} measured={l?.measured ?? null} />
      <p>{caughtText(e.duration_ms)}</p>
      <p className="dim">{COPY.caughtNotFilled}</p>
      {markets && (
        <>
          <p className="dim">{caughtCategory(markets.category)}</p>
          <SweetLadderGrid rows={markets.rows} side="no" dim="category" only={[markets.category]} label={`${markets.category} by rung`} />
        </>
      )}
    </>
  );
}

function Locked({ s }: AnswerProps) {
  const now = useNow(60_000);
  const sel = s.selection?.kind === 'position' ? s.selection.position : null;
  const all = s.data.paper?.positions ?? [];
  // A selected position that has left the list since stays drawn (the selection is a snapshot).
  const list = sel && !all.some((p) => positionKey(p) === positionKey(sel)) ? [sel, ...all] : all;
  const open = list.filter((p) => !p.returned);
  return (
    <>
      {open.length === 0 ? <p>{COPY.nothingLocked}</p> : <LockedBlocks positions={open} selected={sel ? positionKey(sel) : null} nowMs={now} />}
      <p>{COPY.locked}</p>
    </>
  );
}

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [
  { id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero },
  { id: 'make-money', q: 'How can this make money?', pages: ['radar'], Answer: MakeMoney },
  { id: 'caught', q: 'Would we have caught it?', pages: ['episode'], Answer: Caught },
  { id: 'what-episode', q: 'What is an episode?', pages: ['episodes'], Answer: WhatEpisode },
  { id: 'locked', q: 'Why is money locked?', pages: ['paper'], Answer: Locked },
];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
```

Append to `web/src/theme.css`:

```css
/* Spec 7a §9.2: would we have caught it, and why money is locked. */
.cbar { display: grid; gap: 2px; margin: 0; }
.cbar svg { display: block; width: 100%; height: 36px; background: var(--bg); border: 2px solid var(--line2); }
.cbar .cb-dur { fill: var(--teal); }
.cbar .cb-us { fill: var(--rose); opacity: 0.35; }
.cbar .cb-rung.in { fill: var(--fg); }
.cbar .cb-rung.late { fill: var(--bg); stroke: var(--salmon); stroke-width: 1px; vector-effect: non-scaling-stroke; }
.cb-axis { list-style: none; display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); text-align: center; font-size: 13px; line-height: 1.3; }
.cb-axis li { display: grid; min-width: 0; }
.cb-axis li.late { color: var(--dim); }
.locked { display: grid; gap: 6px; margin: 0; }
.lk-row { display: flex; gap: 2px; height: 28px; }
.lk-block { flex: 1 1 0; min-width: 4px; background: var(--rose); }
.lk-block.sel { background: var(--salmon); }
.lk-list { gap: 2px; font-size: 13px; }
.lk-list li.sel { font-weight: 600; }
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/models.test.ts src/explain/questions.test.tsx && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/explain/models.ts src/explain/models.test.ts src/explain/copy.ts src/explain/questions.tsx src/explain/questions.test.tsx src/schematics/CaughtBar.tsx src/schematics/LockedBlocks.tsx src/theme.css && git commit -m "web: explain Episode detail (its duration on the ladder axis, its category by rung) and Paper (locked positions sized by basis, days to resolution)"
```

---

### Task 12: the honesty scan and the fetch rule on every page

**Files:**
- Create: `web/src/explain/honesty.ts`, `web/src/test/settle.ts`
- Test: `web/src/explain/honesty.test.tsx` (create), `web/src/explain/fetchRule.test.tsx` (create)

**Interfaces:**
- Consumes: `App` (Task 8), `appRoutes` (Task 9), every copy constant of Tasks 2–11.
- Produces:

```ts
// explain/honesty.ts
export function checkHonesty(text: string): string[]            // offending words, lower case, in text order; [] = honest
export function unlabelledMoney(root: HTMLElement): string[]      // texts of blocks (li, p, table, figure, dd) holding "$<digit>" with no paper / on the table / captured at N ms label
// test/settle.ts
export function settle(f: { mock: { calls: unknown[] } }): Promise<void>   // flush (fake timers) or wait (real) until fetch is quiet 3 rounds
```

- [ ] **Step 1: Write the failing tests**

Create `web/src/test/settle.ts`:

```ts
import { act } from '@testing-library/react';
import { vi } from 'vitest';

/** Let pending fetches land: flush with fake timers, or wait 10 ms with real ones, until the fetch mock
 *  has been quiet for three rounds (pages chain fetches: accounts, then one equity per account). */
export async function settle(f: { mock: { calls: unknown[] } }): Promise<void> {
  let quiet = 0;
  let n = f.mock.calls.length;
  for (let i = 0; i < 100 && quiet < 3; i++) {
    await act(async () => {
      if (vi.isFakeTimers()) await vi.advanceTimersByTimeAsync(0);
      else await new Promise((r) => setTimeout(r, 10));
    });
    if (f.mock.calls.length === n) quiet += 1;
    else {
      quiet = 0;
      n = f.mock.calls.length;
    }
  }
}
```

Create `web/src/explain/fetchRule.test.tsx`:

```tsx
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { appRoutes } from '../test/appRoutes';
import { mockApi } from '../test/mockApi';
import { settle } from '../test/settle';

const PATHS = ['/', '/sweet', '/episodes', '/episodes/live-1-00000001', '/paper', '/papers', '/health', '/nowhere'];
const pathOf = (c: unknown[]) => new URL(String(c[0]), 'http://localhost').pathname;

describe('the explain panel fetch rule (spec 7a §9.3)', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  for (const p of PATHS) {
    it(`${p}: with ticks and SSE frozen, opening adds exactly one call, to /api/paper/ladder; opening every question adds none`, async () => {
      vi.useFakeTimers(); // no ticker fires; there is no StreamProvider, so no EventSource either
      const f = mockApi(appRoutes());
      render(
        <MemoryRouter initialEntries={[p]}>
          <App />
        </MemoryRouter>,
      );
      await settle(f);
      const before = f.mock.calls.length;
      fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
      await settle(f);
      expect(f.mock.calls.slice(before).map(pathOf)).toEqual(['/api/paper/ladder']);
      const panel = screen.getByRole('complementary', { name: 'explain' });
      for (const b of [...panel.querySelectorAll<HTMLButtonElement>('button.explain-q')]) {
        fireEvent.click(b);
        await settle(f);
      }
      expect(f.mock.calls.length).toBe(before + 1);
    });
  }
});
```

Create `web/src/explain/honesty.test.tsx`:

```tsx
import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';
import { App } from '../App';
import type { BreakEven, Epochs, Ladder } from '../api/types';
import { useEpochs } from '../api/useEpochs';
import { RESET_COPY, RESET_OFF, RESET_QUEUED, ResetFold } from '../components/ResetFold';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import { EPISODE_DEF } from '../help';
import { LADDER, NOT_MONOTONE, breakEvenText, ladderModel } from '../lib/ladder';
import { GLYPH_NAME, GLYPH_ORDER, GRID_HINT, LABEL_TEXT } from '../lib/sweet';
import {
  AXIS_TITLE, LADDER_CAPTION, MARKETS_CAPTION, MARKETS_MISSING, MARKETS_RECOMPUTING, MARKETS_WARMING, NO_RUNGS, TABLE_CAPTION,
} from '../schematics/LatencyLadder';
import { appRoutes } from '../test/appRoutes';
import { epochsFixture, ladderFixture, ladderLiveFixture, sweetRow } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { COPY, caughtCategory, caughtText, onTable100, onTableDepth } from './copy';
import { checkHonesty, unlabelledMoney } from './honesty';

const PAGES = ['/', '/sweet', '/episodes', '/episodes/live-1-00000001', '/paper', '/papers', '/health'];

function scan(el: HTMLElement, where: string) {
  expect(checkHonesty(el.textContent ?? ''), where).toEqual([]);
  expect(unlabelledMoney(el), where).toEqual([]);
}

describe('checkHonesty and unlabelledMoney', () => {
  it('fail on the real-money words and on profit outside the existing phrases', () => {
    expect(checkHonesty('We earned $3 of income; it made money')).toEqual(['earned', 'income', 'made']);
    expect(checkHonesty('Earn more')).toEqual(['earn']);
    expect(checkHonesty('this latency is profitable')).toEqual(['profitable']);
    expect(checkHonesty('PROFIT @ $100, net profit at the peak, profit after taker fees, profit before taker fees')).toEqual([]);
    expect(checkHonesty('a nice profit')).toEqual(['profit']);
    expect(checkHonesty('basket gains, leg-risk cost, pays on paper, loses on paper')).toEqual([]);
  });

  it('find a money figure with no paper / on the table / captured at N ms label', () => {
    const ok = document.createElement('div');
    ok.innerHTML = '<p>On the table: <span>$1.00</span></p><table><caption>paper</caption><tbody><tr><td>$2.00</td></tr></tbody></table>';
    expect(unlabelledMoney(ok)).toEqual([]);
    const bad = document.createElement('div');
    bad.innerHTML = '<p>You have <b>$5.00</b></p>';
    expect(unlabelledMoney(bad)).toEqual(['You have $5.00']);
  });
});

describe('every fixed sentence the panel, the grid and the reset fold can show (spec 7a §9.4)', () => {
  it('is honest', () => {
    const kinds: BreakEven[] = [
      { kind: 'thin' }, { kind: 'all' }, { kind: 'none' },
      { kind: 'at', latency_ms: 70, direction: 'loses_beyond' }, { kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true },
    ];
    const ladders: Ladder[] = [
      ...kinds.map((be) => ladderFixture({ break_even: be })),
      ladderFixture({ rungs: [], break_even: { kind: 'thin' } }),
      ladderFixture({ epoch: 0, start_ms: null, rungs: [], break_even: { kind: 'thin' } }),
      ladderFixture({ rungs: ladderFixture().rungs.map((r) => ({ ...r, latency_ms: null })), break_even: { kind: 'thin' } }),
    ];
    const words = [
      ...Object.values(COPY), caughtText(null), caughtText(4190), caughtCategory('crypto'), onTable100('0.33'), onTable100(null), onTableDepth('0.41'),
      ...ladders.flatMap((l) => Object.values(breakEvenText(ladderModel(l)))),
      LADDER_CAPTION, AXIS_TITLE, MARKETS_CAPTION, MARKETS_WARMING, MARKETS_RECOMPUTING, MARKETS_MISSING, NO_RUNGS, TABLE_CAPTION, NOT_MONOTONE,
      RESET_COPY, RESET_QUEUED, RESET_OFF, GRID_HINT, EPISODE_DEF, ...GLYPH_ORDER.map((k) => GLYPH_NAME[k]), ...Object.values(LABEL_TEXT),
    ];
    for (const w of words) expect(checkHonesty(w), w).toEqual([]);
  });
});

describe('the panel is honest on every page, for every question (spec 7a §9.4)', () => {
  for (const ladder of [ladderFixture(), ladderLiveFixture()]) {
    for (const p of PAGES) {
      it(`${p} with a ${ladder.break_even.kind} ladder`, async () => {
        const f = mockApi(appRoutes({ '/api/paper/ladder': ladder }));
        render(
          <MemoryRouter initialEntries={[p]}>
            <App />
          </MemoryRouter>,
        );
        await settle(f);
        fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
        const panel = screen.getByRole('complementary', { name: 'explain' });
        await within(panel).findByText(breakEvenText(ladderModel(ladder)).big);
        for (const b of [...panel.querySelectorAll<HTMLButtonElement>('button.explain-q')]) {
          if (b.getAttribute('aria-expanded') !== 'true') fireEvent.click(b);
          await settle(f);
          scan(panel, `${p} · ${b.textContent}`);
        }
      });
    }
  }

  it('Radar with an incomplete set selected', async () => {
    const f = mockApi(appRoutes());
    render(
      <MemoryRouter initialEntries={['/']}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'show 1 incomplete set' }));
    fireEvent.click(screen.getByRole('link', { name: 'Who wins the cup?' }).closest('tr') as HTMLTableRowElement);
    fireEvent.click(screen.getByRole('button', { name: /EXPLAIN/ }));
    fireEvent.click(screen.getByRole('button', { name: 'How can this make money?' }));
    await settle(f);
    const panel = screen.getByRole('complementary', { name: 'explain' });
    expect(within(panel).getByRole('columnheader', { name: 'an outcome not in the set ✗' })).toBeInTheDocument();
    scan(panel, 'radar, incomplete set');
  });

  it('the Sweet grid legend and every detail line', () => {
    const labels = ['candidate', 'negative', 'inconclusive', 'thin', 'not_tested', 'dead_on_paper'] as const;
    const rows = labels.map((label, i) =>
      sweetRow({ scenario: LADDER[i].scenario, latency_ms: LADDER[i].latency_ms, label, mean: '0.1', lo: '-0.1', hi: '0.3', n_exec: 30, clusters: 30 }),
    );
    const { container } = render(<SweetLadderGrid rows={rows} side="no" dim="all" label="scan" />);
    const wrap = container.querySelector('.lgrid-wrap') as HTMLElement;
    for (const b of screen.getAllByRole('button')) {
      fireEvent.focus(b);
      scan(wrap, b.getAttribute('aria-label') ?? '');
    }
  });

  it('the reset fold, in every state', async () => {
    const NOW = Date.UTC(2026, 8, 30, 12, 0);
    function Harness() {
      const e = useEpochs();
      return <ResetFold epochs={e} now={NOW} />;
    }
    const states: [Epochs, number | null, string][] = [
      [epochsFixture(), null, 'idle'],
      [epochsFixture({ pending_request: { id: 1, requested_ms: NOW } }), null, 'queued'],
      [epochsFixture({ next_allowed_ms: Date.UTC(2099, 0, 1) }), null, 'next allowed'],
      [epochsFixture(), 503, 'off'],
    ];
    for (const [e, status, name] of states) {
      mockApi({ '/api/paper/epochs': e, '/api/paper/reset': () => jsonResponse(status ?? 202, { error: 'reset_disabled', detail: 'x' }) });
      const { container, unmount } = render(<Harness />);
      await screen.findByText(RESET_COPY);
      if (status !== null) {
        await userEvent.type(screen.getByRole('textbox', { name: 'type reset to confirm' }), 'reset');
        await userEvent.click(screen.getByRole('button', { name: 'RESET' }));
        await screen.findByText(RESET_OFF);
      }
      scan(container, `reset fold: ${name}`);
      unmount();
    }
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/honesty.test.tsx src/explain/fetchRule.test.tsx`
Expected: FAIL — `Failed to resolve import "./honesty"` in honesty.test.tsx. fetchRule.test.tsx already PASSES (it verifies Tasks 8–11; if it fails, the failing path names the page that fetches on open or on a question — fix that task's code, not this test).

- [ ] **Step 3: Implement** — create `web/src/explain/honesty.ts`:

```ts
// Spec 7a §9.4 (09-27 §7.2): the panel's text never claims real money. Fix the copy, never widen the rule.
const BANNED = /\b(earned|earn|made|income|profitable)\b/gi;
/** `profit` is allowed only inside the existing help.ts phrases. */
const PROFIT_OK = /profit @ \$100|net profit at the peak|profit (?:after|before) taker fees/gi;
const MONEY = /\$\d/;
const LABEL = /paper|on the table|captured at \d+ ms/i;

/** The offending words, lower case, in text order; [] means honest. */
export function checkHonesty(text: string): string[] {
  const hits = [...text.matchAll(BANNED)].map((m) => m[0].toLowerCase());
  const bare = [...text.replace(PROFIT_OK, '').matchAll(/\bprofits?\b/gi)].map((m) => m[0].toLowerCase());
  return [...hits, ...bare];
}

/** Every block (li, p, table, figure, dd) that shows a "$<digit>" figure without saying it is paper, on
 *  the table, or captured at N ms. Returns their texts; [] means every figure is labelled. */
export function unlabelledMoney(root: HTMLElement): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
    if (!MONEY.test(n.textContent ?? '')) continue;
    const block = n.parentElement?.closest('li, p, table, figure, dd') ?? root;
    const text = (block.textContent ?? '').replace(/\s+/g, ' ').trim();
    if (!LABEL.test(text) && !out.includes(text)) out.push(text);
  }
  return out;
}
```

- [ ] **Step 4: Run the tests**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && npx vitest run src/explain/honesty.test.tsx src/explain/fetchRule.test.tsx && npm test && npx tsc --noEmit && npm run build`
Expected: PASS; `vite build` prints `✓ built in …`. A failing scan names the page and question: reword that string in `copy.ts` / the component (never widen `BANNED`, `PROFIT_OK` or `LABEL`).

- [ ] **Step 5: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web/web && git add src/explain/honesty.ts src/explain/honesty.test.tsx src/explain/fetchRule.test.tsx src/test/settle.ts && git commit -m "web: the explain panel stays paper-honest on every page and question, and fetches only the ladder when opened"
```

---

### Task 13: runbook for deploy B (web only, at a quiet hour)

**Files:**
- Create: `docs/superpowers/runbooks/phase7a-web-vps.md`

**Interfaces:**
- Consumes: the image names, compose invocations, SSH/scp parameters and checks of `docs/superpowers/runbooks/phase7a-latency-vps.md` (steps 1, 6, 12–13, 15, 17, 28–31) and `phase5b-storage-vps.md` (steps 28–29); the `web:pre7a` tag made in 7a step 15.
- Produces: the owner's runbook; the web container recreated alone; a Chrome check at 1,440, 1,600 and ~360 px; a rollback to `web:pre7a`.

- [ ] **Step 1: Write the runbook** — create `docs/superpowers/runbooks/phase7a-web-vps.md` with exactly:

````markdown
# Phase 7a step B on the VPS: the latency answer on the dashboard (web only)

Note: one image change, `cute-trading/web:dev`: the explain panel with the latency ladder, the Sweet spots grid, and the Paper epoch line, `earlier epochs` and `reset paper money` folds. The api, scanner, analyze, compact and postgres are not touched; step A is live since 2026-09-29 19:58 UTC.
Note: recreate `web` only, at a quiet minute: a `web` recreate once coincided with the scanner's websockets dropping (2026-09-28 02:46 UTC, mechanism unverified); later web recreates did not.
Note: never print `.env` or a container's environment; `docker compose config` only as `--env-file .env.example … config -q`; never restart Docker.
Note: PC steps in Git Bash from the main checkout on the merged commit; VPS steps as root over SSH; browser steps in Chrome, logged in as the owner.

## On the PC

1. The repo on the commit being deployed; the tree must be clean.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && git status --short --untracked-files=no && git branch --show-current && git log -1 --format='%h %s'
```
→ expected: no status lines, `main`, then `<sha> <subject>` of the merged 7a-B commit.
if not: commit or stash the listed changes, then run this step again.

2. Web tests, types and the production build pass.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/web && npm ci --no-audit --no-fund >/dev/null && npm test 2>&1 | grep -E '^ +(Test Files|Tests) ' && npx tsc --noEmit && echo tsc-ok && npm run build 2>&1 | tail -1
```
→ expected: `Test Files  <n> passed (<n>)`, `Tests  <n> passed (<n>)`, `tsc-ok`, then `✓ built in <t>`.
if not: any `failed` or a tsc error: fix on a branch; do not build the image.
**STOP**

3. Disk budget on the PC before building.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && powershell -ExecutionPolicy Bypass -File scripts/disk-budget.ps1
```
→ expected: a table, then `total <x> GB of 30 GB` and no `OVER BUDGET` line.
if not: `docker builder prune -f --keep-storage 5GB`, then run this step again.

4. Build the web image only.
```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading && docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile web build web 2>&1 | tail -3
```
→ expected: the build's last lines without `ERROR`.
if not: read the first `ERROR` line (a type error means step 2 was skipped), fix, run again.

5. The image carries the new bundle.
```bash
MSYS_NO_PATHCONV=1 docker run --rm --entrypoint sh cute-trading/web:dev -c 'grep -l "At what latency would this pay" /usr/share/nginx/html/assets/*.js | wc -l; grep -l "reset-paper" /usr/share/nginx/html/assets/*.js | wc -l'
```
→ expected: `1`, then `1`.
if not: `0` = the image was built from an old tree: redo steps 1 and 4.
**STOP**

6. Save and ship the image.
```bash
docker save cute-trading/web:dev | gzip -1 > "$HOME/ct-7a-web.tar.gz" && ls -l "$HOME/ct-7a-web.tar.gz" && scp -P 50158 -i "C:/Users/Isabelly/.ssh/id_ed25519" "$HOME/ct-7a-web.tar.gz" root@31.97.23.123:/root/ && rm "$HOME/ct-7a-web.tar.gz"
```
→ expected: one `ls -l` line (roughly 15–40 MB), the transfer progress line, then nothing.
if not: check the key path is quoted with forward slashes and the port is 50158.

## On the VPS: deploy

7. The rollback tag from step A points at the web that runs now.
```bash
cd /home/isoca/projects/cute-trading && docker image inspect -f '{{.Id}}' cute-trading/web:pre7a cute-trading/web:dev && docker inspect -f '{{.Image}}' cute-trading-web-1
```
→ expected: three identical `sha256:…` lines.
if not: `No such image: cute-trading/web:pre7a` = `docker tag cute-trading/web:dev cute-trading/web:pre7a`, then this step again; a different running ID = write it in Results and `docker tag <that id> cute-trading/web:pre7a`.
**STOP**

8. Load the new image (moves `web:dev`; `web:pre7a` stays).
```bash
cd /home/isoca/projects/cute-trading && gunzip -c /root/ct-7a-web.tar.gz | docker load && rm /root/ct-7a-web.tar.gz && docker image ls 'cute-trading/web' --format '{{.Repository}}:{{.Tag}} {{.ID}}' && LIMIT_GB=75 sh scripts/disk-budget.sh
```
→ expected: `Loaded image: cute-trading/web:dev`; `web:dev` on a new ID and `web:pre7a` on the old one; then `total <x> GB of 75 GB` and no `OVER BUDGET`.
if not: `OVER BUDGET` = `docker image prune -f`, then `LIMIT_GB=75 sh scripts/disk-budget.sh` again.

9. A quiet minute: the latest minute's messages are below the 24 h median.
```bash
cd /home/isoca/projects/cute-trading && date -u +%H:%M && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT (SELECT msgs FROM health ORDER BY minute_ms DESC LIMIT 1) < (SELECT percentile_disc(0.5) WITHIN GROUP (ORDER BY msgs) FROM health WHERE minute_ms > (extract(epoch FROM now()) * 1000)::bigint - 86400000)"
```
→ expected: the UTC time, then `t`.
if not: `f` = a busy minute: wait 15 minutes and run this step again; never go on during a burst.
**STOP**

10. Scanner websockets baseline, then recreate `web` only.
```bash
cd /home/isoca/projects/cute-trading && date +%s%3N > /root/7b_web_ms && docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | tee /root/7b_scanner_started && docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), shards, reconnects, parked_shards, msgs FROM health ORDER BY minute_ms DESC LIMIT 3" && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker compose -f compose.yml -f compose.web.yml --profile web ps web --format '{{.Service}} {{.Status}}'
```
→ expected: the scanner's start time, three `HH:MI|<shards>|0|0|<msgs>` rows, `Container cute-trading-web-1 …` lines only (no api, no network), then `web Up …`.
if not: `web` not `Up` = `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web`, then the rollback (step 18).

11. After the recreate: no websocket disconnect.
```bash
cd /home/isoca/projects/cute-trading && sleep 180; T0=$(cat /root/7b_web_ms); docker inspect -f '{{.State.StartedAt}}' cute-trading-scanner-1 | diff - /root/7b_scanner_started && echo scanner-not-restarted; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT count(*) FROM integrity_events WHERE kind = 'reconnect_gap' AND ts_ms >= $T0"
```
→ expected: `scanner-not-restarted`, then `0`.
if not: reconnects > 0: do NOT roll back; write the count and the UTC time in Results.

## Checks

12. Unauthenticated requests never reach the dashboard or the api.
```bash
for p in / /sweet /api/paper/ladder; do curl -sS -o /dev/null -w "$p %{http_code}\n" "https://radar.isoca.space$p"; done
```
→ expected: `/ 302`, `/sweet 302`, `/api/paper/ladder 302` (or `401`).
if not: a `200` = forward-auth is off: roll back (step 18) and report.

13. (Chrome, window 1440 px wide) open `https://radar.isoca.space/`, DevTools Network filtered on `ladder`, click the `EXPLAIN ▸` tab on the right edge.
→ expected: a drawer opens left of the tab (the tab stays visible); exactly one `/api/paper/ladder` request; `At what latency would this pay?` is open with the big VT323 line (`≈ N ms`, `none`, `pays at every rung`, `too few trades yet` or `a rung has no account yet`), six columns `0 25 52 100 200 300` plus `extras`, counts `n/30` while thin, `as of HH:MM UTC`, the pink "us" band between the 200 and 300 columns with `us: measured ≈N + matching`, the sp-idle-nodelay and sp-burst lines, and the folds `which markets` (a grid, or `recomputing after the ladder` / `no markets data`) and `the numbers`.
if not: a label drawn inside the SVG or stretched = report (spec §7.2); more than one request = the fetch rule broke: report.

14. (Chrome, same tab) keep the panel open 2 minutes, then press Esc and wait 2 more minutes.
→ expected: one more `/api/paper/ladder` per minute while open; Esc closes it with the focus on the tab; no request while closed.

15. (Chrome) the panel on each page: `/` (How can this make money?), `/episodes` (click a row, then What is an episode?), an episode detail (Would we have caught it?), `/paper` (click an awaiting row, then Why is money locked?), `/sweet`, `/papers`, `/health` (the hero only).
→ expected: each answer is one drawing plus at most two sentences; `as selected: …` after a row click, gone after `back to the page` and after leaving the page; no word `earned`, `made`, `income` or `profitable` anywhere in the panel.

16. (Chrome) `/sweet`, then `/paper`.
→ expected: `/sweet` shows the grid with six NO columns and two YES columns, a tap on a cell writes one detail line under the grid, the old per-scenario chart and its chips are gone, every number is still in `all numbers`; `/paper` shows `since new policies 2026-09-29 19:58 UTC · epoch 1` (or `since reset … · epoch N`) above ACCOUNTS, `earlier epochs` lists epoch 0 with the P0 finals once opened, and `reset paper money` shows the current epoch and a RESET button that stays disabled until `reset` is typed. Do not press RESET for this check (one per UTC day).

17. (Chrome, window 1600 px wide, then DevTools device toolbar 360 × 740) `/`, `/sweet`, `/paper`, each with the panel closed and open; in the console on each:
```js
document.documentElement.scrollWidth <= window.innerWidth
```
→ expected: at 1600 px the panel docks beside the app and the pair is centred; at 360 px it is a drawer; `true` on every page, open and closed; the Sweet grid fits without scrolling.
if not: `false` = horizontal scroll: screenshot and report (a layout fix on a branch; no rollback needed).
**STOP**

## Rollback

18. Back to the pre-7a web (the api stays on 7a; the old web never calls the new routes). At a quiet minute (step 9).
```bash
cd /home/isoca/projects/cute-trading && docker tag cute-trading/web:pre7a cute-trading/web:dev && docker compose -f compose.yml -f compose.web.yml --profile web up -d --no-build --no-deps web && docker image inspect -f '{{.Id}}' cute-trading/web:pre7a && docker inspect -f '{{.Image}}' cute-trading-web-1
```
→ expected: the same `sha256:…` twice.
if not: `docker compose -f compose.yml -f compose.web.yml --profile web logs --tail 30 web` and report.

## Results

- Quiet minute (step 9), UTC time:
- Reconnects after the web recreate (step 11):
- Hero at deploy (step 13): big line, markets status:
- Ladder requests while open / closed (step 14):
- 1600 px dock and 360 px scroll check (step 17):
````

- [ ] **Step 2: Check the format**

Run: `cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web && grep -c '^→ expected' docs/superpowers/runbooks/phase7a-web-vps.md && grep -c '^\*\*STOP\*\*' docs/superpowers/runbooks/phase7a-web-vps.md && grep -c '^[0-9]\+\. ' docs/superpowers/runbooks/phase7a-web-vps.md`
Expected: `18`, `5`, `18` (every numbered step has one expected line; STOP after the tests, the bundle check, the rollback tag, the quiet minute and the Chrome layout check).

- [ ] **Step 3: Commit**

```bash
cd /c/Users/Isabelly/PycharmProjects/cute-trading/.claude/worktrees/phase7a-B-web && git add docs/superpowers/runbooks/phase7a-web-vps.md && git commit -m "runbook: 7a step B, web only at a quiet minute: build, ship, recreate web, websocket check, Chrome at 1440/1600/360 px, rollback to web:pre7a"
```

---

## Spec coverage

| Spec | Requirement | Task(s) |
|---|---|---|
| §5.1 | POST with `Content-Type: application/json` + `X-CT-Intent: reset-paper`, body `{"confirm":"reset"}`; 403/400/409/429/503 as `ApiError` codes | 1 |
| §5.2 | `/api/paper/epochs` consumed (`current`, `pending_request`, `next_allowed_ms`) | 1, 6, 7 |
| §5.3 | Fold `reset paper money` at the bottom of Paper; current epoch and start; typed `reset` + button; no dialogs; states idle / sending / queued (10 s poll while pending) / next allowed; the copy | 6 (`useEpochs`), 7 |
| §6 | `since reset ⟨date⟩ · epoch N` above ACCOUNTS | 6 |
| §6 | `earlier epochs` fold: start, end, cause, each P0 fee-0 account's final equity through `?epoch=` | 1, 6 |
| §7.2 | stacked bar per rung (gains up, leg-risk down, settle by sign), hatched unrealized, net dot, zero line, us band, extras beside the 300 ms rung | 2, 5 |
| §7.2 | one big VT323 number = break-even; every other label HTML; ordinal x axis; break-even interpolated between neighbours | 2, 5 |
| §7.2 | `which markets` fold (compact grid from `markets`); caption naming the unwind rule; typed `model`, never fetches | 5 |
| §8 | grid rows = dimension buckets (`all` first), columns = LADDER by latency (6 NO, 2 YES), extras and fee variants out, glyphs, legend beside, detail line on tap/focus, HTML table of buttons with aria-labels, 360 px budget | 3, 4 |
| §9.1 | vertical `EXPLAIN ▸` tab on every page; dock ≥ 1,500 px as a centred pair; drawer below; Esc closes and returns focus; open state and last question in guarded localStorage; `<aside aria-label="explain">` | 8 |
| §9.2 | hero first and open by default on every page; Radar "How can this make money?" (payoff (b), selected / top open / example, `net_budget100` and `net`); Episode detail "Would we have caught it?"; Episodes "What is an episode?" (bar + labelled example); Paper "Why is money locked?" (blocks by basis, days from `Position.end_date`); copy in `src/explain/copy.ts`, shared sentence imported from `help.ts` | 8, 10, 11 |
| §9.3 | pages publish, panel reads (`ExplainContext`); exactly one fetch (`/api/paper/ladder`) only while open, 60 s poll; Episode detail and the fold reuse `markets`; the fetch-rule test on every page; selection from Radar/Episodes rows, Episode detail (its page data) and Paper positions; "back to the page" | 8, 9, 11, 12 |
| §9.4 | banned words, `profit` phrases, labelled money; scan covers the panel on every page and question, the Sweet grid legend and the reset fold | 12 |
| §10 web | LatencyLadder kinds / signed bars / us band / measured null; grid glyphs / exclusions / YES 2 columns / sort / detail on focus / 360 px rule check; reset fold gate, states, poll only while pending; panel questions from fixtures, fetch rule, selection and back, Esc and focus, banned words; `npx tsc --noEmit` | 2–12 |
| §11 | deploy B: web only, quiet hour, Chrome check at 1,440 and ~360 px, rollback | 13 |
| §17 | missing rungs drawn as `no account` | 2, 5 |
| §17 | thin shows `n/30` | 2, 5 |
| §17 | `as of HH:MM UTC` under the drawing | 2, 5 |
| §17 | `markets_status`: live and warming draw rows, `under_review` → `recomputing after the ladder`, `missing` → `no markets data` | 5 |
| §17 | two time frames labelled (epoch bars vs 30-day Sweet spots) | 5, 11 |
| §17 | us-band from measured + 50 to + 150 ms, interpolated on the ordinal axis | 2, 5 |
| §17 | reset fold sends both headers; 202/409/429/503 (`reset is off on this server`) | 1, 7 |
| §17 | P3 labelled `control: ignores fees` | 6 |
| §17 | deploy B recreates `web` only | 13 |
| 09-27 §7.4 | `Position.end_date` → days to resolution or "unknown" | 1, 11 |

**Gaps not filled here:**
- 09-27 §7.3 asked for the panel's payoff structure to be checked against `crates/arb-wasm/fixtures/payoff.json`; `arb-wasm` belongs to Phase 7b and does not exist yet, so `payoffStructure` is checked against the parent spec §5.7 (b) rules in `models.test.ts`. Add the fixture parity test in 7b.
- Layout (1,440 / 1,600 / 360 px, dock pair centring, drawer beside the tab) cannot be seen by jsdom: `theme.test.ts` pins the rules and runbook step 17 checks it in Chrome.
- `ladderLiveFixture` is rebuilt from the spec §17 summary and runbook 7a step 31 (names, latencies, `thin`, `warming`, measured 155), not a byte copy of the live body, which was not saved.
- `help.ts:160` (`sweetNo`, "made or lost") is not imported by the panel, so spec §9.4 leaves it as is.
