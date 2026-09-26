import { renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { useApi } from './useApi';

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe('useApi', () => {
  it('clears stale data when the identity deps change, before the new answer lands', async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const { result, rerender } = renderHook(
      ({ id }: { id: string }) => useApi((): Promise<string> => (id === 'a' ? first.promise : second.promise), [id]),
      { initialProps: { id: 'a' } },
    );
    first.resolve('episode A');
    await waitFor(() => expect(result.current.data).toBe('episode A'));
    rerender({ id: 'b' });
    // Cleared immediately: the old episode's data must never sit under the new id's title.
    expect(result.current.data).toBeUndefined();
    second.resolve('episode B');
    await waitFor(() => expect(result.current.data).toBe('episode B'));
  });

  it('keeps stale data during a refreshDeps-only refetch (no flicker on a ticker/SSE tick)', async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    let calls = 0;
    const { result, rerender } = renderHook(
      ({ tick }: { tick: number }) => useApi((): Promise<string> => (calls++ === 0 ? first.promise : second.promise), [], [tick]),
      { initialProps: { tick: 0 } },
    );
    first.resolve('open episodes v1');
    await waitFor(() => expect(result.current.data).toBe('open episodes v1'));
    rerender({ tick: 1 }); // e.g. Radar's 30 s ticker or an SSE health/episode event
    expect(result.current.data).toBe('open episodes v1'); // not cleared: no flash to loading
    second.resolve('open episodes v2');
    await waitFor(() => expect(result.current.data).toBe('open episodes v2'));
  });
});
