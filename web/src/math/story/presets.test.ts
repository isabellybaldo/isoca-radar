// @ts-ignore: node:fs exists at runtime under vitest
import { readFileSync } from 'node:fs';
import { beforeAll, expect, it } from 'vitest';
import { basketWord } from '../../schematics/BasketLegs';
import { LADDER } from '../../lib/ladder';
import { call } from '../../wasm/call';
import { describeWasm, loadTestEngine } from '../../wasm/testEngine';
import type { BasketModel, ClassifyModel, Engine, EpisodeModel, MirrorModel, PayoffMatrix, WalkModel } from '../../wasm/types';
import { isError } from '../../wasm/types';
import { CHAPTERS } from './copy';
import { CH1, CH2, CH3, CH3_LABELS, CH4, CH5, CH6 } from './presets';

declare const process: { cwd(): string };

/** The engine's model for an input; a test fails loudly on an engine error. */
function ok<T>(engine: Engine, fn: Parameters<typeof call>[1], input: unknown): T {
  const out = call<T>(engine, fn, input);
  if (isError(out)) throw new Error(`${fn}: ${out.error}`);
  return out;
}
const positive = (d: string) => !d.startsWith('-') && /[1-9]/.test(d);
const less = (a: string, b: string) => Number(a) < Number(b); // comparison only: nothing here is shown as text

describeWasm('story presets on the real engine (spec §4)', () => {
  let e: Engine;
  beforeAll(async () => { e = await loadTestEngine(); });

  it('chapter 1: a healthy book, then a crossed one', () => {
    expect(ok<MirrorModel>(e, 'mirror_json', CH1[0]).crossed).toBe(false);
    expect(ok<MirrorModel>(e, 'mirror_json', CH1[1]).crossed).toBe(true);
  });

  it('chapter 2: every outcome listed has no broken column; one not listed has one', () => {
    expect(ok<PayoffMatrix>(e, 'payoff_json', CH2[0]).broken.some(Boolean)).toBe(false);
    expect(ok<PayoffMatrix>(e, 'payoff_json', CH2[1]).broken.some(Boolean)).toBe(true);
  });

  it('chapter 3: at 1.00 nothing; below it a YES basket settled at resolution with a positive net', () => {
    expect(ok<ClassifyModel>(e, 'classify_json', CH3[0]).kind).toBeNull();
    const m = ok<ClassifyModel>(e, 'classify_json', CH3[1]);
    expect(m.kind).toBe('yes_basket');
    expect(m.realization).toBe('settlement');
    expect(m.net !== null && positive(m.net)).toBe(true);
  });

  it('chapter 3: each label is the sum of its own preset ticks, and has no dollar sign', () => {
    const sum = (i: 0 | 1) => {
      const ticks = CH3[i].legs.reduce((a, l) => a + Math.round(Number(l.asks[0].p) * 100), 0); // test-side check of the label
      return `${Math.floor(ticks / 100)}.${String(ticks % 100).padStart(2, '0')}`;
    };
    expect(CH3_LABELS).toEqual([`prices add to ${sum(0)}`, `prices add to ${sum(1)}`]);
    expect(CH3_LABELS.join(' ')).not.toContain('$');
  });

  it('chapter 4: a thin book ends the walk sooner and leaves less', () => {
    const deep = ok<WalkModel>(e, 'depth_walk_json', CH4[0]);
    const thin = ok<WalkModel>(e, 'depth_walk_json', CH4[1]);
    for (const m of [deep, thin]) {
      expect(positive(m.q_star)).toBe(true);
      expect(positive(m.pi)).toBe(true);
    }
    expect(less(thin.q_star, deep.q_star)).toBe(true);
    expect(less(thin.pi, deep.pi)).toBe(true);
    // the picture must change too: Q* sits at a different share of the walk's length
    const share = (m: WalkModel) => Number(m.q_star) / Number(m.steps[m.steps.length - 1].q_to);
    expect(share(thin)).not.toBeCloseTo(share(deep), 2);
  });

  it('chapter 5: the edge outlasts the order, then closes first', () => {
    const a = ok<EpisodeModel>(e, 'episode_json', CH5[0]);
    const b = ok<EpisodeModel>(e, 'episode_json', CH5[1]);
    expect([a.open_ms !== null, a.caught]).toEqual([true, true]);
    expect([b.open_ms !== null, b.caught]).toEqual([true, false]);
  });

  it('chapter 6: every leg fills, then one misses', () => {
    expect(basketWord(ok<BasketModel>(e, 'basket_json', CH6[0]))).toBe('filled');
    expect(basketWord(ok<BasketModel>(e, 'basket_json', CH6[1]))).toBe('caught');
  });

  it('chapter 7 says "six accounts": the ladder in config/policies.toml has six rungs', () => {
    const toml: string = readFileSync(`${process.cwd()}/../config/policies.toml`, 'utf8');
    expect(LADDER).toHaveLength(6);
    for (const l of LADDER) expect(toml).toContain(`"${l.scenario}"`);
    expect(CHAPTERS[6].lead).toMatch(/\bsix paper accounts\b/);
    // The file's scenarios, less the ones its own comment calls "extras beside the ladder", are the ladder: exactly LADDER.
    const scenarios = toml.split('[[scenarios]]').slice(1).map((block) => /^name\s*=\s*"([^"]+)"/m.exec(block)?.[1] ?? '');
    const comments = toml.split('\n').filter((l) => l.startsWith('#')).map((l) => l.replace(/^#\s*/, '')).join(' ');
    const sentence = comments.split(/(?<=\.)\s+/).find((x) => x.includes('extras beside the ladder')) ?? '';
    const extras = scenarios.filter((n) => new RegExp(`(?<![\\w-])${n}(?![\\w-])`).test(sentence));
    expect(extras).toHaveLength(2); // the sentence was found, and it names two
    const ladder = scenarios.filter((n) => !extras.includes(n));
    expect(ladder).toHaveLength(LADDER.length);
    expect([...ladder].sort()).toEqual(LADDER.map((l) => l.scenario).sort());
  });
});
