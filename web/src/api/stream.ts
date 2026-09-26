export type StreamState = 'live' | 'reconnecting';

export interface StreamHandlers {
  onEpisode?: (id: string) => void;
  onHealth?: (minuteMs: number) => void;
  onState?: (s: StreamState) => void;
}

export interface EventSourceLike {
  readyState: number;
  onopen: ((ev: Event) => unknown) | null;
  onerror: ((ev: Event) => unknown) | null;
  addEventListener(type: string, fn: (ev: MessageEvent) => void): void;
  close(): void;
}

export interface StreamOptions {
  url?: string;
  baseDelayMs?: number;
  maxDelayMs?: number;
  create?: (url: string) => EventSourceLike;
}

const CLOSED = 2;

/**
 * One EventSource on /api/stream. The browser retries a dropped stream by itself; when it gives up
 * (readyState CLOSED, e.g. an HTTP error or an expired login) we reopen with doubling backoff.
 * Returns stop().
 */
export function openStream(h: StreamHandlers, o: StreamOptions = {}): () => void {
  const url = o.url ?? '/api/stream';
  const base = o.baseDelayMs ?? 1000;
  const max = o.maxDelayMs ?? 30_000;
  const create = o.create ?? ((u: string) => new EventSource(u) as unknown as EventSourceLike);
  let delay = base;
  let es: EventSourceLike | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;

  const connect = () => {
    if (stopped) return;
    const current = create(url);
    es = current;
    current.onopen = () => {
      delay = base;
      h.onState?.('live');
    };
    current.onerror = () => {
      h.onState?.('reconnecting');
      if (current.readyState === CLOSED && es === current) {
        current.close();
        es = null;
        timer = setTimeout(connect, delay);
        delay = Math.min(delay * 2, max);
      }
    };
    current.addEventListener('episode', (ev) => h.onEpisode?.(String(ev.data)));
    current.addEventListener('health', (ev) => {
      const m = Number(ev.data);
      if (Number.isFinite(m)) h.onHealth?.(m);
    });
  };

  h.onState?.('reconnecting');
  connect();
  return () => {
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
    es?.close();
  };
}
