import { useCallback, useEffect, useState } from 'react';

export interface ApiState<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
}

/**
 * Fetch on mount and whenever `deps` or `refreshDeps` change; keeps the last data while refetching
 * (no flashing) — with one exception: a `deps` change clears `data`/`error` first, because `deps`
 * identifies *what* is shown (an episode id, a paper account, a filter/cursor) and the old answer
 * would otherwise sit on screen under a new title. `refreshDeps` is for triggers that only ask for
 * a fresher answer to the *same* question — an SSE tick, the 30 s ticker — so it must never clear:
 * that would flash the loading state on every background refresh (Radar's health/open/today all
 * refresh this way).
 */
export function useApi<T>(
  load: (signal: AbortSignal) => Promise<T>,
  deps: readonly unknown[],
  refreshDeps: readonly unknown[] = [],
): ApiState<T> {
  const [data, setData] = useState<T | undefined>(undefined);
  const [error, setError] = useState<unknown>(undefined);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    setData(undefined);
    setError(undefined);
    // `deps` identifies the resource; clear on mount too is a harmless no-op (both start unset).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => {
    const ctl = new AbortController();
    setLoading(true);
    load(ctl.signal).then(
      (d) => {
        if (ctl.signal.aborted) return;
        setData(d);
        setError(undefined);
        setLoading(false);
      },
      (e: unknown) => {
        if (ctl.signal.aborted) return;
        setError(e);
        setLoading(false);
      },
    );
    return () => ctl.abort();
    // `load` is a fresh closure every render; `deps`/`refreshDeps` say when it really changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, ...refreshDeps, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, loading, reload };
}
