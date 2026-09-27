import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { DecayPoint } from '../api/types';
import { DecayChart } from './DecayChart';

const pt = (scenario: string, latency_ms: number, rate_pct: string | null, headline = false): DecayPoint => ({
  scenario, latency_ms, net_ms: 145, captured: '0.4', rate_pct, headline,
});

describe('DecayChart', () => {
  it('keeps points at the same latency apart so both stay visible', () => {
    const { container } = render(
      <DecayChart points={[pt('colo-idle', 52, '24.8'), pt('sp-idle', 300, '3.2', true), pt('sp-idle-nodelay', 300, '3.2')]} measured={null} />,
    );
    const rects = [...container.querySelectorAll('rect.pt')];
    expect(rects).toHaveLength(3);
    const xs = rects.map((r) => Number(r.getAttribute('x')));
    expect(new Set(xs).size).toBe(3);
    const [, a, b] = xs;
    expect(Math.abs(a - b)).toBeGreaterThanOrEqual(5); // a 5-unit square each: no overlap
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-idle-nodelay · 300 ms → 3.2%')).toBeInTheDocument();
  });

  it('does not move a point that has its latency to itself', () => {
    const { container } = render(<DecayChart points={[pt('sp-idle', 300, '3.2')]} measured={{ latency_ms: 300, host: 'h' }} />);
    const pt0 = container.querySelector('rect.pt') as SVGRectElement;
    const marker = container.querySelector('rect.marker') as SVGRectElement;
    expect(Number(pt0.getAttribute('x')) + 2).toBe(Number(marker.getAttribute('x')));
  });
});
