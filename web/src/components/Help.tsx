import { useEffect, useId, useRef, useState } from 'react';
import { HELP, type HelpId } from '../help';

/** The (?) button (spec §8.3): click, Enter or Space toggles; Escape closes and returns focus. */
export function Help({ id }: { id: HelpId }) {
  const [open, setOpen] = useState(false);
  const popId = useId();
  const btn = useRef<HTMLButtonElement>(null);
  const h = HELP[id];
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        btn.current?.focus();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);
  return (
    <span className="help">
      <button
        ref={btn}
        type="button"
        className="help-btn"
        aria-label={`Help: ${h.title}`}
        aria-expanded={open}
        aria-controls={popId}
        onClick={() => setOpen((o) => !o)}
      >
        (?)
      </button>
      <span id={popId} className="help-pop" role="note" aria-live="polite" hidden={!open}>
        {open && (
          <>
            <b>{h.title}</b> {h.text}
          </>
        )}
      </span>
    </span>
  );
}
