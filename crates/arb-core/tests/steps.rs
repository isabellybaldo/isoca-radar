//! Phase 7 spec §3.2 (D1 sums, L2) and §3.4 (walk_steps agrees with walk, L8).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::{Book, d1_sums};
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use arb_core::steps::walk_steps;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn px(s: &str) -> Price {
    Price::parse(s).unwrap()
}

fn sh(s: &str) -> Shares {
    Shares::parse(s).unwrap()
}

#[test]
fn d1_sums_on_the_fed_book() {
    let mut b = Book::default();
    b.apply_snapshot(
        &[(px("0.54"), sh("5603.74"))],
        &[(px("0.55"), sh("32356.03"))],
        0,
    );
    assert_eq!(d1_sums(&b.top()), Some((dec!(1.01), dec!(0.99))));
}

#[test]
fn d1_sums_needs_both_sides() {
    let mut b = Book::default();
    b.apply_snapshot(&[(px("0.54"), sh("1"))], &[], 0);
    assert_eq!(d1_sums(&b.top()), None);
}

#[test]
fn walk_steps_includes_the_unprofitable_tail() {
    let legs = vec![
        BuyLeg {
            levels: vec![(px("0.30"), sh("10")), (px("0.40"), sh("10"))],
            fee: FeeSchedule::ZERO,
        },
        BuyLeg {
            levels: vec![(px("0.60"), sh("15"))],
            fee: FeeSchedule::ZERO,
        },
    ];
    let rule = BasketRule::AllLegs {
        payoff: Decimal::ONE,
    };
    let steps = walk_steps(&legs, rule);
    assert_eq!(steps.len(), 2);
    let s = |i: usize| {
        (
            steps[i].q_from,
            steps[i].q_to,
            steps[i].cost_per_basket,
            steps[i].profitable,
        )
    };
    assert_eq!(s(0), (dec!(0), dec!(10), dec!(0.90), true));
    assert_eq!(s(1), (dec!(10), dec!(15), dec!(1.00), false));
    assert_eq!(walk(&legs, rule, None).q_max, dec!(10));
}

fn level() -> impl Strategy<Value = (Price, Shares)> {
    (1u32..=99, 1u32..=500).prop_map(|(c, s)| {
        (
            Price::new(Decimal::new(c.into(), 2)).unwrap(),
            Shares::new(Decimal::from(s)).unwrap(),
        )
    })
}

fn legs() -> impl Strategy<Value = Vec<BuyLeg>> {
    prop::collection::vec(
        (
            prop::collection::btree_map(1u32..=99, 1u32..=500, 1..=4),
            any::<bool>(),
        ),
        2..=5,
    )
    .prop_map(|v| {
        v.into_iter()
            .map(|(m, fee)| BuyLeg {
                levels: m
                    .into_iter()
                    .map(|(c, s)| {
                        (
                            Price::new(Decimal::new(c.into(), 2)).unwrap(),
                            Shares::new(Decimal::from(s)).unwrap(),
                        )
                    })
                    .collect(),
                fee: if fee {
                    FeeSchedule::new(dec!(0.07), 1).unwrap()
                } else {
                    FeeSchedule::ZERO
                },
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn the_profitable_prefix_is_walk(legs in legs(), nested: bool) {
        let rule = if nested {
            BasketRule::NestedNo
        } else {
            BasketRule::AllLegs { payoff: Decimal::ONE }
        };
        let steps = walk_steps(&legs, rule);
        let w = walk(&legs, rule, None);
        let prefix: Vec<_> = steps.iter().take_while(|s| s.profitable).collect();
        let q: Decimal = prefix.iter().map(|s| s.q_to - s.q_from).sum();
        let pnl: Decimal = prefix
            .iter()
            .map(|s| (s.payoff_per_basket - s.cost_per_basket) * (s.q_to - s.q_from))
            .sum();
        prop_assert_eq!(q, w.q_max);
        prop_assert_eq!(pnl, w.pnl_exact);
        for pair in steps.windows(2) {
            prop_assert_eq!(pair[0].q_to, pair[1].q_from);
        }
        if !nested {
            for pair in steps.windows(2) {
                prop_assert!(pair[0].cost_per_basket <= pair[1].cost_per_basket, "L8");
            }
        }
    }

    #[test]
    fn d1_crossed_iff_long_below_one(b in level(), a in level()) {
        let mut book = Book::default();
        book.apply_snapshot(&[b], &[a], 0);
        let (long, short) = d1_sums(&book.top()).unwrap();
        prop_assert_eq!(book.is_crossed(), long < Decimal::ONE);
        prop_assert_eq!(book.is_crossed(), short > Decimal::ONE);
    }
}
