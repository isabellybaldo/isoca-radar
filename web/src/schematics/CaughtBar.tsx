import { LADDER, latencyX, slotX, usBand, usText } from '../lib/ladder';
import { fmtDuration } from '../lib/time';

const W = 600;
const H = 30;
const N = LADDER.length;

/** A rung arrives in time when its latency is below the episode's duration; an open episode, so far, always. */
export function inTime(latencyMs: number, durationMs: number | null): boolean {
  return durationMs === null || latencyMs < durationMs;
}

/** Spec 7a §9.2: the episode's duration as a bar on the ladder's ordinal axis (six rungs, no extras slot);
 *  rungs left of its end are in time. Labels are HTML under the drawing (the SVG stretches). */
export function CaughtBar({ durationMs, measured }: { durationMs: number | null; measured: { latency_ms: number } | null }) {
  const end = durationMs === null ? 1 : latencyX(durationMs, N);
  const band = measured ? usBand(measured, N) : null;
  const last = LADDER[N - 1].latency_ms;
  return (
    <figure className="cbar" aria-label="would we have caught it">
      <svg className="cb-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
        {band && <rect className="cb-us" x={band.x0 * W} y={0} width={(band.x1 - band.x0) * W} height={H} />}
        {/* 0 ms: no bar at all (even rung 0 is too late); any other duration is at least a sliver. */}
        <rect className="cb-dur" x={0} y={H / 2 - 4} width={durationMs === 0 ? 0 : Math.max(1, end * W)} height={8} />
        {LADDER.map((l, i) => (
          <rect
            key={l.scenario}
            className={inTime(l.latency_ms, durationMs) ? 'cb-rung in' : 'cb-rung late'}
            data-scenario={l.scenario}
            x={slotX(i, N) * W - 4}
            y={4}
            width={8}
            height={H - 8}
          />
        ))}
      </svg>
      <ol className="cb-axis" aria-label="rungs in time">
        {LADDER.map((l) => {
          const ok = inTime(l.latency_ms, durationMs);
          return (
            <li key={l.scenario} className={ok ? undefined : 'late'} title={l.scenario}>
              <span className="num">{l.latency_ms}</span>
              <span>{ok ? 'in time' : 'too late'}</span>
            </li>
          );
        })}
      </ol>
      {/* The pink band's key, as the ladder's legend has it: a label, not a sentence of the answer (D-04). */}
      <figcaption className="dim cb-cap">
        <ul className="plain cb-legend" aria-label="legend">
          <li>
            <span className="cb-sw us" aria-hidden="true" />
            {usText(measured)}
          </li>
        </ul>
        {durationMs !== null && durationMs > last && <span>{`ms on the axis; the bar runs past the last rung: ${fmtDuration(durationMs)}`}</span>}
      </figcaption>
    </figure>
  );
}
