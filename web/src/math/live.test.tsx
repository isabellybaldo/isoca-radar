import { act, render, screen } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import sampleText from './__fixtures__/public.sample.json?raw';
import { snapshotFixture } from './fixtures';
import { LiveLadder } from './LiveLadder';
import { BEHIND, LIVE_UNAVAILABLE, LiveStrip, LiveUnavailable, STRIP_TITLE, asOfLine } from './LiveStrip';
import { FRESH_MS, MAX_MS, REFETCH_MS, SNAPSHOT_URL, freshness, parseSnapshot } from './snapshot';
import { NO_ACCOUNT_LEGEND, NO_RUNGS } from '../schematics/LatencyLadder';
import { FIRST_TIMEOUT_MS, MAX_BODY_BYTES, type SnapshotState, useSnapshot } from './useSnapshot';

const T0 = 1_790_000_000_000;
let clock = T0;
let last: SnapshotState = { kind: 'loading', checkedAt: 0 };
function Probe() {
  last = useSnapshot(() => clock);
  return null;
}
function setVisible(v: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => v });
  document.dispatchEvent(new Event('visibilitychange'));
}
beforeEach(() => {
  clock = T0;
  last = { kind: 'loading', checkedAt: 0 };
  // Timers are faked so settle() flushes by advancing them, never by sleeping; Date stays real (the page's
  // clock is the injected one) and Response streams stay on real ticks.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(document, 'visibilityState');
});

describe('useSnapshot (spec 7b §6.3)', () => {
  it('fetches the one public file once on load, and nothing else, with no timer left running', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<Probe />);
    expect(last.kind).toBe('loading');
    await settle(f);
    expect(last.kind).toBe('ok');
    expect(f.mock.calls.map((c) => String(c[0]))).toEqual(['/math-data/public.json']);
    expect(vi.getTimerCount()).toBe(0); // the first request's 10 s limit is cleared once it settles
  });

  it('reads the realistic sample the api writes', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    render(<Probe />);
    await settle(f);
    expect(last).toEqual({ kind: 'ok', snap: parseSnapshot(JSON.parse(sampleText)), checkedAt: T0 });
  });

  it('StrictMode mounts twice and still sends one request that updates state', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(
      <StrictMode>
        <Probe />
      </StrictMode>,
    );
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    expect(last.kind).toBe('ok');
  });

  it('a mount that leaves aborts its request and sets no state', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    const { unmount } = render(<Probe />);
    unmount();
    await settle(f);
    const init = f.mock.calls[0][1] as RequestInit;
    expect(init.signal?.aborted).toBe(true);
    expect(last.kind).toBe('loading');
  });

  it('404, a network error, an HTML page, bad JSON, v 2, a 203 and a 70 KB body all read missing', async () => {
    const big = JSON.stringify({ ...snapshotFixture({}, T0), pad: 'x'.repeat(70_000) });
    expect(big.length).toBeGreaterThan(MAX_BODY_BYTES);
    const answers: (() => ReturnType<typeof mockApi>)[] = [
      () => mockApi({}),
      () => {
        const f = vi.fn(async () => {
          throw new TypeError('Failed to fetch');
        });
        vi.stubGlobal('fetch', f);
        return f as unknown as ReturnType<typeof mockApi>;
      },
      () => mockApi({ [SNAPSHOT_URL]: () => new Response('<html>502</html>', { status: 200 }) }),
      () => mockApi({ [SNAPSHOT_URL]: () => new Response('{"v":1,', { status: 200 }) }),
      () => mockApi({ [SNAPSHOT_URL]: { ...snapshotFixture({}, T0), v: 2 } }),
      () => mockApi({ [SNAPSHOT_URL]: () => new Response(big, { status: 200 }) }),
      () => mockApi({ [SNAPSHOT_URL]: () => new Response(JSON.stringify(snapshotFixture({}, T0)), { status: 203 }) }),
    ];
    for (const answer of answers) {
      last = { kind: 'loading', checkedAt: 0 };
      const f = answer();
      const { unmount } = render(<Probe />);
      await settle(f);
      expect(last.kind).toBe('missing');
      unmount();
    }
  });

  it('re-fetches only when the tab comes back and the last attempt is over 5 minutes old', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<Probe />);
    await settle(f);
    clock = T0 + REFETCH_MS - 1000;
    setVisible('hidden');
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    clock = T0 + REFETCH_MS + 1000;
    setVisible('hidden');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    // the attempt just made restarts the five minutes
    clock += 1000;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
  });

  it('a failed re-fetch keeps the last good snapshot, and a good one replaces it', async () => {
    let body: () => Response = () => jsonResponse(200, snapshotFixture({}, T0));
    const f = mockApi({ [SNAPSHOT_URL]: () => body() });
    render(<Probe />);
    await settle(f);
    const before = last.kind === 'ok' ? last.snap : null;
    expect(before).not.toBeNull();
    body = () => jsonResponse(404, {});
    clock = T0 + REFETCH_MS + 1;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    expect(last.kind === 'ok' ? last.snap : null).toBe(before);
    const second = clock + REFETCH_MS + 1;
    body = () => jsonResponse(200, snapshotFixture({}, second));
    clock = second;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(3);
    expect(last.kind === 'ok' && last.snap.written_ms).toBe(second);
  });

  it('removes its visibility listener on unmount', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    const { unmount } = render(<Probe />);
    await settle(f);
    unmount();
    clock = T0 + REFETCH_MS + 1;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
  });
});

const staleNow = () => (last.kind === 'ok' ? freshness(last.snap, last.checkedAt) : null);

describe('useSnapshot: ageing, timeout, order, failed attempts (fix round 1)', () => {
  it('a kept snapshot ages into stale on a tab return, with no timer: one re-fetch, which fails, keeps it', async () => {
    let up = true;
    const f = mockApi({ [SNAPSHOT_URL]: () => (up ? jsonResponse(200, snapshotFixture({}, T0)) : jsonResponse(404, {})) });
    render(<Probe />);
    await settle(f);
    expect(staleNow()).toBe('fresh');
    const before = last.kind === 'ok' ? last.snap : null;
    up = false;
    clock = T0 + 31 * 60_000;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    expect(last.kind === 'ok' ? last.snap : null).toBe(before);
    expect(last.checkedAt).toBe(clock);
    expect(staleNow()).toBe('stale');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('a tab return inside the 5 minutes changes checkedAt without a fetch', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: snapshotFixture({}, T0) });
    render(<Probe />);
    await settle(f);
    const before = last;
    clock = T0 + 60_000;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    expect(last).not.toBe(before);
    expect(last.checkedAt).toBe(clock);
  });

  it('the first request that has not settled after 10 s is aborted and reads missing, and its timer is gone', async () => {
    const f = vi.fn(
      (_u: unknown, init?: RequestInit) =>
        new Promise<Response>((_res, rej) => {
          init?.signal?.addEventListener('abort', () => rej(new DOMException('aborted', 'AbortError')));
        }),
    );
    vi.stubGlobal('fetch', f);
    render(<Probe />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(FIRST_TIMEOUT_MS - 1);
    });
    expect(last.kind).toBe('loading');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(last.kind).toBe('missing');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('a late first response never replaces a newer snapshot from a re-fetch', async () => {
    let release: (r: Response) => void = () => undefined;
    let n = 0;
    const f = vi.fn(async () => {
      n += 1;
      if (n === 1) return new Promise<Response>((res) => (release = res));
      return jsonResponse(200, snapshotFixture({}, T0 + 100));
    });
    vi.stubGlobal('fetch', f);
    render(<Probe />);
    clock = T0 + REFETCH_MS + 1;
    setVisible('visible');
    await settle(f);
    expect(last.kind === 'ok' && last.snap.written_ms).toBe(T0 + 100);
    release(jsonResponse(200, snapshotFixture({}, T0)));
    await settle(f);
    expect(last.kind === 'ok' && last.snap.written_ms).toBe(T0 + 100);
  });

  it('a failed first load restarts the 5 minutes, and so does a failed re-fetch', async () => {
    const f = mockApi({});
    render(<Probe />);
    await settle(f);
    expect(last.kind).toBe('missing');
    clock = T0 + 1000;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    clock = T0 + REFETCH_MS + 1;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    clock += 1000;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
  });
});

describe('the body cap (65,536 bytes)', () => {
  const snapJson = () => JSON.stringify(snapshotFixture({}, T0));
  const padded = (bytes: number) => snapJson() + ' '.repeat(bytes - new TextEncoder().encode(snapJson()).length);

  async function read(body: () => Response): Promise<string> {
    last = { kind: 'loading', checkedAt: 0 };
    const f = mockApi({ [SNAPSHOT_URL]: body });
    const { unmount } = render(<Probe />);
    await settle(f);
    const k = last.kind;
    unmount();
    return k;
  }

  it('accepts exactly 65,536 bytes and refuses 65,537', async () => {
    expect(new TextEncoder().encode(padded(MAX_BODY_BYTES)).length).toBe(MAX_BODY_BYTES);
    expect(await read(() => new Response(padded(MAX_BODY_BYTES), { status: 200 }))).toBe('ok');
    expect(await read(() => new Response(padded(MAX_BODY_BYTES + 1), { status: 200 }))).toBe('missing');
  });

  it('counts bytes, not characters: 40,000 two-byte characters are under the cap as text and over it as bytes', async () => {
    const body = JSON.stringify({ ...snapshotFixture({}, T0), pad: 'é'.repeat(40_000) });
    expect(body.length).toBeLessThan(MAX_BODY_BYTES);
    expect(new TextEncoder().encode(body).length).toBeGreaterThan(MAX_BODY_BYTES);
    expect(await read(() => new Response(body, { status: 200 }))).toBe('missing');
  });

  it('stops reading a multi-chunk stream past the cap and cancels it once', async () => {
    const cancel = vi.fn();
    let pulls = 0;
    const stream = new ReadableStream<Uint8Array>({
      pull(c) {
        pulls += 1;
        c.enqueue(new Uint8Array(30_000).fill(32));
      },
      cancel,
    });
    expect(await read(() => new Response(stream, { status: 200 }))).toBe('missing');
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(pulls).toBeLessThan(6);
  });

  it('a 200 with no body reads missing', async () => {
    expect(await read(() => new Response(null, { status: 200 }))).toBe('missing');
  });
});

describe('freshness on the injected clock', () => {
  it('fresh within 30 minutes, stale at +31, and a far-future stamp is stale', () => {
    const s = snapshotFixture({}, T0);
    expect(freshness(s, T0 + FRESH_MS)).toBe('fresh');
    expect(freshness(s, T0 + 31 * 60_000)).toBe('stale');
    expect(freshness(snapshotFixture({}, T0 + 24 * 3_600_000), T0)).toBe('stale');
  });
});

describe('LiveStrip (spec 7b §6.1)', () => {
  it('draws today: the NO and YES counts, the edge on the table, the hours, as of HH:MM UTC', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 14, 5));
    const { container } = render(<LiveStrip today={s.today} writtenMs={s.written_ms} stale={false} />);
    expect(screen.getByText('12')).toHaveClass('m-big');
    expect(screen.getByText('3')).toHaveClass('m-big');
    expect(screen.getByText('$41.37')).toBeInTheDocument();
    expect(screen.getByText('edge on the table today, $100 per episode at most')).toBeInTheDocument();
    // "today" is the UTC day (the api's day_start_ms), and the strip's heading and accessible name say so
    expect(STRIP_TITLE).toBe('today (UTC) on the live scanner');
    expect(new Date(s.today.day_start_ms).toISOString()).toMatch(/T00:00:00\.000Z$/);
    expect(screen.getByText(STRIP_TITLE)).toHaveClass('m-live-k');
    expect(screen.getByRole('region', { name: STRIP_TITLE })).toBe(container.querySelector('section.m-live'));
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

  it('asOfLine reads the time fresh, and the day and the writer behind when stale', () => {
    expect(asOfLine(Date.UTC(2026, 8, 30, 14, 5), false)).toBe('as of 14:05 UTC');
    expect(asOfLine(Date.UTC(2026, 8, 30, 11, 0), true)).toBe('as of 2026-09-30 11:00 UTC: the live writer is behind');
  });

  it('the missing line is one dim sentence', () => {
    render(<LiveUnavailable />);
    expect(screen.getByText(LIVE_UNAVAILABLE)).toHaveClass('dim');
  });
});

describe('LiveLadder (spec 7b §6.2 f)', () => {
  it('heads the live ladder with its time and draws it from the snapshot', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 14, 5));
    render(<LiveLadder snap={s} stale={false} />);
    expect(screen.getByRole('heading', { name: 'on the live scanner, as of 14:05 UTC' })).toBeInTheDocument();
    expect(screen.getByText('52 to 100 ms')).toBeInTheDocument();
  });

  it('says it is behind when stale, and draws the same ladder', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 11, 0));
    render(<LiveLadder snap={s} stale />);
    expect(screen.getByRole('heading', { name: `on the live scanner, as of 2026-09-30 11:00 UTC: ${BEHIND}` })).toBeInTheDocument();
    expect(screen.getByText('52 to 100 ms')).toBeInTheDocument();
  });

  it('draws the sample the api writes with one heading of its own', () => {
    const snap = parseSnapshot(JSON.parse(sampleText));
    if (snap === null) throw new Error('sample must parse');
    render(<LiveLadder snap={snap} stale={false} />);
    expect(screen.getAllByRole('heading', { name: /^on the live scanner/ })).toHaveLength(1);
  });
});

describe('the page cannot be blanked by a stamp (fix round 1)', () => {
  it('the largest accepted written_ms, start_ms and as_of_ms render in the strip and the ladder', () => {
    const base = snapshotFixture({}, MAX_MS);
    const snap = parseSnapshot(
      JSON.parse(
        JSON.stringify({
          ...base,
          ladder: { ...base.ladder, start_ms: MAX_MS, as_of_ms: MAX_MS },
          today: { ...base.today, day_start_ms: MAX_MS },
        }),
      ),
    );
    if (snap === null) throw new Error('the largest Date stamp must parse');
    expect(() => render(<LiveStrip today={snap.today} writtenMs={snap.written_ms} stale />)).not.toThrow();
    expect(() => render(<LiveLadder snap={snap} stale />)).not.toThrow();
  });

  it('the bound is the end of year 9999, so "as of" keeps its shape (a later Date prints +010000-01-01)', () => {
    expect(MAX_MS).toBe(Date.UTC(9999, 11, 31, 23, 59, 59, 999));
    expect(asOfLine(MAX_MS, true)).toBe(`as of 9999-12-31 23:59 UTC: ${BEHIND}`);
    expect(asOfLine(MAX_MS, false)).toBe('as of 23:59 UTC');
    expect(parseSnapshot(JSON.parse(JSON.stringify(snapshotFixture({}, MAX_MS))))?.written_ms).toBe(MAX_MS);
    expect(parseSnapshot(JSON.parse(JSON.stringify(snapshotFixture({}, MAX_MS + 1))))).toBeNull();
  });

  it('LiveLadder passes the ladder its empty states through: epoch 0, no rungs, markets missing', () => {
    const base = snapshotFixture({}, T0);
    const snap = {
      ...base,
      ladder: { ...base.ladder, epoch: 0, start_ms: null, as_of_ms: null, rungs: [], markets: [], markets_status: 'missing' as const, measured: null, break_even: { kind: 'thin' as const } },
    };
    const { container } = render(<LiveLadder snap={snap} stale={false} />);
    expect(screen.getByText(NO_RUNGS)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^on the live scanner/ })).toBeInTheDocument();
    // no epoch yet: six empty slots, and the legend does not say "no account at this latency" of the whole ladder
    expect(screen.getByText('no epoch yet')).toHaveClass('ll-big');
    expect(container.querySelectorAll('rect.ll-missing')).toHaveLength(6);
    expect(screen.queryByText(NO_ACCOUNT_LEGEND)).toBeNull();
  });

  it('LiveLadder explains the dashed box when one rung has an account and another has none', () => {
    const base = snapshotFixture({}, T0);
    const cut = base.ladder.rungs.filter((r) => r.scenario !== 'rung-100');
    expect(cut).toHaveLength(base.ladder.rungs.length - 1);
    const snap = { ...base, ladder: { ...base.ladder, rungs: cut, break_even: { kind: 'thin' as const } } };
    const { container } = render(<LiveLadder snap={snap} stale={false} />);
    expect(container.querySelectorAll('rect.ll-missing')).toHaveLength(1);
    expect(screen.getByText(NO_ACCOUNT_LEGEND)).toBeInTheDocument();
  });
});
