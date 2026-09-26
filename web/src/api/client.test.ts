import { describe, expect, it } from 'vitest';
import { jsonResponse, mockApi } from '../test/mockApi';
import { ApiError, api } from './client';

describe('api client', () => {
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
