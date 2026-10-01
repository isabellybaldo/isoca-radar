// Vitest runs on Node: node:fs exists at runtime (no @types/node installed), as in theme.test.ts.
// @ts-ignore
import { readFileSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import sampleText from './__fixtures__/public.sample.json?raw';
import { snapshotFixture } from './fixtures';
import { FRESH_MS, freshness, hourlyValues, parseSnapshot, publicToLadder } from './snapshot';

/** What fetch().json() hands over: plain JSON (undefined fields gone). */
const json = (x: unknown): unknown => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));

declare const process: { cwd(): string };

/** `WHITELIST` read from `bins/api/src/public.rs` itself (every key path the public file may carry), not a hand copy. */
const rust: string = readFileSync(`${process.cwd()}/../bins/api/src/public.rs`, 'utf8');
const WHITELIST: string[] = [...(/const WHITELIST: \[&str; \d+\] = \[([\s\S]*?)\n {4}\];/.exec(rust)?.[1] ?? '').matchAll(/"([^"]+)"/g)].map((m) => m[1]);

/** The same walk as public.rs `paths`: objects join with ".", array elements with "[]". */
function keyPaths(v: unknown, at: string, out: Set<string>): Set<string> {
  if (Array.isArray(v)) for (const x of v) keyPaths(x, `${at}[]`, out);
  else if (typeof v === 'object' && v !== null) {
    for (const [k, x] of Object.entries(v)) {
      const p = at === '' ? k : `${at}.${k}`;
      out.add(p);
      keyPaths(x, p, out);
    }
  }
  return out;
}

describe('parseSnapshot (spec 7b §5.1, §6.3)', () => {
  it('accepts the snapshot 7b-A writes', () => {
    const s = snapshotFixture({}, 1_790_000_000_000);
    expect(parseSnapshot(json(s))).toEqual(s);
    expect(JSON.stringify(s)).not.toMatch(/account_id|"host"|vps-kvm2/);
  });

  it('accepts the realistic sample, whose key paths are exactly the 48 of the api whitelist (R18)', () => {
    expect(WHITELIST).toHaveLength(48);
    expect(new Set(WHITELIST).size).toBe(48);
    const raw: unknown = JSON.parse(sampleText);
    expect([...keyPaths(raw, '', new Set())].sort()).toEqual([...WHITELIST].sort());
    const s = parseSnapshot(raw);
    expect(s).not.toBeNull();
    // decimals stay strings, long ones included
    expect(s?.ladder.rungs[2].net).toBe('-12345.678901234567');
    expect(s?.today.edge_budget100).toBe('12345.678901234567');
    expect(s?.ladder.break_even).toEqual({ kind: 'at', latency_ms: 140, direction: 'loses_beyond', not_monotone: true });
    expect(sampleText).not.toMatch(/account_id|"host"|vps-kvm2/);
  });

  it('drops a key that is not on the whitelist instead of passing it on', () => {
    const b = snapshotFixture();
    const s = parseSnapshot(json({ ...b, account_id: 'P0', ladder: { ...b.ladder, host: 'vps-kvm2', measured: { latency_ms: 155, host: 'vps-kvm2' } } }));
    expect(s).toEqual(b);
    expect(JSON.stringify(s)).not.toMatch(/account_id|host|vps-kvm2/);
  });

  it('accepts every legal value on the accept side', () => {
    const b = snapshotFixture();
    const L = b.ladder;
    const ok = (x: unknown) => expect(parseSnapshot(json(x))).not.toBeNull();
    ok({ ...b, ladder: { ...L, measured: null, start_ms: null, as_of_ms: null } });
    ok({ ...b, today: { ...b.today, edge_budget100: null, hourly: [] } });
    ok({ ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], latency_ms: null }] } });
    for (const be of [{ kind: 'thin' }, { kind: 'all' }, { kind: 'none' }, { kind: 'at', latency_ms: 0, direction: 'pays_beyond' },
      { kind: 'at', latency_ms: 70, direction: 'loses_beyond', not_monotone: true }, { kind: 'at', latency_ms: 70, direction: 'loses_beyond', not_monotone: false }]) {
      ok({ ...b, ladder: { ...L, break_even: be } });
    }
    for (const label of ['candidate', 'negative', 'inconclusive', 'thin', 'not_tested', 'dead_on_paper']) {
      ok({ ...b, ladder: { ...L, markets: [{ ...L.markets[0], label }] } });
    }
    for (const markets_status of ['live', 'warming', 'under_review', 'missing']) ok({ ...b, ladder: { ...L, markets_status } });
    for (const bucket of ['all', 'untagged', 'other', '1-7', '30-90', '90-365', 'lt1', 'ge365', '00-04']) {
      ok({ ...b, ladder: { ...L, markets: [{ ...L.markets[0], bucket }] } });
    }
    for (const scenario of ['rung-0', 'colo-idle', 'sp-idle-nodelay', 'sp-burst']) {
      ok({ ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], scenario }] } });
    }
  });

  it('drops a market row that does not validate and keeps the snapshot (the writer does the same)', () => {
    const b = snapshotFixture();
    const L = b.ladder;
    const good = L.markets[0];
    const badRows = [
      { ...good, label: undefined }, { ...good, label: 'great' }, { ...good, dimension: 'days' }, { ...good, clusters: 1.5 },
      { ...good, mean: 0.4 }, { ...good, side: 'yes' }, { ...good, fee_bips: 10 }, { ...good, bucket: '__proto__' },
      { ...good, bucket: 'Constructor' }, { ...good, bucket: 'x'.repeat(50_000) }, { ...good, scenario: '__proto__' }, 'junk', null,
    ];
    const s = parseSnapshot(json({ ...b, ladder: { ...L, markets: [...badRows, good] } }));
    expect(s).not.toBeNull();
    expect(s?.ladder.markets).toEqual([good]);
    expect(s?.ladder.rungs).toEqual(L.rungs);
    expect(s?.today).toEqual(b.today);
  });

  it('keeps at most 126 market rows', () => {
    const b = snapshotFixture();
    const rows = Array.from({ length: 130 }, (_, i) => ({ ...b.ladder.markets[0], bucket: `b${i}` }));
    expect(parseSnapshot(json({ ...b, ladder: { ...b.ladder, markets: rows } }))?.ladder.markets).toEqual(rows.slice(0, 126));
  });

  it('rejects every shape the page cannot draw (review focus 1, R15)', () => {
    const b = snapshotFixture();
    const L = b.ladder;
    const bad: [string, unknown][] = [
      ['null', null],
      ['a string', 'nope'],
      ['an array', []],
      ['v 2', { ...b, v: 2 }],
      ['no written_ms', { ...b, written_ms: undefined }],
      ['a fractional written_ms', { ...b, written_ms: 1.5 }],
      ['no today', { ...b, today: undefined }],
      ['no ladder', { ...b, ladder: undefined }],
      ['a rung whose gains is a JSON number', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], gains: 3.8 }] } }],
      ['a rung whose net is not a decimal', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], net: '1e3' }] } }],
      ['a rung without a scenario', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], scenario: undefined }] } }],
      ['a rung with a __proto__ scenario', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], scenario: '__proto__' }] } }],
      ['a rung with a 50,000-char scenario', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], scenario: 'a'.repeat(50_000) }] } }],
      ['a negative n_exec', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], n_exec: -1 }] } }],
      ['a negative latency', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], latency_ms: -1 }] } }],
      ['an unsafe integer', { ...b, ladder: { ...L, epoch: 2 ** 53 } }],
      ['a negative start_ms', { ...b, ladder: { ...L, start_ms: -1 } }],
      ['a decimal of 41 chars', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], net: `1.${'0'.repeat(39)}` }] } }],
      ['duplicate rung scenarios', { ...b, ladder: { ...L, rungs: [L.rungs[0], L.rungs[0]] } }],
      ['17 rungs', { ...b, ladder: { ...L, rungs: Array.from({ length: 17 }, (_, i) => ({ ...L.rungs[0], scenario: `r${i}` })) } }],
      ['25 hourly entries', { ...b, today: { ...b.today, hourly: Array.from({ length: 25 }, () => ({ hour: 0, arb: 1 })) } }],
      ['an hourly entry at hour 24', { ...b, today: { ...b.today, hourly: [{ hour: 24, arb: 1 }] } }],
      ['markets not a list', { ...b, ladder: { ...L, markets: {} } }],
      ['a rung with 1.5 executed', { ...b, ladder: { ...L, rungs: [{ ...L.rungs[0], n_exec: 1.5 }] } }],
      ['a fractional epoch', { ...b, ladder: { ...L, epoch: 1.5 } }],
      ['an unknown break_even', { ...b, ladder: { ...L, break_even: { kind: 'maybe' } } }],
      ['break_even at without a latency', { ...b, ladder: { ...L, break_even: { kind: 'at' } } }],
      ['break_even at without a direction', { ...b, ladder: { ...L, break_even: { kind: 'at', latency_ms: 70 } } }],
      ['break_even at with an unknown direction', { ...b, ladder: { ...L, break_even: { kind: 'at', latency_ms: 70, direction: 'sideways' } } }],
      ['break_even at with a fractional latency', { ...b, ladder: { ...L, break_even: { kind: 'at', latency_ms: 70.5, direction: 'loses_beyond' } } }],
      ['break_even at with a non-boolean not_monotone', { ...b, ladder: { ...L, break_even: { kind: 'at', latency_ms: 70, direction: 'loses_beyond', not_monotone: 'yes' } } }],
      ['a fractional measured latency', { ...b, ladder: { ...L, measured: { latency_ms: 1.5 } } }],
      ['an unknown markets_status', { ...b, ladder: { ...L, markets_status: 'busy' } }],
      ['hourly not a list', { ...b, today: { ...b.today, hourly: {} } }],
      ['an hourly entry with a fractional hour', { ...b, today: { ...b.today, hourly: [{ hour: 0.5, arb: 1 }] } }],
      ['edge as a JSON number', { ...b, today: { ...b.today, edge_budget100: 41.37 } }],
    ];
    for (const [name, x] of bad) expect(parseSnapshot(json(x)), name).toBeNull();
  });

  it('a few minutes of clock skew is fresh; more than 30 min either way is stale (review focus 1)', () => {
    const now = 1_790_000_000_000;
    expect(freshness({ written_ms: now + 60_000 }, now)).toBe('fresh');
    expect(freshness({ written_ms: now + FRESH_MS }, now)).toBe('fresh');
    expect(freshness({ written_ms: now + FRESH_MS + 1 }, now)).toBe('stale');
    expect(freshness({ written_ms: now - FRESH_MS }, now)).toBe('fresh');
    expect(freshness({ written_ms: now - FRESH_MS - 1 }, now)).toBe('stale');
  });

  it('spreads the hourly counts over 24 UTC hours and ignores an hour out of range', () => {
    const v = hourlyValues({ hourly: [{ hour: 0, arb: 2 }, { hour: 3, arb: 5 }, { hour: 24, arb: 9 }, { hour: -1, arb: 9 }] });
    expect(v).toHaveLength(24);
    expect(v.slice(0, 5)).toEqual([2, 0, 0, 5, 0]);
    expect(v.reduce((a, x) => a + x, 0)).toBe(7);
  });
});

describe('publicToLadder (spec 7b §7.2)', () => {
  it('feeds ladderModel and LatencyLadder unchanged, and drops a market row of another dimension', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const s = snapshotFixture();
    const drift = { ...s.ladder, markets: [...s.ladder.markets, { ...s.ladder.markets[0], dimension: 'days' as 'all', bucket: '1-7' }] };
    const l = publicToLadder(drift);
    expect(l.markets).toHaveLength(s.ladder.markets.length);
    expect(l.measured).toEqual({ latency_ms: 155 });
    expect(JSON.stringify(l)).not.toContain('host');
    const { container } = render(<LatencyLadder model={ladderModel(l)} />);
    expect(screen.getByText('≈ 70 ms')).toHaveClass('ll-big');
    expect(container.querySelectorAll('circle.ll-net')).toHaveLength(6);
    expect(screen.getByText('us (measured ≈155 + matching)')).toBeInTheDocument();
    expect(error).not.toHaveBeenCalled();
  });
});
