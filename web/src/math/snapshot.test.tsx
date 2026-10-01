import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import sampleText from './__fixtures__/public.sample.json?raw';
import { snapshotFixture } from './fixtures';
import { FRESH_MS, freshness, hourlyValues, parseSnapshot, publicToLadder } from './snapshot';

/** What fetch().json() hands over: plain JSON (undefined fields gone). */
const json = (x: unknown): unknown => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));

/** Copied from `bins/api/src/public.rs` (`WHITELIST`): every key path the public file may carry. */
const WHITELIST = [
  'v', 'written_ms', 'ladder', 'ladder.epoch', 'ladder.start_ms', 'ladder.as_of_ms', 'ladder.break_even', 'ladder.break_even.kind',
  'ladder.break_even.latency_ms', 'ladder.break_even.direction', 'ladder.break_even.not_monotone', 'ladder.measured',
  'ladder.measured.latency_ms', 'ladder.markets_status', 'ladder.rungs', 'ladder.rungs[].scenario', 'ladder.rungs[].latency_ms',
  'ladder.rungs[].extra', 'ladder.rungs[].n_exec', 'ladder.rungs[].n_leg_risk', 'ladder.rungs[].n_missed', 'ladder.rungs[].gains',
  'ladder.rungs[].leg_risk', 'ladder.rungs[].settle', 'ladder.rungs[].unrealized', 'ladder.rungs[].net', 'ladder.markets',
  'ladder.markets[].side', 'ladder.markets[].scenario', 'ladder.markets[].fee_bips', 'ladder.markets[].latency_ms',
  'ladder.markets[].dimension', 'ladder.markets[].bucket', 'ladder.markets[].label', 'ladder.markets[].n_exec',
  'ladder.markets[].clusters', 'ladder.markets[].mean', 'ladder.markets[].lo', 'ladder.markets[].hi', 'today', 'today.day_start_ms',
  'today.arb', 'today.no', 'today.yes', 'today.edge_budget100', 'today.hourly', 'today.hourly[].hour', 'today.hourly[].arb',
];

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
      ['a market row without a label', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], label: undefined }] } }],
      ['a market row with an unknown label', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], label: 'great' }] } }],
      ['a market row of another dimension', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], dimension: 'days' }] } }],
      ['a market row with 1.5 clusters', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], clusters: 1.5 }] } }],
      ['a market row whose mean is a number', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], mean: 0.4 }] } }],
      ['a YES-side market row', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], side: 'yes' }] } }],
      ['a market row with a fee', { ...b, ladder: { ...L, markets: [{ ...L.markets[0], fee_bips: 10 }] } }],
      ['hourly not a list', { ...b, today: { ...b.today, hourly: {} } }],
      ['an hourly entry with a fractional hour', { ...b, today: { ...b.today, hourly: [{ hour: 0.5, arb: 1 }] } }],
      ['edge as a JSON number', { ...b, today: { ...b.today, edge_budget100: 41.37 } }],
    ];
    for (const [name, x] of bad) expect(parseSnapshot(json(x)), name).toBeNull();
  });

  it('a written_ms in the future is fresh; exactly 30 min is fresh, one ms more is stale (review focus 1)', () => {
    const now = 1_790_000_000_000;
    expect(freshness({ written_ms: now + 60_000 }, now)).toBe('fresh');
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
