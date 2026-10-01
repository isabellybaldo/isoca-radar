import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { HAVE_PKG, WASM_REQUIRED, fixtureCase, loadTestEngine } from '../wasm/testEngine';
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
    expect(screen.getByText(`spread ${model.spread}`)).toBeInTheDocument();
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

describe.skipIf(!HAVE_PKG && !WASM_REQUIRED)('MirrorBooks on the real engine', () => {
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
