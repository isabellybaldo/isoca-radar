// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { OTHER, payoffStructure } from '../explain/models';
import { formatMoneyCents } from '../lib/money';
import { fmtDuration } from '../lib/time';
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
  const draw = (m: EpisodeModel) => render(<><UpdateBars updates={m.updates} peakMs={m.peak_ms} /><DurationBars rows={episodeRows(m)} /></>);

  it('puts the episode on the table and our order on one time scale, and the edge per update', () => {
    const open = MISS.open_ms as number;
    const rows = episodeRows(MISS);
    const onTable = (MISS.close_ms as number) - open;
    const order = (MISS.t_x as number) - open;
    expect(rows).toEqual([
      { kind: 'row', label: 'on the table', ms: onTable, open: false, ticks: [onTable] },
      { kind: 'order', label: 'our order lands', ms: order, open: false, ticks: [] },
    ]);
    const { container } = draw(MISS);
    expect(screen.getByText(`on the table: ${fmtDuration(onTable)}`)).toBeInTheDocument();
    expect(screen.getByText(`our order lands: ${fmtDuration(order)}`)).toBeInTheDocument();
    expect(screen.getByRole('figure', { name: 'how long the episode stayed on the table, and when our order landed' })).toBeInTheDocument();
    expect(container.querySelector('rect.db-bar.order')).not.toBeNull();
    expect(container.querySelectorAll('rect.ub-bar')).toHaveLength(MISS.updates.length);
    expect(container.querySelectorAll('rect.ub-bar.peak')).toHaveLength(1);
    expect(container.querySelector('svg.ub-svg')).toHaveAttribute('preserveAspectRatio', 'none');
    expect(screen.getAllByText('no edge')).toHaveLength(MISS.updates.filter((u) => u.net === null).length);
    expect(screen.getByText(formatMoneyCents(MISS.peak_net, { signed: true }))).toBeInTheDocument();
  });

  it('frames the dollar figures in a last-child caption', () => {
    const { container } = draw(MISS);
    const fig = container.querySelector('figure.ubars') as HTMLElement;
    expect(fig.lastElementChild?.tagName).toBe('FIGCAPTION');
    expect(fig.lastElementChild).toHaveTextContent('net edge on the table at each update');
  });

  it('says peak in words on the update the engine named, and nowhere when there is none', () => {
    for (const name of ['slow-miss', 'tx-at-update']) {
      const m = episode(name);
      const { container, unmount } = draw(m);
      const peaks = [...container.querySelectorAll('.ub-axis li')].filter((li) => li.textContent?.includes('peak'));
      expect(peaks, name).toHaveLength(1);
      expect(peaks[0].textContent, name).toContain(`${m.peak_ms} ms`);
      unmount();
    }
    const never = episode('never-opens');
    const { container } = draw(never);
    expect(container.textContent).not.toContain('peak');
  });

  it('every update column has the same lines, so the times and amounts align: "peak" has its own line, kept empty elsewhere (Task 16 V4)', () => {
    const { container } = draw(MISS);
    const items = [...container.querySelectorAll('.ub-axis li')];
    expect(items).toHaveLength(MISS.updates.length);
    const peak = MISS.updates.findIndex((u) => u.t_ms === MISS.peak_ms);
    expect(peak).toBeGreaterThanOrEqual(0);
    for (const [i, li] of items.entries()) {
      expect([...li.children].map((c) => `${c.tagName.toLowerCase()}${c.className === '' ? '' : `.${c.className}`}`), `u${i + 1}`).toEqual(['span.ub-mark', 'b', 'span', 'span']);
      expect(li.children[0].textContent, `u${i + 1}`).toBe(i === peak ? 'peak' : '');
      expect(li.children[1].textContent).toBe(`u${i + 1}`); // the name never widens: no "u3 · peak"
      expect(li.children[2].textContent).toBe(`${MISS.updates[i].t_ms} ms`);
    }
  });

  it('the dashboard label of DurationBars is unchanged without an order row', () => {
    render(<DurationBars rows={[{ kind: 'row', label: 'x', ms: 10, open: false, ticks: [] }]} />);
    expect(screen.getByRole('figure', { name: 'how long each stayed on the table' })).toBeInTheDocument();
  });

  it('an episode that never opens draws no episode rows and no NaN', () => {
    const never = episode('never-opens');
    expect(episodeRows(never)).toEqual([]);
    const { container } = draw(never);
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
    expect(screen.getByText(`on the table: still open, ${fmtDuration(last - o)} so far`)).toBeInTheDocument();
  });

  it('ticks stop at the close: none after the bar end', () => {
    const m: EpisodeModel = { ...MISS, updates: [...MISS.updates, { t_ms: (MISS.close_ms as number) + 500, net: null }] };
    const row = episodeRows(m)[0];
    expect(Math.max(...row.ticks)).toBeLessThanOrEqual(row.ms);
  });

  it('fast-catch: the order bar is shorter than the table bar', () => {
    const { container } = draw(episode('fast-catch'));
    const w = [...container.querySelectorAll('rect.db-bar')].map((r) => Number(r.getAttribute('width')));
    expect(w[1]).toBeLessThan(w[0]);
  });

  it('tx-at-close: the two bars are equal', () => {
    const m = episode('tx-at-close');
    const [t, o] = episodeRows(m);
    expect(o.ms).toBe(t.ms);
    const { container } = draw(m);
    const widths = [...container.querySelectorAll('rect.db-bar')].map((r) => r.getAttribute('width'));
    expect(widths[0]).toBe(widths[1]);
  });

  it('tx-at-update: the order lands on an update tick', () => {
    const m = episode('tx-at-update');
    const [t, o] = episodeRows(m);
    expect(t.ticks).toContain(o.ms);
  });

  it('a one-update episode draws one bar without NaN', () => {
    const { container } = render(<UpdateBars updates={[{ t_ms: 0, net: MISS.peak_net }]} peakMs={0} />);
    expect(container.querySelectorAll('rect.ub-bar')).toHaveLength(1);
    expect(container.innerHTML).not.toContain('NaN');
  });
});

describeWasm('implication shows only what the engine uses (ruling R10)', () => {
  it('top_edge_gross moves with exactly the sides IMPLICATION_PRICE_USE names, and with no others', async () => {
    const eng = await loadTestEngine();
    type Px = { bid: string; ask: string };
    const BASE: Px[] = [{ bid: '0.70', ask: '0.72' }, { bid: '0.40', ask: '0.42' }];
    const edge = (prices: Px[]) => {
      const input: PayoffIn = { kind: 'implication', s: 2, other_listed: true, prices };
      const o: unknown = JSON.parse(eng.payoff_json(JSON.stringify(input)));
      if (isError(o)) throw new Error(o.error);
      return (o as PayoffMatrix).top_edge_gross;
    };
    const base = edge(BASE);
    for (const [leg, name] of ['A', 'B'].entries()) {
      for (const side of ['bid', 'ask'] as const) {
        const used = IMPLICATION_PRICE_USE.some((u) => u.leg === name && u.side === side);
        const moved = BASE.map((p, i) => (i === leg ? { ...p, [side]: (Number(p[side]) + (side === 'ask' ? 0.05 : -0.05)).toFixed(2) } : p));
        if (used) expect(edge(moved), `${name} ${side}`).not.toBe(base);
        else expect(edge(moved), `${name} ${side}`).toBe(base);
      }
    }
  });
});

const HAVE_PAYOFF: boolean = existsSync(`${FIXTURES}/payoff_json.json`);

describe.skipIf(!HAVE_PAYOFF)('the panel payoff drawing against every YES and NO payoff fixture, S 2–6 (09-27 §7.3)', () => {
  it('matches the engine on the columns the panel draws, and the NO Other column is the known difference', () => {
    let checked = 0;
    let noOther = 0;
    for (const c of fixtureCases('payoff_json')) {
      const i = JSON.parse(c.input) as { kind?: string; s?: number; other_listed?: boolean };
      const o: unknown = JSON.parse(c.output);
      if ((i.kind !== 'yes' && i.kind !== 'no') || typeof i.s !== 'number' || i.s < 2 || i.s > 6 || isError(o)) continue;
      const engine = matrixToPayoff(o as PayoffMatrix, i.kind);
      const uncovered = i.other_listed === false;
      const panel = payoffStructure(i.kind, i.s, uncovered);
      const n = panel.cols.length; // the panel draws a NO basket without an Other column; the engine adds one
      expect(engine.cols.slice(0, n), c.name).toEqual(panel.cols.map((x) => (x === OTHER ? 'Other' : x))); // the panel words the last column out
      expect(engine.rows.map((r) => r.name.replace('_', ' ')), c.name).toEqual(panel.rows.map((r) => r.name));
      expect(engine.rows.map((r) => r.cells.slice(0, n)), c.name).toEqual(panel.rows.map((r) => r.cells));
      expect(engine.basket.slice(0, n).map(Number), c.name).toEqual(panel.basket);
      expect(Number(engine.guaranteed), c.name).toBe(panel.guaranteed);
      if (i.kind === 'yes') {
        expect(engine.broken, c.name).toEqual(panel.broken);
      } else {
        expect(engine.broken.some(Boolean), c.name).toBe(false);
        expect(panel.broken.some(Boolean), c.name).toBe(false);
        if (uncovered) {
          expect(engine.cols.length, c.name).toBe(n + 1);
          expect(engine.basket[n], c.name).toBe(String(i.s));
          noOther += 1;
        } else {
          expect(engine.cols.length, c.name).toBe(n);
        }
      }
      checked += 1;
    }
    expect(checked).toBeGreaterThan(0);
    expect(noOther).toBeGreaterThan(0);
  });
});
