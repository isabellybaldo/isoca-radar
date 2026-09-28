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

#[test]
fn micro_units_refuse_an_overflow() {
    // Decimal's own multiplication overflows
    let e = micros(rust_decimal::Decimal::MAX).unwrap_err().to_string();
    assert!(e.contains("overflows"), "{e}");
    // fits a Decimal, not an i64
    let e = micros(dec!(10_000_000_000_000)).unwrap_err().to_string();
    assert!(e.contains("does not fit an i64"), "{e}");
    assert_eq!(
        micros(dec!(9_223_372_036_854.775807)).unwrap(),
        i64::MAX,
        "the largest value that fits"
    );
}
