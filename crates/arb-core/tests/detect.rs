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

// ---------- F4: no signal from crossed books (D1) ----------

#[test]
fn f4_crossed_leg_blocks_yes_basket_signal() {
    let g = Group {
        legs: vec![
            live("0.28", "0.30"),
            live("0.28", "0.30"),
            live("0.31", "0.30"), // crossed: bid > ask
        ],
        augmented: false,
    };
    assert!(detect_yes_basket(&g).is_none());
}

#[test]
fn f4_crossed_leg_blocks_no_basket_signal() {
    let g = Group {
        legs: vec![live("0.55", "0.56"), live("0.57", "0.56")], // second leg crossed
        augmented: false,
    };
    assert!(detect_no_basket(&g).is_none());
}

#[test]
fn f4_crossed_leg_blocks_implication_signal() {
    let a = live("0.60", "0.62");
    let b = live("0.53", "0.55");
    assert!(
        detect_implication(&a, &b).is_some(),
        "sanity: uncrossed signals"
    );

    let crossed_b = live("0.56", "0.55"); // crossed
    assert!(detect_implication(&a, &crossed_b).is_none());

    let crossed_a = live("0.63", "0.62"); // crossed
    assert!(detect_implication(&crossed_a, &b).is_none());
}

// ---------- M3: net-exactly-zero boundary ----------

#[test]
fn m3_yes_basket_net_exactly_zero_has_gross_but_not_net() {
    // asks 0.20 and 0.60, rate 0.5, exponent 1: m = 1 − 0.80 − 0.5×(0.16 + 0.24) = 0 exactly.
    let f = FeeSchedule::new(dec!(0.5), 1).unwrap();
    let mut a = live("0.10", "0.20");
    let mut b = live("0.50", "0.60");
    a.fee = f;
    b.fee = f;
    let g = Group {
        legs: vec![a, b],
        augmented: false,
    };
    let s = detect_yes_basket(&g).unwrap();
    assert!(s.gross.pnl_gross > Decimal::ZERO);
    assert!(!s.net_positive());
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

/// A live, zero-fee leg with at most one level per side, size 10.
fn leg6(bid: Option<&str>, ask: Option<&str>) -> GroupLeg {
    let s = Shares::parse("10").unwrap();
    let bids: Vec<(Price, Shares)> = bid
        .map(|p| (Price::parse(p).unwrap(), s))
        .into_iter()
        .collect();
    let asks: Vec<(Price, Shares)> = ask
        .map(|p| (Price::parse(p).unwrap(), s))
        .into_iter()
        .collect();
    let mut b = Book::default();
    b.apply_snapshot(&bids, &asks, 0);
    GroupLeg {
        status: LegStatus::Live,
        book: b,
        fee: FeeSchedule::ZERO,
    }
}

fn grp(legs: Vec<GroupLeg>) -> Group {
    Group {
        legs,
        augmented: false,
    }
}

#[test]
fn gate_boundaries_match_the_detectors_at_six_decimals() {
    // Spec §15: ΣA = 1.000000 → none; 0.999999 → yes.
    let at = grp(vec![
        leg6(None, Some("0.400000")),
        leg6(None, Some("0.600000")),
    ]);
    assert!(!yes_basket_may_signal(&at));
    assert!(detect_yes_basket(&at).is_none());
    let under = grp(vec![
        leg6(None, Some("0.399999")),
        leg6(None, Some("0.600000")),
    ]);
    assert!(yes_basket_may_signal(&under));
    assert!(detect_yes_basket(&under).is_some());
    // ΣB = 1.000000 → none; 1.000001 → yes.
    let at = grp(vec![
        leg6(Some("0.400000"), None),
        leg6(Some("0.600000"), None),
    ]);
    assert!(!no_basket_may_signal(&at));
    assert!(detect_no_basket(&at).is_none());
    let over = grp(vec![
        leg6(Some("0.400001"), None),
        leg6(Some("0.600000"), None),
    ]);
    assert!(no_basket_may_signal(&over));
    assert!(detect_no_basket(&over).is_some());
}

#[test]
fn gate_is_false_when_a_leg_is_missing_its_side_or_nothing_is_live() {
    // YES: a live leg with no ask can't be in a complete basket, however cheap the others are.
    let g = grp(vec![leg6(None, Some("0.10")), leg6(Some("0.05"), None)]);
    assert!(!yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_none());
    // YES: no live leg at all.
    let mut dark = grp(vec![leg6(None, Some("0.10"))]);
    dark.legs[0].status = LegStatus::Placeholder;
    assert!(!yes_basket_may_signal(&dark));
    assert!(detect_yes_basket(&dark).is_none());
    // NO: only one live leg has a bid (D2b needs ≥ 2), even at 0.99.
    let g = grp(vec![leg6(Some("0.99"), None), leg6(None, Some("0.50"))]);
    assert!(!no_basket_may_signal(&g));
    assert!(detect_no_basket(&g).is_none());
    // Non-live legs never count: live asks 0.30 + 0.30 < 1 with a ClosedNo leg at 0.50.
    let mut g = grp(vec![
        leg6(None, Some("0.30")),
        leg6(None, Some("0.30")),
        leg6(None, Some("0.50")),
    ]);
    g.legs[2].status = LegStatus::ClosedNo;
    assert!(yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_some());
}

#[test]
fn gate_uses_the_gross_threshold_so_fee_eaten_baskets_still_reach_the_detector() {
    // m3's books: gross edge 0.20 > 0, net exactly 0. The detector reports the gross signal
    // (Signal.gross feeds paper comparisons), so the gate must not hide it.
    let f = FeeSchedule::new(dec!(0.5), 1).unwrap();
    let mut a = live("0.10", "0.20");
    let mut b = live("0.50", "0.60");
    a.fee = f;
    b.fee = f;
    let g = grp(vec![a, b]);
    assert!(yes_basket_may_signal(&g));
    let s = detect_yes_basket(&g).unwrap();
    assert!(!s.net_positive());
}

#[test]
fn a_crossed_book_opens_the_gate_but_the_detector_still_refuses() {
    // Σ asks = 0.50 < 1, but leg 0 is crossed (bid 0.40 > ask 0.30): D1 wins.
    let g = grp(vec![
        leg6(Some("0.40"), Some("0.30")),
        leg6(None, Some("0.20")),
    ]);
    assert!(yes_basket_may_signal(&g));
    assert!(detect_yes_basket(&g).is_none());
}

fn arb_leg() -> impl Strategy<Value = GroupLeg> {
    let status = prop_oneof![
        6 => Just(LegStatus::Live),
        1 => Just(LegStatus::Placeholder),
        1 => Just(LegStatus::ClosedNo),
        1 => Just(LegStatus::ClosedYes),
        1 => Just(LegStatus::Halted),
    ];
    let side = || prop::collection::vec((1u32..=99, 1u32..=50), 0..=3); // (cents, shares)
    (status, side(), side(), 0u32..=25, 1u8..=4).prop_map(|(status, bids, asks, rate, exp)| {
        let lv = |v: &[(u32, u32)]| -> Vec<(Price, Shares)> {
            v.iter()
                .map(|&(c, s)| {
                    (
                        Price::new(Decimal::new(c.into(), 2)).unwrap(),
                        Shares::new(Decimal::from(s)).unwrap(),
                    )
                })
                .collect()
        };
        let mut book = Book::default();
        book.apply_snapshot(&lv(&bids), &lv(&asks), 0);
        GroupLeg {
            status,
            book,
            fee: FeeSchedule::new(Decimal::new(rate.into(), 2), exp).unwrap(),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// S3a-2: a false gate ⇒ the detector returns None (the engine may skip it); a true gate on
    /// an undecided group with uncrossed relevant legs ⇒ Some (the gate is exact, not loose).
    #[test]
    fn gate_false_means_no_signal_and_gate_true_is_exact(
        legs in prop::collection::vec(arb_leg(), 1..=6),
        augmented in any::<bool>(),
    ) {
        let g = Group { legs, augmented };
        let decided = g.legs.iter().any(|l| l.status == LegStatus::ClosedYes);

        let yes = detect_yes_basket(&g);
        let live_crossed = g
            .legs
            .iter()
            .any(|l| l.status == LegStatus::Live && l.book.is_crossed());
        if !yes_basket_may_signal(&g) {
            prop_assert!(yes.is_none(), "YES gate false but the detector signalled");
        } else if !decided && !live_crossed {
            prop_assert!(yes.is_some(), "YES gate true, undecided, uncrossed, yet no signal");
        }

        let no = detect_no_basket(&g);
        let used_crossed = g.legs.iter().any(|l| {
            l.status == LegStatus::Live && l.book.top().best_bid.is_some() && l.book.is_crossed()
        });
        if !no_basket_may_signal(&g) {
            prop_assert!(no.is_none(), "NO gate false but the detector signalled");
        } else if !decided && !used_crossed {
            prop_assert!(no.is_some(), "NO gate true, undecided, uncrossed, yet no signal");
        }
    }
}
