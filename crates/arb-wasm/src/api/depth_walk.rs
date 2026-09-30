//! `depth_walk_json`: the cost staircase past Q* and `walk`'s totals (09-27 §4.1).
use super::run;
use crate::input::{FeeIn, LevelIn, MAX_LEGS, d, fee, levels};
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::steps::walk_steps;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkLegIn {
    levels: Vec<LevelIn>,
    fee: Option<FeeIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkIn {
    rule: String,
    legs: Vec<WalkLegIn>,
}

#[derive(Serialize)]
struct StepOut {
    q_from: String,
    q_to: String,
    cost: String,
    payoff: String,
    profitable: bool,
}

#[derive(Serialize)]
struct WalkOut {
    steps: Vec<StepOut>,
    q_star: String,
    pi: String,
    pi_gross: String,
}

/// The cost staircase, past Q*, and `walk`'s totals.
pub fn depth_walk_json(input: &str) -> String {
    run(input, |i: WalkIn| {
        if i.legs.is_empty() || i.legs.len() > MAX_LEGS {
            return Err(format!("1 to at most {MAX_LEGS} legs"));
        }
        let rule = match i.rule.as_str() {
            "yes" => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
            "no" => BasketRule::NestedNo,
            r => return Err(format!("unknown rule {r:?}")),
        };
        let legs = i
            .legs
            .iter()
            .map(|l| {
                Ok(BuyLeg {
                    levels: levels(&l.levels)?,
                    fee: fee(&l.fee)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let w = walk(&legs, rule, None);
        Ok(WalkOut {
            steps: walk_steps(&legs, rule)
                .into_iter()
                .map(|s| StepOut {
                    q_from: d(s.q_from),
                    q_to: d(s.q_to),
                    cost: d(s.cost_per_basket),
                    payoff: d(s.payoff_per_basket),
                    profitable: s.profitable,
                })
                .collect(),
            q_star: d(w.q_max),
            pi: d(w.pnl),
            pi_gross: d(w.pnl_gross),
        })
    })
}
