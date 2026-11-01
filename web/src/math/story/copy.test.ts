import { describe, expect, it } from 'vitest';
import { checkHonesty } from '../../explain/honesty';
import { FINDING } from '../../frozenMode';
import { CH3_COUNT, CH7_EXTRAS, CH7_LEAD_FROZEN, CHAPTERS, HOOK, HOOK_FROZEN, PLAY, TAKE, WORDS } from './copy';

const every: string[] = [
  HOOK, HOOK_FROZEN, FINDING, CH7_LEAD_FROZEN, CH3_COUNT, CH7_EXTRAS, PLAY.title, PLAY.line,
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

  it('the frozen chapter 7 lead is at most four sentences too', () => {
    expect(CH7_LEAD_FROZEN.split(/(?<=[.?])\s+/).length).toBeLessThanOrEqual(4);
  });

  it('each outcome sentence has a different line per outcome', () => {
    expect(TAKE.ch1(true)).not.toBe(TAKE.ch1(false));
    expect(TAKE.ch2(true)).not.toBe(TAKE.ch2(false));
    expect(TAKE.ch3(true)).not.toBe(TAKE.ch3(false));
    expect(TAKE.ch5(true)).not.toBe(TAKE.ch5(false));
    expect(new Set([TAKE.ch6('filled'), TAKE.ch6('caught'), TAKE.ch6('nothing filled')]).size).toBe(3);
  });

  it('the sentences the whole-branch review corrected say what is true of the page', () => {
    // chapter 7 is drawn from the live snapshot, not by the engine
    expect(HOOK.endsWith(' Chapters 1 to 6 run the scanner’s own Rust code, compiled to WASM; chapter 7 shows its live paper accounts.')).toBe(true);
    expect(HOOK).not.toMatch(/Every drawing/);
    // chapter 3 and section c draw different things, and a fold's state is kept in the browser
    expect(PLAY.line).toBe('The engine behind each chapter, with every control. Nothing you do here leaves your browser.');
    // the second state names what the drawing shows
    expect(TAKE.ch3(true)).toBe('The gap, on every basket the books can fill, is the edge. The scanner runs this check each time a price moves.');
    expect(TAKE.ch3(false)).toBe('At exactly one dollar there is nothing to take.');
  });

  it('the word list has the twelve terms of the spec in order, then three for the ladder', () => {
    expect(WORDS.map(([t]) => t)).toEqual([
      'YES / NO share', 'book, bid, ask', 'basket, leg', 'NO basket', 'edge (Π)', 'Q*', 'episode', 't_x', 'unwind', 'convert', 'on the table', 'on paper', 'rung', 'epoch', 'secondsDelay',
    ]);
  });
});
