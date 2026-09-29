import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture, positionFixture } from '../test/fixtures';
import { OTHER, daysText, durationRows, durationText, payoffKind, payoffStructure, positionKey } from './models';

describe('payoff structure (parent spec §5.7 (b))', () => {
  it('a NO basket of three: each NO pays unless its own outcome wins; the basket pays S − 1 = 2 whoever wins', () => {
    const m = payoffStructure('no', 3, false);
    expect(m.cols).toEqual(['A', 'B', 'C']);
    expect(m.rows).toEqual([
      { name: 'NO A', cells: [0, 1, 1] },
      { name: 'NO B', cells: [1, 0, 1] },
      { name: 'NO C', cells: [1, 1, 0] },
    ]);
    expect(m.basket).toEqual([2, 2, 2]);
    expect(m.guaranteed).toBe(2);
    expect(m.broken).toEqual([false, false, false]);
  });

  it('an incomplete YES set: the uncovered column pays nothing and is broken', () => {
    const m = payoffStructure('yes', 3, true);
    expect(m.cols).toEqual(['A', 'B', 'C', OTHER]);
    expect(m.rows[0]).toEqual({ name: 'YES A', cells: [1, 0, 0, 0] });
    expect(m.basket).toEqual([1, 1, 1, 0]);
    expect(m.broken).toEqual([false, false, false, true]);
  });

  it('draws at most six outcomes but keeps the real S in the basket', () => {
    const yes = payoffStructure('yes', 12, false);
    expect([yes.rows.length, yes.cols.length, yes.s]).toEqual([6, 6, 12]);
    expect(yes.basket).toEqual([1, 1, 1, 1, 1, 1]);
    const no = payoffStructure('no', 12, false);
    expect([no.guaranteed, no.basket[0]]).toEqual([11, 11]);
  });

  it('maps episode kinds; implications have no basket drawing', () => {
    expect(['no_basket', 'yes_basket', 'yes_basket_incomplete', 'implication'].map(payoffKind)).toEqual(['no', 'yes', 'incomplete', null]);
  });
});

describe('duration bars', () => {
  it('the §5.7 (e) example runs from u2 to u5 with a tick per update; a closed row has its duration', () => {
    const [ex, row] = durationRows(episodeSummaryFixture({ closed_ms: 1_790_000_004_190, duration_ms: 4190 }), 0);
    expect(ex).toEqual({ kind: 'example', label: 'example', ms: 3800, open: false, ticks: [0, 1000, 2200, 3800] });
    expect(durationText(ex)).toBe('example: 3.80 s, from u2 to u5, peak at u3');
    expect(durationText(row)).toBe('Bitcoin price on September 26?: 4.19 s');
  });

  it('an open row runs to now; no row is the example alone', () => {
    const [, row] = durationRows(episodeSummaryFixture({ opened_ms: 1000, closed_ms: null, duration_ms: null }), 126_000);
    expect(row).toMatchObject({ ms: 125_000, open: true });
    expect(durationText(row)).toBe('Bitcoin price on September 26?: still open, 2 min 5 s so far');
    expect(durationRows(null, 0)).toHaveLength(1);
  });
});

describe('days to resolution (spec 7a §9.2, Position.end_date; D-07)', () => {
  it('counts whole days up, and says when it is unknown', () => {
    expect(daysText('2026-10-31', Date.UTC(2026, 9, 29))).toBe('resolves in 3 days');
    expect(daysText('2026-10-30T00:00:00Z', Date.UTC(2026, 9, 29, 12))).toBe('resolves in 1 day');
    expect(daysText(null, 0)).toBe('resolution date unknown');
    expect(daysText('soon', 0)).toBe('resolution date unknown');
    expect(positionKey(positionFixture())).toBe('P3-naive/sp-idle|m1|yes');
  });

  it('a date-only end date is the END of that UTC day: on the day itself it is not past', () => {
    expect(daysText('2026-09-29', Date.UTC(2026, 8, 29, 0, 0, 1))).toBe('resolves in 1 day');
    expect(daysText('2026-09-29', Date.UTC(2026, 8, 29, 23, 59, 59))).toBe('resolves in 1 day');
    expect(daysText('2026-09-29', Date.UTC(2026, 8, 30))).toBe('past its end date, awaiting resolution');
    expect(daysText('2026-10-01', Date.UTC(2026, 9, 2, 12))).toBe('past its end date, awaiting resolution');
  });

  it('a full timestamp is taken as it is', () => {
    expect(daysText('2026-09-29T10:00:00Z', Date.UTC(2026, 8, 29, 12))).toBe('past its end date, awaiting resolution');
  });
});
