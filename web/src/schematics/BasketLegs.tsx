import { Money } from '../components/Money';
import { formatDecimal, formatMoneyCents, signOf } from '../lib/money';
import type { BasketModel } from '../wasm/types';
import { geoPos } from './geo';

const W = 300;
const H = 120;

export interface Verdict {
  big: string;
  tone: 'pos' | 'neg' | null;
}

/** Spec 7b §6.2 f and 7b-A's contract: the big number is net_cash, "caught, and lost $x" when it is below
 *  zero (decided on the decimal string's sign), "filled, $x on paper" otherwise. Money through money.ts only. */
export function basketVerdict(m: BasketModel): Verdict {
  const net = m.cash.net_cash;
  const s = signOf(net);
  if (s < 0) return { big: `caught, and lost ${formatMoneyCents(net.trim().replace(/^-/, ''))}`, tone: 'neg' };
  return { big: `filled, ${formatMoneyCents(net, { signed: true })} on paper`, tone: s > 0 ? 'pos' : null };
}

/**
 * Spec 7b §6.2 f: per leg, the planned shares (outline) and what filled; the full baskets (q_eff) as one line
 * across the legs; above it each leg's excess, split into the part sold back at the unwind price and the part
 * still held (cost, not cash). Labels and money are HTML under the drawing.
 */
export function BasketLegs({ model, names }: { model: BasketModel; names: string[] }) {
  const n = Math.max(1, model.legs.length);
  const slot = W / n;
  const bw = slot * 0.5;
  const plan = geoPos(model.q_target);
  const top = Math.max(1, plan, ...model.legs.map((l) => geoPos(l.buy.filled)));
  const sy = (q: number) => H - (q / top) * (H - 4);
  const qe = geoPos(model.q_eff);
  const v = basketVerdict(model);
  return (
    <figure className="blegs" aria-label="each leg: planned, filled, and the excess sold back or held">
      <p className={v.tone === null ? 'bl-big' : `bl-big ${v.tone}`}>{v.big}</p>
      <svg className="bl-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="planned and filled shares per leg">
        {model.legs.map((l, i) => {
          const x = i * slot + (slot - bw) / 2;
          const base = Math.min(geoPos(l.buy.filled), qe);
          const sold = geoPos(l.unwind.filled);
          const held = geoPos(model.cash.held_unsold[i]?.qty ?? '0');
          return (
            <g key={i} data-leg={names[i] ?? String(i + 1)}>
              <rect className="bl-plan" x={x} y={sy(plan)} width={bw} height={H - sy(plan)} />
              <rect className="bl-fill" x={x} y={sy(base)} width={bw} height={H - sy(base)} />
              <rect className="bl-sold" x={x} y={sy(base + sold)} width={bw} height={sy(base) - sy(base + sold)} />
              <rect className="bl-held" x={x} y={sy(base + sold + held)} width={bw} height={sy(base + sold) - sy(base + sold + held)} />
            </g>
          );
        })}
        <line className="bl-qeff" x1={0} x2={W} y1={sy(qe)} y2={sy(qe)} />
      </svg>
      <ol className="bl-axis" aria-label="legs">
        {model.legs.map((l, i) => {
          const h = model.cash.held_unsold[i];
          return (
            <li key={i}>
              <b>{`leg ${names[i] ?? i + 1}`}</b>
              <span>{`${formatDecimal(l.buy.filled)} of ${formatDecimal(l.buy.requested)}`}</span>
              {signOf(l.unwind.filled) > 0 && <span>{`sold ${formatDecimal(l.unwind.filled)}${l.unwind.vwap === null ? '' : ` at ${l.unwind.vwap}`}`}</span>}
              {signOf(l.unwind.requested) > 0 && signOf(l.unwind.filled) === 0 && <span>{`${formatDecimal(l.unwind.requested)} to sell, no order went out`}</span>}
              {h !== undefined && signOf(h.qty) > 0 && <span>{`held ${formatDecimal(h.qty)}, ${formatMoneyCents(h.basis)} cost, not cash`}</span>}
            </li>
          );
        })}
      </ol>
      <p className="dim">{`the line: ${formatDecimal(model.q_eff)} full baskets (q_eff) · planned ${formatDecimal(model.q_target)} of the ${formatDecimal(model.q_open)} the book at open could fill`}</p>
      <dl className="bl-cash">
        <div><dt>spent</dt><dd><Money value={model.cash.spent} cents /></dd></div>
        <div><dt>sold back</dt><dd><Money value={model.cash.unwind_back} cents /></dd></div>
        <div><dt>converted</dt><dd><Money value={model.cash.convert_back} cents /></dd></div>
        <div><dt>net cash, on paper</dt><dd><Money value={model.cash.net_cash} signed tone cents /></dd></div>
        <div><dt>of which fees</dt><dd><Money value={model.cash.fees} cents /></dd></div>
      </dl>
      <p className="dim">every dollar here is on paper, from the replayed books</p>
      {model.value_at_resolution !== null && (
        <p className="dim bl-resolve">
          if a listed leg wins: <Money value={model.value_at_resolution} cents /> at resolution, on paper, not yet cash
        </p>
      )}
      <ul className="plain bl-legend" aria-label="legend">
        <li><span className="bl-sw fill" aria-hidden="true" />in full baskets</li>
        <li><span className="bl-sw sold" aria-hidden="true" />excess sold back at the unwind price</li>
        <li><span className="bl-sw held" aria-hidden="true" />excess still held</li>
        <li><span className="bl-sw plan" aria-hidden="true" />planned</li>
      </ul>
    </figure>
  );
}
