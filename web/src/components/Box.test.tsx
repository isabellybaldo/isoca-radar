import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Box } from './Box';

describe('Box', () => {
  it('puts actions in the title row and marks the hero section', () => {
    const { container } = render(<Box title="EDGE CAPTURE" help="edge" hero actions={<button type="button">24H</button>}><p>body</p></Box>);
    const head = container.querySelector('.box-head') as HTMLElement;
    expect(head).toContainElement(screen.getByRole('button', { name: '24H' }));
    expect(container.querySelector('section')).toHaveClass('box', 'hero');
  });

  it('is a plain box without hero', () => {
    const { container } = render(<Box title="X" help="edge"><p>b</p></Box>);
    expect(container.querySelector('section')).not.toHaveClass('hero');
  });
});
