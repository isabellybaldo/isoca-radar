// 09-27 §4.3, 7b §3.4: the WASM build returns exactly the strings the native fixtures hold
// (crates/arb-wasm/fixtures/<export>.json, 7b-A). Read with node fs; vite's fs.allow stays as it is (7b §11).
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { existsSync, readdirSync } from 'node:fs';
import { beforeAll, expect, it } from 'vitest';
import { FIXTURES, describeWasm, fixtureCases, loadTestEngine } from './testEngine';
import { ENGINE_FNS, type Engine } from './types';

const FILES: string[] = existsSync(FIXTURES) ? readdirSync(FIXTURES).filter((f: string) => f.endsWith('.json')).sort() : [];

describeWasm('native = WASM (parity)', () => {
  let engine: Engine;
  beforeAll(async () => {
    engine = await loadTestEngine();
  });

  it('has exactly one fixture file per engine export (review focus 4)', () => {
    expect(FILES).toEqual(ENGINE_FNS.map((f) => `${f}.json`).sort());
  });

  for (const fn of ENGINE_FNS) {
    it(`${fn}.json: every case, byte for byte`, () => {
      const cases = fixtureCases(fn);
      expect(cases.length).toBeGreaterThan(0);
      for (const c of cases) expect(engine[fn](c.input), `${fn} ${c.name}`).toBe(c.output);
    });
  }
});
