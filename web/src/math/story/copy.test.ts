import { describe, expect, it } from 'vitest';
import { checkHonesty } from '../../explain/honesty';
import { CH3_COUNT, CH7_EXTRAS, CHAPTERS, HOOK, PLAY, TAKE, WORDS } from './copy';

const every: string[] = [
  HOOK, CH3_COUNT, CH7_EXTRAS, PLAY.title, PLAY.line,
  ...CHAPTERS.flatMap((c) => [c.question, c.lead, c.switchLabel, ...(c.options ?? [])]),
  TAKE.ch1(false), TAKE.ch1(true), TAKE.ch2(false), TAKE.ch2(true), TAKE.ch3(false), TAKE.ch3(true), TAKE.ch4,
  TAKE.ch5(false), TAKE.ch5(true), TAKE.ch6('filled'), TAKE.ch6('caught'), TAKE.ch6('nothing filled'), TAKE.ch7,
  ...WORDS.flat(),
];

describe('story copy (spec §5, §6)', () => {
  it('seven chapters, numbered 1 to 7, ids s1 to s7, each asking a question', () => {
    expect(CHAPTERS.map((c) => [c.n, c.id])).toEqual([1, 2, 3, 4, 5, 6, 7].map((n) => [n, `s${n}`]));
    for (const c of CHAPTERS) expect(c.question.endsWith('?')).toBe(true);
  });

  it('uses no word the honesty check bans, no dollar figure, no exclamation mark', () => {
    for (const s of every) {
      expect(checkHonesty(s), s).toEqual([]);
      expect(s, s).not.toMatch(/\$\s?\d/);
      expect(s, s).not.toContain('!');
    }
  });

  it('a lead is at most four sentences', () => {
    for (const c of CHAPTERS) expect(c.lead.split(/(?<=[.?])\s+/).length, c.id).toBeLessThanOrEqual(4);
  });

  it('each outcome sentence has a different line per outcome', () => {
    expect(TAKE.ch1(true)).not.toBe(TAKE.ch1(false));
    expect(TAKE.ch2(true)).not.toBe(TAKE.ch2(false));
    expect(TAKE.ch3(true)).not.toBe(TAKE.ch3(false));
    expect(TAKE.ch5(true)).not.toBe(TAKE.ch5(false));
    expect(new Set([TAKE.ch6('filled'), TAKE.ch6('caught'), TAKE.ch6('nothing filled')]).size).toBe(3);
  });

  it('the word list has the twelve terms of the spec, in its order', () => {
    expect(WORDS.map(([t]) => t)).toEqual([
      'YES / NO share', 'book, bid, ask', 'basket, leg', 'NO basket', 'edge (Π)', 'Q*', 'episode', 't_x', 'unwind', 'convert', 'on the table', 'on paper',
    ]);
  });
});
