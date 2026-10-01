import { PLOT_W as W, STACK_FULL as FULL, stackSpans } from './geo';

const H = 48;
const BAR_TOP = 10;
const BAR_H = 28;

/** A share of the width as a column width. A `width` attribute on a col is a presentational hint, not a style
 *  attribute: the public page's CSP (no inline style) lets it through, and no element carries `style`. */
const share = (ticks: number, hi: number) => `${((ticks / hi) * 100).toFixed(4)}%`;

/**
 * Spec 2026-10-01 (task 7), the story's chapter 3: the best ask of every leg of a basket, laid end to end
 * against the one dollar a complete basket pays. Where the prices stop short of the line the space between is
 * the gap, drawn as its own dashed rect; past the line the overshoot is its own rect. Geometry only: the prices
 * become numbers in `stackSpans` (geo.ts) to be placed, and are written under the bar as the strings given. No
 * sum, no difference and no `$`: the amount is the engine's, shown by whoever draws this.
 * The label row is a layout table whose columns are the bar's own breakpoints (each leg's end, the dollar line,
 * the axis' end), so every label sits under its segment and "one dollar" ends on the line at any width. A price
 * that takes no width (zero, negative, unreadable) is named in a line of its own.
 */
export function PriceStack({ legs }: { legs: { name: string; price: string }[] }) {
  const { spans, end, hi } = stackSpans(legs.map((l) => l.price));
  const x = (t: number) => (t / hi) * W;
  const drawn = legs.map((l, i) => ({ ...l, ...spans[i], i })).filter((l) => l.to > l.from);
  const off = legs.filter((_, i) => spans[i].to === spans[i].from);
  const points = [...new Set([...drawn.map((l) => l.to), FULL, hi])].sort((a, b) => a - b);
  /** How many columns end in (from, to]. */
  const cols = (from: number, to: number) => points.filter((p) => p > from && p <= to).length;
  return (
    <figure className="pstack" aria-label="what the basket costs against what it pays">
      <svg className="ps-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="the prices of a basket laid end to end against the one dollar it pays">
        {drawn.map((l) => (
          <rect key={l.i} className="ps-leg" x={x(l.from)} y={BAR_TOP} width={x(l.to) - x(l.from)} height={BAR_H} />
        ))}
        {end < FULL && <rect className="ps-gap" x={x(end)} y={BAR_TOP} width={x(FULL) - x(end)} height={BAR_H} />}
        {end > FULL && <rect className="ps-over" x={x(FULL)} y={BAR_TOP} width={x(end) - x(FULL)} height={BAR_H} />}
        <line className="ps-dollar" x1={x(FULL)} x2={x(FULL)} y1={0} y2={H} />
      </svg>
      <table className="ps-labels" role="presentation">
        <colgroup>
          {points.map((p, i) => (
            <col key={p} width={share(p - (points[i - 1] ?? 0), hi)} />
          ))}
        </colgroup>
        <tbody>
          <tr>
            {drawn.map((l) => (
              <td key={l.i} className="ps-leg-l" colSpan={cols(l.from, l.to)}>
                <b>{l.name}</b>
                <span>{l.price}</span>
              </td>
            ))}
            <td colSpan={cols(end, hi)} />
          </tr>
          <tr>
            <td className="ps-one" colSpan={cols(0, FULL)}>one dollar</td>
            <td colSpan={cols(FULL, hi)} />
          </tr>
        </tbody>
      </table>
      {off.length > 0 && <p className="dim ps-off">{`not drawn: ${off.map((l) => `${l.name} ${l.price}`).join(' · ')}`}</p>}
      <ul className="plain ps-legend" aria-label="legend">
        <li><span className="ps-sw cost" aria-hidden="true" />what the basket costs</li>
        {end < FULL && <li><span className="ps-sw gap" aria-hidden="true" />the gap</li>}
        {end > FULL && <li><span className="ps-sw over" aria-hidden="true" />past one dollar</li>}
      </ul>
    </figure>
  );
}
