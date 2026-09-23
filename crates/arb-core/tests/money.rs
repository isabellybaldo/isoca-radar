#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::money::{MoneyError, Price, Shares};
use rust_decimal_macros::dec;

#[test]
fn parses_exact_prices() {
    assert_eq!(Price::parse("0.54").unwrap().get(), dec!(0.54));
    assert_eq!(Price::parse("1").unwrap().get(), dec!(1));
    assert_eq!(Price::parse("0").unwrap().get(), dec!(0));
    assert_eq!(Price::parse("0.0025").unwrap().get(), dec!(0.0025));
}

#[test]
fn trailing_zeros_are_the_same_price() {
    assert_eq!(
        Price::parse("0.5").unwrap(),
        Price::parse("0.500000").unwrap()
    );
}

#[test]
fn rejects_more_than_six_decimal_places() {
    assert!(matches!(
        Price::parse("0.1234567"),
        Err(MoneyError::TooPrecise(_))
    ));
    assert!(matches!(
        Shares::parse("1.0000001"),
        Err(MoneyError::TooPrecise(_))
    ));
}

#[test]
fn rejects_out_of_range_and_garbage() {
    assert!(matches!(
        Price::parse("1.01"),
        Err(MoneyError::OutOfRange(_))
    ));
    assert!(matches!(
        Price::parse("-0.1"),
        Err(MoneyError::OutOfRange(_))
    ));
    assert!(matches!(
        Shares::parse("-1"),
        Err(MoneyError::OutOfRange(_))
    ));
    assert!(matches!(Price::parse("abc"), Err(MoneyError::Parse(_))));
}

#[test]
fn complement_is_the_mirror_price() {
    assert_eq!(
        Price::parse("0.54").unwrap().complement(),
        Price::parse("0.46").unwrap()
    );
    assert_eq!(
        Price::parse("0").unwrap().complement(),
        Price::parse("1").unwrap()
    );
}

#[test]
fn large_share_sizes_parse() {
    assert_eq!(Shares::parse("2116131.59").unwrap().get(), dec!(2116131.59));
}
