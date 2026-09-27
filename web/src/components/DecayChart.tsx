import type { DecayPoint } from '../api/types';

/**
 * Edge decay (spec §6.2): latency on x, capture rate on y, one point per scenario, and the
 * measured marker. Rates are parsed for geometry only; every number shown as text is the string sent.
 */
export function DecayChart({ points, measured }: { points: DecayPoint[]; measured: { latency_ms: number; host: string } | null }) {
  const W = 240;
  const H = 80;
  const xMax = Math.max(1, ...points.map((p) => p.latency_ms), measured?.latency_ms ?? 0) * 1.1;
  const rates = points.map((p) => (p.rate_pct === null ? null : Number(p.rate_pct)));
  const yMax = Math.max(100, ...rates.filter((r): r is number => r !== null));
  const sx = (x: number) => 2 + Math.round((x / xMax) * (W - 4));
  const sy = (y: number) => H - 2 - Math.round((Math.max(0, y) / yMax) * (H - 4));
  return (
    <figure className="chart-wrap">
      <svg className="chart decay" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label="capture rate by latency">
        {measured && <rect className="marker" x={sx(measured.latency_ms)} y={0} width={1} height={H} />}
        {points.map((p, i) => {
          const r = rates[i];
          return r === null || r === undefined ? null : (
            <rect key={p.scenario} className={p.headline ? 'pt head' : 'pt'} x={sx(p.latency_ms) - 2} y={sy(r) - 2} width={5} height={5} />
          );
        })}
      </svg>
      <div className="chart-axis">
        <span>0 ms</span>
        <span>latency →</span>
      </div>
      <ul className="plain legend">
        {points.map((p) => (
          <li key={p.scenario}>{`${p.scenario} · ${p.latency_ms} ms → ${p.rate_pct === null ? '—' : `${p.rate_pct}%`}`}</li>
        ))}
      </ul>
      {measured && <p className="dim">{`measured: ${measured.host} ≈ ${measured.latency_ms} ms (an estimate)`}</p>}
    </figure>
  );
}
