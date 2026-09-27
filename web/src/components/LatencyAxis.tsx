import type { DecayPoint } from '../api/types';

const W = 300;
const H = 10;

const rateText = (p: DecayPoint) => (p.rate_pct === null ? '—' : `${p.rate_pct}%`);

/**
 * Redesign §4.2: one latency axis, one row per scenario (so equal latencies never overlap), and the
 * measured latency as a separate marker with its own caption. Scenario latency and measured latency
 * are different things and are labelled so. Numbers are geometry only; text is the string sent.
 */
export function LatencyAxis({ points, measured }: { points: DecayPoint[]; measured: { latency_ms: number; host: string } | null }) {
  const rows = [...points].sort((a, b) => a.latency_ms - b.latency_ms || a.scenario.localeCompare(b.scenario));
  const xMax = Math.max(1, ...rows.map((p) => p.latency_ms), measured?.latency_ms ?? 0) * 1.15;
  const sx = (ms: number) => 1 + Math.round((ms / xMax) * (W - 4));
  return (
    <figure className="latency" aria-label="capture rate by latency">
      <ul className="plain">
        {rows.map((p) => (
          <li key={p.scenario} className="lat-row" data-scenario={p.scenario}>
            <svg className="lat-track" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
              <rect className="axis" x={0} y={H / 2} width={W} height={1} />
              {measured && <rect className="measured" x={sx(measured.latency_ms)} y={0} width={1} height={H} />}
              <rect
                className={['dot', p.headline ? 'head' : null, p.rate_pct === null ? 'hollow' : null].filter(Boolean).join(' ')}
                x={sx(p.latency_ms) - 2}
                y={1}
                width={5}
                height={H - 2}
              />
            </svg>
            <span className={p.headline ? 'lat-label head' : 'lat-label'}>{`${p.scenario} · ${p.latency_ms} ms → ${rateText(p)}`}</span>
          </li>
        ))}
      </ul>
      <div className="chart-axis">
        <span>0 ms</span>
        <span>{`${Math.round(xMax)} ms`}</span>
      </div>
      {measured && <figcaption className="dim">{`measured: ${measured.host} ≈ ${measured.latency_ms} ms (an estimate)`}</figcaption>}
    </figure>
  );
}
