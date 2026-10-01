import { useId } from 'react';
import { Empty } from './Empty';
import { geo } from './geo';
import { Fold } from '../components/Fold';
import { Money } from '../components/Money';
import { SweetLadderGrid } from '../components/SweetLadderGrid';
import {
  type LadderModel, MIN_EXEC, NOT_MONOTONE, NO_MARK_YET, NO_RUNGS, type Rung, SLOTS, asOfText, breakEvenText, epochText, extraText, latencyX, rungCountText, rungMsText,
  slotX, stackRung, usBand, usText, yDomain,
} from '../lib/ladder';

/** Spec 7a §7.2 / §12: one epoch is one path (no interval), and the unwind rule is named. */
export const LADDER_CAPTION = 'One epoch is one path, so there is no interval. A missed leg sells the rest at the bid.';
export const AXIS_TITLE = 'ms from the exchange showing the edge to our order matching';
/** Spec §17: the bars are since the epoch start; the fold is per trade attempt over 30 days, across epochs. */
export const MARKETS_CAPTION = 'Sweet spots: per trade attempt over the last 30 days, across epochs. Not the same number as the bars above.';
export const MARKETS_WARMING = 'warming: part of the 30 days is still being filled.';
export const MARKETS_RECOMPUTING = 'recomputing after the ladder';
export const MARKETS_MISSING = 'no markets data';
/** The sentence lives with the model's other words (lib/ladder.ts: it is also the headline's line when no rung is marked). */
export { NO_RUNGS };
/** Under a rung with no account the axis shows this mark: the words would run into the next rung at 360 px. The
 *  dashed box above it is the drawing; the legend and the screen-reader text carry the words. */
export const NO_ACCOUNT_MARK = '–';
export const NO_ACCOUNT_LEGEND = 'dashed box: no account at this latency';
export const TABLE_CAPTION = 'on paper, since the epoch start, every sum cut at the same mark';

const W = 700;
const H = 200;
const BAR = 40;
const LEGEND: [string, string][] = [
  ['gains', 'basket gains (converts)'],
  ['leg_risk', 'leg-risk cost (unwinds)'],
  ['settle', 'settled at resolution'],
  ['unrealized', 'unrealized, marked now'],
  ['net', 'net on paper'],
  ['extra', 'an extra, net on paper'],
];
const HEADS = ['RUNG', 'N', 'LEG RISK', 'MISSED', 'BASKET GAINS', 'LEG-RISK COST', 'SETTLED', 'UNREALIZED', 'NET'];

function Markets({ model }: { model: LadderModel }) {
  const { status, rows } = model.markets;
  if (status === 'under_review') return <Empty text={MARKETS_RECOMPUTING} />;
  if (status === 'missing') return <Empty text={MARKETS_MISSING} />;
  return (
    <>
      <p className="dim">{MARKETS_CAPTION}</p>
      {status === 'warming' && <p className="dim">{MARKETS_WARMING}</p>}
      <SweetLadderGrid rows={rows} side="no" dim="category" label="which markets" />
    </>
  );
}

function LadderTable({ rungs }: { rungs: Rung[] }) {
  if (rungs.length === 0) return <Empty text={NO_RUNGS} />;
  return (
    <div className="scroll">
      <table>
        <caption className="dim">{TABLE_CAPTION}</caption>
        <thead>
          <tr>
            {HEADS.map((h) => (
              <th key={h} scope="col">{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rungs.map((r) => (
            <tr key={r.scenario}>
              <td>{r.scenario}</td>
              <td className="num">{r.n_exec}</td>
              <td className="num">{r.n_leg_risk}</td>
              <td className="num">{r.n_missed}</td>
              <td className="num"><Money value={r.gains} signed cents /></td>
              <td className="num"><Money value={r.leg_risk} signed cents /></td>
              <td className="num"><Money value={r.settle} signed cents /></td>
              <td className="num"><Money value={r.unrealized} signed cents /></td>
              <td className="num"><Money value={r.net} signed tone cents /></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Spec 7a §7.2, "at what latency would this pay?": one stacked bar per rung on an ordinal axis (basket
 * gains up, leg-risk cost down, settle and unrealized by their sign, unrealized hatched), a net dot,
 * the zero line, the "us" band (stopped before the extras' slot), the extras as squares beside the 300 ms rung, and the break-even line. The one big
 * number is the break-even; every other label is HTML beside the SVG (preserveAspectRatio="none").
 * Pure: it draws `model` and never fetches.
 */
export function LatencyLadder({ model }: { model: LadderModel }) {
  const hatch = `ll-hatch-${useId().replace(/[^A-Za-z0-9_-]/g, '')}`;
  const present = model.columns.flatMap((c) => (c.kind === 'rung' ? [c.rung] : []));
  const d = yDomain([...present, ...model.extras]);
  const sy = (v: number) => ((d.hi - v) / (d.hi - d.lo)) * H;
  const zero = sy(0);
  const be = breakEvenText(model);
  const thin = model.breakEven.kind === 'thin';
  const band = model.measured ? usBand(model.measured) : null;
  const beX = model.breakEven.kind === 'at' ? latencyX(model.breakEven.latency_ms) * W : null;
  // The dashed box needs its legend only beside a drawn rung. With no rung at all (no epoch yet) the big line
  // already says why, and "no account at this latency" under every slot would say it of the whole ladder.
  const someMissing = present.length > 0 && model.columns.some((c) => c.kind === 'missing');
  // With no rung marked in a started epoch the empty slots are accounts without a mark, not absent accounts.
  const noMarks = model.thinReason === 'no_marks';
  return (
    <figure className="ladder" aria-label="at what latency would this pay">
      <p className="ll-kicker">break-even</p>
      <p className="ll-big">{be.big}</p>
      <p className="ll-line">{be.line}</p>
      {model.breakEven.kind === 'at' && model.breakEven.not_monotone === true && <p className="dim">{NOT_MONOTONE}</p>}
      <p className="dim ll-ylabel">{epochText(model)}</p>
      <svg className="ll-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="basket gains and leg-risk cost per rung">
        <defs>
          <pattern id={hatch} width={6} height={6} patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect className="ll-hatch-line" x={0} y={0} width={2} height={6} />
          </pattern>
        </defs>
        {band && <rect className="ll-us" x={band.x0 * W} y={0} width={(band.x1 - band.x0) * W} height={H} />}
        {model.columns.map((c, i) => {
          const cx = slotX(i) * W;
          if (c.kind === 'missing') {
            return <rect key={c.scenario} className="ll-missing" data-scenario={c.scenario} x={cx - BAR / 2} y={1} width={BAR} height={H - 2} />;
          }
          return (
            <g key={c.scenario} data-scenario={c.scenario}>
              {stackRung(c.rung).map((s) => (
                <rect
                  key={s.part}
                  className={`ll-seg ll-${s.part}`}
                  x={cx - BAR / 2}
                  y={Math.min(sy(s.from), sy(s.to))}
                  width={BAR}
                  height={Math.abs(sy(s.from) - sy(s.to))}
                  fill={s.part === 'unrealized' ? `url(#${hatch})` : undefined}
                />
              ))}
              {/* A near-zero radius with a fixed-width non-scaling stroke stays round under preserveAspectRatio="none". */}
              <circle className="ll-net" cx={cx} cy={sy(geo(c.rung.net))} r={0.5} />
            </g>
          );
        })}
        {model.extras.map((r, j) => {
          const cx = slotX(SLOTS - 1) * W + (j % 2 === 0 ? -14 : 14);
          // A zero-length line with a square cap and a non-scaling stroke: a square at any stretch (a rect would squash).
          const y = sy(geo(r.net));
          return <line key={r.scenario} className="ll-extra" data-scenario={r.scenario} x1={cx} x2={cx} y1={y} y2={y} />;
        })}
        <line className="ll-zero" x1={0} x2={W} y1={zero} y2={zero} />
        {beX !== null && <line className="ll-be" x1={beX} x2={beX} y1={0} y2={H} />}
      </svg>
      <ol className="ll-axis" aria-label="rungs">
        {model.columns.map((c) => (
          <li key={c.scenario} className={c.kind === 'missing' ? 'missing' : undefined} title={c.scenario}>
            <span className="num">{rungMsText(c)}</span>
            {c.kind === 'missing' ? (
              <>
                <span aria-hidden="true">{NO_ACCOUNT_MARK}</span>
                <span className="ll-sr">{noMarks ? NO_MARK_YET : rungCountText(c, thin)}</span>
              </>
            ) : (
              <span>{rungCountText(c, thin)}</span>
            )}
          </li>
        ))}
        <li>
          <span>extras</span>
        </li>
      </ol>
      <p className="dim ll-xlabel">{`${AXIS_TITLE} · counts: trades per rung${thin ? `, of the ${MIN_EXEC} needed` : ''}`}</p>
      <p className="dim">{asOfText(model.asOfMs)}</p>
      <ul className="plain ll-legend" aria-label="legend">
        {LEGEND.map(([k, text]) => (
          <li key={k}>
            <span className={`ll-sw ${k}`} aria-hidden="true" />
            {text}
          </li>
        ))}
        <li>
          <span className="ll-sw us" aria-hidden="true" />
          {usText(model.measured)}
        </li>
        {someMissing && (
          <li>
            <span className="ll-sw missing" aria-hidden="true" />
            {NO_ACCOUNT_LEGEND}
          </li>
        )}
      </ul>
      {model.extras.length > 0 && (
        <ul className="plain ll-extras" aria-label="extras">
          {model.extras.map((r) => (
            <li key={r.scenario}>
              {extraText(r)}: <Money value={r.net} signed tone cents /> on paper · n {r.n_exec}
            </li>
          ))}
        </ul>
      )}
      <p className="quip">{LADDER_CAPTION}</p>
      <Fold id="ladder:markets" label="which markets">
        <Markets model={model} />
      </Fold>
      <Fold id="ladder:numbers" label="the numbers">
        <LadderTable rungs={[...present, ...model.extras]} />
      </Fold>
    </figure>
  );
}
