import { describe, expect, it } from 'vitest';
import { fmtGb } from './bytes';

describe('fmtGb', () => {
  it('shows GB (10^9 bytes, like the budgets) with one decimal, rounded down, no float formatting', () => {
    expect(fmtGb(12_345_678_901)).toBe('12.3 GB');
    expect(fmtGb(55_000_000_000)).toBe('55.0 GB');
    expect(fmtGb(99_999_999)).toBe('0.0 GB');
    expect(fmtGb(0)).toBe('0.0 GB');
  });
});
