import type { ReactNode } from 'react';
import type { ApiState } from '../api/useApi';
import { ErrorState, Loading } from './EmptyState';

/** Loading, error or content: never a blank box (spec §6). Stale data stays up during a refetch. */
export function Loadable<T>({ state, what, children }: { state: ApiState<T>; what: string; children: (data: T) => ReactNode }) {
  if (state.data !== undefined) return <>{children(state.data)}</>;
  if (state.error !== undefined) return <ErrorState error={state.error} />;
  return <Loading what={what} />;
}
