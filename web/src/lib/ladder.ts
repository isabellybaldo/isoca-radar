import type { BreakEven, Ladder, LadderRung, MarketsStatus, SweetRow } from '../api/types';
import { fmtUtcDayHm, fmtUtcHm } from './time';

/**
 * Spec 7a §2: the ladder, fastest first. The same six names and latencies as `crates/cards`
 * `edge::LADDER` and `config/policies.toml`; ladder.test.ts checks them against the live response
 * and against policies.toml. The web keeps its own copy so a rung the api leaves out (no account
 * yet) is still drawn (spec §17).
 */
export const LADDER: readonly { scenario: string; latency_ms: number }[] = [
  { scenario: 'rung-0', latency_ms: 0 },
  { scenario: 'rung-25', latency_ms: 25 },
  { scenario: 'colo-idle', latency_ms: 52 },
  { scenario: 'rung-100', latency_ms: 100 },
  { scenario: 'rung-200', latency_ms: 200 },
  { scenario: 'sp-idle', latency_ms: 300 },
];
/** The YES side's LADDER columns (spec §8): the LADDER scenarios with a P1-settlement account. */
export const LADDER_YES: readonly string[] = ['colo-idle', 'sp-idle'];
/** SWEET's minimum executed attempts (`cards::sweet::MIN_EXEC`): below it a rung is thin. */
export const MIN_EXEC = 30;
/** Six rungs plus one slot beside sp-idle for the extras (sp-idle-nodelay, sp-burst). */
export const SLOTS = LADDER.length + 1;

export type LadderColumn =
  | { kind: 'missing'; scenario: string; latency_ms: number }
  | { kind: 'rung'; scenario: string; latency_ms: number; latencyKnown: boolean; rung: LadderRung };

/** Why a `thin` answer is thin, so the big line never says "too few trades" beside rungs that all have enough. */
export type ThinReason = 'few' | 'missing' | 'unknown' | 'no_epoch';

export interface LadderModel {
  epoch: number;
  startMs: number | null;
  asOfMs: number | null;
  /** Always the six LADDER rungs, in order. */
  columns: LadderColumn[];
  extras: LadderRung[];
  breakEven: BreakEven;
  thinReason: ThinReason | null;
  measured: { latency_ms: number; host: string } | null;
  markets: { status: MarketsStatus; rows: SweetRow[] };
}

export function ladderModel(l: Ladder): LadderModel {
  const columns = LADDER.map(({ scenario, latency_ms }): LadderColumn => {
    const rung = l.rungs.find((r) => !r.extra && r.scenario === scenario);
    return rung
      ? { kind: 'rung', scenario, latency_ms, latencyKnown: rung.latency_ms !== null, rung }
      : { kind: 'missing', scenario, latency_ms };
  });
  const present = columns.flatMap((c) => (c.kind === 'rung' ? [c] : []));
  let thinReason: ThinReason | null = null;
  if (l.break_even.kind === 'thin') {
    if (l.start_ms === null) thinReason = 'no_epoch';
    else if (present.some((c) => c.rung.n_exec < MIN_EXEC)) thinReason = 'few';
    else if (present.length < LADDER.length) thinReason = 'missing';
    else if (present.some((c) => !c.latencyKnown)) thinReason = 'unknown';
    else thinReason = 'few';
  }
  return {
    epoch: l.epoch,
    startMs: l.start_ms,
    asOfMs: l.as_of_ms,
    columns,
    extras: l.rungs.filter((r) => r.extra),
    breakEven: l.break_even,
    thinReason,
    measured: l.measured,
    markets: { status: l.markets_status, rows: l.markets },
  };
}

/** Centre of slot `i` as a fraction of the width: the x axis is ordinal, equal spacing per rung (spec §7.2). */
export function slotX(i: number, slots: number = SLOTS): number {
  return (i + 0.5) / slots;
}

/**
 * A latency on the ordinal axis: linear between its two neighbouring rungs' slot centres; below 0 ms it
 * sits on rung-0; beyond the last rung it follows the last segment's slope; always within [slotX(0), 1].
 */
export function latencyX(ms: number, slots: number = SLOTS): number {
  let i = 0;
  while (i < LADDER.length - 2 && ms > LADDER[i + 1].latency_ms) i++;
  const a = LADDER[i];
  const b = LADDER[i + 1];
  const x = slotX(i, slots) + ((ms - a.latency_ms) / (b.latency_ms - a.latency_ms)) * (slotX(i + 1, slots) - slotX(i, slots));
  return Math.min(1, Math.max(slotX(0, slots), x));
}

/** Spec §2: "us" is a band from measured + 50 to measured + 150 ms (matching), never a point. */
export function usBand(m: { latency_ms: number }, slots: number = SLOTS): { x0: number; x1: number } {
  return { x0: latencyX(m.latency_ms + 50, slots), x1: latencyX(m.latency_ms + 150, slots) };
}

export interface Segment {
  part: 'gains' | 'settle' | 'unrealized' | 'leg_risk';
  from: number;
  to: number;
}

const PARTS = ['gains', 'settle', 'unrealized', 'leg_risk'] as const;

/** One rung's stacked bar (spec §7.2): each part by its own sign, positives up from 0 and negatives down
 *  from 0, in the order gains, settle, unrealized, leg-risk. Numbers are geometry only. */
export function stackRung(r: Pick<LadderRung, 'gains' | 'settle' | 'unrealized' | 'leg_risk'>): Segment[] {
  let up = 0;
  let down = 0;
  const out: Segment[] = [];
  for (const part of PARTS) {
    const v = Number(r[part]);
    if (!Number.isFinite(v) || v === 0) continue;
    if (v > 0) {
      out.push({ part, from: up, to: up + v });
      up += v;
    } else {
      out.push({ part, from: down, to: down + v });
      down += v;
    }
  }
  return out;
}

/** The y range holding every stack end and every net, zero always inside, 8 % padding; [−1, 1] when all is zero. */
export function yDomain(rungs: LadderRung[]): { lo: number; hi: number } {
  let hi = 0;
  let lo = 0;
  for (const r of rungs) {
    for (const s of stackRung(r)) {
      hi = Math.max(hi, s.from, s.to);
      lo = Math.min(lo, s.from, s.to);
    }
    const n = Number(r.net);
    if (Number.isFinite(n)) {
      hi = Math.max(hi, n);
      lo = Math.min(lo, n);
    }
  }
  if (hi === lo) return { lo: -1, hi: 1 };
  const pad = (hi - lo) * 0.08;
  return { lo: lo - pad, hi: hi + pad };
}

export interface BreakEvenText {
  big: string;
  line: string;
}

const THIN: Record<ThinReason, BreakEvenText> = {
  few: { big: 'too few trades yet', line: `Each rung needs ${MIN_EXEC} trades; the counts are under the drawing.` },
  missing: { big: 'a rung has no account yet', line: 'Every rung needs its own paper account before the ladder can answer.' },
  unknown: { big: 'rung latencies unknown', line: 'The api has no policies file, so it cannot place the rungs.' },
  no_epoch: { big: 'no epoch yet', line: 'The ladder counts from the start of the first epoch.' },
};

export const NOT_MONOTONE = 'The sign changes more than once along the ladder; this is the first change.';

/** The one big number (spec §7.2) and its line: "pays on paper" / "loses on paper", never "profitable". */
export function breakEvenText(m: LadderModel): BreakEvenText {
  const be = m.breakEven;
  if (be.kind === 'at') {
    return {
      big: `≈ ${be.latency_ms} ms`,
      line: be.direction === 'loses_beyond' ? 'Pays on paper below it, loses on paper beyond it.' : 'Loses on paper below it, pays on paper beyond it.',
    };
  }
  if (be.kind === 'all') return { big: 'pays at every rung', line: 'Pays on paper at every latency on the ladder.' };
  if (be.kind === 'none') return { big: 'none', line: 'Loses on paper at every latency on the ladder.' };
  return THIN[m.thinReason ?? 'few'];
}

/** A column's ms under the drawing; "?" when the api could not place it. */
export function rungMsText(c: LadderColumn): string {
  return c.kind === 'rung' && !c.latencyKnown ? '?' : String(c.latency_ms);
}

/** Spec §17: while thin, each rung shows n / 30 so the owner sees how far it is; a missing rung says so. */
export function rungCountText(c: LadderColumn, thin: boolean): string {
  if (c.kind === 'missing') return 'no account';
  return thin ? `${c.rung.n_exec} / ${MIN_EXEC}` : String(c.rung.n_exec);
}

/** Spec §17: the sums stop at the latest common mark, not at "now". */
export function asOfText(ms: number | null): string {
  return ms === null ? 'no common mark yet' : `as of ${fmtUtcHm(ms)} UTC`;
}

/** The y-axis label: what the bars are, and since when (the hero's time frame is the epoch). */
export function epochText(m: Pick<LadderModel, 'epoch' | 'startMs'>): string {
  return m.startMs === null ? '$ on paper · no epoch has started yet' : `$ on paper since epoch ${m.epoch} began, ${fmtUtcDayHm(m.startMs)}`;
}

/** The us-band label (spec §7.2). */
export function usText(m: { latency_ms: number } | null): string {
  return m === null ? 'us: not measured yet' : `us (measured ≈${m.latency_ms} + matching)`;
}

/** The extras are annotated markers, never rungs (spec §2): name what each one is. */
export function extraText(r: LadderRung): string {
  const ms = r.latency_ms === null ? '?' : String(r.latency_ms);
  if (r.scenario === 'sp-burst') return `sp-burst (${ms} ms idle, more in bursts)`;
  if (r.scenario === 'sp-idle-nodelay') return `sp-idle-nodelay (${ms} ms, secondsDelay ignored)`;
  return `${r.scenario} (${ms} ms)`;
}
