import type { ReactNode } from 'react';
import type { ApiState } from '../api/useApi';
import { ErrorState, Loading } from './EmptyState';

/**
 * Loading, error or content: never a blank box (spec §6). Stale data stays up during a refetch —
 * but a failed refetch must not disappear just because old content is still on screen: when both
 * are present, a one-line error sits above the stale content instead of hiding it.
 */
export function Loadable<T>({ state, what, children }: { state: ApiState<T>; what: string; children: (data: T) => ReactNode }) {
  if (state.data !== undefined) {
    return (
      <>
        {state.error !== undefined && <ErrorState error={state.error} />}
        {children(state.data)}
      </>
    );
  }
  if (state.error !== undefined) return <ErrorState error={state.error} />;
  return <Loading what={what} />;
}
