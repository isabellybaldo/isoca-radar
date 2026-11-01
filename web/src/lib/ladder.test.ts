import policiesToml from '../../../config/policies.toml?raw';
import { describe, expect, it } from 'vitest';
import type { BreakEven } from '../api/types';
import { ladderFixture, ladderLiveFixture, ladderRung } from '../test/fixtures';
import {
  LADDER, MIN_BAND, MIN_EXEC, SLOTS, asOfText, breakEvenText, epochText, extraText, ladderModel, latencyX, rungCountText, rungMsText,
  slotX, stackRung, usBand, usText, yDomain, NO_RUNGS } from './ladder';

describe('the web LADDER', () => {
  it('names the same six rungs and latencies as the live api (spec §17)', () => {
    const live = ladderLiveFixture().rungs.filter((r) => !r.extra).map((r) => ({ scenario: r.scenario, latency_ms: r.latency_ms }));
    expect(LADDER).toEqual(live);
    expect(MIN_EXEC).toBe(30);
    expect(SLOTS).toBe(7);
  });

  it('matches config/policies.toml: same scenario names, latency = proc_ms + net_ms + match_ms (D-10)', () => {
    const byName = new Map<string, number>();
    for (const block of policiesToml.split('[[scenarios]]').slice(1)) {
      const body = block.split(/\n\[\[/)[0];
      const num = (k: string) => Number(new RegExp(`^${k}\\s*=\\s*(\\d+)`, 'm').exec(body)![1]);
      const name = /^name\s*=\s*"([^"]+)"/m.exec(body)![1];
      byName.set(name, num('proc_ms') + num('net_ms') + num('match_ms'));
    }
    expect(LADDER.map((l) => [l.scenario, byName.get(l.scenario)])).toEqual(LADDER.map((l) => [l.scenario, l.latency_ms]));
  });
});

describe('ladderModel', () => {
  it('keeps all six LADDER columns in order, the extras apart, and passes the api break-even through', () => {
    const m = ladderModel(ladderFixture());
    expect(m.columns.map((c) => [c.scenario, c.kind])).toEqual(LADDER.map((l) => [l.scenario, 'rung']));
    expect(m.extras.map((r) => r.scenario)).toEqual(['sp-idle-nodelay', 'sp-burst']);
    expect(m.breakEven).toEqual({ kind: 'at', latency_ms: 70, direction: 'loses_beyond' });
    expect(m.thinReason).toBeNull();
    expect(m.markets.status).toBe('live');
  });

  it('draws a rung the api left out as missing, and says why the answer is thin (review focus 1)', () => {
    const without25 = ladderFixture().rungs.filter((r) => r.scenario !== 'rung-25');
    const m = ladderModel(ladderFixture({ rungs: without25, break_even: { kind: 'thin' } }));
    expect(m.columns[1]).toEqual({ kind: 'missing', scenario: 'rung-25', latency_ms: 25 });
    expect(m.thinReason).toBe('missing');
    expect(breakEvenText(m).big).toBe('a rung has no account yet');
    const few = ladderModel(
      ladderFixture({ rungs: without25.map((r) => (r.scenario === 'rung-0' ? { ...r, n_exec: 29 } : r)), break_even: { kind: 'thin' } }),
    );
    expect(few.thinReason).toBe('few');
    expect(breakEvenText(few).big).toBe('too few trades yet');
  });

  it('an epoch that has started with no ladder rung marked yet: its own reason, never "a rung has no account" (the accounts exist)', () => {
    const none = ladderModel(ladderFixture({ rungs: [], break_even: { kind: 'thin' } }));
    expect(none.thinReason).toBe('no_marks');
    expect(breakEvenText(none)).toEqual({ big: 'no marks yet', line: NO_RUNGS });
    expect(NO_RUNGS).toBe('No ladder account has a mark in this epoch yet.');
    expect(none.columns.every((c) => c.kind === 'missing')).toBe(true);
    // the extras alone do not make a ladder
    const extras = ladderModel(ladderFixture({ rungs: ladderFixture().rungs.filter((r) => r.extra), break_even: { kind: 'thin' } }));
    expect(extras.thinReason).toBe('no_marks');
    // one rung marked and the others not is still the old state, word for word (ruling: a configured rung without an account)
    const one = ladderModel(ladderFixture({ rungs: ladderFixture().rungs.filter((r) => !r.extra).slice(0, 1), break_even: { kind: 'thin' } }));
    expect(one.thinReason).toBe('missing');
    expect(breakEvenText(one)).toEqual({ big: 'a rung has no account yet', line: 'Every rung needs its own paper account before the ladder can answer.' });
    // and a break-even the api did answer is never rewritten
    expect(ladderModel(ladderFixture({ rungs: [] })).thinReason).toBeNull();
  });

  it('epoch 0: no rungs is six missing columns and no epoch yet', () => {
    const m = ladderModel(ladderFixture({ epoch: 0, start_ms: null, as_of_ms: null, rungs: [], break_even: { kind: 'thin' } }));
    expect(m.columns.every((c) => c.kind === 'missing')).toBe(true);
    expect(m.extras).toEqual([]);
    expect(m.thinReason).toBe('no_epoch');
    expect(breakEvenText(m).big).toBe('no epoch yet');
    expect(epochText(m)).toBe('$ on paper · no epoch has started yet');
    expect(asOfText(m.asOfMs)).toBe('no common mark yet');
  });

  it('a rung the api could not place (no policies file) keeps its slot and reads "?" (review focus 2)', () => {
    const m = ladderModel(ladderFixture({ rungs: ladderFixture().rungs.map((r) => ({ ...r, latency_ms: null })), break_even: { kind: 'thin' } }));
    expect(m.thinReason).toBe('unknown');
    expect(m.columns.map(rungMsText)).toEqual(['?', '?', '?', '?', '?', '?']);
    expect(breakEvenText(m).big).toBe('rung latencies unknown');
    expect(extraText(m.extras[1])).toBe('sp-burst (? ms idle, more in bursts)');
  });
});

describe('the ordinal axis', () => {
  it('puts each rung at its slot centre and interpolates between neighbours', () => {
    LADDER.forEach((l, i) => expect(latencyX(l.latency_ms)).toBeCloseTo(slotX(i)));
    expect(latencyX(150)).toBeCloseTo((slotX(3) + slotX(4)) / 2);
    expect(latencyX(70)).toBeCloseTo(slotX(2) + (18 / 48) * (slotX(3) - slotX(2)));
    expect(slotX(0, 6)).toBeCloseTo(1 / 12);
  });

  it('places the us-band between the right rungs, even past the last one (review focus 2)', () => {
    const b = usBand({ latency_ms: 155 }); // 205..305 ms
    expect(b.x0).toBeGreaterThan(slotX(4));
    expect(b.x0).toBeLessThan(slotX(5));
    expect(b.x1).toBeGreaterThan(slotX(5));
    expect(b.x1).toBeLessThan(slotX(SLOTS - 1));
    const low = usBand({ latency_ms: 0 }); // 50..150 ms
    expect(low.x0).toBeGreaterThan(slotX(1));
    expect(low.x0).toBeLessThan(slotX(2));
    expect(low.x1).toBeGreaterThan(slotX(3));
    expect(low.x1).toBeLessThan(slotX(4));
    expect(latencyX(5150)).toBe(1);
    expect(latencyX(-40)).toBe(slotX(0));
    expect(Number.isFinite(latencyX(Number.MAX_SAFE_INTEGER))).toBe(true);
  });

  it('stops the us-band before the extras slot and never lets it collapse, at any slot count', () => {
    for (const slots of [SLOTS, LADDER.length]) {
      const edge = LADDER.length / slots; // the right edge of the 300 ms slot
      for (const latency_ms of [155, 250, 5000]) {
        const b = usBand({ latency_ms }, slots);
        expect(b.x1, `${slots} slots, measured ${latency_ms}`).toBeLessThanOrEqual(edge);
        expect(b.x1 - b.x0, `${slots} slots, measured ${latency_ms}`).toBeGreaterThanOrEqual(MIN_BAND - 1e-12);
      }
    }
    expect(usBand({ latency_ms: 5000 }).x1).toBe(LADDER.length / SLOTS);
  });
});

describe('bars', () => {
  it('stacks each part by its own sign: gains and settle up, unrealized by its sign, leg-risk down', () => {
    const s = stackRung(ladderRung({ gains: '22.1', leg_risk: '-20', settle: '1.5', unrealized: '-0.5' }));
    expect(s.map((x) => x.part)).toEqual(['gains', 'settle', 'unrealized', 'leg_risk']);
    expect(s[0]).toEqual({ part: 'gains', from: 0, to: 22.1 });
    expect(s[1].from).toBeCloseTo(22.1);
    expect(s[1].to).toBeCloseTo(23.6);
    expect(s[2]).toEqual({ part: 'unrealized', from: 0, to: -0.5 });
    expect(s[3].from).toBeCloseTo(-0.5);
    expect(s[3].to).toBeCloseTo(-20.5);
  });

  it('a fresh epoch (every figure "0") draws nothing and the y domain stays finite (review focus 3)', () => {
    const r = ladderLiveFixture().rungs;
    expect(r.flatMap((x) => stackRung(x))).toEqual([]);
    expect(yDomain(r)).toEqual({ lo: -1, hi: 1 });
    expect(yDomain([])).toEqual({ lo: -1, hi: 1 });
  });

  it('the y domain holds every stack end and every net, with zero inside', () => {
    const d = yDomain(ladderFixture().rungs);
    expect(d.hi).toBeGreaterThan(42.5); // rung-0: 41.5 + 1 settled
    expect(d.lo).toBeLessThan(-82); // sp-burst's leg-risk cost
  });
});

describe('words', () => {
  it('says the break-even in every kind, with "pays on paper" / "loses on paper"', () => {
    const m = (be: BreakEven) => ladderModel(ladderFixture({ break_even: be }));
    expect(breakEvenText(m({ kind: 'at', latency_ms: 70, direction: 'loses_beyond' }))).toEqual({
      big: '52 to 100 ms', line: 'Pays on paper at 52 ms, loses on paper at 100 ms. No account ran in between.',
    });
    expect(breakEvenText(m({ kind: 'at', latency_ms: 40, direction: 'pays_beyond', not_monotone: true }))).toEqual({
      big: '25 to 52 ms', line: 'Loses on paper at 25 ms, pays on paper at 52 ms. No account ran in between.',
    });
    expect(breakEvenText(m({ kind: 'at', latency_ms: 25, direction: 'loses_beyond' }))).toEqual({
      big: '≈ 25 ms', line: 'Pays on paper below it, loses on paper beyond it.',
    });
    expect(breakEvenText(m({ kind: 'all' }))).toEqual({ big: 'pays at every rung', line: 'Pays on paper at every latency on the ladder.' });
    expect(breakEvenText(m({ kind: 'none' }))).toEqual({ big: 'none', line: 'Loses on paper at every latency on the ladder.' });
    expect(breakEvenText(ladderModel(ladderLiveFixture()))).toEqual({
      big: 'too few trades yet', line: 'Each rung needs 30 trades; the counts are under the drawing.',
    });
  });

  it('labels counts n / 30 only while thin, and the rest of the drawing in words', () => {
    const live = ladderModel(ladderLiveFixture());
    expect(live.columns.map((c) => rungCountText(c, true))).toEqual(['2 / 30', '2 / 30', '1 / 30', '1 / 30', '0 / 30', '0 / 30']);
    expect(rungCountText(ladderModel(ladderFixture()).columns[0], false)).toBe('40');
    expect(rungCountText({ kind: 'missing', scenario: 'rung-25', latency_ms: 25 }, true)).toBe('no account');
    expect(rungMsText(live.columns[2])).toBe('52');
    expect(asOfText(Date.UTC(2026, 8, 29, 20, 9))).toBe('as of 20:09 UTC');
    expect(epochText(live)).toBe('$ on paper since epoch 1 began, 2026-09-29 19:58 UTC');
    expect(usText({ latency_ms: 155 })).toBe('us (measured ≈155 + matching)');
    expect(usText(null)).toBe('us: not measured yet');
    expect(extraText(ladderRung({ scenario: 'sp-burst', latency_ms: 450, extra: true }))).toBe('sp-burst (450 ms idle, more in bursts)');
    expect(extraText(ladderRung({ scenario: 'sp-idle-nodelay', latency_ms: 300, extra: true }))).toBe('sp-idle-nodelay (300 ms, market order delay ignored)');
  });
});
