// The story's chapter 3 drawing (spec 2026-10-01, task 7). The prices here are the story's own presets and a few
// chosen to exercise the edges; the component shows them as given and computes no money.
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { PriceStack } from './PriceStack';

const legs = (...prices: string[]) => prices.map((price, i) => ({ name: 'ABCDEF'[i], price }));
const num = (el: Element | null | undefined, a: string) => Number(el?.getAttribute(a));
const segs = (c: HTMLElement) => [...c.querySelectorAll('rect.ps-leg')];
const right = (el: Element | null | undefined) => num(el, 'x') + num(el, 'width');
const dollarX = (c: HTMLElement) => num(c.querySelector('line.ps-dollar'), 'x1');
const viewW = (c: HTMLElement) => Number(c.querySelector('svg.ps-svg')?.getAttribute('viewBox')?.split(' ')[2]);
/** Every attribute of every element drawn, to look for a NaN or an Infinity anywhere. */
const attrs = (c: HTMLElement) => [...c.querySelectorAll('*')].flatMap((el) => [...el.attributes].map((a) => `${a.name}=${a.value}`)).join(' ');
/** The label row's columns as fractions of the width, from the cols' percentages. */
const cols = (c: HTMLElement) => [...c.querySelectorAll('table.ps-labels col')].map((col) => Number((col.getAttribute('width') ?? '').replace('%', '')) / 100);
const span = (td: Element) => Number(td.getAttribute('colspan') ?? '1');

describe('PriceStack (the story, chapter 3)', () => {
  it('lays three prices end to end and leaves a gap up to the dollar line when they add up to less', () => {
    const { container } = render(<PriceStack legs={legs('0.30', '0.32', '0.35')} />);
    const svg = container.querySelector('svg.ps-svg');
    expect(svg).toHaveAttribute('preserveAspectRatio', 'none');
    expect(svg).toHaveAttribute('role', 'img');
    expect(svg?.getAttribute('aria-label')).not.toMatch(/\d|\$/);
    expect(svg?.querySelector('text')).toBeNull();
    const s = segs(container);
    expect(s).toHaveLength(3);
    // widths in the prices' proportion, each starting where the one before ends
    expect(num(s[0], 'x')).toBe(0);
    expect(num(s[1], 'width') / num(s[0], 'width')).toBeCloseTo(32 / 30);
    expect(num(s[2], 'width') / num(s[0], 'width')).toBeCloseTo(35 / 30);
    expect(num(s[1], 'x')).toBeCloseTo(right(s[0]));
    expect(num(s[2], 'x')).toBeCloseTo(right(s[1]));
    // the gap: its own rect, from the end of the last segment to the dollar line
    const gap = container.querySelectorAll('rect.ps-gap');
    expect(gap).toHaveLength(1);
    expect(num(gap[0], 'x')).toBeCloseTo(right(s[2]));
    expect(right(gap[0])).toBeCloseTo(dollarX(container));
    expect(num(gap[0], 'width') / num(s[0], 'width')).toBeCloseTo(3 / 30);
    expect(container.querySelector('rect.ps-over')).toBeNull();
    // the gap is narrow (three ticks): a bracket above the bar spans exactly it
    const mark = container.querySelectorAll('path.ps-mark');
    expect(mark).toHaveLength(1);
    const d = /^M([\d.]+) ([\d.]+)V([\d.]+)H([\d.]+)V([\d.]+)$/.exec(mark[0].getAttribute('d') ?? '');
    expect(d, mark[0].getAttribute('d') ?? 'no d').not.toBeNull();
    const [x0, yFoot, yTop, x1, yFoot2] = (d as RegExpExecArray).slice(1).map(Number);
    expect(x0).toBeCloseTo(num(gap[0], 'x'));
    expect(x1).toBeCloseTo(right(gap[0]));
    expect(yFoot2).toBe(yFoot);
    expect(yTop).toBeLessThan(yFoot); // open towards the bar,
    expect(yFoot).toBeLessThanOrEqual(num(gap[0], 'y')); // in the headroom above it
    expect(yTop).toBeGreaterThan(0);
    // the dollar line is inside the frame, not on it
    expect(dollarX(container)).toBeGreaterThan(0);
    expect(dollarX(container)).toBeLessThan(viewW(container));
    expect(screen.getByText('the gap')).toBeInTheDocument();
    expect(screen.getByText('what the basket costs')).toBeInTheDocument();
  });

  it('draws no gap when the prices add up to exactly one dollar: the stack ends on the line', () => {
    const { container } = render(<PriceStack legs={legs('0.33', '0.33', '0.34')} />);
    const s = segs(container);
    expect(s).toHaveLength(3);
    expect(container.querySelector('rect.ps-gap')).toBeNull();
    expect(container.querySelector('rect.ps-over')).toBeNull();
    expect(right(s[2])).toBeCloseTo(dollarX(container));
    expect(container.querySelector('path.ps-mark')).toBeNull(); // no gap, no mark
    expect(screen.queryByText('the gap')).toBeNull();
    expect(screen.getByText('what the basket costs')).toBeInTheDocument();
  });

  it('past one dollar: an overshoot rect from the line to the end of the stack, no gap, nothing outside the box', () => {
    const { container } = render(<PriceStack legs={legs('0.40', '0.45', '0.50')} />);
    const s = segs(container);
    expect(container.querySelector('rect.ps-gap')).toBeNull();
    const over = container.querySelectorAll('rect.ps-over');
    expect(over).toHaveLength(1);
    expect(num(over[0], 'x')).toBeCloseTo(dollarX(container));
    expect(right(over[0])).toBeCloseTo(right(s[2]));
    expect(right(s[2])).toBeLessThan(viewW(container));
    expect(num(s[1], 'width') / num(s[0], 'width')).toBeCloseTo(45 / 40);
    expect(container.querySelector('path.ps-mark')).toBeNull();
    expect(screen.queryByText('the gap')).toBeNull();
  });

  it('labels each leg in HTML under its own segment, with the price string as given, and the line as "one dollar"', () => {
    const { container } = render(<PriceStack legs={legs('0.30', '0.320', '.35')} />);
    const cells = [...container.querySelectorAll('table.ps-labels td.ps-leg-l')];
    expect(cells.map((td) => td.textContent)).toEqual(['A0.30', 'B0.320', 'C.35']); // never reformatted
    expect(container.querySelector('table.ps-labels')).toHaveAttribute('role', 'presentation');
    // the label row mirrors the drawing: its columns are the segments' shares of the width
    const s = segs(container);
    const w = cols(container);
    expect(w.reduce((a, b) => a + b, 0)).toBeCloseTo(1, 3);
    let at = 0;
    cells.forEach((td, i) => {
      const mine = w.slice(at, at + span(td)).reduce((a, b) => a + b, 0);
      expect(mine, td.textContent ?? '').toBeCloseTo(num(s[i], 'width') / viewW(container), 3);
      at += span(td);
    });
    // "one dollar" ends on the line
    const one = container.querySelector('td.ps-one');
    expect(one).toHaveTextContent(/^one dollar$/);
    expect(w.slice(0, span(one as Element)).reduce((a, b) => a + b, 0)).toBeCloseTo(dollarX(container) / viewW(container), 3);
    // no money computed by the page: no $, and no figure but the prices given
    const text = container.textContent ?? '';
    expect(text).not.toContain('$');
    expect(text.match(/\d[\d.]*|\.\d+/g)).toEqual(['0.30', '0.320', '.35']);
    expect(container.querySelectorAll('[style]')).toHaveLength(0);
  });

  it('a label splits at the dollar line when its segment crosses it, and still spans its own segment', () => {
    const { container } = render(<PriceStack legs={legs('0.60', '0.70')} />);
    const cells = [...container.querySelectorAll('td.ps-leg-l')];
    expect(cells.map(span)).toEqual([1, 2]);
    const w = cols(container);
    expect(w[1] + w[2]).toBeCloseTo(num(segs(container)[1], 'width') / viewW(container), 3);
    expect(w[0] + w[1]).toBeCloseTo(dollarX(container) / viewW(container), 3);
  });

  it.each([
    ['no leg', legs(), null],
    ['a malformed price', legs('0.30', 'abc', '0.35'), 'not drawn: B abc'],
    ['an empty price', legs('', '0.50'), 'not drawn: A'],
    ['a negative price', legs('-0.20', '0.50'), 'not drawn: A -0.20'],
    ['a price too large to place', legs('0.30', '1e305'), 'not drawn: B 1e305'],
    ['a price past any axis', legs('1e999', '0.10'), 'not drawn: A 1e999'],
    ['only zero', legs('0', '0.00'), 'not drawn: A 0 · B 0.00'],
    ['two finite prices whose sum is not', legs('1e300', '1e300'), 'not drawn: A 1e300 · B 1e300'],
    ['a price that would shrink the dollar to nothing', legs('0.30', '20000', '0.35'), 'not drawn: B 20000'],
    ['a price too small to have a width', legs('0.50', '0.0000001'), 'not drawn: B 0.0000001'],
  ])('%s draws without NaN, inside the box, and names what it left out', (_name, given, off) => {
    const { container } = render(<PriceStack legs={given} />);
    expect(attrs(container)).not.toMatch(/NaN|Infinity/);
    expect(container.textContent).not.toMatch(/NaN|Infinity/);
    const w = viewW(container);
    for (const r of container.querySelectorAll('svg.ps-svg rect')) {
      expect(num(r, 'x')).toBeGreaterThanOrEqual(0);
      expect(num(r, 'width')).toBeGreaterThanOrEqual(0);
      expect(right(r)).toBeLessThanOrEqual(w);
    }
    expect(dollarX(container)).toBeGreaterThan(0);
    expect(dollarX(container)).toBeLessThan(w);
    for (const c of cols(container)) expect(c).toBeGreaterThan(0);
    expect(cols(container).reduce((a, b) => a + b, 0)).toBeCloseTo(1, 3);
    // every leg is still named with the string given: under its segment, or in the line for what takes no width
    expect(container.querySelector('.ps-off')?.textContent?.trim() ?? null).toBe(off);
    const left = given.filter((l) => !(off ?? '').replace('not drawn: ', '').split(' · ').some((x) => x.split(' ')[0] === l.name));
    expect([...container.querySelectorAll('td.ps-leg-l')].map((td) => td.textContent)).toEqual(left.map((l) => `${l.name}${l.price}`));
    expect(segs(container)).toHaveLength(left.length);
  });

  it('a price that takes no width is named in its own line, not squeezed into the row', () => {
    const { container } = render(<PriceStack legs={legs('0.30', 'abc', '0.35')} />);
    expect(segs(container)).toHaveLength(2);
    expect([...container.querySelectorAll('td.ps-leg-l')].map((td) => td.textContent)).toEqual(['A0.30', 'C0.35']);
    expect(container.querySelector('.ps-off')).toHaveTextContent('not drawn: B abc');
  });
});
