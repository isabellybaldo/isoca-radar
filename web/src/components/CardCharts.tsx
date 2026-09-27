import type { CardBody, CardSeries } from '../api/types';
import { fmtDuration } from '../lib/time';
import { LineChart, type Series } from './LineChart';

/** Two pixel bars (G1). Counts, not money. */
export function SideBars({ bars }: { bars: NonNullable<CardSeries['bars']> }) {
  const max = Math.max(1, ...bars.map((b) => b.value));
  const W = 100;
  return (
    <svg className="bars" viewBox={`0 0 ${W} ${bars.length * 12}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={bars.map((b) => `${b.label} ${b.value}`).join(', ')}>
      {bars.map((b, i) => (
        <rect key={b.label} className={`bar b${i}`} x={0} y={i * 12 + 1} width={b.value <= 0 ? 0 : Math.max(1, Math.round((b.value / max) * W))} height={10} />
      ))}
    </svg>
  );
}

const TONES = ['p0', 'p1', 'p2', 'p3'];

/**
 * The union of every curve's real `t_ms` values, ascending — the survival chart's one shared time
 * axis (fix round 1, controller ruling on F7): a duration must land at the same x on every curve,
 * so the always-18-point Kaplan–Meier curve and the empirical curve (which skips grid points it
 * never measured, so it is shorter) still compare like for like instead of by raw array position.
 */
export function sharedTimeGrid(curves: NonNullable<CardSeries['curves']>): number[] {
  const ts = new Set<number>();
  for (const c of curves) for (const p of c.points) ts.add(p.t_ms);
  return [...ts].sort((a, b) => a - b);
}

/**
 * Position of a real time on that shared grid — pixel geometry only, never shown as text (plan
 * ruling 8). Exact for a time already on the grid; linearly interpolated between its two nearest
 * grid neighbours otherwise, so a median that falls between two grid points still lands at the
 * right x instead of snapping to the nearest one.
 */
export function xOfTime(grid: number[], t: number): number {
  if (grid.length === 0) return 0;
  if (t <= grid[0]) return 0;
  if (t >= grid[grid.length - 1]) return grid.length - 1;
  let lo = 0;
  while (lo + 1 < grid.length && grid[lo + 1] < t) lo++;
  const tLo = grid[lo];
  const tHi = grid[lo + 1];
  return tHi === tLo ? lo : lo + (t - tLo) / (tHi - tLo);
}

/**
 * Survival step curves (G2), all sharing one time axis, y is Ŝ. F7 (controller ruling): medians are
 * marked, per spec §5.2 — a vertical marker sits at each curve's own median duration, on that same
 * shared axis. Axis-range text goes through `fmtDuration`, never a raw number.
 */
export function Survival({ curves }: { curves: NonNullable<CardSeries['curves']> }) {
  const grid = sharedTimeGrid(curves);
  const series: Series[] = curves.map((c, i) => ({
    label: c.label,
    tone: TONES[i % TONES.length],
    // geometry only: survival probabilities are never shown as text here
    points: c.points.filter((p) => p.s !== '').map((p) => ({ x: xOfTime(grid, p.t_ms), y: Number(p.s) })),
  }));
  const markers = curves
    .map((c) => (c.median_ms === null ? null : xOfTime(grid, Number(c.median_ms))))
    .filter((m): m is number => m !== null);
  return (
    <LineChart
      label="survival by duration, both sides, both methods, medians marked"
      xMin={0}
      xMax={Math.max(1, grid.length - 1)}
      series={series}
      markers={markers}
      xLabels={grid.length > 0 ? { left: fmtDuration(grid[0]), right: fmtDuration(grid[grid.length - 1]) } : undefined}
      yLabels={{ bottom: '0', top: '1' }}
    />
  );
}

/** 25 bp bins per side (G4): net filled, gross behind it. */
export function Histogram({ bins }: { bins: NonNullable<CardSeries['bins']> }) {
  const sides = [...new Set(bins.map((b) => b.side))];
  return (
    <div className="hist">
      {sides.map((side) => {
        const bs = bins.filter((b) => b.side === side);
        const max = Math.max(1, ...bs.map((b) => Math.max(b.net, b.gross)));
        const h = (v: number) => (v <= 0 ? 0 : Math.max(1, Math.round((v / max) * 24)));
        return (
          <figure key={side}>
            <svg className="histo" viewBox={`0 0 ${bs.length * 4} 24`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={`${side} net edge, 25 bp bins`}>
              {bs.map((b, i) => (
                <g key={b.lo_bps}>
                  <rect className="gross" x={i * 4} y={24 - h(b.gross)} width={4} height={h(b.gross)} />
                  <rect className="net" x={i * 4 + 1} y={24 - h(b.net)} width={2} height={h(b.net)} />
                </g>
              ))}
            </svg>
            <figcaption className="dim">{side} · 0 to 500+ bp</figcaption>
          </figure>
        );
      })}
    </div>
  );
}

export function CardChart({ card }: { card: CardBody }) {
  const s = card.series;
  if (s.bars && s.bars.length > 0) return <SideBars bars={s.bars} />;
  if (s.curves && s.curves.length > 0) return <Survival curves={s.curves} />;
  if (s.bins && s.bins.length > 0) return <Histogram bins={s.bins} />;
  return null;
}
