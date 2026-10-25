// Task 2 of the frozen site: /math built with VITE_FROZEN=1 reads the committed snapshot once and never says
// behind, live or private. Timers are faked and settle() flushes them, as in MathPage.test.tsx.
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FROZEN_LINE, frozenOn } from '../frozenMode';
import { jsonResponse, mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { describeWasm, loadTestEngine } from '../wasm/testEngine';
import type { Engine } from '../wasm/types';
import frozenText from '../../frozen/public.json?raw';
import { FOOTER, FOOTER_FROZEN, SECTIONS, F_CAPTION_FROZEN } from './copy';
import { BEHIND, LIVE_UNAVAILABLE } from './LiveStrip';
import { MathPage } from './MathPage';
import { MARKETS_WARMING_FROZEN } from '../schematics/LatencyLadder';
import { snapshotUrl, SNAPSHOT_URL } from './snapshot';
import { CHAPTERS, CH3_COUNT, CH7_LEAD_FROZEN, HOOK, HOOK_FROZEN } from './story/copy';

const FROZEN_URL = '/frozen/public.json';
const NOW = Date.UTC(2027, 0, 1);
const never = new Promise<Engine>(() => {});
const OLD_WORDS = ['warming: part of', 'on the live scanner', 'live paper accounts', 'The live dashboard is private'];

beforeEach(() => {
  vi.stubEnv('VITE_FROZEN', '1');
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  vi.spyOn(Date, 'now').mockReturnValue(NOW);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(document, 'visibilityState');
});

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
const pageText = () => document.body.textContent ?? '';

describe('/math in frozen mode', () => {
  it('snapshotUrl is the frozen file only when frozen', () => {
    expect(snapshotUrl()).toBe(FROZEN_URL);
    vi.unstubAllEnvs();
    expect(snapshotUrl()).toBe(SNAPSHOT_URL);
  });

  it('reads the frozen file once, says when it froze, and never says behind or live', async () => {
    const f = mockApi({ [FROZEN_URL]: JSON.parse(frozenText) });
    render(<MathPage engine={never} />);
    await settle(f);
    await until(() => screen.getByText(`on the scanner, ${frozenOn()}`));
    expect(f).toHaveBeenCalledTimes(1);
    expect(String(f.mock.calls[0][0])).toBe(FROZEN_URL);
    const text = pageText();
    expect(text).toContain(FROZEN_LINE);
    expect(text).toContain(frozenOn());
    for (const w of [BEHIND, LIVE_UNAVAILABLE, ...OLD_WORDS]) expect(text, w).not.toContain(w);
    expect(text).toContain(MARKETS_WARMING_FROZEN);
    expect(text).toContain(HOOK_FROZEN);
    expect(text).toContain(CH7_LEAD_FROZEN);
    expect(screen.getByText(FOOTER_FROZEN, { exact: false })).toBeInTheDocument();
    const homes = screen.getAllByRole('link', { name: '← isoca-radar' });
    expect(homes).toHaveLength(2);
    for (const a of homes) expect(a).toHaveAttribute('href', '/');
    // a way home at the top, before chapter 1
    expect(homes[0].compareDocumentPosition(document.querySelector('h2') as Element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // the file's `today` is the day after capture stopped: no strip, no count line, no "as of" beside the heading
    for (const w of ['the last captured day (UTC)', CH3_COUNT, 'arbitrage episodes per UTC hour today', 'edge on the table today', 'as of ']) expect(text, w).not.toContain(w);
    expect(screen.getByRole('link', { name: 'What it took to run live →' })).toHaveAttribute('href', '/#cost');
    expect(document.querySelector('.stale')).toBeNull();
    expect(FOOTER_FROZEN).not.toBe(FOOTER);
    expect(HOOK_FROZEN).not.toBe(HOOK);
    expect(CHAPTERS[6].lead).not.toBe(CH7_LEAD_FROZEN);
  });

  it('does not fetch again when the tab comes back, even long after', async () => {
    const f = mockApi({ [FROZEN_URL]: JSON.parse(frozenText) });
    render(<MathPage engine={never} />);
    await settle(f);
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    vi.mocked(Date.now).mockReturnValue(NOW + 3_600_000);
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
  });

  it('a failed load is not an age warning: no behind, the frozen line stays', async () => {
    const f = mockApi({ [FROZEN_URL]: jsonResponse(404, {}) });
    render(<MathPage engine={never} />);
    await settle(f);
    const text = pageText();
    expect(text).not.toContain(BEHIND);
    expect(text).not.toContain(LIVE_UNAVAILABLE);
    expect(text).toContain(FROZEN_LINE);
  });
});

describeWasm('/math in frozen mode, chapter 6 on the real engine', () => {
  it('the section f caption says the paper accounts used the rules', async () => {
    const engine = await loadTestEngine();
    const f = mockApi({ [FROZEN_URL]: JSON.parse(frozenText) });
    render(<MathPage engine={Promise.resolve(engine)} />);
    await settle(f);
    await until(() => screen.getByText(F_CAPTION_FROZEN));
    expect(F_CAPTION_FROZEN).toContain('the same rules the scanner’s paper accounts used; here the unwind happens at one instant, the sim waited the account’s delay.');
    expect(pageText()).not.toContain(SECTIONS.f.caption);
  });
});
