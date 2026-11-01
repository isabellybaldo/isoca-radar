// Frozen-site plan, task 4: the landing, in the portfolio shell. Exact copy; "live" only in the cost heading.
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { FINDING, FROZEN_LINE } from '../frozenMode';
import { Landing } from './Landing';
import { Shell } from './Shell';

const COST = 'What it took to run live';

describe('Landing', () => {
  it('has the title, both cards, the lead, the cost section and the frozen line', () => {
    render(<Shell><Landing /></Shell>);
    expect(screen.getByRole('heading', { level: 1, name: 'isoca-radar' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'The math' })).toHaveAttribute('href', '/math');
    expect(screen.getByRole('link', { name: 'Sweet spots' })).toHaveAttribute('href', '/sweet');
    expect(screen.getByText(/Paper only: it never placed an order\.$/)).toBeInTheDocument();
    const cost = screen.getByRole('heading', { name: COST });
    // /math links to /#cost (ruling R1).
    expect(cost.closest('section')).toHaveAttribute('id', 'cost');
    expect(screen.getByRole('img')).toBeInTheDocument();
    expect(screen.getByText(FROZEN_LINE)).toBeInTheDocument();
    expect(screen.getByText(FINDING)).toBeInTheDocument();
  });

  it('the shell links home, to /math and to /sweet with plain links', () => {
    render(<Shell><p>x</p></Shell>);
    expect(screen.getByRole('link', { name: '♥ isoca-radar' })).toHaveAttribute('href', '/');
    expect(screen.getByRole('link', { name: 'MATH' })).toHaveAttribute('href', '/math');
    expect(screen.getByRole('link', { name: 'SWEET SPOTS' })).toHaveAttribute('href', '/sweet');
  });

  it('says "live" nowhere but in the cost heading', () => {
    render(<Shell><Landing /></Shell>);
    const text = (document.body.textContent ?? '').replace(COST, '');
    expect(text).not.toMatch(/\blive\b/i);
  });
});
