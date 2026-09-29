import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { EpisodeDetail, EpisodeSummary, PaperAccount, Position } from '../api/types';
import { readOpen, writeOpen } from './storage';

/** The pages that publish data to the panel (spec 7a §9.2); every other page gets the hero only. */
export type ExplainPage = 'radar' | 'episodes' | 'episode' | 'paper';
export type Selection = { kind: 'episode'; episode: EpisodeSummary } | { kind: 'position'; position: Position } | null;
export interface PageData {
  radar?: { open?: EpisodeSummary[] };
  episodes?: { items?: EpisodeSummary[] };
  episode?: { detail?: EpisodeDetail };
  paper?: { accounts?: PaperAccount[]; positions?: Position[] };
}
export interface ExplainState {
  page: ExplainPage | null;
  data: PageData;
  selection: Selection;
}

/** Stable for the provider's life: pages that only publish and select read this context alone, so a
 *  change of the panel's state (page, data, selection, open) never re-renders them. */
interface Actions {
  setOpen: (v: boolean) => void;
  select: (s: Selection) => void;
  publish: <P extends ExplainPage>(page: P, d: PageData[P] | null) => void;
}

interface State extends ExplainState {
  isOpen: boolean;
}

type Ctx = State & Actions;

// Outside a provider (page tests render pages alone) everything is a no-op.
const A = createContext<Actions>({ setOpen: () => {}, select: () => {}, publish: () => {} });
const S = createContext<State>({ page: null, data: {}, selection: null, isOpen: false });

export function ExplainProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<ExplainPage | null>(null);
  const [data, setData] = useState<PageData>({});
  const [selection, setSelection] = useState<Selection>(null);
  const [isOpen, setIsOpen] = useState(readOpen);
  const setOpen = useCallback((v: boolean) => {
    setIsOpen(v);
    writeOpen(v);
  }, []);
  const publish = useCallback(<P extends ExplainPage>(p: P, d: PageData[P] | null) => {
    if (d === null) {
      setPage((cur) => (cur === p ? null : cur));
      setSelection(null);
      setData((cur) => ({ ...cur, [p]: undefined }) as PageData);
      return;
    }
    setPage(p);
    setData((cur) => ({ ...cur, [p]: d }) as PageData);
  }, []);
  const actions = useMemo<Actions>(() => ({ setOpen, select: setSelection, publish }), [setOpen, publish]);
  const state = useMemo<State>(() => ({ page, data, selection, isOpen }), [page, data, selection, isOpen]);
  return (
    <A.Provider value={actions}>
      <S.Provider value={state}>{children}</S.Provider>
    </A.Provider>
  );
}

/** State and actions together, for the panel and its tab; pages use useExplainSelect / useExplainPage. */
export function useExplainCtx(): Ctx {
  return { ...useContext(S), ...useContext(A) };
}

export function useExplainSelect(): (s: Selection) => void {
  return useContext(A).select;
}

/** Same keys, each value the same identity: a fresh wrapper around the same api data is no change. */
function shallowEqual(a: object, b: object): boolean {
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/**
 * Spec 7a §9.3: pages publish what they already fetched; the panel only reads it. Pass api data
 * identities (useApi's `data`), never fresh literals: each new identity republishes. Unmounting (leaving
 * the page) clears the page and the selection.
 */
export function useExplainPage<P extends ExplainPage>(page: P, d: NonNullable<PageData[P]>): void {
  const { publish } = useContext(A);
  const last = useRef<object | null>(null);
  // No dependency list (its size must never change, and `d` may gain or lose keys): the shallow compare
  // against the last published object decides, so a fresh wrapper with the same values is not a publish.
  useEffect(() => {
    if (last.current !== null && shallowEqual(last.current, d)) return;
    last.current = d;
    publish(page, d);
  });
  useEffect(
    () => () => {
      last.current = null; // a new page (or a remount) publishes afresh
      publish(page, null);
    },
    [page, publish],
  );
}
