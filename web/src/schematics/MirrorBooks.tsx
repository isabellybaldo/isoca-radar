import { Fold } from '../components/Fold';
import { formatDecimal } from '../lib/money';
import type { Level, MirrorModel } from '../wasm/types';
import { geoPos, tick } from './geo';

export const CROSSED = 'Crossed book. Somewhere a feed is lying.';

const W = 300;
const H = 64;
const MID = H / 2;
const BAR = 28;

/** The mirrored row of a YES price: the NO level whose price is 1 − p. Paired by price, never by input index. */
const mirrorOf = (rows: Level[], p: string): Level | undefined => rows.find((r) => tick(r.p) + tick(p) === 100);

/**
 * Parent §5.7 (a): one book, twice. The top track is the YES book, the bottom one the NO book arb-core
 * mirrored from it; x is the price from 0 to 1, a bar's height the square root of its size; a thin line
 * joins each YES price p to its NO price 1 − p. Every number is HTML beside the SVG (preserveAspectRatio="none").
 * The best levels come from the engine's best_bid / best_ask, so a reordered input cannot mislabel them.
 */
export function MirrorBooks({ bids, asks, model }: { bids: Level[]; asks: Level[]; model: MirrorModel }) {
  const max = Math.max(1, ...[...bids, ...asks, ...model.no_bids, ...model.no_asks].map((l) => geoPos(l.s)));
  const h = (l: Level) => Math.sqrt(geoPos(l.s) / max) * BAR;
  const cx = (p: string) => Math.min(W - 1, Math.max(1, geoPos(p) * W));
  const up = (l: Level, cls: string, key: string) => <rect key={key} className={cls} x={cx(l.p) - 1} y={MID - 2 - h(l)} width={2} height={h(l)} />;
  const down = (l: Level, cls: string, key: string) => <rect key={key} className={cls} x={cx(l.p) - 1} y={MID + 2} width={2} height={h(l)} />;
  const links = [
    ...bids.map((l) => [l, mirrorOf(model.no_asks, l.p)] as const),
    ...asks.map((l) => [l, mirrorOf(model.no_bids, l.p)] as const),
  ];
  const rows = [
    ...bids.map((l) => ({ l, side: 'bid', to: 'ask', n: mirrorOf(model.no_asks, l.p) })),
    ...asks.map((l) => ({ l, side: 'ask', to: 'bid', n: mirrorOf(model.no_bids, l.p) })),
  ].filter((r) => r.n !== undefined);
  const noAsk = model.best_bid && mirrorOf(model.no_asks, model.best_bid.p);
  const noBid = model.best_ask && mirrorOf(model.no_bids, model.best_ask.p);
  return (
    <figure className="mirror" aria-label="one book, shown twice">
      <p className="mb-big">
        <span>{`D1 long ${model.d1_long ?? '—'}`}</span>
        <span>{`short ${model.d1_short ?? '—'}`}</span>
      </p>
      {model.crossed && <p className="mb-alert">{CROSSED}</p>}
      <div className="mb-row">
        <ul className="plain mb-lab" aria-hidden="true">
          <li>YES</li>
          <li>NO</li>
        </ul>
        <svg className="mb-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="the YES book above, its NO mirror below">
          <line className="mb-mid" x1={W / 2} x2={W / 2} y1={0} y2={H} />
          {bids.map((l, i) => up(l, 'mb-bid', `yb${i}`))}
          {asks.map((l, i) => up(l, 'mb-ask', `ya${i}`))}
          {model.no_bids.map((l, i) => down(l, 'mb-bid', `nb${i}`))}
          {model.no_asks.map((l, i) => down(l, 'mb-ask', `na${i}`))}
          {links.map(([y, n], i) =>
            n === undefined ? null : <line key={`k${i}`} className="mb-link" x1={cx(y.p)} y1={MID - 2} x2={cx(n.p)} y2={MID + 2} />,
          )}
        </svg>
      </div>
      <ol className="mb-axis" aria-hidden="true">
        <li>0</li>
        <li>0.50</li>
        <li>1</li>
      </ol>
      <ul className="plain mb-refl" aria-label="the mirror at the top of the book">
        {model.best_bid && noAsk && <li>{`NO ask ${noAsk.p} (1 − ${model.best_bid.p})`}</li>}
        {model.best_ask && noBid && <li>{`NO bid ${noBid.p} (1 − ${model.best_ask.p})`}</li>}
        {model.spread !== null && <li>{`spread ${formatDecimal(model.spread)}`}</li>}
      </ul>
      <p className="dim mb-note">bars: size at each price, teal bids and salmon asks; a line joins YES price p to NO price 1 − p</p>
      <Fold id="mirror-levels" label="every level">
        <ul className="plain mb-levels">
          {rows.map((r) => (
            <li key={`${r.side}${r.l.p}`}>{`YES ${r.side} ${r.l.p} → NO ${r.to} ${r.n?.p} (1 − ${r.l.p}) · size ${r.l.s}`}</li>
          ))}
        </ul>
      </Fold>
    </figure>
  );
}
