import { fmtAge, fmtLocal } from '../lib/time';

export function Age({ sinceMs, nowMs }: { sinceMs: number; nowMs: number }) {
  return <span title={`opened ${fmtLocal(sinceMs)}`}>{fmtAge(nowMs - sinceMs)}</span>;
}
