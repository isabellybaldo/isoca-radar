import type { ReactElement } from 'react';
import type { Ladder } from '../api/types';
import type { ApiState } from '../api/useApi';
import { Loadable } from '../components/Loadable';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import type { ExplainPage, ExplainState } from './ExplainContext';

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

/** Spec 7a §9.2: the hero first on every page, then the page's own question. */
export const QUESTIONS: Question[] = [{ id: HERO_ID, q: 'At what latency would this pay?', pages: 'all', Answer: Hero }];

export function questionsFor(page: ExplainPage | null): Question[] {
  return QUESTIONS.filter((q) => q.pages === 'all' || (page !== null && q.pages.includes(page)));
}
