import type { CardBody, CardStatus } from '../api/types';
import type { TagTone } from '../components/Tag';
import { compareDecimal, formatDecimal, formatMoney } from './money';
import { DAY_MS, isoDay } from './time';

/** Spec §7: results older than this show "updated n min ago" in the thin (dimmed) style. */
export const STALE_MS = 45 * 60_000;
export const UNDER_REVIEW = 'The premises changed. The history is being recomputed; the numbers come back when it is done.';
/** The Edge capture row the Radar teaser quotes (cards::edge::ON_TABLE). */
export const ON_TABLE = 'edge on the table';

/** Below this much edge on the table (USD), a capture rate is noise: the web shows "—" instead. */
export const THIN_EDGE_USD = '1.00';
export const THIN_EDGE_REASON = 'too little edge on the table yet';
const RATE_ROW = 'capture rate';

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

/**
 * Rendering-only guard for the Edge capture card: when the edge on the table is under
 * THIN_EDGE_USD, every capture rate (the table rows and the decay points) becomes null ("—") and
 * the reason joins the card's thin reasons. The stored card is unchanged; other cards pass through.
 */
export function withThinEdge(card: CardBody): CardBody {
  const onTable = card.columns.find((r) => r.label === ON_TABLE)?.ours ?? null;
  if (onTable === null || compareDecimal(onTable, THIN_EDGE_USD) >= 0) return card;
  return {
    ...card,
    columns: card.columns.map((r) => (r.label.startsWith(RATE_ROW) ? { ...r, paper: null, their: null, ours: null } : r)),
    series: { ...card.series, decay: card.series.decay?.map((p) => ({ ...p, rate_pct: null })) },
    thin_reasons: [...card.thin_reasons, THIN_EDGE_REASON],
  };
}

/** Spec §6.2: "Edge on the table today: +$X · captured at 145 ms: Y%." (string operations only). */
export function teaserLine(card: CardBody): string {
  const onTable = card.columns.find((r) => r.label === ON_TABLE)?.ours ?? null;
  const head = card.series.decay?.find((p) => p.headline);
  const rate = head?.rate_pct ?? null;
  return `Edge on the table today: ${formatMoney(onTable, { signed: true })} · captured at ${head ? head.net_ms : 145} ms: ${rate === null ? '—' : `${rate}%`}.`;
}

export function updatedText(lastTickMs: number | null, nowMs: number): { text: string; stale: boolean } {
  if (lastTickMs === null) return { text: 'analyze has not run yet', stale: true };
  const age = Math.max(0, nowMs - lastTickMs);
  return { text: `updated ${Math.floor(age / 60_000)} min ago`, stale: age > STALE_MS };
}

/** A card body's own age (its `computed_ms`): 30d and `all` bodies re-render at most hourly, so they
 *  can be older than the tick the page's "updated n min ago" reports. */
export function computedText(computedMs: number, nowMs: number): string {
  return `computed ${Math.floor(Math.max(0, nowMs - computedMs) / 60_000)} min ago`;
}
