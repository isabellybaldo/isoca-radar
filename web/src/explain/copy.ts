// Spec 7a §9.2/§9.4: every answer is one drawing plus at most two sentences (D-04); every money figure is
// labelled "on the table", "paper" or "captured at N ms"; no real-money words.
import { EPISODE_DEF } from '../help';
import { formatMoneyCents } from '../lib/money';
import { fmtDuration } from '../lib/time';

/** A money figure inside a sentence, or "not measured" (a missing figure never reads $0.00). */
export const money = (v: string | null): string => (v === null ? 'not measured' : formatMoneyCents(v, { signed: true }));

export const COPY = {
  example: 'example: a NO basket with three outcomes, no money figure',
  makeMoneyYes: 'One YES of every outcome pays $1 whoever wins, so buying them for less leaves the gap on the table.',
  makeMoneyNo: 'One NO of every outcome converts to S − 1 dollars at once, so buying them for less leaves the gap on the table.',
  makeMoneyIncomplete: 'The set misses an outcome (✗): if that one wins, the basket pays nothing, so the gap is not guaranteed.',
  whatEpisode: EPISODE_DEF,
  episodeBar: 'The bar is how long the edge stayed on the table.',
  caughtNotFilled: 'In time is not filled: the first question shows what arriving in time was worth on paper.',
  caughtWaiting: 'Waiting for the episode to load.',
  locked: 'A YES basket pays only at resolution, so its cost stays locked on paper until the event ends. NO baskets convert at once and lock nothing.',
  nothingLocked: 'Nothing is locked: no paper position is waiting for a resolution.',
};

export function caughtText(durationMs: number | null): string {
  return durationMs === null
    ? 'Still open: every rung would have arrived while it was on the table so far.'
    : `It stayed on the table for ${fmtDuration(durationMs)}: rungs faster than that arrive in time.`;
}

export const onTable100 = (v: string | null): string => `On the table at $100 spent: ${money(v)}, at the peak.`;
export const onTableDepth = (v: string): string => `On the table at full depth: ${money(v)}.`;
