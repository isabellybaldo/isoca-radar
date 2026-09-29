import { useEffect, useState } from 'react';

const query = (q: string): MediaQueryList | null => (typeof globalThis.matchMedia === 'function' ? globalThis.matchMedia(q) : null);

/** A CSS media query as state; false where matchMedia is missing (jsdom). */
export function useMedia(q: string): boolean {
  const [on, setOn] = useState(() => query(q)?.matches ?? false);
  useEffect(() => {
    const m = query(q);
    if (!m) return;
    setOn(m.matches);
    const f = () => setOn(m.matches);
    m.addEventListener('change', f);
    return () => m.removeEventListener('change', f);
  }, [q]);
  return on;
}
