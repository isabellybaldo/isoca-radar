// Starting states (09-27 §6.2, 7b §6.2). Prices move on the 0.01 grid as whole ticks (string math only).
// Each literal equals a 7b-A fixture input; sections-abc.test.tsx pins that.
import type { LegStatus, Level } from '../wasm/types';
import type { FState, LegBook } from './basket';
import { drag, room } from './ladderDrag';
import { fromTick, toTick } from './ticks';

export { fromTick, toTick } from './ticks';

export const LEGS = ['A', 'B', 'C', 'D', 'E', 'F'];

/** Parent §5.7 (a): the Fed book, best first; the same levels as 7b-A's fixture mirror_json/fed-book. */
export const FED_BIDS: Level[] = [
  { p: '0.54', s: '5603.74' }, { p: '0.53', s: '85887.16' }, { p: '0.52', s: '42239.53' }, { p: '0.51', s: '28133.09' },
];
export const FED_ASKS: Level[] = [
  { p: '0.55', s: '32356.03' }, { p: '0.56', s: '252281.27' }, { p: '0.57', s: '23963.36' }, { p: '0.58', s: '5637.30' },
];

/** Section b's optional prices, one per leg (S up to 6): the YES asks or the NO bids, in ticks. */
export const PAYOFF_TICKS = [30, 31, 33, 30, 30, 30];
/** The implication's two prices, in ticks: A's bid and B's ask (the only two the engine reads). A priced "A ⇒ B" with no edge. */
export const IMPLICATION_TICKS = [29, 31];

export interface ClassifyLeg {
  status: LegStatus;
  bid: number;
  ask: number;
}
/** Section c: three live legs, Σ YES bids 1.05, a small NO-basket edge (7b-A's fixture classify_json/no-edge-3). */
export const CLASSIFY_LEGS: ClassifyLeg[] = [
  { status: 'live', bid: 40, ask: 41 },
  { status: 'live', bid: 35, ask: 36 },
  { status: 'live', bid: 30, ask: 31 },
];

/** Section d: three legs, three ask levels each (the asks of the outcome bought, cheapest first); Σ best asks
 *  0.94, a positive edge (sections-def.test.tsx has the engine confirm it). */
export const WALK_LEGS: Level[][] = [
  [{ p: '0.30', s: '40' }, { p: '0.32', s: '60' }, { p: '0.35', s: '100' }],
  [{ p: '0.31', s: '50' }, { p: '0.33', s: '50' }, { p: '0.36', s: '100' }],
  [{ p: '0.33', s: '30' }, { p: '0.34', s: '70' }, { p: '0.37', s: '100' }],
];
/** The fourth leg "add a leg" appends. */
export const NEW_WALK_LEG: Level[] = [{ p: '0.05', s: '50' }, { p: '0.06', s: '50' }, { p: '0.07', s: '100' }];

/** "exactly 1.00" (09-27 §6.2 d): shift leg A's levels so the best asks sum to 1.00. Each shifted level is first
 *  held inside its room (ladderDrag), then the side goes through the same push-the-neighbours move as a drag, so
 *  it stays strictly ascending inside 0.01-0.99 even when the shift runs into an edge (the sum is then as near
 *  1.00 as the grid allows). */
export function exactlyOne(legs: Level[][]): Level[][] {
  const sum = legs.reduce((a, l) => a + (l[0] ? toTick(l[0].p) : 0), 0);
  const shift = 100 - sum;
  return legs.map((l, i) => {
    if (i !== 0 || l.length === 0) return l;
    const held = l.map((x, k) => {
      const [lo, hi] = room('asks', l.length, k);
      return { ...x, p: fromTick(Math.min(hi, Math.max(lo, toTick(x.p) + shift))) };
    });
    return drag(held, 'asks', 0, toTick(held[0].p));
  });
}

/** Parent §5.7 (e): five updates of a 3-leg NO basket, YES bids in ticks (asks one tick above, 100 deep):
 *  u1 Σ 0.99, u2 1.02 (open), u3 1.06 (peak), u4 1.02, u5 0.99 (close). */
export const EPISODE_T = [0, 400, 900, 1400, 1900];
export const EPISODE_BIDS: number[][] = [[33, 33, 33], [35, 34, 33], [37, 35, 34], [35, 34, 33], [33, 33, 33]];

/** Section e's presets: the six LADDER rungs of config/policies.toml (defaults.test.ts reads the file and checks them). */
export const PRESETS = [
  { name: 'rung-0', proc_ms: 0, net_ms: 0, match_ms: 0 },
  { name: 'rung-25', proc_ms: 0, net_ms: 0, match_ms: 25 },
  { name: 'colo-idle', proc_ms: 1, net_ms: 1, match_ms: 50 },
  { name: 'rung-100', proc_ms: 5, net_ms: 45, match_ms: 50 },
  { name: 'rung-200', proc_ms: 5, net_ms: 95, match_ms: 100 },
  { name: 'sp-idle', proc_ms: 5, net_ms: 145, match_ms: 150 },
] as const;

/** Section f (7b §6.2), exactly 7b-A's fixture basket_json/no-all-fill: a 3-leg NO basket, YES bids
 *  0.40 / 0.35 / 0.30 × 100 (NO asks 0.60 / 0.65 / 0.70, Σ 1.95 < 2), asks a tick up. */
export const F_BOOKS: LegBook[] = [
  { bids: [{ p: '0.40', s: '100' }], asks: [{ p: '0.41', s: '100' }] },
  { bids: [{ p: '0.35', s: '100' }], asks: [{ p: '0.36', s: '100' }] },
  { bids: [{ p: '0.30', s: '100' }], asks: [{ p: '0.31', s: '100' }] },
];
/** t_x and unwind books equal the open ones, 50 shares: fixture no-all-fill. */
export const F_START: FState = { depth: [100, 100, 100], against: [0, 0, 0], q: 50, fees: false };
/** "the usual burst" = fixture no-one-leg-empty-burst: leg B without bids at t_x, every unwind book 5 ticks against us. */
export const F_BURST: FState = { depth: [100, 0, 100], against: [5, 5, 5], q: 50, fees: false };
