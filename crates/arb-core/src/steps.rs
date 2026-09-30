//! The full depth staircase for drawing (Phase 7 spec §3.4). It lives beside `fill`, not in it:
//! `fill.rs` is the `depth` premise's file, and an edit there restarts every card (premises.rs).
use crate::fill::{BasketRule, BuyLeg};
use rust_decimal::Decimal;

/// One segment of the merged level breakpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub q_from: Decimal,
    pub q_to: Decimal,
    /// Σ price + Σ φ(price) with exact fees: what one more basket costs on this segment.
    pub cost_per_basket: Decimal,
    pub payoff_per_basket: Decimal,
    /// payoff − cost > 0.
    pub profitable: bool,
}

/// Every segment `fill::walk` would visit, without its stop at the first unprofitable one, until
/// the rule runs out of legs. The profitable prefix is exactly `walk(legs, rule, None)` (tested).
pub fn walk_steps(legs: &[BuyLeg], rule: BasketRule) -> Vec<Step> {
    let n = legs.len();
    let mut idx = vec![0usize; n];
    let mut rem: Vec<Decimal> = legs
        .iter()
        .map(|l| l.levels.first().map_or(Decimal::ZERO, |x| x.1.get()))
        .collect();
    let mut q = Decimal::ZERO;
    let mut out = Vec::new();
    loop {
        let active: Vec<usize> = (0..n).filter(|&i| idx[i] < legs[i].levels.len()).collect();
        let payoff = match rule {
            BasketRule::AllLegs { payoff } => {
                if n == 0 || active.len() < n {
                    break;
                }
                payoff
            }
            BasketRule::NestedNo => {
                if active.len() < 2 {
                    break;
                }
                Decimal::from(active.len()) - Decimal::ONE
            }
        };
        let cost: Decimal = active
            .iter()
            .map(|&i| {
                let p = legs[i].levels[idx[i]].0;
                p.get() + legs[i].fee.phi(p)
            })
            .sum();
        let seg = active
            .iter()
            .map(|&i| rem[i])
            .min()
            .unwrap_or(Decimal::ZERO);
        if seg <= Decimal::ZERO {
            break;
        }
        out.push(Step {
            q_from: q,
            q_to: q + seg,
            cost_per_basket: cost,
            payoff_per_basket: payoff,
            profitable: payoff - cost > Decimal::ZERO,
        });
        q += seg;
        for &i in &active {
            rem[i] -= seg;
            if rem[i] == Decimal::ZERO {
                idx[i] += 1;
                rem[i] = legs[i]
                    .levels
                    .get(idx[i])
                    .map_or(Decimal::ZERO, |x| x.1.get());
            }
        }
    }
    out
}
