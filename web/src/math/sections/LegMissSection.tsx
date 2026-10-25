import { useMemo, useState } from 'react';
import { BasketLegs, basketWord } from '../../schematics/BasketLegs';
import { call } from '../../wasm/call';
import type { BasketModel, Engine } from '../../wasm/types';
import { isError } from '../../wasm/types';
import { type FState, basketInput } from '../basket';
import { isFrozen } from '../../frozenMode';
import { F_CAPTION_FROZEN, F_QUIPS, SECTIONS } from '../copy';
import { F_BOOKS, F_BURST, F_START, LEGS } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

/** 7b §6.2 f: per leg the depth left at t_x and the unwind book against us; shares; fees; "the usual burst".
 *  No live part: the ladder from the snapshot is the story's chapter 7 (spec 2026-10-01 §3). */
export function LegMissSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<FState>(F_START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<BasketModel>(engine, 'basket_json', basketInput(F_BOOKS, input)), [engine, input]);
  const upd = (xs: number[], i: number, v: number) => xs.map((x, j) => (j === i ? v : x));
  const names = LEGS.slice(0, F_BOOKS.length);
  return (
    <Section
      id="f"
      {...SECTIONS.f}
      caption={isFrozen() ? F_CAPTION_FROZEN : SECTIONS.f.caption}
      quip={isError(out) ? null : F_QUIPS[basketWord(out)]}
      onReset={() => setSt(F_START)}
      controls={
        <>
          <button type="button" className="m-btn" onClick={() => setSt(F_BURST)}>the usual burst</button>
          {names.map((n, i) => (
            <fieldset key={n} className="m-leg">
              <legend>{`leg ${n}`}</legend>
              <Range label={`leg ${n} depth left at t_x`} text="depth left at t_x" min={0} max={100} step={5} value={st.depth[i]} show={`${st.depth[i]}%`} onChange={(v) => setSt((s) => ({ ...s, depth: upd(s.depth, i, v) }))} />
              <Range label={`leg ${n} unwind book against us`} text="unwind book against us" min={0} max={20} value={st.against[i]} show={`${st.against[i]} ticks`} onChange={(v) => setSt((s) => ({ ...s, against: upd(s.against, i, v) }))} />
            </fieldset>
          ))}
          <Range label="shares to buy" min={5} max={200} step={5} value={st.q} onChange={(q) => setSt((s) => ({ ...s, q }))} />
          <Check label="taker fees on (crypto: rate 0.07, exponent 1)" checked={st.fees} onChange={(fees) => setSt((s) => ({ ...s, fees }))} />
        </>
      }
    >
      <Out out={out}>{(m) => <BasketLegs model={m} names={names} />}</Out>
    </Section>
  );
}
