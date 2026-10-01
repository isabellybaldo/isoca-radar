// The pre-merge check of the 7b-B runbook: the page's own parser against the file the api REALLY wrote on the
// VPS, so a writer/parser mismatch is found before the deploy and not as "live numbers unavailable" after it.
//   PUBLIC_JSON=<path to the file> npx vitest run src/math/realSnapshot.test.ts --reporter=default
// It prints one line, `rungs N, markets N, hourly N`, and fails when the page would refuse the file. Without
// PUBLIC_JSON it is skipped with a printed reason; the check itself is tested below on the sample either way.
// @ts-ignore: node:fs exists at runtime under vitest (no @types/node installed)
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import sampleText from './__fixtures__/public.sample.json?raw';
import { type PublicSnapshot, parseSnapshot } from './snapshot';
import { MAX_BODY_BYTES } from './useSnapshot';

declare const process: { env: Record<string, string | undefined> };

const FILE = process.env.PUBLIC_JSON ?? '';
const SKIP_REASON =
  'realSnapshot: PUBLIC_JSON is not set, skipped (PUBLIC_JSON=<path to a public.json> npx vitest run src/math/realSnapshot.test.ts --reporter=default)';

/** The file as the page reads it (fetchSnapshot): at most MAX_BODY_BYTES, JSON, then parseSnapshot. Throws with
 *  the reason when the page would show "live numbers unavailable" for it. */
function readAsThePage(bytes: Uint8Array): { snap: PublicSnapshot; raw: unknown } {
  if (bytes.byteLength === 0) throw new Error('the file is empty');
  if (bytes.byteLength > MAX_BODY_BYTES) throw new Error(`the file is ${bytes.byteLength} bytes: the page refuses a body over ${MAX_BODY_BYTES}`);
  let raw: unknown;
  try {
    raw = JSON.parse(new TextDecoder().decode(bytes));
  } catch (e) {
    throw new Error(`the file is not JSON: ${e instanceof Error ? e.message : String(e)}`);
  }
  const snap = parseSnapshot(raw);
  if (snap === null) throw new Error('parseSnapshot refused the file: the writer (bins/api/src/public.rs) and the parser (web/src/math/snapshot.ts) disagree');
  return { snap, raw };
}

const summary = (s: PublicSnapshot) => `rungs ${s.ladder.rungs.length}, markets ${s.ladder.markets.length}, hourly ${s.today.hourly.length}`;

/** What the parser accepted but changed: market rows it dropped, a measured latency it read as not measured.
 *  Not failures (the page draws the rest), but worth a look before a merge. */
function notes(snap: PublicSnapshot, raw: unknown): string[] {
  const ladder = (raw as { ladder?: { markets?: unknown; measured?: unknown } }).ladder;
  const out: string[] = [];
  const written = Array.isArray(ladder?.markets) ? ladder.markets.length : 0;
  if (written !== snap.ladder.markets.length) out.push(`note: the file has ${written} market rows, the page keeps ${snap.ladder.markets.length}`);
  if (ladder?.measured !== null && snap.ladder.measured === null) out.push(`note: measured ${JSON.stringify(ladder?.measured)} reads as not measured`);
  return out;
}

const bytesOf = (text: string) => new TextEncoder().encode(text);

describe('the real-snapshot check itself, on the sample the api writes', () => {
  it('accepts the sample and summarises it', () => {
    const { snap, raw } = readAsThePage(bytesOf(sampleText));
    expect(snap.v).toBe(1);
    const written = JSON.parse(sampleText) as { ladder: { rungs: unknown[]; markets: unknown[] }; today: { hourly: unknown[] } };
    expect(summary(snap)).toBe(`rungs ${written.ladder.rungs.length}, markets ${written.ladder.markets.length}, hourly ${written.today.hourly.length}`);
    expect(notes(snap, raw)).toEqual([]);
  });

  it('refuses a broken copy, with the reason', () => {
    const broken = (change: (o: Record<string, unknown>) => unknown) => bytesOf(JSON.stringify(change(JSON.parse(sampleText) as Record<string, unknown>)));
    expect(() => readAsThePage(broken((o) => ({ ...o, v: 2 })))).toThrow(/parseSnapshot refused/);
    expect(() => readAsThePage(broken((o) => ({ ...o, today: undefined })))).toThrow(/parseSnapshot refused/);
    expect(() => readAsThePage(broken((o) => ({ ...o, ladder: { ...(o.ladder as object), epoch: 1.5 } })))).toThrow(/parseSnapshot refused/);
    expect(() => readAsThePage(bytesOf(sampleText.slice(0, sampleText.length - 20)))).toThrow(/not JSON/);
    expect(() => readAsThePage(bytesOf(''))).toThrow(/empty/);
    expect(() => readAsThePage(bytesOf(`${sampleText}${' '.repeat(MAX_BODY_BYTES)}`))).toThrow(/refuses a body over 65536/);
  });

  it('notes what the parser accepted but changed', () => {
    const o = JSON.parse(sampleText) as { ladder: { markets: Record<string, unknown>[] } };
    const changed = { ...o, ladder: { ...o.ladder, measured: { latency_ms: -5 }, markets: [...o.ladder.markets, { ...o.ladder.markets[0], bucket: 'Not A Slug' }] } };
    const { snap, raw } = readAsThePage(bytesOf(JSON.stringify(changed)));
    expect(notes(snap, raw)).toEqual([
      `note: the file has ${o.ladder.markets.length + 1} market rows, the page keeps ${o.ladder.markets.length}`,
      'note: measured {"latency_ms":-5} reads as not measured',
    ]);
  });
});

if (FILE === '') console.info(SKIP_REASON);

describe.skipIf(FILE === '')('the real public.json named by PUBLIC_JSON', () => {
  it('is a snapshot the page can draw: v 1, with the ladder and today', () => {
    const { snap, raw } = readAsThePage(readFileSync(FILE) as Uint8Array);
    expect(snap).not.toBeNull();
    expect(snap.v).toBe(1);
    expect(Array.isArray(snap.ladder.rungs) && Array.isArray(snap.ladder.markets)).toBe(true);
    expect(Array.isArray(snap.today.hourly)).toBe(true);
    console.info(summary(snap));
    for (const n of notes(snap, raw)) console.info(n);
  });
});
