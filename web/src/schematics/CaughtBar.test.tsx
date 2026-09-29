import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { usText } from '../lib/ladder';
import { CaughtBar } from './CaughtBar';

describe('CaughtBar', () => {
  it('names the us band in an HTML legend under the drawing, never inside the SVG (M5)', () => {
    const { container } = render(<CaughtBar durationMs={4190} measured={{ latency_ms: 155 }} />);
    const legend = screen.getByRole('list', { name: 'legend' });
    expect(within(legend).getByText(usText({ latency_ms: 155 }))).toBeInTheDocument();
    expect(legend.querySelector('.cb-sw.us')).not.toBeNull();
    expect(container.querySelector('svg text')).toBeNull();
    expect(container.querySelectorAll('rect.cb-us')).toHaveLength(1);
  });

  it('without a measurement there is no band, and the legend says so', () => {
    const { container } = render(<CaughtBar durationMs={4190} measured={null} />);
    expect(container.querySelector('rect.cb-us')).toBeNull();
    expect(within(screen.getByRole('list', { name: 'legend' })).getByText(usText(null))).toBeInTheDocument();
  });

  it('a 0 ms episode draws a 0-width bar: even rung 0 is too late (M5)', () => {
    const { container } = render(<CaughtBar durationMs={0} measured={null} />);
    expect(container.querySelector('rect.cb-dur')).toHaveAttribute('width', '0');
    expect(container.querySelectorAll('rect.cb-rung.late')).toHaveLength(6);
    expect(within(screen.getByRole('list', { name: 'rungs in time' })).getAllByText('too late')).toHaveLength(6);
  });

  it('draws no NaN for an open episode', () => {
    const { container } = render(<CaughtBar durationMs={null} measured={{ latency_ms: 155 }} />);
    expect(container.innerHTML).not.toMatch(/NaN/);
  });
});
