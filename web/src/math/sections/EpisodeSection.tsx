import { useMemo, useState } from 'react';
import { Money } from '../../components/Money';
import { DurationBars } from '../../schematics/DurationBars';
import { UpdateBars } from '../../schematics/UpdateBars';
import { call } from '../../wasm/call';
import { type Engine, type EngineError, type EpisodeIn, type EpisodeModel, type LegIn, isError } from '../../wasm/types';
import { episodeRows } from '../adapt';
import { E_QUIPS, SECTIONS } from '../copy';
import { EPISODE_BIDS, EPISODE_T, PRESETS, fromTick } from '../defaults';
import { useFrameThrottle } from '../useFrameThrottle';
import { Check, Out, Range, Section, Seg } from './Section';

type Lat = { proc_ms: number; net_ms: number; match_ms: number };
export interface EpisodeState {
  sc: Lat;
  delay: boolean;
  upto: number;
}
const SP_IDLE = PRESETS[PRESETS.length - 1];
export const EPISODE_START: EpisodeState = {
  sc: { proc_ms: SP_IDLE.proc_ms, net_ms: SP_IDLE.net_ms, match_ms: SP_IDLE.match_ms },
  delay: false,
  upto: EPISODE_T.length,
};
const START = EPISODE_START;
const OPTIONS: [string, string][] = PRESETS.map((p): [string, string] => [p.name, p.name]);
const KNOBS: [keyof Lat, string][] = [['proc_ms', 'proc ms'], ['net_ms', 'net ms'], ['match_ms', 'match ms']];

const leg = (bid: number): LegIn => ({ status: 'live', bids: [{ p: fromTick(bid), s: '100' }], asks: [{ p: fromTick(bid + 1), s: '100' }], fee: null });

/** episode_json's input: no basket (the engine has none to fill, 7b §6.2 e) and no gap_ms (it changes nothing). */
export function episodeInput(st: EpisodeState): EpisodeIn {
  return {
    kind: 'no',
    updates: EPISODE_T.slice(0, st.upto).map((t, i) => ({ t_ms: t, legs: EPISODE_BIDS[i].map(leg) })),
    scenario: { ...st.sc, honor_seconds_delay: true },
    seconds_delay: st.delay ? 1 : null,
  };
}

/** The exact times behind the verdict, so two bars that read alike cannot contradict it. */
function timesLine(m: EpisodeModel): string | null {
  if (m.open_ms === null) return null;
  const last = m.updates.length > 0 ? m.updates[m.updates.length - 1].t_ms : m.open_ms;
  const close = m.close_ms === null ? `still open at the last update (${last} ms)` : `closed at ${m.close_ms} ms`;
  return `opened at ${m.open_ms} ms · our order lands at ${m.t_x} ms · ${close}`;
}

/** The closing line from the engine's own verdict, as f does: none on an engine error, and none while no episode
 *  has opened (there is no edge yet to speak of). */
function closing(out: EpisodeModel | EngineError): string | null {
  if (isError(out) || out.open_ms === null) return null;
  return out.caught ? E_QUIPS.caught : E_QUIPS.missed;
}

/** 09-27 §6.2 e: scrub updates, proc/net/match sliders with the LADDER presets, secondsDelay. */
export function EpisodeSection({ engine }: { engine: Engine }) {
  const [st, setSt] = useState<EpisodeState>(START);
  const input = useFrameThrottle(st);
  const out = useMemo(() => call<EpisodeModel>(engine, 'episode_json', episodeInput(input)), [engine, input]);
  const preset = PRESETS.find((p) => p.proc_ms === st.sc.proc_ms && p.net_ms === st.sc.net_ms && p.match_ms === st.sc.match_ms)?.name ?? 'custom';
  return (
    <Section
      id="e"
      {...SECTIONS.e}
      quip={closing(out)}
      onReset={() => setSt(START)}
      controls={
        <>
          <Seg
            label="latency presets from the ladder"
            options={OPTIONS}
            value={preset}
            onPick={(name) => {
              const p = PRESETS.find((x) => x.name === name);
              if (p) setSt((s) => ({ ...s, sc: { proc_ms: p.proc_ms, net_ms: p.net_ms, match_ms: p.match_ms } }));
            }}
          />
          {KNOBS.map(([k, label]) => (
            <Range key={k} label={label} min={0} max={1000} value={st.sc[k]} show={`${st.sc[k]} ms`} onChange={(v) => setSt((s) => ({ ...s, sc: { ...s.sc, [k]: v } }))} />
          ))}
          <Range label="updates so far" min={2} max={EPISODE_T.length} value={st.upto} show={`u1–u${st.upto}`} onChange={(upto) => setSt((s) => ({ ...s, upto }))} />
          <Check label="the market has a 1 s secondsDelay" checked={st.delay} onChange={(delay) => setSt((s) => ({ ...s, delay }))} />
        </>
      }
    >
      <Out out={out}>
        {(m) => {
          const lat = m.t_x !== null && m.open_ms !== null ? m.t_x - m.open_ms : null;
          const word = m.open_ms === null ? 'no episode yet' : m.caught ? 'caught' : 'missed';
          const times = timesLine(m);
          return (
            <>
              <p className={m.open_ms === null ? 'm-verdict' : m.caught ? 'm-verdict pos' : 'm-verdict neg'}>{word}</p>
              {m.caught && m.captured !== null && lat !== null && (
                <p className="m-verdict-line">
                  {`captured ${lat} ms after it opened: `}
                  <Money value={m.captured} signed cents /> on the table at t_x, in this teaching model
                </p>
              )}
              {times !== null && <p className="dim m-times">{times}</p>}
              <UpdateBars updates={m.updates} peakMs={m.peak_ms} />
              <DurationBars rows={episodeRows(m)} />
            </>
          );
        }}
      </Out>
    </Section>
  );
}
