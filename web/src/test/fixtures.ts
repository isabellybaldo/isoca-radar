import type { EpisodeSummary, HealthNow, Today } from '../api/types';

export function healthFixture(o: Partial<HealthNow> = {}): HealthNow {
  return {
    now_ms: 1_790_000_000_000, age_ms: 30_000, run_id: 3, minute_ms: 1_789_999_940_000, tokens: 4000, shards: 40,
    parked_shards: 0, msgs: 258_000, drops: 0, drops_5m: 0, reconnects: 0, lag_p50_ms: 78, lag_p99_ms: 1900,
    rest_p50_ms: 80, rest_p99_ms: 300, clock_offset_ms: 3, ok: true, capture_paused: false, dropped_groups: 0,
    ...o,
  };
}

export function episodeSummaryFixture(o: Partial<EpisodeSummary> = {}): EpisodeSummary {
  return {
    id: 'live-1-00000001', kind: 'no_basket', realization: 'converter', event_title: 'Bitcoin price on September 26?',
    event_slug: 'bitcoin', n_legs: 7, s_legs: 5, opened_ms: 1_790_000_000_000, closed_ms: null, duration_ms: null,
    edge: '0.0049', q_star: '83', net: '0.41', gross: '1.37', suspect: false, stale: false, censored: false,
    ...o,
  };
}

export function todayFixture(gross: number, net: number): Today {
  return {
    day_start_ms: 1_789_948_800_000,
    gross,
    net,
    hourly: Array.from({ length: 24 }, (_, hour) => ({ hour, gross: hour === 1 ? gross : 0, net: hour === 1 ? net : 0 })),
  };
}
