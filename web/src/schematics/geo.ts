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

/** A decimal string in ticks, with the float noise of the multiplication removed (0.94 * 100 is not 94); a
 *  negative value is 0. Null when the string is not a number, or too large to place on an axis. */
const ticksOf = (v: string): number | null => {
  if (v.trim() === '') return null;
  const t = Math.round(Math.max(0, Number(v)) * 1e8) / 1e6;
  return Number.isFinite(t) ? t : null;
};

/** As `ticksOf`, for drawing: what cannot be placed draws at zero instead of NaN. */
const inTicks = (v: string): number => ticksOf(v) ?? 0;

/** The values a domain is built from: what cannot be placed is left out, so it cannot un-zoom the axis. */
const placed = (values: string[]): number[] => values.flatMap((v) => ticksOf(v) ?? []);

/** Where a value in ticks falls in a domain, from 0 at `lo` to 1 at `hi`. */
export const along = (d: TickDomain, v: string): number => (inTicks(v) - d.lo) / (d.hi - d.lo);

/** What a complete basket pays, in ticks: one dollar. */
export const STACK_FULL = 100;

/** The longest stack that is drawn, in ticks (ten thousand dollars): past it the dollar line would have no
 *  share of the bar left to be drawn in. */
export const STACK_MAX = 1e6;
/** The smallest share of the bar that can be written: a column's width is a percentage to four decimals. */
export const STACK_MIN_SHARE = 5e-7;

/** Prices laid end to end, in ticks, against the `STACK_FULL` a basket pays (the story's price stack). Each
 *  span runs from where the one before ended. A price takes no width (its span is empty, and the drawing names
 *  it as not drawn) when it cannot be placed (malformed, empty, negative, zero, too large for `inTicks`), when
 *  it would take the running sum past `STACK_MAX` (which also keeps the sum finite: two prices of 1e300 are
 *  each finite and their sum is not), or when its share of the bar would round to zero. So every number here
 *  is finite and every span that has a width can be drawn. The sums are made on whole millionths of a tick,
 *  so 0.33 + 0.33 + 0.34 ends exactly on the line. `hi` is the axis' end: a tenth past the farther of the
 *  stack's end and the line, so neither sits on the frame. */
export function stackSpans(prices: string[]): { spans: { from: number; to: number }[]; end: number; hi: number } {
  const MICRO = 1e6;
  let sum = 0;
  const fits = prices.map((p) => {
    const m = Math.round(inTicks(p) * MICRO);
    if (!(m > 0) || !(sum + m <= STACK_MAX * MICRO)) return 0;
    sum += m;
    return m;
  });
  // Shares are judged on the axis of everything that fits; leaving the slivers out only shortens that axis,
  // so what stays keeps at least the share it was judged by.
  const axis = Math.max(STACK_FULL * MICRO, sum) * 1.1;
  let at = 0;
  const spans = fits.map((m) => {
    const from = at;
    if (m / axis >= STACK_MIN_SHARE) at += m;
    return { from: from / MICRO, to: at / MICRO };
  });
  const end = at / MICRO;
  const far = Math.max(STACK_FULL, end);
  return { spans, end, hi: far + far / 10 };
}

const MIRROR_PAD = 2;
const MIRROR_MIN_HALF = 5;

/** Drawing a's price axis: symmetric around 0.50 (p and 1 − p are mirror images, the 0.50 line stays in the
 *  middle), two ticks wider than the farthest price drawn, never narrower than ten ticks, never outside 0 to 1. */
export function mirrorDomain(prices: string[]): TickDomain {
  const far = Math.max(0, ...placed(prices).map((t) => Math.abs(t - 50)));
  const half = Math.min(50, Math.max(MIRROR_MIN_HALF, Math.ceil(far) + MIRROR_PAD));
  return { lo: 50 - half, hi: 50 + half };
}

const COST_PAD = 0.08;
const COST_MIN_SPAN = 4;

/** Drawing d's cost axis: from the lowest to the highest value drawn (step costs and what a basket pays),
 *  padded by a tick or 8% of the range, at least four ticks tall, never below zero. Nothing to draw: 0 to 1. */
export function costDomain(values: string[]): TickDomain {
  const t = placed(values);
  if (t.length === 0) return { lo: 0, hi: 100 };
  const min = Math.floor(Math.min(...t));
  const max = Math.ceil(Math.max(...t));
  const pad = Math.max(1, Math.ceil((max - min) * COST_PAD));
  const grow = Math.max(0, Math.ceil((COST_MIN_SPAN - (max - min + 2 * pad)) / 2));
  const lo = Math.max(0, min - pad - grow);
  return { lo, hi: Math.max(max + pad + grow, lo + COST_MIN_SPAN) };
}
