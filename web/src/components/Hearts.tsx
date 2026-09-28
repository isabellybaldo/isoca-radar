import type { HealthNow } from '../api/types';

export const MAX_HEARTS = 10;

/** One lost heart per trouble kind (spec §5 Radar). */
export function troubles(h: HealthNow): string[] {
  const t: string[] = [];
  if (h.drops_5m > 0) t.push('drops in the last 5 min');
  if (h.parked_shards > 0) t.push('parked shards');
  if (h.lag_p99_ms !== null && h.lag_p99_ms > 5000) t.push('lag p99 over 5 s');
  if (h.age_ms > 180_000) t.push('health row older than 3 min');
  if (h.capture_paused) t.push('capture paused');
  // 250 mirrors CLOCK_SKEW_MS in bins/scanner/src/clock.rs.
  if (h.clock_offset_ms !== null && Math.abs(h.clock_offset_ms) > 250) t.push(`clock off by ${h.clock_offset_ms} ms`);
  // spec 5b §5: one heart for the compactor: behind (3 mirrors lag_alarm_hours), silent for 2 h, or
  // never reported although hours are being recorded
  if (h.compact_lag_hours !== null && h.compact_lag_hours > 3) t.push('compactor behind: raw at risk');
  else if (h.compact_age_ms !== null && h.compact_age_ms > 7_200_000) t.push('compactor status older than 2 h');
  else if (h.compact_age_ms === null && h.compact_started_ms !== null && h.now_ms - h.compact_started_ms > 7_200_000) t.push('compactor has never reported');
  return t;
}

export function Hearts({ health }: { health: HealthNow }) {
  const t = troubles(health);
  const full = MAX_HEARTS - t.length;
  const label = `health ${full} of ${MAX_HEARTS}${t.length > 0 ? `: ${t.join(', ')}` : ''}`;
  return (
    <span className="hearts" role="img" aria-label={label} title={t.length > 0 ? t.join(', ') : 'all good'}>
      {'♥'.repeat(full)}
      {'♡'.repeat(MAX_HEARTS - full)}
    </span>
  );
}
