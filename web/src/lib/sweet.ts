import type { SweetLabel, SweetRow, SweetSeries } from '../api/types';
import { compareDecimal, formatMoney } from './money';
import { fmtUtcHm } from './time';

/** Spec 5d §7.6, verbatim. */
export const SWEET_CAVEAT =
  "Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.";
export const NOTHING_PAYS = "Nothing pays at sp-idle yet. Don't trade this, or get closer.";
export const SWEET_EMPTY = 'No sweet spots yet. analyze has not finished a tick under these premises.';
export const FEW_TRADES = 'few trades carry this row';
export const HEADLINE_SCENARIO = 'sp-idle';
export const COLO_TITLE = 'a co-located trader';

export const LABEL_TEXT: Record<SweetLabel, string> = {
  candidate: 'survives here; worth a real small-order test.',
  negative: "loses here. Don't trade this.",
  inconclusive: 'could go either way at this sample size.',
  thin: 'too few trades or events to say anything.',
  not_tested: 'no paper account trades this.',
  dead_on_paper: 'even first in line, the money is locked too long to be worth it.',
};
export const LABEL_TAG: Record<SweetLabel, string> = {
  candidate: 'CANDIDATE',
  negative: 'NEGATIVE',
  inconclusive: 'UNSURE',
  thin: 'THIN',
  not_tested: 'NOT TESTED',
  dead_on_paper: 'DEAD ON PAPER',
};

export type SweetSide = 'no' | 'yes';
export type SweetDim = 'all' | 'category' | 'days' | 's' | 'hour';
export const DIMS: [SweetDim, string][] = [
  ['all', 'ALL'],
  ['category', 'CATEGORY'],
  ['days', 'DAYS'],
  ['s', 'S'],
  ['hour', 'HOUR'],
];

const BUCKET_TEXT: Record<string, string> = {
  all: 'all',
  lt1: '< 1 day',
  '1-7': '1–7 days',
  '7-30': '7–30 days',
  '30-90': '30–90 days',
  '90-365': '90–365 days',
  ge365: '≥ 365 days',
  unknown: 'unknown',
  gt10: '> 10 legs',
};

/** Canonical bucket order per dimension (`cards::sweet::{DAYS,S,HOUR}_BUCKETS`), used to sort a
 *  grid's rows even when the caller hands `grid` an unsorted row set (as unit tests do); in
 *  production the api already sends rows sorted this way (Task 7's `sweet_parts`). */
const BUCKET_ORDER: Partial<Record<SweetDim, string[]>> = {
  days: ['lt1', '1-7', '7-30', '30-90', '90-365', 'ge365', 'unknown'],
  s: ['2', '3', '4-5', '6-10', 'gt10'],
  hour: ['00-04', '04-08', '08-12', '12-16', '16-20', '20-24'],
};

/** "sports", "2 legs", "1–7 days", "00–04 UTC". */
export function bucketText(dim: string, bucket: string): string {
  if (dim === 's') return BUCKET_TEXT[bucket] ?? `${bucket.replace('-', '–')} legs`;
  if (dim === 'hour') return `${bucket.replace('-', '–')} UTC`;
  return BUCKET_TEXT[bucket] ?? bucket;
}

export function footnote(m: number): string {
  return `${m} rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.`;
}

/** A row backend counts toward m (`cards::sweet::is_thin`/`is_not_tested`): thin, not_tested and
 *  dead_on_paper (a YES row P1 never traded, same as not_tested — spec §7.4) are all excluded. */
const tested = (r: SweetRow) => r.label !== 'thin' && r.label !== 'not_tested' && r.label !== 'dead_on_paper';

/** Redesign §5.1 / I3: replaces the dead headline(); the 10 bips figure shown dim under the /sweet
 *  tiles, next to the whole NO side result at that fee. Null when the backend sent no 10 bips row. */
export function feeLine(s: SweetSeries): string | null {
  const fee10 = s.fees.find((f) => f.fee_bips === 10);
  if (!fee10) return null;
  return `whole NO side at 10 bips: ${LABEL_TAG[fee10.label]}, mean ${formatMoney(fee10.mean, { signed: true })}`;
}

/** True when both sides were tested at the headline scenario and neither has a candidate. */
export function nothingPays(s: SweetSeries): boolean {
  const rows = s.rows.filter((r) => r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);
  return rows.some(tested) && !rows.some((r) => r.label === 'candidate');
}

export interface Grid {
  scenarios: { name: string; latency_ms: number | null }[];
  buckets: string[];
  cell: (bucket: string, scenario: string) => SweetRow | undefined;
}

/** Rows = buckets (sorted canonically per dimension), columns = scenarios by latency; fee rows are
 *  left out. */
export function grid(s: SweetSeries, side: SweetSide, dim: SweetDim): Grid {
  const rows = s.rows.filter((r) => r.side === side && r.dimension === dim && r.fee_bips === 0);
  const buckets: string[] = [];
  const scen = new Map<string, number | null>();
  for (const r of rows) {
    if (!buckets.includes(r.bucket)) buckets.push(r.bucket);
    scen.set(r.scenario, r.latency_ms);
  }
  const order = BUCKET_ORDER[dim];
  if (order) buckets.sort((a, b) => order.indexOf(a) - order.indexOf(b));
  const scenarios = [...scen.entries()]
    .map(([name, latency_ms]) => ({ name, latency_ms }))
    .sort((a, b) => (a.latency_ms ?? Infinity) - (b.latency_ms ?? Infinity) || a.name.localeCompare(b.name));
  const byKey = new Map(rows.map((r) => [`${r.bucket}|${r.scenario}`, r]));
  return { scenarios, buckets, cell: (b, sc) => byKey.get(`${b}|${sc}`) };
}

/** "+$0.10 [−$0.02, +$0.22]"; "—" without a mean. Money through formatMoney only. */
export function interval(r: SweetRow): string {
  if (r.mean === null) return '—';
  const m = formatMoney(r.mean, { signed: true });
  if (r.lo === null || r.hi === null) return m;
  return `${m} [${formatMoney(r.lo, { signed: true })}, ${formatMoney(r.hi, { signed: true })}]`;
}

export const ONE_LINE_CAVEAT = 'Paper, first in line. A candidate is a question for real small orders, not an answer.';

/** One scenario's rows for a side and slice, fee 0, canonical bucket order (redesign §5.1). */
export function rowsFor(s: SweetSeries, side: SweetSide, dim: SweetDim, scenario: string): SweetRow[] {
  const g = grid(s, side, dim);
  return g.buckets.map((b) => g.cell(b, scenario)).filter((r): r is SweetRow => r !== undefined);
}

const headlineCandidates = (s: SweetSeries) =>
  s.rows.filter((r) => r.label === 'candidate' && r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);

export function candidateCount(s: SweetSeries, side: SweetSide): number {
  return headlineCandidates(s).filter((r) => r.side === side).length;
}

/** Radar card (redesign §5.2): the candidate whose interval sits furthest above zero. */
export function bestCandidate(s: SweetSeries): SweetRow | null {
  let best: SweetRow | null = null;
  for (const r of headlineCandidates(s)) {
    if (r.lo === null) continue;
    if (best === null || best.lo === null || compareDecimal(r.lo, best.lo) > 0) best = r;
  }
  return best;
}

export function rowName(r: SweetRow): string {
  return r.dimension === 'all' ? `whole ${r.side.toUpperCase()} side` : bucketText(r.dimension, r.bucket);
}

export function recomputingText(computedMs: number): string {
  return `Recomputing after a premise change. The last result was ${fmtUtcHm(computedMs)} UTC; new numbers appear when it finishes.`;
}
