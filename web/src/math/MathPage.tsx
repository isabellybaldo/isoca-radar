import { type ReactNode, useEffect, useRef, useState } from 'react';
import { Fold } from '../components/Fold';
import { FINDING, FROZEN_LINE, isFrozen } from '../frozenMode';
import type { Engine } from '../wasm/types';
import { ENGINE_FAILED, FOOTER, FOOTER_FROZEN, HOME_LINK, LOADING, TITLE } from './copy';
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
import { type PublicSnapshot, freshness } from './snapshot';
import { HOOK, HOOK_FROZEN, PLAY } from './story/copy';
import { Story } from './story/Story';
import { Words } from './story/Words';
import { useSnapshot } from './useSnapshot';

/** The fold the word list waits in, at the top of the playground: closed until asked for. */
export const WORDS_LABEL = 'the words used on this page';

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

/** The sections exist only once the engine has loaded, so the browser's own jump to /math#d finds nothing on a
 *  cold load; and the strip and the ladder arrive with the snapshot, above the playground, so a jump made
 *  before them would be pushed away. `ready`: the engine is there and the snapshot has settled (ok or missing).
 *  Then go to the section or the playground the address names: once, with the browser's default behaviour (no
 *  smooth scrolling of our own, so reduced motion stays the visitor's setting). Chapters are not link targets. */
function useSectionLink(ready: boolean): void {
  const done = useRef(false);
  useEffect(() => {
    if (!ready || done.current) return;
    done.current = true;
    const m = /^#([a-f]|play)$/.exec(window.location.hash);
    if (m !== null) document.getElementById(m[1])?.scrollIntoView();
  }, [ready]);
}

/**
 * Spec 2026-10-01 §3 (on 09-27 §6.1 and 7b §6.1): one column: the header, the hook, the story's seven
 * chapters, the playground (its word list in a fold, then sections a to f) and a line saying the dashboard is private.
 * No router, no api client, no help.ts, no link out. The snapshot and the engine are independent, and each says
 * so on its own when it is missing:
 * - the snapshot (one request, `useSnapshot`, called here only): loading draws nothing live; ok draws the strip
 *   in chapter 3 and the ladder in chapter 7; missing, or a live part that threw, is the `LiveUnavailable` line
 *   in both places; stale is the same with the line the two components already carry. Freshness is
 *   `freshness(snap, checkedAt)`: re-evaluated on a fetch attempt or a return to the tab, never by a timer.
 * - the engine: one neutral line while it loads, one sentence if it does not, above the story. The story's
 *   prose and the playground's heading and word list never wait for it; the drawings and a to f do.
 * `now` is the page's one clock (tests inject it).
 */
export function MathPage({ engine, now }: { engine: Promise<Engine>; now?: () => number }) {
  const eng = useEngine(engine);
  const load = useSnapshot(now);
  useSectionLink(eng.kind === 'ok' && load.kind !== 'loading');
  /** written_ms of the snapshot a live part threw on; a newer snapshot gets a fresh try (the boundaries are keyed by it). */
  const [brokeOn, setBrokeOn] = useState<number | null>(null);
  const snap = load.kind === 'ok' && load.snap.written_ms !== brokeOn ? load.snap : null;
  const frozen = isFrozen();
  // Frozen: the file is old by design, so it is never "behind".
  const stale = !frozen && snap !== null && freshness(snap, load.checkedAt) === 'stale';
  const unavailable = load.kind === 'missing' || (load.kind === 'ok' && snap === null);
  /** A live part in its place: the part while the snapshot is good, the "unavailable" line when it is missing
   *  or a part threw on it, and exactly null while it loads (the story prints nothing around a null). */
  const live = (draw: (s: PublicSnapshot) => ReactNode): ReactNode => {
    if (snap !== null) {
      return (
        <LiveBoundary key={snap.written_ms} onError={() => setBrokeOn(snap.written_ms)}>
          {draw(snap)}
        </LiveBoundary>
      );
    }
    return unavailable && !frozen ? <LiveUnavailable /> : null;
  };
  return (
    <div className="app math">
      <header className="top m-top">
        <h1>{TITLE}</h1>
        {frozen && <a className="dim" href="/">{HOME_LINK}</a>}
        {frozen && <p className="dim">{FROZEN_LINE}</p>}
      </header>
      <main className="body m-body">
        {frozen && <p className="m-tag"><strong>{FINDING}</strong></p>}
        <p className="m-tag">{frozen ? HOOK_FROZEN : HOOK}</p>
        {eng.kind === 'loading' && <p className="dim" role="status">{LOADING}</p>}
        {eng.kind === 'failed' && <p className="error" role="alert">{ENGINE_FAILED}</p>}
        <Story
          engine={eng.kind === 'ok' ? eng.engine : null}
          // Frozen: the file's `today` is the day after capture stopped (all zeros), not a captured day: no strip.
          strip={frozen ? null : live((s) => <LiveStrip today={s.today} writtenMs={s.written_ms} stale={stale} />)}
          ladder={live((s) => <LiveLadder snap={s} stale={stale} />)}
          ladderDrawn={snap !== null}
        />
        <section id="play" className="m-play" aria-labelledby="m-play">
          <h2 id="m-play">{PLAY.title}</h2>
          <p className="m-ch-lead">{PLAY.line}</p>
          <Fold id="math:words" label={WORDS_LABEL}>
            <Words />
          </Fold>
          {eng.kind === 'ok' && (
            <>
              <MirrorSection engine={eng.engine} />
              <PayoffSection engine={eng.engine} />
              <ClassifySection engine={eng.engine} />
              <DepthSection engine={eng.engine} />
              <EpisodeSection engine={eng.engine} />
              <LegMissSection engine={eng.engine} />
            </>
          )}
        </section>
      </main>
      <footer className="m-foot">
        {frozen ? (
          <p>
            {FOOTER_FROZEN} <a href="/">{HOME_LINK}</a>
          </p>
        ) : (
          <p>{FOOTER}</p>
        )}
      </footer>
    </div>
  );
}
