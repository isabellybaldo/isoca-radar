// Engine outputs as the models of the reused 7a views (spec 7b section 1, change 3).
import type { DurationRow, PayoffModel } from '../schematics/models';
import type { EpisodeModel, PayoffKind, PayoffMatrix } from '../wasm/types';

/** payoff_json's matrix as PayoffGrid's model. The basket row keeps the engine's decimal strings (shown as
 *  sent, never through Number()); a cell is "0" or "1" (a $1 payoff or none), null where the column cannot happen. */
export function matrixToPayoff(m: PayoffMatrix, kind: PayoffKind): PayoffModel {
  const cell = (v: string | null): 0 | 1 | null => (v === null ? null : v === '0' ? 0 : 1);
  return {
    kind,
    s: m.rows.length,
    cols: m.cols,
    rows: m.rows.map((name, i) => ({ name, cells: (m.cells[i] ?? []).map(cell) })),
    basket: m.basket,
    guaranteed: m.guaranteed,
    broken: m.broken,
    possible: m.possible,
  };
}

/** Which price each implication leg feeds (crates/arb-wasm/src/api/payoff.rs, top_edge_gross): leg A is bought
 *  as NO_A at 1 - bid, leg B as YES_B at the ask. The other side of each is read by nothing, so the page offers
 *  no control for it (ruling R10). */
export const IMPLICATION_PRICE_USE = [
  { leg: 'A', side: 'bid', label: 'A bid (we buy NO_A at 1 − bid)' },
  { leg: 'B', side: 'ask', label: 'B ask (we buy YES_B at the ask)' },
] as const;

/** Section e on DurationBars: the episode on the table from its open (update ticks after it), and our order
 *  landing at t_x, on one time scale. Nothing when the episode never opened. */
export function episodeRows(m: EpisodeModel): DurationRow[] {
  const open = m.open_ms;
  if (open === null) return [];
  const last = m.updates.length > 0 ? m.updates[m.updates.length - 1].t_ms : open;
  const end = m.close_ms ?? Math.max(open, last);
  const rows: DurationRow[] = [
    { kind: 'row', label: 'on the table', ms: Math.max(0, end - open), open: m.close_ms === null, ticks: m.updates.map((u) => u.t_ms - open).filter((t) => t > 0) },
  ];
  if (m.t_x !== null) rows.push({ kind: 'order', label: 'our order lands', ms: Math.max(0, m.t_x - open), open: false, ticks: [] });
  return rows;
}
