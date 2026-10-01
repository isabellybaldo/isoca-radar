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
    for (const sel of [".m-range input[type='range']", '.m-btn', '.m-sec .seg button', '.m-ch .seg button', '.m-size input, .m-select select', '.m-sec .fold summary', '.m-ch .fold summary', '.m-check']) {
      expect(rule(sel), sel).toMatch(/min-height:\s*44px/);
    }
  });
  it("keeps the section titles' pixel look now that they are h3s (theme.css styles only .box h2), and the chapter line tight under them", () => {
    const h = rule('.m-sec > h3 {');
    expect(h).toMatch(/font-family:\s*var\(--px\)/);
    expect(h).toMatch(/font-size:\s*9px/);
    expect(h).toMatch(/margin:\s*0 0 8px/);
    expect(h).toMatch(/line-height:\s*1\.5/);
    expect(rule('.m-back {')).toMatch(/margin:\s*0;\s*font-size:\s*13px/);
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
  it('lets nothing in a section, a chapter, the playground or the reused ladder widen the page: single shrinkable columns (Task 11 handoff)', () => {
    expect(rule('.m-sec {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    const story = rule('.m-ch, .m-play {');
    expect(story).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(story).toMatch(/min-width:\s*0/);
    expect(rule('.m-ch > *, .m-play > * {')).toMatch(/min-width:\s*0/); // a wide child (a table, a figure) shrinks with its column
    expect(rule('.m-liveladder {')).toMatch(/min-width:\s*0/);
    expect(rule('.m-liveladder {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-liveladder .ladder, .m-liveladder .fold-body {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-liveladder .fold, .m-liveladder .scroll {')).toMatch(/min-width:\s*0/);
  });
  it('keeps the reused ladder frameless inside chapter 7 and every fold frameless in a chapter or a section, like the explain panel does', () => {
    expect(rule('.m-ch .ll-svg {')).toMatch(/border:\s*0/);
    expect(css).not.toMatch(/\.m-sec \.ll-svg/); // no ladder is left in a section
    expect(rule('.m-sec .fold {')).toMatch(/border:\s*0;\s*border-top:\s*1px dashed var\(--line2\)/);
    expect(rule('.m-ch .fold, .m-sec .fold {')).toMatch(/border:\s*0;\s*border-top:\s*1px dashed var\(--line2\)/);
    // the ladder was "under f's caption, behind a dashed rule": under a chapter's lead that rule would double the chapter's own
    expect(rule('.m-liveladder {')).not.toMatch(/border|padding|margin/);
  });
  it('the story: a chapter switch wraps to two rows when its labels are long (review focus 5), and its buttons stay in the chip font', () => {
    expect(rule('.m-ch .seg {')).toMatch(/flex-wrap:\s*wrap/);
    expect(rule('.m-ch .seg button')).toMatch(/min-height:\s*44px/);
    expect(rule('.m-ch .seg button')).not.toMatch(/font-family|white-space|(?<![-\w])width/);
  });
  it('the story: chapters are not boxes; a rule above each, a body-font question with its number in the big font, prose that wraps', () => {
    // '\n': the rule that starts a line, not the tail of ".m-ch, .m-play {"
    expect(rule('\n.m-ch {')).toMatch(/border-top:\s*2px solid var\(--line2\)/);
    expect(rule('\n.m-play {')).toMatch(/border-top:\s*3px solid var\(--line\)/); // the story ends, the playground starts
    for (const sel of ['.m-ch, .m-play {', '\n.m-ch {', '\n.m-play {']) expect(rule(sel), sel).not.toMatch(/background|box-shadow|border:/);
    // with no card behind them, a chapter's plots take the card colour (out-specifying theme.css's .dbars .db-svg)
    expect(rule('section.m-ch :is(.mb-svg, .ds-svg, .bl-svg, .ub-svg, .db-svg, .ll-svg) {')).toMatch(/background:\s*var\(--card\)/);
    const h = rule('.m-ch > h2, .m-play > h2 {');
    expect(h).toMatch(/font-size:\s*20px/);
    expect(h).toMatch(/overflow-wrap:\s*anywhere/);
    expect(h).not.toMatch(/font-family|--px/); // the question is a sentence: the body font, not the 9 px pixel title of .box h2
    expect(rule('.m-ch-n {')).toMatch(/font-family:\s*var\(--big\)/);
    const prose = rule('.m-ch-lead, .m-ch-take {');
    expect(prose).toMatch(/max-width:\s*78ch/);
    expect(prose).toMatch(/overflow-wrap:\s*anywhere/);
    expect(prose).not.toMatch(/font-family/);
    expect(rule('\n.m-ch-take {')).toMatch(/border-left:\s*3px solid var\(--teal\)/);
  });
  it('the word list: a shrinkable term column beside its meaning, both wrapping', () => {
    expect(rule('.m-word {')).toMatch(/grid-template-columns:\s*minmax\(0, \d+ch\) minmax\(0, 1fr\)/);
    expect(rule('.m-word dt {')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(rule('.m-word dd {')).toMatch(/overflow-wrap:\s*anywhere/);
  });
  it("keeps a rung's name on one line in the live ladder's numbers table (it broke mid-word at 360 px); the table scrolls in its own wrapper", () => {
    const first = rule('.m-liveladder .scroll th:first-child, .m-liveladder .scroll td:first-child {');
    expect(first).toMatch(/white-space:\s*nowrap/);
    expect(first).toMatch(/overflow-wrap:\s*normal/);
  });
  it('keeps the fold marker apart from its label and dims a disabled slider once (Task 9 review)', () => {
    expect(rule('.m-sec .fold summary')).toMatch(/(?<![-\w])gap:\s*\d+px/);
    expect(css.match(/opacity:\s*0\.45/g)).toHaveLength(1);
    expect(css).not.toMatch(/\.m-range input:disabled/);
  });
  it('keeps the value beside a slider inside the section: a column as wide as the longest value, "20 ticks"', () => {
    expect(Number(/minmax\(0, 2fr\) (\d+)ch/.exec(rule('.m-range {'))?.[1])).toBeGreaterThanOrEqual('20 ticks'.length);
  });
  it('lays the page out as one column in the dashboard frame, with a link target per section, per chapter and for the playground', () => {
    expect(rule('.app.math {')).toMatch(/max-width:\s*960px/);
    expect(rule('.m-sec[id] {')).toMatch(/scroll-margin-top/);
    expect(rule('.m-ch[id]')).toMatch(/scroll-margin-top/);
    expect(rule('.m-play[id]')).toMatch(/scroll-margin-top/);
    expect(rule('.m-body {')).toMatch(/grid-template-columns:\s*minmax\(0, 1fr\)/);
    expect(rule('.m-foot {')).toMatch(/padding:/);
  });
  it('keeps sentences in the body font: no rule for the tagline, the captions or the footer sets a font', () => {
    const rules = [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^}]*)\}/g)].map((m) => ({ sel: m[1].trim(), body: m[2] }));
    for (const cls of ['.m-tag', '.m-foot', '.m-cap', '.m-ch-lead', '.m-ch-take', '.m-word', '.m-words', '.m-back']) {
      const mine = rules.filter((r) => r.sel.split(',').some((s) => new RegExp(`\\${cls}(?![\\w-])`).test(s)));
      expect(mine.length, `no rule for ${cls}`).toBeGreaterThan(0);
      for (const r of mine) expect(r.body, `${r.sel}`).not.toMatch(/font-family|--px|--big/);
    }
    expect(rule('.m-top h1 {')).toMatch(/font-family:\s*var\(--px\)/);
  });
});
