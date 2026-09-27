import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { edgeFixture } from '../test/fixtures';
import { LatencyAxis } from './LatencyAxis';

const pts = edgeFixture().series.decay!;
const measured = { latency_ms: 200, host: 'vps-kvm2' };
const x = (el: Element | null) => Number(el?.getAttribute('x'));

describe('LatencyAxis', () => {
  it('puts each scenario on its own row at its latency, with its rate', () => {
    const { container } = render(<LatencyAxis points={pts} measured={measured} />);
    expect(screen.getByRole('figure', { name: 'capture rate by latency' })).toBeInTheDocument();
    expect(screen.getByText('colo-idle · 52 ms → 24.8%')).toBeInTheDocument();
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-burst · 450 ms → —')).toBeInTheDocument();
    const dot = (s: string) => container.querySelector(`li[data-scenario="${s}"] rect.lat-dot`);
    expect(x(dot('colo-idle'))).toBeLessThan(x(dot('sp-idle')));
    expect(x(dot('sp-idle'))).toBeLessThan(x(dot('sp-burst')));
    expect(dot('sp-idle')).toHaveClass('head');
    expect(dot('sp-burst')).toHaveClass('hollow');
  });

  it('labels the measured latency apart from the scenarios, at one x on every row', () => {
    const { container } = render(<LatencyAxis points={pts} measured={measured} />);
    expect(screen.getByText('measured: vps-kvm2 ≈ 200 ms (an estimate)')).toBeInTheDocument();
    const xs = [...container.querySelectorAll('rect.measured')].map(x);
    expect(xs).toHaveLength(pts.length);
    expect(new Set(xs).size).toBe(1);
  });

  it('stacks scenarios that share a latency on separate rows (sp-idle and sp-idle-nodelay)', () => {
    const both = [...pts, { ...pts[1], scenario: 'sp-idle-nodelay', headline: false }];
    render(<LatencyAxis points={both} measured={null} />);
    expect(screen.getByText('sp-idle · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.getByText('sp-idle-nodelay · 300 ms → 3.2%')).toBeInTheDocument();
    expect(screen.queryByText(/^measured:/)).toBeNull();
  });
});
