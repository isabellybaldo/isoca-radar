// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { LADDER } from '../lib/ladder';
import { NEW_WALK_LEG, PRESETS, WALK_LEGS, exactlyOne, toTick } from './defaults';

declare const process: { cwd(): string };
const policies: string = readFileSync(`${process.cwd()}/../config/policies.toml`, 'utf8');
const ascending = (legs: { p: string }[][]) => legs.every((l) => l.every((x, k) => k === 0 || toTick(x.p) > toTick(l[k - 1].p)) && l.every((x) => toTick(x.p) >= 1 && toTick(x.p) <= 99));
const best = (legs: { p: string }[][]) => legs.reduce((a, l) => a + toTick(l[0].p), 0);

describe('starting states', () => {
  it("e's presets are the LADDER rungs of config/policies.toml, same proc, net and match", () => {
    expect(PRESETS.map((p) => p.name)).toEqual(LADDER.map((l) => l.scenario));
    const blocks = policies.split('[[scenarios]]').slice(1);
    expect(blocks.length).toBeGreaterThan(5);
    for (const p of PRESETS) {
      const block = blocks.find((b) => new RegExp(`name = "${p.name}"`).test(b));
      expect(block, p.name).toBeDefined();
      for (const k of ['proc_ms', 'net_ms', 'match_ms'] as const) {
        expect(Number(new RegExp(`${k} = (\\d+)`).exec(block ?? '')?.[1]), `${p.name} ${k}`).toBe(p[k]);
      }
    }
  });

  it('"exactly 1.00" shifts leg A so the best asks sum to 1.00, keeping its level order', () => {
    const one = exactlyOne(WALK_LEGS);
    expect(best(one)).toBe(100);
    expect(one[0].map((l) => l.p)).toEqual(['0.36', '0.38', '0.41']);
    expect(one.slice(1)).toEqual(WALK_LEGS.slice(1));
    expect(ascending(one)).toBe(true);
  });

  it('"exactly 1.00" keeps every leg strictly ascending whatever the start (R8)', () => {
    const level = (p: number) => ({ p: `0.${String(p).padStart(2, '0')}`, s: '10' });
    const tall = [[level(97), level(98), level(99)], [level(1), level(2), level(3)]];
    expect(ascending(exactlyOne(tall))).toBe(true);
    const low = [[level(5), level(6), level(7)], [level(30), level(31)], [level(1), level(2)]];
    expect(ascending(exactlyOne(low))).toBe(true);
    expect(best(exactlyOne(low))).toBe(100);
    expect(ascending([NEW_WALK_LEG, ...WALK_LEGS])).toBe(true);
    expect(ascending(WALK_LEGS)).toBe(true);
  });
});
