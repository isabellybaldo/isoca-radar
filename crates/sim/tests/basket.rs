//! 7b §3.2–3.3: the basket fill arithmetic extracted from `PaperTrader::execute`/`unwind`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use common::{book, d, p};
use proptest::prelude::*;
use rust_decimal::Decimal;
use sim::consumed::ConsumedLiquidity;
use sim::types::{LegKey, Outcome};
use sim::venue::{
    BasketBuy, ConvertTerms, FillLeg, HeldUnsold, OrderFill, basket_buy, basket_cash, unwind_sell,
};
use std::collections::HashMap;

fn key(i: usize, outcome: Outcome) -> LegKey {
    LegKey {
        market_id: format!("m{i}"),
        yes_token: format!("y{i}"),
        outcome,
    }
}

fn yes_leg(i: usize, limit: &str) -> FillLeg {
    FillLeg {
        key: key(i, Outcome::Yes),
        fee: FeeSchedule::ZERO,
        limit: p(limit),
    }
}

fn books(v: Vec<Book>) -> HashMap<String, Book> {
    v.into_iter()
        .enumerate()
        .map(|(i, b)| (format!("y{i}"), b))
        .collect()
}

fn filled(b: &sim::venue::BasketBuy) -> Vec<Decimal> {
    b.per_leg.iter().map(|f| f.filled).collect()
}

#[test]
fn all_legs_fill_and_nothing_is_left_over() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.60", "20")]),
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("10")]);
    assert_eq!((b.q_eff, b.excess.clone()), (d("10"), vec![d("0"), d("0")]));
    assert_eq!(b.per_leg[0].notional, d("3"));
}

#[test]
fn one_empty_leg_makes_every_other_legs_whole_fill_excess() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.61", "20")]), // above the 0.60 limit: nothing
        book(&[], &[("0.05", "20")]),
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60"), yes_leg(2, "0.05")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(b.q_eff, d("0"));
    assert_eq!(b.excess, vec![d("10"), d("0"), d("10")]);
}

#[test]
fn partial_fills_leave_the_difference_as_excess() {
    let bk = books(vec![
        book(&[], &[("0.30", "20")]),
        book(&[], &[("0.60", "6"), ("0.61", "50")]), // the limit stops at 6
    ]);
    let legs = [yes_leg(0, "0.30"), yes_leg(1, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("6")]);
    assert_eq!((b.q_eff, b.excess), (d("6"), vec![d("4"), d("0")]));
}

#[test]
fn a_missing_book_is_an_empty_fill_and_no_legs_is_zero() {
    let bk = books(vec![book(&[], &[("0.30", "20")])]);
    let legs = [yes_leg(0, "0.30"), yes_leg(7, "0.60")];
    let b = basket_buy(&bk, &legs, d("10"), &mut ConsumedLiquidity::default());
    assert_eq!(filled(&b), vec![d("10"), d("0")]);
    assert_eq!(b.per_leg[1], sim::venue::OrderFill::default());
    assert_eq!(b.q_eff, d("0"));
    let none = basket_buy(&bk, &[], d("10"), &mut ConsumedLiquidity::default());
    assert_eq!((none.q_eff, none.per_leg.len()), (d("0"), 0));
}

#[test]
fn a_no_basket_buys_the_mirrored_yes_bids_and_consumes_them() {
    let bk = books(vec![
        book(&[("0.60", "8")], &[]),
        book(&[("0.55", "20")], &[]),
    ]);
    let legs: Vec<FillLeg> = (0..2)
        .map(|i| FillLeg {
            key: key(i, Outcome::No),
            fee: FeeSchedule::ZERO,
            limit: p("0.45"),
        })
        .collect();
    let mut c = ConsumedLiquidity::default();
    let b = basket_buy(&bk, &legs, d("10"), &mut c);
    assert_eq!(filled(&b), vec![d("8"), d("10")]);
    assert_eq!((b.q_eff, b.excess.clone()), (d("8"), vec![d("0"), d("2")]));
    let again = basket_buy(&bk, &legs, d("10"), &mut c);
    assert_eq!(
        filled(&again),
        vec![d("0"), d("10")],
        "leg 0's bid is consumed"
    );
}

fn side() -> impl Strategy<Value = Vec<(Price, Shares)>> {
    prop::collection::btree_map(1u32..=99, 1u32..=400, 0..=4).prop_map(|m| {
        m.into_iter()
            .map(|(c, s)| {
                (
                    Price::new(Decimal::new(c.into(), 2)).unwrap(),
                    Shares::new(Decimal::new(s.into(), 1)).unwrap(),
                )
            })
            .collect()
    })
}

fn yes_book() -> impl Strategy<Value = Book> {
    (side(), side()).prop_map(|(bids, asks)| {
        let mut b = Book::default();
        b.apply_snapshot(&bids, &asks, 0);
        b
    })
}

proptest! {
    #[test]
    fn q_eff_is_the_smallest_fill_and_excess_is_never_negative(
        bks in prop::collection::vec(yes_book(), 2..=6),
        no in any::<bool>(),
        limit in 1u32..=99,
        q in 1u32..=500,
    ) {
        let outcome = if no { Outcome::No } else { Outcome::Yes };
        let legs: Vec<FillLeg> = (0..bks.len())
            .map(|i| FillLeg {
                key: key(i, outcome),
                fee: FeeSchedule::new(d("0.07"), 1).unwrap(),
                limit: Price::new(Decimal::new(limit.into(), 2)).unwrap(),
            })
            .collect();
        let b = basket_buy(&books(bks), &legs, Decimal::from(q), &mut ConsumedLiquidity::default());
        prop_assert_eq!(b.per_leg.len(), legs.len());
        for (f, x) in b.per_leg.iter().zip(&b.excess) {
            prop_assert!(b.q_eff <= f.filled);
            prop_assert!(*x >= Decimal::ZERO);
            prop_assert_eq!(*x, f.filled - b.q_eff);
            prop_assert!(f.filled <= Decimal::from(q));
        }
        prop_assert!(b.excess.iter().any(|x| x.is_zero()), "the smallest leg has no excess");
    }
}

fn no_key() -> LegKey {
    key(0, Outcome::No)
}

#[test]
fn an_unwind_below_the_minimum_sends_nothing_but_keeps_the_request() {
    let b = book(&[], &[("0.40", "100")]);
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("4.99"),
        d("30"),
        d("5"),
        &mut c,
    );
    assert_eq!((req, f), (d("4.99"), OrderFill::default()));
    assert!(c.is_empty(), "no order, nothing consumed");
}

#[test]
fn an_unwind_with_no_book_sends_nothing() {
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        None,
        &no_key(),
        FeeSchedule::ZERO,
        d("12"),
        d("12"),
        d("5"),
        &mut c,
    );
    assert_eq!((req, f), (d("12"), OrderFill::default()));
}

#[test]
fn held_and_qty_are_both_rounded_to_cents_toward_zero() {
    // NO unwind sells into the mirrored YES asks: NO bid = 1 - 0.40 = 0.60
    let b = book(&[], &[("0.40", "100")]);
    let mut c = ConsumedLiquidity::default();
    let (req, f) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("9.999"),
        d("7.4567"),
        d("5"),
        &mut c,
    );
    assert_eq!(
        req,
        d("7.45"),
        "min(round2(9.999) = 9.99, round2(7.4567) = 7.45)"
    );
    assert_eq!((f.filled, f.notional), (d("7.45"), d("4.47")));
    let (req, _) = unwind_sell(
        Some(&b),
        &no_key(),
        FeeSchedule::ZERO,
        d("6.019"),
        d("50"),
        d("5"),
        &mut c,
    );
    assert_eq!(req, d("6.01"));
}

proptest! {
    #[test]
    fn an_unwind_never_sells_more_than_it_may(
        bk in yes_book(),
        no in any::<bool>(),
        qty in 0u32..=100_000,
        held in 0u32..=100_000,
        min in 1u32..=10,
    ) {
        let leg = key(0, if no { Outcome::No } else { Outcome::Yes });
        let (qty, held) = (Decimal::new(qty.into(), 3), Decimal::new(held.into(), 3));
        let cap = qty.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToZero)
            .min(held.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToZero));
        let (req, f) = unwind_sell(Some(&bk), &leg, FeeSchedule::ZERO, qty, held, Decimal::from(min), &mut ConsumedLiquidity::default());
        prop_assert_eq!(req, cap);
        prop_assert!(f.filled <= cap);
        if cap < Decimal::from(min) {
            prop_assert_eq!(f, OrderFill::default());
        }
    }
}

fn of(filled: &str, notional: &str, fees: &str) -> OrderFill {
    OrderFill {
        filled: d(filled),
        notional: d(notional),
        fees: d(fees),
        levels_used: 1,
    }
}

#[test]
fn a_caught_no_basket_with_one_short_leg_loses_cash_and_holds_the_rest() {
    // A fills 10 @ 0.50, B 6 @ 0.50: 6 sets convert (s = 2 pays 1 each), A's 4 extra stay held
    let buy = BasketBuy {
        per_leg: vec![of("10", "5", "0"), of("6", "3", "0")],
        q_eff: d("6"),
        excess: vec![d("4"), d("0")],
    };
    let c = basket_cash(
        &buy,
        &[OrderFill::default(), OrderFill::default()],
        Some(ConvertTerms {
            s: 2,
            q: d("6"),
            fee_bips: d("50"),
        }),
    );
    assert_eq!((c.spent, c.unwind_back), (d("8"), d("0")));
    assert_eq!(
        (c.convert_back, c.fees),
        (d("5.97"), d("0.03")),
        "6 − 6 × 50 bips"
    );
    assert_eq!(c.net_cash, d("-2.03"));
    assert_eq!(
        c.held_unsold,
        vec![
            HeldUnsold {
                qty: d("4"),
                basis: d("2")
            },
            HeldUnsold {
                qty: d("0"),
                basis: d("0")
            }
        ]
    );
}

#[test]
fn a_yes_basket_keeps_its_full_sets_and_counts_only_the_unsold_excess() {
    let buy = BasketBuy {
        per_leg: vec![of("10", "3", "0.1"), of("6", "3.6", "0.1")],
        q_eff: d("6"),
        excess: vec![d("4"), d("0")],
    };
    // 3 of A's 4 extra sold at 0.25 with a 0.01 fee
    let c = basket_cash(&buy, &[of("3", "0.75", "0.01"), OrderFill::default()], None);
    assert_eq!(
        (c.spent, c.unwind_back, c.convert_back),
        (d("6.8"), d("0.74"), d("0"))
    );
    assert_eq!((c.fees, c.net_cash), (d("0.21"), d("-6.06")));
    assert_eq!(
        c.held_unsold[0].qty,
        d("1"),
        "10 − 3 sold − 6 kept to resolution"
    );
    assert_eq!(c.held_unsold[1].qty, d("0"));
}

fn spec(fee_bips: Decimal) -> sim::config::AccountSpec {
    sim::config::AccountSpec {
        policy: sim::config::PolicyKind::P0Converter,
        scenario: "t".into(),
        bankroll: d("1000000"),
        per_trade: d("100"),
        fee_bips,
    }
}

proptest! {
    #[test]
    fn basket_cash_is_what_the_ledger_books(
        tx in prop::collection::vec(yes_book(), 2..=6),
        unwind in prop::collection::vec(yes_book(), 6),
        no in any::<bool>(),
        limit in 1u32..=99,
        q in 5u32..=500,
        bips in prop_oneof![Just(0u32), Just(50u32), 0u32..=10_000],
    ) {
        let outcome = if no { Outcome::No } else { Outcome::Yes };
        let fee = FeeSchedule::new(d("0.07"), 1).unwrap();
        let legs: Vec<FillLeg> = (0..tx.len())
            .map(|i| FillLeg { key: key(i, outcome), fee, limit: Price::new(Decimal::new(limit.into(), 2)).unwrap() })
            .collect();
        let mut c = ConsumedLiquidity::default();
        let buy = basket_buy(&books(tx), &legs, Decimal::from(q), &mut c);
        let unwinds: Vec<OrderFill> = legs
            .iter()
            .zip(buy.per_leg.iter().zip(&buy.excess))
            .enumerate()
            .map(|(i, (l, (f, x)))| {
                if *x > Decimal::ZERO {
                    unwind_sell(Some(&unwind[i]), &l.key, fee, *x, f.filled, d("5"), &mut c).1
                } else {
                    OrderFill::default()
                }
            })
            .collect();
        let fee_bips = Decimal::from(bips);
        let convert = no.then_some(ConvertTerms { s: legs.len(), q: buy.q_eff, fee_bips });
        let cash = basket_cash(&buy, &unwinds, convert);

        let mut acct = sim::account::Account::new(spec(fee_bips));
        let mut rows = Vec::new();
        for (l, f) in legs.iter().zip(&buy.per_leg) {
            rows.extend(acct.buy(0, "e", &l.key, f));
        }
        for (l, u) in legs.iter().zip(&unwinds) {
            rows.extend(acct.sell(1, "e", &l.key, u));
        }
        if no && buy.q_eff > Decimal::ZERO {
            let s_legs: Vec<LegKey> = legs.iter().map(|l| l.key.clone()).collect();
            rows.extend(acct.convert(2, "e", &s_legs, &[], buy.q_eff));
        }
        let cash_delta: Decimal = rows.iter().map(|r| r.cash_delta).sum();
        let realized: Decimal = rows.iter().map(|r| r.realized_delta).sum();
        let open_basis = acct.valuation(|_| None).basis_open;
        prop_assert_eq!(cash.net_cash, cash_delta);
        prop_assert_eq!(realized, cash.net_cash + open_basis, "realized = net cash + the basis still held");
        let held_basis: Decimal = cash.held_unsold.iter().map(|h| h.basis).sum();
        if no {
            prop_assert_eq!(held_basis, open_basis, "after the conversion only unsold excess is held");
        } else {
            prop_assert!(held_basis <= open_basis);
        }
        for ((h, x), u) in cash.held_unsold.iter().zip(&buy.excess).zip(&unwinds) {
            prop_assert_eq!(h.qty, *x - u.filled);
        }
    }
}
