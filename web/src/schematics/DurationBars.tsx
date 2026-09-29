import { type DurationRow, durationText } from '../explain/models';

const W = 300;
const H = 10;

/** Spec 7a §9.2: each episode as a bar on one shared time scale, the example's updates as ticks.
 *  Labels are HTML under each track (the SVG stretches: preserveAspectRatio="none"). */
export function DurationBars({ rows }: { rows: DurationRow[] }) {
  const max = Math.max(1, ...rows.map((r) => r.ms));
  const sx = (ms: number) => (ms / max) * W;
  return (
    <figure className="dbars" aria-label="how long each stayed on the table">
      {rows.map((r) => (
        <div key={`${r.kind}|${r.label}`} className="db-row">
          <svg className="db-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
            <rect
              className={['db-bar', r.kind === 'example' ? 'example' : null, r.open ? 'open' : null].filter(Boolean).join(' ')}
              x={0}
              y={2}
              width={Math.max(1, sx(r.ms))}
              height={H - 4}
            />
            {r.ticks.map((t) => (
              <rect key={t} className="db-tick" x={Math.min(W - 1, sx(t))} y={0} width={1} height={H} />
            ))}
          </svg>
          <p className="db-label">{durationText(r)}</p>
        </div>
      ))}
    </figure>
  );
}
