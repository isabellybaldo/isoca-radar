import type {
  EpisodeDetail, EpisodeSummary, HealthNow, HealthPoint, LedgerEntry, PaperAccount, Position, Today,
} from '../api/types';

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

export function episodeDetailFixture(o: Partial<EpisodeDetail['episode']> = {}): EpisodeDetail {
  const opened = Date.UTC(2026, 8, 25, 1, 12, 4, 210);
  return {
    episode: {
      id: 'live-1-00000001', run_id: 1, source: 'live', kind: 'no_basket', realization: 'converter', event_id: 'ev1',
      event_title: 'Bitcoin price on September 26?', event_slug: 'bitcoin', category: null, n_outcomes: 7, n_legs: 7,
      legs_dropped: 0, augmented: false, incomplete_set: false, uncovered_legs: 0, opened_ms: opened,
      closed_ms: opened + 4190, duration_ms: 4190, censor_left: false, censor_right: false, censor_gap: false,
      detect_lag_ms: 84, n_updates: 3, peak_ms: opened + 1000, edge: '0.0049', edge_gross: '0.0137', q_star: '83',
      cost: '99.59', payout: '100', fees: '0', net: '0.41', gross: '1.37', profit_per_dollar: '0.0041',
      net_budget100: '0.33', bottleneck_leg_idx: 0, below_min_size: false, max_seconds_delay: 3, days_to_end: null,
      annualized_return: null, post_end: false, suspect: true, stale: false, open_lag_ms: 12, oracle_downgraded: null,
      scanner_version: 'sha',
      ...o,
    },
    legs: [
      { leg_idx: 0, market_id: 'm0', action: 'buy_no', best_price: '0.62', vwap: '0.6231', levels: 2, size_at_best: '140', filled: '83', notional: '51.7', fees: '0.21', in_s: true },
      { leg_idx: 1, market_id: 'm1', action: 'buy_no', best_price: null, vwap: null, levels: 0, size_at_best: null, filled: '0', notional: '0', fees: '0.07', in_s: false },
    ],
    observations: [
      { offset_ms: 1000, source: 'rest', observed_ms: opened + 1000, skipped: false, skip_reason: null, edge: '0.0022', q_star: '61', pnl: '0.2', still_profitable: true, fill_at_original_size: false },
      { offset_ms: 3000, source: 'rest', observed_ms: opened + 3000, skipped: false, skip_reason: null, edge: '-0.0008', q_star: '0', pnl: '0', still_profitable: false, fill_at_original_size: false },
      { offset_ms: 5000, source: 'rest', observed_ms: null, skipped: true, skip_reason: 'recheck capacity', edge: null, q_star: null, pnl: null, still_profitable: null, fill_at_original_size: null },
    ],
  };
}

export function accountFixture(o: Partial<PaperAccount> = {}): PaperAccount {
  return {
    account_id: 'P0-converter/sp-idle', policy: 'P0-converter', scenario: 'sp-idle', fee_bips: '0', bankroll: '1000',
    run_id: 3, marked_ms: 1_790_000_000_000, cash: '900', equity: '1002.56', realized: '2.56', unrealized: '0',
    fees_paid: '0.21', locked: '0', max_drawdown: '0.4', n_positions: 0,
    ...o,
  };
}

export function positionFixture(o: Partial<Position> = {}): Position {
  return {
    account_id: 'P3-naive/sp-idle', market_id: 'm1', outcome: 'yes', qty: '40', basis: '18', returned: false,
    event_title: 'Fed decision in October?',
    ...o,
  };
}

export function ledgerFixture(o: Partial<LedgerEntry> = {}): LedgerEntry {
  return {
    run_id: 3, account_id: 'P0-converter/sp-idle', seq: 1, t_ms: Date.UTC(2026, 8, 25, 1, 12, 4), kind: 'buy',
    episode_id: 'live-1-00000001', market_id: 'm1', outcome: 'no', qty: '51.2', price: '0.62', cash_delta: '-99.84',
    fees: '0.21', realized_delta: '0', cash_after: '900.16', equity: null,
    ...o,
  };
}

export function healthPointFixture(minute_ms: number, reconnects: number): HealthPoint {
  return {
    minute_ms, run_id: 3, msgs: 258_000, drops: 0, reconnects, lag_p50_ms: 78, lag_p99_ms: 1900, shards: 40,
    parked_shards: 0, capture_paused: false,
  };
}
