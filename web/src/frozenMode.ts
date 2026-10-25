import { fmtUtcDayHm } from './lib/time';

// The portfolio site's frozen mode: a build-time flag, read on every call so tests can stub it.
export function isFrozen(): boolean {
  return import.meta.env.VITE_FROZEN === '1';
}

// Capture stopped on the VPS at this moment (2026-10-19 17:58 UTC).
export const FROZEN_MS = 1792432680000;

export const FROZEN_LINE = 'Frozen snapshot from 2026-10-19 17:58 UTC. Capture stopped on 2026-10-19.';

export function frozenOn(): string {
  return `frozen on ${fmtUtcDayHm(FROZEN_MS)}`;
}
