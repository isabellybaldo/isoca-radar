import { useEffect, useRef, useState } from 'react';

const hasFrames = (): boolean => typeof globalThis.requestAnimationFrame === 'function';

/**
 * 09-27 §6.2: every control change recomputes synchronously, at most once per animation frame while
 * dragging. Returns `value`, updated at most once per frame and always ending on the latest one.
 */
export function useFrameThrottle<T>(value: T): T {
  const [out, setOut] = useState(value);
  const latest = useRef(value);
  const pending = useRef<{ id: number; frame: boolean } | null>(null);
  useEffect(() => {
    latest.current = value;
    if (Object.is(value, out) || pending.current !== null) return;
    const done = () => {
      pending.current = null;
      setOut(latest.current);
    };
    const frame = hasFrames();
    const id = frame ? globalThis.requestAnimationFrame(() => done()) : (setTimeout(done, 16) as unknown as number);
    pending.current = { id, frame };
  }, [value, out]);
  useEffect(
    () => () => {
      const p = pending.current;
      if (p === null) return;
      if (p.frame) globalThis.cancelAnimationFrame(p.id);
      else clearTimeout(p.id);
      pending.current = null;
    },
    [],
  );
  return out;
}
