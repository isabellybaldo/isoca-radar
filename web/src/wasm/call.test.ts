import { describe, expect, it } from 'vitest';
import { ENGINE_BROKE, MAX_ERROR, call } from './call';
import { fixtureCase } from './testEngine';
import { ENGINE_FNS, type Engine, isError } from './types';

const engine = (f: (input: string) => string): Engine => Object.fromEntries(ENGINE_FNS.map((n) => [n, f])) as Engine;

describe('call (review focus 2)', () => {
  it('sends the input as JSON and hands back the parsed model', () => {
    let seen = '';
    const out = call<{ ok: number }>(engine((i) => { seen = i; return '{"ok":1}'; }), 'mirror_json', { bids: [], asks: [] });
    expect(seen).toBe('{"bids":[],"asks":[]}');
    expect(out).toEqual({ ok: 1 });
  });

  it("keeps the engine's own sentence", () => {
    const c = fixtureCase('mirror_json', 'duplicate-price');
    const sentence = (JSON.parse(c.output) as { error: string }).error;
    const out = call(engine(() => c.output), 'mirror_json', {});
    expect(isError(out)).toBe(true);
    expect(isError(out) ? out.error : '').toBe(sentence);
  });

  it('turns a trap, non-JSON and an array into one sentence', () => {
    const trap = (): string => {
      throw new Error('unreachable');
    };
    for (const f of [trap, () => 'not json', () => '[1]', () => 'null']) {
      expect(call(engine(f), 'basket_json', {})).toEqual({ error: ENGINE_BROKE });
    }
  });

  it('never throws: a non-string return, a JSON primitive, a circular input', () => {
    for (const f of [() => undefined, () => 7, () => '1', () => '"x"', () => 'true']) {
      expect(call(engine(f as unknown as (i: string) => string), 'basket_json', {})).toEqual({ error: ENGINE_BROKE });
    }
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(call(engine(() => '{"ok":1}'), 'basket_json', loop)).toEqual({ error: ENGINE_BROKE });
  });

  it('caps the engine sentence at 200 characters plus an ellipsis', () => {
    const long = 'x'.repeat(MAX_ERROR + 50);
    expect(call(engine(() => JSON.stringify({ error: long })), 'mirror_json', {})).toEqual({ error: `${'x'.repeat(MAX_ERROR)}…` });
    const exact = 'y'.repeat(MAX_ERROR);
    expect(call(engine(() => JSON.stringify({ error: exact })), 'mirror_json', {})).toEqual({ error: exact });
  });

  it('an error key that is not a string is a broken engine', () => {
    for (const bad of ['{"error":1}', '{"error":null}', '{"error":{"a":1}}']) {
      expect(call(engine(() => bad), 'mirror_json', {})).toEqual({ error: ENGINE_BROKE });
    }
  });
});
