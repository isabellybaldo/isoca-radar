import type { ReactNode } from 'react';

export interface Tile {
  label: string;
  value: ReactNode;
  title?: string;
  tone?: 'pos' | 'neg';
}

/** Big numbers with a small sentence-case label (spec §3.1): only where the number is the answer (§3.3). */
export function Tiles({ items, dim }: { items: Tile[]; dim?: boolean }) {
  return (
    <dl className={dim ? 'tiles dim' : 'tiles'} role="list">
      {items.map((t) => (
        <div key={t.label} className="tile" role="listitem">
          <dt>{t.label}</dt>
          <dd className={t.tone} title={t.title}>
            {t.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
