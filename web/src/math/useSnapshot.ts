// Spec 7b §6.3: the one request the public page makes besides its own assets. One GET of the static file on
// load; again only when the tab becomes visible and the last attempt is over five minutes old. No timers.
import { useEffect, useRef, useState } from 'react';
import { type PublicSnapshot, REFETCH_MS, SNAPSHOT_URL, parseSnapshot } from './snapshot';

export type SnapshotLoad = { kind: 'loading' } | { kind: 'missing' } | { kind: 'ok'; snap: PublicSnapshot };

/** The writer's file is a few KB; anything past this is not it (and is never handed to JSON.parse). */
export const MAX_BODY_BYTES = 65_536;

/** The body as text, or null once it grows past MAX_BODY_BYTES (the stream is cancelled, not buffered whole). */
async function boundedText(r: Response): Promise<string | null> {
  if (r.body === null) return null;
  const reader = r.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_BODY_BYTES) {
      void reader.cancel().catch(() => undefined);
      return null;
    }
    chunks.push(value);
  }
  const all = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    all.set(c, at);
    at += c.byteLength;
  }
  return new TextDecoder().decode(all);
}

/** One GET of the static file. A non-200, a network error, an abort, an oversize body, bad JSON, v != 1 or a
 *  missing field is null (§6.3 "missing"). */
export async function fetchSnapshot(signal?: AbortSignal): Promise<PublicSnapshot | null> {
  try {
    const r = await fetch(SNAPSHOT_URL, { signal });
    if (r.status !== 200) return null;
    const text = await boundedText(r);
    return text === null ? null : parseSnapshot(JSON.parse(text));
  } catch {
    return null;
  }
}

/** The first request is shared by mounts that overlap in time (React StrictMode mounts, unmounts and mounts
 *  again in one commit), so that is still one request on the wire. It is aborted when the last mount leaves. */
let first: { promise: Promise<PublicSnapshot | null>; abort: AbortController; users: number } | null = null;

function joinFirst(): NonNullable<typeof first> {
  if (first === null) {
    const abort = new AbortController();
    const mine = { promise: fetchSnapshot(abort.signal), abort, users: 0 };
    first = mine;
    void mine.promise.then(() => {
      if (first === mine) first = null;
    });
  }
  first.users += 1;
  return first;
}

function leaveFirst(f: NonNullable<typeof first>): void {
  f.users -= 1;
  // Deferred a microtask: a StrictMode re-mount joins again before this runs, and keeps the request alive.
  queueMicrotask(() => {
    if (f.users <= 0) {
      f.abort.abort();
      if (first === f) first = null;
    }
  });
}

/**
 * `now` is the page's one clock (default Date.now), read for the "last attempt" stamp. A failed re-fetch
 * keeps the last good snapshot, whose own written_ms then ages it into stale; it never blanks numbers shown.
 */
export function useSnapshot(now: () => number = () => Date.now()): SnapshotLoad {
  const [load, setLoad] = useState<SnapshotLoad>({ kind: 'loading' });
  const clock = useRef(now);
  clock.current = now;
  useEffect(() => {
    let live = true;
    let lastAttempt = clock.current();
    let refetch: AbortController | null = null;
    const apply = (s: PublicSnapshot | null) => {
      if (live) setLoad((prev) => (s !== null ? { kind: 'ok', snap: s } : prev.kind === 'ok' ? prev : { kind: 'missing' }));
    };
    const shared = joinFirst();
    void shared.promise.then(apply);
    const onVisible = () => {
      if (document.visibilityState !== 'visible' || clock.current() - lastAttempt <= REFETCH_MS) return;
      lastAttempt = clock.current();
      refetch?.abort();
      const mine = new AbortController();
      refetch = mine;
      void fetchSnapshot(mine.signal).then((s) => {
        if (!mine.signal.aborted) apply(s);
      });
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      live = false;
      document.removeEventListener('visibilitychange', onVisible);
      refetch?.abort();
      leaveFirst(shared);
    };
  }, []);
  return load;
}
