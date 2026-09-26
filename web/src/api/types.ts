/** A decimal string from the api ("0.41"), never a JS number (spec §4). */
export type Dec = string;

export interface HealthNow {
  now_ms: number;
  age_ms: number;
  run_id: number;
  minute_ms: number;
  tokens: number;
  shards: number;
  parked_shards: number;
  msgs: number;
  drops: number;
  drops_5m: number;
  reconnects: number;
  lag_p50_ms: number | null;
  lag_p99_ms: number | null;
  rest_p50_ms: number | null;
  rest_p99_ms: number | null;
  clock_offset_ms: number | null;
  ok: boolean | null;
  capture_paused: boolean;
  dropped_groups: number;
}
export interface HealthEnvelope {
  health: HealthNow | null;
}
export interface HealthPoint {
  minute_ms: number;
  run_id: number;
  msgs: number;
  drops: number;
  reconnects: number;
  lag_p50_ms: number | null;
  lag_p99_ms: number | null;
  shards: number;
  parked_shards: number;
  capture_paused: boolean;
}
export interface HealthSeries {
  from_ms: number;
  to_ms: number;
  points: HealthPoint[];
}
export interface EpisodeSummary {
  id: string;
  kind: string;
  realization: string;
  event_title: string | null;
  event_slug: string | null;
  n_legs: number;
  s_legs: number;
  opened_ms: number;
  closed_ms: number | null;
  duration_ms: number | null;
  edge: Dec;
  q_star: Dec;
  net: Dec;
  gross: Dec;
  suspect: boolean;
  stale: boolean;
  censored: boolean;
}
export interface OpenEpisodes {
  now_ms: number;
  items: EpisodeSummary[];
}
export interface Page<T> {
  items: T[];
  next_cursor: string | null;
}
export interface Episode {
  id: string;
  run_id: number;
  source: string;
  kind: string;
  realization: string;
  event_id: string;
  event_title: string | null;
  event_slug: string | null;
  category: string | null;
  n_outcomes: number;
  n_legs: number;
  legs_dropped: number;
  augmented: boolean;
  incomplete_set: boolean;
  uncovered_legs: number;
  opened_ms: number;
  closed_ms: number | null;
  duration_ms: number | null;
  censor_left: boolean;
  censor_right: boolean;
  censor_gap: boolean;
  detect_lag_ms: number | null;
  n_updates: number;
  peak_ms: number;
  edge: Dec;
  edge_gross: Dec;
  q_star: Dec;
  cost: Dec;
  payout: Dec;
  fees: Dec;
  net: Dec;
  gross: Dec;
  profit_per_dollar: Dec | null;
  net_budget100: Dec | null;
  bottleneck_leg_idx: number | null;
  below_min_size: boolean | null;
  max_seconds_delay: number | null;
  days_to_end: Dec | null;
  annualized_return: Dec | null;
  post_end: boolean | null;
  suspect: boolean;
  stale: boolean;
  open_lag_ms: number | null;
  oracle_downgraded: boolean | null;
  scanner_version: string;
}
export interface Leg {
  leg_idx: number;
  market_id: string;
  action: string;
  best_price: Dec | null;
  vwap: Dec | null;
  levels: number;
  size_at_best: Dec | null;
  filled: Dec;
  notional: Dec;
  fees: Dec;
  in_s: boolean;
}
export interface Observation {
  offset_ms: number;
  source: string;
  observed_ms: number | null;
  skipped: boolean;
  skip_reason: string | null;
  edge: Dec | null;
  q_star: Dec | null;
  pnl: Dec | null;
  still_profitable: boolean | null;
  fill_at_original_size: boolean | null;
}
export interface EpisodeDetail {
  episode: Episode;
  legs: Leg[];
  observations: Observation[];
}
export interface Today {
  day_start_ms: number;
  gross: number;
  net: number;
  hourly: { hour: number; gross: number; net: number }[];
}
export interface Integrity {
  from_ms: number;
  to_ms: number;
  by_kind: { kind: string; count: number }[];
  total: number;
  msgs: number;
  per_million: Dec | null;
}
export interface PaperAccount {
  account_id: string;
  policy: string;
  scenario: string;
  fee_bips: Dec;
  bankroll: Dec;
  run_id: number;
  marked_ms: number | null;
  cash: Dec | null;
  equity: Dec | null;
  realized: Dec | null;
  unrealized: Dec | null;
  fees_paid: Dec | null;
  locked: Dec | null;
  max_drawdown: Dec | null;
  n_positions: number | null;
}
export interface EquitySeries {
  account_id: string;
  from_ms: number;
  to_ms: number;
  points: { t_ms: number; equity: Dec }[];
}
export interface Position {
  account_id: string;
  market_id: string;
  outcome: string;
  qty: Dec;
  basis: Dec;
  returned: boolean;
  event_title: string | null;
}
export interface LedgerEntry {
  run_id: number;
  account_id: string;
  seq: number;
  t_ms: number;
  kind: string;
  episode_id: string | null;
  market_id: string | null;
  outcome: string | null;
  qty: Dec;
  price: Dec | null;
  cash_delta: Dec;
  fees: Dec;
  realized_delta: Dec;
  cash_after: Dec;
  equity: Dec | null;
}
