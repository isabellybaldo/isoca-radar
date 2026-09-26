import { beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource } from '../test/fakeEventSource';
import { openStream, type StreamState } from './stream';

const create = (u: string) => new FakeEventSource(u);
const es = (i: number) => FakeEventSource.instances[i];

describe('openStream', () => {
  beforeEach(() => {
    FakeEventSource.instances = [];
  });

  it('goes live on open and reports episode and health events', () => {
    const states: StreamState[] = [];
    const episodes: string[] = [];
    const minutes: number[] = [];
    openStream(
      { onState: (s) => states.push(s), onEpisode: (id) => episodes.push(id), onHealth: (m) => minutes.push(m) },
      { create },
    );
    expect(es(0).url).toBe('/api/stream');
    es(0).open();
    es(0).emit('episode', 'live-1-00000001');
    es(0).emit('health', '120000');
    es(0).emit('health', 'nonsense');
    expect(states).toEqual(['reconnecting', 'live']);
    expect(episodes).toEqual(['live-1-00000001']);
    expect(minutes).toEqual([120000]);
  });

  it('reconnects with doubling backoff only after the browser gives up, and stops cleanly', () => {
    vi.useFakeTimers();
    const states: StreamState[] = [];
    const stop = openStream({ onState: (s) => states.push(s) }, { create, baseDelayMs: 1000, maxDelayMs: 4000 });
    es(0).fail(false); // the browser is retrying by itself
    vi.advanceTimersByTime(10_000);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(states.at(-1)).toBe('reconnecting');
    es(0).fail(true);
    vi.advanceTimersByTime(999);
    expect(FakeEventSource.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(2);
    es(1).fail(true);
    vi.advanceTimersByTime(1999);
    expect(FakeEventSource.instances).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeEventSource.instances).toHaveLength(3);
    es(2).fail(true);
    vi.advanceTimersByTime(4000);
    es(3).fail(true);
    vi.advanceTimersByTime(4000); // capped at maxDelayMs
    expect(FakeEventSource.instances).toHaveLength(5);
    es(4).open();
    expect(states.at(-1)).toBe('live');
    es(4).fail(true);
    vi.advanceTimersByTime(1000); // an open resets the backoff
    expect(FakeEventSource.instances).toHaveLength(6);
    stop();
    expect(es(5).readyState).toBe(2);
    es(5).fail(true);
    vi.advanceTimersByTime(60_000);
    expect(FakeEventSource.instances).toHaveLength(6);
    vi.useRealTimers();
  });
});
