import { type ReactNode, useMemo, useState } from 'react';
import { BasketLegs, basketWord } from '../../schematics/BasketLegs';
import { ClassifyTree } from '../../schematics/ClassifyTree';
import { DepthStairs } from '../../schematics/DepthStairs';
import { MirrorBooks } from '../../schematics/MirrorBooks';
import { PriceStack } from '../../schematics/PriceStack';
import { call } from '../../wasm/call';
import {
  type BasketIn, type BasketModel, type ClassifyIn, type ClassifyModel, type DepthWalkIn, type Engine, type EngineFn, type EpisodeIn,
  type EpisodeModel, type MirrorIn, type MirrorModel, type PayoffIn, type PayoffMatrix, type WalkModel, isError,
} from '../../wasm/types';
import { F_BOOKS, LEGS } from '../defaults';
import { EpisodeView } from '../sections/EpisodeSection';
import { PayoffView } from '../sections/PayoffSection';
import { Out } from '../sections/Section';
import { Chapter } from './Chapter';
import { isFrozen } from '../../frozenMode';
import { COST_LINK } from '../copy';
import { CH3_COUNT, CH7_EXTRAS, CH7_LEAD_FROZEN, CHAPTERS, type ChapterCopy, TAKE } from './copy';
import { CH1, CH2, CH3, CH3_LABELS, CH4, CH5, CH6, type Pair } from './presets';

/** One engine-backed chapter: its own two-state switch, the engine's answer for the state shown, the drawing,
 *  the takeaway picked from that answer (none when the engine answered with an error), then `after`.
 *  Without the engine it is the chapter's prose (and `after`, which is live data and needs no engine). */
function Preset<I, M>({ engine, copy, fn, inputs, options = copy.options, draw, take, after }: {
  engine: Engine | null;
  copy: ChapterCopy;
  fn: EngineFn;
  inputs: Pair<I>;
  /** The switch's two labels when the copy has none of its own (chapter 3's are built from its preset). */
  options?: Pair<string> | null;
  draw: (m: M, input: I) => ReactNode;
  take: (m: M) => string;
  after?: ReactNode;
}) {
  const [v, setV] = useState<0 | 1>(0);
  const out = useMemo(() => (engine === null ? null : call<M>(engine, fn, inputs[v])), [engine, fn, inputs, v]);
  if (out === null) return <Chapter copy={copy} after={after} />;
  return (
    <Chapter copy={copy} options={options} value={v} onPick={setV} take={isError(out) ? null : take(out)} after={after}>
      <Out out={out}>{(m) => draw(m, inputs[v])}</Out>
    </Chapter>
  );
}

const F_NAMES = LEGS.slice(0, F_BOOKS.length);
const [C1, C2, C3, C4, C5, C6, C7] = CHAPTERS;

/** Spec 2026-10-01 §3, §5.1: the seven chapters. `engine` null (loading or failed): prose only. `strip` and
 *  `ladder` are what the page decided to show for the live parts of chapters 3 and 7: the component, the
 *  "unavailable" line, or null while the snapshot loads. `ladderDrawn`: the ladder itself is there. */
export function Story({ engine, strip, ladder, ladderDrawn }: { engine: Engine | null; strip: ReactNode; ladder: ReactNode; ladderDrawn: boolean }) {
  return (
    <>
      <Preset<MirrorIn, MirrorModel>
        engine={engine} copy={C1} fn="mirror_json" inputs={CH1}
        draw={(m, i) => <MirrorBooks bids={i.bids} asks={i.asks} model={m} headline={false} foldId="story:mirror-levels" />}
        take={(m) => TAKE.ch1(m.crossed)}
      />
      <Preset<PayoffIn, PayoffMatrix>
        engine={engine} copy={C2} fn="payoff_json" inputs={CH2}
        draw={(m, i) => <PayoffView m={m} kind={i.kind} />}
        take={(m) => TAKE.ch2(m.broken.some(Boolean))}
      />
      <Preset<ClassifyIn, ClassifyModel>
        engine={engine} copy={C3} fn="classify_json" inputs={CH3} options={CH3_LABELS}
        draw={(m, i) => (
          <>
            <PriceStack legs={i.legs.map((l, k) => ({ name: LEGS[k] ?? String(k + 1), price: l.asks[0]?.p ?? '' }))} />
            <ClassifyTree model={m} path={false} />
          </>
        )}
        take={(m) => TAKE.ch3(m.kind !== null)}
        after={strip !== null && (
          <>
            <p className="m-ch-lead">{CH3_COUNT}</p>
            {strip}
          </>
        )}
      />
      <Preset<DepthWalkIn, WalkModel>
        engine={engine} copy={C4} fn="depth_walk_json" inputs={CH4}
        draw={(m) => <DepthStairs model={m} />}
        take={() => TAKE.ch4}
      />
      <Preset<EpisodeIn, EpisodeModel>
        engine={engine} copy={C5} fn="episode_json" inputs={CH5}
        draw={(m) => <EpisodeView m={m} />}
        take={(m) => TAKE.ch5(m.caught)}
      />
      <Preset<BasketIn, BasketModel>
        engine={engine} copy={C6} fn="basket_json" inputs={CH6}
        draw={(m) => <BasketLegs model={m} names={F_NAMES} />}
        take={(m) => TAKE.ch6(basketWord(m))}
      />
      <Chapter copy={isFrozen() ? { ...C7, lead: CH7_LEAD_FROZEN } : C7} take={TAKE.ch7}>
        {ladder}
        {ladderDrawn && <p className="dim m-note">{CH7_EXTRAS}</p>}
      </Chapter>
      {isFrozen() && (
        <p className="m-note">
          <a href="/#cost">{COST_LINK}</a>
        </p>
      )}
    </>
  );
}
