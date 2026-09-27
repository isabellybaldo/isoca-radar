import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Tiles } from './Tiles';

describe('Tiles', () => {
  it('renders each value with its label, tone and title', () => {
    render(<Tiles items={[{ label: 'on the table', value: '$258.01' }, { label: 'captured', value: '−$462.87', tone: 'neg', title: '−$462.8712' }]} />);
    const list = screen.getByRole('list');
    expect(list.tagName).toBe('DL');
    expect(screen.getByText('on the table')).toBeInTheDocument();
    expect(screen.getByText('$258.01')).toBeInTheDocument();
    const neg = screen.getByText('−$462.87');
    expect(neg).toHaveClass('neg');
    expect(neg).toHaveAttribute('title', '−$462.8712');
  });
});
