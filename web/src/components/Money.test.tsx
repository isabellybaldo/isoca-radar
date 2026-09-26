import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Money } from './Money';

describe('Money', () => {
  it('renders the decimal string exactly', () => {
    render(
      <>
        <Money value="12345678901234567.89" />
        <Money value="0.000001" />
      </>,
    );
    expect(screen.getByText('$12,345,678,901,234,567.89')).toBeInTheDocument();
    expect(screen.getByText('$0.000001')).toBeInTheDocument();
  });

  it('tones gains and losses, and keeps null distinct from zero', () => {
    render(
      <>
        <Money value="0.41" signed tone />
        <Money value="-0.12" signed tone />
        <Money value="0" signed tone />
        <Money value={null} />
      </>,
    );
    expect(screen.getByText('+$0.41')).toHaveClass('pos');
    expect(screen.getByText('−$0.12')).toHaveClass('neg');
    expect(screen.getByText('$0.00')).not.toHaveClass('pos');
    expect(screen.getByText('—')).toHaveAttribute('title', 'not measured');
  });
});
