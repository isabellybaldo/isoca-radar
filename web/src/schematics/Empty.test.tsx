import { cleanup, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EmptyState } from '../components/EmptyState';
import { Empty } from './Empty';

describe('Empty (spec 7b §7.2)', () => {
  it('draws the plain line, and the dashboard EmptyState draws the same markup', () => {
    const a = render(<Empty text="nothing yet" />).container.innerHTML;
    cleanup();
    const b = render(<EmptyState text="nothing yet" />).container.innerHTML;
    expect(a).toBe('<p class="quip empty">nothing yet</p>');
    expect(b).toBe(a);
  });
});
