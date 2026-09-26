import { beforeEach, describe, expect, it, vi } from 'vitest';
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
});
