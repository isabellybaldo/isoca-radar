// Starting states (09-27 §6.2, 7b §6.2). Prices move on the 0.01 grid as whole ticks (string math only).
// Each literal equals a 7b-A fixture input; sections-abc.test.tsx pins that.
import type { LegStatus, Level } from '../wasm/types';

/** '0.55' → 55 ('0.5' → 50). */
export function toTick(p: string): number {
  const [i = '0', f = ''] = p.split('.');
  return Number(i) * 100 + Number(f.padEnd(2, '0').slice(0, 2));
}

/** 55 → '0.55', clamped to 0.01–0.99. */
export function fromTick(t: number): string {
  const c = Math.min(99, Math.max(1, Math.round(t)));
  return `0.${String(c).padStart(2, '0')}`;
}

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
