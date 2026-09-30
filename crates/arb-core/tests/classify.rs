//! Phase 7 spec §3.3: classify = the existing detectors plus the path taken (parent §5.7 (c)).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Book;
use arb_core::classify::{Node, classify};
use arb_core::detect::{
    Group, GroupLeg, Kind, LegStatus, Realization, Signal, detect_no_basket, detect_yes_basket,
};
use arb_core::fee::FeeSchedule;
use arb_core::fill::BasketQuote;
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;

fn leg(status: LegStatus, bid: Option<&str>, ask: Option<&str>) -> GroupLeg {
    let side = |p: Option<&str>| {
        p.map(|p| vec![(Price::parse(p).unwrap(), Shares::parse("100").unwrap())])
            .unwrap_or_default()
    };
    let mut book = Book::default();
    book.apply_snapshot(&side(bid), &side(ask), 0);
    GroupLeg {
        status,
        book,
        fee: FeeSchedule::ZERO,
    }
}

fn live(bid: &str, ask: &str) -> GroupLeg {
    leg(LegStatus::Live, Some(bid), Some(ask))
}

/// `Signal` has no `PartialEq` (detect.rs is premise-locked): compare what it carries.
type Key = (Kind, Realization, BasketQuote, BasketQuote, Vec<usize>);
fn key(s: &Signal) -> Key {
    (
        s.kind,
        s.realization,
        s.quote.clone(),
        s.gross.clone(),
        s.legs_used.clone(),
    )
}

#[test]
fn a_no_basket_edge_stops_after_d2b() {
    let g = Group {
        legs: vec![
            live("0.40", "0.41"),
            live("0.35", "0.36"),
            live("0.30", "0.31"),
        ],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::NoBasketTried, Node::NoBasketPositive]);
    assert_eq!(c.signal.unwrap().kind, Kind::NoBasket);
}

#[test]
fn an_other_leg_makes_the_yes_basket_incomplete() {
    let g = Group {
        legs: vec![
            live("0.20", "0.30"),
            live("0.20", "0.30"),
            leg(LegStatus::Other, None, None),
        ],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(
        c.path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketNotPositive,
            Node::YesBasketTried,
            Node::YesBasketPositive,
            Node::Incomplete
        ]
    );
    assert_eq!(c.signal.unwrap().kind, Kind::YesBasketIncomplete);
}

#[test]
fn a_decided_event_yields_nothing() {
    let g = Group {
        legs: vec![leg(LegStatus::ClosedYes, None, None), live("0.20", "0.30")],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(c.path, vec![Node::Decided]);
    assert!(c.signal.is_none());
}

#[test]
fn a_missing_ask_and_a_crossed_book_are_named() {
    let g = Group {
        legs: vec![
            live("0.20", "0.30"),
            leg(LegStatus::Live, Some("0.20"), None),
        ],
        augmented: false,
    };
    assert_eq!(classify(&g).path.last(), Some(&Node::YesBasketMissingAsk));
    let crossed = Group {
        legs: vec![live("0.56", "0.55"), live("0.60", "0.61")],
        augmented: false,
    };
    assert_eq!(
        classify(&crossed).path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketCrossed,
            Node::YesBasketTried,
            Node::YesBasketCrossed
        ]
    );
}

fn status() -> impl Strategy<Value = LegStatus> {
    prop_oneof![
        Just(LegStatus::Live),
        Just(LegStatus::Live),
        Just(LegStatus::Live),
        Just(LegStatus::Placeholder),
        Just(LegStatus::Other),
        Just(LegStatus::ClosedNo),
        Just(LegStatus::ClosedYes),
        Just(LegStatus::Halted),
    ]
}

fn side() -> impl Strategy<Value = Vec<(Price, Shares)>> {
    prop::collection::btree_map(1u32..=99, 1u32..=300, 0..=3).prop_map(|m| {
        m.into_iter()
            .map(|(c, s)| {
                (
                    Price::new(Decimal::new(c.into(), 2)).unwrap(),
                    Shares::new(Decimal::from(s)).unwrap(),
                )
            })
            .collect()
    })
}

fn group() -> impl Strategy<Value = Group> {
    (
        prop::collection::vec((status(), side(), side(), any::<bool>()), 1..=6),
        any::<bool>(),
    )
        .prop_map(|(legs, augmented)| Group {
            legs: legs
                .into_iter()
                .map(|(status, bids, asks, fee)| {
                    let mut book = Book::default();
                    book.apply_snapshot(&bids, &asks, 0);
                    GroupLeg {
                        status,
                        book,
                        fee: if fee {
                            FeeSchedule::new(Decimal::new(7, 2), 1).unwrap()
                        } else {
                            FeeSchedule::ZERO
                        },
                    }
                })
                .collect(),
            augmented,
        })
}

proptest! {
    #[test]
    fn classify_agrees_with_the_detectors(g in group()) {
        let c = classify(&g);
        let want = detect_no_basket(&g).or_else(|| detect_yes_basket(&g));
        prop_assert_eq!(c.signal.as_ref().map(key), want.as_ref().map(key));
        let last = c.path.last().copied();
        let positive = matches!(
            last,
            Some(Node::NoBasketPositive | Node::Complete | Node::Incomplete)
        );
        prop_assert_eq!(positive, c.signal.is_some(), "the path ends positive iff there is a signal: {:?}", c.path);
        prop_assert!(
            !(detect_no_basket(&g).is_some() && detect_yes_basket(&g).is_some()),
            "L7"
        );
    }
}

/// The guards restated on the `Group` alone, without classify.rs's helpers.
fn has_bid(l: &GroupLeg) -> bool {
    l.status == LegStatus::Live && l.book.top().best_bid.is_some()
}

proptest! {
    #[test]
    fn the_negative_node_is_the_one_the_guards_name(g in group()) {
        let c = classify(&g);
        let has = |n: Node| c.path.contains(&n);
        let decided = g.legs.iter().any(|l| l.status == LegStatus::ClosedYes);
        prop_assert_eq!(c.path == vec![Node::Decided], decided, "{:?}", c.path);
        if decided {
            return Ok(());
        }
        prop_assert_eq!(c.path[0], Node::NoBasketTried);
        let used: Vec<&GroupLeg> = g.legs.iter().filter(|l| has_bid(l)).collect();
        prop_assert_eq!(has(Node::NoBasketFewLegs), used.len() < 2);
        let no_crossed = used.len() >= 2 && used.iter().any(|l| l.book.is_crossed());
        prop_assert_eq!(has(Node::NoBasketCrossed), no_crossed);
        let no_not_positive = used.len() >= 2 && !no_crossed && detect_no_basket(&g).is_none();
        prop_assert_eq!(has(Node::NoBasketNotPositive), no_not_positive);
        prop_assert_eq!(
            has(Node::NoBasketPositive),
            used.len() >= 2 && !no_crossed && !no_not_positive
        );
        // D2a is reached iff D2b was not positive.
        let reached = !has(Node::NoBasketPositive);
        prop_assert_eq!(has(Node::YesBasketTried), reached);
        if !reached {
            return Ok(());
        }
        let live: Vec<&GroupLeg> = g.legs.iter().filter(|l| l.status == LegStatus::Live).collect();
        let yes_crossed = live.iter().any(|l| l.book.is_crossed());
        let missing = live.iter().any(|l| l.book.top().best_ask.is_none());
        prop_assert_eq!(has(Node::YesBasketNoLive), live.is_empty());
        prop_assert_eq!(has(Node::YesBasketCrossed), !live.is_empty() && yes_crossed);
        prop_assert_eq!(
            has(Node::YesBasketMissingAsk),
            !live.is_empty() && !yes_crossed && missing
        );
        let walked = !live.is_empty() && !yes_crossed && !missing;
        let yes_none = detect_yes_basket(&g).is_none();
        prop_assert_eq!(has(Node::YesBasketNotPositive), walked && yes_none);
        prop_assert_eq!(has(Node::YesBasketPositive), walked && !yes_none);
        if walked && yes_none {
            prop_assert_eq!(c.path.last(), Some(&Node::YesBasketNotPositive));
        }
    }
}

#[test]
fn a_single_bid_leg_is_too_few_for_the_no_basket() {
    let g = Group {
        legs: vec![
            live("0.20", "0.30"),
            leg(LegStatus::Live, None, Some("0.30")),
        ],
        augmented: false,
    };
    assert_eq!(
        classify(&g).path[..2],
        [Node::NoBasketTried, Node::NoBasketFewLegs]
    );
}

#[test]
fn no_live_leg_ends_the_yes_basket_at_no_live() {
    let g = Group {
        legs: vec![
            leg(LegStatus::Other, None, None),
            leg(LegStatus::ClosedNo, None, None),
        ],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(
        c.path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketFewLegs,
            Node::YesBasketTried,
            Node::YesBasketNoLive
        ]
    );
    assert!(c.signal.is_none());
}

#[test]
fn a_priced_up_event_is_not_positive_on_both_baskets() {
    let g = Group {
        legs: vec![live("0.49", "0.51"), live("0.49", "0.51")],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(
        c.path,
        vec![
            Node::NoBasketTried,
            Node::NoBasketNotPositive,
            Node::YesBasketTried,
            Node::YesBasketNotPositive
        ]
    );
    assert!(c.signal.is_none());
}

#[test]
fn a_complete_yes_basket_is_named_complete() {
    let g = Group {
        legs: vec![live("0.20", "0.30"), live("0.20", "0.30")],
        augmented: false,
    };
    let c = classify(&g);
    assert_eq!(c.path.last(), Some(&Node::Complete));
    assert_eq!(c.signal.unwrap().kind, Kind::YesBasket);
}
