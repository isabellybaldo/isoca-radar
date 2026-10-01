import { act, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fromTick, toTick } from './defaults';
import { useFrameThrottle } from './useFrameThrottle';

afterEach(() => vi.unstubAllGlobals());

describe('useFrameThrottle (09-27 §6.2: one engine call per frame while dragging)', () => {
  it('passes one value per frame, and it is the latest', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal('cancelAnimationFrame', () => {});
    const seen: number[] = [];
    function Probe({ v }: { v: number }) {
      seen.push(useFrameThrottle(v));
      return null;
    }
    const { rerender } = render(<Probe v={1} />);
    rerender(<Probe v={2} />);
    rerender(<Probe v={3} />);
    rerender(<Probe v={4} />);
    expect(frames).toHaveLength(1);
    act(() => {
      frames.shift()?.(0);
    });
    expect(seen.at(-1)).toBe(4);
    expect([...new Set(seen)]).toEqual([1, 4]);
    expect(frames).toHaveLength(0);
  });
});

describe('useFrameThrottle: lifecycle', () => {
  function probe(seen: number[]) {
    return function Probe({ v }: { v: number }) {
      seen.push(useFrameThrottle(v));
      return null;
    };
  }

  it('unmounting with a pending frame cancels it, and no update follows', () => {
    const frames: FrameRequestCallback[] = [];
    const cancelled: number[] = [];
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal('cancelAnimationFrame', (id: number) => cancelled.push(id));
    const seen: number[] = [];
    const Probe = probe(seen);
    const { rerender, unmount } = render(<Probe v={1} />);
    rerender(<Probe v={2} />);
    expect(frames).toHaveLength(1);
    const before = seen.length;
    unmount();
    expect(cancelled).toEqual([1]);
    act(() => {
      frames.shift()?.(0);
    });
    expect(seen).toHaveLength(before);
  });

  it('falls back to a 16 ms timer when there is no requestAnimationFrame', () => {
    vi.useFakeTimers();
    try {
      vi.stubGlobal('requestAnimationFrame', undefined);
      const seen: number[] = [];
      const Probe = probe(seen);
      const { rerender } = render(<Probe v={1} />);
      rerender(<Probe v={2} />);
      rerender(<Probe v={3} />);
      expect(seen.at(-1)).toBe(1);
      act(() => {
        vi.advanceTimersByTime(16);
      });
      expect(seen.at(-1)).toBe(3);
      expect([...new Set(seen)]).toEqual([1, 3]);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('ticks', () => {
  it('moves prices on the 0.01 grid by string math, clamped to 0.01–0.99', () => {
    expect(toTick('0.55')).toBe(55);
    expect(toTick('0.5')).toBe(50);
    expect(fromTick(55)).toBe('0.55');
    expect(fromTick(5)).toBe('0.05');
    expect(fromTick(0)).toBe('0.01');
    expect(fromTick(140)).toBe('0.99');
    expect(fromTick(Number.NaN)).toBe('0.50');
    expect(toTick('x')).toBe(50);
  });
});
