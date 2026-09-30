import { useEffect, useRef, useState } from 'react';
import { api } from './client';
import type { Epochs } from './types';
import { useApi, type ApiState } from './useApi';

/** Spec 7a §5.3: while a reset request is pending, ask again every 10 s; never otherwise. */
export const RESET_POLL_MS = 10_000;

export function useEpochs(): ApiState<Epochs> {
  const [poll, setPoll] = useState(0);
  const st = useApi((s) => api.paperEpochs(s), [], [poll]);
  const pending = st.data?.pending_request != null;
  useEffect(() => {
    if (!pending) return;
    const id = setInterval(() => setPoll((n) => n + 1), RESET_POLL_MS);
    return () => clearInterval(id);
  }, [pending]);
  return st;
}

/** Increments each time the current epoch changes after it was first seen (a reset was applied), so a
 *  page refetches its accounts once; the first answer never counts as a change. */
export function useEpochBump(current: number | undefined): number {
  const [bump, setBump] = useState(0);
  const seen = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (current === undefined) return;
    if (seen.current !== undefined && seen.current !== current) setBump((b) => b + 1);
    seen.current = current;
  }, [current]);
  return bump;
}
