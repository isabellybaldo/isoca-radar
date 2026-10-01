import { render, screen } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import sampleText from './__fixtures__/public.sample.json?raw';
import { snapshotFixture } from './fixtures';
import { LiveLadder } from './LiveLadder';
import { BEHIND, LIVE_UNAVAILABLE, LiveStrip, LiveUnavailable, asOfLine } from './LiveStrip';
import { FRESH_MS, REFETCH_MS, SNAPSHOT_URL, freshness, parseSnapshot } from './snapshot';
import { MAX_BODY_BYTES, type SnapshotLoad, useSnapshot } from './useSnapshot';

const T0 = 1_790_000_000_000;
let clock = T0;
let last: SnapshotLoad = { kind: 'loading' };
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
  last = { kind: 'loading' };
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
    expect(vi.getTimerCount()).toBe(0);
  });

  it('reads the realistic sample the api writes', async () => {
    const f = mockApi({ [SNAPSHOT_URL]: () => new Response(sampleText, { status: 200 }) });
    render(<Probe />);
    await settle(f);
    expect(last).toEqual({ kind: 'ok', snap: parseSnapshot(JSON.parse(sampleText)) });
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
      last = { kind: 'loading' };
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
    const before = last;
    expect(before.kind).toBe('ok');
    body = () => jsonResponse(404, {});
    clock = T0 + REFETCH_MS + 1;
    setVisible('visible');
    await settle(f);
    expect(f).toHaveBeenCalledTimes(2);
    expect(last).toBe(before);
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
    expect(screen.getByText('edge on the table')).toBeInTheDocument();
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
    expect(screen.getByText('≈ 70 ms')).toBeInTheDocument();
  });

  it('says it is behind when stale, and draws the same ladder', () => {
    const s = snapshotFixture({}, Date.UTC(2026, 8, 30, 11, 0));
    render(<LiveLadder snap={s} stale />);
    expect(screen.getByRole('heading', { name: `on the live scanner, as of 2026-09-30 11:00 UTC: ${BEHIND}` })).toBeInTheDocument();
    expect(screen.getByText('≈ 70 ms')).toBeInTheDocument();
  });

  it('draws the sample the api writes with one heading of its own', () => {
    const snap = parseSnapshot(JSON.parse(sampleText));
    if (snap === null) throw new Error('sample must parse');
    render(<LiveLadder snap={snap} stale={false} />);
    expect(screen.getAllByRole('heading', { name: /^on the live scanner/ })).toHaveLength(1);
  });
});
