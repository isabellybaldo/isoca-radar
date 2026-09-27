import { describe, expect, it, vi } from 'vitest';
import { cardFixture, CARD_NOW, edgeFixture } from '../test/fixtures';
import {
  STALE_MS, THIN_EDGE_REASON, THIN_EDGE_USD, computedText, fmtValue, shortHash, statusLabel, statusTone, teaserLine, updatedText,
  warmingText, withThinEdge,
} from './cards';
import { isoDay } from './time';

describe('card helpers', () => {
  it('labels statuses in words and tones them', () => {
    expect(['live', 'warming', 'thin', 'under_review'].map((s) => statusLabel(s as never))).toEqual(['LIVE', 'WARMING', 'THIN', 'UNDER REVIEW']);
    expect(statusTone('live')).toBe('no');
    expect(statusTone('under_review')).toBe('flag');
    expect(statusTone('thin')).toBe('plain');
  });

  it('says since when and how many days of data', () => {
    const since = CARD_NOW - 6 * 86_400_000 - 5_000;
    expect(warmingText(cardFixture({ since_ms: since }))).toBe(`since ${isoDay(since)} · 6 days of data`);
    expect(warmingText(cardFixture({ since_ms: CARD_NOW - 86_400_000 }))).toContain('· 1 day of data');
    expect(warmingText(cardFixture({ since_ms: null }))).toBe('no run has recorded these premises yet');
  });

  it('formats values by unit with string operations only, and null as —', () => {
    const spies = [
      vi.spyOn(Number.prototype, 'toFixed'),
      vi.spyOn(Number.prototype, 'toLocaleString'),
      vi.spyOn(Intl, 'NumberFormat'),
      vi.spyOn(globalThis, 'parseFloat'),
      vi.spyOn(Math, 'round'),
    ];
    expect(fmtValue('12345678901234567.89', 'USD')).toBe('$12,345,678,901,234,567.89');
    expect(fmtValue('58.7', '%')).toBe('58.7%');
    expect(fmtValue('16.15', 's')).toBe('16.15 s');
    expect(fmtValue('52.5', 'bps')).toBe('52.5 bp');
    expect(fmtValue('200', 'ms')).toBe('200 ms');
    expect(fmtValue('30.26', '×')).toBe('30.26×');
    expect(fmtValue('1150', 'count')).toBe('1,150');
    expect(fmtValue('0', 'count')).toBe('0');
    expect(fmtValue(null, 'USD')).toBe('—');
    for (const s of spies) expect(s).not.toHaveBeenCalled();
    expect(shortHash('3f2a9c1b7d4e5f60')).toBe('3f2a9c1b7d4e');
  });

  it('says when the cards were updated and when that is too long ago', () => {
    expect(updatedText(null, CARD_NOW)).toEqual({ text: 'analyze has not run yet', stale: true });
    expect(updatedText(CARD_NOW - 5 * 60_000, CARD_NOW)).toEqual({ text: 'updated 5 min ago', stale: false });
    expect(updatedText(CARD_NOW - STALE_MS - 60_000, CARD_NOW)).toEqual({ text: 'updated 46 min ago', stale: true });
  });
});

describe('teaserLine', () => {
  it('says the Radar line in the spec voice', () => {
    expect(teaserLine(edgeFixture())).toBe('Edge on the table today: +$12.50 · captured at 145 ms: 3.2%.');
  });

  it('shows — for the rate when nothing was on the table, and $0.00 unsigned', () => {
    const zero = edgeFixture({
      columns: [{ label: 'edge on the table', unit: 'USD', paper: null, their: null, ours: '0' }],
      series: { decay: [{ scenario: 'sp-idle', latency_ms: 300, net_ms: 145, captured: '0', rate_pct: null, headline: true }] },
    });
    expect(teaserLine(zero)).toBe('Edge on the table today: $0.00 · captured at 145 ms: —.');
  });
});

describe('computedText', () => {
  it('says how old a card body is, in whole minutes, never negative', () => {
    expect(computedText(CARD_NOW - 2 * 3_600_000, CARD_NOW)).toBe('computed 120 min ago');
    expect(computedText(CARD_NOW - 59_999, CARD_NOW)).toBe('computed 0 min ago');
    expect(computedText(CARD_NOW + 5_000, CARD_NOW)).toBe('computed 0 min ago');
  });
});

describe('thin edge on the table', () => {
  const onTable = (v: string | null) => {
    const c = edgeFixture();
    c.columns = c.columns.map((r) => (r.label === 'edge on the table' ? { ...r, ours: v } : r));
    return c;
  };

  it('blanks every capture rate and says why when the edge on the table is under the threshold', () => {
    expect(THIN_EDGE_USD).toBe('1.00');
    const c = withThinEdge(onTable('0.99'));
    expect(c.columns.find((r) => r.label === 'capture rate, sp-idle')?.ours).toBeNull();
    expect(c.columns.find((r) => r.label === 'captured at sp-idle (145 ms)')?.ours).toBe('0.4');
    expect(c.series.decay?.every((p) => p.rate_pct === null)).toBe(true);
    expect(c.thin_reasons).toContain(THIN_EDGE_REASON);
    expect(THIN_EDGE_REASON).toBe('too little edge on the table yet');
  });

  it('leaves the card alone at or above the threshold, or when nothing was measured', () => {
    const at = onTable('1');
    expect(withThinEdge(at)).toBe(at);
    const none = onTable(null);
    expect(withThinEdge(none)).toBe(none);
    const g1 = cardFixture();
    expect(withThinEdge(g1)).toBe(g1);
  });
});
