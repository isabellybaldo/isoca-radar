import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { HealthNow } from '../api/types';
import { Hearts, troubles } from './Hearts';

const ok: HealthNow = {
  now_ms: 0, age_ms: 30_000, run_id: 3, minute_ms: 0, tokens: 4000, shards: 40, parked_shards: 0, msgs: 258_000,
  drops: 0, drops_5m: 0, reconnects: 0, lag_p50_ms: 78, lag_p99_ms: 1900, rest_p50_ms: 80, rest_p99_ms: 300,
  clock_offset_ms: 3, ok: true, capture_paused: false, dropped_groups: 0,
};

describe('Hearts', () => {
  it('shows ten hearts when nothing is wrong', () => {
    render(<Hearts health={ok} />);
    expect(screen.getByRole('img', { name: 'health 10 of 10' })).toHaveTextContent('♥'.repeat(10));
  });

  it('loses one heart per trouble kind and names each one', () => {
    const bad = { ...ok, drops_5m: 2, parked_shards: 1, lag_p99_ms: 5001, age_ms: 180_001, capture_paused: true };
    expect(troubles(bad)).toEqual([
      'drops in the last 5 min',
      'parked shards',
      'lag p99 over 5 s',
      'health row older than 3 min',
      'capture paused',
    ]);
    render(<Hearts health={bad} />);
    expect(screen.getByRole('img', { name: /^health 5 of 10: drops/ })).toHaveTextContent('♥♥♥♥♥♡♡♡♡♡');
    expect(troubles({ ...ok, lag_p99_ms: null, age_ms: 180_000 })).toEqual([]);
  });

  it('costs a heart for a clock offset over 250 ms in either direction, but not under or null', () => {
    expect(troubles({ ...ok, clock_offset_ms: 251 })).toEqual(['clock off by 251 ms']);
    expect(troubles({ ...ok, clock_offset_ms: -2_000 })).toEqual(['clock off by -2000 ms']);
    expect(troubles({ ...ok, clock_offset_ms: 250 })).toEqual([]);
    expect(troubles({ ...ok, clock_offset_ms: null })).toEqual([]);
  });
});
