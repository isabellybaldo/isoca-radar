import type { ReactNode } from 'react';

export interface Tile {
  label: string;
  value: ReactNode;
  title?: string;
  tone?: 'pos' | 'neg';
}

/** Big numbers with a small sentence-case label (spec §3.1): only where the number is the answer
 *  (§3.3). M8: a real <ul>/<li> list (dl/dt/dd is not a listitem-of-listitems a11y checkers accept)
 *  — label-then-value in the DOM, the value visually on top via .tile's column-reverse. */
export function Tiles({ items, dim }: { items: Tile[]; dim?: boolean }) {
  return (
    <ul className={dim ? 'tiles dim' : 'tiles'}>
      {items.map((t) => (
        <li key={t.label} className="tile">
          <span className="tile-label">{t.label}</span>
          <span className={['tile-value', t.tone].filter(Boolean).join(' ')} title={t.title}>
            {t.value}
          </span>
        </li>
      ))}
    </ul>
  );
}
