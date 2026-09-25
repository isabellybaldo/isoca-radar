//! The venue as the sim sees it (spec §5.6): FAK orders walked against the feed's book minus the
//! liquidity this account already consumed. Buy and sell fees are USDC, rounded up per level fill.
use crate::consumed::{ConsumedLiquidity, LevelKey};
use crate::types::{BookSide, LegKey, Outcome};
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderFill {
    pub filled: Decimal,
    pub notional: Decimal,
    pub fees: Decimal,
    pub levels_used: u32,
}

impl OrderFill {
    pub fn vwap(&self) -> Option<Decimal> {
        if self.filled > Decimal::ZERO {
            self.notional.checked_div(self.filled)
        } else {
            None
        }
    }
}

/// The traded outcome's levels, best first: what we'd buy (asks) or sell into (bids). NO is L1.
pub fn outcome_levels(book: &Book, outcome: Outcome, buying: bool) -> Vec<(Price, Shares)> {
    match (outcome, buying) {
        (Outcome::Yes, true) => book.asks_best_first(),
        (Outcome::Yes, false) => book.bids_best_first(),
        (Outcome::No, true) => book.no_asks_best_first(),
        (Outcome::No, false) => book.no_bids_best_first(),
    }
}

/// The levels we would buy along, best first (used by policies and by S3c's engine hook).
pub fn bought_levels(book: &Book, outcome: Outcome) -> Vec<(Price, Shares)> {
    outcome_levels(book, outcome, true)
}

/// The YES-book level a trade at `price` of `leg.outcome` consumes (L1 mirror for NO).
pub fn yes_level(leg: &LegKey, buying: bool, price: Price) -> LevelKey {
    let (side, p) = match (leg.outcome, buying) {
        (Outcome::Yes, true) => (BookSide::Ask, price),
        (Outcome::Yes, false) => (BookSide::Bid, price),
        (Outcome::No, true) => (BookSide::Bid, price.complement()),
        (Outcome::No, false) => (BookSide::Ask, price.complement()),
    };
    LevelKey {
        yes_token: leg.yes_token.clone(),
        side,
        price: p.get(),
    }
}

fn take(
    book: &Book,
    leg: &LegKey,
    fee: FeeSchedule,
    limit: Option<Price>,
    qty: Decimal,
    consumed: &mut ConsumedLiquidity,
    buying: bool,
) -> OrderFill {
    let mut out = OrderFill::default();
    let mut left = qty;
    for (p, s) in outcome_levels(book, leg.outcome, buying) {
        if left <= Decimal::ZERO {
            break;
        }
        if let Some(l) = limit
            && ((buying && p > l) || (!buying && p < l))
        {
            break;
        }
        let key = yes_level(leg, buying, p);
        let q = consumed.available(&key, s.get()).min(left);
        if q <= Decimal::ZERO {
            continue;
        }
        consumed.consume(key, s.get(), q);
        out.filled += q;
        out.notional += p.get() * q;
        out.fees += fee.fee_conservative(p, q);
        out.levels_used += 1;
        left -= q;
    }
    out
}

/// FAK buy: take up to `qty` at prices ≤ `limit`; whatever isn't there is not filled.
pub fn fak_buy(
    book: &Book,
    leg: &LegKey,
    fee: FeeSchedule,
    limit: Price,
    qty: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> OrderFill {
    take(book, leg, fee, Some(limit), qty, consumed, true)
}

/// FAK sell into the bids with no floor (the leg-risk unwind, spec §5.6).
pub fn fak_sell(
    book: &Book,
    leg: &LegKey,
    fee: FeeSchedule,
    qty: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> OrderFill {
    take(book, leg, fee, None, qty, consumed, false)
}

/// Largest size ≤ `q` with ≤ 2 dp such that `limit × size` has ≤ 2 dp for every limit (a FAK buy's
/// USDC amount), or 0 if that is below `min` (5 shares, Q6).
pub fn order_size(limits: &[Price], q: Decimal, min: Decimal) -> Decimal {
    let step = Decimal::new(1, 2);
    let mut s = q.round_dp_with_strategy(2, RoundingStrategy::ToZero);
    // Prices have ≤ 6 dp, so a valid size is at most 10_000 steps below any start; bounded anyway.
    for _ in 0..100_000 {
        if s < min || s <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        if limits
            .iter()
            .all(|l| (l.get() * s).normalize().scale() <= 2)
        {
            return s;
        }
        s -= step;
    }
    Decimal::ZERO
}
