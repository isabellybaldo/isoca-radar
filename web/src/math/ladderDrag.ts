// One way to move a price level on a side of a book without breaking its strict order (rulings R8, 7b-B).
// Used by section a (bids fall and asks rise from the best) and section d (a leg's asks, cheapest first).
import type { Level } from '../wasm/types';
import { fromTick, toTick } from './ticks';

export type Side = 'bids' | 'asks';

/** The ticks level i may take so every level on its side still fits strictly in order inside 1–99 (bids fall and
 *  asks rise from the best): the levels behind it need room. A crossed book between the sides stays possible. */
export function room(side: Side, n: number, i: number): [number, number] {
  return side === 'bids' ? [n - i, 99 - i] : [1 + i, 99 - (n - 1 - i)];
}

/** Moves level i to `want` ticks (clamped to its room) and pushes its same-side neighbours along, so the side stays
 *  strictly ordered with no two levels at one price. Works on q = p for asks and 100 − p for bids (ascending). */
export function drag(rows: Level[], side: Side, i: number, want: number): Level[] {
  const flip = (t: number) => (side === 'bids' ? 100 - t : t);
  const [lo, hi] = room(side, rows.length, i);
  const t = Math.min(hi, Math.max(lo, want));
  const q = rows.map((l) => flip(toTick(l.p)));
  q[i] = flip(t);
  for (let k = i - 1; k >= 0; k--) q[k] = Math.min(q[k], q[k + 1] - 1);
  for (let k = i + 1; k < q.length; k++) q[k] = Math.max(q[k], q[k - 1] + 1);
  return rows.map((l, k) => ({ ...l, p: fromTick(flip(q[k])) }));
}
