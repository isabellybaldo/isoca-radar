import type { ReactElement } from 'react';
import type { EpisodeSummary, Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { useNow } from '../lib/hooks';
import { ladderModel } from '../lib/ladder';
import { isArbitrage } from '../pages/Radar';
import { CaughtBar } from '../schematics/CaughtBar';
import { DurationBars } from '../schematics/DurationBars';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { LockedBlocks } from '../schematics/LockedBlocks';
import { PayoffGrid } from '../schematics/PayoffGrid';
import { COPY, caughtText, onTable100, onTableDepth } from './copy';
import type { ExplainPage, ExplainState } from './ExplainContext';
import { EXAMPLE_EPISODE, durationRows, payoffKind, payoffStructure, positionKey } from './models';

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

/** Spec 7a §9.2: the episode's duration on the ladder axis, from the ladder the panel already fetched
 *  (only `measured` is used: no second fetch, and one drawing, D-04). */
function Caught({ s, ladder }: AnswerProps) {
  const e = s.data.episode?.detail?.episode;
  if (!e) return <p className="dim">{COPY.caughtWaiting}</p>;
  return (
    <>
      <CaughtBar durationMs={e.duration_ms} measured={ladder.data?.measured ?? null} />
      <p>{caughtText(e.duration_ms)}</p>
      <p className="dim">{COPY.caughtNotFilled}</p>
    </>
  );
}

function Locked({ s }: AnswerProps) {
  const now = useNow(60_000);
  const sel = s.selection?.kind === 'position' ? s.selection.position : null;
  const all = s.data.paper?.positions ?? [];
  // A selected position that has left the list since stays drawn (the selection is a snapshot).
  const list = sel && !all.some((p) => positionKey(p) === positionKey(sel)) ? [sel, ...all] : all;
  const open = list.filter((p) => !p.returned);
  return (
    <>
      {open.length === 0 ? <p>{COPY.nothingLocked}</p> : <LockedBlocks positions={open} selected={sel ? positionKey(sel) : null} nowMs={now} />}
      <p>{COPY.locked}</p>
    </>
  );
}

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [
  { id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero },
  { id: 'make-money', q: 'How can this make money?', pages: ['radar'], Answer: MakeMoney },
  { id: 'caught', q: 'Would we have caught it?', pages: ['episode'], Answer: Caught },
  { id: 'what-episode', q: 'What is an episode?', pages: ['episodes'], Answer: WhatEpisode },
  { id: 'locked', q: 'Why is money locked?', pages: ['paper'], Answer: Locked },
];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
