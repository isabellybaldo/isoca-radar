import type { Epochs, PaperAccount } from '../api/types';
import { LADDER } from './ladder';
import { isZero } from './money';
import { fmtUtcDayHm } from './time';

/** An earlier epoch's final equity is its last mark; marks are per minute, so its last 6 h are plenty. */
export const EPOCH_HOURS = 6;
/** Spec §17: P3-naive loses the same amount at every latency; it is the fee-blind control. */
export const P3_NOTE = 'control: ignores fees';

/** Spec 7a §6: the line above ACCOUNTS. An epoch started by new policies is not called a reset. */
export function epochLine(e: Epochs): string {
  const cur = e.epochs.find((x) => x.epoch === e.current);
  if (!cur) return 'epoch 0 · no reset yet';
  return `${cur.cause === 'reset' ? 'since reset' : 'since new policies'} ${fmtUtcDayHm(cur.start_ms)} · epoch ${cur.epoch}`;
}

export interface EpochSpan {
  epoch: number;
  /** null for epoch 0: it is everything before epoch 1. */
  start_ms: number | null;
  end_ms: number;
  cause: string;
}

/** Every epoch before the current one, newest first; each ends where the next one starts. */
export function earlierEpochs(e: Epochs): EpochSpan[] {
  const list = [...e.epochs].sort((a, b) => a.epoch - b.epoch);
  return list
    .map((x, i): EpochSpan =>
      i === 0
        ? { epoch: 0, start_ms: null, end_ms: x.start_ms, cause: 'before epochs' }
        : { epoch: list[i - 1].epoch, start_ms: list[i - 1].start_ms, end_ms: x.start_ms, cause: list[i - 1].cause },
    )
    .reverse();
}

/** Spec 7a §6: the P0 accounts at fee 0 (the ladder and its extras), LADDER order, then by name. */
export function epochAccounts(accounts: PaperAccount[]): PaperAccount[] {
  const rank = (a: PaperAccount) => {
    const i = LADDER.findIndex((l) => l.scenario === a.scenario);
    return i === -1 ? LADDER.length : i;
  };
  return accounts
    .filter((a) => a.policy === 'P0-converter' && isZero(a.fee_bips))
    .sort((x, y) => rank(x) - rank(y) || x.scenario.localeCompare(y.scenario));
}

/** Spec 7a §5.3: the reset fold's first line, the current epoch and its start. */
export function currentEpochText(e: Epochs): string {
  const cur = e.epochs.find((x) => x.epoch === e.current);
  return cur ? `epoch ${cur.epoch}, started ${fmtUtcDayHm(cur.start_ms)} (${cur.cause})` : 'epoch 0: no epoch has started yet';
}

export function nextAllowedText(ms: number): string {
  return `next reset allowed after ${fmtUtcDayHm(ms)}`;
}
