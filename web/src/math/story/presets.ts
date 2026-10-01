// The story's engine inputs (spec 2026-10-01 §4): two per chapter, built from the playground's own starting
// states and input builders, so a chapter asks the engine the same kind of question its playground section
// does. They need not draw the same thing: chapter 3 draws its preset's prices as a stack (the engine is not
// asked for that picture), and presets.test.ts pins that the engine's answer agrees with it (no signal at a
// sum of 1.00, a YES basket below it), as it pins what every other chapter's words rely on.
import { tickText } from '../../schematics/geo';
import type { BasketIn, ClassifyIn, DepthWalkIn, EpisodeIn, MirrorIn, PayoffIn } from '../../wasm/types';
import { basketInput, drain } from '../basket';
import { F_BOOKS, F_BURST, F_START } from '../defaults';
import { type ClassifyState, classifyInput } from '../sections/ClassifySection';
import { DEPTH_START, depthInput } from '../sections/DepthSection';
import { EPISODE_START, episodeInput } from '../sections/EpisodeSection';
import { MIRROR_START, crossed } from '../sections/MirrorSection';
import { PAYOFF_START, payoffInput } from '../sections/PayoffSection';

export type Pair<T> = readonly [T, T];

export const CH1: Pair<MirrorIn> = [MIRROR_START, crossed(MIRROR_START)];

/** A YES basket over three outcomes: with "Other" listed every column pays; without it one column pays nothing. */
export const CH2: Pair<PayoffIn> = [
  payoffInput({ ...PAYOFF_START, kind: 'yes', s: 3, listed: true, priced: false }),
  payoffInput({ ...PAYOFF_START, kind: 'yes', s: 3, listed: false, priced: false }),
];

/** Best asks in ticks: Σ 1.00 (nothing to take), then Σ 0.97 (a YES-basket edge). Bids sit one tick below. */
const CH3_ASKS: Pair<number[]> = [[33, 33, 34], [30, 32, 35]];
const ch3State = (asks: number[]): ClassifyState => ({
  augmented: false,
  legs: asks.map((ask) => ({ status: 'live', bid: ask - 1, ask })),
});
export const CH3: Pair<ClassifyIn> = [classifyInput(ch3State(CH3_ASKS[0])), classifyInput(ch3State(CH3_ASKS[1]))];
/** The sum of whole ticks as a decimal string, integer maths only (100 → "1.00"). */
const sumText = (ticks: number[]) => tickText(ticks.reduce((a, t) => a + t, 0));
export const CH3_LABELS: Pair<string> = [`prices add to ${sumText(CH3_ASKS[0])}`, `prices add to ${sumText(CH3_ASKS[1])}`];

/** The thin book: each leg's two cheapest levels at a quarter of their size, the third as it is. (Quartering
 *  every level would leave the stairs' picture unchanged: they scale to their last step.) */
export const CH4: Pair<DepthWalkIn> = [
  depthInput(DEPTH_START),
  depthInput({ ...DEPTH_START, legs: DEPTH_START.legs.map((l) => [...drain(l.slice(0, 2), 25), ...l.slice(2)]) }),
];

/** The second state's clock: the same five updates, the last four 50 ms apart, so the episode closes at 550 ms. */
export const SHORT_T = [0, 400, 450, 500, 550];
export const CH5: Pair<EpisodeIn> = [episodeInput(EPISODE_START), episodeInput(EPISODE_START, SHORT_T)];

export const CH6: Pair<BasketIn> = [basketInput(F_BOOKS, F_START), basketInput(F_BOOKS, F_BURST)];
