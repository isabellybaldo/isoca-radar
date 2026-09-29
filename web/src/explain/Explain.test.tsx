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
    // D-06: docked and open, the tab sits in the flow on the panel's left edge (theme.css pins the rule).
    expect(tab()).toHaveAttribute('data-dock');
    a.unmount();
    stubMedia(false);
    mount();
    expect(panel()).toHaveAttribute('data-mode', 'drawer');
    expect(tab()).not.toHaveAttribute('data-dock');
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
