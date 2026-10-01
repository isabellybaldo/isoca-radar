// @vitest-environment node
// @ts-ignore: node built-ins exist at runtime under vitest (no @types/node installed)
import { execFileSync } from 'node:child_process';
// @ts-ignore
import { randomBytes } from 'node:crypto';
// @ts-ignore
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
// @ts-ignore
import { tmpdir } from 'node:os';
// @ts-ignore
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';

declare const process: { execPath: string; env: Record<string, string | undefined> };

const SCRIPT: string = fileURLToPath(new URL('../../scripts/check-math-bundle.mjs', import.meta.url));
const made: string[] = [];
afterEach(() => {
  for (const d of made.splice(0)) rmSync(d, { recursive: true, force: true });
});

const HTML = '<script type="module" src="/math-assets/m.js"></script>';

function dist(files: Record<string, string | Uint8Array>, html: string = HTML): string {
  const d: string = mkdtempSync(`${tmpdir()}/math-dist-`);
  made.push(d);
  mkdirSync(`${d}/math-assets`);
  writeFileSync(`${d}/math.html`, html);
  for (const [name, body] of Object.entries(files)) writeFileSync(`${d}/math-assets/${name}`, body);
  return d;
}

function check(d: string): { ok: boolean; out: string } {
  try {
    const out: string = execFileSync(process.execPath, [SCRIPT], { env: { ...process.env, MATH_DIST: d }, encoding: 'utf8', stdio: 'pipe' });
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

  const wasmRef = 'fetch("/math-assets/a.wasm")';
  const rejects: [string, Record<string, string | Uint8Array>, string, string][] = [
    ['an inline script', { 'm.js': 'x' }, `${HTML}<script>1</script>`, 'inline <script>'],
    ['an inline style', { 'm.js': 'x' }, `${HTML}<style>a{}</style>`, 'inline <style>'],
    ['a style= attribute', { 'm.js': 'x' }, `${HTML}<div style="x"></div>`, 'style= attribute'],
    ['an onclick= attribute', { 'm.js': 'x' }, `${HTML}<div onclick="x()"></div>`, 'on<event>= attribute'],
    ['a meta http-equiv', { 'm.js': 'x' }, `${HTML}<meta http-equiv="refresh" content="0">`, 'http-equiv'],
    ['a foreign src', { 'm.js': 'x' }, `${HTML}<img src="https://evil.example/x.png">`, 'https://evil.example/x.png'],
    ['a foreign single-quoted href', { 'm.js': 'x' }, `${HTML}<link href='https://evil.example/s.css'>`, 'https://evil.example/s.css'],
    ['a foreign URL in JS', { 'm.js': 'a="https://evil.example/x"' }, HTML, 'absolute URL https://evil.example/x'],
    ['a wasm data: URL in JS', { 'm.js': 'a="data:application/wasm;base64,AAA"' }, HTML, 'data: URL'],
    ['a wasm named in JS and not emitted', { 'm.js': wasmRef }, HTML, 'a.wasm, which is not a file'],
    ['"/api/" in a non-js file', { 'm.js': 'x', 'm.txt': '/api/x' }, HTML, '"/api/" found in m.txt'],
    ['a build-machine path in the wasm', { 'm.js': 'x', 'm.wasm': 'panic C:\\Users\\x\\.cargo\\a.rs' }, HTML, 'embeds a build-machine path'],
    ['a /home/ path in the wasm', { 'm.js': 'x', 'm.wasm': 'panic /home/x/a.rs' }, HTML, 'embeds a build-machine path'],
  ];
  for (const [name, files, html, msg] of rejects) {
    it(`fails on ${name}`, () => {
      const r = check(dist(files, html));
      expect(r.ok, r.out).toBe(false);
      expect(r.out).toContain(msg);
    });
  }

  it('fails on a subdirectory of math-assets', () => {
    const d = dist({ 'm.js': 'x' });
    mkdirSync(`${d}/math-assets/sub`);
    const r = check(d);
    expect(r.ok).toBe(false);
    expect(r.out).toContain('math-assets/sub is a directory');
  });

  it('passes the allow-listed inert strings, a wasm that is emitted, an /api/ panic path in the wasm and the Google Fonts links', () => {
    const html = `${HTML}<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin><link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=VT323"><link rel="icon" href="data:,">`;
    const js = ['http://www.w3.org/2000/svg', 'http://www.w3.org/1998/Math/MathML', 'http://www.w3.org/1999/xlink', 'http://www.w3.org/XML/1998/namespace', 'https://react.dev/errors/'].map((u) => `"${u}"`).join(',') + ',' + wasmRef;
    const r = check(dist({ 'm.js': js, 'a.wasm': '/src/crates/arb-wasm/src/api/x.rs' }, html));
    expect(r.ok, r.out).toBe(true);
  });
});
