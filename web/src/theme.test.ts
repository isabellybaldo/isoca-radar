// Structural checks of the readability pass (spec §8.4). jsdom has no layout engine, so the
// 360 px no-horizontal-scroll check itself is a manual step in the runbook; this pins the rules
// that make it true. Vitest runs on Node: node:fs exists at runtime (no @types/node installed).
// @ts-ignore
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const css: string = readFileSync(`${process.cwd()}/src/theme.css`, 'utf8');
const html: string = readFileSync(`${process.cwd()}/index.html`, 'utf8');

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
    expect(html).toContain('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&family=Press+Start+2P&family=VT323&display=swap');
    expect(html).not.toMatch(/<script(?![^>]*type="module")/);
  });

  it('never forces a page wider than 360 px', () => {
    for (const m of css.matchAll(/(?<![-\w])(min-width|width):\s*(\d+)px/g)) {
      expect(Number(m[2]), `${m[0]}`).toBeLessThanOrEqual(328);
    }
    expect(css).not.toMatch(/minmax\(\s*\d+px/);
    expect(rule('.scroll')).toMatch(/overflow-x:\s*auto/);
    expect(rule('body')).toMatch(/overflow-wrap:\s*anywhere/);
    expect(css).toMatch(/\[hidden\]\s*\{\s*display:\s*none\s*!important/);
  });
});
