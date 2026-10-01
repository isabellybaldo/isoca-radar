// The one geometry helper the schematics share (7b-B R12/R14). A decimal string becomes a number here, for
// drawing (SVG coordinates, bar sizes) and for pairing prices; no value shown as text goes through it.

/** A decimal string as geometry; malformed or not finite draws at zero instead of NaN. */
export const geo = (v: string): number => {
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
};

/** As `geo`, but a negative value draws at zero too (sizes, prices). */
export const geoPos = (v: string): number => Math.max(0, geo(v));

/** A 0.01-grid price as its whole tick (0.54 -> 54), so two prices compare without float noise. */
export const tick = (p: string): number => Math.round(geoPos(p) * 100);

/** The viewBox every schematic here draws in (labels are HTML, so only the ratio matters). */
export const PLOT_W = 300;
export const PLOT_H = 120;

/** An axis zoomed onto what is drawn, in whole 0.01 ticks, so both ends can be written without float text. */
export interface TickDomain {
  lo: number;
  hi: number;
}

/** Whole ticks as a decimal string (109 -> "1.09"): integer math only, for axis labels. */
export const tickText = (t: number): string => `${Math.floor(t / 100)}.${String(t % 100).padStart(2, '0')}`;

/** A decimal string in ticks, with the float noise of the multiplication removed (0.94 * 100 is not 94). */
const inTicks = (v: string): number => Math.round(geoPos(v) * 1e8) / 1e6;

/** Where a value in ticks falls in a domain, from 0 at `lo` to 1 at `hi`. */
export const along = (d: TickDomain, v: string): number => (inTicks(v) - d.lo) / (d.hi - d.lo);

const MIRROR_PAD = 2;
const MIRROR_MIN_HALF = 5;

/** Drawing a's price axis: symmetric around 0.50 (p and 1 − p are mirror images, the 0.50 line stays in the
 *  middle), two ticks wider than the farthest price drawn, never narrower than ten ticks, never outside 0 to 1. */
export function mirrorDomain(prices: string[]): TickDomain {
  const far = Math.max(0, ...prices.map((p) => Math.abs(inTicks(p) - 50)));
  const half = Math.min(50, Math.max(MIRROR_MIN_HALF, Math.ceil(far) + MIRROR_PAD));
  return { lo: 50 - half, hi: 50 + half };
}

const COST_PAD = 0.08;
const COST_MIN_SPAN = 4;

/** Drawing d's cost axis: from the lowest to the highest value drawn (step costs and what a basket pays),
 *  padded by a tick or 8% of the range, at least four ticks tall, never below zero. Nothing to draw: 0 to 1. */
export function costDomain(values: string[]): TickDomain {
  if (values.length === 0) return { lo: 0, hi: 100 };
  const t = values.map(inTicks);
  const min = Math.floor(Math.min(...t));
  const max = Math.ceil(Math.max(...t));
  const pad = Math.max(1, Math.ceil((max - min) * COST_PAD));
  const grow = Math.max(0, Math.ceil((COST_MIN_SPAN - (max - min + 2 * pad)) / 2));
  const lo = Math.max(0, min - pad - grow);
  return { lo, hi: Math.max(max + pad + grow, lo + COST_MIN_SPAN) };
}
