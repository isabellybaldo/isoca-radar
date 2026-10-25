// Frozen-site plan, task 4: what running the scanner live took. Every label here was checked against its source
// (task-4 report); "7 services · 1.1 GB of memory limits" was dropped: postgres and the scanner have no mem_limit.
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { RunCost } from './RunCost';

const LABELS = [
  '123 GB/day in · 1.4 MB/s, all day',
  '33 GB/day after zstd',
  'scanner ≈ 65 % of a core, all day',
  'compact: 2 cores while compacting',
  'kept: 24 h raw ≤ 30 GB · archive ≤ 40 GB',
  'left behind: 28 GB of data',
];

describe('RunCost', () => {
  it('is one labelled image', () => {
    render(<RunCost />);
    const img = screen.getByRole('img');
    expect(img.getAttribute('aria-label')).toMatch(/Polymarket feed/);
  });

  it.each(LABELS)('writes %s', (label) => {
    render(<RunCost />);
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it('names the five stages in order', () => {
    const { container } = render(<RunCost />);
    const text = container.textContent ?? '';
    const at = ['Polymarket feed', 'scanner', 'raw capture', 'compact', 'Parquet archive'].map((s) => text.indexOf(s));
    expect(at.every((i) => i >= 0)).toBe(true);
    expect([...at].sort((a, b) => a - b)).toEqual(at);
  });

  it('draws no NaN', () => {
    const { container } = render(<RunCost />);
    for (const el of container.querySelectorAll('*')) for (const a of el.attributes) expect(a.value, a.name).not.toMatch(/NaN|Infinity/);
  });
});
