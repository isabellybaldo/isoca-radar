import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Tiles } from './Tiles';

describe('Tiles', () => {
  it('renders each value with its label, tone and title', () => {
    render(<Tiles items={[{ label: 'on the table', value: '$258.01' }, { label: 'captured', value: '−$462.87', tone: 'neg', title: '−$462.8712' }]} />);
    const list = screen.getByRole('list');
    expect(list.tagName).toBe('UL');
    expect(screen.getByText('on the table')).toBeInTheDocument();
    expect(screen.getByText('$258.01')).toBeInTheDocument();
    const neg = screen.getByText('−$462.87');
    expect(neg).toHaveClass('tile-value', 'neg');
    expect(neg).toHaveAttribute('title', '−$462.8712');
  });

  it('M8: is a real <ul>/<li> list, label before value in the DOM', () => {
    const { container } = render(<Tiles items={[{ label: 'on the table', value: '$258.01' }]} />);
    const li = screen.getByRole('listitem');
    expect(li.tagName).toBe('LI');
    expect(container.querySelector('li.tile > .tile-label')).toHaveTextContent('on the table');
    expect(container.querySelector('li.tile > .tile-value')).toHaveTextContent('$258.01');
    const [label, value] = li.children;
    expect(label).toHaveClass('tile-label');
    expect(value).toHaveClass('tile-value');
  });

  it('adds class dim on the tiles element when dim is set, so interim values do not read as a result', () => {
    render(<Tiles items={[{ label: 'on the table', value: '$258.01' }]} dim />);
    expect(screen.getByRole('list')).toHaveClass('tiles', 'dim');
  });
});
