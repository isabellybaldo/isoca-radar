import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SplitBar } from './SplitBar';

describe('SplitBar', () => {
  it('sizes each part by its share and writes its value', () => {
    const { container } = render(
      <SplitBar label="on the table, by side" parts={[{ name: 'NO cashable', value: '51.49', tone: 'p0' }, { name: 'YES locked', value: '206.52', tone: 'p3' }]} />,
    );
    expect(screen.getByRole('figure', { name: 'on the table, by side' })).toBeInTheDocument();
    expect(screen.getByText('NO cashable $51.49')).toBeInTheDocument();
    expect(screen.getByText('YES locked $206.52')).toBeInTheDocument();
    const [a, b] = [...container.querySelectorAll('.split-part')] as HTMLElement[];
    expect(a.style.flexGrow).toBe('51.49');
    expect(b.style.flexGrow).toBe('206.52');
  });

  it('draws nothing but the labels when both parts are empty or missing', () => {
    const { container } = render(<SplitBar label="x" parts={[{ name: 'NO', value: null, tone: 'p0' }, { name: 'YES', value: '0', tone: 'p3' }]} />);
    expect(container.querySelectorAll('.split-part')).toHaveLength(0);
    expect(screen.getByText('NO —')).toBeInTheDocument();
  });
});
