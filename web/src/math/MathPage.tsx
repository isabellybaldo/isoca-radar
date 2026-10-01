import { useEffect, useState } from 'react';
import type { Engine } from '../wasm/types';
import { ENGINE_FAILED, FOOTER, LOADING, TAGLINE, TITLE } from './copy';
import { LiveBoundary } from './LiveBoundary';
import { LiveLadder } from './LiveLadder';
import { LiveStrip, LiveUnavailable } from './LiveStrip';
import './math.css';
import { ClassifySection } from './sections/ClassifySection';
import { DepthSection } from './sections/DepthSection';
import { EpisodeSection } from './sections/EpisodeSection';
import { LegMissSection } from './sections/LegMissSection';
import { MirrorSection } from './sections/MirrorSection';
import { PayoffSection } from './sections/PayoffSection';
import { freshness } from './snapshot';
import { useSnapshot } from './useSnapshot';

type EngineLoad = { kind: 'loading' } | { kind: 'failed' } | { kind: 'ok'; engine: Engine };

/** The engine promise as state. A rejection is handled here, so it is never an unhandled one. */
function useEngine(engine: Promise<Engine>): EngineLoad {
  const [st, setSt] = useState<EngineLoad>({ kind: 'loading' });
  useEffect(() => {
    let live = true;
    engine.then(
      (e) => {
        if (live) setSt({ kind: 'ok', engine: e });
      },
      () => {
        if (live) setSt({ kind: 'failed' });
      },
    );
    return () => {
      live = false;
    };
  }, [engine]);
  return st;
}

/**
 * 09-27 §6.1 + 7b §6.1: one column: the header, the live strip, sections a to f, and a line saying the
 * dashboard is private. No router, no api client, no help.ts, no link out. The snapshot and the engine are
 * independent, and each says so on its own when it is missing:
 * - the snapshot (one request, `useSnapshot`, called here only): loading draws nothing live; missing, or a
 *   live part that threw, is the one `LiveUnavailable` line; ok draws the strip here and the ladder at the end
 *   of f; stale is the same with the line the two components already carry. Freshness is `freshness(snap,
 *   checkedAt)`: re-evaluated on a fetch attempt or a return to the tab, never by a timer.
 * - the engine: one neutral line while it loads, one sentence if it does not; a to f exist only once it has.
 * `now` is the page's one clock (tests inject it).
 */
export function MathPage({ engine, now }: { engine: Promise<Engine>; now?: () => number }) {
  const eng = useEngine(engine);
  const load = useSnapshot(now);
  /** written_ms of the snapshot a live part threw on; a newer snapshot gets a fresh try (the boundaries are keyed by it). */
  const [brokeOn, setBrokeOn] = useState<number | null>(null);
  const snap = load.kind === 'ok' && load.snap.written_ms !== brokeOn ? load.snap : null;
  const stale = snap !== null && freshness(snap, load.checkedAt) === 'stale';
  const unavailable = load.kind === 'missing' || (load.kind === 'ok' && snap === null);
  return (
    <div className="app math">
      <header className="top m-top">
        <h1>{TITLE}</h1>
      </header>
      <main className="body m-body">
        <p className="m-tag">{TAGLINE}</p>
        {snap !== null && (
          <LiveBoundary key={snap.written_ms} onError={() => setBrokeOn(snap.written_ms)}>
            <LiveStrip today={snap.today} writtenMs={snap.written_ms} stale={stale} />
          </LiveBoundary>
        )}
        {unavailable && <LiveUnavailable />}
        {eng.kind === 'loading' && <p className="dim" role="status">{LOADING}</p>}
        {eng.kind === 'failed' && <p className="error" role="alert">{ENGINE_FAILED}</p>}
        {eng.kind === 'ok' && (
          <>
            <MirrorSection engine={eng.engine} />
            <PayoffSection engine={eng.engine} />
            <ClassifySection engine={eng.engine} />
            <DepthSection engine={eng.engine} />
            <EpisodeSection engine={eng.engine} />
            <LegMissSection engine={eng.engine}>
              {snap !== null && (
                <LiveBoundary key={snap.written_ms} onError={() => setBrokeOn(snap.written_ms)}>
                  <LiveLadder snap={snap} stale={stale} />
                </LiveBoundary>
              )}
            </LegMissSection>
          </>
        )}
        <p className="m-foot">{FOOTER}</p>
      </main>
    </div>
  );
}
