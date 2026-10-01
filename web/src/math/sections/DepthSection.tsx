import { useMemo, useState } from 'react';
import { Fold } from '../../components/Fold';
import { DepthStairs } from '../../schematics/DepthStairs';
import { call } from '../../wasm/call';
import type { DepthWalkIn, Engine, Level, WalkModel } from '../../wasm/types';
import { CRYPTO_FEE } from '../basket';
import { SECTIONS } from '../copy';
import { LEGS, NEW_WALK_LEG, WALK_LEGS, exactlyOne, toTick } from '../defaults';
import { drag, room } from '../ladderDrag';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

export interface DepthState {
  legs: Level[][];
  fees: boolean;
}
export const DEPTH_START: DepthState = { legs: WALK_LEGS, fees: false };
const START = DEPTH_START;

/** depth_walk_json's input: each leg's levels are the asks of the outcome bought, cheapest first (kept strictly
 *  ascending by every control below). */
export function depthInput(st: DepthState): DepthWalkIn {
  return { rule: 'yes', legs: st.legs.map((levels) => ({ levels, fee: st.fees ? CRYPTO_FEE : null })) };
}

/** 09-27 §6.2 d: drag levels on up to 4 legs, fees on or off, "exactly 1.00". A dragged level pushes its
 *  neighbours (the helper section a uses), so the engine never gets "levels must be cheapest first". */
export function DepthSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<DepthState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<WalkModel>(engine, 'depth_walk_json', depthInput(input)), [engine, input]);
  const setP = (li: number, i: number, t: number) =>
    setSt((s) => ({ ...s, legs: s.legs.map((l, k) => (k === li ? drag(l, 'asks', i, t) : l)) }));
  return (
    <Section
      id="d"
      {...SECTIONS.d}
      onReset={() => setSt(START)}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setSt((s) => ({ ...s, legs: exactlyOne(s.legs) }))}>exactly 1.00</button>
          <button type="button" className="m-btn" disabled={st.legs.length >= 4} onClick={() => setSt((s) => ({ ...s, legs: [...s.legs, NEW_WALK_LEG] }))}>add a leg</button>
          <button type="button" className="m-btn" disabled={st.legs.length <= 2} onClick={() => setSt((s) => ({ ...s, legs: s.legs.slice(0, -1) }))}>remove a leg</button>
          <Check label="taker fees on (crypto: rate 0.07, exponent 1)" checked={st.fees} onChange={(fees) => setSt((s) => ({ ...s, fees }))} />
          <Fold id="math:d:levels" label="move the levels">
            {st.legs.map((l, li) =>
              l.map((x, i) => {
                const [lo, hi] = room('asks', l.length, i);
                return <Range key={`${li}-${i}`} label={`leg ${LEGS[li]} level ${i + 1} price`} min={lo} max={hi} value={toTick(x.p)} show={x.p} onChange={(t) => setP(li, i, t)} />;
              }),
            )}
          </Fold>
        </>
      }
    >
      <Out out={out}>{(m) => <DepthStairs model={m} />}</Out>
    </Section>
  );
}
