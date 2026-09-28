//! The scanner engine's book rules (Task 3). For now: the one expression the engine shares.
use arb_core::money::Price;
use rust_decimal::Decimal;

/// Polymarket reports an EMPTY bid side as best_bid "0" and an EMPTY ask side as best_ask "1"
/// (live smoke test 2026-09-25): no real order rests at either, so neither is a level. The
/// engine calls this too, so the two can never disagree about it.
pub fn server_top(
    best_bid: Option<Price>,
    best_ask: Option<Price>,
) -> (Option<Price>, Option<Price>) {
    (
        best_bid.filter(|x| x.get() != Decimal::ZERO),
        best_ask.filter(|x| x.get() != Decimal::ONE),
    )
}
