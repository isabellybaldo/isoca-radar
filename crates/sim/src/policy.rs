//! Policies P0–P3 (spec §11) and sizing (spec §5.6): one shot per episode,
//! q_target = min(Q*(open), per_trade / unit_cost, free_cash / unit_cost), rounded to venue rules.
use crate::config::{AccountSpec, PolicyKind, SimConfig};
use crate::types::{LegKey, Outcome};
use crate::venue::order_size;
use arb_core::detect::{Kind, Signal};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct LegInfo {
    pub market_id: String,
    pub yes_token: String,
    pub fee: FeeSchedule,
    pub seconds_delay: Option<i32>,
}

/// Everything a policy may look at when an episode opens.
#[derive(Debug, Clone)]
pub struct OpenCtx {
    pub episode_id: String,
    pub event_id: String,
    /// OUR receive time of the opening frame (spec §5.6 clock).
    pub r_open_ms: i64,
    pub signal: Signal,
    /// Every leg of the group (Q) in the group's order; `signal.legs_used` indexes into it.
    pub legs: Vec<LegInfo>,
    /// Per used leg, same order as `signal.legs_used`: the bought outcome's levels at open.
    pub open_levels: Vec<Vec<(Price, Shares)>>,
    pub days_to_end: Option<Decimal>,
    pub annualized: Option<Decimal>,
    /// D3 only: the pair was approved by a human (S5). Always false until then.
    pub pair_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentLeg {
    pub key: LegKey,
    pub limit: Price,
    pub fee: FeeSchedule,
    pub seconds_delay: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// NO basket: convert to collateral after the settlement delay; these YES come back.
    Convert { complements: Vec<LegKey> },
    /// YES basket / D3: hold until resolution.
    ToResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub legs: Vec<IntentLeg>,
    pub q_target: Decimal,
    /// Worst-case cost at the limits (fees included): reserved from free cash until execution.
    pub reserve: Decimal,
    pub hold: Hold,
    /// The edge the policy acted on (net, or gross for P3) and the full-set size at open.
    pub detected_pnl: Decimal,
    pub detected_q: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NotNetPositive,
    NoLockupFigures,
    TooLong,
    AnnualizedTooLow,
    PairNotApproved,
    NoCash,
    BelowMinSize,
    BadInput,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::NotNetPositive => "not_net_positive",
            SkipReason::NoLockupFigures => "no_lockup_figures",
            SkipReason::TooLong => "too_long",
            SkipReason::AnnualizedTooLow => "annualized_too_low",
            SkipReason::PairNotApproved => "pair_not_approved",
            SkipReason::NoCash => "no_cash",
            SkipReason::BelowMinSize => "below_min_size",
            SkipReason::BadInput => "bad_input",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    NotMine,
    Skip(SkipReason),
    Take(Planned),
}

pub fn takes_kind(p: PolicyKind, k: Kind) -> bool {
    match p {
        PolicyKind::P0Converter => k == Kind::NoBasket,
        PolicyKind::P1Settlement => k == Kind::YesBasket,
        PolicyKind::P2Combo => k == Kind::Implication,
        PolicyKind::P3Naive => true,
    }
}

/// The outcome bought on the `pos`-th used leg of a signal of kind `k`.
pub fn outcome_for(k: Kind, pos: usize) -> Outcome {
    match k {
        Kind::NoBasket => Outcome::No,
        Kind::Implication if pos == 0 => Outcome::No,
        _ => Outcome::Yes,
    }
}

/// Cost of buying `q` along `levels` (+ conservative fees unless `gross`) and the worst price
/// touched. None if the levels hold less than `q`.
pub fn walk_cost(
    levels: &[(Price, Shares)],
    q: Decimal,
    fee: FeeSchedule,
    gross: bool,
) -> Option<(Decimal, Price)> {
    let mut left = q;
    let mut cost = Decimal::ZERO;
    let mut worst = None;
    for &(p, s) in levels {
        if left <= Decimal::ZERO {
            break;
        }
        let take = s.get().min(left);
        if take <= Decimal::ZERO {
            continue;
        }
        cost += p.get() * take;
        if !gross {
            cost += fee.fee_conservative(p, take);
        }
        worst = Some(p);
        left -= take;
    }
    if left > Decimal::ZERO {
        return None;
    }
    worst.map(|w| (cost, w))
}

pub fn decide(cfg: &SimConfig, spec: &AccountSpec, free_cash: Decimal, ctx: &OpenCtx) -> Decision {
    let kind = ctx.signal.kind;
    if !takes_kind(spec.policy, kind) {
        return Decision::NotMine;
    }
    let naive = spec.policy == PolicyKind::P3Naive;
    let quote = if naive {
        &ctx.signal.gross
    } else {
        &ctx.signal.quote
    };
    let edge = if naive { quote.pnl_gross } else { quote.pnl };
    if edge <= Decimal::ZERO {
        return Decision::Skip(SkipReason::NotNetPositive);
    }
    match spec.policy {
        PolicyKind::P1Settlement => {
            let (Some(days), Some(ann)) = (ctx.days_to_end, ctx.annualized) else {
                return Decision::Skip(SkipReason::NoLockupFigures);
            };
            if days < Decimal::ZERO || days > cfg.p1.max_days_to_end {
                return Decision::Skip(SkipReason::TooLong);
            }
            if ann < cfg.p1.min_annualized {
                return Decision::Skip(SkipReason::AnnualizedTooLow);
            }
        }
        PolicyKind::P2Combo if !ctx.pair_approved => {
            return Decision::Skip(SkipReason::PairNotApproved);
        }
        _ => {}
    }
    let used = &ctx.signal.legs_used;
    let mut infos = Vec::with_capacity(used.len());
    for &gi in used {
        match ctx.legs.get(gi) {
            Some(l) => infos.push(l),
            None => return Decision::Skip(SkipReason::BadInput),
        }
    }
    if ctx.open_levels.len() != used.len() {
        return Decision::Skip(SkipReason::BadInput);
    }
    // One complete basket: every used leg. A NO walk drops legs as they exhaust (L11); v1 trades
    // only the full set, whose size is the smallest leg fill (conservative, see plan rulings).
    let q_open = quote
        .legs
        .iter()
        .map(|l| l.filled)
        .min()
        .unwrap_or(Decimal::ZERO);
    if q_open <= Decimal::ZERO {
        return Decision::Skip(SkipReason::BelowMinSize);
    }
    let at = |q: Decimal| -> Option<(Decimal, Vec<Price>)> {
        let mut total = Decimal::ZERO;
        let mut limits = Vec::with_capacity(infos.len());
        for (levels, info) in ctx.open_levels.iter().zip(&infos) {
            let (c, w) = walk_cost(levels, q, info.fee, naive)?;
            total += c;
            limits.push(w);
        }
        Some((total, limits))
    };
    let Some((_cost_open, _)) = at(q_open) else {
        return Decision::Skip(SkipReason::BadInput);
    };
    let budget = spec.per_trade.min(free_cash);
    // Bisect to find largest q <= q_open such that walked cost <= budget.
    // Use 60 iterations for Decimal precision.
    let mut q_max = q_open;
    let mut q_min = Decimal::ZERO;
    for _ in 0..60 {
        let q_mid = (q_min + q_max) / Decimal::from(2);
        let Some((cost, _)) = at(q_mid) else {
            q_max = q_mid;
            continue;
        };
        if cost <= budget {
            q_min = q_mid;
        } else {
            q_max = q_mid;
        }
    }
    let q_want = q_min;
    // Check NoCash: can we afford even min_order_shares?
    let Some((cost_min, _)) = at(cfg.min_order_shares) else {
        return Decision::Skip(SkipReason::NoCash);
    };
    if cost_min > free_cash {
        return Decision::Skip(SkipReason::NoCash);
    }
    let Some((_, mut limits)) = at(q_want) else {
        return Decision::Skip(SkipReason::BadInput);
    };
    let mut q = order_size(&limits, q_want, cfg.min_order_shares);
    if q <= Decimal::ZERO {
        return Decision::Skip(SkipReason::BelowMinSize);
    }
    let mut legs: Vec<IntentLeg> = infos
        .iter()
        .zip(&limits)
        .enumerate()
        .map(|(pos, (info, &limit))| IntentLeg {
            key: LegKey {
                market_id: info.market_id.clone(),
                yes_token: info.yes_token.clone(),
                outcome: outcome_for(kind, pos),
            },
            limit,
            fee: info.fee,
            seconds_delay: info.seconds_delay,
        })
        .collect();
    let mut reserve = legs
        .iter()
        .map(|l| (l.limit.get() + l.fee.phi(l.limit)) * q)
        .sum::<Decimal>()
        + Decimal::new(1, 3);
    // If reserve exceeds free cash, solve directly for q.
    // Compute C = Σ(limit + φ(limit)), the per-unit worst case.
    let mut c: Decimal = legs
        .iter()
        .map(|l| l.limit.get() + l.fee.phi(l.limit))
        .sum();
    if reserve > free_cash {
        // Solve: C·q + 0.001 = free_cash → q = (free_cash - 0.001) / C
        let q_target_exact = (free_cash - Decimal::new(1, 3)) / c;
        if q_target_exact <= Decimal::ZERO {
            return Decision::Skip(SkipReason::NoCash);
        }
        q = order_size(&limits, q_target_exact, cfg.min_order_shares);
        if q < cfg.min_order_shares {
            return Decision::Skip(SkipReason::NoCash);
        }
        // Refresh limits: the shrink may cross below a book level, requiring updated limits.
        // Loop at most 5 times until limits stabilize.
        for _ in 0..5 {
            let Some((_, new_limits)) = at(q) else {
                return Decision::Skip(SkipReason::BadInput);
            };
            let q2 = order_size(&new_limits, q, cfg.min_order_shares);
            if q2 < cfg.min_order_shares {
                return Decision::Skip(SkipReason::NoCash);
            }
            if q2 == q {
                limits = new_limits;
                break;
            }
            q = q2;
        }
        // Rebuild per-leg limits and C from final limits.
        c = limits
            .iter()
            .enumerate()
            .map(|(i, &limit)| {
                let fee = infos
                    .get(i)
                    .map(|info| info.fee)
                    .unwrap_or(FeeSchedule::ZERO);
                limit.get() + fee.phi(limit)
            })
            .sum();
        reserve = c * q + Decimal::new(1, 3);
        // Defensive check: if recomputed reserve is still > free_cash, bail out.
        if reserve > free_cash {
            return Decision::Skip(SkipReason::NoCash);
        }
        // Rebuild legs with updated limits.
        legs = infos
            .iter()
            .zip(&limits)
            .enumerate()
            .map(|(pos, (info, &limit))| IntentLeg {
                key: LegKey {
                    market_id: info.market_id.clone(),
                    yes_token: info.yes_token.clone(),
                    outcome: outcome_for(kind, pos),
                },
                limit,
                fee: info.fee,
                seconds_delay: info.seconds_delay,
            })
            .collect();
    }
    let hold = if kind == Kind::NoBasket {
        Hold::Convert {
            complements: ctx
                .legs
                .iter()
                .enumerate()
                .filter(|(i, _)| !used.contains(i))
                .map(|(_, l)| LegKey {
                    market_id: l.market_id.clone(),
                    yes_token: l.yes_token.clone(),
                    outcome: Outcome::Yes,
                })
                .collect(),
        }
    } else {
        Hold::ToResolution
    };
    Decision::Take(Planned {
        legs,
        q_target: q,
        reserve,
        hold,
        detected_pnl: edge,
        detected_q: q_open,
    })
}
