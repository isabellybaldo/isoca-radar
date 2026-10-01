import type { PayoffModel } from './models';

/** Parent spec §5.7 (b) as an HTML table (no SVG, nothing to distort): 1 where a token pays $1, the
 *  basket row, and ✗ on a column where the basket pays less than it guarantees. */
export function PayoffGrid({ model, label }: { model: PayoffModel; label: string }) {
  return (
    <figure className="payoff">
      <div className="scroll">
        <table aria-label={label}>
          <thead>
            <tr>
              <th scope="col">pays $ if →</th>
              {model.cols.map((c, i) => (
                <th key={c} scope="col" className={model.broken[i] ? 'broken' : undefined}>
                  {model.broken[i] ? `${c} ✗` : c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {model.rows.map((r) => (
              <tr key={r.name}>
                <th scope="row">{r.name}</th>
                {r.cells.map((v, i) => (
                  <td key={model.cols[i]} className={v === 1 ? 'pay' : 'nopay'}>
                    {v}
                  </td>
                ))}
              </tr>
            ))}
            <tr className="basket">
              <th scope="row">basket</th>
              {model.basket.map((b, i) => (
                <td key={model.cols[i]} className={model.broken[i] ? 'broken' : 'pay'}>
                  {b}
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
      {model.s > model.rows.length && <figcaption className="dim">{`first ${model.rows.length} of ${model.s} outcomes shown`}</figcaption>}
    </figure>
  );
}
