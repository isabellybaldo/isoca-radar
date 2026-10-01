import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it } from 'vitest';
import { describeWasm, fixtureCase, fixtureCases, loadTestEngine } from '../wasm/testEngine';
import { CLASSIFY_NODES, type ClassifyModel, type Engine } from '../wasm/types';
import { formatMoneyCents } from '../lib/money';
import { ClassifyTree, NODES, NOT_RISK_FREE, endText } from './ClassifyTree';

/** The engine's own output for a real classify_json fixture (never retyped). */
const classify = (name: string) => JSON.parse(fixtureCase('classify_json', name).output) as ClassifyModel;
/** The money line as the page words it, from the engine's strings through the page's own formatter (R12). */
const netLine = (m: ClassifyModel, qualifier: string) =>
  `net Π ${formatMoneyCents(m.net, { signed: true })} on the table${qualifier} · gross ${formatMoneyCents(m.gross, { signed: true })} · Q* ${m.q_star}`;
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
    expect(m.realization).toBe('converter');
    expect(container.querySelector('.cl-net')?.textContent).toBe(netLine(m, ''));
    expect(container.querySelector('.cl-net')).not.toHaveTextContent(NOT_RISK_FREE);
  });

  it('path={false} leaves the fourteen tests out and keeps where they end and the money line (the story, chapter 3)', () => {
    const m = classify('no-edge-3');
    const { container } = render(<ClassifyTree model={m} path={false} />);
    expect(container.querySelector('.cl-path')).toBeNull();
    expect(container.querySelectorAll('.cl-node, .cl-dot, .cl-ran')).toHaveLength(0);
    expect([...(container.querySelector('figure.ctree')?.children ?? [])].map((k) => k.className)).toEqual(['cl-end', 'cl-net']);
    expect(screen.getByText(endText(m))).toHaveClass('cl-end');
    expect(container.querySelector('.cl-net')?.textContent).toBe(netLine(m, ''));
  });

  it('path={false} with no signal is the one word, and no money line', () => {
    const { container } = render(<ClassifyTree model={classify('decided')} path={false} />);
    expect(container.querySelector('figure.ctree')?.textContent).toBe('nothing');
    expect(container.querySelector('.cl-net')).toBeNull();
  });

  it('by default the path is there (section c)', () => {
    const { container } = render(<ClassifyTree model={classify('decided')} />);
    expect(container.querySelector('ol.cl-path')?.children).toHaveLength(14);
    expect([...(container.querySelector('figure.ctree')?.children ?? [])].map((k) => k.className)).toEqual(['cl-path', 'cl-end']);
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
    expect(m.realization).toBe('not_arbitrage');
    expect(NOT_RISK_FREE).toBe('not risk-free'); // section b's words for the same situation
    expect(container.querySelector('.cl-net')?.textContent).toBe(netLine(m, `, ${NOT_RISK_FREE}`));
  });

  it('the money line says not risk-free exactly when the engine says not_arbitrage, on every real fixture', () => {
    const seen = { not_arbitrage: 0, arbitrage: 0 };
    for (const c of fixtureCases('classify_json')) {
      if (c.output.startsWith('{"error"')) continue;
      const m = JSON.parse(c.output) as ClassifyModel;
      const { container, unmount } = render(<ClassifyTree model={m} />);
      const line = container.querySelector('.cl-net');
      expect(line === null, c.name).toBe(m.net === null);
      if (line !== null) {
        const risky = m.realization === 'not_arbitrage';
        expect(line.textContent, c.name).toBe(netLine(m, risky ? `, ${NOT_RISK_FREE}` : ''));
        expect(line.textContent?.includes(NOT_RISK_FREE), c.name).toBe(risky);
        seen[risky ? 'not_arbitrage' : 'arbitrage'] += 1;
      }
      unmount();
    }
    expect(seen.not_arbitrage).toBeGreaterThan(1); // yes-incomplete-other and augmented-complete
    expect(seen.arbitrage).toBeGreaterThan(1);
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

  it('only an incomplete YES basket reaches not_arbitrage, and its money line carries the qualifier', () => {
    let risky = 0;
    for (const c of fixtureCases('classify_json')) {
      const m = JSON.parse(engine.classify_json(c.input)) as ClassifyModel | { error: string };
      if ('error' in m) continue;
      expect(m.realization === 'not_arbitrage', c.name).toBe(m.kind === 'yes_basket_incomplete');
      if (m.realization !== 'not_arbitrage') continue;
      const { container, unmount } = render(<ClassifyTree model={m} />);
      expect(container.querySelector('.cl-end')?.textContent, c.name).toBe('YES basket, incomplete set · not arbitrage');
      expect(container.querySelector('.cl-net')?.textContent, c.name).toBe(netLine(m, `, ${NOT_RISK_FREE}`));
      unmount();
      risky += 1;
    }
    expect(risky).toBeGreaterThan(0);
  });
});
