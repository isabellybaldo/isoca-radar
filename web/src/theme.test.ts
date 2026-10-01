// Structural checks of the readability pass (spec §8.4). jsdom has no layout engine, so the
// 360 px no-horizontal-scroll check itself is a manual step in the runbook; this pins the rules
// that make it true. Vitest runs on Node: node:fs exists at runtime (no @types/node installed).
// @ts-ignore
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const css: string = readFileSync(`${process.cwd()}/src/theme.css`, 'utf8');
const html: string = readFileSync(`${process.cwd()}/index.html`, 'utf8');
const read = (path: string): string => {
  try {
    return readFileSync(`${process.cwd()}/${path}`, 'utf8');
  } catch {
    return ''; // a missing file fails the test that needs it, with a readable message
  }
};
/** The public page's own layout and its entry (spec 7b §6.1): the same structural rules run over them. */
const mathCss: string = read('src/math/math.css');
const mathHtml: string = read('math.html');
const FONTS = 'https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap';

function rule(selector: string): string {
  const i = css.indexOf(`\n${selector} {`); // at a line start: `html, body {` is not the body rule
  expect(i, `no rule for ${selector}`).toBeGreaterThanOrEqual(0);
  return css.slice(i, css.indexOf('}', i));
}

describe('theme', () => {
  it('sets IBM Plex Mono body text at 15–16 px with line-height ≥ 1.5, VT323 only for big numbers', () => {
    expect(css).toMatch(/--body:\s*'IBM Plex Mono'/);
    expect(css).toMatch(/--big:\s*'VT323'/);
    const body = rule('body');
    expect(body).toMatch(/font-family:\s*var\(--body\)/);
    const size = Number(/font-size:\s*(\d+)px/.exec(body)?.[1]);
    expect(size >= 15 && size <= 16).toBe(true);
    expect(Number(/line-height:\s*([\d.]+)/.exec(body)?.[1])).toBeGreaterThanOrEqual(1.5);
    expect(rule('table')).toMatch(/font-size:\s*15px/);
    expect(css.match(/var\(--big\)/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('loads the three fonts from Google Fonts only (the CSP stays as it is)', () => {
    expect(html).toContain(FONTS);
    expect(html).not.toMatch(/<script(?![^>]*type="module")/);
  });

  it('never forces a page wider than 360 px: the dashboard sheet and the public math sheet', () => {
    for (const [name, sheet] of [['theme.css', css], ['math/math.css', mathCss]] as const) {
      expect(sheet.length, `${name} is missing`).toBeGreaterThan(0);
      for (const m of sheet.matchAll(/(?<![-\w])(min-width|width):\s*(\d+)px/g)) {
        expect(Number(m[2]), `${name}: ${m[0]}`).toBeLessThanOrEqual(328);
      }
      expect(sheet, name).not.toMatch(/minmax\(\s*\d+px/);
    }
    expect(rule('.scroll')).toMatch(/overflow-x:\s*auto/);
    expect(rule('body')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(css).toMatch(/\[hidden\]\s*\{\s*display:\s*none\s*!important/);
  });
  it('fits the Sweet spots grid in 360 px: an 88 px label column and six 36 px cells, no cell padding (spec 7a §8)', () => {
    const px = (sel: string) => Number(/(?<![-\w])width:\s*(\d+)px/.exec(rule(sel))?.[1]);
    expect(px('.lgrid .lg-row')).toBe(88);
    expect(px('.lgrid .lg-cell')).toBe(36);
    expect(px('.lgrid .lg-row') + 6 * px('.lgrid .lg-cell')).toBeLessThanOrEqual(328);
    expect(rule('.lgrid th, .lgrid td')).toMatch(/padding:\s*0/);
    expect(rule('.lgrid')).toMatch(/width:\s*auto/);
    expect(rule('ul.plain.lg-legend')).toMatch(/gap:\s*2px/); // out-specifies ul.plain { gap: 6px } (D-05)
  });
  it('keeps the explain drawer inside the viewport and beside its tab (spec 7a §9.1)', () => {
    expect(rule(".explain[data-mode='drawer']")).toMatch(/width:\s*min\(360px,\s*calc\(100vw - 28px\)\)/);
    expect(rule(".explain[data-mode='dock']")).toMatch(/flex:\s*0 0 360px/);
  });
  it("docks the explain tab on the panel's left edge, in the flow, never over the panel (D-06)", () => {
    const gap = Number(/(?<![-\w])gap:\s*(\d+)px/.exec(rule('.shell'))?.[1]);
    const tab = rule('.explain-tab[data-dock]');
    expect(tab).toMatch(/position:\s*sticky/); // in the flex row between .app and the panel, not fixed over it
    expect(tab).toMatch(/transform:\s*none/);
    expect(Number(/margin-right:\s*-(\d+)px/.exec(tab)?.[1])).toBe(gap); // flush with the panel's left border
    expect(rule('.explain-tab')).toMatch(/border-right:\s*0/);
  });

  it('keeps the mirror axis labels under the plot: same gutter as the rows, labels in the label font, alert token', () => {
    expect(rule('.mb-row')).toMatch(/grid-template-columns:\s*4ch/);
    expect(rule('.mb-axis')).toMatch(/margin-left:\s*calc\(4ch \+ 6px\)/);
    expect(rule('.mb-axis')).not.toMatch(/font-size/); // a font-size here would make 4ch differ from the row's
    expect(rule('ul.plain.mb-lab')).not.toMatch(/--px/);
    expect(rule('.mb-alert')).toMatch(/var\(--bad\)/);
  });

  it("wraps the panel's axis labels between words, never inside one: `extras` stays whole, `in time` cells keep a gap", () => {
    // The body's overflow-wrap: anywhere split `extras` into extra/s and let six `in time` labels touch (7a-B Chrome check).
    for (const sel of ['.ll-axis li', '.cb-axis li', '.mb-axis li', '.ds-axis li', '.bl-axis li', '.ub-axis li']) expect(rule(sel)).toMatch(/overflow-wrap:\s*normal/);
    // 7 slots in a 360 px panel leave ~44 px each: `extras` (6 ch) fits at 12 px (43.2 px), not at 13 px.
    expect(Number(/font-size:\s*(\d+)px/.exec(rule('.ll-axis'))?.[1])).toBeLessThanOrEqual(12);
    // A gap narrower than `in time` forces the two words onto two lines instead of into the next cell.
    expect(Number(/column-gap:\s*(\d+)px/.exec(rule('.cb-axis'))?.[1])).toBeGreaterThanOrEqual(6);
  });

  it('keeps the basket leg labels breakable inside a cell and the planned outline visible (7b-B review I4)', () => {
    expect(rule('.bl-axis li span')).toMatch(/overflow-wrap:\s*anywhere/); // a long figure splits rather than overflow 360 px
    expect(rule('.ub-axis li span')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(rule('.bl-svg .bl-plan')).toMatch(/var\(--dim\)/);
    expect(rule('.ds-svg .ds-loss')).toMatch(/stroke-dasharray/); // the loss area differs from the profit area by more than colour
  });

  it('gives the public page its own entry: same fonts, no inline script or style, nothing of the dashboard (spec 7b §6.1, the math CSP)', () => {
    expect(mathHtml, 'math.html is missing').not.toBe('');
    expect(mathHtml).toMatch(/<html lang="en">/);
    expect(mathHtml).toContain('<title>isoca-radar · math</title>');
    expect(mathHtml).toMatch(/<meta name="description" content="[^"]{50,200}" \/>/);
    expect(mathHtml).toContain('<meta name="viewport" content="width=device-width, initial-scale=1" />');
    expect(mathHtml).not.toMatch(/noindex/); // public, meant to be found (index.html is not)
    // style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com: the dashboard's three links, nothing else remote
    expect(mathHtml).toContain(FONTS);
    expect([...mathHtml.matchAll(/https?:\/\/[^"'\s]+/g)].map((m) => m[0]).filter((u) => !/^https:\/\/fonts\.(googleapis|gstatic)\.com/.test(u))).toEqual([]);
    for (const tag of html.match(/<link [^>]*>/g) ?? []) expect(mathHtml, tag).toContain(tag);
    // script-src 'self' 'wasm-unsafe-eval': one module script with a src, no inline script, style or handler
    expect(mathHtml.match(/<script[^>]*>[^<]*<\/script>/g)).toEqual(['<script type="module" src="/src/math/main.tsx"></script>']);
    expect(mathHtml).not.toMatch(/\sstyle=|<style|\son\w+=/);
    expect(mathHtml).not.toContain('/api/');
    expect(mathHtml).not.toContain('/src/main.tsx');
    expect(mathHtml).toContain('<div id="root"></div>');
  });
});
