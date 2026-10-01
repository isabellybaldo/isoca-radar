import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { describeWasm, fixtureCase, fixtureCases, loadTestEngine } from '../wasm/testEngine';
import type { WalkModel } from '../wasm/types';
import { DepthStairs } from './DepthStairs';
import { formatDecimal } from '../lib/money';
import { costDomain, geo, tickText } from './geo';

/** A real depth_walk_json fixture: the engine's own output for its input (never retyped). */
const walk = (name: string) => JSON.parse(fixtureCase('depth_walk_json', name).output) as WalkModel;
const okCases = (): { name: string; model: WalkModel }[] =>
  fixtureCases('depth_walk_json').flatMap((c) => {
    const o = JSON.parse(c.output) as WalkModel | { error: string };
    return 'error' in o ? [] : [{ name: c.name, model: o }];
  });
const last = (m: WalkModel) => (m.steps.length > 0 ? m.steps[m.steps.length - 1].q_to : '0');
const yOf = (el: Element | undefined) => Number(el?.getAttribute('y1'));
const H = 120;
/** Every horizontal line of the drawing: the step costs and what each step pays. */
const lineYs = (c: HTMLElement) => [...c.querySelectorAll('line.ds-cost, line.ds-pay')].map(yOf);
const yLabels = (c: HTMLElement) => [...c.querySelectorAll('.ds-yaxis li')].map((li) => li.textContent);

describe('DepthStairs (parent §5.7 d)', () => {
  it('shades Π up to Q*, draws the steps past it, marks Q*, and says Π and Q* in HTML', () => {
    const m = walk('yes-two-legs');
    const { container } = render(<DepthStairs model={m} />);
    const svg = container.querySelector('svg.ds-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.ds-pi')).toHaveLength(1);
    expect(container.querySelectorAll('rect.ds-loss')).toHaveLength(0); // the step past Q* has cost = payoff: zero height
    expect(container.querySelectorAll('line.ds-cost')).toHaveLength(m.steps.length);
    expect(container.querySelectorAll('line.ds-pay')).toHaveLength(m.steps.length);
    expect(Number(container.querySelector('line.ds-qstar')?.getAttribute('x1'))).toBeCloseTo((geo(m.q_star) / geo(last(m))) * 300);
    expect(container.querySelector('.ds-big')).toHaveTextContent(/^Π \+\$1\.00/); // pi "1" through <Money cents>
    expect(screen.getByText(`Q* ${formatDecimal(m.q_star)}`)).toBeInTheDocument();
    expect(container.querySelector('.ds-gross')).toHaveTextContent(/^these baskets before fees \+\$1\.00$/);
    expect(screen.getByText(`${formatDecimal(last(m))} shares`)).toBeInTheDocument();
  });

  it('frames its dollars "on the table" and shows the engine Π, not a sum of the steps', () => {
    const { container } = render(<DepthStairs model={walk('fees-on')} />);
    expect(container.textContent).toContain('on the table');
    expect(container.querySelector('.ds-big')).toHaveTextContent(/^Π \+\$0\.69/); // pi 0.685 rounds to the cent
    expect(container.querySelector('.ds-gross')).toHaveTextContent(/^these baskets before fees \+\$1\.00$/);
  });

  it('keeps its dollars in the figure whose caption says "on the table"', () => {
    const { container } = render(<DepthStairs model={walk('yes-two-legs')} />);
    expect(container.querySelector('figure figcaption')).toHaveTextContent(/^on the table/);
  });

  it('pins the legend text to its swatch: profit area, loss area, cost line, payoff line, Q* marker', () => {
    const { container } = render(<DepthStairs model={walk('yes-two-legs')} />);
    const rows = [...container.querySelectorAll('ul.ds-legend li')].map((li) => [li.querySelector('.ds-sw')?.className, li.textContent]);
    expect(rows).toEqual([
      ['ds-sw pi', 'the next basket pays more than it costs'],
      ['ds-sw loss', 'past Q*: it costs more than it pays'],
      ['ds-sw cost', 'what the next basket costs'],
      ['ds-sw pay', 'what a basket pays'],
      ['ds-sw qstar', 'Q*: the last basket worth buying'],
    ]);
  });

  it('zooms the cost axis onto the stairs and labels both ends in HTML beside the drawing (Task 16 V2)', () => {
    for (const { name, model } of okCases()) {
      const { container, unmount } = render(<DepthStairs model={model} />);
      const d = costDomain(model.steps.flatMap((s) => [s.cost, s.payoff]));
      if (model.steps.length === 0) {
        expect(container.querySelector('.ds-yaxis'), name).toBeNull(); // nothing drawn: no axis ends beside an empty plot
        unmount();
        continue;
      }
      expect(yLabels(container), name).toEqual([tickText(d.hi), tickText(d.lo)]); // top first
      expect(container.querySelector('.ds-yaxis'), name).toHaveAttribute('aria-hidden', 'true');
      for (const y of lineYs(container)) {
        expect(y, name).toBeGreaterThan(0); // no line on the frame
        expect(y, name).toBeLessThan(H);
      }
      unmount();
    }
  });

  it('the stairs and the pays line use most of the height, so a step can be told from what it pays (Task 16 V2)', () => {
    for (const name of ['yes-two-legs', 'fees-on', 'no-nested-three']) {
      const { container, unmount } = render(<DepthStairs model={walk(name)} />);
      const ys = lineYs(container);
      expect(Math.max(...ys) - Math.min(...ys), name).toBeGreaterThanOrEqual(0.6 * H);
      unmount();
    }
  });

  it('draws each step against its own payoff (the nested NO rule, fixture no-nested-three)', () => {
    const m = walk('no-nested-three');
    const { container } = render(<DepthStairs model={m} />);
    const pays = [...container.querySelectorAll('line.ds-pay')].map(yOf);
    expect(pays).toHaveLength(m.steps.length);
    expect(pays[0]).toBeLessThan(pays[pays.length - 1]); // payoff 2 sits above payoff 1
    const losses = [...container.querySelectorAll('rect.ds-loss')];
    expect(losses).toHaveLength(m.steps.filter((s) => !s.profitable).length);
    const loss = losses[losses.length - 1];
    expect(Number(loss.getAttribute('y')) + Number(loss.getAttribute('height'))).toBeCloseTo(pays[pays.length - 1]);
  });

  it('every fixture case draws without NaN and keeps Q* inside the plot', () => {
    for (const { name, model } of okCases()) {
      const { container, unmount } = render(<DepthStairs model={model} />);
      expect(container.innerHTML, name).not.toContain('NaN');
      const x = Number(container.querySelector('line.ds-qstar')?.getAttribute('x1'));
      expect(x, name).toBeGreaterThanOrEqual(0);
      expect(x, name).toBeLessThanOrEqual(300);
      expect(screen.getByText(`Q* ${formatDecimal(model.q_star)}`), name).toBeInTheDocument();
      unmount();
    }
  });

  it('no steps and Q* 0 draw no NaN (fixture zero-depth-leg)', () => {
    const { container } = render(<DepthStairs model={walk('zero-depth-leg')} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(container.querySelectorAll('rect.ds-pi, rect.ds-loss')).toHaveLength(0);
    expect(container.querySelector('.ds-yaxis')).toBeNull();
    expect(container.querySelector('.ds-plot svg.ds-svg')).not.toBeNull(); // the empty plot and its shares axis stay
    expect(container.querySelector('.ds-plot .ds-axis')).not.toBeNull();
    expect(screen.getByText('Q* 0')).toBeInTheDocument();
  });
});

describeWasm('DepthStairs on the real engine', () => {
  it('draws what depth_walk_json returns for each fixture input (its output equals the fixture output)', async () => {
    const engine = await loadTestEngine();
    for (const c of fixtureCases('depth_walk_json')) {
      const raw = engine.depth_walk_json(c.input);
      expect(raw, c.name).toBe(c.output); // the engine's string equals the fixture's, not our own function
      const out = JSON.parse(raw) as WalkModel | { error: string };
      if ('error' in out) continue;
      const { container, unmount } = render(<DepthStairs model={out} />);
      expect(container.innerHTML, c.name).not.toContain('NaN');
      expect(container.querySelectorAll('line.ds-cost'), c.name).toHaveLength(out.steps.length);
      unmount();
    }
  });
});
