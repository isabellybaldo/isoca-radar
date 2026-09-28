import type { EpisodeSummary } from '../api/types';
import { formatMoneyCents, signOf } from '../lib/money';

const W = 600;
const H = 40;

const tone = (kind: string) => (kind === 'no_basket' ? 'no' : kind === 'yes_basket' ? 'yes' : 'other');

/** 95th percentile of |net|, so one outlier doesn't flatten every other tick. Geometry only. */
function cap(rows: EpisodeSummary[]): number {
  const v = rows.map((r) => Math.abs(Number(r.net))).sort((a, b) => a - b);
  return Math.max(0.01, v[Math.min(v.length - 1, Math.floor(v.length * 0.95))] ?? 0.01);
}

/** Redesign §4.1: every episode in view on one strip; colour = kind, height = |NET|, hollow = flagged. */
export function EpisodeStrip({ rows }: { rows: EpisodeSummary[] }) {
  if (rows.length === 0) return null;
  const ts = rows.map((r) => r.opened_ms);
  let lo = Math.min(...ts);
  let hi = Math.max(...ts);
  if (hi === lo) {
    lo -= 60_000;
    hi += 60_000;
  }
  const c = cap(rows);
  const sx = (t: number) => 1 + Math.round(((t - lo) / (hi - lo)) * (W - 4));
  const pos = rows.filter((r) => signOf(r.net) > 0).length;
  return (
    <figure className="strip" aria-label={`${rows.length} episodes in view, ${pos} net-positive`}>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="presentation">
        <rect className="axis" x={0} y={H - 1} width={W} height={1} />
        {rows.map((r) => {
          const h = Math.max(2, Math.round((Math.min(Math.abs(Number(r.net)), c) / c) * (H - 4)));
          const flagged = r.suspect || r.stale || r.censored;
          return (
            <rect key={r.id} className={`tick ${tone(r.kind)}${flagged ? ' hollow' : ''}`} x={sx(r.opened_ms)} y={H - 1 - h} width={2} height={h}>
              <title>{`${r.event_title ?? r.id} ${formatMoneyCents(r.net, { signed: true })}`}</title>
            </rect>
          );
        })}
      </svg>
      <figcaption className="dim">{`${rows.length} in view · ${pos} net-positive`}</figcaption>
    </figure>
  );
}
