import { useMemo, useState } from 'react';
import { formatDecimal } from '../../lib/money';
import { PayoffGrid } from '../../schematics/PayoffGrid';
import { call } from '../../wasm/call';
import type { Engine, PayoffIn, PayoffKind, PayoffMatrix } from '../../wasm/types';
import { matrixToPayoff } from '../adapt';
import { SECTIONS } from '../copy';
import { IMPLICATION_TICKS, LEGS, PAYOFF_TICKS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section, Seg } from './Section';

export interface PayoffState {
  kind: PayoffKind;
  s: number;
  listed: boolean;
  priced: boolean;
  /** YES: each leg's ask; NO: each leg's bid; in ticks. */
  ticks: number[];
  /** Implication: [A's bid, B's ask] in ticks, the only two prices the engine reads (R10). */
  impl: number[];
}
export const PAYOFF_START: PayoffState = { kind: 'yes', s: 3, listed: false, priced: false, ticks: PAYOFF_TICKS, impl: IMPLICATION_TICKS };
const START = PAYOFF_START;
const KINDS: [PayoffKind, string][] = [['yes', 'YES basket'], ['no', 'NO basket'], ['implication', 'implication A ⇒ B']];
const SIZES: [number, string][] = [2, 3, 4, 5, 6].map((k): [number, string] => [k, `S ${k}`]);
/** Parent §5.7 (b), L5: the gross edge at the top of the book (arb_core::fill::walk); the value is the engine's. */
const FORMULA: Record<PayoffKind, string> = { yes: '1 − Σ asks', no: 'Σ bids − 1', implication: 'A bid − B ask' };

/** The engine input. prices has length s, or 2 for the implication (whose s the engine needs but ignores; the
 *  page sends 2). A tick is the YES ask (YES), the YES bid (NO), or for the implication A's bid and B's ask:
 *  the side each leg does not use is a grid neighbour the engine never reads. Strings from integer ticks (R12). */
export function payoffInput(st: PayoffState): PayoffIn {
  const n = st.kind === 'implication' ? 2 : st.s;
  let prices: PayoffIn['prices'] = null;
  if (st.priced) {
    if (st.kind === 'implication') {
      const [a, b] = st.impl;
      prices = [{ bid: fromTick(a), ask: fromTick(a + 1) }, { bid: fromTick(b - 1), ask: fromTick(b) }];
    } else {
      prices = st.ticks.slice(0, n).map((t) => (st.kind === 'no' ? { bid: fromTick(t), ask: fromTick(t + 1) } : { bid: fromTick(t - 1), ask: fromTick(t) }));
    }
  }
  return { kind: st.kind, s: n, other_listed: st.kind === 'implication' ? false : st.listed, prices };
}

/** 09-27 §6.2 b: YES / NO / implication, S 2–6, "Other listed", optional prices per leg. */
export function PayoffSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<PayoffState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<PayoffMatrix>(engine, 'payoff_json', payoffInput(input)), [engine, input]);
  const set = (p: Partial<PayoffState>) => setSt((s) => ({ ...s, ...p }));
  const side = st.kind === 'no' ? 'bid' : 'ask';
  const formula = FORMULA[input.kind];
  const sliders = () => {
    if (!st.priced) return null;
    if (st.kind === 'implication') {
      const [a, b] = st.impl;
      return (
        <>
          <Range label="A bid (NO_A costs 1 − bid)" min={1} max={98} value={a} show={fromTick(a)} onChange={(v) => set({ impl: [v, b] })} />
          <Range label="B ask" min={2} max={99} value={b} show={fromTick(b)} onChange={(v) => set({ impl: [a, v] })} />
        </>
      );
    }
    return st.ticks.slice(0, st.s).map((t, i) => (
      <Range
        key={LEGS[i] ?? i}
        label={`leg ${LEGS[i] ?? i + 1} ${side}`}
        min={2}
        max={98}
        value={t}
        show={fromTick(t)}
        onChange={(v) => set({ ticks: st.ticks.map((x, k) => (k === i ? v : x)) })}
      />
    ));
  };
  return (
    <Section
      id="b"
      {...SECTIONS.b}
      onReset={() => setSt(START)}
      controls={
        <>
          <Seg label="basket" options={KINDS} value={st.kind} onPick={(kind) => set({ kind })} />
          {st.kind !== 'implication' && <Seg label="outcomes in the set" options={SIZES} value={st.s} onPick={(s) => set({ s })} />}
          {st.kind !== 'implication' && <Check label="“Other” is listed" checked={st.listed} onChange={(listed) => set({ listed })} />}
          <Check label="with prices" checked={st.priced} onChange={(priced) => set({ priced })} />
          {sliders()}
        </>
      }
    >
      <Out out={out}>
        {(m) => (
          <>
            <PayoffGrid model={matrixToPayoff(m, input.kind)} label={input.kind === 'implication' ? 'payoff of the implication' : `payoff of the ${input.kind.toUpperCase()} basket`} />
            {m.top_edge_gross !== null && (
              <p className="m-formula">
                <span>{`${formula} =`}</span> <span className="m-big">{formatDecimal(m.top_edge_gross)}</span>{' '}
                <span className="dim">per basket at the top of the book, before fees, on the table</span>
              </p>
            )}
          </>
        )}
      </Out>
    </Section>
  );
}
