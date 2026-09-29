import { vi } from 'vitest';

export function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

/** A route value is a JSON body, or a function of the URL (and the request init) returning a body or a Response. */
export type Route = unknown | ((url: URL, init?: RequestInit) => unknown);

/** Stub global fetch with a path → response table. Unknown paths answer a JSON 404. */
export function mockApi(routes: Record<string, Route>) {
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), 'http://localhost');
    if (!(url.pathname in routes)) {
      return jsonResponse(404, { error: 'not_found', detail: 'Nothing here. It may have been a dream.' });
    }
    const route = routes[url.pathname];
    const body = typeof route === 'function' ? (route as (u: URL, i?: RequestInit) => unknown)(url, init) : route;
    return body instanceof Response ? body : jsonResponse(200, body);
  });
  vi.stubGlobal('fetch', fn);
  return fn;
}
