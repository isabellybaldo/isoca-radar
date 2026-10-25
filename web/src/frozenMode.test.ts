import { afterEach, describe, expect, it, vi } from 'vitest';
import { FROZEN_LINE, FROZEN_MS, frozenOn, isFrozen } from './frozenMode';

afterEach(() => vi.unstubAllEnvs());
describe('frozen mode', () => {
  it('is off unless VITE_FROZEN is 1', () => {
    expect(isFrozen()).toBe(false);
    vi.stubEnv('VITE_FROZEN', '1');
    expect(isFrozen()).toBe(true);
  });
  it('names the frozen moment in UTC', () => {
    expect(FROZEN_MS).toBe(1792432680000);
    expect(FROZEN_LINE).toBe('Frozen snapshot from 2026-10-19 17:58 UTC. Capture stopped on 2026-10-19.');
    expect(frozenOn()).toContain('2026-10-19');
    expect(frozenOn()).toContain('17:58');
  });
});
