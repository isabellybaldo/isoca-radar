import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { describeWasm, fixtureCase, fixtureCases, loadTestEngine } from '../wasm/testEngine';
import { CLASSIFY_NODES, type ClassifyModel, type Engine } from '../wasm/types';
import { ClassifyTree, NODES, endText } from './ClassifyTree';

/** The engine's own output for a real classify_json fixture (never retyped). */
const classify = (name: string) => JSON.parse(fixtureCase('classify_json', name).output) as ClassifyModel;
const lit = (c: HTMLElement) => [...c.querySelectorAll('li.cl-node.lit')].map((li) => li.getAttribute('data-node'));

describe('ClassifyTree (parent §5.7 c)', () => {
  it('lists the 14 nodes in the engine contract order', () => {
    expect(NODES.map(([id]) => id)).toEqual([...CLASSIFY_NODES]);
  });

  it('lights the path classify took and ends in the kind', () => {
    const m = classify('no-edge-3');
    const { container } = render(<ClassifyTree model={m} />);
    expect(container.querySelectorAll('li.cl-node')).toHaveLength(14);
    expect(lit(container)).toEqual(m.path);
    expect(screen.getByText('NO basket · via the converter')).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net')).toHaveTextContent(`net Π +$${Number(m.net).toFixed(2)} on the table · gross +$${Number(m.gross).toFixed(2)} · Q* ${m.q_star}`);
  });

  it('ends in nothing for a decided event', () => {
    const m = classify('decided');
    const { container } = render(<ClassifyTree model={m} />);
    expect(lit(container)).toEqual(['decided']);
    expect(screen.getByText('nothing')).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net')).toBeNull();
  });

  it('marks only the last lit node as the current step; the other lit nodes say ran', () => {
    const m = classify('no-edge-3');
    const { container } = render(<ClassifyTree model={m} />);
    const cur = [...container.querySelectorAll('li[aria-current]')].map((li) => li.getAttribute('data-node'));
    expect(cur).toEqual([m.path[m.path.length - 1]]);
    expect(container.querySelectorAll('.cl-ran')).toHaveLength(m.path.length - 1);
    expect(container.querySelector('li[data-node="decided"] .cl-ran')).toBeNull();
  });

  it('lights both branches of a crossed leg and says nothing', () => {
    const m = classify('crossed-leg');
    const { container } = render(<ClassifyTree model={m} />);
    expect(lit(container)).toEqual(['no_basket_tried', 'no_basket_crossed', 'yes_basket_tried', 'yes_basket_crossed']);
    expect(screen.getByText('nothing')).toHaveClass('cl-end');
  });

  it('an incomplete set ends in a YES basket that is not arbitrage', () => {
    const m = classify('augmented-complete');
    const { container } = render(<ClassifyTree model={m} />);
    expect(lit(container)).toEqual(m.path);
    expect(endText(m)).toBe('YES basket, incomplete set · not arbitrage');
    expect(screen.getByText(endText(m))).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net') === null).toBe(m.net === null);
  });

  it('shows a node the page does not know instead of dropping it (review focus 4)', () => {
    const m: ClassifyModel = { ...classify('decided'), path: ['no_basket_tried', 'brand_new_node'] };
    const { container } = render(<ClassifyTree model={m} />);
    expect(lit(container)).toEqual(['no_basket_tried', 'brand_new_node']);
    expect(container.querySelectorAll('li.cl-node')).toHaveLength(15);
    expect(screen.getByText('brand_new_node')).toBeInTheDocument();
  });

  it('every real fixture path is lit on known nodes only, in tree order', () => {
    const order = NODES.map(([id]) => id as string);
    for (const c of fixtureCases('classify_json')) {
      if (c.output.startsWith('{"error"')) continue;
      const m = JSON.parse(c.output) as ClassifyModel;
      const { container, unmount } = render(<ClassifyTree model={m} />);
      expect(container.querySelectorAll('li.cl-node'), c.name).toHaveLength(14);
      expect(lit(container), c.name).toEqual([...m.path].sort((a, b) => order.indexOf(a) - order.indexOf(b)));
      unmount();
    }
  });
});

describeWasm('ClassifyTree on the real engine', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });
  it('lights the path classify_json returns', () => {
    const m = JSON.parse(engine.classify_json(fixtureCase('classify_json', 'fees-on').input)) as ClassifyModel;
    const { container } = render(<ClassifyTree model={m} />);
    expect(lit(container)).toEqual(m.path);
    expect(screen.getByText(endText(m))).toHaveClass('cl-end');
  });
});
