//! The venue as the sim sees it (spec §5.6): FAK orders walked against the feed's book minus the
//! liquidity this account already consumed. Buy and sell fees are USDC, rounded up per level fill.
use crate::consumed::{ConsumedLiquidity, LevelKey};
use crate::types::{BookSide, LegKey, Outcome};
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Read access to the engine's YES books. It lives here, not in `trader`, so the pure fill
/// functions below never refer back to the trader (7b §3.2); `trader` re-exports it.
pub trait Books {
    fn yes_book(&self, yes_token: &str) -> Option<&Book>;
}

impl Books for HashMap<String, Book> {
    fn yes_book(&self, yes_token: &str) -> Option<&Book> {
        self.get(yes_token)
    }
}

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

/// One leg of a basket buy (7b §3.2): the token, its fee schedule and the policy's limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillLeg {
    pub key: LegKey,
    pub fee: FeeSchedule,
    pub limit: Price,
}

/// What a basket buy got: one fill per leg (in `legs` order), the full sets it made (`q_eff`,
/// the smallest leg fill, 0 with no legs) and each leg's shares beyond them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasketBuy {
    pub per_leg: Vec<OrderFill>,
    pub q_eff: Decimal,
    /// `per_leg[i].filled − q_eff`: the leg-risk inventory the sim unwinds.
    pub excess: Vec<Decimal>,
}

/// The paper sim's execution at t_x (spec §5.6, 7b §3.2): a FAK buy of `q_target` on every leg at
/// its limit, against the books minus `consumed`, in leg order. A leg whose book is missing gets an
/// empty fill.
pub fn basket_buy(
    books: &impl Books,
    legs: &[FillLeg],
    q_target: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> BasketBuy {
    let per_leg: Vec<OrderFill> = legs
        .iter()
        .map(|l| match books.yes_book(&l.key.yes_token) {
            Some(b) => fak_buy(b, &l.key, l.fee, l.limit, q_target, consumed),
            None => OrderFill::default(),
        })
        .collect();
    let q_eff = per_leg
        .iter()
        .map(|f| f.filled)
        .min()
        .unwrap_or(Decimal::ZERO);
    let excess = per_leg.iter().map(|f| f.filled - q_eff).collect();
    BasketBuy {
        per_leg,
        q_eff,
        excess,
    }
}

/// The leg-risk unwind (spec §5.6, 7b §3.2): rounds the queued `qty` and the `held` position
/// (the raw quantity the account holds) to 2 dp toward zero and asks for the smaller, so a stale
/// unwind never sells shares the account no longer has. Below `min_order`, or with no book, no
/// order goes out: the fill is empty and the requested quantity is still returned (the sim's
/// zero-fill row keeps the held leg risk visible).
pub fn unwind_sell(
    book: Option<&Book>,
    leg: &LegKey,
    fee: FeeSchedule,
    qty: Decimal,
    held: Decimal,
    min_order: Decimal,
    consumed: &mut ConsumedLiquidity,
) -> (Decimal, OrderFill) {
    let held = held.round_dp_with_strategy(2, RoundingStrategy::ToZero);
    let requested = qty
        .round_dp_with_strategy(2, RoundingStrategy::ToZero)
        .min(held);
    let fill = match book {
        Some(b) if requested >= min_order => fak_sell(b, leg, fee, requested, consumed),
        _ => OrderFill::default(),
    };
    (requested, fill)
}
