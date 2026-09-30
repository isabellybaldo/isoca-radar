//! `payoff_json`: the §5.7 (b) table; with prices, the gross edge at top of book (09-27 §4.1).
use super::run;
use crate::input::{d, price};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use arb_core::payoff;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceIn {
    bid: String,
    ask: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayoffIn {
    kind: String,
    s: usize,
    other_listed: bool,
    prices: Option<Vec<PriceIn>>,
}

#[derive(Serialize)]
struct PayoffOut {
    rows: Vec<String>,
    cols: Vec<String>,
    possible: Vec<bool>,
    cells: Vec<Vec<Option<String>>>,
    basket: Vec<Option<String>>,
    guaranteed: String,
    broken: Vec<bool>,
    top_edge_gross: Option<String>,
}

fn one_share(p: Price) -> Result<Vec<(Price, Shares)>, String> {
    Ok(vec![(
        p,
        Shares::new(Decimal::ONE).map_err(|e| e.to_string())?,
    )])
}

/// The §5.7 (b) payoff table; with prices, the gross edge at top of book (L5).
pub fn payoff_json(input: &str) -> String {
    run(input, |i: PayoffIn| {
        if i.kind != "implication" && !(2..=6).contains(&i.s) {
            return Err("s must be 2 to 6".to_string());
        }
        let m = match i.kind.as_str() {
            "yes" => payoff::yes_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "no" => payoff::no_basket(i.s, i.other_listed).map_err(|e| e.to_string())?,
            "implication" => payoff::implication(),
            k => return Err(format!("unknown kind {k:?}")),
        };
        let top_edge_gross = match &i.prices {
            None => None,
            Some(ps) => {
                let want = if i.kind == "implication" { 2 } else { i.s };
                if ps.len() != want {
                    return Err(format!("expected {want} prices, got {}", ps.len()));
                }
                // One share per leg at the top of book, zero fees: the detector's own rule.
                let mut legs = Vec::with_capacity(ps.len());
                for (n, pr) in ps.iter().enumerate() {
                    let (bid, ask) = (price(&pr.bid)?, price(&pr.ask)?);
                    let buy_no = i.kind == "no" || (i.kind == "implication" && n == 0);
                    let p = if buy_no { bid.complement() } else { ask };
                    legs.push(BuyLeg {
                        levels: one_share(p)?,
                        fee: FeeSchedule::ZERO,
                    });
                }
                let rule = if i.kind == "no" {
                    BasketRule::NestedNo
                } else {
                    BasketRule::AllLegs {
                        payoff: Decimal::ONE,
                    }
                };
                Some(d(walk(&legs, rule, None).top_edge_gross))
            }
        };
        let opt = |x: &Option<Decimal>| x.map(d);
        Ok(PayoffOut {
            rows: m.rows,
            cols: m.cols,
            possible: m.possible,
            cells: m
                .cells
                .iter()
                .map(|r| r.iter().map(opt).collect())
                .collect(),
            basket: m.basket.iter().map(opt).collect(),
            guaranteed: d(m.guaranteed),
            broken: m.broken,
            top_edge_gross,
        })
    })
}
