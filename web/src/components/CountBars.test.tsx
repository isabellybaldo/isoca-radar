import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { CountBars } from './CountBars';

describe('CountBars', () => {
  it('draws one bar per kind, longest first as given, with the count written out', () => {
    const { container } = render(<CountBars label="integrity by kind" items={[{ label: 'top_mismatch', count: 4256 }, { label: 'drift', count: 961 }]} />);
    expect(screen.getByRole('list', { name: 'integrity by kind' })).toBeInTheDocument();
    expect(screen.getByText('4,256')).toHaveClass('cb-count', 'num');
    const bars = [...container.querySelectorAll('.cb-bar')] as HTMLElement[];
    expect(bars[0].style.width).toBe('100%');
    expect(bars[1].style.width).toBe('23%');
  });

  it('renders bars with widths proportional to the items as given, respecting order', () => {
    const { container } = render(<CountBars label="test" items={[{ label: 'a', count: 100 }, { label: 'b', count: 50 }, { label: 'c', count: 25 }]} />);
    const bars = [...container.querySelectorAll('.cb-bar')] as HTMLElement[];
    expect(bars[0].style.width).toBe('100%');
    expect(bars[1].style.width).toBe('50%');
    expect(bars[2].style.width).toBe('25%');
  });
});
