import { Fold } from '../components/Fold';
import { formatDecimal } from '../lib/money';
import type { Level, MirrorModel } from '../wasm/types';
import { along, geoPos, mirrorDomain, tick, tickText } from './geo';

export const CROSSED = 'Crossed book. Somewhere a feed is lying.';

const W = 300;
const BAR = 28;
/** The band between the two rows, where the join lines run: wide enough to follow a line from p to 1 − p. */
const GAP = 20;
const H = 2 * BAR + GAP;
const NO_TOP = BAR + GAP;
/** A bar's share of one tick of the axis: neighbours a tick apart never touch, at any width. */
const BAR_SHARE = 0.7;

/** The ticks where a row holds both a bid and an ask (a crossed book): the two share the slot there. */
const sharedTicks = (a: Level[], b: Level[]): Set<number> => {
  const mine = new Set(a.map((l) => tick(l.p)));
  return new Set(b.map((l) => tick(l.p)).filter((t) => mine.has(t)));
};

/** The mirrored row of a YES price: the NO level whose price is 1 − p. Paired by price, never by input index. */
const mirrorOf = (rows: Level[], p: string): Level | undefined => rows.find((r) => tick(r.p) + tick(p) === 100);

/**
 * Parent §5.7 (a): one book, twice. The top track is the YES book, the bottom one the NO book arb-core
 * mirrored from it; x is the price, zoomed onto the prices drawn and symmetric around 0.50 (mirrorDomain), a
 * bar's height the square root of its size; a thin line joins each YES price p to its NO price 1 − p across the
 * band between the rows. Every number is HTML beside the SVG (preserveAspectRatio="none").
 * The best levels come from the engine's best_bid / best_ask, so a reordered input cannot mislabel them.
 */
export function MirrorBooks({ bids, asks, model }: { bids: Level[]; asks: Level[]; model: MirrorModel }) {
  const max = Math.max(1, ...[...bids, ...asks, ...model.no_bids, ...model.no_asks].map((l) => geoPos(l.s)));
  const h = (l: Level) => Math.sqrt(geoPos(l.s) / max) * BAR;
  const d = mirrorDomain([...bids, ...asks, ...model.no_bids, ...model.no_asks].map((l) => l.p));
  const bw = (W / (d.hi - d.lo)) * BAR_SHARE;
  const cx = (p: string) => Math.min(W - bw / 2, Math.max(bw / 2, along(d, p) * W));
  const yesShared = sharedTicks(bids, asks);
  const noShared = sharedTicks(model.no_bids, model.no_asks);
  // A bid and an ask at one price would cover each other: there the bid takes the left half of the slot and the
  // ask the right half. The join lines stay on the slot's middle.
  const bar = (l: Level, side: 'bid' | 'ask', row: 'yes' | 'no', key: string) => {
    const half = (row === 'yes' ? yesShared : noShared).has(tick(l.p));
    const x = half && side === 'ask' ? cx(l.p) : cx(l.p) - bw / 2;
    return <rect key={key} className={`mb-${side}`} x={x} y={row === 'yes' ? BAR - h(l) : NO_TOP} width={half ? bw / 2 : bw} height={h(l)} />;
  };
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
          <g className="mb-yes">
            {bids.map((l, i) => bar(l, 'bid', 'yes', `yb${i}`))}
            {asks.map((l, i) => bar(l, 'ask', 'yes', `ya${i}`))}
          </g>
          <g className="mb-no">
            {model.no_bids.map((l, i) => bar(l, 'bid', 'no', `nb${i}`))}
            {model.no_asks.map((l, i) => bar(l, 'ask', 'no', `na${i}`))}
          </g>
          {links.map(([y, n], i) =>
            n === undefined ? null : <line key={`k${i}`} className="mb-link" x1={cx(y.p)} y1={BAR} x2={cx(n.p)} y2={NO_TOP} />,
          )}
        </svg>
      </div>
      <ol className="mb-axis" aria-hidden="true">
        <li>{tickText(d.lo)}</li>
        <li>0.50</li>
        <li>{tickText(d.hi)}</li>
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
