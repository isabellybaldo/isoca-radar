import { describe, expect, it } from 'vitest';
import { ENGINE_BROKE, call } from './call';
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
    const out = call(engine(() => '{"error":"two levels at the same price 0.50"}'), 'mirror_json', {});
    expect(isError(out)).toBe(true);
    expect(isError(out) ? out.error : '').toBe('two levels at the same price 0.50');
  });

  it('turns a trap, non-JSON and an array into one sentence', () => {
    const trap = (): string => {
      throw new Error('unreachable');
    };
    for (const f of [trap, () => 'not json', () => '[1]', () => 'null']) {
      expect(call(engine(f), 'basket_json', {})).toEqual({ error: ENGINE_BROKE });
    }
  });
});
