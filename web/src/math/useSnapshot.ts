// Spec 7b §6.3: the one request the public page makes besides its own assets. One GET of the static file on
// load; again only when the tab becomes visible and the last attempt is over five minutes old. No polling; the only
// timer is the 10 s limit on the first request, cleared when it settles.
import { useEffect, useRef, useState } from 'react';
import { type PublicSnapshot, REFETCH_MS, SNAPSHOT_URL, parseSnapshot } from './snapshot';

export type SnapshotLoad = { kind: 'loading' } | { kind: 'missing' } | { kind: 'ok'; snap: PublicSnapshot };
/** What the hook returns: the load, and `checkedAt`, the injected clock at the last visibility return or fetch attempt. */
export type SnapshotState = SnapshotLoad & { checkedAt: number };

/** The first request is given this long, then aborted and reported missing (the one timer the page may hold). */
export const FIRST_TIMEOUT_MS = 10_000;

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
    if (r.status !== 200) {
      void r.body?.cancel().catch(() => undefined);
      return null;
    }
    const text = await boundedText(r);
    return text === null ? null : parseSnapshot(JSON.parse(text));
  } catch {
    return null;
  }
}

type First = { promise: Promise<PublicSnapshot | null>; abort: AbortController; users: number };
/** The first request is shared by mounts that overlap in time (React StrictMode mounts, unmounts and mounts
 *  again in one commit), so that is still one request on the wire. It is aborted when the last mount leaves,
 *  or after FIRST_TIMEOUT_MS; its timer is cleared as soon as it settles. */
let first: First | null = null;

function joinFirst(): First {
  if (first === null) {
    const abort = new AbortController();
    const timer = setTimeout(() => abort.abort(), FIRST_TIMEOUT_MS);
    const mine: First = { promise: fetchSnapshot(abort.signal), abort, users: 0 };
    first = mine;
    void mine.promise.then(() => {
      clearTimeout(timer);
      if (first === mine) first = null;
    });
  }
  first.users += 1;
  return first;
}

function leaveFirst(f: First): void {
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
 * Call it ONCE, at page level; LiveStrip and LiveLadder take props. `now` is the page's one clock (default
 * Date.now). `checkedAt` is that clock at the last fetch attempt or tab return, and changes on every such
 * event even when nothing is re-fetched, so the page computes `freshness(snap, checkedAt)` and a kept
 * snapshot ages into "the live writer is behind". Staleness is re-evaluated on visibility changes and fetch
 * attempts only: no interval timer. A failed re-fetch keeps the last good snapshot, and a snapshot is never
 * replaced by an older one (larger written_ms wins).
 */
export function useSnapshot(now: () => number = () => Date.now()): SnapshotState {
  const [st, setSt] = useState<SnapshotState>(() => ({ kind: 'loading', checkedAt: now() }));
  const clock = useRef(now);
  useEffect(() => {
    clock.current = now;
  });
  useEffect(() => {
    let live = true;
    let lastAttempt = clock.current();
    let refetch: AbortController | null = null;
    const apply = (s: PublicSnapshot | null) => {
      if (!live) return;
      const checkedAt = clock.current();
      setSt((prev) => {
        if (prev.kind === 'ok' && (s === null || s.written_ms < prev.snap.written_ms)) return { ...prev, checkedAt };
        return s !== null ? { kind: 'ok', snap: s, checkedAt } : { kind: 'missing', checkedAt };
      });
    };
    const shared = joinFirst();
    void shared.promise.then(apply);
    const onVisible = () => {
      if (document.visibilityState !== 'visible') return;
      const t = clock.current();
      if (t - lastAttempt <= REFETCH_MS) {
        setSt((prev) => ({ ...prev, checkedAt: t }));
        return;
      }
      lastAttempt = t;
      refetch?.abort();
      const mine = new AbortController();
      refetch = mine;
      setSt((prev) => ({ ...prev, checkedAt: t }));
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
  return st;
}
