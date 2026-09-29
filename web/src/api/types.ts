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
  /** Phase 5b archive (compact_status); null until the compactor first reports. */
  archive_bytes: number | null;
  archive_oldest_day: string | null;
  compact_lag_hours: number | null;
  compact_last_error: string | null;
  /** now − the compactor's last report, server clock. */
  compact_age_ms: number | null;
  /** When the first hour was recorded in compact_hours (done or lost). */
  compact_started_ms: number | null;
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
  /** Net profit at the peak with at most $100 spent; null when not sized. */
  net_budget100: Dec | null;
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
/** Live complete-basket (YES / NO) arbitrage episodes opened today, UTC. */
export interface Today {
  day_start_ms: number;
  arb: number;
  no: number;
  yes: number;
  /** Sum of each episode's $100-capped net profit; null when none was measured. */
  edge_budget100: Dec | null;
  hourly: { hour: number; arb: number }[];
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
  /** The event's end date as stored (`events.end_date`, text); null when unknown (spec 7a §9.2). */
  end_date: string | null;
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
export type CardStatus = 'live' | 'warming' | 'thin' | 'under_review';
export interface CardRow {
  label: string;
  /** count | × | s | % | bps | USD | ms */
  unit: string;
  paper: Dec | null;
  their: Dec | null;
  ours: Dec | null;
}
export interface DecayPoint {
  scenario: string;
  latency_ms: number;
  net_ms: number;
  captured: Dec | null;
  rate_pct: Dec | null;
  headline: boolean;
}
export type SweetLabel = 'candidate' | 'negative' | 'inconclusive' | 'thin' | 'not_tested' | 'dead_on_paper';
export interface SweetRow {
  side: 'yes' | 'no';
  scenario: string;
  fee_bips: number;
  latency_ms: number | null;
  dimension: 'all' | 'category' | 'days' | 's' | 'hour';
  bucket: string;
  label: SweetLabel;
  few_trades: boolean;
  n_exec: number;
  n_filled: number;
  n_missed: number;
  clusters: number;
  n_attempts: number;
  skipped: Record<string, number>;
  episodes: number;
  flagged: number;
  on_table: Dec;
  on_table_eligible: Dec;
  captured: Dec;
  captured_leftover0: Dec;
  mean_leftover0: Dec | null;
  mean: Dec | null;
  lo: Dec | null;
  hi: Dec | null;
  eff_n: Dec | null;
  cost: Dec;
  annualized: Dec | null;
  annualized_lo: Dec | null;
  annualized_upper: Dec | null;
  leftover_unpriced: number;
  pooled: number;
}
export interface SweetFee {
  fee_bips: number;
  captured: Dec;
  mean: Dec | null;
  label: SweetLabel;
}
export interface SweetSeries {
  rows: SweetRow[];
  m: number;
  z: Dec;
  category_available: boolean;
  hurdle: Dec | null;
  fees: SweetFee[];
}
export interface CardSeries {
  bars?: { label: string; value: number }[];
  curves?: { label: string; points: { t_ms: number; s: Dec }[]; median_ms: Dec | null }[];
  bins?: { side: string; lo_bps: number; hi_bps: number | null; net: number; gross: number }[];
  decay?: DecayPoint[];
  measured?: { latency_ms: number; host: string };
  sweet?: SweetSeries;
}
export interface CardBody {
  card: string;
  paper: string;
  title: string;
  win: string;
  status: CardStatus;
  premise_key: string;
  hash: string;
  data_from_ms: number | null;
  data_to_ms: number | null;
  since_ms: number | null;
  n: number;
  columns: CardRow[];
  series: CardSeries;
  notes: string[];
  thin_reasons: string[];
  episode_link: string | null;
  computed_ms: number;
}
export interface CardsResponse {
  win: string;
  cards: CardBody[];
}
export interface PaperEntry {
  id: string;
  cite: string;
  status: 'live' | 'coming_later' | 'dropped';
  note: string | null;
  cards: string[];
}
export interface CardsMeta {
  tick: { last_tick_ms: number; analyze_version: string; tick_ms: number } | null;
  last_error: { ms: number; text: string } | null;
  catch_up: Record<string, { key: string; next_hour: number; done: boolean }>;
  papers: PaperEntry[];
  not_replicated: { item: string; why: string }[];
}

/** Phase 7a §7.1: one paper account of the latency ladder since the epoch start, cut at `as_of_ms`. */
export interface LadderRung {
  account_id: string;
  scenario: string;
  /** proc + net + match from the api's policies file; null when the api has none. */
  latency_ms: number | null;
  /** sp-burst / sp-idle-nodelay: drawn beside the ladder, never in the break-even. */
  extra: boolean;
  n_exec: number;
  n_leg_risk: number;
  /** Execs with q_eff = 0 (SWEET's meaning). */
  n_missed: number;
  /** Σ realized_delta of converts: basket gains, fees included. */
  gains: Dec;
  /** Σ realized_delta of unwind sells: leg-risk cost, fees included. */
  leg_risk: Dec;
  /** Σ realized_delta of settlements. */
  settle: Dec;
  unrealized: Dec;
  /** Latest common mark's equity − bankroll = gains + leg_risk + settle + unrealized, exactly. */
  net: Dec;
}
/** `cards::ladder::BreakEven`: internally tagged by `kind`; `not_monotone` is sent only when true. */
export type BreakEven =
  | { kind: 'thin' }
  | { kind: 'all' }
  | { kind: 'none' }
  | { kind: 'at'; latency_ms: number; direction: 'loses_beyond' | 'pays_beyond'; not_monotone?: boolean };
export type MarketsStatus = 'live' | 'warming' | 'under_review' | 'missing';
/** GET /api/paper/ladder. */
export interface Ladder {
  epoch: number;
  /** null in epoch 0 (no epoch row yet). */
  start_ms: number | null;
  /** The latest instant every ladder account has a mark; every sum stops there. */
  as_of_ms: number | null;
  /** LADDER rungs by latency, then the extras; a LADDER rung with no account is left out. */
  rungs: LadderRung[];
  /** SWEET's 30d NO rows at fee 0 for the LADDER scenarios, dimensions all + category. */
  markets: SweetRow[];
  measured: { latency_ms: number; host: string } | null;
  markets_status: MarketsStatus;
  break_even: BreakEven;
}
export interface PaperEpoch {
  epoch: number;
  start_ms: number;
  cause: 'policies' | 'reset';
}
/** GET /api/paper/epochs; `current` is 0 before the first epoch row. */
export interface Epochs {
  epochs: PaperEpoch[];
  current: number;
  pending_request: { id: number; requested_ms: number } | null;
  /** The next UTC midnight after today's request, or now. */
  next_allowed_ms: number;
}
/** POST /api/paper/reset → 202. */
export interface ResetQueued {
  request_id: number;
  requested_ms: number;
}
