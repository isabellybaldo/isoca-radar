// Tests only: the generated package through import.meta.glob (an empty map when pkg is missing, so
// `npm test` works without Rust, 09-27 §4.2) and the .wasm bytes from disk through initSync.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe } from 'vitest';
import { ENGINE_FNS, type Engine, type EngineFn } from './types';

declare const process: { cwd(): string; env: Record<string, string | undefined> };

interface Pkg extends Engine {
  initSync(o: { module: unknown }): unknown;
}
const KEY = './pkg/arb_wasm.js';
const PKG = import.meta.glob<Pkg>('./pkg/arb_wasm.js');

export const HAVE_PKG = KEY in PKG;
/** CI sets WASM_REQUIRED=1: a missing package is then a failure, not a skip (like STORE_TESTS_REQUIRED). */
export const WASM_REQUIRED = process.env.WASM_REQUIRED === '1';

export function skipReason(what: string): string {
  return `${what}: web/src/wasm/pkg is missing, skipped (run scripts/build-wasm.sh; WASM_REQUIRED=1 makes this a failure)`;
}

/** 7b-A's golden fixtures: crates/arb-wasm/fixtures/<export>.json, [{name, input, output}] as exact strings. */
export const FIXTURES = `${process.cwd()}/../crates/arb-wasm/fixtures`;
export interface FixtureCase {
  name: string;
  input: string;
  output: string;
}

export function fixtureCases(fn: EngineFn): FixtureCase[] {
  return JSON.parse(readFileSync(`${FIXTURES}/${fn}.json`, 'utf8')) as FixtureCase[];
}

/** One named case (sections start from these: fed-book, no-edge-3, no-all-fill, …); throws when missing. */
export function fixtureCase(fn: EngineFn, name: string): FixtureCase {
  const c = fixtureCases(fn).find((x) => x.name === name);
  if (c === undefined) throw new Error(`no fixture case ${fn}/${name}`);
  return c;
}

/** A describe block that needs the real WASM: skipped with a printed reason when pkg is missing, a failure under WASM_REQUIRED=1. */
export function describeWasm(name: string, fn: () => void): void {
  const skip = !HAVE_PKG && !WASM_REQUIRED;
  if (skip) console.info(skipReason(name));
  describe.skipIf(skip)(name, fn);
}

let cached: Promise<Engine> | null = null;

export function loadTestEngine(): Promise<Engine> {
  const load = PKG[KEY];
  if (load === undefined) return Promise.reject(new Error('web/src/wasm/pkg is missing: run scripts/build-wasm.sh'));
  cached ??= load().then((m) => {
    m.initSync({ module: readFileSync(`${process.cwd()}/src/wasm/pkg/arb_wasm_bg.wasm`) });
    return Object.fromEntries(ENGINE_FNS.map((f) => [f, m[f]])) as Engine;
  });
  return cached;
}
