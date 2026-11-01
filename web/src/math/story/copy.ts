// The story's own copy (spec 2026-10-01 §5, §6). No figure lives here: payouts are "one dollar", every number
// a reader sees comes from the engine or the snapshot. A line that states an outcome is picked by the engine's
// own flag (TAKE), never fixed.
import type { BasketWord } from '../../schematics/BasketLegs';

export const HOOK =
  'Sometimes the prices of every outcome of an event add up to less than the one dollar the winner pays. In theory, that gap is free money. This page shows how a scanner finds it, sizes it and times it, and what can go wrong between seeing it and holding it. Chapters 1 to 6 run the scanner’s own Rust code, compiled to WASM; chapter 7 shows its live paper accounts.';
export const HOOK_FROZEN =
  'Sometimes the prices of every outcome of an event add up to less than the one dollar the winner pays. In theory, that gap is free money. This page shows how a scanner finds it, sizes it and times it, and what can go wrong between seeing it and holding it. Chapters 1 to 6 run the scanner’s own Rust code, compiled to WASM; chapter 7 shows its paper accounts as they stood when capture stopped on 2026-10-19.';

export interface ChapterCopy {
  n: number;
  id: string;
  question: string;
  lead: string;
  /** The switch's accessible name. */
  switchLabel: string;
  /** null: chapter 3 (its labels are built from the preset's ticks) and chapter 7 (live data, no switch). */
  options: readonly [string, string] | null;
}

export const CHAPTERS: readonly ChapterCopy[] = [
  {
    n: 1, id: 's1', question: 'What is being traded?',
    lead: 'A market asks one yes-or-no question. A YES share pays one dollar if the answer is yes, a NO share pays one dollar if it is no. So an offer to sell YES at a price p (an ask) is the same offer as a bid for NO at 1 − p: one list of bids and asks, a book, drawn twice.',
    switchLabel: 'the book shown', options: ['a healthy book', 'a crossed book'],
  },
  {
    n: 2, id: 's2', question: 'What happens with several outcomes?',
    lead: 'Think of an election with candidates A, B and C, each with its own market. Hold one YES share of each: a basket, and each market in it is a leg. If one of the three must win, it pays one dollar whoever it is.',
    switchLabel: 'the outcomes listed', options: ['every outcome is listed', 'one outcome is not listed'],
  },
  {
    n: 3, id: 's3', question: 'When is that free money?',
    lead: 'A complete basket always pays one dollar. When its prices add up to less, the difference, on every basket the books can fill, is the edge, written Π: money on the table before anyone has traded. Buying one NO share of each works too: all but one of them pay, whoever wins, and the edge is there when the YES bids add up to more than one dollar.',
    switchLabel: 'what the prices add up to', options: null,
  },
  {
    n: 4, id: 's4', question: 'How much of it is there?',
    lead: 'Each price is good for a few shares only. Buy more and you reach the next, worse price. Walk down the books basket by basket and stop where the next one would no longer pay more than it costs: the size reached is Q*.',
    switchLabel: 'how deep the books are', options: ['a deep book', 'a thin book'],
  },
  {
    n: 5, id: 's5', question: 'How long does it last?',
    lead: 'An edge opens on one price update and closes on a later one: an episode. An order needs time to be decided, to travel and to be matched; it lands at t_x. If the episode has closed by then, there is nothing left to take.',
    switchLabel: 'how long the edge stays', options: ['it outlasts the order', 'it closes first'],
  },
  {
    n: 6, id: 's6', question: 'What if you get only part of it?',
    lead: 'A basket is several orders, one per leg. If one leg has no shares left at t_x, the others still fill, and those extra shares are a bet, not a basket. The way out is to sell them back at the bid: an unwind. The drawing uses a NO basket, which can be converted back into cash at once.',
    switchLabel: 'what fills', options: ['every leg fills', 'one leg misses'],
  },
  {
    n: 7, id: 's7', question: 'So does it pay?',
    lead: 'The live scanner keeps six paper accounts that differ only in latency: the six rungs of a ladder, fastest to slowest. They start together from one clean slate, an epoch, and settle every trade by the rules of chapter 6.',
    switchLabel: '', options: null,
  },
];

export const CH7_LEAD_FROZEN =
  'The scanner kept six paper accounts that differed only in latency: the six rungs of a ladder, fastest to slowest. They started together from one clean slate, an epoch, and settled every trade by the rules of chapter 6. Here they are as they stood on 2026-10-19, when the host began limiting the server’s CPU and capture ended early.';

export const TAKE = {
  ch1: (crossed: boolean) =>
    crossed ? 'A bid above the ask would be free money inside one market. The scanner distrusts its feed instead.' : 'One book, two views.',
  ch2: (broken: boolean) =>
    broken
      ? 'Someone outside the basket can win, and that column pays nothing. Not an arbitrage: see ✗.'
      : 'The basket pays the same in every column. That is what risk-free means here.',
  ch3: (signal: boolean) =>
    signal
      ? 'The gap, on every basket the books can fill, is the edge. The scanner runs this check each time a price moves.'
      : 'At exactly one dollar there is nothing to take.',
  ch4: 'The edge has a size. A thinner book reaches it sooner.',
  ch5: (caught: boolean) =>
    caught ? 'The order landed in time. Whether every leg fills is the next chapter.' : 'The edge was real. It just did not wait.',
  ch6: (word: BasketWord): string =>
    ({
      filled: 'Every leg filled the same amount, so nothing is left over.',
      caught: 'Some legs filled more than others. The extra shares are now a bet, and the exit is the bid.',
      'nothing filled': 'Nothing filled, so there is nothing to sell back.',
    })[word],
  ch7: 'Every figure here is on paper. No order was ever sent.',
} as const;

export const CH3_COUNT = 'The scanner counts each stretch an edge stays open as one episode. Today’s count:';
export const CH7_EXTRAS =
  'extras: one account whose matching is slower and slows further when the feed is busy, and one that ignores a market’s own order delay.';

export const PLAY = {
  title: 'Try it yourself',
  line: 'The engine behind each chapter, with every control. Nothing you do here leaves your browser.',
} as const;

export const WORDS: readonly (readonly [string, string])[] = [
  ['YES / NO share', 'pays one dollar if the answer is yes / no.'],
  ['book, bid, ask', 'the list of offers to buy (bids) and to sell (asks) one share type.'],
  ['basket, leg', 'one share of every outcome of an event; each market in it is a leg.'],
  ['NO basket', 'one NO share of each; all but one pay.'],
  ['edge (Π)', 'what a basket pays over what it costs, on every basket the books can fill.'],
  ['Q*', 'how many baskets are worth buying before the next costs as much as it pays.'],
  ['episode', 'one stretch during which an edge stays open.'],
  ['t_x', 'the moment an order lands at the exchange.'],
  ['unwind', 'selling extra shares back at the bid.'],
  ['convert', 'turning a NO basket back into cash at the exchange.'],
  ['on the table', 'an edge the books show right now, before any order.'],
  ['on paper', 'what a simulated account would have gained or lost; no order was sent.'],
  ['rung', 'one paper account on the latency ladder; rung-25 is the one at 25 ms.'],
  ['epoch', 'one run of the paper accounts from a clean slate.'],
  ['secondsDelay', 'a market’s own hold on an incoming order, in seconds.'],
];
