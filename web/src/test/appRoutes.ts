import {
  accountFixture, edgeFixture, episodeDetailFixture, episodeSummaryFixture, epochsFixture, healthFixture, ladderFixture, metaFixture,
  positionFixture, sweetFixture, sweetRow, todayFixture,
} from './fixtures';
import type { Route } from './mockApi';

/** Every route the dashboard pages ask for, answered from fixtures (the panel tests render the whole App). */
export function appRoutes(o: Record<string, Route> = {}): Record<string, Route> {
  return {
    '/api/health/now': { health: healthFixture() },
    '/api/health/series': { from_ms: 0, to_ms: 1, points: [] },
    '/api/integrity/summary': { from_ms: 0, to_ms: 1, by_kind: [], total: 0, msgs: 258_000, per_million: '0' },
    '/api/episodes/open': {
      now_ms: 0,
      items: [
        episodeSummaryFixture(),
        episodeSummaryFixture({ id: 'live-1-00000002', kind: 'yes_basket_incomplete', realization: 'not_arbitrage', event_title: 'Who wins the cup?', s_legs: 3 }),
      ],
    },
    '/api/today': todayFixture(1, 0, '0.33'),
    '/api/cards': (u: URL) => ({
      win: u.searchParams.get('win'),
      cards: [
        edgeFixture({ win: u.searchParams.get('win') ?? '24h' }),
        sweetFixture([sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1', lo: '-0.2', hi: '-0.01' })]),
      ],
    }),
    '/api/cards/meta': metaFixture(),
    '/api/episodes': { items: [episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 })], next_cursor: null },
    '/api/episodes/live-1-00000001': episodeDetailFixture({ category: 'crypto' }),
    '/api/paper/accounts': { items: [accountFixture(), accountFixture({ account_id: 'P3-naive/sp-idle', policy: 'P3-naive' })] },
    '/api/paper/positions': { items: [positionFixture(), positionFixture({ market_id: 'm2', end_date: null, event_title: 'Who wins the cup?' })] },
    '/api/paper/equity': (u: URL) => ({ account_id: u.searchParams.get('account'), from_ms: 0, to_ms: 1, points: [{ t_ms: 0, equity: '1000' }] }),
    '/api/paper/ledger': { items: [], next_cursor: null },
    '/api/paper/epochs': epochsFixture(),
    '/api/paper/ladder': ladderFixture(),
    ...o,
  };
}
