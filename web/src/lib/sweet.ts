import type { SweetLabel, SweetRow, SweetSeries } from '../api/types';
import { formatMoney } from './money';

/** Spec 5d §7.6, verbatim. */
export const SWEET_CAVEAT =
  "Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.";
export const NOTHING_PAYS = "Nothing here pays at 300 ms. Don't trade this, or get closer.";
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

/** Spec 5d §8: one sentence per side at the headline scenario, the 10 bips figure beside it (NO). */
export function headline(s: SweetSeries, side: SweetSide): string {
  const rows = s.rows.filter((r) => r.side === side && r.scenario === HEADLINE_SCENARIO && r.fee_bips === 0);
  const cands = rows.filter((r) => r.label === 'candidate');
  const fee10 = s.fees.find((f) => f.fee_bips === 10);
  const fee = side === 'no' && fee10
    ? ` (whole NO side at 10 bips: ${LABEL_TAG[fee10.label]}, mean ${formatMoney(fee10.mean, { signed: true })})`
    : '';
  const SIDE = side.toUpperCase();
  if (cands.length === 0) return `${SIDE}: 0 candidates in ${rows.filter(tested).length} tested rows${fee}.`;
  const named = cands
    .filter((r) => r.dimension !== 'all')
    .slice(0, 3)
    .map((r) => bucketText(r.dimension, r.bucket));
  const list = named.length > 0 ? `: ${named.join(', ')}` : '';
  return `${SIDE}: ${cands.length} ${cands.length === 1 ? 'candidate' : 'candidates'}${list}${fee}.`;
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
