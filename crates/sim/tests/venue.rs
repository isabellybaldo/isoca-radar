#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::fee::FeeSchedule;
use common::{book, d, fee, p};
use sim::consumed::{ConsumedLiquidity, LevelKey};
use sim::types::{BookSide, LegKey, Outcome};
use sim::venue::{fak_buy, fak_sell, order_size};

fn leg(outcome: Outcome) -> LegKey {
    LegKey {
        market_id: "m1".into(),
        yes_token: "y1".into(),
        outcome,
    }
}

#[test]
fn buy_walks_asks_up_to_the_limit_with_conservative_fees() {
    let b = book(&[], &[("0.40", "10"), ("0.42", "20")]);
    let mut c = ConsumedLiquidity::default();
    let f = fak_buy(
        &b,
        &leg(Outcome::Yes),
        fee("0.25", 1),
        p("0.42"),
        d("25"),
        &mut c,
    );
    assert_eq!(f.filled, d("25"));
    assert_eq!(f.notional, d("10.3"));
    // φ(0.40) = 0.25·0.24 = 0.06 → 0.6; φ(0.42) = 0.25·0.2436 = 0.0609 → 15 × = 0.9135
    assert_eq!(f.fees, d("1.5135"));
    assert_eq!(f.levels_used, 2);
    let f2 = fak_buy(
        &b,
        &leg(Outcome::Yes),
        FeeSchedule::ZERO,
        p("0.40"),
        d("25"),
        &mut ConsumedLiquidity::default(),
    );
    assert_eq!(f2.filled, d("10"), "the limit stops the walk");
}

#[test]
fn buying_no_consumes_the_mirrored_yes_bid() {
    let b = book(&[("0.60", "10")], &[]);
    let mut c = ConsumedLiquidity::default();
    let f = fak_buy(
        &b,
        &leg(Outcome::No),
        FeeSchedule::ZERO,
        p("0.40"),
        d("4"),
        &mut c,
    );
    assert_eq!((f.filled, f.notional), (d("4"), d("1.6")));
    let k = LevelKey {
        yes_token: "y1".into(),
        side: BookSide::Bid,
        price: d("0.60"),
    };
    assert_eq!(c.available(&k, d("10")), d("6"));
}

#[test]
fn consumed_liquidity_is_not_filled_twice() {
    let b = book(&[], &[("0.40", "10")]);
    let mut c = ConsumedLiquidity::default();
    let k = LevelKey {
        yes_token: "y1".into(),
        side: BookSide::Ask,
        price: d("0.40"),
    };
    c.consume(k, d("10"), d("7"));
    let f = fak_buy(
        &b,
        &leg(Outcome::Yes),
        FeeSchedule::ZERO,
        p("0.40"),
        d("25"),
        &mut c,
    );
    assert_eq!(f.filled, d("3"));
}

#[test]
fn selling_no_hits_the_mirrored_yes_ask() {
    let b = book(&[], &[("0.45", "100")]);
    let mut c = ConsumedLiquidity::default();
    let f = fak_sell(&b, &leg(Outcome::No), FeeSchedule::ZERO, d("10"), &mut c);
    assert_eq!((f.filled, f.notional), (d("10"), d("5.5")));
    let k = LevelKey {
        yes_token: "y1".into(),
        side: BookSide::Ask,
        price: d("0.45"),
    };
    assert_eq!(c.available(&k, d("100")), d("90"));
    assert_eq!(f.vwap(), Some(d("0.55")));
}

#[test]
fn order_size_rounds_down_to_venue_precision() {
    assert_eq!(
        order_size(&[p("0.6"), p("0.65"), p("0.7")], d("51.28"), d("5")),
        d("51.2")
    );
    assert_eq!(order_size(&[p("0.5")], d("7.999"), d("5")), d("7.98"));
    assert_eq!(order_size(&[p("0.333")], d("10"), d("5")), d("10"));
    assert_eq!(
        order_size(&[p("0.5")], d("4.99"), d("5")),
        d("0"),
        "below the 5-share minimum"
    );
}

#[test]
fn books_live_in_venue_and_the_trader_re_exports_them() {
    use arb_core::book::Book;
    use std::collections::HashMap;
    fn via_venue(b: &impl sim::venue::Books) -> bool {
        b.yes_book("y1").is_some()
    }
    fn via_trader(b: &impl sim::trader::Books) -> bool {
        b.yes_book("y1").is_some() && b.yes_book("y2").is_none()
    }
    let mut m: HashMap<String, Book> = HashMap::new();
    m.insert("y1".into(), book(&[("0.40", "10")], &[]));
    assert!(via_venue(&m) && via_trader(&m));
}
