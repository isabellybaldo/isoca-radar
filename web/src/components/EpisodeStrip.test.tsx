import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { episodeSummaryFixture } from '../test/fixtures';
import { EpisodeStrip } from './EpisodeStrip';

describe('EpisodeStrip', () => {
  it('draws one tick per episode, hollow when flagged, and counts net-positive', () => {
    const rows = [
      episodeSummaryFixture({ id: 'a', opened_ms: 1000, net: '0.41' }),
      episodeSummaryFixture({ id: 'b', opened_ms: 5000, net: '0', kind: 'yes_basket', stale: true }),
      episodeSummaryFixture({ id: 'c', opened_ms: 9000, net: '-0.2', kind: 'implication' }),
    ];
    const { container } = render(<EpisodeStrip rows={rows} />);
    expect(screen.getByRole('figure', { name: '3 episodes in view, 1 net-positive' })).toBeInTheDocument();
    expect(screen.getByText('3 in view · 1 net-positive')).toBeInTheDocument();
    const ticks = container.querySelectorAll('rect.tick');
    expect(ticks).toHaveLength(3);
    expect(container.querySelectorAll('rect.tick.hollow')).toHaveLength(1);
    expect(ticks[0]).toHaveClass('no');
    expect(ticks[1]).toHaveClass('yes');
    expect(ticks[2]).toHaveClass('other');
    expect(Number(ticks[0].getAttribute('x'))).toBeLessThan(Number(ticks[2].getAttribute('x')));
  });

  it('still draws a single episode, or several at the same instant (review focus 5)', () => {
    const { container } = render(<EpisodeStrip rows={[episodeSummaryFixture(), episodeSummaryFixture({ id: 'x' })]} />);
    const xs = [...container.querySelectorAll('rect.tick')].map((r) => Number(r.getAttribute('x')));
    expect(xs.every((x) => Number.isFinite(x))).toBe(true);
  });
});
