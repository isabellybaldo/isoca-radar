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

/** The ticks level i may take so every level on its side still fits strictly in order inside 1–99 (bids fall and
 *  asks rise from the best): the levels behind it need room. A crossed book between the sides stays possible. */
function room(side: 'bids' | 'asks', n: number, i: number): [number, number] {
  return side === 'bids' ? [n - i, 99 - i] : [1 + i, 99 - (n - 1 - i)];
}

/** Moves level i to `want` ticks (clamped to its room) and pushes its same-side neighbours along, so the side stays
 *  strictly ordered with no two levels at one price. Works on q = p for asks and 100 − p for bids (ascending). */
function drag(rows: Level[], side: 'bids' | 'asks', i: number, want: number): Level[] {
  const flip = (t: number) => (side === 'bids' ? 100 - t : t);
  const [lo, hi] = room(side, rows.length, i);
  const t = Math.min(hi, Math.max(lo, want));
  const q = rows.map((l) => flip(toTick(l.p)));
  q[i] = flip(t);
  for (let k = i - 1; k >= 0; k--) q[k] = Math.min(q[k], q[k + 1] - 1);
  for (let k = i + 1; k < q.length; k++) q[k] = Math.max(q[k], q[k - 1] + 1);
  return rows.map((l, k) => ({ ...l, p: fromTick(flip(q[k])) }));
}

/** 09-27 §6.2 a: drag any level's price (0.01 ticks) or size; "cross the book". */
export function MirrorSection({ engine }: { engine: Engine }) {
  const [book, setBook] = useState<MirrorIn>(START);
  const input = useFrameThrottle(book);
  const out = useMemo(() => call<MirrorModel>(engine, 'mirror_json', input), [engine, input]);
  const setLevel = (side: 'bids' | 'asks', i: number, patch: Partial<Level>) =>
    setBook((b) => {
      const rows = patch.p === undefined ? b[side] : drag(b[side], side, i, toTick(patch.p));
      const next = patch.s === undefined ? rows : rows.map((l, k) => (k === i ? { ...l, s: patch.s as string } : l));
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
                const [lo, hi] = room(side, book[side].length, i);
                return (
                  <fieldset key={`${side}${i}`} className="m-level">
                    <legend>{name}</legend>
                    <Range label={`${name} price`} min={lo} max={hi} value={toTick(l.p)} show={l.p} onChange={(t) => setLevel(side, i, { p: fromTick(t) })} />
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
