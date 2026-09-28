import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { HealthNow } from '../api/types';
import { Hearts, troubles } from './Hearts';

const ok: HealthNow = {
  now_ms: 0, age_ms: 30_000, run_id: 3, minute_ms: 0, tokens: 4000, shards: 40, parked_shards: 0, msgs: 258_000,
  drops: 0, drops_5m: 0, reconnects: 0, lag_p50_ms: 78, lag_p99_ms: 1900, rest_p50_ms: 80, rest_p99_ms: 300,
  clock_offset_ms: 3, ok: true, capture_paused: false, dropped_groups: 0,
  archive_bytes: 12_400_000_000, archive_oldest_day: '2026-09-28', compact_lag_hours: 1, compact_last_error: null,
  compact_age_ms: 600_000, compact_started_ms: -36_000_000,
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

  it('costs one heart when the compactor is behind or silent, never two', () => {
    expect(troubles({ ...ok, compact_lag_hours: 4 })).toEqual(['compactor behind: raw at risk']);
    expect(troubles({ ...ok, compact_age_ms: 7_200_001 })).toEqual(['compactor status older than 2 h']);
    expect(troubles({ ...ok, compact_lag_hours: 9, compact_age_ms: 9_000_000 })).toEqual(['compactor behind: raw at risk']);
    expect(troubles({ ...ok, compact_lag_hours: 3, compact_age_ms: 7_200_000 })).toEqual([]);
    expect(troubles({ ...ok, compact_lag_hours: null, compact_age_ms: null, compact_started_ms: null })).toEqual([]);
    // no status row 2 h after the first compact_hours row (ok.now_ms is 0)
    expect(troubles({ ...ok, compact_age_ms: null, compact_started_ms: -7_200_001 })).toEqual(['compactor has never reported']);
    expect(troubles({ ...ok, compact_age_ms: null, compact_started_ms: -7_200_000 })).toEqual([]);
  });
});
