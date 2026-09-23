#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::stats::{empirical_survival, kaplan_meier};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn kaplan_meier_matches_hand_computed_table() {
    // durations: 1 (event), 2 (censored), 3 (event), 3 (event), 5 (censored)
    // t=1: at risk 5, 1 event → S = 4/5 = 0.8
    // t=3: at risk 3, 2 events → S = 0.8 × 1/3 = 0.2666…
    let s = kaplan_meier(&[(1, true), (2, false), (3, true), (3, true), (5, false)]);
    assert_eq!(s.len(), 2);
    assert_eq!(s[0], (1, dec!(0.8)));
    assert_eq!(s[1].0, 3);
    assert_eq!(s[1].1.round_dp(10), dec!(0.2666666667));
}

#[test]
fn kaplan_meier_without_events_is_empty() {
    assert!(kaplan_meier(&[(4, false), (9, false)]).is_empty());
}

#[test]
fn empirical_survival_is_share_above_tau() {
    let d = [1_000, 3_600, 16_150, 60_000];
    assert_eq!(empirical_survival(&d, 3_600), dec!(0.5));
    assert_eq!(empirical_survival(&d, 0), Decimal::ONE);
    assert_eq!(empirical_survival(&[], 10), Decimal::ZERO);
}
