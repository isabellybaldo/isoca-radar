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
