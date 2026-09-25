#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use rust_decimal_macros::dec;
use sim::consumed::{ConsumedLiquidity, LevelKey};
use sim::types::BookSide;

fn key() -> LevelKey {
    LevelKey {
        yes_token: "y1".into(),
        side: BookSide::Ask,
        price: dec!(0.40),
    }
}

#[test]
fn nothing_consumed_means_the_feed_size() {
    assert_eq!(
        ConsumedLiquidity::default().available(&key(), dec!(100)),
        dec!(100)
    );
}

#[test]
fn consumed_clears_only_when_the_level_shrinks_by_at_least_that_much() {
    let mut c = ConsumedLiquidity::default();
    c.consume(key(), dec!(100), dec!(50));
    assert_eq!(c.available(&key(), dec!(100)), dec!(50));
    c.on_level(&key(), dec!(100));
    assert_eq!(c.len(), 1, "unchanged level: still consumed");
    c.on_level(&key(), dec!(60));
    assert_eq!(c.len(), 1, "shrunk by 40 < 50: still consumed");
    assert_eq!(c.available(&key(), dec!(60)), dec!(10));
    c.on_level(&key(), dec!(50));
    assert!(c.is_empty(), "shrunk by 50: someone else took it");
}

#[test]
fn a_removed_level_clears() {
    let mut c = ConsumedLiquidity::default();
    c.consume(key(), dec!(100), dec!(10));
    c.on_level(&key(), dec!(0));
    assert!(c.is_empty());
}

#[test]
fn growth_does_not_clear_and_new_orders_are_available() {
    let mut c = ConsumedLiquidity::default();
    c.consume(key(), dec!(100), dec!(50));
    c.on_level(&key(), dec!(130));
    assert_eq!(c.available(&key(), dec!(130)), dec!(80));
    assert_eq!(c.len(), 1);
}

#[test]
fn snapshot_clears_missing_levels_only() {
    let mut c = ConsumedLiquidity::default();
    c.consume(key(), dec!(100), dec!(10));
    let other = LevelKey {
        yes_token: "y1".into(),
        side: BookSide::Bid,
        price: dec!(0.30),
    };
    c.consume(other.clone(), dec!(20), dec!(5));
    let mut b = Book::default();
    let lv = |p: &str, s: &str| (Price::parse(p).unwrap(), Shares::parse(s).unwrap());
    b.apply_snapshot(&[lv("0.30", "20")], &[], 0); // ask 0.40 is gone, bid 0.30 unchanged
    c.on_snapshot("y1", &b);
    assert_eq!(c.len(), 1);
    assert_eq!(c.available(&other, dec!(20)), dec!(15));
}

#[test]
fn round_trips_through_json() {
    let mut c = ConsumedLiquidity::default();
    c.consume(key(), dec!(100), dec!(12.5));
    let back: ConsumedLiquidity =
        serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}
