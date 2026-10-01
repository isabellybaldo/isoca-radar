// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { OTHER, payoffStructure } from '../explain/models';
import { formatMoneyCents } from '../lib/money';
import { DurationBars } from '../schematics/DurationBars';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { UpdateBars } from '../schematics/UpdateBars';
import { FIXTURES, describeWasm, fixtureCase, fixtureCases, loadTestEngine } from '../wasm/testEngine';
import { type EpisodeModel, type PayoffIn, type PayoffMatrix, isError } from '../wasm/types';
import { IMPLICATION_PRICE_USE, episodeRows, matrixToPayoff } from './adapt';

/** Real engine outputs: the fixture's own output string for the case (never retyped). */
const matrix = (name: string) => JSON.parse(fixtureCase('payoff_json', name).output) as PayoffMatrix;
const episode = (name: string) => JSON.parse(fixtureCase('episode_json', name).output) as EpisodeModel;
/** Section e's headline case: open at 1000 ms, closed at 1200 ms, our order lands at 1295 ms (too late). */
const MISS = episode('slow-miss');

describe('b reuses PayoffGrid (spec 7b §1 change 3)', () => {
  it('draws the implication: the impossible column and its cells as a dash', () => {
    const m = matrix('implication-prices');
    render(<PayoffGrid model={matrixToPayoff(m, 'implication')} label="payoff of the implication" />);
    expect(screen.getByRole('columnheader', { name: `${m.cols[1]} (impossible)` })).toBeInTheDocument();
    expect(screen.getAllByText('—')).toHaveLength(3); // one per row and the basket row
    const top = m.basket.find((b) => b !== null && Number(b) > 1) as string; // the basket pays 2 where both legs pay
    expect(screen.getByText(top)).toHaveClass('pay');
  });

  it('keeps decimals as the engine sent them: strings in the basket, no Number() text', () => {
    const m = matrix('implication-prices');
    expect(matrixToPayoff(m, 'implication').basket).toEqual(m.basket);
  });

  it('keeps the YES basket cross on the uncovered column', () => {
    const m = matrix('yes-3-other');
    render(<PayoffGrid model={matrixToPayoff(m, 'yes')} label="payoff of the YES basket" />);
    expect(screen.getByRole('columnheader', { name: `${m.cols[3]} ✗` })).toBeInTheDocument();
  });

  it('leaves the dashboard panel rendering as it was: no dash, no impossible class', () => {
    const { container } = render(<PayoffGrid model={payoffStructure('yes', 3, true)} label="payoff" />);
    expect(container.querySelector('.impossible')).toBeNull();
    expect(container.textContent).not.toContain('—');
    expect(screen.getByRole('columnheader', { name: `${OTHER} ✗` })).toBeInTheDocument();
  });
});

describe('e reuses DurationBars and adds UpdateBars (spec 7b §6.2 e)', () => {
  it('puts the episode on the table and our order on one time scale, and the edge per update', () => {
    const open = MISS.open_ms as number;
    const rows = episodeRows(MISS);
    const onTable = (MISS.close_ms as number) - open;
    expect(rows).toEqual([
      { kind: 'row', label: 'on the table', ms: onTable, open: false, ticks: [onTable] },
      { kind: 'order', label: 'our order lands', ms: (MISS.t_x as number) - open, open: false, ticks: [] },
    ]);
    const { container } = render(<><UpdateBars updates={MISS.updates} peakMs={MISS.peak_ms} /><DurationBars rows={rows} /></>);
    expect(screen.getByText('on the table: 0.20 s')).toBeInTheDocument();
    expect(screen.getByText('our order lands: 0.29 s')).toBeInTheDocument();
    expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
    expect(container.querySelectorAll('rect.ub-bar')).toHaveLength(MISS.updates.length);
    expect(container.querySelectorAll('rect.ub-bar.peak')).toHaveLength(1);
    expect(container.querySelector('svg.ub-svg')).toHaveAttribute('preserveAspectRatio', 'none');
    expect(screen.getAllByText('no edge')).toHaveLength(MISS.updates.filter((u) => u.net === null).length);
    expect(screen.getByText(formatMoneyCents(MISS.peak_net, { signed: true }))).toBeInTheDocument();
  });

  it('an episode that never opens draws no episode rows and no NaN', () => {
    const never = episode('never-opens');
    expect(episodeRows(never)).toEqual([]);
    const { container } = render(<><UpdateBars updates={never.updates} peakMs={never.peak_ms} /><DurationBars rows={episodeRows(never)} /></>);
    expect(container.innerHTML).not.toContain('NaN');
    expect(container.querySelectorAll('rect.ub-bar.peak')).toHaveLength(0);
  });

  it('an episode still open at the end says so, and its order is drawn from the open', () => {
    const open = episode('still-open-at-end');
    const last = open.updates[open.updates.length - 1].t_ms;
    const o = open.open_ms as number;
    const rows = episodeRows(open);
    expect(rows[0]).toMatchObject({ kind: 'row', ms: last - o, open: true });
    expect(rows[1]).toMatchObject({ kind: 'order', ms: (open.t_x as number) - o });
    const { container } = render(<DurationBars rows={rows} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(screen.getByText('on the table: still open, 4.00 s so far')).toBeInTheDocument();
  });

  it('a one-update episode draws one bar without NaN', () => {
    const { container } = render(<UpdateBars updates={[{ t_ms: 0, net: MISS.peak_net }]} peakMs={0} />);
    expect(container.querySelectorAll('rect.ub-bar')).toHaveLength(1);
    expect(container.innerHTML).not.toContain('NaN');
  });
});

describeWasm('implication shows only what the engine uses (ruling R10)', () => {
  it('top_edge_gross follows A bid and B ask, and ignores A ask and B bid', async () => {
    const eng = await loadTestEngine();
    const edge = (a: { bid: string; ask: string }, b: { bid: string; ask: string }) => {
      const input: PayoffIn = { kind: 'implication', s: 2, other_listed: true, prices: [a, b] };
      const o: unknown = JSON.parse(eng.payoff_json(JSON.stringify(input)));
      if (isError(o)) throw new Error(o.error);
      return (o as PayoffMatrix).top_edge_gross;
    };
    const base = edge({ bid: '0.70', ask: '0.72' }, { bid: '0.40', ask: '0.42' });
    expect(edge({ bid: '0.70', ask: '0.90' }, { bid: '0.10', ask: '0.42' })).toBe(base); // unused sides
    expect(edge({ bid: '0.60', ask: '0.72' }, { bid: '0.40', ask: '0.42' })).not.toBe(base); // A bid
    expect(edge({ bid: '0.70', ask: '0.72' }, { bid: '0.40', ask: '0.50' })).not.toBe(base); // B ask
  });

  it('the adapter says which side each leg uses', () => {
    expect(IMPLICATION_PRICE_USE).toEqual([
      { leg: 'A', side: 'bid', label: 'A bid (we buy NO_A at 1 − bid)' },
      { leg: 'B', side: 'ask', label: 'B ask (we buy YES_B at the ask)' },
    ]);
  });
});

const HAVE_PAYOFF: boolean = existsSync(`${FIXTURES}/payoff_json.json`);

describe.skipIf(!HAVE_PAYOFF)('the panel payoff structure against the payoff fixtures (09-27 §7.3, left open by 7a-B)', () => {
  it('agrees on every YES and NO case without prices, S 2–6', () => {
    let checked = 0;
    for (const c of fixtureCases('payoff_json')) {
      const i = JSON.parse(c.input) as { kind?: string; s?: number; other_listed?: boolean; prices?: unknown };
      const o: unknown = JSON.parse(c.output);
      if ((i.kind !== 'yes' && i.kind !== 'no') || typeof i.s !== 'number' || i.s < 2 || i.s > 6) continue;
      if ((i.prices !== null && i.prices !== undefined) || isError(o)) continue;
      const engine = matrixToPayoff(o as PayoffMatrix, i.kind);
      const panel = payoffStructure(i.kind, i.s, i.other_listed === false);
      const n = panel.cols.length; // the panel draws a NO basket without an Other column; the engine adds one
      expect(engine.cols.slice(0, n), c.name).toEqual(panel.cols.map((x) => (x === OTHER ? 'Other' : x))); // the panel words the last column out
      expect(engine.rows.map((r) => r.name.replace('_', ' ')), c.name).toEqual(panel.rows.map((r) => r.name));
      expect(engine.rows.map((r) => r.cells.slice(0, n)), c.name).toEqual(panel.rows.map((r) => r.cells));
      if (i.kind === 'yes') {
        expect(engine.broken, c.name).toEqual(panel.broken);
        expect(engine.basket.map(Number), c.name).toEqual(panel.basket);
      } else {
        expect(engine.broken.some(Boolean), c.name).toBe(false);
        expect(panel.broken.some(Boolean), c.name).toBe(false);
        expect(Number(engine.guaranteed), c.name).toBe(panel.guaranteed);
      }
      checked += 1;
    }
    expect(checked).toBeGreaterThan(0);
  });
});
