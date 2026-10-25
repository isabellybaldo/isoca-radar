// Frozen-site plan Task 5: the portfolio workflow builds web/dist-portfolio/ and keeps it as the radar-site
// artifact. Plain text checks, read with node fs as deploy.test.ts does; CRLF normalized.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const repo = (p: string): string => (readFileSync(`${process.cwd()}/../${p}`, 'utf8') as string).replace(/\r\n/g, '\n');

describe('portfolio workflow (frozen site, Task 5)', () => {
  const yml = repo('.github/workflows/portfolio.yml');

  it('runs on push to main, on pull requests and by hand, with read-only contents', () => {
    expect(yml).toMatch(/^on:\n {2}push:\n {4}branches: \[main\]\n {2}pull_request:\n {2}workflow_dispatch:/m);
    expect(yml).toMatch(/^permissions:\n {2}contents: read$/m);
  });

  it('builds the portfolio and keeps web/dist-portfolio as the radar-site artifact for 7 days', () => {
    expect(yml).toContain('npm run build:portfolio');
    expect(yml).toContain('name: radar-site');
    expect(yml).toContain('path: web/dist-portfolio');
    expect(yml).toContain('retention-days: 7');
    expect(yml.indexOf('npm run build:portfolio')).toBeLessThan(yml.indexOf('actions/upload-artifact@'));
  });

  it('pins every action to a full commit SHA', () => {
    const uses = yml.split('\n').filter((l) => /^\s*(- )?uses:/.test(l));
    expect(uses.length).toBeGreaterThanOrEqual(4);
    for (const l of uses) expect(l).toMatch(/@[0-9a-f]{40}\b/);
  });

  it('repeats the web job toolchain steps of ci.yml in its order', () => {
    const steps = [
      'rustup show',
      'rustup target add wasm32-unknown-unknown',
      'sh scripts/wasm-bindgen-version.sh',
      '~/.cargo/bin/wasm-bindgen',
      'command -v wasm-bindgen || cargo install wasm-bindgen-cli --locked --version',
      'sh scripts/build-wasm.sh',
      'node-version: 22',
      'npm ci',
      'npm run build:portfolio',
    ];
    const ci = repo('.github/workflows/ci.yml');
    const web = ci.slice(ci.indexOf('\n  web:\n'), ci.indexOf('\n  compose:\n'));
    for (const s of steps.slice(0, 8)) expect(web, s).toContain(s);
    const at = steps.map((s) => yml.indexOf(s));
    expect(at.every((i) => i >= 0)).toBe(true);
    expect([...at].sort((a, b) => a - b)).toEqual(at);
  });
});
