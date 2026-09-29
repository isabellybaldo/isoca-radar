import { type ReactNode, useEffect, useRef, useState } from 'react';
import { ApiError, api } from '../api/client';
import type { Epochs } from '../api/types';
import type { ApiState } from '../api/useApi';
import { currentEpochText, nextAllowedText } from '../lib/epochs';
import { ErrorState } from './EmptyState';
import { Fold } from './Fold';
import { useNow } from '../lib/hooks';
import { Loadable } from './Loadable';

export const RESET_COPY = 'Starts every paper account again at its bankroll. Earlier epochs stay in the history.';
export const RESET_QUEUED = 'queued: the scanner applies it within ~30 s';
export const RESET_OFF = 'reset is off on this server';
export const RESET_WORD = 'reset';
/** D-02: a next-allowed time this close to now (or past) counts as allowed: the api's clock and ours differ a little. */
const SLACK_MS = 60_000;

type Phase = 'idle' | 'sending' | 'sent' | 'off' | 'failed';

function ResetBody({ e, now, reload }: { e: Epochs; now: number; reload: () => void }) {
  const [typed, setTyped] = useState('');
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<unknown>(undefined);
  const busy = useRef(false);
  // A fresh answer from the server replaces "sent": from then on its pending_request speaks.
  useEffect(() => {
    setPhase((p) => (p === 'sent' ? 'idle' : p));
  }, [e]);
  const send = async () => {
    if (busy.current || typed !== RESET_WORD) return;
    busy.current = true;
    setPhase('sending');
    try {
      await api.paperReset();
      setTyped('');
      setPhase('sent');
      reload();
    } catch (err) {
      if (err instanceof ApiError && (err.code === 'reset_pending' || err.code === 'reset_daily_limit')) {
        // Someone queued first, or today's reset is used: the refetched epochs say which.
        setTyped('');
        setPhase('idle');
        reload();
      } else if (err instanceof ApiError && err.code === 'reset_disabled') {
        setPhase('off');
      } else {
        setError(err);
        setPhase('failed');
      }
    } finally {
      busy.current = false;
    }
  };
  let state: ReactNode = null;
  if (e.pending_request !== null || phase === 'sent') state = <p className="quip" role="status">{RESET_QUEUED}</p>;
  else if (phase === 'off') state = <p className="quip" role="status">{RESET_OFF}</p>;
  else if (e.next_allowed_ms - now > SLACK_MS) state = <p className="quip" role="status">{nextAllowedText(e.next_allowed_ms)}</p>;
  return (
    <>
      <p>{currentEpochText(e)}</p>
      <p className="dim">{RESET_COPY}</p>
      {state ?? (
        <form
          className="resetform"
          onSubmit={(ev) => {
            ev.preventDefault();
            void send();
          }}
        >
          <label>
            type <b>{RESET_WORD}</b> to confirm{' '}
            <input
              type="text"
              aria-label="type reset to confirm"
              value={typed}
              onChange={(ev) => setTyped(ev.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <button type="submit" disabled={typed !== RESET_WORD || phase === 'sending'}>
            {phase === 'sending' ? 'SENDING…' : 'RESET'}
          </button>
        </form>
      )}
      {phase === 'failed' && <ErrorState error={error} />}
    </>
  );
}

/** Spec 7a §5.3: at the bottom of the Paper page, typed confirmation, once per UTC day, no dialogs. */
export function ResetFold({ epochs, now }: { epochs: ApiState<Epochs>; now?: number }) {
  // D-02: the clock is read at render (`now` is only for tests); the tick just makes an idle fold re-render.
  useNow(30_000);
  const at = now ?? Date.now();
  return (
    <Fold id="paper:reset" label="reset paper money">
      <Loadable state={epochs} what="epochs">
        {(e) => <ResetBody e={e} now={at} reload={epochs.reload} />}
      </Loadable>
    </Fold>
  );
}
