// The JSON contract with crates/arb-wasm, copied from 7b-A's "Contract for 7b-B" (field order = serialized
// order). Dec is a JSON string holding a normalized decimal. The parity test compares whole strings.

export type Dec = string;
export interface Level { p: Dec; s: Dec }
export interface Book { bids: Level[]; asks: Level[] }
export interface Fee { rate: Dec; exponent: number }
export type LegStatus = 'live' | 'placeholder' | 'other' | 'closed_no' | 'closed_yes' | 'halted';
export interface LegIn { status: LegStatus; bids: Level[]; asks: Level[]; fee: Fee | null }

export interface MirrorIn { bids: Level[]; asks: Level[] }
export interface MirrorModel {
  no_asks: Level[];
  no_bids: Level[];
  best_bid: Level | null;
  best_ask: Level | null;
  spread: Dec | null;
  d1_long: Dec | null;
  d1_short: Dec | null;
  crossed: boolean;
}

export type PayoffKind = 'yes' | 'no' | 'implication';
/** prices: length s, or 2 for the implication (whose s is ignored). */
export interface PayoffIn { kind: PayoffKind; s: number; other_listed: boolean; prices: { bid: Dec; ask: Dec }[] | null }
export interface PayoffMatrix {
  rows: string[];
  cols: string[];
  possible: boolean[];
  cells: (Dec | null)[][];
  basket: (Dec | null)[];
  guaranteed: Dec;
  broken: boolean[];
  top_edge_gross: Dec | null;
}

export const CLASSIFY_NODES = [
  'decided', 'no_basket_tried', 'no_basket_few_legs', 'no_basket_crossed', 'no_basket_not_positive', 'no_basket_positive',
  'yes_basket_tried', 'yes_basket_no_live', 'yes_basket_crossed', 'yes_basket_missing_ask', 'yes_basket_not_positive',
  'yes_basket_positive', 'complete', 'incomplete',
] as const;
export type ClassifyNode = (typeof CLASSIFY_NODES)[number];
export type SignalKind = 'yes_basket' | 'yes_basket_incomplete' | 'no_basket' | 'implication';
export type Realization = 'settlement' | 'converter' | 'not_arbitrage';
export interface ClassifyIn { augmented: boolean; legs: LegIn[] }
/** path is typed string[]: a node this page does not know is still drawn (review focus 4). */
export interface ClassifyModel {
  path: string[];
  kind: SignalKind | null;
  realization: Realization | null;
  net: Dec | null;
  gross: Dec | null;
  q_star: Dec | null;
}

export interface DepthWalkIn { rule: 'yes' | 'no'; legs: { levels: Level[]; fee: Fee | null }[] }
/** payoff per step: 1 for the YES rule; for the nested NO rule, active legs − 1. */
export interface WalkStep { q_from: Dec; q_to: Dec; cost: Dec; payoff: Dec; profitable: boolean }
export interface WalkModel { steps: WalkStep[]; q_star: Dec; pi: Dec; pi_gross: Dec }

export interface ScenarioIn { proc_ms: number; net_ms: number; match_ms: number; honor_seconds_delay: boolean }
export interface EpisodeIn {
  kind: 'yes' | 'no';
  gap_ms: number;
  updates: { t_ms: number; legs: LegIn[] }[];
  scenario: ScenarioIn;
  /** 0–10 s, or null. */
  seconds_delay: number | null;
}
export interface EpisodeModel {
  updates: { t_ms: number; net: Dec | null }[];
  open_ms: number | null;
  close_ms: number | null;
  peak_ms: number | null;
  peak_net: Dec | null;
  t_x: number | null;
  captured: Dec | null;
  caught: boolean;
}

/** Every book is a YES book; for kind "no" the buy walks the mirrored bids, the unwind the mirrored asks. */
export interface BasketLegIn { open: Book; t_x: Book; unwind: Book; fee: Fee | null }
/** min_order ("5") and convert_fee_bips ("0") are left to their defaults by the page. */
export interface BasketIn { kind: 'yes' | 'no'; legs: BasketLegIn[]; q_request: Dec }
export interface Fill { requested: Dec; filled: Dec; vwap: Dec | null; fees: Dec; levels_used: number }
export interface BasketModel {
  kind: 'yes' | 'no';
  min_order: Dec;
  q_open: Dec;
  q_target: Dec;
  legs: { limit: Dec; buy: Fill; excess: Dec; unwind: Fill }[];
  q_eff: Dec;
  cash: { spent: Dec; fees: Dec; unwind_back: Dec; convert_back: Dec; net_cash: Dec; held_unsold: { qty: Dec; basis: Dec }[] };
  value_at_resolution: Dec | null;
}

export const ENGINE_FNS = ['mirror_json', 'payoff_json', 'classify_json', 'depth_walk_json', 'episode_json', 'basket_json'] as const;
export type EngineFn = (typeof ENGINE_FNS)[number];
/** The six exports of the generated package (C3). */
export type Engine = Record<EngineFn, (input: string) => string>;
export type EngineError = { error: string };

export function isError(x: unknown): x is EngineError {
  return typeof x === 'object' && x !== null && typeof (x as { error?: unknown }).error === 'string';
}
