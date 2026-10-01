// Spec 7b §7.2: the view models the public /math page shares with the explain panel. They moved here from
// src/explain/models.ts (which re-exports them), so no public schematic imports dashboard code.
import { fmtDuration } from '../lib/time';

export interface PayoffModel {
  kind: 'yes' | 'no' | 'implication';
  /** The basket's real number of outcomes (may exceed the rows drawn). */
  s: number;
  cols: string[];
  /** 1 where the row's token pays $1 in that column; null in a column that cannot happen. */
  rows: { name: string; cells: (0 | 1 | null)[] }[];
  /** What the whole basket pays in each column, over all s outcomes; null where the column cannot happen.
   *  A string is the engine's decimal, shown as sent (the panel's own model uses numbers). */
  basket: (number | string | null)[];
  /** Parent spec §5.7 (b): 1 for a YES basket, s − 1 for a NO basket, 1 for the implication. */
  guaranteed: number | string;
  broken: boolean[];
  /** 09-27 §3.1: false for a column that cannot happen (the implication's A∧¬B); absent means all can. */
  possible?: boolean[];
}

export interface DurationRow {
  /** 'order' (section e): when our order lands, drawn in the accent colour. */
  kind: 'example' | 'row' | 'order';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open. */
  ticks: number[];
}

export function durationText(r: DurationRow): string {
  if (r.kind === 'example') return `example: ${fmtDuration(r.ms)}, from u2 to u5, peak at u3`;
  return r.open ? `${r.label}: still open, ${fmtDuration(r.ms)} so far` : `${r.label}: ${fmtDuration(r.ms)}`;
}
