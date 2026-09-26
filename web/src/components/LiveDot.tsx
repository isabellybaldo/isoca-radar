import type { StreamState } from '../api/stream';

/** Solid while the stream is connected, hollow while reconnecting (spec §5 header). */
export function LiveDot({ state }: { state: StreamState }) {
  const live = state === 'live';
  return (
    <span className="live" role="status" aria-live="polite">
      <span className={live ? 'dot' : 'dot hollow'} aria-hidden="true" />
      {live ? 'LIVE' : 'RECONNECTING'}
    </span>
  );
}
