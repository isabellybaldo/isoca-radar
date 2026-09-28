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
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  rowClass?: (row: T) => string | undefined;
}) {
  return (
    <div className="scroll">
      <table>
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
            <tr key={rowKey(r)} className={rowClass?.(r)}>
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
