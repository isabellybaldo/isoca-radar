import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { ErrorState, Loading, errorText } from './EmptyState';
import { Loadable } from './Loadable';

describe('states', () => {
  it('shows the api detail for api errors', () => {
    const e = new ApiError(503, 'db_unavailable', 'The database is thinking. Try again in a moment.');
    render(<ErrorState error={e} />);
    expect(screen.getByRole('alert')).toHaveTextContent('The database is thinking. Try again in a moment.');
  });

  it('a failed fetch says how to sign in again', () => {
    expect(errorText(new TypeError('Failed to fetch'))).toContain('reload the page to sign in again');
  });

  it('Loadable picks loading, error or content, never blank', () => {
    const { rerender } = render(<Loadable state={{ data: undefined, error: undefined, loading: true, reload: () => {} }} what="x">{() => 'ok'}</Loadable>);
    expect(screen.getByRole('status')).toHaveTextContent('Loading x');
    rerender(<Loadable state={{ data: undefined, error: new Error('boom'), loading: false, reload: () => {} }} what="x">{() => 'ok'}</Loadable>);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    rerender(<Loadable state={{ data: 1, error: undefined, loading: false, reload: () => {} }} what="x">{(n) => `got ${n}`}</Loadable>);
    expect(screen.getByText('got 1')).toBeInTheDocument();
    render(<Loading />);
  });
});
