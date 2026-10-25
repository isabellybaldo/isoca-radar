// Spec 7b §6.1 / §6.3: today's counts from the public snapshot, in the story's chapter 3 (spec 2026-10-01 §3).
import { Money } from '../components/Money';
import { Spark } from '../components/Spark';
import { frozenOn, isFrozen } from '../frozenMode';
import { fmtUtcDayHm, fmtUtcHm } from '../lib/time';
import { type PublicToday, hourlyValues } from './snapshot';

export const BEHIND = 'the live writer is behind';
export const LIVE_UNAVAILABLE = 'live numbers unavailable';
/** The strip's heading and its accessible name. "today" is the api's day, which starts at 00:00 UTC. */
export const STRIP_TITLE = 'today (UTC) on the live scanner';
export const STRIP_TITLE_FROZEN = 'the last captured day (UTC)';

/** "as of HH:MM UTC"; stale: the day too, and that the writer is behind. Frozen: when it froze, never behind. */
export function asOfLine(writtenMs: number, stale: boolean): string {
  if (isFrozen()) return frozenOn();
  return stale ? `as of ${fmtUtcDayHm(writtenMs)}: ${BEHIND}` : `as of ${fmtUtcHm(writtenMs)} UTC`;
}

/** First load found no usable file: the one dim line; the rest of the page never depends on it. */
export function LiveUnavailable() {
  return <p className="dim m-live-n">{LIVE_UNAVAILABLE}</p>;
}

/** Today's arbitrage episodes (NO / YES), the edge on the table, the day by UTC hour, and when it was written. */
export function LiveStrip({ today, writtenMs, stale }: { today: PublicToday; writtenMs: number; stale: boolean }) {
  const title = isFrozen() ? STRIP_TITLE_FROZEN : STRIP_TITLE;
  return (
    <section className="m-live" aria-label={title}>
      <p className="m-live-k dim">{title}</p>
      <p className="m-live-n">
        <span className="m-big">{today.no}</span> NO · <span className="m-big">{today.yes}</span> YES <span className="dim">arbitrage episodes</span>
      </p>
      <p className="m-live-n">
        {today.edge_budget100 === null ? (
          <span className="dim">edge on the table: not measured</span>
        ) : (
          <>
            <span className="m-big"><Money value={today.edge_budget100} cents /></span> <span className="dim">edge on the table today, $100 per episode at most</span>
          </>
        )}
      </p>
      <div className="m-live-spark">
        <Spark values={hourlyValues(today)} label="arbitrage episodes per UTC hour today" />
      </div>
      <p className={`m-live-n ${stale ? 'stale' : 'dim'}`}>{asOfLine(writtenMs, stale)}</p>
    </section>
  );
}
