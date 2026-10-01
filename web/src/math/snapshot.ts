// Spec 7b §5.1 and §6.3: the public snapshot the api writes every 5 minutes (plan 7b-A) and nginx serves
// as a static file. The types copy 7b-A's "Contract for 7b-B" (the fields the page reads, bins/api/src/public.rs);
// parseSnapshot is strict (R15): bad JSON, v != 1, a missing, mistyped or fractional field, an unknown enum value
// all give null, which the page shows as "live numbers unavailable". It returns a fresh object built from the
// whitelisted fields, so a key the api never promised is not passed on. Decimals stay strings (R12).
// Type-only imports from src/api are erased (spec 7b §7.2).
import type { BreakEven, MarketsStatus, SweetLabel } from '../api/types';
import type { LadderIn } from '../lib/ladder';

export const SNAPSHOT_URL = '/math-data/public.json';
/** §6.3: within 30 minutes of written_ms the snapshot is fresh. */
export const FRESH_MS = 30 * 60_000;
/** §6.3: a tab coming back re-fetches only when the last fetch is older than this. */
export const REFETCH_MS = 5 * 60_000;

export interface PublicRung {
  scenario: string;
  latency_ms: number | null;
  extra: boolean;
  n_exec: number;
  n_leg_risk: number;
  n_missed: number;
  gains: string;
  leg_risk: string;
  settle: string;
  unrealized: string;
  net: string;
}

/** 7b-A: the rows the "which markets" grid draws: NO side, fee 0, the six LADDER scenarios. */
export interface PublicSweetRow {
  side: 'no';
  scenario: string;
  fee_bips: 0;
  latency_ms: number | null;
  dimension: 'all' | 'category';
  bucket: string;
  label: SweetLabel;
  n_exec: number;
  clusters: number;
  mean: string | null;
  lo: string | null;
  hi: string | null;
}

export interface PublicLadder {
  epoch: number;
  start_ms: number | null;
  as_of_ms: number | null;
  break_even: BreakEven;
  measured: { latency_ms: number } | null;
  markets_status: MarketsStatus;
  rungs: PublicRung[];
  markets: PublicSweetRow[];
}

export interface PublicToday {
  day_start_ms: number;
  arb: number;
  no: number;
  yes: number;
  edge_budget100: string | null;
  hourly: { hour: number; arb: number }[];
}

export interface PublicSnapshot {
  v: 1;
  written_ms: number;
  ladder: PublicLadder;
  today: PublicToday;
}

type Obj = Record<string, unknown>;
const isObj = (x: unknown): x is Obj => typeof x === 'object' && x !== null && !Array.isArray(x);
const isInt = (x: unknown): x is number => typeof x === 'number' && Number.isInteger(x);
const isIntOrNull = (x: unknown): x is number | null => x === null || isInt(x);
const isStr = (x: unknown): x is string => typeof x === 'string';
/** A decimal as the api prints it: a string of digits, optional sign and fraction. Never converted to a number. */
const isDec = (x: unknown): x is string => isStr(x) && /^-?\d+(\.\d+)?$/.test(x);
const isDecOrNull = (x: unknown): x is string | null => x === null || isDec(x);
const oneOf = <T extends string>(set: readonly T[], x: unknown): x is T => isStr(x) && (set as readonly string[]).includes(x);

const MARKETS_STATUS = ['live', 'warming', 'under_review', 'missing'] as const;
const LABELS = ['candidate', 'negative', 'inconclusive', 'thin', 'not_tested', 'dead_on_paper'] as const;
const DIMENSIONS = ['all', 'category'] as const;
const DIRECTIONS = ['loses_beyond', 'pays_beyond'] as const;

function readRung(x: unknown): PublicRung | null {
  if (!isObj(x)) return null;
  const { scenario, latency_ms, extra, n_exec, n_leg_risk, n_missed, gains, leg_risk, settle, unrealized, net } = x;
  if (!isStr(scenario) || !isIntOrNull(latency_ms) || typeof extra !== 'boolean') return null;
  if (!isInt(n_exec) || !isInt(n_leg_risk) || !isInt(n_missed)) return null;
  if (!isDec(gains) || !isDec(leg_risk) || !isDec(settle) || !isDec(unrealized) || !isDec(net)) return null;
  return { scenario, latency_ms, extra, n_exec, n_leg_risk, n_missed, gains, leg_risk, settle, unrealized, net };
}

function readRow(x: unknown): PublicSweetRow | null {
  if (!isObj(x)) return null;
  const { side, scenario, fee_bips, latency_ms, dimension, bucket, label, n_exec, clusters, mean, lo, hi } = x;
  if (side !== 'no' || fee_bips !== 0 || !isStr(scenario) || !isIntOrNull(latency_ms) || !isStr(bucket)) return null;
  if (!oneOf(DIMENSIONS, dimension) || !oneOf(LABELS, label)) return null;
  if (!isInt(n_exec) || !isInt(clusters) || !isDecOrNull(mean) || !isDecOrNull(lo) || !isDecOrNull(hi)) return null;
  return { side, scenario, fee_bips, latency_ms, dimension, bucket, label, n_exec, clusters, mean, lo, hi };
}

function readBreakEven(x: unknown): BreakEven | null {
  if (!isObj(x)) return null;
  if (x.kind === 'thin' || x.kind === 'all' || x.kind === 'none') return { kind: x.kind };
  if (x.kind !== 'at' || !isInt(x.latency_ms) || !oneOf(DIRECTIONS, x.direction)) return null;
  if (x.not_monotone === undefined) return { kind: 'at', latency_ms: x.latency_ms, direction: x.direction };
  if (typeof x.not_monotone !== 'boolean') return null;
  return { kind: 'at', latency_ms: x.latency_ms, direction: x.direction, not_monotone: x.not_monotone };
}

/** Every element must read, or the whole list is refused (null). */
function readAll<T>(x: unknown, read: (e: unknown) => T | null): T[] | null {
  if (!Array.isArray(x)) return null;
  const out: T[] = [];
  for (const e of x) {
    const r = read(e);
    if (r === null) return null;
    out.push(r);
  }
  return out;
}

function readHour(x: unknown): { hour: number; arb: number } | null {
  return isObj(x) && isInt(x.hour) && isInt(x.arb) ? { hour: x.hour, arb: x.arb } : null;
}

function readLadder(l: unknown): PublicLadder | null {
  if (!isObj(l)) return null;
  const breakEven = readBreakEven(l.break_even);
  const rungs = readAll(l.rungs, readRung);
  const markets = readAll(l.markets, readRow);
  const m = l.measured;
  const measured = m === null ? null : isObj(m) && isInt(m.latency_ms) ? { latency_ms: m.latency_ms } : undefined;
  if (!isInt(l.epoch) || !isIntOrNull(l.start_ms) || !isIntOrNull(l.as_of_ms) || !oneOf(MARKETS_STATUS, l.markets_status)) return null;
  if (breakEven === null || rungs === null || markets === null || measured === undefined) return null;
  return {
    epoch: l.epoch,
    start_ms: l.start_ms,
    as_of_ms: l.as_of_ms,
    break_even: breakEven,
    measured,
    markets_status: l.markets_status,
    rungs,
    markets,
  };
}

function readToday(t: unknown): PublicToday | null {
  if (!isObj(t)) return null;
  const hourly = readAll(t.hourly, readHour);
  if (!isInt(t.day_start_ms) || !isInt(t.arb) || !isInt(t.no) || !isInt(t.yes) || !isDecOrNull(t.edge_budget100) || hourly === null) return null;
  return { day_start_ms: t.day_start_ms, arb: t.arb, no: t.no, yes: t.yes, edge_budget100: t.edge_budget100, hourly };
}

/** The snapshot, or null for anything the page cannot draw (§6.3 "missing"). */
export function parseSnapshot(raw: unknown): PublicSnapshot | null {
  if (!isObj(raw) || raw.v !== 1 || !isInt(raw.written_ms)) return null;
  const ladder = readLadder(raw.ladder);
  const today = readToday(raw.today);
  return ladder === null || today === null ? null : { v: 1, written_ms: raw.written_ms, ladder, today };
}

/** §6.3: fresh within 30 minutes of written_ms; a stamp in the future (clock skew) is fresh, never "behind". */
export function freshness(s: Pick<PublicSnapshot, 'written_ms'>, nowMs: number): 'fresh' | 'stale' {
  return nowMs - s.written_ms <= FRESH_MS ? 'fresh' : 'stale';
}

/** §7.2: the snapshot's ladder as ladderModel's input. Only `all` and `category` rows ship (§5.1); a row of
 *  another dimension is dropped here rather than trusted. The snapshot has no measured host (it names the
 *  VPS) and LadderIn has no host field, so none is invented. */
export function publicToLadder(l: PublicLadder): LadderIn {
  return {
    epoch: l.epoch,
    start_ms: l.start_ms,
    as_of_ms: l.as_of_ms,
    rungs: l.rungs,
    markets: l.markets.filter((r) => r.dimension === 'all' || r.dimension === 'category'),
    measured: l.measured,
    markets_status: l.markets_status,
    break_even: l.break_even,
  };
}

/** Today's arbitrage episodes per UTC hour, 24 slots (counts, never money). */
export function hourlyValues(t: Pick<PublicToday, 'hourly'>): number[] {
  const out = Array.from({ length: 24 }, () => 0);
  for (const h of t.hourly) if (Number.isInteger(h.hour) && h.hour >= 0 && h.hour < 24) out[h.hour] = Math.max(0, h.arb);
  return out;
}
