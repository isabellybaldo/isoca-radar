import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { describeWasm, fixtureCase, loadTestEngine } from '../wasm/testEngine';
import type { Engine, MirrorIn, MirrorModel } from '../wasm/types';
import { CROSSED, MirrorBooks } from './MirrorBooks';

/** A real mirror_json fixture: the input book and the engine's own output for it (never retyped). */
const mirror = (name: string) => {
  const c = fixtureCase('mirror_json', name);
  return { input: JSON.parse(c.input) as MirrorIn, model: JSON.parse(c.output) as MirrorModel };
};
const num = (el: Element | undefined, a: string) => Number(el?.getAttribute(a));

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
    expect(num(links[0], 'x1')).toBeCloseTo(Number(input.bids[0].p) * 300);
    expect(num(links[0], 'x2')).toBeCloseTo(Number(model.no_asks[0].p) * 300);
    expect(screen.getByText(`D1 long ${model.d1_long}`)).toBeInTheDocument();
    expect(screen.getByText(`short ${model.d1_short}`)).toBeInTheDocument();
    expect(screen.getByText(`NO ask ${model.no_asks[0].p} (1 − ${model.best_bid?.p})`)).toBeInTheDocument();
    expect(screen.getByText(`NO bid ${model.no_bids[0].p} (1 − ${model.best_ask?.p})`)).toBeInTheDocument();
    expect(screen.getByText(`spread ${model.spread}`)).toBeInTheDocument();
    expect(screen.queryByText(CROSSED)).toBeNull();
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
      expect(num(l, 'x1') + num(l, 'x2')).toBeCloseTo(300); // p and 1 − p are mirror images on a 300-wide axis
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
});
