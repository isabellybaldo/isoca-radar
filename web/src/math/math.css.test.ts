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
  it('wraps the live strip and the live ladder heading, so a long line never forces 360 px (R18)', () => {
    expect(rule('.m-live-k, .m-live-n, .m-live-h')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(rule('.m-live {')).toMatch(/min-width:\s*0/);
  });
  it('lets nothing in a section or the reused ladder widen the page: single shrinkable columns (Task 11 handoff)', () => {
    expect(rule('.m-sec {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-liveladder {')).toMatch(/min-width:\s*0/);
    expect(rule('.m-liveladder {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-liveladder .ladder, .m-liveladder .fold-body {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-liveladder .fold, .m-liveladder .scroll {')).toMatch(/min-width:\s*0/);
  });
  it('keeps the reused ladder frameless inside the section box, like the explain panel does', () => {
    expect(rule('.m-sec .ll-svg {')).toMatch(/border:\s*0/);
    expect(rule('.m-sec .fold {')).toMatch(/border:\s*0;\s*border-top:\s*1px dashed var\(--line2\)/);
  });
  it('keeps the fold marker apart from its label and dims a disabled slider once (Task 9 review)', () => {
    expect(rule('.m-sec .fold summary')).toMatch(/(?<![-\w])gap:\s*\d+px/);
    expect(css.match(/opacity:\s*0\.45/g)).toHaveLength(1);
    expect(css).not.toMatch(/\.m-range input:disabled/);
  });
  it('keeps the value beside a slider inside the section: a column as wide as the longest value, "20 ticks"', () => {
    expect(Number(/minmax\(0, 2fr\) (\d+)ch/.exec(rule('.m-range {'))?.[1])).toBeGreaterThanOrEqual('20 ticks'.length);
  });
  it('lays the page out as one column in the dashboard frame, with a link target per section', () => {
    expect(rule('.app.math {')).toMatch(/max-width:\s*960px/);
    expect(rule('.m-sec[id] {')).toMatch(/scroll-margin-top/);
    expect(rule('.m-foot {')).toMatch(/padding:/);
  });
  it('keeps sentences in the body font: no rule for the tagline, the captions or the footer sets a font', () => {
    const rules = [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^}]*)\}/g)].map((m) => ({ sel: m[1].trim(), body: m[2] }));
    for (const cls of ['.m-tag', '.m-foot', '.m-cap']) {
      const mine = rules.filter((r) => r.sel.split(',').some((s) => new RegExp(`\\${cls}(?![\\w-])`).test(s)));
      expect(mine.length, `no rule for ${cls}`).toBeGreaterThan(0);
      for (const r of mine) expect(r.body, `${r.sel}`).not.toMatch(/font-family|--px|--big/);
    }
    expect(rule('.m-top h1 {')).toMatch(/font-family:\s*var\(--px\)/);
  });
});
