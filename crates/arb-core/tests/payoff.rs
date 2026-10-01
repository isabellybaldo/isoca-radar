//! Phase 7 spec §3.1: the §5.7 (b) payoff tables as code (L3, L4, L10).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::payoff::{PayoffError, implication, no_basket, yes_basket};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn yes_basket_with_other_uncovered_matches_the_spec_table() {
    let m = yes_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["YES_A", "YES_B", "YES_C"]);
    assert_eq!(m.cols, vec!["A", "B", "C", "Other"]);
    assert_eq!(
        m.cells[0],
        vec![Some(dec!(1)), Some(dec!(0)), Some(dec!(0)), Some(dec!(0))]
    );
    assert_eq!(
        m.basket,
        vec![Some(dec!(1)), Some(dec!(1)), Some(dec!(1)), Some(dec!(0))]
    );
    assert_eq!(m.guaranteed, dec!(1));
    assert_eq!(m.broken, vec![false, false, false, true]);
}

#[test]
fn no_basket_pays_s_minus_one_and_s_in_other() {
    let m = no_basket(3, false).unwrap();
    assert_eq!(m.rows, vec!["NO_A", "NO_B", "NO_C"]);
    assert_eq!(
        m.basket,
        vec![Some(dec!(2)), Some(dec!(2)), Some(dec!(2)), Some(dec!(3))]
    );
    assert_eq!(m.guaranteed, dec!(2));
    assert!(m.broken.iter().all(|b| !b));
}

#[test]
fn implication_has_four_columns_one_impossible() {
    let m = implication();
    assert_eq!(m.cols, vec!["A∧B", "A∧¬B", "¬A∧B", "¬A∧¬B"]);
    assert_eq!(m.possible, vec![true, false, true, true]);
    assert_eq!(m.rows, vec!["NO_A", "YES_B"]);
    assert_eq!(
        m.cells[0],
        vec![Some(dec!(0)), None, Some(dec!(1)), Some(dec!(1))]
    );
    assert_eq!(
        m.cells[1],
        vec![Some(dec!(1)), None, Some(dec!(1)), Some(dec!(0))]
    );
    assert_eq!(
        m.basket,
        vec![Some(dec!(1)), None, Some(dec!(2)), Some(dec!(1))]
    );
    assert_eq!(m.broken, vec![false, false, false, false]);
}

#[test]
fn sizes_outside_2_to_12_are_errors_not_clamped() {
    assert_eq!(yes_basket(1, true).unwrap_err(), PayoffError::Size(1));
    assert_eq!(no_basket(13, true).unwrap_err(), PayoffError::Size(13));
    assert_eq!(yes_basket(0, false).unwrap_err(), PayoffError::Size(0));
}

proptest! {
    #[test]
    fn l3_yes_basket_broken_iff_other_uncovered(s in 2usize..=12, listed: bool) {
        let m = yes_basket(s, listed).unwrap();
        prop_assert_eq!(m.broken.iter().any(|b| *b), !listed);
    }

    #[test]
    fn l4_no_basket_never_broken(s in 2usize..=12, listed: bool) {
        let m = no_basket(s, listed).unwrap();
        prop_assert!(m.broken.iter().all(|b| !b));
        let floor = Decimal::from(s) - Decimal::ONE;
        for b in m.basket.iter().flatten() {
            prop_assert!(*b >= floor);
        }
    }
}

#[test]
fn l10_implication_pays_at_least_one_where_possible() {
    let m = implication();
    for (c, b) in m.basket.iter().enumerate() {
        if m.possible[c] {
            assert!(b.unwrap() >= dec!(1));
        }
    }
}
