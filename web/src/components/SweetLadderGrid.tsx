import { useState } from 'react';
import type { SweetRow } from '../api/types';
import {
  GLYPHS, GLYPH_NAME, GLYPH_ORDER, GRID_HINT, type LadderGridRow, type SweetDim, type SweetSide, bucketText, cellDetail, glyphKey, ladderGrid,
} from '../lib/sweet';

const cellId = (row: LadderGridRow, scenario: string) => `${row.dim}|${row.bucket}|${scenario}`;

/**
 * Spec 7a §8: buckets × LADDER rungs as a plain HTML table of buttons (no SVG, so no aspect-ratio
 * trouble). At 360 px: 88 px labels + 6 × 36 px cells = 304 px inside 328. Tap or focus a cell for one
 * detail line under the grid (not a popover); the legend sits beside it, in words.
 */
export function SweetLadderGrid({ rows, side, dim, label, only }: {
  rows: SweetRow[];
  side: SweetSide;
  dim: SweetDim;
  label: string;
  only?: string[];
}) {
  const g = ladderGrid(rows, side, dim, only);
  const [pick, setPick] = useState<string | null>(null);
  let detail = GRID_HINT;
  for (const row of g.rows) {
    for (const col of g.columns) {
      if (cellId(row, col.scenario) === pick) detail = cellDetail(row, col, g.cell(row, col.scenario));
    }
  }
  return (
    <div className="lgrid-wrap">
      <table className="lgrid" aria-label={label}>
        <thead>
          <tr>
            <th scope="col" className="lg-row">ms</th>
            {g.columns.map((c) => (
              <th key={c.scenario} scope="col" className="lg-col" title={c.scenario}>
                {c.latency_ms}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {g.rows.map((row) => (
            <tr key={`${row.dim}|${row.bucket}`}>
              <th scope="row" className="lg-row">{bucketText(row.dim, row.bucket)}</th>
              {g.columns.map((c) => {
                const k = glyphKey(g.cell(row, c.scenario));
                const id = cellId(row, c.scenario);
                return (
                  <td key={c.scenario}>
                    <button
                      type="button"
                      className={`lg-cell g-${k}`}
                      aria-label={`${bucketText(row.dim, row.bucket)} at ${c.scenario} (${c.latency_ms} ms): ${GLYPH_NAME[k]}`}
                      aria-pressed={pick === id}
                      onClick={() => setPick(id)}
                      onFocus={() => setPick(id)}
                    >
                      {GLYPHS[k]}
                    </button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <ul className="plain lg-legend" aria-label="grid legend">
        {GLYPH_ORDER.map((k) => (
          <li key={k}>
            <span aria-hidden="true">{GLYPHS[k]}</span> {GLYPH_NAME[k]}
          </li>
        ))}
      </ul>
      <p className="lg-detail" aria-live="polite">{detail}</p>
    </div>
  );
}
