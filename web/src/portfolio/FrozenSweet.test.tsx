// Task 3 of the frozen site: Sweet spots, ALL window, from the committed file; one fetch, no switch, no age wording.
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import cardsText from '../../frozen/cards-all.json?raw';
import { mockApi } from '../test/mockApi';
import { settle } from '../test/settle';
import { FrozenSweet } from './FrozenSweet';

const URL_ALL = '/frozen/cards-all.json';

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  vi.spyOn(Date, 'now').mockReturnValue(Date.UTC(2027, 0, 1));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const tenMinutes = () => act(async () => { await vi.advanceTimersByTimeAsync(10 * 60_000); });
const pageText = () => document.body.textContent ?? '';

describe('FrozenSweet', () => {
  it('fetches the frozen file once, even after ten minutes, and shows the frozen ALL body', async () => {
    const f = mockApi({ [URL_ALL]: JSON.parse(cardsText) });
    render(<FrozenSweet />);
    await settle(f);
    expect(f).toHaveBeenCalledTimes(1);
    expect(String(f.mock.calls[0][0])).toBe(URL_ALL);
    await tenMinutes();
    expect(f).toHaveBeenCalledTimes(1);
    expect(screen.getByText('FROZEN · ALL')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NO' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'YES' })).toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'NO · ALL' })).toBeInTheDocument();
  });

  it('has no window switch, no status tag and no age wording', async () => {
    const f = mockApi({ [URL_ALL]: JSON.parse(cardsText) });
    render(<FrozenSweet />);
    await settle(f);
    for (const name of ['7D', '30D', '24H']) expect(screen.queryByRole('button', { name })).toBeNull();
    const text = pageText();
    for (const w of ['LIVE', 'WARMING', 'UNDER REVIEW', 'min ago']) expect(text, w).not.toContain(w);
  });

  it('says when it was computed, once, in the fold', async () => {
    const f = mockApi({ [URL_ALL]: JSON.parse(cardsText) });
    render(<FrozenSweet />);
    await settle(f);
    expect(pageText()).toMatch(/computed 2026-10-19 \d\d:\d\d:\d\d UTC/);
    expect(pageText()).not.toContain('UTC UTC');
  });

  it('shows the error panel once when the fetch fails, and does not retry', async () => {
    const f = vi.fn(async () => { throw new TypeError('offline'); });
    vi.stubGlobal('fetch', f);
    render(<FrozenSweet />);
    await settle(f);
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    await tenMinutes();
    expect(f).toHaveBeenCalledTimes(1);
  });
});
