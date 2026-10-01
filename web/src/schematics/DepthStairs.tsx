import { Money } from '../components/Money';
import { formatDecimal } from '../lib/money';
import type { WalkModel } from '../wasm/types';
import { geoPos, PLOT_H as H, PLOT_W as W } from './geo';

/**
 * Parent §5.7 (d): the cost of each next basket as a staircase against what that basket pays (1 for the
 * YES rule; active legs - 1 for the nested NO rule, so the payoff is read per step). Between cost and payoff
 * each step is shaded: teal where it pays (up to Q*), salmon past Q* where it costs more than it pays
 * (walk_steps keeps those). Π, Π before fees and Q* are the engine's own figures, in HTML; the step costs
 * are for drawing only (7b-B R19), so no total is ever summed from them.
 */
export function DepthStairs({ model }: { model: WalkModel }) {
  const last = model.steps.length > 0 ? model.steps[model.steps.length - 1].q_to : '0';
  const qMax = Math.max(1, geoPos(last));
  const yMax = Math.max(0, ...model.steps.flatMap((s) => [geoPos(s.cost), geoPos(s.payoff)])) * 1.1 || 1;
  const sx = (q: string) => Math.min(W, (geoPos(q) / qMax) * W);
  const sy = (v: number) => H - (v / yMax) * H;
  const qs = sx(model.q_star);
  return (
    <figure className="dstairs" aria-label="the cost of each next basket against what it pays">
      <p className="ds-big">
        <span>
          Π <Money value={model.pi} signed cents />
        </span>
        <span>{`Q* ${formatDecimal(model.q_star)}`}</span>
      </p>
      <figcaption className="dim">
        on the table, if every level stayed put · <span className="ds-gross">before fees <Money value={model.pi_gross} signed cents /></span>
      </figcaption>
      <svg className="ds-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="the cost staircase">
        {model.steps.map((s, i) => {
          const c = sy(geoPos(s.cost));
          const p = sy(geoPos(s.payoff));
          const x = sx(s.q_from);
          const w = Math.max(0, sx(s.q_to) - x);
          if (c === p) return null;
          return <rect key={i} className={s.profitable ? 'ds-pi' : 'ds-loss'} x={x} y={Math.min(c, p)} width={w} height={Math.abs(c - p)} />;
        })}
        {model.steps.map((s, i) => {
          const y = sy(geoPos(s.cost));
          return <line key={`c${i}`} className="ds-cost" x1={sx(s.q_from)} x2={sx(s.q_to)} y1={y} y2={y} />;
        })}
        {model.steps.map((s, i) => {
          const y = sy(geoPos(s.payoff));
          return <line key={`p${i}`} className="ds-pay" x1={sx(s.q_from)} x2={sx(s.q_to)} y1={y} y2={y} />;
        })}
        <line className="ds-qstar" x1={qs} x2={qs} y1={0} y2={H} />
      </svg>
      <ol className="ds-axis" aria-hidden="true">
        <li>0</li>
        <li>{`${formatDecimal(last)} shares`}</li>
      </ol>
      <ul className="plain ds-legend" aria-label="legend">
        <li><span className="ds-sw pi" aria-hidden="true" />the next basket pays more than it costs</li>
        <li><span className="ds-sw loss" aria-hidden="true" />past Q*: it costs more than it pays</li>
        <li><span className="ds-sw cost" aria-hidden="true" />what the next basket costs</li>
        <li><span className="ds-sw pay" aria-hidden="true" />what a basket pays</li>
        <li><span className="ds-sw qstar" aria-hidden="true" />Q*: the last basket worth buying</li>
      </ul>
    </figure>
  );
}
