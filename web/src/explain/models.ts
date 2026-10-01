// Panel-side models (09-27 §2): structure only; every money figure shown comes from the api.
import type { EpisodeSummary, Position } from '../api/types';
import { DAY_MS } from '../lib/time';
import type { DurationRow, PayoffModel } from '../schematics/models';

export { durationText } from '../schematics/models';
export type { DurationRow, PayoffModel } from '../schematics/models';

/** The ✗ column of an incomplete YES set: EpisodeSummary does not say which outcome is uncovered. */
export const OTHER = 'an outcome not in the set';
/** The panel draws at most six outcomes; a caption says when the basket has more. */
export const PAYOFF_MAX = 6;
const NAMES = ['A', 'B', 'C', 'D', 'E', 'F'];

export type PayoffKind = 'yes' | 'no' | 'incomplete';

export function payoffKind(kind: string): PayoffKind | null {
  if (kind === 'yes_basket') return 'yes';
  if (kind === 'no_basket') return 'no';
  if (kind === 'yes_basket_incomplete') return 'incomplete';
  return null;
}

/** Parent spec §5.7 (b): who pays in which outcome. Counts, not money. */
export function payoffStructure(kind: 'yes' | 'no', s: number, uncovered: boolean): PayoffModel {
  const all = Math.max(2, s);
  const outcomes = NAMES.slice(0, Math.min(PAYOFF_MAX, all));
  const cols = kind === 'yes' && uncovered ? [...outcomes, OTHER] : outcomes;
  const rows = outcomes.map((o, i) => ({
    name: `${kind.toUpperCase()} ${o}`,
    cells: cols.map((_, c): 0 | 1 => (kind === 'yes' ? (c === i ? 1 : 0) : c === i ? 0 : 1)),
  }));
  const guaranteed = kind === 'yes' ? 1 : all - 1;
  const basket = cols.map((c) => (c === OTHER ? 0 : guaranteed));
  return { kind, s: all, cols, rows, basket, guaranteed, broken: basket.map((b) => b < guaranteed) };
}

/** Parent spec §5.7 (e): five updates, the episode from u2 to u5, the peak at u3. The times are illustrative. */
export const EXAMPLE_EPISODE = { s_legs: 3, updates_ms: [0, 400, 1400, 2600, 4200], open_ms: 400, peak_ms: 1400, close_ms: 4200 } as const;

/** Spec 7a §9.2: a duration bar for the row, next to the example labelled example. */
export function durationRows(row: EpisodeSummary | null, nowMs: number): DurationRow[] {
  const e = EXAMPLE_EPISODE;
  const example: DurationRow = {
    kind: 'example',
    label: 'example',
    ms: e.close_ms - e.open_ms,
    open: false,
    ticks: e.updates_ms.filter((t) => t >= e.open_ms).map((t) => t - e.open_ms),
  };
  if (row === null) return [example];
  return [
    example,
    { kind: 'row', label: row.event_title ?? row.id, ms: row.duration_ms ?? Math.max(0, nowMs - row.opened_ms), open: row.duration_ms === null, ticks: [] },
  ];
}

export const positionKey = (p: Position): string => `${p.account_id}|${p.market_id}|${p.outcome}`;

const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/;

/**
 * 09-27 §7.4: days to resolution from `Position.end_date` (stored text), or "unknown". A date-only value
 * is the END of that UTC day (D-07): on the end date itself it is not past.
 */
export function daysText(endDate: string | null, nowMs: number): string {
  if (endDate === null) return 'resolution date unknown';
  const t = Date.parse(endDate);
  if (Number.isNaN(t)) return 'resolution date unknown';
  const end = DATE_ONLY.test(endDate) ? t + DAY_MS : t;
  const d = Math.ceil((end - nowMs) / DAY_MS);
  if (d <= 0) return 'past its end date, awaiting resolution';
  return d === 1 ? 'resolves in 1 day' : `resolves in ${d} days`;
}
