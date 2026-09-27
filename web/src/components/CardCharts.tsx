import type { CardBody, CardSeries } from '../api/types';
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
 * Survival step curves on the τ grid (G2): x is the grid index (0.1 s … 24 h, log-like), y is Ŝ.
 * F7 (controller ruling): medians are marked, per spec §5.2 — a vertical marker sits at the first
 * grid point of each curve where survival reaches that curve's own median duration.
 */
export function Survival({ curves }: { curves: NonNullable<CardSeries['curves']> }) {
  const n = Math.max(1, ...curves.map((c) => c.points.length));
  const series: Series[] = curves.map((c, i) => ({
    label: c.label,
    tone: TONES[i % TONES.length],
    // geometry only: survival probabilities are never shown as text here
    points: c.points.filter((p) => p.s !== '').map((p, j) => ({ x: j, y: Number(p.s) })),
  }));
  const markers = curves
    .map((c) => {
      if (c.median_ms === null || c.points.length === 0) return null;
      const target = Number(c.median_ms);
      const idx = c.points.findIndex((p) => p.t_ms >= target);
      return idx === -1 ? c.points.length - 1 : idx;
    })
    .filter((m): m is number => m !== null);
  return (
    <LineChart
      label="survival by duration, both sides, both methods, medians marked"
      xMin={0}
      xMax={n - 1}
      series={series}
      markers={markers}
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
