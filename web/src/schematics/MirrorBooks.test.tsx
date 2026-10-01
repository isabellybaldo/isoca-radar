import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { describeWasm, fixtureCase, loadTestEngine } from '../wasm/testEngine';
import type { Engine, MirrorIn, MirrorModel } from '../wasm/types';
import { along, mirrorDomain, tickText } from './geo';
import { CROSSED, MirrorBooks } from './MirrorBooks';

/** A real mirror_json fixture: the input book and the engine's own output for it (never retyped). */
const mirror = (name: string) => {
  const c = fixtureCase('mirror_json', name);
  return { input: JSON.parse(c.input) as MirrorIn, model: JSON.parse(c.output) as MirrorModel };
};
const num = (el: Element | undefined, a: string) => Number(el?.getAttribute(a));
/** The axis the drawing must use: zoomed onto every price in both rows (YES levels and their NO mirrors). */
const domainOf = (input: MirrorIn, model: MirrorModel) => mirrorDomain([...input.bids, ...input.asks, ...model.no_bids, ...model.no_asks].map((l) => l.p));
/** One row's bars as [left, right] in viewBox units, left to right. */
const spans = (c: HTMLElement, row: 'mb-yes' | 'mb-no') =>
  [...c.querySelectorAll(`g.${row} rect`)].map((r) => [num(r, 'x'), num(r, 'x') + num(r, 'width')]).sort((a, b) => a[0] - b[0]);
const axis = (c: HTMLElement) => [...c.querySelectorAll('.mb-axis li')].map((li) => li.textContent);
/** The plot's width on a 360 px screen is about 272 px (314 px of section less the 4ch row label); 240 is a floor. */
const PX_360 = 240 / 300;

describe('MirrorBooks (parent §5.7 a)', () => {
  it('draws both books on one price axis and joins each YES level to its NO mirror', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    const svg = container.querySelector('svg.mb-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.mb-bid')).toHaveLength(input.bids.length + model.no_bids.length);
    expect(container.querySelectorAll('rect.mb-ask')).toHaveLength(input.asks.length + model.no_asks.length);
    const links = [...container.querySelectorAll('line.mb-link')];
    expect(links).toHaveLength(input.bids.length + input.asks.length);
    // the best bid's line runs from its YES price to the NO ask at 1 - p
    const d = domainOf(input, model);
    expect(num(links[0], 'x1')).toBeCloseTo(along(d, input.bids[0].p) * 300);
    expect(num(links[0], 'x2')).toBeCloseTo(along(d, model.no_asks[0].p) * 300);
    expect(screen.getByText(`D1 long ${model.d1_long}`)).toBeInTheDocument();
    expect(screen.getByText(`short ${model.d1_short}`)).toBeInTheDocument();
    expect(screen.getByText(`NO ask ${model.no_asks[0].p} (1 − ${model.best_bid?.p})`)).toBeInTheDocument();
    expect(screen.getByText(`NO bid ${model.no_bids[0].p} (1 − ${model.best_ask?.p})`)).toBeInTheDocument();
    expect(screen.getByText(`spread ${model.spread}`)).toBeInTheDocument();
    expect(screen.queryByText(CROSSED)).toBeNull();
  });

  it('by default the D1 line is there', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(container.querySelector('.mb-big')).toHaveTextContent(`D1 long ${model.d1_long}`);
  });

  it('headline={false} leaves the D1 line out and keeps the drawing, the alert and the mirror lines', () => {
    const { input, model } = mirror('crossed');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} headline={false} />);
    expect(container.querySelector('.mb-big')).toBeNull();
    expect(container.textContent).not.toContain('D1');
    expect(container.querySelector('svg.mb-svg')).not.toBeNull();
    expect(container.querySelectorAll('line.mb-link')).toHaveLength(input.bids.length + input.asks.length);
    expect(screen.getByText(CROSSED)).toHaveClass('mb-alert');
    expect(container.querySelector('.mb-refl')?.children.length).toBeGreaterThan(0);
  });

  it('zooms the price axis onto the prices drawn: the ends and 0.50 are labelled, 0.50 stays in the middle (Task 16 V1)', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    const d = domainOf(input, model);
    expect(d.hi - d.lo).toBeLessThan(100); // the fed book sits near 0.50: the axis is not 0 to 1
    expect(axis(container)).toEqual([tickText(d.lo), '0.50', tickText(d.hi)]);
    expect(num(container.querySelector('line.mb-mid') ?? undefined, 'x1')).toBe(150);
    // the two rows now spread over most of the width instead of a band in the middle
    const all = [...spans(container, 'mb-yes'), ...spans(container, 'mb-no')];
    expect(Math.max(...all.map((b) => b[1])) - Math.min(...all.map((b) => b[0]))).toBeGreaterThan(0.75 * 300);
  });

  it('keeps every bar inside the plot and apart from its neighbours at 360 px, 8 levels a row (Task 16 V1)', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    for (const row of ['mb-yes', 'mb-no'] as const) {
      const bars = spans(container, row);
      expect(bars, row).toHaveLength(8);
      for (const [i, [x0, x1]] of bars.entries()) {
        expect(x0, row).toBeGreaterThanOrEqual(0);
        expect(x1, row).toBeLessThanOrEqual(300);
        expect((x1 - x0) * PX_360, row).toBeGreaterThanOrEqual(4); // a bar one can see
        if (i > 0) expect((x0 - bars[i - 1][1]) * PX_360, row).toBeGreaterThanOrEqual(2); // a gap one can see
      }
    }
  });

  it('each join line runs from the foot of a YES bar to the top of its NO mirror, across a gap one can see (Task 16 V1)', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    const mid = (r: number[]) => (r[0] + r[1]) / 2;
    const yes = spans(container, 'mb-yes').map(mid);
    const no = spans(container, 'mb-no').map(mid);
    const height = Number(container.querySelector('svg.mb-svg')?.getAttribute('viewBox')?.split(' ')[3]);
    const links = [...container.querySelectorAll('line.mb-link')];
    expect(links).toHaveLength(8);
    for (const l of links) {
      expect(yes.some((x) => Math.abs(x - num(l, 'x1')) < 1e-6)).toBe(true);
      expect(no.some((x) => Math.abs(x - num(l, 'x2')) < 1e-6)).toBe(true);
      expect((num(l, 'y2') - num(l, 'y1')) / height).toBeGreaterThanOrEqual(0.2); // a fifth of the drawing, not 4 units
    }
  });

  it('names a crossed book in the alert colour', () => {
    const { input, model } = mirror('crossed');
    expect(model.crossed).toBe(true);
    render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(screen.getByText(CROSSED)).toHaveClass('mb-alert');
  });

  it('crossed is the engine flag, not a price comparison', () => {
    const fed = mirror('fed-book');
    const crossedPrices = mirror('crossed');
    const { unmount } = render(<MirrorBooks bids={fed.input.bids} asks={fed.input.asks} model={{ ...fed.model, crossed: true }} />);
    expect(screen.getByText(CROSSED)).toBeInTheDocument();
    unmount();
    render(<MirrorBooks bids={crossedPrices.input.bids} asks={crossedPrices.input.asks} model={{ ...crossedPrices.model, crossed: false }} />);
    expect(screen.queryByText(CROSSED)).toBeNull();
  });

  it('a crossed spread shows a real minus sign', () => {
    const { input, model } = mirror('crossed');
    render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(screen.getByText('spread −0.01')).toBeInTheDocument();
  });

  it('colours the NO rows by the side they are on (bids-only: 1 YES bid bar, 1 NO ask bar, no NO bid bar)', () => {
    const { input, model } = mirror('bids-only');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(container.querySelectorAll('rect.mb-bid')).toHaveLength(1);
    expect(container.querySelectorAll('rect.mb-ask')).toHaveLength(1);
  });

  it('lists every level with its mirror in a closed fold, sizes as sent', () => {
    const { input, model } = mirror('fed-book');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    const fold = container.querySelector('details.fold');
    expect(fold).not.toHaveAttribute('open');
    expect(fold?.querySelector('summary')).toHaveTextContent('every level');
    const li = [...container.querySelectorAll('ul.mb-levels li')];
    expect(li).toHaveLength(input.bids.length + input.asks.length);
    const b = input.bids[1];
    const n = model.no_asks.find((x) => x.s === b.s);
    expect(li[1]).toHaveTextContent(`YES bid ${b.p} → NO ask ${n?.p} (1 − ${b.p}) · size ${b.s}`);
  });

  it('an empty book draws no bars and no NaN (review focus 3)', () => {
    const { input, model } = mirror('empty');
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(container.querySelectorAll('rect')).toHaveLength(0);
    expect(screen.getByText('D1 long —')).toBeInTheDocument();
    expect(container.querySelector('.mb-refl')?.children).toHaveLength(0);
  });

  it('a one-sided book names only the side it has', () => {
    const { input, model } = mirror('bids-only');
    render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(screen.getByText(`NO ask ${model.no_asks[0].p} (1 − ${model.best_bid?.p})`)).toBeInTheDocument();
    expect(screen.queryByText(/^NO bid/)).toBeNull();
    expect(screen.queryByText(/^spread/)).toBeNull();
  });

  it('pairs mirrored rows by price, not by input index (R9)', () => {
    const { input, model } = mirror('fed-book');
    // a drag reordered the YES levels: the best bid is no longer first, the mirror rows keep the engine's order
    const bids = [...input.bids].reverse();
    const asks = [...input.asks].reverse();
    const { container } = render(<MirrorBooks bids={bids} asks={asks} model={model} />);
    expect(screen.getByText(`NO ask ${model.no_asks[0].p} (1 − ${model.best_bid?.p})`)).toBeInTheDocument();
    expect(screen.getByText(`NO bid ${model.no_bids[0].p} (1 − ${model.best_ask?.p})`)).toBeInTheDocument();
    for (const l of container.querySelectorAll('line.mb-link')) {
      expect(num(l, 'x1') + num(l, 'x2')).toBeCloseTo(300); // p and 1 − p are mirror images around the 0.50 line
    }
  });
});

describeWasm('MirrorBooks on the real engine', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });
  it('draws what mirror_json returns for the fed book', () => {
    const c = fixtureCase('mirror_json', 'fed-book');
    const input = JSON.parse(c.input) as MirrorIn;
    const model = JSON.parse(engine.mirror_json(c.input)) as MirrorModel;
    const { container } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
    expect(container.querySelectorAll('line.mb-link')).toHaveLength(input.bids.length + input.asks.length);
    expect(screen.getByText(`D1 long ${model.d1_long}`)).toBeInTheDocument();
  });

  it('the axis follows a level dragged far away, and a one-level book does not blow up (Task 16 V1)', () => {
    const fed = JSON.parse(fixtureCase('mirror_json', 'fed-book').input) as MirrorIn;
    const far: MirrorIn = { ...fed, bids: [...fed.bids.slice(0, 3), { p: '0.05', s: fed.bids[3].s }] };
    const one: MirrorIn = { bids: [fed.bids[0]], asks: [] };
    for (const input of [far, one]) {
      const model = JSON.parse(engine.mirror_json(JSON.stringify(input))) as MirrorModel;
      const d = domainOf(input, model);
      const { container, unmount } = render(<MirrorBooks bids={input.bids} asks={input.asks} model={model} />);
      expect(axis(container)).toEqual([tickText(d.lo), '0.50', tickText(d.hi)]);
      for (const row of ['mb-yes', 'mb-no'] as const) {
        for (const [x0, x1] of spans(container, row)) {
          expect(x0).toBeGreaterThanOrEqual(0);
          expect(x1).toBeLessThanOrEqual(300);
          expect(x1 - x0).toBeLessThanOrEqual(30); // one level is one bar, not a slab
        }
      }
      unmount();
    }
    const farModel = JSON.parse(engine.mirror_json(JSON.stringify(far))) as MirrorModel;
    expect(domainOf(far, farModel)).toEqual({ lo: 3, hi: 97 });
  });
});
