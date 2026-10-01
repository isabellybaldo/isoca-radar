import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { formatDecimal, formatMoneyCents } from '../lib/money';
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
const fill = (requested: string, filled: string, vwap: string | null, levels_used = 1): Fill => ({ requested, filled, vwap, fees: '0', levels_used });
/** View models only (a different net cash on a real fixture's shape), to pin the formatting rules. */
const withNet = (m: BasketModel, net_cash: string): BasketModel => ({ ...m, cash: { ...m.cash, net_cash } });
const cashRows = (c: HTMLElement) =>
  [...c.querySelectorAll('.bl-cash > div')].map((d) => [d.querySelector('dt')?.textContent, d.querySelector('dd')?.textContent]);

/** The exact phrase per named fixture (figures come from the fixture; the phrases are typed, per ruling I2). */
const PHRASES: [string, string, 'pos' | 'neg' | null][] = [
  ['no-all-fill', 'filled, +$2.50 on paper', 'pos'],
  ['no-one-leg-empty-burst', 'caught, and lost $6.00 on paper', 'neg'],
  ['no-one-leg-partial', 'caught, +$0.40 on paper', 'pos'],
  ['no-excess-below-min', 'caught, $1.70 out on paper, shares still held', null],
  ['no-unwind-book-empty', 'caught, $65.00 out on paper, shares still held', null],
  ['no-convert-fee-10000', 'filled, and lost $97.50 on paper', 'neg'],
  ['yes-all-fill', 'filled, $45.00 out on paper, shares still held', null],
  ['yes-one-leg-empty', 'caught, and lost $10.00 on paper', 'neg'],
  ['price-0.01', 'caught, +$11.50 on paper', 'pos'],
];

describe('basketVerdict (ruling I2: a word and a money half, no arithmetic)', () => {
  it.each(PHRASES)('%s reads "%s"', (name, big, tone) => {
    expect(basketVerdict(basket(name))).toEqual({ big, tone });
  });

  it('prints $0.00, never "lost $0.00", when the net rounds to nothing, on the decimal string', () => {
    const all = basket('no-all-fill');
    expect(basketVerdict(withNet(all, '-0.001'))).toEqual({ big: 'filled, $0.00 on paper', tone: null });
    expect(basketVerdict(withNet(all, '0'))).toEqual({ big: 'filled, $0.00 on paper', tone: null });
    expect(basketVerdict(withNet(all, '-0.004')).big).not.toContain('lost');
  });

  it('says "nothing filled" when q_eff is 0 and no leg has an excess', () => {
    const all = basket('no-all-fill');
    const zero: BasketModel = { ...all, q_eff: '0', cash: { ...all.cash, net_cash: '0' } };
    expect(basketVerdict(zero)).toEqual({ big: 'nothing filled, $0.00 on paper', tone: null });
  });
});

describe('BasketLegs (spec 7b §6.2 f)', () => {
  it('draws planned vs filled per leg, q_eff as one line inside the plot, the excess sold back (fixture burst)', () => {
    const m = basket('no-one-leg-empty-burst');
    const { container } = render(<BasketLegs model={m} names={NAMES} />);
    const svg = container.querySelector('svg.bl-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg?.querySelector('text')).toBeNull();
    expect(container.querySelectorAll('rect.bl-plan')).toHaveLength(m.legs.length);
    expect(container.querySelectorAll('line.bl-qeff')).toHaveLength(1);
    expect(Number(container.querySelector('line.bl-qeff')?.getAttribute('y1'))).toBeCloseTo(119); // q_eff 0: on the floor, inset so the 2px stroke is not clipped
    const a = container.querySelector('g[data-leg="A"]');
    expect(Number(a?.querySelector('rect.bl-sold')?.getAttribute('height'))).toBeGreaterThan(0);
    expect(Number(a?.querySelector('rect.bl-held')?.getAttribute('height'))).toBe(0);
    expect(Number(container.querySelector('g[data-leg="B"] rect.bl-fill')?.getAttribute('height'))).toBe(0);
    expect(legs()[1]).toHaveTextContent(/^leg B0 of 50/);
    expect(legs()[0]).toHaveTextContent('sold 50 at 0.54');
    expect(screen.getByText('caught, and lost $6.00 on paper')).toHaveClass('bl-big', 'neg');
  });

  it('says why an excess was not sold: under the minimum, no order went out; or no buyer in the book (I1)', () => {
    const below = basket('no-excess-below-min');
    const { container, unmount } = render(<BasketLegs model={below} names={NAMES} />);
    expect(legs()[1]).toHaveTextContent(`${formatDecimal(below.legs[1].unwind.requested)} to sell, under the ${formatDecimal(below.min_order)} minimum, no order went out`);
    expect(legs()[1]).toHaveTextContent('held 3, $1.95 cost, not cash');
    expect(Number(container.querySelector('g[data-leg="B"] rect.bl-held')?.getAttribute('height'))).toBeGreaterThan(0);
    unmount();
    render(<BasketLegs model={basket('no-unwind-book-empty')} names={NAMES} />);
    expect(legs()[0]).toHaveTextContent('50 to sell, no buyer in the book');
    expect(legs()[0]).not.toHaveTextContent('no order went out');
    expect(legs()[0]).toHaveTextContent('held 50, $30.00 cost, not cash');
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

  it('rounds a 28-digit average to "at avg 0.5867" when several levels filled, "at" for one, and keeps tokens short (I4)', () => {
    const burst = basket('no-one-leg-empty-burst');
    const m: BasketModel = {
      ...burst,
      legs: [
        { ...burst.legs[0], unwind: fill('50', '30', '0.5866666666666666666666666667', 2) },
        { ...burst.legs[1], unwind: fill('50', '30', '0.5866666666666666666666666667', 1) },
        burst.legs[2],
        burst.legs[2],
      ],
      cash: { ...burst.cash, held_unsold: [{ qty: '20', basis: '990000' }, ...burst.cash.held_unsold.slice(1), burst.cash.held_unsold[2]] },
    };
    render(<BasketLegs model={m} names={NAMES} />);
    expect(legs()[0]).toHaveTextContent('sold 30 at avg 0.5867');
    expect(legs()[1]).toHaveTextContent('sold 30 at 0.5867');
    expect(legs()[1]).not.toHaveTextContent('avg');
    expect(legs()[0]).toHaveTextContent('held 20, $990,000.00 cost, not cash');
    for (const li of legs()) for (const tok of (li.textContent ?? '').split(/\s+/)) expect(tok.length, tok).toBeLessThanOrEqual(12);
  });

  it('pins the cash rows, label to figure, on the fixture burst and a fees-on case (I5)', () => {
    const money = (v: string, signed = false) => formatMoneyCents(v, { signed });
    for (const name of ['no-one-leg-empty-burst', 'no-fees-on']) {
      const m = basket(name);
      const { container, unmount } = render(<BasketLegs model={m} names={NAMES} />);
      expect(cashRows(container), name).toEqual([
        ['spent', money(m.cash.spent)],
        ['sold back', money(m.cash.unwind_back)],
        ['converted', money(m.cash.convert_back)],
        ['net cash, on paper', money(m.cash.net_cash, true)],
        ['fees, already inside the rows above', money(m.cash.fees)],
      ]);
      unmount();
    }
    expect(money(basket('no-fees-on').cash.fees)).not.toBe('$0.00'); // the fees case really has fees
  });

  it('hides "converted" for a YES basket, and shows value at resolution as "not yet cash"; a NO basket shows none', () => {
    const yes = basket('yes-all-fill');
    expect(yes.value_at_resolution).not.toBeNull();
    const { container, unmount } = render(<BasketLegs model={yes} names={NAMES} />);
    expect(cashRows(container).map((r) => r[0])).not.toContain('converted');
    expect(container.querySelector('.bl-resolve')).toHaveTextContent('not yet cash');
    unmount();
    const no = render(<BasketLegs model={basket('no-all-fill')} names={NAMES} />);
    expect(cashRows(no.container).map((r) => r[0])).toContain('converted');
    expect(no.container.querySelector('.bl-resolve')).toBeNull();
    expect(no.container.textContent).not.toContain('not yet cash');
  });

  it('keeps every dollar inside the figure whose caption says "on paper", and does not caption q_open as Q* (R11, R19)', () => {
    const { container } = render(<BasketLegs model={basket('no-all-fill')} names={NAMES} />);
    expect(container.querySelector('figure figcaption')).toHaveTextContent('on paper, from the books above');
    expect(container.textContent).not.toContain('replayed');
    expect(container.textContent).not.toMatch(/Q\*/);
  });

  it('pins the legend text to its swatch', () => {
    const { container } = render(<BasketLegs model={basket('no-all-fill')} names={NAMES} />);
    const rows = [...container.querySelectorAll('ul.bl-legend li')].map((li) => [li.querySelector('.bl-sw')?.className, li.textContent]);
    expect(rows).toEqual([
      ['bl-sw fill', 'in full baskets'],
      ['bl-sw sold', 'excess sold back at the unwind price'],
      ['bl-sw held', 'excess still held'],
      ['bl-sw plan', 'planned'],
    ]);
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
    expect(screen.getByText('nothing filled, $0.00 on paper')).toBeInTheDocument();
  });
});

describeWasm('BasketLegs on the real engine', () => {
  it('the engine output equals the fixture output string, and each leg label reads its figures', async () => {
    const engine = await loadTestEngine();
    for (const c of fixtureCases('basket_json')) {
      const raw = engine.basket_json(c.input);
      expect(raw, c.name).toBe(c.output);
      const out = JSON.parse(raw) as BasketModel | { error: string };
      if ('error' in out) continue;
      const { container, unmount } = render(<BasketLegs model={out} names={NAMES} />);
      expect(container.innerHTML, c.name).not.toContain('NaN');
      out.legs.forEach((l, i) => {
        expect(legs()[i], c.name).toHaveTextContent(`${formatDecimal(l.buy.filled)} of ${formatDecimal(l.buy.requested)}`);
      });
      unmount();
    }
  });
});
