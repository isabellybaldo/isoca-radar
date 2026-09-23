#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::{Book, Side};
use arb_core::money::{Price, Shares};

fn lv(p: &str, s: &str) -> (Price, Shares) {
    (Price::parse(p).unwrap(), Shares::parse(s).unwrap())
}

/// Live "Fed Decision in October? · No change" YES book, 2026-09-22 (spec §5.7a).
fn fed() -> Book {
    let mut b = Book::default();
    // REST order: bids ascending, asks descending (best = last). Input order must not matter.
    b.apply_snapshot(
        &[
            lv("0.52", "42239.53"),
            lv("0.53", "85887.16"),
            lv("0.54", "5603.74"),
        ],
        &[
            lv("0.57", "23963.36"),
            lv("0.56", "252281.27"),
            lv("0.55", "32356.03"),
        ],
        1_790_121_492_396,
    );
    b
}

#[test]
fn best_prices_regardless_of_input_order() {
    let t = fed().top();
    assert_eq!(t.best_bid, Some(lv("0.54", "5603.74")));
    assert_eq!(t.best_ask, Some(lv("0.55", "32356.03")));
    assert_eq!(fed().server_ts_ms(), 1_790_121_492_396);
}

#[test]
fn l1_no_asks_mirror_yes_bids_level_by_level() {
    let no_asks = fed().no_asks_best_first();
    assert_eq!(
        no_asks,
        vec![
            lv("0.46", "5603.74"),
            lv("0.47", "85887.16"),
            lv("0.48", "42239.53")
        ]
    );
    let no_bids = fed().no_bids_best_first();
    assert_eq!(no_bids[0], lv("0.45", "32356.03"));
}

#[test]
fn delta_zero_removes_level_and_reports_top_change() {
    let mut b = fed();
    assert!(b.apply_delta(
        Side::Bid,
        Price::parse("0.54").unwrap(),
        Shares::parse("0").unwrap(),
        2
    ));
    assert_eq!(b.top().best_bid, Some(lv("0.53", "85887.16")));
    // a change below the top is not a top change
    assert!(!b.apply_delta(
        Side::Ask,
        Price::parse("0.57").unwrap(),
        Shares::parse("1").unwrap(),
        3
    ));
    assert_eq!(b.server_ts_ms(), 3);
}

#[test]
fn l2_crossed_only_when_bid_exceeds_ask() {
    let mut b = Book::default();
    b.apply_snapshot(&[lv("0.50", "10")], &[lv("0.51", "10")], 0);
    assert!(!b.is_crossed());
    b.apply_snapshot(&[lv("0.50", "10")], &[lv("0.50", "10")], 0);
    assert!(!b.is_crossed(), "locked is not crossed");
    b.apply_snapshot(&[lv("0.52", "10")], &[lv("0.50", "10")], 0);
    assert!(b.is_crossed());
}

#[test]
fn snapshot_ignores_zero_size_levels() {
    let mut b = Book::default();
    b.apply_snapshot(&[lv("0.40", "0"), lv("0.39", "5")], &[], 0);
    assert_eq!(b.top().best_bid, Some(lv("0.39", "5")));
}
