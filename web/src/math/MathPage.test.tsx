// The public page as a whole (spec 7b §6, §8). Timers are faked so settle() flushes by advancing them, never by
// sleeping; the page's clock is injected. Engine and snapshot figures are read from the engine, the fixtures or
// the lib that formats them, never retyped (R13).
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkHonesty } from '../explain/honesty';
import { breakEvenText, ladderModel } from '../lib/ladder';
import { mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { describeWasm, loadTestEngine } from '../wasm/testEngine';
import type { Engine } from '../wasm/types';
import sampleText from './__fixtures__/public.sample.json?raw';
import { ENGINE_FAILED, FOOTER, LOADING, SECTIONS, TAGLINE, TITLE } from './copy';
import { snapshotFixture } from './fixtures';
import { BEHIND, LIVE_UNAVAILABLE } from './LiveStrip';
import { MathPage } from './MathPage';
import { FRESH_MS, REFETCH_MS, SNAPSHOT_URL, parseSnapshot, publicToLadder } from './snapshot';

/** The live parts are the real ones unless a test sets a flag: then that part throws while rendering. */
const boom = vi.hoisted(() => ({ strip: false, ladder: false }));
vi.mock('./LiveStrip', async (original) => {
  const m = await original<typeof import('./LiveStrip')>();
  return {
    ...m,
    LiveStrip: (p: Parameters<typeof m.LiveStrip>[0]) => {
      if (boom.strip) throw new Error('the strip broke');
      return m.LiveStrip(p);
    },
  };
});
vi.mock('./LiveLadder', async (original) => {
  const m = await original<typeof import('./LiveLadder')>();
  return {
    ...m,
    LiveLadder: (p: Parameters<typeof m.LiveLadder>[0]) => {
      if (boom.ladder) throw new Error('the ladder broke');
      return m.LiveLadder(p);
    },
  };
});

const T0 = 1_790_000_000_000;
let clock = T0;
const now = () => clock;
const never = new Promise<Engine>(() => {});
const IDS = ['a', 'b', 'c', 'd', 'e', 'f'] as const;
const TITLES = IDS.map((k) => SECTIONS[k].title);
const STRIP = { name: 'today on the live scanner' };

function setVisible(v: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => v });
  document.dispatchEvent(new Event('visibilitychange'));
}
const requested = (f: { mock: { calls: unknown[][] } }) => f.mock.calls.map((c) => String(c[0]));

beforeEach(() => {
  clock = T0;
  boom.strip = false;
  boom.ladder = false;
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(document, 'visibilityState');
});

describe('MathPage, the engine aside (spec 7b §6.1, §6.3)', () => {
  it('is one page with the header, the tagline and the line saying the dashboard is private; no link leaves it', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    const { container } = render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.getAllByRole('heading', { level: 1 }).map((h) => h.textContent)).toEqual([TITLE]);
    expect(TITLE).toBe('isoca-radar · math');
    expect(screen.getByText(TAGLINE)).toBeInTheDocument();
    expect(screen.getByText(FOOTER)).toBeInTheDocument();
    expect(FOOTER).toMatch(/dashboard is private/);
    expect(container.querySelectorAll('a, nav')).toHaveLength(0);
  });

  it('while the engine loads: one neutral line, no section, and the live strip already draws', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.getByRole('status')).toHaveTextContent(LOADING);
    expect(screen.queryAllByRole('heading', { level: 2 })).toHaveLength(0);
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
    expect(screen.getByRole('region', STRIP)).toBeInTheDocument();
  });

  it('says one honest sentence when the engine does not load, draws no section, and the live strip still draws', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.reject(new Error('blocked'))} now={now} />);
    await settle(f);
    expect(screen.getAllByRole('alert').map((a) => a.textContent)).toEqual([ENGINE_FAILED]);
    expect(ENGINE_FAILED).toMatch(/did not load: the drawings need WebAssembly/);
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.queryAllByRole('heading', { level: 2 })).toHaveLength(0);
    expect(screen.getByRole('region', STRIP)).toBeInTheDocument();
    expect(screen.getByText(FOOTER)).toBeInTheDocument();
  });

  it('shows nothing live while the snapshot is still loading: no flash of "unavailable"', async () => {
    const f = vi.fn((_url: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_, reject) => init?.signal?.addEventListener('abort', () => reject(new Error('aborted')))));
    vi.stubGlobal('fetch', f);
    const { unmount } = render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
    expect(screen.queryByRole('region', STRIP)).toBeNull();
    unmount();
    await settle(f);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('a missing snapshot is exactly one dim line, and the page asks for nothing but that file', async () => {
    const f = mockApi({});
    render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(1);
    expect(screen.getByText(LIVE_UNAVAILABLE)).toHaveClass('dim');
    expect(screen.queryByRole('region', STRIP)).toBeNull();
    expect(requested(f)).toEqual(['/math-data/public.json']);
  });

  it('StrictMode still sends one request', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(
      <StrictMode>
        <MathPage engine={never} now={now} />
      </StrictMode>,
    );
    await settle(f);
    expect(requested(f)).toEqual(['/math-data/public.json']);
    expect(screen.getAllByRole('region', STRIP)).toHaveLength(1);
  });

  it('a snapshot older than 30 minutes says the writer is behind', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0 - FRESH_MS - 1) });
    render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.getByText(new RegExp(`: ${BEHIND}$`))).toHaveClass('stale');
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
  });

  it('holds no interval timer: a fresh snapshot turns stale when the visitor returns to the tab, not before', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(vi.getTimerCount()).toBe(0);
    expect(screen.queryByText(new RegExp(BEHIND))).toBeNull();
    clock = T0 + FRESH_MS + 1;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(FRESH_MS + 1);
    });
    expect(screen.queryByText(new RegExp(BEHIND))).toBeNull(); // nothing ticks on its own
    act(() => setVisible('visible'));
    await settle(f);
    expect(screen.getByText(new RegExp(`: ${BEHIND}$`))).toHaveClass('stale');
    expect(requested(f)).toEqual(['/math-data/public.json', '/math-data/public.json']);
    expect(vi.getTimerCount()).toBe(0);
  });

  describe('when a live part throws while rendering', () => {
    let errors: ReturnType<typeof vi.spyOn>;
    beforeEach(() => {
      errors = vi.spyOn(console, 'error').mockImplementation(() => undefined); // React reports what a boundary caught
    });

    it('the strip is replaced by the one "unavailable" line and the rest of the page stands', async () => {
      boom.strip = true;
      const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
      render(<MathPage engine={never} now={now} />);
      await settle(f);
      expect(errors).toHaveBeenCalled();
      expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(1);
      expect(screen.queryByRole('region', STRIP)).toBeNull();
      expect(screen.getByText(TAGLINE)).toBeInTheDocument();
      expect(screen.getByRole('status')).toHaveTextContent(LOADING);
      expect(screen.getByText(FOOTER)).toBeInTheDocument();
    });

    it('a newer snapshot gets a fresh try', async () => {
      boom.strip = true;
      const f = mockApi({ [SNAPSHOT_URL]: () => snapshotFixture({}, clock) });
      render(<MathPage engine={never} now={now} />);
      await settle(f);
      expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(1);
      boom.strip = false;
      clock = T0 + REFETCH_MS + 1;
      act(() => setVisible('visible'));
      await settle(f);
      expect(screen.getByRole('region', STRIP)).toBeInTheDocument();
      expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
    });
  });
});

/** Every text node under `root`, and every aria-label and title: what a visitor can read or hear. */
function readable(root: HTMLElement): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) out.push(n.textContent ?? '');
  for (const el of root.querySelectorAll('[aria-label], [title]')) out.push(el.getAttribute('aria-label') ?? '', el.getAttribute('title') ?? '');
  return out.filter((s) => s.trim() !== '');
}

/** A Rust path in a caption (`arb_core::fill::walk`, `sim::venue::{basket_buy, unwind_sell}`): a name, not a claim. */
const RUST_PATH = /\b(?:arb_core|sim)::(?:\w+::)*(?:\{[^}]*\}|\w+)/g;
const FRAME = /\bon paper\b|\bon the table\b/;
const text = (el: Element | null | undefined) => el?.textContent?.replace(/\s+/g, ' ').trim() ?? '';

/**
 * R11 on the rendered page. A text node showing a "$<digit>" figure is framed when "on paper" or "on the
 * table" is in (1) its row: the nearest ancestor that is an li, p, tr, figcaption, caption or the div pairing a
 * dt with its dd; or (2) the caption of the closest figure or table around it: that figure's own figcaption
 * children, or that table's caption. Never the section or the page as a whole. Returns the unframed rows.
 */
function unframedMoney(root: HTMLElement): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
    if (!/\$\d/.test(n.textContent ?? '')) continue;
    const el = n.parentElement;
    const row = el?.closest('li, p, tr, figcaption, caption, dl > div') ?? null;
    if (row !== null && FRAME.test(text(row))) continue;
    const fig = el?.closest('figure, table') ?? null;
    const captions = fig === null ? [] : [...fig.children].filter((c) => c.matches('figcaption, caption'));
    if (captions.some((c) => FRAME.test(text(c)))) continue;
    const shown = text(row ?? el);
    if (!out.includes(shown)) out.push(shown);
  }
  return out;
}

async function openEveryFold(root: HTMLElement, f: { mock: { calls: unknown[] } }): Promise<number> {
  let opened = 0;
  for (let round = 0; round < 5; round++) {
    const closed = [...root.querySelectorAll('details:not([open]) > summary')];
    if (closed.length === 0) break;
    for (const s of closed) fireEvent.click(s);
    opened += closed.length;
    await settle(f);
  }
  expect(root.querySelectorAll('details:not([open])')).toHaveLength(0);
  return opened;
}

describe('the money frame rule itself can fail', () => {
  it('finds a bare figure, a figure framed only by a far-away paragraph, and passes a row or a caption', () => {
    const { container } = render(
      <div>
        <p>net $1.00</p>
        <section><p>all of this is on paper</p><ul><li>spent $2.00</li></ul></section>
        <p>$3.00 on the table</p>
        <figure><dl><div><dt>spent</dt><dd>$4.00</dd></div></dl><figcaption>every dollar here is on paper</figcaption></figure>
        <table><caption>on paper</caption><tbody><tr><td>$5.00</td></tr></tbody></table>
      </div>,
    );
    expect(unframedMoney(container)).toEqual(['net $1.00', 'spent $2.00']);
  });
});

describeWasm('MathPage on the real engine (spec 7b §6, §8)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it('draws a–f in order under the live strip, each with its anchor; f ends with the live ladder', async () => {
    const snap = snapshotFixture({}, T0);
    const f = mockApi({ [SNAPSHOT_URL]: snap });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await settle(f);
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(TITLES);
    expect([...container.querySelectorAll('section.m-sec')].map((s) => s.id)).toEqual([...IDS]);
    const strip = screen.getByRole('region', STRIP);
    const first = screen.getByRole('region', { name: SECTIONS.a.title });
    expect(strip.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const sf = screen.getByRole('region', { name: SECTIONS.f.title });
    expect(within(sf).getByRole('heading', { level: 3 })).toHaveTextContent(/^on the live scanner, as of \d\d:\d\d UTC$/);
    expect(sf.lastElementChild).toHaveClass('m-liveladder');
    expect(within(sf).getByText(breakEvenText(ladderModel(publicToLadder(snap.ladder))).big)).toBeInTheDocument();
    expect(screen.getAllByRole('heading', { level: 3 })).toHaveLength(1);
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
    expect(container.querySelectorAll('[style]')).toHaveLength(0); // the math CSP has no 'unsafe-inline' for styles
    expect(container.querySelectorAll('svg text')).toHaveLength(0);
    expect(container.innerHTML).not.toContain('NaN');
    expect(requested(f)).toEqual(['/math-data/public.json']);
  });

  it('a stale snapshot says so in the strip and above the ladder', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0 - FRESH_MS - 1) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await settle(f);
    const behind = screen.getAllByText(new RegExp(`: ${BEHIND}$`));
    expect(behind).toHaveLength(2);
    expect(behind.some((el) => el.tagName === 'H3')).toBe(true);
    expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(6);
  });

  it('without the snapshot, f has no live part and every section still works', async () => {
    const f = mockApi({});
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await settle(f);
    expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(6);
    expect(screen.queryAllByRole('heading', { level: 3 })).toHaveLength(0);
    expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(1);
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
    for (const k of IDS) expect(screen.getByText(SECTIONS[k].caption)).toBeInTheDocument();
  });

  it('when the live ladder throws, the strip goes too: one "unavailable" line, six working sections', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    boom.ladder = true;
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await settle(f);
    expect(errors).toHaveBeenCalled();
    expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(1);
    expect(screen.queryByRole('region', STRIP)).toBeNull();
    expect(screen.queryAllByRole('heading', { level: 3 })).toHaveLength(0);
    expect(screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent)).toEqual(TITLES);
    for (const k of IDS) expect(screen.getByText(SECTIONS[k].caption)).toBeInTheDocument();
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
  });

  it('the whole page, every fold open, on the sample the api writes: honest words, framed dollars, no /api/', async () => {
    const sample = parseSnapshot(JSON.parse(sampleText));
    if (sample === null) throw new Error('the sample must parse');
    clock = sample.written_ms + 60_000;
    const f = mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await settle(f);
    expect(await openEveryFold(container, f)).toBeGreaterThanOrEqual(4); // a and d's levels, the ladder's two
    expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(6);
    expect(screen.getByRole('region', STRIP)).toBeInTheDocument();
    expect(screen.getAllByRole('heading', { level: 3 })).toHaveLength(1);
    const words = readable(container);
    expect(words.filter((s) => /\$\d/.test(s)).length).toBeGreaterThan(10); // the scan looks at real figures
    // (a) the dashboard's list (earned, earn, made, income, profitable, a bare profit), outside the Rust paths the captions quote
    expect(words.flatMap((s) => checkHonesty(s.replace(RUST_PATH, '')).map((w) => `${w}: ${s}`))).toEqual([]);
    // (b) R11: every $ figure is framed in its row or its caption
    expect(unframedMoney(container)).toEqual([]);
    // (c) nothing on the page points at the private api
    expect(container.innerHTML).not.toContain('/api/');
    expect(requested(f)).toEqual(['/math-data/public.json']);
    expect(container.querySelectorAll('[style]')).toHaveLength(0);
    expect(container.querySelectorAll('svg text')).toHaveLength(0);
    expect(container.innerHTML).not.toContain('NaN');
    // (d) scrollWidth has no meaning in jsdom: the structural rules are pinned in theme.test.ts and math.css.test.ts
  });
});
