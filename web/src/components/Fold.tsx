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

/** Spec §3.1: closed by default, remembered per id. M3: the summary click is native (no
 *  preventDefault); `open` follows the element's own toggle via onToggle, so keyboard activation
 *  (space/enter on the native summary) and a click behave the same way. */
export function Fold({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  const [open, setOpen] = useState(() => readOpen(id));
  return (
    <details
      className="fold"
      open={open}
      onToggle={(e) => {
        const next = e.currentTarget.open;
        setOpen(next);
        writeOpen(id, next);
      }}
    >
      <summary>{label}</summary>
      <div className="fold-body">{children}</div>
    </details>
  );
}
