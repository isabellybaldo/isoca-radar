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

  it('shows the error above stale content instead of hiding it (a failed refetch)', () => {
    const e = new ApiError(503, 'db_unavailable', 'The database is thinking. Try again in a moment.');
    render(
      <Loadable state={{ data: 1, error: e, loading: false, reload: () => {} }} what="x">
        {(n) => <p>{`got ${n}`}</p>}
      </Loadable>,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('The database is thinking. Try again in a moment.');
    expect(screen.getByText('got 1')).toBeInTheDocument();
    const order = screen.getByRole('alert').compareDocumentPosition(screen.getByText('got 1'));
    expect(Boolean(order & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
  });
});
