// Prices on the 0.01 grid as whole ticks (string math only; shared by defaults.ts and ladderDrag.ts).
/** '0.55' → 55 ('0.5' → 50). */
export function toTick(p: string): number {
  const [i = '0', f = ''] = p.split('.');
  const t = Number(i) * 100 + Number(f.padEnd(2, '0').slice(0, 2));
  return Number.isFinite(t) ? t : 50;
}

/** 55 → '0.55', clamped to 0.01–0.99 (a non-number gives 0.50, never 'NaN'). */
export function fromTick(t: number): string {
  const c = Number.isFinite(t) ? Math.min(99, Math.max(1, Math.round(t))) : 50;
  return `0.${String(c).padStart(2, '0')}`;
}
