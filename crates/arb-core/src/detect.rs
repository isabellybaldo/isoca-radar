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

/// D3 (L10): for a curated pair where A ⇒ B, buy NO_A + YES_B. Both legs must be live.
pub fn detect_implication(a: &GroupLeg, b: &GroupLeg) -> Option<Signal> {
    if a.status != LegStatus::Live || b.status != LegStatus::Live {
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
