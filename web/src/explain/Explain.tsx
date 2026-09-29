import { type RefObject, useEffect, useRef, useState } from 'react';
import { api } from '../api/client';
import { useApi } from '../api/useApi';
import { useTicker } from '../lib/hooks';
import { type Selection, useExplainCtx } from './ExplainContext';
import { HERO_ID, questionsFor } from './questions';
import { readQuestion, writeQuestion } from './storage';
import { useMedia } from './useMedia';

/** Spec 7a §9.3: the panel's one fetch is polled every 60 s while open, never while closed. */
export const LADDER_POLL_MS = 60_000;
/** Spec 7a §9.1: docked beside the app at ≥ 1,500 px, an overlay drawer below. */
export const DOCK_QUERY = '(min-width: 1500px)';

function selectionLabel(s: NonNullable<Selection>): string {
  return s.kind === 'episode' ? (s.episode.event_title ?? s.episode.id) : (s.position.event_title ?? s.position.market_id);
}

/** Mounted only while open, so its ticker and its fetch stop when the panel closes. */
function Panel({ mode, focusOnMount, onClose }: { mode: 'dock' | 'drawer'; focusOnMount: RefObject<boolean>; onClose: () => void }) {
  const { page, data, selection, select } = useExplainCtx();
  const tick = useTicker(LADDER_POLL_MS);
  const ladder = useApi((s) => api.paperLadder(s), [], [tick]);
  const aside = useRef<HTMLElement>(null);
  const [cur, setCur] = useState<string | null>(() => readQuestion() ?? HERO_ID);
  useEffect(() => {
    if (focusOnMount.current) {
      aside.current?.focus();
      focusOnMount.current = false;
    }
  }, [focusOnMount]);
  const qs = questionsFor(page);
  // A remembered question that is not on this page falls back to the hero; null = all closed.
  const active = cur === null ? null : qs.some((q) => q.id === cur) ? cur : HERO_ID;
  const s = { page, data, selection };
  return (
    <aside id="explain-panel" ref={aside} tabIndex={-1} aria-label="explain" className="explain" data-mode={mode}>
      <div className="explain-head">
        <h2>EXPLAIN</h2>
        <button type="button" className="explain-close" onClick={onClose}>
          close
        </button>
      </div>
      {selection && (
        <p className="explain-sel">
          {`as selected: ${selectionLabel(selection)} · `}
          <button type="button" onClick={() => select(null)}>
            back to the page
          </button>
        </p>
      )}
      <ul className="explain-qs">
        {qs.map((q) => {
          const on = q.id === active;
          return (
            <li key={q.id}>
              <button
                type="button"
                className="explain-q"
                aria-expanded={on}
                onClick={() => {
                  const next = on ? null : q.id;
                  setCur(next);
                  if (next !== null) writeQuestion(next);
                }}
              >
                {q.q}
              </button>
              {on && (
                <div className="explain-a">
                  <q.Answer s={s} ladder={ladder} />
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

/** Spec 7a §9.1: a vertical EXPLAIN ▸ tab on every page; Esc closes and returns focus to it. */
export function Explain() {
  const { isOpen, setOpen } = useExplainCtx();
  const docked = useMedia(DOCK_QUERY);
  const tab = useRef<HTMLButtonElement>(null);
  const focusOnMount = useRef(false); // focus the panel when the owner opens it, not on a remembered open
  const close = () => {
    setOpen(false);
    tab.current?.focus();
  };
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return; // another Esc user (a (?) popover) took it
      // Esc typed in a field on the page belongs to that field, not to the panel.
      const t = e.target;
      if (t instanceof HTMLElement && t.matches('input, textarea, select') && !t.closest('#explain-panel')) return;
      setOpen(false);
      tab.current?.focus();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [isOpen, setOpen]);
  return (
    <>
      <button
        ref={tab}
        type="button"
        className="explain-tab"
        aria-expanded={isOpen}
        aria-controls="explain-panel"
        // D-06: docked and open, the tab joins the flex row on the panel's left edge instead of floating over it.
        data-dock={docked && isOpen ? '' : undefined}
        onClick={() => {
          focusOnMount.current = !isOpen;
          setOpen(!isOpen);
        }}
      >
        EXPLAIN ▸
      </button>
      {isOpen && <Panel mode={docked ? 'dock' : 'drawer'} focusOnMount={focusOnMount} onClose={close} />}
    </>
  );
}
