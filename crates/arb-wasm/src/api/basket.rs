//! `basket_json`: section f, one leg misses (7b §3.4), on the paper sim's own rules.
use super::run;
use crate::input::{BookIn, FeeIn, book, d, decimal, fee, quantity};
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::Price;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sim::consumed::ConsumedLiquidity;
use sim::policy::{Settle, settle, walk_cost};
use sim::types::{LegKey, Outcome};
use sim::venue::{
    ConvertTerms, FillLeg, OrderFill, basket_buy, basket_cash, bought_levels, unwind_sell,
};
use std::collections::HashMap;

pub const MAX_BASKET_LEGS: usize = 4;
const MAX_FEE_BIPS: u32 = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketLegIn {
    open: BookIn,
    t_x: BookIn,
    unwind: BookIn,
    #[serde(default)]
    fee: Option<FeeIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketIn {
    kind: String,
    legs: Vec<BasketLegIn>,
    q_request: String,
    #[serde(default)]
    min_order: Option<String>,
    #[serde(default)]
    convert_fee_bips: Option<String>,
}

#[derive(Serialize)]
struct FillOut {
    requested: String,
    filled: String,
    vwap: Option<String>,
    fees: String,
    levels_used: u32,
}

fn fill_out(requested: Decimal, f: &OrderFill) -> FillOut {
    FillOut {
        requested: d(requested),
        filled: d(f.filled),
        vwap: f.vwap().map(d),
        fees: d(f.fees),
        levels_used: f.levels_used,
    }
}

#[derive(Serialize)]
struct BasketLegOut {
    limit: String,
    buy: FillOut,
    excess: String,
    unwind: FillOut,
}

#[derive(Serialize)]
struct HeldOut {
    qty: String,
    basis: String,
}

#[derive(Serialize)]
struct CashOut {
    spent: String,
    fees: String,
    unwind_back: String,
    convert_back: String,
    net_cash: String,
    held_unsold: Vec<HeldOut>,
}

#[derive(Serialize)]
struct BasketOut {
    kind: &'static str,
    min_order: String,
    q_open: String,
    q_target: String,
    legs: Vec<BasketLegOut>,
    q_eff: String,
    cash: CashOut,
    value_at_resolution: Option<String>,
}

struct BasketLeg {
    key: LegKey,
    fee: FeeSchedule,
    open: Book,
    t_x: Book,
    unwind: Book,
}

/// Section f (7b §3.4): plan at open as the policy does (`sim::policy::settle`), buy at t_x
/// (`sim::venue::basket_buy`), unwind each leg's excess at once (`unwind_sell`), and book the cash
/// (`basket_cash`). Every book is a YES book: a NO buy walks the mirrored YES bids, a NO unwind
/// sells into the mirrored YES asks.
pub fn basket_json(input: &str) -> String {
    run(input, |i: BasketIn| {
        let (outcome, kind_out) = match i.kind.as_str() {
            "yes" => (Outcome::Yes, "yes"),
            "no" => (Outcome::No, "no"),
            k => return Err(format!("unknown kind {k:?}")),
        };
        if !(2..=MAX_BASKET_LEGS).contains(&i.legs.len()) {
            return Err(format!("a basket has 2 to {MAX_BASKET_LEGS} legs"));
        }
        let q_request = quantity(&i.q_request, "q_request")?;
        let min_order = match &i.min_order {
            None => Decimal::from(5),
            Some(s) => quantity(s, "min_order")?,
        };
        let fee_bips = match &i.convert_fee_bips {
            None => Decimal::ZERO,
            Some(s) => decimal(s, "convert_fee_bips")?,
        };
        if fee_bips < Decimal::ZERO || fee_bips > Decimal::from(MAX_FEE_BIPS) {
            return Err("convert_fee_bips must be 0 to 10,000".to_string());
        }
        if outcome == Outcome::Yes && !fee_bips.is_zero() {
            return Err("convert_fee_bips applies to a NO basket only".to_string());
        }
        let legs = i
            .legs
            .iter()
            .enumerate()
            .map(|(n, l)| {
                Ok(BasketLeg {
                    key: LegKey {
                        market_id: format!("m{n}"),
                        yes_token: format!("y{n}"),
                        outcome,
                    },
                    fee: fee(&l.fee)?,
                    open: book(&l.open.bids, &l.open.asks)?,
                    t_x: book(&l.t_x.bids, &l.t_x.asks)?,
                    unwind: book(&l.unwind.bids, &l.unwind.asks)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let too_thin = || format!("the book at open is too thin for {} shares", d(min_order));

        // Plan at open exactly as `sim::policy::decide` does: the net walk's edge and full-set
        // size, then `settle` for the venue-rounded size and each leg's limit.
        let open_levels: Vec<_> = legs
            .iter()
            .map(|l| bought_levels(&l.open, outcome))
            .collect();
        let buy_legs: Vec<BuyLeg> = open_levels
            .iter()
            .zip(&legs)
            .map(|(levels, l)| BuyLeg {
                levels: levels.clone(),
                fee: l.fee,
            })
            .collect();
        let rule = match outcome {
            Outcome::No => BasketRule::NestedNo,
            Outcome::Yes => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
        };
        let quote = walk(&buy_legs, rule, None);
        if quote.pnl <= Decimal::ZERO {
            return Err(
                "the basket has no edge at open after fees, so the policy would not trade"
                    .to_string(),
            );
        }
        let q_open = quote
            .legs
            .iter()
            .map(|l| l.filled)
            .min()
            .unwrap_or(Decimal::ZERO);
        if q_open < min_order {
            return Err(too_thin());
        }
        if q_request < min_order {
            return Err(format!(
                "q_request must be at least the minimum order of {} shares",
                d(min_order)
            ));
        }
        let at = |q: Decimal| -> Option<(Decimal, Vec<Price>)> {
            let mut total = Decimal::ZERO;
            let mut limits = Vec::with_capacity(legs.len());
            for (levels, l) in open_levels.iter().zip(&legs) {
                let (c, w) = walk_cost(levels, q, l.fee, false)?;
                total += c;
                limits.push(w);
            }
            Some((total, limits))
        };
        let (q_target, limits) = match settle(&at, q_request.min(q_open), min_order) {
            Ok(v) => v,
            Err(Settle::TooSmall) => return Err(too_thin()),
            Err(Settle::BadInput) => {
                return Err("the book at open can't be quoted at that size".to_string());
            }
        };
        if q_target <= Decimal::ZERO {
            return Err(too_thin());
        }

        // At t_x: the sim's basket buy, then each leg's excess unwound at once into its unwind
        // book (the live sim waits the account's delay; one consumed ledger, as the sim keeps).
        let fill_legs: Vec<FillLeg> = legs
            .iter()
            .zip(&limits)
            .map(|(l, &limit)| FillLeg {
                key: l.key.clone(),
                fee: l.fee,
                limit,
            })
            .collect();
        let tx: HashMap<String, Book> = legs
            .iter()
            .map(|l| (l.key.yes_token.clone(), l.t_x.clone()))
            .collect();
        let mut consumed = ConsumedLiquidity::default();
        let buy = basket_buy(&tx, &fill_legs, q_target, &mut consumed);
        let mut unwinds = Vec::with_capacity(legs.len());
        let mut requested = Vec::with_capacity(legs.len());
        for ((l, f), &excess) in legs.iter().zip(&buy.per_leg).zip(&buy.excess) {
            if excess > Decimal::ZERO {
                let (r, u) = unwind_sell(
                    Some(&l.unwind),
                    &l.key,
                    l.fee,
                    excess,
                    f.filled,
                    min_order,
                    &mut consumed,
                );
                requested.push(r);
                unwinds.push(u);
            } else {
                requested.push(Decimal::ZERO);
                unwinds.push(OrderFill::default());
            }
        }
        let convert = (outcome == Outcome::No).then_some(ConvertTerms {
            s: legs.len(),
            q: buy.q_eff,
            fee_bips,
        });
        let cash = basket_cash(&buy, &unwinds, convert);
        let legs_out = limits
            .iter()
            .zip(&buy.per_leg)
            .zip(&buy.excess)
            .zip(unwinds.iter().zip(&requested))
            .map(|(((limit, f), &excess), (u, &r))| BasketLegOut {
                limit: d(limit.get()),
                buy: fill_out(q_target, f),
                excess: d(excess),
                unwind: fill_out(r, u),
            })
            .collect();
        Ok(BasketOut {
            kind: kind_out,
            min_order: d(min_order),
            q_open: d(q_open),
            q_target: d(q_target),
            legs: legs_out,
            q_eff: d(buy.q_eff),
            cash: CashOut {
                spent: d(cash.spent),
                fees: d(cash.fees),
                unwind_back: d(cash.unwind_back),
                convert_back: d(cash.convert_back),
                net_cash: d(cash.net_cash),
                held_unsold: cash
                    .held_unsold
                    .iter()
                    .map(|h| HeldOut {
                        qty: d(h.qty),
                        basis: d(h.basis),
                    })
                    .collect(),
            },
            value_at_resolution: (outcome == Outcome::Yes).then(|| d(buy.q_eff)),
        })
    })
}
