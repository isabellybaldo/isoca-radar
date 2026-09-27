import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { CardSeries } from '../api/types';
import { Survival, sharedTimeGrid, xOfTime } from './CardCharts';

type Curve = NonNullable<CardSeries['curves']>[number];

// The always-18-point Kaplan–Meier curve vs. the empirical curve, which skips grid points it never
// measured (500 and 5000 here) and so is shorter — exactly the shape that made the old per-curve
// array-index x put the same real duration at different pixels on the two curves.
const km: Curve = {
  label: 'YES · Kaplan–Meier (ours)',
  points: [
    { t_ms: 100, s: '1.0000' },
    { t_ms: 500, s: '0.9000' },
    { t_ms: 1000, s: '0.5000' },
    { t_ms: 5000, s: '0.2000' },
    { t_ms: 10000, s: '0.1000' },
  ],
  median_ms: '1000',
};
const empirical: Curve = {
  label: 'YES · empirical (theirs)',
  points: [
    { t_ms: 100, s: '1.0000' },
    { t_ms: 1000, s: '0.4000' },
    { t_ms: 10000, s: '0.1000' },
  ],
  median_ms: '2000', // between two grid points: exercises interpolation, not an exact hit
};

describe('Survival chart shared time axis (fix round 1)', () => {
  it('sharedTimeGrid is the ascending union of every t_ms, even across curves of different lengths', () => {
    expect(sharedTimeGrid([km, empirical])).toEqual([100, 500, 1000, 5000, 10000]);
  });

  it('xOfTime is exact on the grid and linear between neighbours', () => {
    const grid = sharedTimeGrid([km, empirical]);
    expect(xOfTime(grid, 100)).toBe(0);
    expect(xOfTime(grid, 1000)).toBe(2);
    expect(xOfTime(grid, 10000)).toBe(4);
    expect(xOfTime(grid, 2000)).toBeCloseTo(2.25, 10); // 2 + (2000-1000)/(5000-1000)
  });

  it('the same t_ms maps to the same x whether it came from the long curve or the short one', () => {
    const grid = sharedTimeGrid([km, empirical]);
    // t_ms=1000 is km's point index 2 (of 5) but empirical's point index 1 (of 3): different local
    // array positions, same real duration.
    expect(km.points[2].t_ms).toBe(empirical.points[1].t_ms);
    const kmX = xOfTime(grid, km.points[2].t_ms);
    const empX = xOfTime(grid, empirical.points[1].t_ms);
    expect(kmX).toBe(empX);
    expect(kmX).toBe(2);
  });

  it('each median marker lands at the x of its own median_ms', () => {
    const grid = sharedTimeGrid([km, empirical]);
    expect(xOfTime(grid, Number(km.median_ms))).toBe(2); // 1000 is exactly on the grid
    expect(xOfTime(grid, Number(empirical.median_ms))).toBeCloseTo(2.25, 10); // 2000 sits between 1000 and 5000
  });

  it('renders one marker per curve with a median, on the shared axis', () => {
    const { container } = render(<Survival curves={[km, empirical]} />);
    expect(container.querySelectorAll('rect.marker')).toHaveLength(2);
  });
});
