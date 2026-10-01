// jsdom has no layout: pin the rules that make the section controls touch-sized (09-27 §6.2, constraints: 360 px).
// @ts-ignore
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const css: string = readFileSync(`${process.cwd()}/src/math/math.css`, 'utf8');
const rule = (sel: string): string => {
  const i = css.indexOf(sel);
  expect(i, `no rule for ${sel}`).toBeGreaterThanOrEqual(0);
  return css.slice(i, css.indexOf('}', i));
};

describe('math.css', () => {
  it('gives every control a 44 px tap height', () => {
    for (const sel of [".m-range input[type='range']", '.m-btn', '.m-sec .seg button', '.m-size input, .m-select select', '.m-sec .fold summary', '.m-check']) {
      expect(rule(sel), sel).toMatch(/min-height:\s*44px/);
    }
  });
  it('draws no box inside the section box', () => {
    expect(rule('.m-level, .m-leg')).toMatch(/border:\s*0/);
  });
  it('uses no fixed width above 328 px and no minmax(<N>px', () => {
    for (const m of css.matchAll(/(?<![-\w])(min-width|width):\s*(\d+)px/g)) expect(Number(m[2])).toBeLessThanOrEqual(328);
    expect(css).not.toMatch(/minmax\(\s*\d+px/);
  });
});
