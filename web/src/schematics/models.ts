// Spec 7b §7.2: the view models the public /math page shares with the explain panel. They moved here from
// src/explain/models.ts (which re-exports them), so no public schematic imports dashboard code.
import { fmtDuration } from '../lib/time';

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

export interface DurationRow {
  kind: 'example' | 'row';
  label: string;
  ms: number;
  open: boolean;
  /** Update times after the open (the example only). */
  ticks: number[];
}

export function durationText(r: DurationRow): string {
  if (r.kind === 'example') return `example: ${fmtDuration(r.ms)}, from u2 to u5, peak at u3`;
  return r.open ? `${r.label}: still open, ${fmtDuration(r.ms)} so far` : `${r.label}: ${fmtDuration(r.ms)}`;
}
