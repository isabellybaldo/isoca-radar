// Panel-side models (09-27 §2): structure only; every money figure shown comes from the api.
import type { EpisodeSummary } from '../api/types';
import { fmtDuration } from '../lib/time';

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

export interface PayoffModel {
  kind: 'yes' | 'no';
  /** The basket's real number of outcomes (may exceed the rows drawn). */
  s: number;
  cols: string[];
  rows: { name: string; cells: (0 | 1)[] }[];
  /** What the whole basket pays in each column, over all s outcomes. */
  basket: number[];
  /** Parent spec §5.7 (b): 1 for a YES basket, s − 1 for a NO basket. */
  guaranteed: number;
  broken: boolean[];
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

export interface DurationRow {
  kind: 'example' | 'row';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open (the example only). */
  ticks: number[];
}

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

export function durationText(r: DurationRow): string {
  if (r.kind === 'example') return `example: ${fmtDuration(r.ms)}, from u2 to u5, peak at u3`;
  return r.open ? `${r.label}: still open, ${fmtDuration(r.ms)} so far` : `${r.label}: ${fmtDuration(r.ms)}`;
}
