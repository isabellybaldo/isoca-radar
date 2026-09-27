/** Every flag sentence, in one place: the episode detail, the Episodes legend and tooltips, and Help. */
export const FLAG_WORDS = {
  suspect:
    "SUSPECT: one leg's book disagreed with the server's top while this was open. The stream repaired it; the numbers may still be optimistic.",
  stale: "STALE: at least one leg's feed went quiet while this was open, so its book may have been out of date.",
  censor_left: 'CENSORED (LEFT): it was already open when we started watching, so the real start is earlier.',
  censor_right: 'CENSORED (RIGHT): we stopped watching before it closed, so the real end is later.',
  censor_gap: 'CENSORED (GAP): the feed had a gap while it was open; the duration spans the gap.',
  incomplete: 'INCOMPLETE SET: the listed outcomes may not cover every result. Not an arbitrage if "Other" wins.',
  below_min: 'BELOW MIN SIZE: the size at the best price is under the venue minimum order.',
};

/** The list only knows "censored"; its tooltip names the three ways that happens. */
export const CENSORED_WORDS = `One of: ${FLAG_WORDS.censor_left} ${FLAG_WORDS.censor_right} ${FLAG_WORDS.censor_gap}`;
