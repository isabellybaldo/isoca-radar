import { type ReactNode, useState } from 'react';

const key = (id: string) => `fold:${id}`;

function readOpen(id: string): boolean {
  try {
    return localStorage.getItem(key(id)) === '1';
  } catch {
    return false;
  }
}

function writeOpen(id: string, open: boolean): void {
  try {
    if (open) localStorage.setItem(key(id), '1');
    else localStorage.removeItem(key(id));
  } catch {
    // storage blocked (private window, preview): the fold still works, it just forgets
  }
}

/** Spec §3.1: closed by default, remembered per id. Controlled so the state never depends on the
 *  browser's own <details> toggle (jsdom and old browsers differ); the summary stays keyboard-native. */
export function Fold({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  const [open, setOpen] = useState(() => readOpen(id));
  return (
    <details className="fold" open={open}>
      <summary
        onClick={(e) => {
          e.preventDefault();
          const next = !open;
          setOpen(next);
          writeOpen(id, next);
        }}
      >
        {label}
      </summary>
      <div className="fold-body">{children}</div>
    </details>
  );
}
