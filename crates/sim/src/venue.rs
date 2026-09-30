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

/// A NO→collateral conversion's terms (7b §3.2): `s` legs, `q` sets, the adapter fee in basis
/// points. Valued as `Account::convert` books it; the complement YES it returns count 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertTerms {
    pub s: usize,
    pub q: Decimal,
    pub fee_bips: Decimal,
}

/// Shares a leg still holds that were bought beyond the full sets and never sold, at their cost
/// basis (fees included). Cost still held, not cash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldUnsold {
    pub qty: Decimal,
    pub basis: Decimal,
}

/// A basket's cash, as the ledger books it (7b §3.2). Every figure is ≥ 0 except `net_cash`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasketCash {
    /// Σ buy notional + buy fees.
    pub spent: Decimal,
    /// Every fee paid: buys, unwinds and the conversion (already inside the other figures).
    pub fees: Decimal,
    /// Σ unwind notional − unwind fees.
    pub unwind_back: Decimal,
    /// (s − 1)·q − the conversion fee.
    pub convert_back: Decimal,
    /// unwind_back + convert_back − spent: the Σ cash_delta of the ledger rows.
    pub net_cash: Decimal,
    /// One per leg, in leg order.
    pub held_unsold: Vec<HeldUnsold>,
}

/// `Account::take_out` on a (qty, basis) pair: the same 12 dp basis rounding.
fn take_out(pos: &mut (Decimal, Decimal), qty: Decimal) -> (Decimal, Decimal) {
    let (held, basis) = *pos;
    let q = qty.min(held);
    if q <= Decimal::ZERO {
        return (Decimal::ZERO, Decimal::ZERO);
    }
    let out = if q == held {
        basis
    } else {
        (basis * q / held).round_dp_with_strategy(12, RoundingStrategy::MidpointNearestEven)
    };
    *pos = (held - q, basis - out);
    (q, out)
}

/// What a basket did to cash (7b §3.2), in the ledger's order: every buy, then each leg's unwind
/// (`unwinds[i]` is leg i's; a missing entry is no unwind), then the conversion when `convert` is
/// given (capped, like `Account::convert`, by what every leg still holds). `held_unsold` is each
/// leg's excess that was never sold: for a conversion everything left after it, otherwise what
/// is left beyond the `q_eff` full sets kept to resolution.
pub fn basket_cash(
    buy: &BasketBuy,
    unwinds: &[OrderFill],
    convert: Option<ConvertTerms>,
) -> BasketCash {
    let mut cash = BasketCash::default();
    let mut pos: Vec<(Decimal, Decimal)> = Vec::with_capacity(buy.per_leg.len());
    for f in &buy.per_leg {
        if f.filled > Decimal::ZERO {
            let cost = f.notional + f.fees;
            cash.spent += cost;
            cash.fees += f.fees;
            pos.push((f.filled, cost));
        } else {
            pos.push((Decimal::ZERO, Decimal::ZERO));
        }
    }
    for (i, u) in unwinds.iter().enumerate() {
        if u.filled > Decimal::ZERO {
            cash.unwind_back += u.notional - u.fees;
            cash.fees += u.fees;
            if let Some(p) = pos.get_mut(i) {
                take_out(p, u.filled);
            }
        }
    }
    if let Some(t) = convert
        && t.s >= 2
    {
        let held = pos.iter().map(|p| p.0).min().unwrap_or(Decimal::ZERO);
        let q = t.q.min(held);
        if q > Decimal::ZERO {
            for p in &mut pos {
                take_out(p, q);
            }
            let gross = (Decimal::from(t.s) - Decimal::ONE) * q;
            let fee = gross * t.fee_bips / Decimal::from(10_000);
            cash.convert_back = gross - fee;
            cash.fees += fee;
        }
    }
    cash.net_cash = cash.unwind_back + cash.convert_back - cash.spent;
    let kept = if convert.is_some() {
        Decimal::ZERO
    } else {
        buy.q_eff
    };
    cash.held_unsold = pos
        .into_iter()
        .map(|(qty, basis)| {
            let unsold = (qty - kept).max(Decimal::ZERO);
            let basis = if unsold == qty {
                basis
            } else {
                (basis * unsold / qty)
                    .round_dp_with_strategy(12, RoundingStrategy::MidpointNearestEven)
            };
            HeldUnsold { qty: unsold, basis }
        })
        .collect();
    cash
}
