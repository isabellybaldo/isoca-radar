import { describe, expect, it } from 'vitest';
import { sweetRow } from '../test/fixtures';
import type { SweetSeries } from '../api/types';
import { FEW_TRADES, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, footnote, grid, headline, interval, nothingPays } from './sweet';

const series = (rows = [sweetRow()], o: Partial<SweetSeries> = {}): SweetSeries => ({
  rows, m: rows.filter((r) => r.label !== 'thin' && r.label !== 'not_tested').length, z: '2.576',
  category_available: true, hurdle: '0.1', fees: [], ...o,
});

describe('sweet helpers', () => {
  it('keeps the spec copy verbatim', () => {
    expect(SWEET_CAVEAT).toBe("Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.");
    expect(NOTHING_PAYS).toBe("Nothing here pays at 300 ms. Don't trade this, or get closer.");
    expect(LABEL_TEXT.candidate).toBe('survives here; worth a real small-order test.');
    expect(LABEL_TEXT.negative).toBe("loses here. Don't trade this.");
    expect(LABEL_TEXT.dead_on_paper).toBe('even first in line, the money is locked too long to be worth it.');
    expect(FEW_TRADES).toBe('few trades carry this row');
    expect(footnote(34)).toBe("34 rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.");
  });

  it('writes the headline per side at sp-idle with the 10 bips figure beside it', () => {
    const rows = [
      sweetRow({ label: 'negative' }),
      sweetRow({ dimension: 'days', bucket: '1-7', label: 'inconclusive' }),
      sweetRow({ fee_bips: 10, label: 'negative' }),
    ];
    expect(headline(series(rows, { fees: [{ fee_bips: 10, captured: '-1', mean: '-0.1', label: 'negative' }] }), 'no'))
      .toBe('NO: 0 candidates in 2 tested rows (whole NO side at 10 bips: NEGATIVE, mean −$0.10).');
    const cands = [sweetRow({ dimension: 'category', bucket: 'sports', label: 'candidate' }), sweetRow({ dimension: 's', bucket: '2', label: 'candidate' })];
    expect(headline(series(cands), 'no')).toBe('NO: 2 candidates: sports, 2 legs.');
    expect(headline(series([]), 'yes')).toBe('YES: 0 candidates in 0 tested rows.');
  });

  it('excludes dead_on_paper from the tested count and from "nothing pays", same as backend m', () => {
    const rows = [sweetRow({ scenario: 'sp-idle', label: 'dead_on_paper' })];
    expect(headline(series(rows), 'no')).toBe('NO: 0 candidates in 0 tested rows.');
    expect(nothingPays(series(rows))).toBe(false);
  });

  it('lays out buckets by scenarios and formats intervals as money', () => {
    const rows = [
      sweetRow({ dimension: 'days', bucket: '7-30', scenario: 'colo-idle', latency_ms: 52 }),
      sweetRow({ dimension: 'days', bucket: 'lt1', scenario: 'sp-idle', latency_ms: 300, mean: '0.1', lo: '-0.02', hi: '0.22' }),
      sweetRow({ dimension: 'days', bucket: 'lt1', scenario: 'sp-idle', fee_bips: 10 }),
    ];
    const g = grid(series(rows), 'no', 'days');
    expect(g.buckets).toEqual(['lt1', '7-30']);
    expect(g.scenarios.map((s) => s.name)).toEqual(['colo-idle', 'sp-idle']);
    expect(interval(g.cell('lt1', 'sp-idle')!)).toBe('+$0.10 [−$0.02, +$0.22]');
    expect(g.cell('7-30', 'sp-idle')).toBeUndefined();
  });
});
