import { describe, expect, it, vi } from 'vitest';
import { cardFixture, CARD_NOW } from '../test/fixtures';
import { STALE_MS, fmtValue, shortHash, statusLabel, statusTone, updatedText, warmingText } from './cards';
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
