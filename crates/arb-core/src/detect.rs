//! Detectors (spec §5.3): D1 canary, D2a YES basket, D2b NO basket, D3 implication.
use crate::book::Book;
use crate::fee::FeeSchedule;
use crate::fill::{BasketQuote, BasketRule, BuyLeg, walk};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegStatus {
    Live,
    Placeholder,
    Other,
    ClosedNo,
    ClosedYes,
    Halted,
}

#[derive(Debug, Clone)]
pub struct GroupLeg {
    pub status: LegStatus,
    pub book: Book,
    pub fee: FeeSchedule,
}

/// One NegRisk event: every outcome, tradeable or not.
#[derive(Debug, Clone)]
pub struct Group {
    pub legs: Vec<GroupLeg>,
    pub augmented: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    YesBasket,
    YesBasketIncomplete,
    NoBasket,
    Implication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Realization {
    Settlement,
    Converter,
    NotArbitrage,
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub kind: Kind,
    pub realization: Realization,
    /// Walk with each leg's real fee schedule.
    pub quote: BasketQuote,
    /// Walk with zero fees (for gross-only episodes and paper comparisons).
    pub gross: BasketQuote,
    /// Indices into the group's legs (or [0, 1] for an implication pair).
    pub legs_used: Vec<usize>,
}

impl Signal {
    pub fn net_positive(&self) -> bool {
        self.quote.pnl > Decimal::ZERO
    }
}

/// D1 (L2): true means our local state is wrong, never an opportunity.
pub fn canary(book: &Book) -> bool {
    book.is_crossed()
}

fn run(legs: Vec<BuyLeg>, rule: BasketRule) -> (BasketQuote, BasketQuote) {
    let gross_legs: Vec<BuyLeg> = legs
        .iter()
        .map(|l| BuyLeg {
            levels: l.levels.clone(),
            fee: FeeSchedule::ZERO,
        })
        .collect();
    (walk(&legs, rule, None), walk(&gross_legs, rule, None))
}

fn decided(g: &Group) -> bool {
    g.legs.iter().any(|l| l.status == LegStatus::ClosedYes)
}

/// D2a (L3): buy one YES of every live leg. Arbitrage only if the set is complete.
/// D1: a crossed book means our local state is wrong, never a signal — returns `None` if any
/// live leg's book is crossed.
pub fn detect_yes_basket(g: &Group) -> Option<Signal> {
    if decided(g) {
        return None;
    }
    let live: Vec<usize> = (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live)
        .collect();
    if live.is_empty() {
        return None;
    }
    if live.iter().any(|&i| g.legs[i].book.is_crossed()) {
        return None;
    }
    let legs: Vec<BuyLeg> = live
        .iter()
        .map(|&i| BuyLeg {
            levels: g.legs[i].book.asks_best_first(),
            fee: g.legs[i].fee,
        })
        .collect();
    if legs.iter().any(|l| l.levels.is_empty()) {
        return None;
    }
    let (quote, gross) = run(
        legs,
        BasketRule::AllLegs {
            payoff: Decimal::ONE,
        },
    );
    if gross.pnl_gross <= Decimal::ZERO {
        return None;
    }
    let complete = !g.augmented
        && g.legs
            .iter()
            .all(|l| matches!(l.status, LegStatus::Live | LegStatus::ClosedNo));
    let (kind, realization) = if complete {
        (Kind::YesBasket, Realization::Settlement)
    } else {
        (Kind::YesBasketIncomplete, Realization::NotArbitrage)
    };
    Some(Signal {
        kind,
        realization,
        quote,
        gross,
        legs_used: live,
    })
}

/// D2b (L4, L5, L11): buy NO on every live leg that has a YES bid; legs exit as they exhaust.
/// D1: a crossed book means our local state is wrong, never a signal — returns `None` if any
/// used leg's book is crossed.
pub fn detect_no_basket(g: &Group) -> Option<Signal> {
    if decided(g) {
        return None;
    }
    let used: Vec<usize> = (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live && g.legs[i].book.top().best_bid.is_some())
        .collect();
    if used.len() < 2 {
        return None;
    }
    if used.iter().any(|&i| g.legs[i].book.is_crossed()) {
        return None;
    }
    let legs: Vec<BuyLeg> = used
        .iter()
        .map(|&i| BuyLeg {
            levels: g.legs[i].book.no_asks_best_first(),
            fee: g.legs[i].fee,
        })
        .collect();
    let (quote, gross) = run(legs, BasketRule::NestedNo);
    if gross.pnl_gross <= Decimal::ZERO {
        return None;
    }
    Some(Signal {
        kind: Kind::NoBasket,
        realization: Realization::Converter,
        quote,
        gross,
        legs_used: used,
    })
}

/// Spec §5.4/§8.2 threshold for D2a: can `detect_yes_basket(g)` return `Some`? O(legs),
/// reads only each live leg's top of book, allocates nothing.
///
/// Contract: `false` ⇒ `detect_yes_basket(g)` is `None`, so a caller may skip the depth walk.
/// `true` on an undecided group with no crossed live leg ⇒ it is `Some`.
///
/// Proof: the detector walks only when every live leg (≥ 1) has an ask. Its zero-fee walk
/// (`run(…).1`, the one `pnl_gross` comes from) then starts with every live leg at its best ask
/// (`asks_best_first()[0] == top().best_ask`), so its first-segment edge is m = 1 − ΣA, exact in
/// `Decimal` (≤ 6 dp prices, ≤ 100 legs). `walk` breaks before taking anything when m ≤ 0, which
/// leaves `pnl_gross = 0`, so the detector returns `None`. Conversely m > 0 takes a first segment
/// of positive size (books hold only sizes > 0), and every segment walked has m > 0, so
/// `pnl_gross > 0`. Decided and crossed groups are left to the detector, which rejects them itself.
/// The threshold is the detector's own GROSS one on purpose: D2a reports gross-positive signals
/// (`Signal.gross`) even when fees make them net-negative.
pub fn yes_basket_may_signal(g: &Group) -> bool {
    let mut sum_ask = Decimal::ZERO;
    let mut any_live = false;
    for leg in g.legs.iter().filter(|l| l.status == LegStatus::Live) {
        let Some((ask, _)) = leg.book.top().best_ask else {
            return false; // an incomplete basket: the detector returns None too
        };
        sum_ask += ask.get();
        any_live = true;
    }
    any_live && sum_ask < Decimal::ONE
}

/// Spec §5.4/§8.2 threshold for D2b: can `detect_no_basket(g)` return `Some`? O(legs).
///
/// Contract: as `yes_basket_may_signal`. Proof: the detector's zero-fee `NestedNo` walk starts
/// with the k ≥ 2 live legs that have a YES bid, each at its best NO ask 1 − b_i
/// (`no_asks_best_first()[0] == top().best_bid.complement()`), paying k − 1, so its first edge is
/// m = (k − 1) − Σ(1 − b_i) = ΣB − 1, exact in `Decimal`. m ≤ 0 ⇒ `pnl_gross = 0` ⇒ `None`;
/// m > 0 ⇒ `pnl_gross > 0` ⇒ `Some` on an undecided group with no crossed used leg.
pub fn no_basket_may_signal(g: &Group) -> bool {
    let mut sum_bid = Decimal::ZERO;
    let mut used = 0usize;
    for leg in g.legs.iter().filter(|l| l.status == LegStatus::Live) {
        if let Some((bid, _)) = leg.book.top().best_bid {
            sum_bid += bid.get();
            used += 1;
        }
    }
    used >= 2 && sum_bid > Decimal::ONE
}

/// D3 (L10): for a curated pair where A ⇒ B, buy NO_A + YES_B. Both legs must be live.
/// D1: a crossed book means our local state is wrong, never a signal — returns `None` if either
/// leg's book is crossed.
pub fn detect_implication(a: &GroupLeg, b: &GroupLeg) -> Option<Signal> {
    if a.status != LegStatus::Live || b.status != LegStatus::Live {
        return None;
    }
    if a.book.is_crossed() || b.book.is_crossed() {
        return None;
    }
    let legs = vec![
        BuyLeg {
            levels: a.book.no_asks_best_first(),
            fee: a.fee,
        },
        BuyLeg {
            levels: b.book.asks_best_first(),
            fee: b.fee,
        },
    ];
    if legs.iter().any(|l| l.levels.is_empty()) {
        return None;
    }
    let (quote, gross) = run(
        legs,
        BasketRule::AllLegs {
            payoff: Decimal::ONE,
        },
    );
    if gross.pnl_gross <= Decimal::ZERO {
        return None;
    }
    Some(Signal {
        kind: Kind::Implication,
        realization: Realization::Settlement,
        quote,
        gross,
        legs_used: vec![0, 1],
    })
}
