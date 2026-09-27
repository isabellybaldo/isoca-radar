import { useEffect, useId, useRef, useState } from 'react';
import { HELP, type HelpId } from '../help';

/**
 * The (?) button (spec §8.3): click, Enter or Space toggles; Escape closes and returns focus; a
 * click or tap outside the button and its popover closes it too.
 */
export function Help({ id }: { id: HelpId }) {
  const [open, setOpen] = useState(false);
  const popId = useId();
  const btn = useRef<HTMLButtonElement>(null);
  const root = useRef<HTMLSpanElement>(null);
  const h = HELP[id];
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false);
        btn.current?.focus();
      }
    };
    const onPointer = (e: Event) => {
      if (e.target instanceof Node && root.current?.contains(e.target)) return;
      setOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onPointer);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
    };
  }, [open]);
  return (
    <span className="help" ref={root}>
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
