#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::rows::*;
use rust_decimal_macros::dec;

#[test]
fn micro_units_are_exact_or_refused() {
    assert_eq!(micros(dec!(0.123456)).unwrap(), 123_456);
    assert_eq!(micros(dec!(100)).unwrap(), 100_000_000);
    assert_eq!(micros(dec!(0)).unwrap(), 0);
    assert!(micros(dec!(0.1234567)).is_err());
}
