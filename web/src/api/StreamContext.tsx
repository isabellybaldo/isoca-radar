import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { openStream, type StreamState } from './stream';

export interface StreamValue {
  state: StreamState;
  /** Increments at most once per EPISODE_COALESCE_MS while episode events arrive. */
  episodeTick: number;
  healthTick: number;
}

/** The scanner NOTIFYs on every episode write (open, peak, close): refetch at most once a second. */
export const EPISODE_COALESCE_MS = 1000;

const initial: StreamValue = { state: 'reconnecting', episodeTick: 0, healthTick: 0 };
const Ctx = createContext<StreamValue>(initial);

export function StreamProvider({ children }: { children: ReactNode }) {
  const [value, setValue] = useState<StreamValue>(initial);
  useEffect(() => {
    let pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const flush = () => {
      timer = undefined;
      if (pending) {
        pending = false;
        setValue((p) => ({ ...p, episodeTick: p.episodeTick + 1 }));
      }
    };
    const stop = openStream({
      onState: (state) => setValue((p) => (p.state === state ? p : { ...p, state })),
      onEpisode: () => {
        pending = true;
        if (timer === undefined) timer = setTimeout(flush, EPISODE_COALESCE_MS);
      },
      onHealth: () => setValue((p) => ({ ...p, healthTick: p.healthTick + 1 })),
    });
    return () => {
      stop();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, []);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useStream(): StreamValue {
  return useContext(Ctx);
}
