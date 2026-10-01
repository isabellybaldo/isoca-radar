import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { signOf } from '../lib/money';
import { describeWasm, fixtureCase, fixtureCases, loadTestEngine } from '../wasm/testEngine';
import type { BasketModel, Fill } from '../wasm/types';
import { BasketLegs, basketVerdict } from './BasketLegs';

/** A real basket_json fixture: the engine's own output for its input (never retyped). */
const basket = (name: string) => JSON.parse(fixtureCase('basket_json', name).output) as BasketModel;
const okCases = (): { name: string; model: BasketModel }[] =>
  fixtureCases('basket_json').flatMap((c) => {
    const o = JSON.parse(c.output) as BasketModel | { error: string };
    return 'error' in o ? [] : [{ name: c.name, model: o }];
  });
const NAMES = ['A', 'B', 'C', 'D'];
const legs = () => within(screen.getByRole('list', { name: 'legs' })).getAllByRole('listitem');
const fill = (requested: string, filled: string, vwap: string | null): Fill => ({ requested, filled, vwap, fees: '0', levels_used: 1 });
/** View models only (a different net cash on a real fixture's shape), to pin the formatting and the sign rule. */
const withNet = (m: BasketModel, net_cash: string): BasketModel => ({ ...m, cash: { ...m.cash, net_cash } });

describe('basketVerdict (spec 7b §6.2 f, 7b-A contract)', () => {
  it('says "caught, and lost $x" when net cash is below zero, "filled, $x on paper" otherwise', () => {
    const all = basket('no-all-fill');
    const burst = basket('no-one-leg-empty-burst');
    expect(signOf(all.cash.net_cash)).toBe(1);
    expect(signOf(burst.cash.net_cash)).toBe(-1);
    expect(basketVerdict(all)).toEqual({ big: 'filled, +$2.50 on paper', tone: 'pos' });
    expect(basketVerdict(burst)).toEqual({ big: 'caught, and lost $6.00', tone: 'neg' });
    expect(basketVerdict(withNet(burst, '0'))).toEqual({ big: 'filled, $0.00 on paper', tone: null });
  });

  it('decides the sign on the decimal string, not a float', () => {
    const all = basket('no-all-fill');
    expect(basketVerdict(withNet(all, '-0.001'))).toEqual({ big: 'caught, and lost $0.00', tone: 'neg' });
    expect(basketVerdict(withNet(all, '0.000'))).toEqual({ big: 'filled, $0.00 on paper', tone: null });
  });

  it('every fixture case gets the phrase its net-cash sign calls for', () => {
    for (const { name, model } of okCases()) {
      const v = basketVerdict(model);
      if (signOf(model.cash.net_cash) < 0) expect(v.big, name).toMatch(/^caught, and lost \$/);
      else expect(v.big, name).toMatch(/^filled, .*\$.* on paper$/);
    }
  });
});

describe('BasketLegs (spec 7b §6.2 f)', () => {
  it('draws planned vs filled per leg, q_eff as one line, the excess sold back (fixture burst)', () => {
    const m = basket('no-one-leg-empty-burst');
    const { container } = render(<BasketLegs model={m} names={NAMES} />);
    const svg = container.querySelector('svg.bl-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.bl-plan')).toHaveLength(m.legs.length);
    expect(container.querySelectorAll('line.bl-qeff')).toHaveLength(1);
    expect(Number(container.querySelector('line.bl-qeff')?.getAttribute('y1'))).toBeCloseTo(120); // q_eff 0 sits on the floor
    const a = container.querySelector('g[data-leg="A"]');
    expect(Number(a?.querySelector('rect.bl-sold')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(Number(a?.querySelector('rect.bl-held')?.getAttribute('height'))).toBe(0);
    expect(Number(container.querySelector('g[data-leg="B"] rect.bl-fill')?.getAttribute('height'))).toBe(0);
    expect(legs()[1]).toHaveTextContent(/^leg B0 of 50/);
    expect(legs()[0]).toHaveTextContent('sold 50 at 0.54');
    expect(screen.getByText('caught, and lost $6.00')).toHaveClass('bl-big', 'neg');
  });

  it('shows the excess held, not cash, when it could not be sold (fixtures below-min and unwind-empty)', () => {
    const { container, unmount } = render(<BasketLegs model={basket('no-excess-below-min')} names={NAMES} />);
    expect(legs()[1]).toHaveTextContent('3 to sell, no order went out');
    expect(legs()[1]).toHaveTextContent('held 3, $1.95 cost, not cash');
    expect(Number(container.querySelector('g[data-leg="B"] rect.bl-held')?.getAttribute('height'))).toBeGreaterThan(0);
    unmount();
    render(<BasketLegs model={basket('no-unwind-book-empty')} names={NAMES} />);
    expect(legs()[0]).toHaveTextContent('held 50, $30.00 cost, not cash');
    expect(screen.getByText('caught, and lost $65.00')).toBeInTheDocument();
  });

  it('splits one leg excess into the part sold and the part still held', () => {
    const burst = basket('no-one-leg-empty-burst');
    const m: BasketModel = {
      ...burst,
      legs: [{ ...burst.legs[0], unwind: fill('50', '45', '0.54') }, ...burst.legs.slice(1)],
      cash: { ...burst.cash, held_unsold: [{ qty: '5', basis: '3' }, ...burst.cash.held_unsold.slice(1)] },
    };
    const { container } = render(<BasketLegs model={m} names={NAMES} />);
    const a = container.querySelector('g[data-leg="A"]');
    expect(Number(a?.querySelector('rect.bl-sold')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(Number(a?.querySelector('rect.bl-held')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(legs()[0]).toHaveTextContent('sold 45 at 0.54');
    expect(legs()[0]).toHaveTextContent('held 5, $3.00 cost, not cash');
  });

  it('frames every dollar: net cash "on paper", fees "of which fees" after the total they sit in (R11)', () => {
    const { container } = render(<BasketLegs model={basket('no-fees-on')} names={NAMES} />);
    const dts = [...container.querySelectorAll('.bl-cash dt')].map((d) => d.textContent);
    expect(dts).toContain('net cash, on paper');
    expect(dts).toContain('of which fees');
    expect(dts).not.toContain('fees');
    expect(dts.indexOf('of which fees')).toBeGreaterThan(dts.indexOf('net cash, on paper'));
    expect(container.querySelector('.bl-big')).toHaveTextContent('on paper');
  });

  it('a YES basket shows its value at resolution as "not yet cash"; a NO basket shows none', () => {
    const yes = basket('yes-all-fill');
    expect(yes.value_at_resolution).not.toBeNull();
    const { container, unmount } = render(<BasketLegs model={yes} names={NAMES} />);
    expect(container.querySelector('.bl-resolve')).toHaveTextContent('not yet cash');
    unmount();
    const no = render(<BasketLegs model={basket('no-all-fill')} names={NAMES} />);
    expect(no.container.querySelector('.bl-resolve')).toBeNull();
    expect(no.container.textContent).not.toContain('not yet cash');
  });

  it('does not caption q_open as Q* (R19)', () => {
    const { container } = render(<BasketLegs model={basket('no-all-fill')} names={NAMES} />);
    expect(container.textContent).not.toMatch(/Q\*/);
  });

  it('every fixture case draws without NaN, with one planned outline per leg', () => {
    for (const { name, model } of okCases()) {
      const { container, unmount } = render(<BasketLegs model={model} names={NAMES} />);
      expect(container.innerHTML, name).not.toContain('NaN');
      expect(container.querySelectorAll('rect.bl-plan'), name).toHaveLength(model.legs.length);
      unmount();
    }
  });

  it('nothing filled draws no NaN (the zero view model)', () => {
    const all = basket('no-all-fill');
    const zero: BasketModel = {
      ...all, q_target: '0', q_eff: '0',
      legs: all.legs.map((l) => ({ ...l, buy: fill('0', '0', null), excess: '0', unwind: fill('0', '0', null) })),
      cash: { ...all.cash, spent: '0', convert_back: '0', net_cash: '0' },
    };
    const { container } = render(<BasketLegs model={zero} names={NAMES} />);
    expect(container.innerHTML).not.toContain('NaN');
    expect(screen.getByText('filled, $0.00 on paper')).toBeInTheDocument();
  });
});

describeWasm('BasketLegs on the real engine', () => {
  it('draws what basket_json returns for each fixture input, and its verdict matches the sign', async () => {
    const engine = await loadTestEngine();
    for (const c of fixtureCases('basket_json')) {
      const out = JSON.parse(engine.basket_json(c.input)) as BasketModel | { error: string };
      if ('error' in out) continue;
      const { container, unmount } = render(<BasketLegs model={out} names={NAMES} />);
      expect(container.innerHTML, c.name).not.toContain('NaN');
      expect(container.querySelector('.bl-big')?.textContent, c.name).toBe(basketVerdict(out).big);
      unmount();
    }
  });
});
