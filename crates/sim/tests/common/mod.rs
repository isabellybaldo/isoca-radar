#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;

pub fn p(s: &str) -> Price {
    Price::parse(s).unwrap()
}

pub fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}

pub fn levels(v: &[(&str, &str)]) -> Vec<(Price, Shares)> {
    v.iter()
        .map(|(a, b)| (p(a), Shares::parse(b).unwrap()))
        .collect()
}

pub fn book(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> Book {
    let mut b = Book::default();
    b.apply_snapshot(&levels(bids), &levels(asks), 0);
    b
}

pub fn fee(rate: &str, exponent: u8) -> FeeSchedule {
    FeeSchedule::new(d(rate), exponent).unwrap()
}
