import type {
  EpisodeDetail, EpisodeSummary, EquitySeries, HealthEnvelope, HealthSeries, Integrity, LedgerEntry,
  OpenEpisodes, Page, PaperAccount, Position, Today,
} from './types';

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly detail: string;
  constructor(status: number, code: string, detail: string) {
    super(detail);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.detail = detail;
  }
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, { signal, credentials: 'same-origin', headers: { Accept: 'application/json' } });
  if (!res.ok) {
    let code = `http_${res.status}`;
    let detail = 'Something broke. Not your fault. Probably.';
    try {
      const body = (await res.json()) as { error?: string; detail?: string };
      if (body.error) code = body.error;
      if (body.detail) detail = body.detail;
    } catch {
      // not JSON (a proxy page): keep the generic text
    }
    throw new ApiError(res.status, code, detail);
  }
  return (await res.json()) as T;
}

function qs(params: Record<string, string | number | null | undefined>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export interface EpisodeQuery {
  kind?: string;
  from?: number;
  to?: number;
  net?: boolean;
  flags?: string[];
  cursor?: string | null;
  limit?: number;
}

export const api = {
  healthNow: (signal?: AbortSignal) => getJson<HealthEnvelope>('/api/health/now', signal),
  healthSeries: (hours: number, signal?: AbortSignal) =>
    getJson<HealthSeries>(`/api/health/series${qs({ hours })}`, signal),
  openEpisodes: (signal?: AbortSignal) => getJson<OpenEpisodes>('/api/episodes/open', signal),
  episodes: (q: EpisodeQuery, signal?: AbortSignal) =>
    getJson<Page<EpisodeSummary>>(
      `/api/episodes${qs({
        kind: q.kind,
        from: q.from,
        to: q.to,
        net: q.net ? 1 : undefined,
        flags: q.flags && q.flags.length > 0 ? q.flags.join(',') : undefined,
        cursor: q.cursor,
        limit: q.limit,
      })}`,
      signal,
    ),
  episode: (id: string, signal?: AbortSignal) =>
    getJson<EpisodeDetail>(`/api/episodes/${encodeURIComponent(id)}`, signal),
  today: (signal?: AbortSignal) => getJson<Today>('/api/today', signal),
  integrity: (hours: number, signal?: AbortSignal) =>
    getJson<Integrity>(`/api/integrity/summary${qs({ hours })}`, signal),
  paperAccounts: (signal?: AbortSignal) => getJson<{ items: PaperAccount[] }>('/api/paper/accounts', signal),
  paperEquity: (account: string, hours: number, signal?: AbortSignal) =>
    getJson<EquitySeries>(`/api/paper/equity${qs({ account, hours })}`, signal),
  paperPositions: (signal?: AbortSignal) => getJson<{ items: Position[] }>('/api/paper/positions', signal),
  paperLedger: (account: string, cursor: string | null, signal?: AbortSignal) =>
    getJson<Page<LedgerEntry>>(`/api/paper/ledger${qs({ account, cursor })}`, signal),
};
