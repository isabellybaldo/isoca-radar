import type { ReactElement } from 'react';
import type { EpisodeSummary, Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { useNow } from '../lib/hooks';
import { ladderModel } from '../lib/ladder';
import { isArbitrage } from '../pages/Radar';
import { DurationBars } from '../schematics/DurationBars';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { COPY, onTable100, onTableDepth } from './copy';
import type { ExplainPage, ExplainState } from './ExplainContext';
import { EXAMPLE_EPISODE, durationRows, payoffKind, payoffStructure } from './models';

export interface AnswerProps {
  s: ExplainState;
  /** The panel's one fetch (spec §9.3); every answer reuses it, none fetches. */
  ladder: ApiState<Ladder>;
}

export interface Question {
  id: string;
  q: string;
  pages: 'all' | ExplainPage[];
  Answer: (p: AnswerProps) => ReactElement;
}

export const HERO_ID = 'latency';

function Hero({ ladder }: AnswerProps) {
  return <Loadable state={ladder} what="the ladder">{(l) => <LatencyLadder model={ladderModel(l)} />}</Loadable>;
}

/** The selected row, or else the top open arbitrage row (complete YES or NO basket; D-15: Radar's predicate). */
function radarEpisode(s: ExplainState): EpisodeSummary | null {
  if (s.selection?.kind === 'episode') return s.selection.episode;
  return s.data.radar?.open?.find((e) => isArbitrage(e.kind)) ?? null;
}

function MakeMoney({ s }: AnswerProps) {
  const e = radarEpisode(s);
  const kind = e === null ? null : payoffKind(e.kind);
  if (e === null || kind === null) {
    return (
      <>
        <p className="dim">{COPY.example}</p>
        <PayoffGrid model={payoffStructure('no', EXAMPLE_EPISODE.s_legs, false)} label="payoff of the example basket" />
        <p>{COPY.makeMoneyNo}</p>
      </>
    );
  }
  return (
    <>
      <PayoffGrid model={payoffStructure(kind === 'no' ? 'no' : 'yes', e.s_legs, kind === 'incomplete')} label={`payoff of ${e.event_title ?? e.id}`} />
      <p>{kind === 'no' ? COPY.makeMoneyNo : kind === 'yes' ? COPY.makeMoneyYes : COPY.makeMoneyIncomplete}</p>
      {kind !== 'incomplete' && <p className="fig">{onTable100(e.net_budget100)}</p>}
      {kind !== 'incomplete' && <p className="fig">{onTableDepth(e.net)}</p>}
    </>
  );
}

function WhatEpisode({ s }: AnswerProps) {
  const now = useNow(1_000);
  const row = s.selection?.kind === 'episode' ? s.selection.episode : (s.data.episodes?.items?.[0] ?? null);
  return (
    <>
      <DurationBars rows={durationRows(row, now)} />
      <p>{COPY.whatEpisode}</p>
      <p>{COPY.episodeBar}</p>
    </>
  );
}

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [
  { id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero },
  { id: 'make-money', q: 'How can this make money?', pages: ['radar'], Answer: MakeMoney },
  { id: 'what-episode', q: 'What is an episode?', pages: ['episodes'], Answer: WhatEpisode },
];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
