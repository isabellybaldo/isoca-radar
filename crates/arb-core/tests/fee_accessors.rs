#![allow(clippy::unwrap_used)]
use arb_core::fee::FeeSchedule;
use rust_decimal_macros::dec;

#[test]
fn schedule_exposes_rate_and_exponent() {
    let f = FeeSchedule::new(dec!(0.07), 2).unwrap();
    assert_eq!((f.rate(), f.exponent()), (dec!(0.07), 2));
    assert_eq!(
        (FeeSchedule::ZERO.rate(), FeeSchedule::ZERO.exponent()),
        (dec!(0), 1)
    );
}
