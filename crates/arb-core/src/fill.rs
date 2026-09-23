//! Depth-aware basket walking (spec §5.4; lemmas L8, L9, L11).
//! Every basket leg is a BUY; NO legs are bought on the mirrored NO asks (L1).
use crate::fee::FeeSchedule;
use crate::money::{Price, Shares};
use rust_decimal::{Decimal, RoundingStrategy};

/// A leg we buy; `levels` best-first (ascending price).
#[derive(Debug, Clone)]
pub struct BuyLeg {
    pub levels: Vec<(Price, Shares)>,
    pub fee: FeeSchedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasketRule {
    /// Every leg in every basket (D2a, D3); stop when any leg runs out.
    AllLegs { payoff: Decimal },
    /// NO baskets (D2b): each basket holds every leg that still has depth and pays (legs − 1). L4, L11.
    NestedNo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegFill {
    pub filled: Decimal,
    pub notional: Decimal,
    pub levels_used: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasketQuote {
    pub q_max: Decimal,
    pub cost: Decimal,
    pub payout: Decimal,
    /// Conservative fees, rounded up to 5 dp per level fill (what Π books).
    pub fees: Decimal,
    /// Unrounded fees (for math checks).
    pub fees_exact: Decimal,
    pub pnl: Decimal,
    pub pnl_exact: Decimal,
    pub pnl_gross: Decimal,
    /// First-segment marginal edge with exact fees (may be ≤ 0).
    pub top_edge: Decimal,
    pub top_edge_gross: Decimal,
    pub legs: Vec<LegFill>,
}

/// Walk the merged level breakpoints of all legs while the marginal edge is > 0 (L9).
/// `budget` caps total cost (for the $100 figure); the capped take is rounded down to 6 dp.
pub fn walk(legs: &[BuyLeg], rule: BasketRule, budget: Option<Decimal>) -> BasketQuote {
    let n = legs.len();
    let mut q = BasketQuote {
        legs: vec![LegFill::default(); n],
        ..Default::default()
    };
    let mut idx = vec![0usize; n];
    let mut rem: Vec<Decimal> = legs
        .iter()
        .map(|l| l.levels.first().map_or(Decimal::ZERO, |x| x.1.get()))
        .collect();
    let mut first = true;

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
                Decimal::from(active.len() as u64) - Decimal::ONE
            }
        };
        let prices: Vec<Price> = active.iter().map(|&i| legs[i].levels[idx[i]].0).collect();
        let sum_px: Decimal = prices.iter().map(|p| p.get()).sum();
        let sum_phi: Decimal = active
            .iter()
            .zip(&prices)
            .map(|(&i, &p)| legs[i].fee.phi(p))
            .sum();
        let m_gross = payoff - sum_px;
        let m = m_gross - sum_phi;
        if first {
            q.top_edge = m;
            q.top_edge_gross = m_gross;
            first = false;
        }
        if m <= Decimal::ZERO {
            break;
        }

        let seg = active
            .iter()
            .map(|&i| rem[i])
            .min()
            .unwrap_or(Decimal::ZERO);
        let mut take = seg;
        let mut capped = false;
        if let Some(cap) = budget
            && sum_px > Decimal::ZERO
        {
            let room =
                ((cap - q.cost) / sum_px).round_dp_with_strategy(6, RoundingStrategy::ToZero);
            if room <= Decimal::ZERO {
                break;
            }
            if room < seg {
                take = room;
                capped = true;
            }
        }

        let fees_seg: Decimal = active
            .iter()
            .zip(&prices)
            .map(|(&i, &p)| legs[i].fee.fee_conservative(p, take))
            .sum();
        q.fees += fees_seg;
        q.fees_exact += sum_phi * take;
        q.cost += sum_px * take;
        q.payout += payoff * take;
        q.q_max += take;

        for (&i, &p) in active.iter().zip(&prices) {
            let lf = &mut q.legs[i];
            lf.filled += take;
            lf.notional += p.get() * take;
            rem[i] -= take;
            if rem[i] == Decimal::ZERO {
                idx[i] += 1;
                lf.levels_used = idx[i];
                rem[i] = legs[i]
                    .levels
                    .get(idx[i])
                    .map_or(Decimal::ZERO, |x| x.1.get());
            } else {
                lf.levels_used = idx[i] + 1;
            }
        }
        if capped {
            break;
        }
    }

    q.pnl_gross = q.payout - q.cost;
    q.pnl = q.pnl_gross - q.fees;
    q.pnl_exact = q.pnl_gross - q.fees_exact;
    q
}
