// Frozen-site plan, task 4: the portfolio build's config and script. Plain text checks (like deploy.test.ts).
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const web = (p: string): string => (readFileSync(`${process.cwd()}/${p}`, 'utf8') as string).replace(/\r\n/g, '\n');

const BUILD =
  'tsc --noEmit && tsc --noEmit -p tsconfig.math.json && vite build -c vite.portfolio.config.ts --mode frozen && vite build -c vite.math.config.ts --mode frozen --outDir dist-portfolio && node scripts/finish-portfolio.mjs';

describe('the portfolio build', () => {
  it('vite.portfolio.config.ts puts both pages and their assets under site-assets, with no preload polyfill', () => {
    const c = web('vite.portfolio.config.ts');
    expect(c).toContain("assetsDir: 'site-assets'");
    expect(c).toContain('polyfill: false');
    expect(c).toContain('portfolio/index.html');
    expect(c).toContain('portfolio/sweet.html');
  });

  it('package.json builds it with one script', () => {
    expect(JSON.parse(web('package.json')).scripts['build:portfolio']).toBe(BUILD);
  });
});
