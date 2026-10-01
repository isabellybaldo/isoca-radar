import { useMemo, useState } from 'react';
import { Fold } from '../../components/Fold';
import { MirrorBooks } from '../../schematics/MirrorBooks';
import { call } from '../../wasm/call';
import type { Engine, Level, MirrorIn, MirrorModel } from '../../wasm/types';
import { SECTIONS } from '../copy';
import { FED_ASKS, FED_BIDS, fromTick, toTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Out, Range, Section } from './Section';

export const MIRROR_START: MirrorIn = { bids: FED_BIDS, asks: FED_ASKS };
const START = MIRROR_START;
const SIDE = { bids: 'bid', asks: 'ask' } as const;

/** Best bid one tick above the best ask; the other bids at or above it go (no two levels at one price). */
function crossed(b: MirrorIn): MirrorIn {
  const p = fromTick(toTick(b.asks[0]?.p ?? '0.50') + 1);
  return { asks: b.asks, bids: [{ p, s: b.bids[0]?.s ?? '100' }, ...b.bids.slice(1).filter((l) => toTick(l.p) < toTick(p))] };
}

/** The ticks a level may take: strictly between its neighbours on its own side (bids fall, asks rise), so a
 *  drag can never put two levels at one price. A crossed book between the sides stays possible. */
function room(side: 'bids' | 'asks', rows: Level[], i: number): [number, number] {
  const prev = rows[i - 1] === undefined ? null : toTick(rows[i - 1].p);
  const next = rows[i + 1] === undefined ? null : toTick(rows[i + 1].p);
  const [lo, hi] = side === 'bids' ? [next, prev] : [prev, next];
  return [lo === null ? 1 : lo + 1, hi === null ? 99 : hi - 1];
}

/** 09-27 §6.2 a: drag any level's price (0.01 ticks) or size; "cross the book". */
export function MirrorSection({ engine }: { engine: Engine }) {
  const [book, setBook] = useState<MirrorIn>(START);
  const input = useFrameThrottle(book);
  const out = useMemo(() => call<MirrorModel>(engine, 'mirror_json', input), [engine, input]);
  const setLevel = (side: 'bids' | 'asks', i: number, patch: Partial<Level>) =>
    setBook((b) => {
      const next = b[side].map((l, k) => {
        if (k !== i) return l;
        if (patch.p === undefined) return { ...l, ...patch };
        const [lo, hi] = room(side, b[side], i);
        return { ...l, ...patch, p: fromTick(Math.min(hi, Math.max(lo, toTick(patch.p)))) };
      });
      return side === 'bids' ? { ...b, bids: next } : { ...b, asks: next };
    });
  return (
    <Section
      id="a"
      {...SECTIONS.a}
      onReset={() => setBook(START)}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setBook(crossed)}>cross the book</button>
          <Fold id="math:a:levels" label="move the levels">
            {(['bids', 'asks'] as const).flatMap((side) =>
              book[side].map((l, i) => {
                const name = `YES ${SIDE[side]} ${i + 1}`;
                const [lo, hi] = room(side, book[side], i);
                return (
                  <fieldset key={`${side}${i}`} className="m-level">
                    <legend>{name}</legend>
                    <Range label={`${name} price`} min={lo} max={Math.max(lo, hi)} value={toTick(l.p)} show={l.p} onChange={(t) => setLevel(side, i, { p: fromTick(t) })} />
                    <label className="m-size">
                      size
                      <input type="number" aria-label={`${name} size`} min={0} step="0.01" value={l.s} onChange={(e) => setLevel(side, i, { s: e.target.value })} />
                    </label>
                  </fieldset>
                );
              }),
            )}
          </Fold>
        </>
      }
    >
      <Out out={out}>{(m) => <MirrorBooks bids={input.bids} asks={input.asks} model={m} />}</Out>
    </Section>
  );
}
