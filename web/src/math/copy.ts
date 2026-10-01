import type { BasketWord } from '../schematics/BasketLegs';
// The public page's own copy (09-27 §5.1: no help.ts). Honest money words only: on paper, on the table,
// captured at N ms (Task 12 scans the rendered page). Every caption names a function that exists in the
// Rust code (ruling R7).
export const TITLE = 'isoca-radar · math';
export const TAGLINE =
  'How a basket arbitrage on Polymarket is found, sized and timed, and why it still loses. Every number below comes from the scanner’s own Rust code, compiled to WASM.';
export const FOOTER = 'The live dashboard is private. The math is not.';
export const ENGINE_FAILED = 'The math engine did not load. Your browser may be too old, or something blocked it.';
export const LOADING = 'Loading the math engine. The pixels are warming up.';
export const LIVE_MISSING = 'live numbers unavailable';

/** Section b's deadpan line per basket kind; each is true of that kind's matrix (fixtures yes-3-other, no-3-other, implication-prices). */
export const B_QUIPS = {
  yes: 'A YES basket is only risk-free if nothing outside it can win.',
  no: 'A NO basket pays whoever wins, and an outcome outside the set only adds to it.',
  implication: 'If A happens B must too, so one column cannot happen, and the basket holds in every other.',
} as const;

/** Section f's deadpan line per engine verdict word (BasketWord), so it never claims a miss the output does not show. */
export const F_QUIPS: Record<BasketWord, string> = {
  filled: 'Every leg filled the same amount, so nothing is left over. Drain one at t_x to see what a miss does.',
  caught: 'Some legs filled more than others. The extra shares are now a bet, and the exit is the bid.',
  'nothing filled': 'Nothing filled, so there is nothing to sell back.',
};

export const SECTIONS = {
  a: {
    title: 'a · one book, twice',
    quip: 'Same book, twice. If both sides ever look like free money, the free money is a bug.',
    caption:
      'Computed by arb_core::book::d1_sums and the Book mirror (arb_core::book::Book), with arb_core::detect::canary for the crossed-book alert: the scanner’s own code compiled to WASM.',
  },
  b: {
    title: 'b · payoff tables',
    quip: B_QUIPS.yes,
    caption: 'Computed by arb_core::payoff; the edge at the top of the book by arb_core::fill::walk, one share a leg, no fees.',
  },
  c: {
    title: 'c · classify one update',
    quip: 'Most updates end in “nothing”. That is the right answer most of the time.',
    caption: 'Computed by arb_core::classify::classify.',
  },
  d: {
    title: 'd · depth walk',
    quip: 'The first shares are the cheap ones. The walk stops where the next basket costs more than it pays.',
    caption: 'Computed by arb_core::fill::walk; the steps drawn come from arb_core::steps::walk_steps.',
  },
  e: {
    title: 'e · episode and latency',
    quip: 'The edge was real. It just did not wait.',
    caption:
      'Teaching model: one taker at t_x gets the edge of the latest update, or nothing once the episode closed; no queue, no consumed liquidity, no unwinds, so caught is not filled: see f. Computed by arb_core::classify::classify, arb_core::episode::EpisodeTracker and sim::latency::exec_delay_ms.',
  },
  f: {
    title: 'f · one leg misses',
    quip: F_QUIPS.filled,
    caption:
      'Computed by sim::policy::settle and sim::venue::{basket_buy, unwind_sell, basket_cash}: the same rules the paper accounts on the live scanner use; here the unwind happens at one instant, the live sim waits the account’s delay.',
  },
} as const;

/** Section c's static note (09-27 §6.2): shown beside the drawing, never a control. */
export const POST_END_NOTE = 'post_end is not here: the scanner adds it from event dates and it never changes the kind.';
