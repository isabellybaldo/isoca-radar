// The zoomed axes (7b-B Task 16). The inputs here are prices and costs on the page's grid, chosen to exercise
// the helpers; the engine's own figures for the page's inputs are checked where the drawings are rendered.
import { describe, expect, it } from 'vitest';
import { FED_ASKS, FED_BIDS, fromTick, toTick } from '../math/defaults';
import { type TickDomain, along, costDomain, mirrorDomain, stackSpans, tickText } from './geo';

/** The prices section a draws for a YES book: its own levels and their NO mirrors (1 − p). */
const drawn = (yes: string[]) => [...yes, ...yes.map((p) => fromTick(100 - toTick(p)))];
const FED = [...FED_BIDS, ...FED_ASKS].map((l) => l.p);
const holds = (d: TickDomain, prices: string[]) => prices.every((p) => Number(p) * 100 >= d.lo && Number(p) * 100 <= d.hi);

describe('tickText', () => {
  it('writes whole ticks as a decimal string, no float text', () => {
    expect([0, 5, 40, 50, 100, 109, 250].map(tickText)).toEqual(['0.00', '0.05', '0.40', '0.50', '1.00', '1.09', '2.50']);
  });
});

describe('stackSpans (the price stack)', () => {
  it('lays prices end to end in ticks, and ends the axis a tenth of a dollar past the dollar line', () => {
    expect(stackSpans(['0.30', '0.32', '0.35'])).toEqual({ spans: [{ from: 0, to: 30 }, { from: 30, to: 62 }, { from: 62, to: 97 }], end: 97, hi: 110 });
    expect(stackSpans(['0.33', '0.33', '0.34'])).toMatchObject({ end: 100, hi: 110 }); // exactly on the line: no float noise
    expect(stackSpans(['0.1', '0.2', '0.7']).end).toBe(100);
  });

  it('past the dollar line the axis follows the stack, so the overshoot stays inside the box', () => {
    const s = stackSpans(['0.40', '0.45', '0.50']);
    expect(s.end).toBe(135);
    expect(s.hi).toBeGreaterThan(s.end);
    expect(s.hi).toBeCloseTo(148.5);
  });

  it('a price that cannot be placed takes no width: malformed, empty, negative, too large', () => {
    for (const bad of ['', ' ', 'abc', 'NaN', 'Infinity', '-0.20', '1e305', '1e999']) {
      const s = stackSpans(['0.30', bad, '0.35']);
      expect(s, JSON.stringify(bad)).toEqual({ spans: [{ from: 0, to: 30 }, { from: 30, to: 30 }, { from: 30, to: 65 }], end: 65, hi: 110 });
    }
    expect(stackSpans([])).toEqual({ spans: [], end: 0, hi: 110 });
    expect(stackSpans(['abc'])).toEqual({ spans: [{ from: 0, to: 0 }], end: 0, hi: 110 });
  });
});

describe('mirrorDomain (drawing a)', () => {
  it('zooms onto the default book: 0.42 to 0.58 padded by two ticks, 0.50 in the middle', () => {
    const d = mirrorDomain(drawn(FED));
    expect(d).toEqual({ lo: 40, hi: 60 });
    expect(holds(d, drawn(FED))).toBe(true);
  });

  it('follows a level dragged to 0.05', () => {
    const prices = drawn([...FED.slice(1), '0.05']);
    const d = mirrorDomain(prices);
    expect(d).toEqual({ lo: 3, hi: 97 });
    expect(holds(d, prices)).toBe(true);
  });

  it('never gets narrower than ten ticks: one level only, or none', () => {
    expect(mirrorDomain(drawn(['0.50']))).toEqual({ lo: 45, hi: 55 });
    expect(mirrorDomain(drawn(['0.51']))).toEqual({ lo: 45, hi: 55 });
    expect(mirrorDomain(drawn(['0.54']))).toEqual({ lo: 44, hi: 56 });
    expect(mirrorDomain([])).toEqual({ lo: 45, hi: 55 });
  });

  it('holds a crossed book (the best bid one tick above the best ask)', () => {
    const prices = drawn(['0.56', '0.53', '0.52', '0.51', ...FED_ASKS.map((l) => l.p)]);
    const d = mirrorDomain(prices);
    expect(d).toEqual({ lo: 40, hi: 60 });
    expect(holds(d, prices)).toBe(true);
  });

  it('stays inside 0 to 1 at the edges of the grid', () => {
    expect(mirrorDomain(drawn(['0.01']))).toEqual({ lo: 0, hi: 100 });
    expect(mirrorDomain(drawn(['0.99', '0.60']))).toEqual({ lo: 0, hi: 100 });
  });

  it('always contains every drawn price and 0.50, symmetric around 0.50, at least ten ticks wide', () => {
    for (let t = 1; t <= 99; t++) {
      const prices = drawn([fromTick(t)]);
      const d = mirrorDomain(prices);
      expect(holds(d, prices), `${t}`).toBe(true);
      expect(d.lo + d.hi, `${t}`).toBe(100);
      expect(d.lo, `${t}`).toBeLessThan(50);
      expect(d.hi - d.lo, `${t}`).toBeGreaterThanOrEqual(10);
      expect(d.lo, `${t}`).toBeGreaterThanOrEqual(0);
      expect(Number.isInteger(d.lo) && Number.isInteger(d.hi), `${t}`).toBe(true);
    }
  });

  it('a price off the 0.01 grid is still inside', () => {
    const d = mirrorDomain(['0.545', '0.455']);
    expect(d.lo).toBeLessThanOrEqual(45);
    expect(d.hi).toBeGreaterThanOrEqual(55);
  });
});

describe('costDomain (drawing d)', () => {
  const inside = (d: TickDomain, vs: string[]) => vs.every((v) => Number(v) * 100 > d.lo && Number(v) * 100 < d.hi);

  it('zooms onto the values: both ends are whole ticks strictly outside the lowest and the highest', () => {
    const vs = ['0.40', '0.47', '0.52', '0.6'];
    const d = costDomain(vs);
    expect(d).toEqual({ lo: 38, hi: 62 });
    expect(inside(d, vs)).toBe(true);
  });

  it('one step whose cost equals its payoff still has a height: at least four ticks', () => {
    expect(costDomain(['1.00', '1'])).toEqual({ lo: 98, hi: 102 });
  });

  it('one step only: its cost and its payoff, with room either side', () => {
    const d = costDomain(['0.70', '0.75']);
    expect(d).toEqual({ lo: 69, hi: 76 });
  });

  it('costs off the grid (taker fees) are inside, the ends stay whole ticks', () => {
    const vs = ['0.4123', '0.5871', '0.5'];
    const d = costDomain(vs);
    expect(Number.isInteger(d.lo) && Number.isInteger(d.hi)).toBe(true);
    expect(inside(d, vs)).toBe(true);
  });

  it('pads a wide range in proportion (the nested NO rule pays 1 and 2)', () => {
    const d = costDomain(['1', '2', '1.90']);
    expect(d).toEqual({ lo: 92, hi: 208 });
  });

  it('a malformed or overflowing string is skipped, it does not un-zoom the axis; a real 0 still counts (review 1, minors 1 and 2)', () => {
    const zoomed = costDomain(['0.70', '0.75']);
    for (const bad of ['', ' ', 'abc', 'NaN', 'Infinity', '1e305', '1e999']) {
      const d = costDomain(['0.70', bad, '0.75']);
      expect(d, JSON.stringify(bad)).toEqual(zoomed);
      expect(Number.isFinite(along(d, bad)), JSON.stringify(bad)).toBe(true);
    }
    expect(costDomain(['abc', '1e305'])).toEqual({ lo: 0, hi: 100 }); // nothing left to draw
    expect(costDomain(['0', '0.75']).lo).toBe(0);
    expect(costDomain(['0.00', '0.75']).hi).toBeGreaterThan(75);
    expect(mirrorDomain(['0.54', 'abc', '1e305', '0.46'])).toEqual(mirrorDomain(['0.54', '0.46']));
    for (const d of [costDomain(['1e305']), mirrorDomain(['1e305'])]) {
      expect(Number.isInteger(d.lo) && Number.isInteger(d.hi)).toBe(true);
      expect(`${tickText(d.lo)} ${tickText(d.hi)}`).not.toMatch(/NaN|Infinity/);
    }
  });

  it('never goes below zero, and nothing to draw gives 0 to 1', () => {
    expect(costDomain(['0.01', '0.02']).lo).toBe(0);
    expect(costDomain(['0', '0']).hi - costDomain(['0', '0']).lo).toBeGreaterThanOrEqual(4);
    expect(costDomain([])).toEqual({ lo: 0, hi: 100 });
  });
});
