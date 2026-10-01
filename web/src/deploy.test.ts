// Spec 7b §7, §5.3, §9 (09-27 §4.2–§5.3): the checked-in build and deploy files carry what the public page
// needs. Plain text checks, read with node fs (as theme.test.ts reads theme.css); CRLF normalized.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

declare const process: { cwd(): string };
const repo = (p: string): string => (readFileSync(`${process.cwd()}/../${p}`, 'utf8') as string).replace(/\r\n/g, '\n');

/** One top-level job of a workflow: from `  <name>:` up to the next `  <job>:` line. */
function job(yml: string, name: string): string {
  const start = yml.indexOf(`\n  ${name}:\n`);
  expect(start, `no job ${name}`).toBeGreaterThanOrEqual(0);
  const rest = yml.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[a-z][\w-]*:\n/);
  return next < 0 ? rest : rest.slice(0, next + 2);
}

/** The `run:` lines of a job (comments may mention anything). */
const runLines = (j: string): string[] =>
  j.split('\n').filter((l) => /^\s*(- )?run:/.test(l));

describe('ci (09-27 §4.3)', () => {
  it('the web job builds the engine, runs the WASM side of parity and the full build, with WASM_REQUIRED', () => {
    const yml = repo('.github/workflows/ci.yml');
    const web = job(yml, 'web');
    const runs = runLines(web).join('\n');
    expect(web).toContain('WASM_REQUIRED: "1"');
    expect(web).toContain('rustup target add wasm32-unknown-unknown');
    expect(web).toContain('cargo install wasm-bindgen-cli --locked --version');
    expect(web).toContain('sh scripts/wasm-bindgen-version.sh'); // one source for the CLI version
    expect(web).not.toContain('awk');
    expect(runs).not.toContain('cargo test -p arb-wasm'); // 7b-A's wasm job runs the native side
    expect(yml).toContain('\n  wasm:\n');
    expect(job(yml, 'wasm')).toContain('cargo test -p arb-wasm');
    // the engine ships as the release package, never the debug one
    expect(runs).not.toContain('PROFILE=dev');
    expect(web).toContain('npx tsc --noEmit -p tsconfig.math.json');
    const steps = [
      'sh scripts/build-wasm.sh',
      'npm ci',
      'npx tsc --noEmit\n',
      'npx tsc --noEmit -p tsconfig.math.json',
      'npm test',
      'npm run build',
    ].map((s) => web.indexOf(s));
    expect(steps.every((i) => i >= 0)).toBe(true);
    expect([...steps].sort((a, b) => a - b)).toEqual(steps);
  });
});
