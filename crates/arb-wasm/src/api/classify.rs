//! `classify_json`: one update through the §5.7 (c) tree (09-27 §4.1).
use super::run;
use crate::input::{LegIn, d, group};
use arb_core::classify::{Node, classify};
use arb_core::detect::{Kind, Realization, Signal};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyIn {
    #[serde(default)]
    augmented: bool,
    legs: Vec<LegIn>,
}

#[derive(Serialize)]
struct ClassifyOut {
    path: Vec<&'static str>,
    kind: Option<&'static str>,
    realization: Option<&'static str>,
    net: Option<String>,
    gross: Option<String>,
    q_star: Option<String>,
}

fn node(n: Node) -> &'static str {
    match n {
        Node::Decided => "decided",
        Node::NoBasketTried => "no_basket_tried",
        Node::NoBasketFewLegs => "no_basket_few_legs",
        Node::NoBasketCrossed => "no_basket_crossed",
        Node::NoBasketNotPositive => "no_basket_not_positive",
        Node::NoBasketPositive => "no_basket_positive",
        Node::YesBasketTried => "yes_basket_tried",
        Node::YesBasketNoLive => "yes_basket_no_live",
        Node::YesBasketCrossed => "yes_basket_crossed",
        Node::YesBasketMissingAsk => "yes_basket_missing_ask",
        Node::YesBasketNotPositive => "yes_basket_not_positive",
        Node::YesBasketPositive => "yes_basket_positive",
        Node::Complete => "complete",
        Node::Incomplete => "incomplete",
    }
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::YesBasket => "yes_basket",
        Kind::YesBasketIncomplete => "yes_basket_incomplete",
        Kind::NoBasket => "no_basket",
        Kind::Implication => "implication",
    }
}

fn realization(r: Realization) -> &'static str {
    match r {
        Realization::Settlement => "settlement",
        Realization::Converter => "converter",
        Realization::NotArbitrage => "not_arbitrage",
    }
}

/// One update through the §5.7 (c) tree: the path taken and the detectors' answer.
pub fn classify_json(input: &str) -> String {
    run(input, |i: ClassifyIn| {
        let c = classify(&group(&i.legs, i.augmented)?);
        let s: Option<&Signal> = c.signal.as_ref();
        Ok(ClassifyOut {
            path: c.path.into_iter().map(node).collect(),
            kind: s.map(|s| kind(s.kind)),
            realization: s.map(|s| realization(s.realization)),
            net: s.map(|s| d(s.quote.pnl)),
            gross: s.map(|s| d(s.gross.pnl_gross)),
            q_star: s.map(|s| d(s.quote.q_max)),
        })
    })
}
