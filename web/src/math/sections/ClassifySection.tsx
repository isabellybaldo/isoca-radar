import { useMemo, useState } from 'react';
import { ClassifyTree } from '../../schematics/ClassifyTree';
import { call } from '../../wasm/call';
import type { ClassifyIn, ClassifyModel, Engine, LegStatus } from '../../wasm/types';
import { POST_END_NOTE, SECTIONS } from '../copy';
import { CLASSIFY_LEGS, type ClassifyLeg, LEGS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section } from './Section';

export interface ClassifyState {
  legs: ClassifyLeg[];
  augmented: boolean;
}
export const CLASSIFY_START: ClassifyState = { legs: CLASSIFY_LEGS, augmented: false };
const START = CLASSIFY_START;
const STATUSES: LegStatus[] = ['live', 'placeholder', 'other', 'halted', 'closed_no', 'closed_yes'];

/** One level a side per leg (10 shares, as 7b-A's classify fixtures), no fees: the update classify reads. */
export function classifyInput(st: ClassifyState): ClassifyIn {
  return {
    augmented: st.augmented,
    legs: st.legs.map((l) => ({ status: l.status, bids: [{ p: fromTick(l.bid), s: '10' }], asks: [{ p: fromTick(l.ask), s: '10' }], fee: null })),
  };
}

/** 09-27 §6.2 c: per leg its status and best bid and ask; "augmented". */
export function ClassifySection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<ClassifyState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<ClassifyModel>(engine, 'classify_json', classifyInput(input)), [engine, input]);
  const setLeg = (i: number, patch: Partial<ClassifyLeg>) => setSt((s) => ({ ...s, legs: s.legs.map((l, k) => (k === i ? { ...l, ...patch } : l)) }));
  return (
    <Section
      id="c"
      {...SECTIONS.c}
      onReset={() => setSt(START)}
      controls={
        <>
          {st.legs.map((l, i) => (
            <fieldset key={LEGS[i]} className="m-leg">
              <legend>{`leg ${LEGS[i]}`}</legend>
              <label className="m-select">
                status
                <select aria-label={`leg ${LEGS[i]} status`} value={l.status} onChange={(e) => setLeg(i, { status: e.target.value as LegStatus })}>
                  {STATUSES.map((s) => (
                    <option key={s} value={s}>{s.replace('_', ' ')}</option>
                  ))}
                </select>
              </label>
              <Range label={`leg ${LEGS[i]} best bid`} min={1} max={99} value={l.bid} show={fromTick(l.bid)} onChange={(bid) => setLeg(i, { bid })} />
              <Range label={`leg ${LEGS[i]} best ask`} min={1} max={99} value={l.ask} show={fromTick(l.ask)} onChange={(ask) => setLeg(i, { ask })} />
            </fieldset>
          ))}
          <Check label="augmented event (an “Other” leg can appear)" checked={st.augmented} onChange={(augmented) => setSt((s) => ({ ...s, augmented }))} />
        </>
      }
    >
      <Out out={out}>{(m) => <ClassifyTree model={m} />}</Out>
      <p className="dim m-note">{POST_END_NOTE}</p>
    </Section>
  );
}
