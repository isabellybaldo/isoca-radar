import type { ReactNode } from 'react';

export interface Column<T> {
  key: string;
  label: string;
  render: (row: T) => ReactNode;
  className?: (row: T) => string | undefined;
  /** M6: number columns (money, qty) get white-space: nowrap; font-variant-numeric: tabular-nums,
   *  same as everywhere else numbers are written, so they never wrap mid-value. */
  num?: boolean;
}

function tdClass<T>(c: Column<T>, row: T): string | undefined {
  return [c.num ? 'num' : null, c.className?.(row) ?? null].filter(Boolean).join(' ') || undefined;
}

export function Table<T>({
  columns,
  rows,
  rowKey,
  rowClass,
  className,
  onRowClick,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  rowClass?: (row: T) => string | undefined;
  /** Hook for a page's own layout of this table (e.g. Radar's phone cards). */
  className?: string;
  /** Spec 7a §9.3: a row click sets the explain panel's selection; links inside still navigate. */
  onRowClick?: (row: T) => void;
}) {
  return (
    <div className="scroll">
      <table className={className}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} scope="col">
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr
              key={rowKey(r)}
              className={[rowClass?.(r), onRowClick ? 'pick' : null].filter(Boolean).join(' ') || undefined}
              onClick={onRowClick ? () => onRowClick(r) : undefined}
            >
              {columns.map((c) => (
                <td key={c.key} className={tdClass(c, r)}>
                  {c.render(r)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
