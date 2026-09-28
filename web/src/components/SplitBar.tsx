import { formatMoneyCents } from '../lib/money';

export interface SplitPart {
  name: string;
  value: string | null;
  tone: 'p0' | 'p3';
}

/** Redesign §4.2: the edge on the table split by side, values written on the bar's ends. */
export function SplitBar({ parts, label }: { parts: SplitPart[]; label: string }) {
  const sized = parts.filter((p) => p.value !== null && Number(p.value) > 0);
  return (
    <figure className="split" aria-label={label}>
      <div className="split-labels">
        {parts.map((p) => (
          <span key={p.name} className="num">{`${p.name} ${p.value === null ? '—' : formatMoneyCents(p.value)}`}</span>
        ))}
      </div>
      {sized.length > 0 && (
        <div className="split-bar">
          {sized.map((p) => (
            <span key={p.name} className={`split-part sw ${p.tone}`} style={{ flexGrow: Number(p.value) }} />
          ))}
        </div>
      )}
    </figure>
  );
}
