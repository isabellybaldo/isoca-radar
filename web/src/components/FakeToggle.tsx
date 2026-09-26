import { useEffect, useRef, useState } from 'react';

export const NIGHT_ONLY = 'Night mode only.';

/** The ☾/☀ switch that switches nothing (spec §5). It must never touch the theme. */
export function FakeToggle() {
  const [tip, setTip] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const onClick = () => {
    setTip(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setTip(false), 1800);
  };
  return (
    <button
      type="button"
      className={tip ? 'toggle wiggle' : 'toggle'}
      onClick={onClick}
      aria-label="Toggle theme"
      aria-describedby={tip ? 'night-only' : undefined}
    >
      ☾ / ☀
      {tip && (
        <span id="night-only" className="tip" role="tooltip">
          {NIGHT_ONLY}
        </span>
      )}
    </button>
  );
}
