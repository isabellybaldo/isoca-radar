import type { ReactElement } from 'react';
import { EmptyState } from './EmptyState';

export interface Point {
  x: number;
  y: number;
}
export interface Series {
  label: string;
  /** CSS tone class: p0 | p1 | p2 | p3 */
  tone: string;
  points: Point[];
}

export const CHART_EMPTY = 'Nothing to draw yet. The chart is patient.';
const W = 240;
const H = 60;

/** The last point per pixel column, so a week of minutes draws at most `width` steps. */
export function toColumns(points: Point[], xMin: number, xMax: number, width: number): Point[] {
  const span = Math.max(1, xMax - xMin);
  const out: Point[] = [];
  let lastCol = -1;
  for (const p of points) {
    const col = Math.floor(((p.x - xMin) / span) * (width - 1));
    if (col === lastCol && out.length > 0) out[out.length - 1] = p;
    else {
      out.push(p);
      lastCol = col;
    }
  }
  return out;
}

function stepRects(pts: [number, number][], key: string): ReactElement[] {
  const rects: ReactElement[] = [];
  for (let i = 0; i < pts.length; i++) {
    const [x, y] = pts[i];
    const next = i + 1 < pts.length ? pts[i + 1] : undefined;
    const nx = next ? next[0] : x + 1;
    rects.push(<rect key={`${key}h${i}`} x={x} y={y} width={Math.max(1, nx - x)} height={1} />);
    if (next && next[1] !== y) {
      rects.push(<rect key={`${key}v${i}`} x={nx} y={Math.min(y, next[1])} width={1} height={Math.abs(next[1] - y) + 1} />);
    }
  }
  return rects;
}

/**
 * Hand-made pixel step chart (no chart library). Values are numbers used for GEOMETRY only;
 * any money shown as text goes through `yLabels`, already formatted from the decimal strings.
 */
export function LineChart({
  series,
  label,
  xMin,
  xMax,
  markers = [],
  yLabels,
}: {
  series: Series[];
  label: string;
  xMin: number;
  xMax: number;
  markers?: number[];
  yLabels?: { top: string; bottom: string };
}) {
  const cols = series.map((s) => ({ ...s, points: toColumns(s.points, xMin, xMax, W) }));
  const all = cols.flatMap((s) => s.points);
  if (all.length === 0) return <EmptyState text={CHART_EMPTY} />;
  let yMin = Math.min(...all.map((p) => p.y));
  let yMax = Math.max(...all.map((p) => p.y));
  if (yMax === yMin) {
    yMin -= 1;
    yMax += 1;
  }
  const span = Math.max(1, xMax - xMin);
  const sx = (x: number) => Math.min(W - 1, Math.max(0, Math.round(((x - xMin) / span) * (W - 1))));
  const sy = (y: number) => 2 + Math.round((1 - (y - yMin) / (yMax - yMin)) * (H - 5));
  return (
    <figure className="chart-wrap">
      <svg className="chart" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={label}>
        {markers.map((m, i) => (
          <rect key={`m${i}`} className="marker" x={sx(m)} y={0} width={1} height={H} />
        ))}
        {cols.map((s) => (
          <g key={s.label} className={`line ${s.tone}`}>
            {stepRects(s.points.map((p) => [sx(p.x), sy(p.y)] as [number, number]), s.label)}
          </g>
        ))}
      </svg>
      {yLabels && (
        <div className="chart-axis">
          <span>low {yLabels.bottom}</span>
          <span>high {yLabels.top}</span>
        </div>
      )}
      <div className="legend">
        {series.map((s) => (
          <span key={s.label}>
            <span className={`sw ${s.tone}`} aria-hidden="true" />
            {s.label}
          </span>
        ))}
      </div>
    </figure>
  );
}
