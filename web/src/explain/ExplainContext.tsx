import { type ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
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

interface Ctx extends ExplainState {
  isOpen: boolean;
  setOpen: (v: boolean) => void;
  select: (s: Selection) => void;
  publish: <P extends ExplainPage>(page: P, d: PageData[P] | null) => void;
}

// Outside a provider (page tests render pages alone) everything is a no-op.
const NOOP: Ctx = { page: null, data: {}, selection: null, isOpen: false, setOpen: () => {}, select: () => {}, publish: () => {} };
const C = createContext<Ctx>(NOOP);

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
  const value = useMemo<Ctx>(
    () => ({ page, data, selection, isOpen, setOpen, select: setSelection, publish }),
    [page, data, selection, isOpen, setOpen, publish],
  );
  return <C.Provider value={value}>{children}</C.Provider>;
}

export function useExplainCtx(): Ctx {
  return useContext(C);
}

export function useExplainSelect(): (s: Selection) => void {
  return useContext(C).select;
}

/**
 * Spec 7a §9.3: pages publish what they already fetched; the panel only reads it. Pass api data
 * identities (useApi's `data`), never fresh literals: each new identity republishes. Unmounting (leaving
 * the page) clears the page and the selection.
 */
export function useExplainPage<P extends ExplainPage>(page: P, d: NonNullable<PageData[P]>): void {
  const { publish } = useContext(C);
  const deps = Object.values(d);
  useEffect(() => {
    publish(page, d);
    // `d` is a fresh object each render; its values say when it really changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, publish, ...deps]);
  useEffect(() => () => publish(page, null), [page, publish]);
}
