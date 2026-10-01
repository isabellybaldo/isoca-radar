// The public page as a whole (spec 7b §6, §8). Timers are faked so settle() flushes by advancing them, never by
// sleeping; the page's clock is injected. Engine and snapshot figures are read from the engine, the fixtures or
// the lib that formats them, never retyped (R13).
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkHonesty } from '../explain/honesty';
import { breakEvenText, ladderModel } from '../lib/ladder';
import { bucketText, cellDetail } from '../lib/sweet';
import { NO_RUNGS } from '../schematics/LatencyLadder';
import { CROSSED } from '../schematics/MirrorBooks';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { describeWasm, loadTestEngine } from '../wasm/testEngine';
import { ENGINE_FNS, type BasketIn, type DepthWalkIn, type Engine, type EngineFn, type PayoffIn, type WalkModel } from '../wasm/types';
import sampleText from './__fixtures__/public.sample.json?raw';
import { basketInput } from './basket';
import { ENGINE_FAILED, FOOTER, LOADING, SECTIONS, TITLE } from './copy';
import { F_BOOKS, F_BURST } from './defaults';
import { snapshotFixture } from './fixtures';
import { BEHIND, LIVE_UNAVAILABLE, STRIP_TITLE } from './LiveStrip';
import { MathPage, WORDS_LABEL } from './MathPage';
import { DEPTH_START, depthInput } from './sections/DepthSection';
import { FRESH_MS, REFETCH_MS, SNAPSHOT_URL, type PublicSnapshot, parseSnapshot, publicToLadder } from './snapshot';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, HOOK, PLAY, TAKE, WORDS } from './story/copy';

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
const STRIP = { name: STRIP_TITLE };

function setVisible(v: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => v });
  document.dispatchEvent(new Event('visibilitychange'));
}
const requested = (f: { mock: { calls: unknown[][] } }) => f.mock.calls.map((c) => String(c[0]));

/** Under the fake timers: flush (never sleep) until `expectation` holds; if it never does, its own failure is thrown. */
async function until(expectation: () => unknown): Promise<void> {
  for (let i = 0; i < 500; i++) {
    try {
      expectation();
      return;
    } catch {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
    }
  }
  expectation();
}
/** settle() only waits for the requests to stop; what a test reads next must be on screen, so wait for exactly that. */
async function landed(f: { mock: { calls: unknown[] } }, expectation: () => unknown): Promise<void> {
  await settle(f);
  await until(expectation);
}
const STORY = CHAPTERS.map((c) => c.id);
const HEADS = [...CHAPTERS.map((c) => `${c.n} ${c.question}`), PLAY.title];
const BEHIND_LINE = new RegExp(`: ${BEHIND}$`);
/** A chapter, the playground or a section by its id; a failure names the id that is not on the page. */
const part = (id: string): HTMLElement => {
  const el = document.getElementById(id);
  if (el === null) throw new Error(`#${id} is not on the page`);
  return el;
};
const stripShown = () => screen.getByRole('region', STRIP);
/** The "unavailable" line stands in for the strip in chapter 3 and for the ladder in chapter 7: once in each, nowhere else. */
const unavailableShown = () => {
  for (const id of ['s3', 's7']) expect(within(part(id)).getAllByText(LIVE_UNAVAILABLE), id).toHaveLength(1);
  expect(screen.getAllByText(LIVE_UNAVAILABLE)).toHaveLength(2);
};
/** "the live writer is behind": in the strip (chapter 3) and in the ladder's heading (chapter 7), once in each. */
const behindShown = () => {
  for (const id of ['s3', 's7']) expect(within(part(id)).getAllByText(BEHIND_LINE), id).toHaveLength(1);
  expect(screen.getAllByText(BEHIND_LINE)).toHaveLength(2);
};
const h2s = () => screen.queryAllByRole('heading', { level: 2 }).map((h) => h.textContent);
/** A chapter's own switch: the two aria-pressed buttons of the .seg that is the chapter's child. */
const switchOf = (id: string) => [...part(id).querySelectorAll(':scope > .seg > button[aria-pressed]')] as HTMLElement[];
const pressed = (id: string) => switchOf(id).map((b) => b.getAttribute('aria-pressed'));
/** Spec §3.2, the engine not there: the story's prose, and nothing of chapters 1 to 6 but prose (chapter 3's live strip aside). */
const proseOnly = () => {
  expect(h2s()).toEqual(HEADS);
  for (const c of CHAPTERS) expect(within(part(c.id)).getByText(c.lead), c.id).toBeInTheDocument();
  expect(within(part('play')).getByText(PLAY.line)).toBeInTheDocument();
  expect(part('play').querySelectorAll('dl.m-words dt')).toHaveLength(WORDS.length);
  expect(document.querySelectorAll('section.m-sec')).toHaveLength(0);
  expect(document.getElementById('a')).toBeNull();
  for (const id of STORY.slice(0, 6)) {
    expect(within(part(id)).queryAllByRole('group'), id).toHaveLength(0);
    expect(part(id).querySelectorAll('.m-ch-take'), id).toHaveLength(0);
    expect([...part(id).querySelectorAll('figure, svg')].filter((el) => el.closest('.m-live') === null), id).toHaveLength(0);
  }
};
/** The six sections' own h3s, and the live ladder's heading (h3.m-live-h, inside chapter 7). */
const h3s = () => screen.queryAllByRole('heading', { level: 3 });
const sectionHeads = () => h3s().filter((h) => h.parentElement?.classList.contains('m-sec'));
const ladderHeads = () => h3s().filter((h) => h.classList.contains('m-live-h'));
const sectionsShown = () => expect(sectionHeads()).toHaveLength(6);
const ladderShown = () => expect(ladderHeads()).toHaveLength(1);
/** Nothing went wrong on screen: no alert (the engine did not load) and no engine sentence in place of a drawing
 *  (Section's Out: a polite status with the error class). */
const noErrors = () => {
  expect(screen.queryAllByRole('alert')).toHaveLength(0);
  expect(document.querySelectorAll('.error')).toHaveLength(0);
};

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
  Reflect.deleteProperty(Element.prototype, 'scrollIntoView');
  window.history.replaceState(null, '', '/');
});

/** jsdom has no scrollIntoView: a stand-in that records the id of every element asked to scroll. */
function recordScrolls(): string[] {
  const ids: string[] = [];
  Object.defineProperty(Element.prototype, 'scrollIntoView', {
    configurable: true,
    value(this: Element, ...args: unknown[]) {
      ids.push(`${this.id}(${args.length})`);
    },
  });
  return ids;
}

describe('MathPage, the engine aside (spec 7b §6.1, §6.3)', () => {
  it('is one page with the header, the hook and the line saying the dashboard is private; no link leaves it', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    const { container } = render(<MathPage engine={never} now={now} />);
    await landed(f, stripShown);
    expect(screen.getAllByRole('heading', { level: 1 }).map((h) => h.textContent)).toEqual([TITLE]);
    expect(TITLE).toBe('isoca-radar · math');
    expect(STRIP_TITLE).toBe('today (UTC) on the live scanner'); // the api's day starts at 00:00 UTC, and the strip says so
    expect(within(screen.getByRole('region', STRIP)).getByText(STRIP_TITLE)).toHaveClass('m-live-k');
    expect(screen.getByText(HOOK)).toHaveClass('m-tag');
    expect(screen.getByRole('main').firstElementChild).toBe(screen.getByText(HOOK)); // the page opens on it
    expect(screen.getByRole('contentinfo')).toHaveTextContent(FOOTER); // a footer landmark,
    expect(screen.getByRole('main')).not.toHaveTextContent(FOOTER); // after main, not in it
    expect(screen.getByRole('main').compareDocumentPosition(screen.getByRole('contentinfo')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(FOOTER).toMatch(/dashboard is private/);
    expect(container.querySelectorAll('a, nav')).toHaveLength(0);
  });

  it('while the engine loads: one neutral line above the story, the eight headings and their prose, no drawing, no section a–f, and the live strip already draws in chapter 3', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={never} now={now} />);
    await landed(f, stripShown);
    expect(screen.getAllByRole('status').map((s) => s.textContent)).toEqual([LOADING]);
    expect(screen.getByRole('status').compareDocumentPosition(part('s1')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    proseOnly();
    expect(sectionHeads()).toHaveLength(0);
    expect(screen.queryAllByRole('alert')).toHaveLength(0);
    expect(within(part('s3')).getByRole('region', STRIP)).toBeInTheDocument();
    expect(within(part('s3')).getByText(CH3_COUNT)).toBeInTheDocument();
    // the live ladder does not wait for the engine either: it is chapter 7's, with its extras line
    expect(ladderHeads().map((h) => h.closest('section.m-ch')?.id)).toEqual(['s7']);
    expect(within(part('s7')).getByText(CH7_EXTRAS)).toBeInTheDocument();
  });

  it('says one honest sentence above the story when the engine does not load: the prose stays, no drawing, no section a–f, and the live strip still draws', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.reject(new Error('blocked'))} now={now} />);
    await landed(f, () => (stripShown(), screen.getByRole('alert')));
    expect(screen.getAllByRole('alert').map((a) => a.textContent)).toEqual([ENGINE_FAILED]);
    expect(ENGINE_FAILED).toMatch(/did not load: the drawings need WebAssembly/);
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByRole('alert').compareDocumentPosition(part('s1')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    proseOnly();
    expect(sectionHeads()).toHaveLength(0);
    expect(within(part('s3')).getByRole('region', STRIP)).toBeInTheDocument();
    expect(screen.getByText(FOOTER)).toBeInTheDocument();
  });

  it.each(['#d', '#play'])('a link to %s scrolls nowhere while the engine has not loaded (#play is on the page, the sections are not)', async (hash) => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', `/math${hash}`);
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={never} now={now} />);
    await landed(f, stripShown);
    expect(part('play')).toBeInTheDocument();
    expect(scrolled).toEqual([]);
  });

  it('shows nothing live while the snapshot is still loading: no flash of "unavailable"', async () => {
    const f = vi.fn((_url: RequestInfo | URL, init?: RequestInit) => new Promise<Response>((_, reject) => init?.signal?.addEventListener('abort', () => reject(new Error('aborted')))));
    vi.stubGlobal('fetch', f);
    const { unmount } = render(<MathPage engine={never} now={now} />);
    await settle(f);
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
    expect(screen.queryByRole('region', STRIP)).toBeNull();
    expect(screen.queryByText(CH3_COUNT)).toBeNull(); // no "Today's count:" with nothing under it
    expect(ladderHeads()).toHaveLength(0);
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
    expect(h2s()).toEqual(HEADS); // nothing else changes: the story is there
    unmount();
    await landed(f, () => expect(vi.getTimerCount()).toBe(0));
    expect(vi.getTimerCount()).toBe(0);
  });

  it('a missing snapshot is one dim line in chapter 3 and one in chapter 7, and the page asks for nothing but that file', async () => {
    const f = mockApi({});
    render(<MathPage engine={never} now={now} />);
    await landed(f, unavailableShown);
    unavailableShown();
    for (const line of screen.getAllByText(LIVE_UNAVAILABLE)) expect(line).toHaveClass('dim');
    expect(within(part('s3')).getByText(CH3_COUNT)).toBeInTheDocument(); // the count line, then the line standing in for the strip
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull(); // "extras" never sits under "unavailable"
    expect(ladderHeads()).toHaveLength(0);
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
    await landed(f, stripShown);
    expect(requested(f)).toEqual(['/math-data/public.json']);
    expect(screen.getAllByRole('region', STRIP)).toHaveLength(1);
  });

  it('a snapshot older than 30 minutes says the writer is behind', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0 - FRESH_MS - 1) });
    render(<MathPage engine={never} now={now} />);
    await landed(f, behindShown);
    expect(within(stripShown()).getByText(BEHIND_LINE)).toHaveClass('stale');
    expect(within(part('s7')).getByText(BEHIND_LINE).tagName).toBe('H3');
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
  });

  it('holds no interval timer: a fresh snapshot turns stale when the visitor returns to the tab, not before', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={never} now={now} />);
    await landed(f, stripShown);
    expect(vi.getTimerCount()).toBe(0);
    expect(screen.queryByText(new RegExp(BEHIND))).toBeNull();
    clock = T0 + FRESH_MS + 1;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(FRESH_MS + 1);
    });
    expect(screen.queryByText(new RegExp(BEHIND))).toBeNull(); // nothing ticks on its own
    act(() => setVisible('visible'));
    await landed(f, behindShown);
    expect(within(stripShown()).getByText(BEHIND_LINE)).toHaveClass('stale');
    expect(requested(f)).toEqual(['/math-data/public.json', '/math-data/public.json']);
    expect(vi.getTimerCount()).toBe(0);
  });

  describe('when a live part throws while rendering', () => {
    let errors: ReturnType<typeof vi.spyOn>;
    beforeEach(() => {
      errors = vi.spyOn(console, 'error').mockImplementation(() => undefined); // React reports what a boundary caught
    });

    it('the strip and the ladder are each replaced by the "unavailable" line and the rest of the page stands', async () => {
      boom.strip = true;
      const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
      render(<MathPage engine={never} now={now} />);
      await landed(f, unavailableShown);
      expect(errors).toHaveBeenCalled();
      unavailableShown();
      expect(screen.queryByRole('region', STRIP)).toBeNull();
      expect(ladderHeads()).toHaveLength(0);
      expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
      expect(screen.getByText(HOOK)).toBeInTheDocument();
      expect(h2s()).toEqual(HEADS);
      expect(screen.getByRole('status')).toHaveTextContent(LOADING);
      expect(screen.getByText(FOOTER)).toBeInTheDocument();
    });

    it('a newer snapshot gets a fresh try', async () => {
      boom.strip = true;
      const f = mockApi({ [SNAPSHOT_URL]: () => snapshotFixture({}, clock) });
      render(<MathPage engine={never} now={now} />);
      await landed(f, unavailableShown);
      unavailableShown();
      boom.strip = false;
      clock = T0 + REFETCH_MS + 1;
      act(() => setVisible('visible'));
      await landed(f, () => (stripShown(), ladderShown()));
      expect(within(part('s3')).getByRole('region', STRIP)).toBeInTheDocument();
      expect(ladderHeads().map((h) => h.closest('section.m-ch')?.id)).toEqual(['s7']);
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

/** Rules (a) to (c) on what is on screen now, plus: no link leaves the page. `state` names it in a failure. */
function expectHonest(root: HTMLElement, state: string): void {
  const words = readable(root);
  // (a) the dashboard's list (earned, earn, made, income, profitable, a bare profit); the Rust paths the captions
  // quote are scanned like any other text
  expect(words.flatMap((s) => checkHonesty(s).map((w) => `${w}: ${s}`)), `${state}: words`).toEqual([]);
  // (b) R11: every $ figure is framed in its row or its caption
  expect(unframedMoney(root), `${state}: dollars`).toEqual([]);
  // (c) nothing on the page points at the private api, and no link leaves it
  expect(root.innerHTML, state).not.toContain('/api/');
  expect(root.querySelectorAll('a, nav'), `${state}: links`).toHaveLength(0);
  expect(root.querySelectorAll('[style]'), `${state}: style attributes`).toHaveLength(0);
  expect(root.querySelectorAll('svg text'), `${state}: svg text`).toHaveLength(0);
  expect(root.innerHTML, state).not.toContain('NaN');
  expect(document.querySelectorAll('[role~="alert"]'), `${state}: alerts`).toHaveLength(0); // by attribute: a role query walks the whole page
  expect([...root.querySelectorAll('.error')].map((e) => e.textContent), `${state}: engine sentences`).toEqual([]);
}

async function openEveryFold(root: HTMLElement, f: { mock: { calls: unknown[] } }): Promise<number> {
  let opened = 0;
  for (let round = 0; round < 5; round++) {
    const closed = [...root.querySelectorAll('details:not([open]) > summary')];
    if (closed.length === 0) break;
    for (const s of closed) fireEvent.click(s);
    opened += closed.length;
    await landed(f, () => expect(closed.every((x) => (x.parentElement as HTMLDetailsElement).open)).toBe(true));
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

  it('tells the story first (the hook, chapters 1 to 7), then the playground (heading, line, word list, a–f in order, each with its anchor); the strip is in chapter 3 and the ladder in chapter 7, not in f', async () => {
    const snap = snapshotFixture({}, T0);
    const f = mockApi({ [SNAPSHOT_URL]: snap });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    const kids = (el: Element) => [...el.children].map((k) => `${k.tagName.toLowerCase()}${k.id !== '' ? `#${k.id}` : k.className !== '' ? `.${k.className}` : ''}`);
    expect(kids(screen.getByRole('main'))).toEqual(['p.m-tag', ...STORY.map((id) => `section#${id}`), 'section#play']);
    expect(kids(part('play'))).toEqual(['h2#m-play', 'p.m-ch-lead', 'details.fold', ...IDS.map((id) => `section#${id}`)]);
    // the word list waits in a closed fold: its twelve terms are on the page, in order, one tap away
    const words = within(part('play')).getByText(WORDS_LABEL).closest('details') as HTMLDetailsElement;
    expect(words.parentElement).toBe(part('play'));
    expect(words.open).toBe(false);
    expect(words.querySelector('.fold-body')?.firstElementChild).toBe(part('play').querySelector('dl.m-words'));
    expect(WORDS_LABEL).toBe('the words used on this page');
    // chapter 3 draws the prices against the dollar line; the decision tree is section c's
    expect(part('s3').querySelectorAll('svg.ps-svg')).toHaveLength(1);
    expect(part('s3').querySelectorAll('.cl-path, .cl-node')).toHaveLength(0);
    expect(part('c').querySelectorAll('ol.cl-path > li.cl-node')).toHaveLength(14);
    expect(part('c').querySelector('.pstack')).toBeNull();
    expect(screen.getByRole('region', { name: PLAY.title })).toBe(part('play'));
    expect(within(part('play')).getByText(PLAY.line)).toHaveClass('m-ch-lead');
    expect([...part('play').querySelectorAll('dl.m-words dt')].map((t) => t.textContent)).toEqual(WORDS.map(([term]) => term));
    expect(sectionHeads().map((h) => h.textContent)).toEqual(TITLES);
    expect([...container.querySelectorAll('section.m-sec')].map((s) => s.id)).toEqual([...IDS]);
    const strip = screen.getByRole('region', STRIP);
    const first = screen.getByRole('region', { name: SECTIONS.a.title });
    expect(strip.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(part('s3').lastElementChild).toBe(strip); // under chapter 3's takeaway and its count line
    expect(strip.previousElementSibling).toHaveTextContent(CH3_COUNT);
    expect(screen.getAllByRole('region', STRIP)).toHaveLength(1);
    const s7 = part('s7');
    const sf = screen.getByRole('region', { name: SECTIONS.f.title });
    expect(within(s7).getByRole('heading', { name: /^on the live scanner/ })).toHaveTextContent(/^on the live scanner, as of \d\d:\d\d UTC$/);
    expect(kids(s7).slice(2)).toEqual(['div.m-liveladder', 'p.dim m-note', 'p.m-ch-take']); // under the lead: the ladder, the extras line, the fixed line
    expect(within(s7).getByText(CH7_EXTRAS)).toBeInTheDocument();
    expect(s7.lastElementChild).toHaveTextContent(TAKE.ch7);
    expect(sf.querySelector('.m-liveladder')).toBeNull();
    expect(sf.lastElementChild).toHaveClass('m-cap'); // f ends with its own caption
    expect(container.querySelectorAll('.m-liveladder')).toHaveLength(1);
    expect(within(s7).getByText(breakEvenText(ladderModel(publicToLadder(snap.ladder))).big)).toBeInTheDocument();
    expect(ladderHeads()).toHaveLength(1);
    expect(screen.queryByText(LIVE_UNAVAILABLE)).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
    noErrors();
    expect(container.querySelectorAll('[style]')).toHaveLength(0); // the math CSP has no 'unsafe-inline' for styles
    expect(container.querySelectorAll('svg text')).toHaveLength(0);
    expect(container.innerHTML).not.toContain('NaN');
    expect(requested(f)).toEqual(['/math-data/public.json']);
  });

  it('a stale snapshot says so in the strip (chapter 3) and above the ladder (chapter 7)', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0 - FRESH_MS - 1) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (sectionsShown(), behindShown()));
    const behind = screen.getAllByText(BEHIND_LINE);
    expect(behind).toHaveLength(2);
    expect(within(stripShown()).getByText(BEHIND_LINE)).toHaveClass('stale');
    expect(within(part('s7')).getByText(BEHIND_LINE)).toBe(ladderHeads()[0]);
    expect(behind.some((el) => el.tagName === 'H3')).toBe(true);
    expect(sectionHeads()).toHaveLength(6);
  });

  it('without the snapshot, chapters 3 and 7 say so, and every chapter and section still works', async () => {
    const f = mockApi({});
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (sectionsShown(), unavailableShown()));
    expect(sectionHeads()).toHaveLength(6);
    expect(ladderHeads()).toHaveLength(0);
    unavailableShown();
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
    for (const id of STORY.slice(0, 6)) expect(pressed(id), id).toEqual(['true', 'false']);
    expect([...document.querySelectorAll('.m-ch-take')].map((p) => p.closest('section')?.id)).toEqual(STORY);
    noErrors();
    for (const k of IDS) expect(screen.getByText(SECTIONS[k].caption)).toBeInTheDocument();
  });

  it('when the live ladder throws, the strip goes too: the "unavailable" line in chapters 3 and 7, six working sections', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    boom.ladder = true;
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (sectionsShown(), unavailableShown()));
    expect(errors).toHaveBeenCalled();
    unavailableShown();
    expect(screen.queryByRole('region', STRIP)).toBeNull();
    expect(ladderHeads()).toHaveLength(0);
    expect(screen.queryByText(CH7_EXTRAS)).toBeNull();
    expect(sectionHeads().map((h) => h.textContent)).toEqual(TITLES);
    for (const k of IDS) expect(screen.getByText(SECTIONS[k].caption)).toBeInTheDocument();
    noErrors();
  });

  it('the whole page, every fold open, on the sample the api writes: honest words, framed dollars, no /api/', async () => {
    const sample = parseSnapshot(JSON.parse(sampleText));
    if (sample === null) throw new Error('the sample must parse');
    clock = sample.written_ms + 60_000;
    const f = mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    expect(await openEveryFold(container, f)).toBeGreaterThanOrEqual(7); // chapter 1's levels, the ladder's two, the word list, a's two, d's levels
    expect((within(part('play')).getByText(WORDS_LABEL).closest('details') as HTMLDetailsElement).open).toBe(true);
    expect(localStorage.getItem('fold:math:words')).toBe('1');
    expect(sectionHeads()).toHaveLength(6);
    expect(screen.getByRole('region', STRIP)).toBeInTheDocument();
    expect(ladderHeads()).toHaveLength(1);
    expect(readable(container).filter((s) => /\$\d/.test(s)).length).toBeGreaterThan(10); // the scan looks at real figures
    expectHonest(container, 'the starting state');
    expect(requested(f)).toEqual(['/math-data/public.json']);
    // (d) scrollWidth has no meaning in jsdom: the structural rules are pinned in theme.test.ts and math.css.test.ts
  });

  it('stays honest in the states a visitor reaches: crossed book, each basket kind with prices, fees on, the usual burst', async () => {
    vi.useRealTimers(); // the controls redraw on an animation frame: every wait below is a waitFor/findBy on an exact expectation
    // A slow frame (50 ms, like a busy CI runner): an assertion that reads the output before its frame has landed fails here, every time.
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 50));
    vi.stubGlobal('cancelAnimationFrame', (id: number) => clearTimeout(id));
    const sent: { fn: EngineFn; input: unknown }[] = [];
    const spied = Object.fromEntries(ENGINE_FNS.map((fn) => [fn, (i: string) => (sent.push({ fn, input: JSON.parse(i) }), engine[fn](i))])) as Engine;
    const last = <T,>(fn: EngineFn) => sent.filter((x) => x.fn === fn).at(-1)?.input as T;
    const sample = parseSnapshot(JSON.parse(sampleText));
    if (sample === null) throw new Error('the sample must parse');
    clock = sample.written_ms + 60_000;
    mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    const { container } = render(<MathPage engine={Promise.resolve(spied)} now={now} />);
    await screen.findByRole('region', STRIP);
    await screen.findByRole('region', { name: SECTIONS.f.title });
    const s7 = within(part('s7'));
    await s7.findByRole('heading', { name: /^on the live scanner/ });
    await waitFor(() => expect(sectionHeads()).toHaveLength(6));
    // Each section is found by its accessible name, once: a role query walks the whole page, which is twice as long now.
    const regions = new Map(IDS.map((k) => [k, screen.getByRole('region', { name: SECTIONS[k].title })]));
    const sec = (k: (typeof IDS)[number]) => within(regions.get(k) as HTMLElement);
    for (const s of container.querySelectorAll('details:not([open]) > summary')) fireEvent.click(s);
    await waitFor(() => expect(container.querySelectorAll('details:not([open])')).toHaveLength(0));

    // chapter 7's live ladder, "which markets": a tapped cell with a mean writes a $ line under the grid
    const row = sample.ladder.markets.find((r) => r.mean !== null);
    if (row === undefined) throw new Error('the sample must carry a market row with a mean');
    expect(part('f').querySelectorAll('.lg-cell')).toHaveLength(0); // no market cell is left in f
    const cell = s7.getByRole('button', { name: new RegExp(`^${bucketText(row.dimension, row.bucket)} at ${row.scenario} `) });
    fireEvent.click(cell);
    expect(cell).toHaveAttribute('aria-pressed', 'true');
    const detail = cellDetail({ dim: row.dimension, bucket: row.bucket }, { scenario: row.scenario, latency_ms: row.latency_ms ?? 0 }, row);
    expect(detail).toMatch(/\$\d/); // the line carries a real figure
    expect(s7.getByText(detail)).toHaveClass('lg-detail');
    expectHonest(container, 'chapter 7: a market cell with a mean');

    fireEvent.click(sec('a').getByRole('button', { name: 'cross the book' }));
    expect(await sec('a').findByText(CROSSED)).toBeInTheDocument();
    expectHonest(container, 'a: crossed book');

    fireEvent.click(sec('b').getByLabelText('with prices'));
    for (const [button, kind, table] of [
      ['YES basket', 'yes', 'payoff of the YES basket'],
      ['NO basket', 'no', 'payoff of the NO basket'],
      ['implication A ⇒ B', 'implication', 'payoff of the implication'],
    ] as const) {
      fireEvent.click(sec('b').getByRole('button', { name: button }));
      // One expectation on the last request: the kind AND the prices. "YES basket" is already the kind at the start,
      // so waiting for the kind alone is satisfied by the first, unpriced request (the CI race).
      await waitFor(() => expect(last<PayoffIn>('payoff_json')).toMatchObject({ kind, prices: expect.any(Array) }));
      expect(await sec('b').findByRole('table', { name: table })).toBeInTheDocument();
      expect(await sec('b').findByText(/ =$/)).toBeInTheDocument(); // the formula line with the engine's edge is on screen
      expectHonest(container, `b: ${kind} with prices`);
    }

    fireEvent.click(sec('d').getByLabelText(/taker fees on/));
    const walked = JSON.parse(engine.depth_walk_json(JSON.stringify(depthInput({ ...DEPTH_START, fees: true })))) as WalkModel;
    await waitFor(() => expect(last<DepthWalkIn>('depth_walk_json')).toEqual(depthInput({ ...DEPTH_START, fees: true })));
    expect(await sec('d').findByText(`Q* ${walked.q_star}`)).toBeInTheDocument();
    expectHonest(container, 'd: fees on');

    fireEvent.click(sec('f').getByRole('button', { name: 'the usual burst' }));
    await waitFor(() => expect(last<BasketIn>('basket_json')).toEqual(basketInput(F_BOOKS, F_BURST)));
    expectHonest(container, 'f: the usual burst');
    fireEvent.click(sec('f').getByLabelText(/taker fees on/));
    await waitFor(() => expect(last<BasketIn>('basket_json')).toEqual(basketInput(F_BOOKS, { ...F_BURST, fees: true })));
    expectHonest(container, 'f: the usual burst with fees');

    // and all of it again with every chapter on its second state
    for (const id of STORY.slice(0, 6)) fireEvent.click(switchOf(id)[1]);
    for (const id of STORY.slice(0, 6)) expect(pressed(id), id).toEqual(['false', 'true']);
    expectHonest(container, 'the playground moved, the story on its second states');
  }, 15_000); // real 50 ms frames, a dozen waits and nine scans of a page twice as long: 1.5 s alone, near 4 s beside the whole suite

  it('a link to a section (/math#d) is scrolled to once the engine is ready and the snapshot has settled, once, with the default behaviour', async () => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', '/math#d');
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(
      <StrictMode>
        <MathPage engine={Promise.resolve(engine)} now={now} />
      </StrictMode>,
    );
    await landed(f, () => expect(scrolled).toEqual(['d(0)']));
    expect(scrolled).toEqual(['d(0)']); // no argument: the browser's default, so reduced motion is the visitor's setting
    clock = T0 + FRESH_MS + 1;
    act(() => setVisible('visible')); // the page draws again
    await landed(f, behindShown);
    expect(scrolled).toEqual(['d(0)']);
  });

  it('review focus 2: on a cold /math#play with a late snapshot, the scroll waits for the snapshot', async () => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', '/math#play');
    let answer: (r: Response) => void = () => undefined;
    const f = vi.fn((_url: RequestInfo | URL, _init?: RequestInit) => new Promise<Response>((resolve) => (answer = resolve)));
    vi.stubGlobal('fetch', f);
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, sectionsShown);
    expect(sectionHeads()).toHaveLength(6); // the engine is ready and the target is on the page,
    expect(screen.queryByRole('region', STRIP)).toBeNull(); // the strip and the ladder are not: they would push it away
    expect(ladderHeads()).toHaveLength(0);
    expect(scrolled).toEqual([]);
    await act(async () => answer(jsonResponse(200, snapshotFixture({}, T0))));
    await landed(f, () => (stripShown(), ladderShown(), expect(scrolled).toHaveLength(1)));
    expect(ladderHeads()[0].compareDocumentPosition(part('play')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy(); // the ladder is above the target
    expect(scrolled).toEqual(['play(0)']);
    clock = T0 + FRESH_MS + 1;
    act(() => setVisible('visible'));
    await landed(f, behindShown);
    expect(scrolled).toEqual(['play(0)']); // once
  });

  it('a snapshot that is missing has settled too: /math#f scrolls once the "unavailable" lines stand', async () => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', '/math#f');
    const f = mockApi({});
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (sectionsShown(), unavailableShown(), expect(scrolled).toHaveLength(1)));
    expect(scrolled).toEqual(['f(0)']);
  });

  it('the snapshot first, the engine late: /math#d scrolls when the sections arrive', async () => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', '/math#d');
    let ready: (e: Engine) => void = () => undefined;
    const late = new Promise<Engine>((resolve) => (ready = resolve));
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={late} now={now} />);
    await landed(f, () => (stripShown(), ladderShown()));
    expect(scrolled).toEqual([]);
    await act(async () => ready(engine));
    await landed(f, () => (sectionsShown(), expect(scrolled).toHaveLength(1)));
    expect(scrolled).toEqual(['d(0)']);
  });

  it.each(['#m-d', '#s3', '#m-play', '#playground'])('a hash that is not a section or the playground (%s) scrolls nowhere', async (hash) => {
    const scrolled = recordScrolls();
    window.history.replaceState(null, '', `/math${hash}`);
    const f = mockApi({});
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (sectionsShown(), unavailableShown()));
    expect(sectionHeads()).toHaveLength(6);
    expect(scrolled).toEqual([]);
  });

  it('one h1; chapters and the playground are h2; sections are h3', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
    expect(screen.getAllByRole('heading', { level: 2 })).toHaveLength(8);
    expect(h2s()).toEqual(HEADS);
    expect(screen.getAllByRole('heading', { level: 3 }).length).toBeGreaterThanOrEqual(6);
    expect(h3s().map((h) => h.textContent)).toEqual([ladderHeads()[0].textContent, ...TITLES]); // the ladder's heading (chapter 7), then a–f
    expect(screen.queryAllByRole('heading', { level: 4 })).toHaveLength(0);
  });

  it('ruling R5: no id repeats in the document, and the story\'s "every level" fold and section a\'s remember their state apart', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    const ids = [...document.querySelectorAll('[id]')].map((el) => el.id);
    expect(ids).toEqual(expect.arrayContaining([...STORY, ...STORY.map((id) => `m-${id}`), 'play', 'm-play', ...IDS, ...IDS.map((id) => `m-${id}`)]));
    expect(ids.filter((id, i) => ids.indexOf(id) !== i)).toEqual([]);
    const fold = (id: string) => within(part(id)).getByText('every level').closest('details') as HTMLDetailsElement;
    expect([fold('s1').open, fold('a').open]).toEqual([false, false]);
    fireEvent.click(within(part('s1')).getByText('every level'));
    await landed(f, () => expect(fold('s1').open).toBe(true));
    expect(fold('a').open).toBe(false);
    expect(localStorage.getItem('fold:story:mirror-levels')).toBe('1');
    expect(localStorage.getItem('fold:mirror-levels')).toBeNull();
    // R6: a second render of the page only after the first is gone (jsdom answers a repeated id with its first match)
    cleanup();
    render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    expect([fold('s1').open, fold('a').open]).toEqual([true, false]); // remembered for the story, not for a
  });

  it('review focus 4: a day with no episodes and an epoch with no rungs still reads true', async () => {
    const base = snapshotFixture({}, T0);
    // what the api writes for an epoch with no rung: break_even "thin" (cards::ladder::break_even on an empty ladder)
    const empty: PublicSnapshot = {
      ...base,
      ladder: { ...base.ladder, rungs: [], markets: [], break_even: { kind: 'thin' } },
      today: { ...base.today, arb: 0, no: 0, yes: 0, edge_budget100: '0', hourly: base.today.hourly.map((h) => ({ ...h, arb: 0 })) },
    };
    const f = mockApi({ [SNAPSHOT_URL]: empty });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    const s3 = within(part('s3'));
    expect(s3.getByText(CH3_COUNT)).toBeInTheDocument();
    expect([...stripShown().querySelectorAll('.m-big')].slice(0, 2).map((b) => b.textContent)).toEqual(['0', '0']);
    expect(stripShown()).toHaveTextContent(/on the table/);
    const s7 = within(part('s7'));
    expect(s7.getByText(NO_RUNGS)).toBeInTheDocument();
    expect(s7.getByText(breakEvenText(ladderModel(publicToLadder(empty.ladder))).big)).toHaveClass('ll-big');
    expect(part('s7').querySelectorAll('.ll-svg .ll-seg, .ll-svg .ll-net')).toHaveLength(0); // nothing is drawn as a result
    expect(part('s7').lastElementChild).toHaveTextContent(TAKE.ch7);
    expect(sectionHeads()).toHaveLength(6);
    noErrors();
    expectHonest(container, 'an empty day');
  });

  it('the story is honest in both states of every chapter', async () => {
    const sample = parseSnapshot(JSON.parse(sampleText));
    if (sample === null) throw new Error('the sample must parse');
    clock = sample.written_ms + 60_000;
    const f = mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    const { container } = render(<MathPage engine={Promise.resolve(engine)} now={now} />);
    await landed(f, () => (stripShown(), sectionsShown(), ladderShown()));
    await openEveryFold(container, f);
    const takes = () => STORY.map((id) => part(id).querySelector('.m-ch-take')?.textContent);
    const first = takes();
    for (const id of STORY.slice(0, 6)) expect(pressed(id), id).toEqual(['true', 'false']);
    expect(first.every((t) => typeof t === 'string' && t !== '')).toBe(true);
    const dollars = () => STORY.map((id) => readable(part(id)).filter((s) => /\$\d/.test(s)).length);
    expect(dollars()[6]).toBeGreaterThan(0); // the scan looks at real figures: chapter 7's are the snapshot's
    expectHonest(container, 'story: first states');
    for (const id of STORY.slice(0, 6)) fireEvent.click(switchOf(id)[1]);
    for (const id of STORY.slice(0, 6)) expect(pressed(id), id).toEqual(['false', 'true']);
    const second = takes();
    // every chapter with two lines changed its line (chapter 4's is one line for both states, chapter 7's is fixed)
    expect(STORY.filter((_, i) => second[i] !== first[i])).toEqual(['s1', 's2', 's3', 's5', 's6']);
    expect(dollars()[2]).toBeGreaterThan(0); // chapter 3's edge, on the table
    expectHonest(container, 'story: second states');
    expect(sectionHeads()).toHaveLength(6); // the playground did not move
  });
});
