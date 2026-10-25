// Spec 7b §6.2, 2026-10-01 §3 (chapter 7): the dashboard's ladder schematic fed by the public snapshot. It
// brings its own "which markets" fold and its own "on paper" wording; this adds only the heading with the time.
import { frozenOn, isFrozen } from '../frozenMode';
import { ladderModel } from '../lib/ladder';
import { LatencyLadder } from '../schematics/LatencyLadder';
import { asOfLine } from './LiveStrip';
import { type PublicSnapshot, publicToLadder } from './snapshot';

export function LiveLadder({ snap, stale }: { snap: PublicSnapshot; stale: boolean }) {
  return (
    <div className="m-liveladder">
      <h3 className="m-live-h">{isFrozen() ? `on the scanner, ${frozenOn()}` : `on the live scanner, ${asOfLine(snap.written_ms, stale)}`}</h3>
      <LatencyLadder model={ladderModel(publicToLadder(snap.ladder))} />
    </div>
  );
}
