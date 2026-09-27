import type { CardBody, CardStatus } from '../api/types';
import type { TagTone } from '../components/Tag';
import { formatDecimal, formatMoney } from './money';
import { DAY_MS, isoDay } from './time';

/** Spec §7: results older than this show "updated n min ago" in the thin (dimmed) style. */
export const STALE_MS = 45 * 60_000;
export const UNDER_REVIEW = 'The premises changed. The history is being recomputed; the numbers come back when it is done.';
/** The Edge capture row the Radar teaser quotes (cards::edge::ON_TABLE). */
export const ON_TABLE = 'edge on the table';

const LABELS: Record<CardStatus, string> = { live: 'LIVE', warming: 'WARMING', thin: 'THIN', under_review: 'UNDER REVIEW' };

export function statusLabel(s: CardStatus): string {
  return LABELS[s];
}

export function statusTone(s: CardStatus): TagTone {
  if (s === 'live') return 'no';
  if (s === 'under_review') return 'flag';
  return 'plain';
}

export function shortHash(h: string): string {
  return h.slice(0, 12);
}

/** "since 2026-09-20 · 6 days of data" (integer ms math; not money). */
export function warmingText(c: CardBody): string {
  if (c.since_ms === null) return 'no run has recorded these premises yet';
  const days = Math.floor((c.computed_ms - c.since_ms) / DAY_MS);
  return `since ${isoDay(c.since_ms)} · ${days} ${days === 1 ? 'day' : 'days'} of data`;
}

/** A card value as sent, by unit; money through formatMoney. null (not measured) is "—". */
export function fmtValue(v: string | null, unit: string): string {
  if (v === null) return '—';
  switch (unit) {
    case 'USD':
      return formatMoney(v);
    case '%':
      return `${formatDecimal(v)}%`;
    case 's':
      return `${formatDecimal(v)} s`;
    case 'bps':
      return `${formatDecimal(v)} bp`;
    case 'ms':
      return `${formatDecimal(v)} ms`;
    case '×':
      return `${formatDecimal(v)}×`;
    default:
      return formatDecimal(v);
  }
}

export function updatedText(lastTickMs: number | null, nowMs: number): { text: string; stale: boolean } {
  if (lastTickMs === null) return { text: 'analyze has not run yet', stale: true };
  const age = Math.max(0, nowMs - lastTickMs);
  return { text: `updated ${Math.floor(age / 60_000)} min ago`, stale: age > STALE_MS };
}
