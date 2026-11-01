// The headline sentence is fixed copy, so it is held to the committed snapshot it summarises.
import { describe, expect, it } from 'vitest';
import frozenText from '../frozen/public.json?raw';
import { FINDING } from './frozenMode';
import { LADDER, breakEvenSpan } from './lib/ladder';

const { ladder } = JSON.parse(frozenText) as {
  ladder: {
    break_even: { kind: string; latency_ms: number; direction: string };
    rungs: { scenario: string; net: string; extra: boolean }[];
  };
};

describe('FINDING', () => {
  it('states what the frozen ladder shows: pays at 0 ms, loses at every delay from 25 ms on', () => {
    expect(ladder.break_even.kind).toBe('at');
    expect(ladder.break_even.direction).toBe('loses_beyond');
    expect(breakEvenSpan(ladder.break_even.latency_ms)).toEqual({ lo: 0, hi: 25 });
    expect(FINDING).toContain('below 25 ms');
    for (const r of ladder.rungs) {
      if (r.scenario === 'rung-0') expect(Number(r.net), r.scenario).toBeGreaterThan(0);
      else expect(Number(r.net), r.scenario).toBeLessThan(0);
    }
  });

  it('names the two real setups at their ladder latencies', () => {
    const ms = (s: string) => LADDER.find((r) => r.scenario === s)?.latency_ms;
    expect(FINDING).toContain(`beside the exchange was modelled at ${ms('colo-idle')} ms`);
    expect(FINDING).toContain(`São Paulo at ${ms('sp-idle')} ms`);
  });
});
