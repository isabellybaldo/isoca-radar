import { beforeEach, describe, expect, it, vi } from 'vitest';
import { epochsFixture, ladderFixture } from '../test/fixtures';
import { jsonResponse, mockApi } from '../test/mockApi';
import { ApiError, SESSION_EXPIRED_TEXT, api } from './client';

describe('api client', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  it('on a 401 (session expired), reloads the page once instead of surfacing an error', async () => {
    mockApi({ '/api/episodes/open': () => jsonResponse(401, { error: 'unauthorized', detail: 'no session' }) });
    const reload = vi.fn();
    vi.stubGlobal('location', { reload });

    let settled = false;
    void api.openEpisodes().then(
      () => (settled = true),
      () => (settled = true),
    );
    await new Promise((r) => setTimeout(r, 0));

    expect(reload).toHaveBeenCalledOnce();
    expect(settled).toBe(false); // never resolves/rejects: the page is navigating away
  });

  it('on a repeated 401 within the guard window, shows the sign-in-again copy instead of reloading again', async () => {
    mockApi({ '/api/episodes/open': () => jsonResponse(401, { error: 'unauthorized', detail: 'no session' }) });
    const reload = vi.fn();
    vi.stubGlobal('location', { reload });

    void api.openEpisodes().catch(() => {}); // first call: triggers the reload, marks the guard; never settles
    await new Promise((r) => setTimeout(r, 0));
    const err = await api.openEpisodes().catch((e: unknown) => e); // second call within the window

    expect(reload).toHaveBeenCalledOnce();
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 401, detail: SESSION_EXPIRED_TEXT });
  });

  it('turns JSON errors into ApiError with the server detail', async () => {
    mockApi({
      '/api/episodes/open': () =>
        jsonResponse(503, { error: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' }),
    });
    const err = await api.openEpisodes().catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 503, code: 'db_unavailable', detail: 'The database is thinking. Try again in a moment.' });
  });

  it('sends only the filters that are set', async () => {
    const f = mockApi({ '/api/episodes': { items: [], next_cursor: null } });
    await api.episodes({ kind: 'no_basket', net: true, flags: ['suspect', 'stale'], cursor: null, limit: 50 });
    expect(String(f.mock.calls[0][0])).toBe('/api/episodes?kind=no_basket&net=1&flags=suspect%2Cstale&limit=50');
  });

  it('keeps money as strings', async () => {
    mockApi({ '/api/paper/accounts': { items: [{ equity: '1002.56' }] } });
    const r = await api.paperAccounts();
    expect(typeof r.items[0].equity).toBe('string');
  });

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

  it('reads the ladder and the epochs from their paths, body untouched', async () => {
    const f = mockApi({ '/api/paper/ladder': ladderFixture(), '/api/paper/epochs': epochsFixture() });
    expect(await api.paperLadder()).toEqual(ladderFixture());
    expect(await api.paperEpochs()).toEqual(epochsFixture());
    expect(f.mock.calls.map((c) => String(c[0]))).toEqual(['/api/paper/ladder', '/api/paper/epochs']);
  });
});
