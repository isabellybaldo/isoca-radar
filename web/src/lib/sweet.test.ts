import { describe, expect, it } from 'vitest';
import { sweetFixture, sweetRow } from '../test/fixtures';
import type { SweetRow, SweetSeries } from '../api/types';
import {
  FEW_TRADES, GLYPHS, LABEL_TEXT, NOTHING_PAYS, SWEET_CAVEAT, bestCandidate, bucketText, candidateCount, cellDetail, feeLine, footnote, glyphKey, grid,
  gridNote, interval, ladderGrid, nothingPays, recomputingText, rowName, rowsFor,
} from './sweet';

const series = (rows = [sweetRow()], o: Partial<SweetSeries> = {}): SweetSeries => ({
  rows, m: rows.filter((r) => r.label !== 'thin' && r.label !== 'not_tested').length, z: '2.576',
  category_available: true, hurdle: '0.1', fees: [], ...o,
});

describe('sweet helpers', () => {
  it('keeps the spec copy verbatim', () => {
    expect(SWEET_CAVEAT).toBe("Captured is paper: first in line at t_x against the recorded book, minus our own taken liquidity. Real bots compete for the same depth; a friend's bot filled 6 of 24 signals. A candidate is a question for real small orders, not an answer.");
    expect(NOTHING_PAYS).toBe("Nothing pays at sp-idle yet. Don't trade this, or get closer.");
    expect(LABEL_TEXT.candidate).toBe('survives here; worth a real small-order test.');
    expect(LABEL_TEXT.negative).toBe("loses here. Don't trade this.");
    expect(LABEL_TEXT.dead_on_paper).toBe('even first in line, the money is locked too long to be worth it.');
    expect(FEW_TRADES).toBe('few trades carry this row');
    expect(footnote(34)).toBe("34 rows tested; each interval is widened so that, together, a false 'candidate' shows up less than 1 time in 20.");
  });

  it('writes the 10 bips fee line for the whole NO side, or null when the backend sent none', () => {
    expect(feeLine(series([sweetRow()], { fees: [{ fee_bips: 10, captured: '-1', mean: '-0.1', label: 'negative' }] })))
      .toBe('whole NO side at 10 bips: NEGATIVE, mean −$0.10');
    expect(feeLine(series([sweetRow()], { fees: [] }))).toBeNull();
  });

  it('excludes dead_on_paper from "nothing pays", same as backend m', () => {
    const rows = [sweetRow({ scenario: 'sp-idle', label: 'dead_on_paper' })];
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

describe('redesign helpers', () => {
  const s = sweetFixture([
    sweetRow({ dimension: 'hour', bucket: '12-16', scenario: 'sp-idle', label: 'candidate', lo: '0.04', mean: '0.31', hi: '0.58' }),
    sweetRow({ dimension: 'hour', bucket: '00-04', scenario: 'sp-idle', label: 'negative', lo: '-0.3', mean: '-0.2', hi: '-0.1' }),
    sweetRow({ dimension: 'hour', bucket: '00-04', scenario: 'colo-idle', latency_ms: 52, label: 'candidate', lo: '0.5', mean: '0.6', hi: '0.7' }),
    sweetRow({ dimension: 'days', bucket: '1-7', scenario: 'sp-idle', label: 'candidate', lo: '0.09', mean: '0.2', hi: '0.3' }),
    sweetRow({ dimension: 'days', bucket: '1-7', scenario: 'sp-idle', fee_bips: 10, label: 'candidate', lo: '0.9', mean: '1', hi: '1.1' }),
    sweetRow({ side: 'yes', dimension: 'all', bucket: 'all', scenario: 'sp-idle', label: 'candidate', lo: '0.01', mean: '0.1', hi: '0.2' }),
  ]).series.sweet!;

  it('rowsFor keeps one scenario, fee 0, in canonical bucket order', () => {
    expect(rowsFor(s, 'no', 'hour', 'sp-idle').map((r) => r.bucket)).toEqual(['00-04', '12-16']);
    expect(rowsFor(s, 'no', 'hour', 'colo-idle').map((r) => r.bucket)).toEqual(['00-04']);
    expect(rowsFor(s, 'no', 'days', 'sp-idle')).toHaveLength(1);
  });

  it('counts candidates per side at the headline scenario and fee 0 only', () => {
    expect(candidateCount(s, 'no')).toBe(2);
    expect(candidateCount(s, 'yes')).toBe(1);
  });

  it('picks the candidate with the highest lower bound, ignoring other scenarios and fees', () => {
    expect(bestCandidate(s)?.bucket).toBe('1-7');
    expect(bestCandidate(sweetFixture([sweetRow({ label: 'negative', lo: '-1', mean: '-0.5', hi: '0' })]).series.sweet!)).toBeNull();
  });

  it('names a row for people', () => {
    expect(rowName(sweetRow({ side: 'yes', dimension: 'all', bucket: 'all' }))).toBe('whole YES side');
    expect(rowName(sweetRow({ dimension: 'hour', bucket: '12-16' }))).toBe('12–16 UTC');
  });

  it('says what recomputing means and when it last had a result', () => {
    expect(recomputingText(Date.UTC(2026, 8, 27, 17, 14))).toBe(
      'Recomputing after a premise change. The last result was 17:14 UTC; new numbers appear when it finishes.',
    );
  });
});

describe('ladder grid (spec 7a §8)', () => {
  const rows = [
    sweetRow({ scenario: 'sp-idle', latency_ms: 300, label: 'negative', mean: '-0.1' }),
    sweetRow({ scenario: 'rung-0', latency_ms: 0, label: 'candidate', mean: '0.2' }),
    sweetRow({ scenario: 'colo-idle', latency_ms: 52, label: 'thin', mean: '0.05' }),
    sweetRow({ scenario: 'sp-idle', latency_ms: 300, fee_bips: 10, label: 'candidate' }),
    sweetRow({ scenario: 'sp-burst', latency_ms: 450, label: 'candidate' }),
    sweetRow({ scenario: 'sp-idle-nodelay', latency_ms: 300, label: 'candidate' }),
    sweetRow({ dimension: 'category', bucket: 'weather', scenario: 'rung-0', latency_ms: 0, label: 'thin', mean: '-0.3' }),
    sweetRow({ dimension: 'category', bucket: 'crypto', scenario: 'rung-0', latency_ms: 0, label: 'inconclusive' }),
    sweetRow({ side: 'yes', scenario: 'sp-idle', latency_ms: 300, label: 'dead_on_paper' }),
    sweetRow({ side: 'yes', scenario: 'sp-burst', latency_ms: 450, label: 'candidate' }),
  ];

  it('has six NO columns and two YES columns, sorted by latency', () => {
    expect(ladderGrid(rows, 'no', 'all').columns.map((c) => c.scenario)).toEqual(['rung-0', 'rung-25', 'colo-idle', 'rung-100', 'rung-200', 'sp-idle']);
    expect(ladderGrid(rows, 'yes', 'all').columns).toEqual([{ scenario: 'colo-idle', latency_ms: 52 }, { scenario: 'sp-idle', latency_ms: 300 }]);
  });

  it('never shows a fee variant or an extra: sp-idle is the fee-0 row', () => {
    const g = ladderGrid(rows, 'no', 'all');
    expect(g.cell({ dim: 'all', bucket: 'all' }, 'sp-idle')?.label).toBe('negative');
    expect(g.columns.some((c) => c.scenario === 'sp-burst' || c.scenario === 'sp-idle-nodelay')).toBe(false);
    expect(ladderGrid(rows, 'yes', 'all').cell({ dim: 'all', bucket: 'all' }, 'sp-idle')?.label).toBe('dead_on_paper');
  });

  it('puts the all row first, then the slice in its own order; `only` keeps the named buckets', () => {
    expect(ladderGrid(rows, 'no', 'category').rows).toEqual([
      { dim: 'all', bucket: 'all' }, { dim: 'category', bucket: 'weather' }, { dim: 'category', bucket: 'crypto' },
    ]);
    expect(ladderGrid(rows, 'no', 'category', ['crypto']).rows).toEqual([{ dim: 'all', bucket: 'all' }, { dim: 'category', bucket: 'crypto' }]);
    expect(ladderGrid(rows, 'no', 'all').rows).toEqual([{ dim: 'all', bucket: 'all' }]);
  });

  it('gives one glyph per label and sign', () => {
    const k = (o: Partial<SweetRow>) => GLYPHS[glyphKey(sweetRow(o))];
    expect(k({ label: 'candidate' })).toBe('●');
    expect(k({ label: 'negative' })).toBe('○');
    expect(k({ label: 'inconclusive' })).toBe('◌');
    expect(k({ label: 'thin', mean: '0.1' })).toBe('◐');
    expect(k({ label: 'thin', mean: '-0.1' })).toBe('◑');
    expect(k({ label: 'thin', mean: '0' })).toBe('◑');
    expect(k({ label: 'thin', mean: null })).toBe('◑');
    expect(k({ label: 'dead_on_paper' })).toBe('✗');
    expect(k({ label: 'not_tested' })).toBe('–');
    expect(GLYPHS[glyphKey(undefined)]).toBe('·');
  });

  it('writes the detail line with the capture at that latency, and the footnote', () => {
    const r = sweetRow({
      dimension: 'category', bucket: 'crypto', scenario: 'rung-25', latency_ms: 25, label: 'thin', mean: '0.3', lo: '-0.1', hi: '0.7', n_exec: 12, clusters: 9,
    });
    expect(cellDetail({ dim: 'category', bucket: 'crypto' }, { scenario: 'rung-25', latency_ms: 25 }, r)).toBe(
      'crypto · rung-25 · n 12 · C 9 · captured at 25 ms, per $100: +$0.30 [−$0.10, +$0.70] · too few trades or events to say anything.',
    );
    expect(cellDetail({ dim: 'all', bucket: 'all' }, { scenario: 'rung-0', latency_ms: 0 }, undefined)).toBe('all · rung-0 · no attempts in this slice');
    expect(gridNote({ m: 364, z: '3.62' })).toBe('z = 3.62 across 364 rows tested: the more rows, the wider each interval.');
  });
});

describe('bucketText', () => {
  it('never returns an inherited Object property for a bucket named like one', () => {
    for (const b of ['__proto__', 'constructor', 'toString', 'hasOwnProperty']) {
      for (const dim of ['category', 'days', 's']) expect(typeof bucketText(dim, b)).toBe('string');
      expect(bucketText('category', b)).toBe(b);
    }
    expect(bucketText('days', '1-7')).toBe('1–7 days');
  });
});
