// @vitest-environment node
// @ts-ignore: node built-ins exist at runtime under vitest (no @types/node installed)
import { execFileSync } from 'node:child_process';
// @ts-ignore
import { randomBytes } from 'node:crypto';
// @ts-ignore
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
// @ts-ignore
import { tmpdir } from 'node:os';
import { describe, expect, it } from 'vitest';

declare const process: { execPath: string; env: Record<string, string | undefined> };

function dist(files: Record<string, string | Uint8Array>): string {
  const d: string = mkdtempSync(`${tmpdir()}/math-dist-`);
  mkdirSync(`${d}/math-assets`);
  writeFileSync(`${d}/math.html`, '<script type="module" src="/math-assets/m.js"></script>');
  for (const [name, body] of Object.entries(files)) writeFileSync(`${d}/math-assets/${name}`, body);
  return d;
}

function check(d: string): { ok: boolean; out: string } {
  try {
    const out: string = execFileSync(process.execPath, ['scripts/check-math-bundle.mjs'], { env: { ...process.env, MATH_DIST: d }, encoding: 'utf8', stdio: 'pipe' });
    return { ok: true, out };
  } catch (e) {
    const x = e as { stdout?: string; stderr?: string };
    return { ok: false, out: `${x.stdout ?? ''}${x.stderr ?? ''}` };
  }
}

describe('check-math-bundle.mjs (09-27 §4.4)', () => {
  it('passes a small bundle and writes a .gz beside each asset', () => {
    const d = dist({ 'm.js': 'console.log("math")', 'm.css': 'body{}' });
    const r = check(d);
    expect(r.ok, r.out).toBe(true);
    expect(r.out).toMatch(/math-assets total: \d+ B gzip \(limit 350000\)/);
    expect(existsSync(`${d}/math-assets/m.js.gz`)).toBe(true);
  });

  it('fails on "/api/" in an asset (review focus 5)', () => {
    const r = check(dist({ 'm.js': 'fetch("/api/today")' }));
    expect(r.ok).toBe(false);
    expect(r.out).toContain('"/api/" found in m.js');
  });

  it('fails over 350 KB gzip', () => {
    const r = check(dist({ 'm.js': 'x', 'm.wasm': randomBytes(400_000) }));
    expect(r.ok).toBe(false);
    expect(r.out).toContain('over the 350 KB gzip budget');
  });
});
