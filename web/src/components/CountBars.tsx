import { groupThousands } from '../lib/money';

/** Redesign §4.4: counts as bars with the number written at the end (integrity by kind). */
export function CountBars({ items, label }: { items: { label: string; count: number }[]; label: string }) {
  const max = Math.max(1, ...items.map((i) => i.count));
  return (
    <ul className="plain countbars" aria-label={label}>
      {items.map((i) => (
        <li key={i.label}>
          <span className="cb-label">{i.label}</span>
          <span className="cb-track">
            <span className="cb-bar" style={{ width: `${Math.round((i.count / max) * 100)}%` }} />
          </span>
          <span className="cb-count num">{groupThousands(String(i.count))}</span>
        </li>
      ))}
    </ul>
  );
}
