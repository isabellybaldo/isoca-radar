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
use sim::venue::{FillLeg, basket_buy};
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
