import { FLAG_WORDS } from './lib/flags';

/** (?) copy for every section of every page (spec §8.3): short, plain, deadpan but clear. */
export interface HelpText {
  title: string;
  text: string;
}

export const HELP = {
  health: {
    title: 'What are the hearts?',
    text: 'Ten hearts is a healthy scanner: every shard connected, no dropped messages in the last 5 minutes, feed lag under a second, capture running. Each trouble costs hearts and is named in words next to them. A clock more than 250 ms away from Polymarket’s also costs a heart: after a host freeze the feed lag reads wrong until the clock is back.',
  },
  open: {
    title: 'What is an episode?',
    text: 'One stretch of time in which buying a full basket of one event (every YES, or every NO) cost less than it pays out, after taker fees. It opens on the first order-book update where that is true and closes on the first where it is not. Open ones sit here while they last, which is usually not long.',
  },
  today: {
    title: 'What does today count?',
    text: 'Arbitrage episodes opened since 00:00 UTC: complete YES and NO baskets only, by side. Incomplete sets are not counted. Edge on the table adds up each one’s profit had you spent at most $100 on it; an episode whose $100 figure was not measured is left out of the sum, not counted as zero. The last line compares the edge that was there with what a 145 ms trader would have caught.',
  },
  columns: {
    title: 'What do the columns mean?',
    text: 'KIND: YES buys every YES token (exactly one pays $1); NO buys every NO token and converts them back to collateral. S: how many outcomes are in the set bought. PROFIT @ $100: the net profit at the peak if you spent at most $100. NET @ DEPTH and GROSS: the profit after and before taker fees, taking all the depth the books offered. Q*: that executable size, in shares. AGE: time since it opened. EDGE: the per-share edge, drawn as a bar relative to the biggest open one.',
  },
  incomplete: {
    title: 'Why is an incomplete set not arbitrage?',
    text: 'The listed outcomes may not cover every result: an outcome outside the set (often "Other") can win, and then every token bought pays nothing. The price gap is real, the guarantee is not. They are listed for context and counted nowhere as money.',
  },
  filters: {
    title: 'What do the filters do?',
    text: 'They narrow the list by kind, day and flags, and they live in the URL, so a filtered view can be bookmarked. Nothing here changes the data, only what you see of it. UTC days. Seven days at a time.',
  },
  episodes: {
    title: 'What is in this list?',
    text: 'Every live episode, newest first, a page at a time. NET is the best one-shot profit after fees at the peak, Q* the size that profit needs. Flags mark episodes the engine did not fully trust; they stay listed, and cards exclude them where the method says so.',
  },
  timing: {
    title: 'What do the times mean?',
    text: 'Open and close are the exchange server times of the first and last qualifying book update. Duration is close minus open. Censored means we did not see the whole episode: it was already open at a start or reconnect, a feed gap interrupted it, or the scanner stopped first.',
  },
  peak: {
    title: 'What is the peak?',
    text: 'The moment the one-shot profit was largest. Edge is per share, Q* is how many shares the book could take at that edge, and net budget $100 is the profit if you spent at most $100. It is measured, not simulated; getting it is a separate question.',
  },
  flags: {
    title: 'What do the flags mean?',
    text: `Flags mark episodes the engine did not fully trust. ${FLAG_WORDS.suspect} ${FLAG_WORDS.stale} ${FLAG_WORDS.censor_left} ${FLAG_WORDS.censor_right} ${FLAG_WORDS.censor_gap} OUR METHOD on the cards drops suspect and stale episodes and says how many.`,
  },
  legs: {
    title: 'What are legs and S?',
    text: 'A leg is one outcome of the event. S is the set of legs actually bought at the peak; legs outside S had no ask worth taking. The table shows each leg’s price, depth used and fees.',
  },
  rechecks: {
    title: 'What are the re-checks?',
    text: 'One, two and five seconds after an episode opens, the scanner asks the exchange for the same books again. They show whether the edge was still there for someone slower than the websocket. Skipped means the re-check budget was full.',
  },
  equity: {
    title: 'What is equity?',
    text: 'Cash plus positions marked at the current prices, for each paper account in the chosen scenario. Every account starts with the same bankroll. P3 naive is the control (it buys incomplete sets too); hide it when its drawdown squashes the other lines. No real money exists anywhere in this project.',
  },
  paper: {
    title: 'What is paper trading?',
    text: 'Simulated accounts that act on live episodes under stated latency and fee assumptions, with no connection to any exchange. They show what the strategies would have done, not what anyone did.',
  },
  edge: {
    title: 'Why is the NO side cashable and the YES side locked?',
    text: 'A full NO basket can be converted back to collateral right away, so its profit is cash within seconds. A full YES basket pays only when the event resolves, which can take weeks. Edge on the table is an upper bound: it assumes we were first in line at the best moment, which nobody is every time.',
  },
  window: {
    title: 'What does the window change?',
    text: 'How far back the charts and the integrity count look. Health is kept per minute; the longest window is a week.',
  },
  capture: {
    title: 'What is capture?',
    text: 'The scanner keeps a compressed copy of every raw message for replay. It pauses when the disk budget is tight and deletes the oldest hours first, so the budget is never exceeded.',
  },
  archive: {
    title: 'What is the archive?',
    text: 'Raw capture is kept for about a day. Every closed hour is reduced to compact tables (top of book, depth every 5 minutes, full detail around arbitrage episodes, and the stretches where data is missing) and kept for months inside a 40 GB budget, oldest days deleted first. Lag is how many hours the compactor is behind; above 3, raw hours may be deleted before they are reduced, and a heart is lost.',
  },
  perMinute: {
    title: 'What is in the per-minute row?',
    text: 'The latest health minute: connections, messages, drops, reconnects, feed lag and REST timings. If it is old, the scanner is not reporting and everything else on the page may be stale.',
  },
  msgs: {
    title: 'What is msgs/s?',
    text: 'Websocket messages received per second, averaged per minute. Bursts come with news and market opens; that is when latency matters most.',
  },
  lag: {
    title: 'What is lag?',
    text: 'The time between the exchange stamping a message and the scanner receiving it, at the median and the 99th percentile. It is part of the measured latency on the Edge capture chart.',
  },
  drops: {
    title: 'What are drops and reconnects?',
    text: 'Drops are messages the scanner could not process in time; reconnects are websocket sessions that had to be reopened. Both censor episodes that were open at the time.',
  },
  integrity: {
    title: 'What is integrity?',
    text: 'Checks that the books we hold agree with the exchange: crossed books, mirror mismatches, drifts and gaps, per million messages. It is our own version of the paper’s validation appendix.',
  },
  papers: {
    title: 'What is this page?',
    text: 'Their number, their method on our data, our method. Same market, different weeks. Published results on prediction-market arbitrage, recomputed live on our own data. Each card shows the paper’s number, the paper’s method on our data, and our method, with the sample size, a status and a hash that identifies exactly what was computed.',
  },
  gebele: {
    title: 'What does this paper measure?',
    text: 'How often executable arbitrage appears on Polymarket, on which side, how long it lasts and how big it is, from order-book data over five weeks in 2026. The cards replay those measurements on what the scanner sees, every 15 minutes.',
  },
  otherPapers: {
    title: 'Why is this paper not live?',
    text: 'Some results need data we do not have yet (on-chain history) or were dropped. They are listed so the page does not pretend they were never read.',
  },
  notReplicated: {
    title: 'What is not replicated, and why?',
    text: 'Results that come from on-chain conversions or from the paper’s own message validation. We do not observe them, so we do not print a number we cannot compute.',
  },
  status: {
    title: 'What does warming mean?',
    text: 'live: the whole window is covered by data computed under the current premises. warming: the premises changed or the data starts inside the window, so it is only partly filled. thin: too few episodes for the number to mean much; it is dimmed, with the reason. under review: the premises changed and the history is being recomputed.',
  },
  g1: {
    title: 'What is side asymmetry?',
    text: 'Whether mispricing shows up more on the YES side (all YES tokens together cost under $1) or the NO side (all NO tokens cost under the number of outcomes minus one). The paper found YES far more often. The merge rows show how much flickering books inflate the count. Why is our NO side so much bigger than the paper’s? Most of our NO episodes last about 27 ms (median). They come from crypto price-bucket markets (e.g. "XRP price on Sep 30?") where one market maker requotes every bucket, one at a time. For a few milliseconds the old and new bids overlap and the NO basket looks free. It’s real on the book, and gone before anyone at our latency can take it. Count only episodes lasting ≥ 100 ms and YES leads about 2.8 : 1, the same direction as the paper, whose hourly snapshots would rarely catch a 27 ms flicker.',
  },
  g2: {
    title: 'What is Kaplan–Meier?',
    text: 'A way to estimate how long episodes last when some were still running when we stopped watching. Instead of dropping those, it counts them as lasted-at-least-this-long. The paper uses the plain share of finished episodes longer than τ, drawn next to it.',
  },
  g3: {
    title: 'What is the long-lived share?',
    text: 'The share of episodes that last 50 minutes or more. The paper measured it inside one-hour windows, which can only see episodes present at both ends of the hour; our method measures whole durations and reports how much the one-hour view shifts the median.',
  },
  g4: {
    title: 'What is net edge by side?',
    text: 'The best per-share edge of each episode after taker fees, in basis points (1 bp = $0.0001 per $1), binned by 25 bp. Gross, before fees, is drawn behind it. The paper shows this only as a figure, so only its counts are quoted.',
  },
  sweet: {
    title: 'What is a sweet spot?',
    text: 'A slice of the paper attempts (a category, a time to resolution, a basket size, an hour of day) where the edge we actually captured, after fees and waiting for our money, stays above zero even at the edge of its interval. Almost nothing qualifies. That is the point of looking.',
  },
  sweetThin: {
    title: 'Why are most rows thin?',
    text: 'A row needs at least 30 executed attempts spread over at least 30 event-hours before it gets any verdict. Weeks of data spread over about 200 rows leave most of them short.',
  },
  sweetBonferroni: {
    title: 'What is Bonferroni, in one line?',
    text: 'Test enough rows and one looks good by luck; each interval is widened by the number of rows tested, so a false candidate shows up less than 1 time in 20 overall.',
  },
  sweetClusters: {
    title: 'Why count events, not trades?',
    text: 'One event can open many episodes in an hour, and those trades share the same books and the same competitors. Counting them as independent would make the intervals look tighter than they are.',
  },
  sweetYes: {
    title: 'Why is YES beyond 30 days not tested?',
    text: 'The YES paper account only buys complete baskets that resolve within 30 days and clear a 10% annualized edge. Anything else it skips, so there is nothing captured to measure; the row shows the edge on the table and whether even that clears the hurdle. The annualized figure is a simple annual rate, days floored at 7 like P1.',
  },
  sweetNo: {
    title: 'What does the NO mean measure?',
    text: 'Dollars per $100 per trade, not a total: the NO account trades up to about $100 an attempt, so the mean is what one attempt like that made or lost. It is not annualized: the NO side gets its money back in seconds, so there is no lock-up to annualize away.',
  },
} satisfies Record<string, HelpText>;

export type HelpId = keyof typeof HELP;
