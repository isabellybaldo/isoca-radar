#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::detect::*;
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn book(bid: &str, ask: &str, size: &str) -> Book {
    let mut b = Book::default();
    let s = Shares::parse(size).unwrap();
    b.apply_snapshot(
        &[(Price::parse(bid).unwrap(), s)],
        &[(Price::parse(ask).unwrap(), s)],
        0,
    );
    b
}
fn live(bid: &str, ask: &str) -> GroupLeg {
    GroupLeg {
        status: LegStatus::Live,
        book: book(bid, ask, "100"),
        fee: FeeSchedule::ZERO,
    }
}

#[test]
fn l2_canary_only_on_crossed_books() {
    assert!(!canary(&book("0.54", "0.55", "1")));
    assert!(!canary(&book("0.55", "0.55", "1")));
    assert!(canary(&book("0.56", "0.55", "1")));
}

#[test]
fn l3_complete_yes_basket_is_settlement_arbitrage() {
    let g = Group {
        legs: vec![
            live("0.28", "0.30"),
            live("0.28", "0.30"),
            live("0.28", "0.30"),
        ],
        augmented: false,
    };
    let s = detect_yes_basket(&g).unwrap();
    assert_eq!(s.kind, Kind::YesBasket);
    assert_eq!(s.realization, Realization::Settlement);
    assert!(s.net_positive());
    assert_eq!(s.quote.pnl, dec!(10)); // (1 − 0.90) × 100
}

#[test]
fn l3_augmented_trap_is_never_arbitrage() {
    // Named legs' asks sum to 0.88, like the 2028 nominee events on 2026-09-22.
    let g = Group {
        legs: vec![live("0.40", "0.44"), live("0.40", "0.44")],
        augmented: true,
    };
    let s = detect_yes_basket(&g).unwrap();
    assert_eq!(s.kind, Kind::YesBasketIncomplete);
    assert_eq!(s.realization, Realization::NotArbitrage);
}

#[test]
fn closed_no_leg_keeps_completeness_and_closed_yes_decides_the_event() {
    let mut legs = vec![live("0.40", "0.45"), live("0.40", "0.45")];
    legs.push(GroupLeg {
        status: LegStatus::ClosedNo,
        book: Book::default(),
        fee: FeeSchedule::ZERO,
    });
    let g = Group {
        legs: legs.clone(),
        augmented: false,
    };
    assert_eq!(detect_yes_basket(&g).unwrap().kind, Kind::YesBasket);
    legs[2].status = LegStatus::ClosedYes;
    let g = Group {
        legs,
        augmented: false,
    };
    assert!(detect_yes_basket(&g).is_none());
    assert!(detect_no_basket(&g).is_none());
}

#[test]
fn placeholder_leg_makes_yes_basket_incomplete() {
    let mut legs = vec![live("0.40", "0.45"), live("0.40", "0.45")];
    legs.push(GroupLeg {
        status: LegStatus::Placeholder,
        book: Book::default(),
        fee: FeeSchedule::ZERO,
    });
    let s = detect_yes_basket(&Group {
        legs,
        augmented: false,
    })
    .unwrap();
    assert_eq!(s.kind, Kind::YesBasketIncomplete);
}

#[test]
fn exact_one_is_no_signal_both_sides() {
    let g = Group {
        legs: vec![live("0.49", "0.50"), live("0.49", "0.50")],
        augmented: false,
    };
    assert!(detect_yes_basket(&g).is_none(), "asks sum to exactly 1.00");
    let g = Group {
        legs: vec![live("0.50", "0.51"), live("0.50", "0.51")],
        augmented: false,
    };
    assert!(detect_no_basket(&g).is_none(), "bids sum to exactly 1.00");
}

#[test]
fn l4_no_basket_skips_legs_without_bids_and_works_when_augmented() {
    let mut no_bid = Book::default();
    no_bid.apply_snapshot(
        &[],
        &[(Price::parse("0.01").unwrap(), Shares::parse("100").unwrap())],
        0,
    );
    let legs = vec![
        live("0.55", "0.56"),
        live("0.50", "0.51"),
        GroupLeg {
            status: LegStatus::Live,
            book: no_bid,
            fee: FeeSchedule::ZERO,
        },
    ];
    let s = detect_no_basket(&Group {
        legs,
        augmented: true,
    })
    .unwrap();
    assert_eq!(s.kind, Kind::NoBasket);
    assert_eq!(s.realization, Realization::Converter);
    assert_eq!(s.legs_used, vec![0, 1]);
    assert_eq!(s.quote.pnl, dec!(5)); // (0.55 + 0.50 − 1) × 100
}

#[test]
fn gross_only_signal_is_reported_but_not_net_positive() {
    let f = FeeSchedule::new(dec!(0.05), 1).unwrap();
    let mut a = live("0.50", "0.51");
    let mut b = live("0.502", "0.51");
    a.fee = f;
    b.fee = f;
    let s = detect_no_basket(&Group {
        legs: vec![a, b],
        augmented: false,
    })
    .unwrap();
    assert!(s.gross.pnl_gross > Decimal::ZERO);
    assert!(!s.net_positive());
}

#[test]
fn l10_implication_edge() {
    // A ⇒ B. A's YES bid 0.60 → NO_A ask 0.40; B's YES ask 0.55. Edge = 0.60 − 0.55 = 0.05.
    let a = live("0.60", "0.62");
    let b = live("0.53", "0.55");
    let s = detect_implication(&a, &b).unwrap();
    assert_eq!(s.kind, Kind::Implication);
    assert_eq!(s.realization, Realization::Settlement);
    assert_eq!(s.quote.pnl, dec!(5));
}

#[test]
fn implication_requires_both_legs_live() {
    let a = live("0.60", "0.62");
    let mut b = live("0.53", "0.55");
    assert!(detect_implication(&a, &b).is_some());
    for status in [
        LegStatus::ClosedYes,
        LegStatus::ClosedNo,
        LegStatus::Placeholder,
        LegStatus::Other,
        LegStatus::Halted,
    ] {
        b.status = status;
        assert!(detect_implication(&a, &b).is_none(), "{status:?}");
        assert!(detect_implication(&b, &a).is_none(), "{status:?} as A");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// L7: on uncrossed books the YES and NO baskets are never both profitable (even gross).
    #[test]
    fn l7_yes_and_no_never_both_positive(legs in prop::collection::vec((1u32..=96, 1u32..=3), 2..=6)) {
        let group = Group {
            legs: legs.iter().map(|&(b, spread)| {
                let bid = Price::new(Decimal::new(b.into(), 2)).unwrap();
                let ask = Price::new(Decimal::new((b + spread).into(), 2)).unwrap();
                let mut bk = Book::default();
                let s = Shares::parse("10").unwrap();
                bk.apply_snapshot(&[(bid, s)], &[(ask, s)], 0);
                GroupLeg { status: LegStatus::Live, book: bk, fee: FeeSchedule::ZERO }
            }).collect(),
            augmented: false,
        };
        let yes = detect_yes_basket(&group).is_some();
        let no = detect_no_basket(&group).is_some();
        prop_assert!(!(yes && no));
    }
}
