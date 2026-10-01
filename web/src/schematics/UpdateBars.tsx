import { Money } from '../components/Money';
import { PLOT_W, geoPos } from './geo';

const H = 48;

/** Parent 5.7 (e): the net edge classify found at each update (none: no bar), the first peak marked. The update
 *  names, times and amounts are HTML under the drawing (the SVG stretches). */
export function UpdateBars({ updates, peakMs }: { updates: { t_ms: number; net: string | null }[]; peakMs: number | null }) {
  const vals = updates.map((u) => geoPos(u.net ?? '0'));
  const max = Math.max(0, ...vals) || 1;
  const slot = PLOT_W / Math.max(1, updates.length);
  const bw = slot * 0.6;
  const peak = peakMs === null ? -1 : updates.findIndex((u, i) => u.t_ms === peakMs && vals[i] > 0);
  return (
    <figure className="ubars" aria-label="the edge at each update">
      <svg className="ub-svg" viewBox={`0 0 ${PLOT_W} ${H}`} preserveAspectRatio="none" role="img" aria-label="net edge per update">
        {updates.map((u, i) => {
          const h = (vals[i] / max) * (H - 2);
          return <rect key={`${i}|${u.t_ms}`} className={i === peak ? 'ub-bar peak' : 'ub-bar'} x={i * slot + (slot - bw) / 2} y={H - h} width={bw} height={h} />;
        })}
        <line className="ub-zero" x1={0} x2={PLOT_W} y1={H - 0.5} y2={H - 0.5} />
      </svg>
      <ol className="ub-axis" aria-label="updates">
        {updates.map((u, i) => (
          <li key={`${i}|${u.t_ms}`}>
            <b>{i === peak ? `u${i + 1} · peak` : `u${i + 1}`}</b>
            <span>{`${u.t_ms} ms`}</span>
            <span>{u.net === null ? 'no edge' : <Money value={u.net} signed cents />}</span>
          </li>
        ))}
      </ol>
      <figcaption className="dim">net edge on the table at each update</figcaption>
    </figure>
  );
}
