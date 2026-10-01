// Section f's engine input (spec 7b §3.4, §6.2 f): YES-book levels at open, at t_x and at unwind time.
// A NO buy walks the YES bids (mirrored); a NO unwind sells into the YES asks (mirrored).
import type { BasketIn, Fee, Level } from '../wasm/types';
import { fromTick, toTick } from './ticks';

export interface LegBook {
  bids: Level[];
  asks: Level[];
}
export interface FState {
  /** Per leg, the depth left at t_x in % of the open book. */
  depth: number[];
  /** Per leg, ticks the unwind book moved against us. */
  against: number[];
  q: number;
  fees: boolean;
}

/** fee = shares × 0.07 × p(1 − p): the crypto schedule (crates/arb-core/tests/fee.rs). */
export const CRYPTO_FEE: Fee = { rate: '0.07', exponent: 1 };

/** Sizes cut to `pct` % in whole shares (sizes are shares, not money); a level cut to 0 is gone, so a leg can
 *  end with an empty side, which the engine accepts. Applied to the side a NO buy walks (the YES bids); the
 *  t_x asks stay, as in 7b-A's fixtures. */
export function drain(levels: Level[], pct: number): Level[] {
  return levels.map((l) => ({ p: l.p, s: String(Math.floor((Number(l.s) * pct) / 100)) })).filter((l) => l.s !== '0');
}

/** For a NO holder the unwind book moves against us when the YES asks rise (the NO bids fall). Prices stay on
 *  the 0.01–0.99 grid. */
export function against(b: LegBook, ticks: number): LegBook {
  return { bids: b.bids, asks: b.asks.map((l) => ({ p: fromTick(toTick(l.p) + ticks), s: l.s })) };
}

/** 7b-A's basket_json input: kind "no", the fee on every leg, min_order and convert_fee_bips left to the
 *  engine's defaults ("5", the sim's min_order_shares, and "0"). */
export function basketInput(books: LegBook[], st: FState): BasketIn {
  return {
    kind: 'no',
    legs: books.map((b, i) => ({
      open: b,
      t_x: { bids: drain(b.bids, st.depth[i] ?? 100), asks: b.asks },
      unwind: against(b, st.against[i] ?? 0),
      fee: st.fees ? CRYPTO_FEE : null,
    })),
    q_request: String(st.q),
  };
}
