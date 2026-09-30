//! The §5.7 (c) decision tree as data (Phase 7 spec §3.3): the detectors' answer plus the path of
//! tests that led to it. It lives beside `detect`, not in it: `detect.rs` is the `detector_no` and
//! `detector_yes` premises' file, and an edit there restarts every card (premises.rs). So the
//! guards below re-read the same conditions `detect_no_basket`/`detect_yes_basket` check, and the
//! signal itself always comes from those detectors (a proptest pins the two together).
use crate::detect::{Group, Kind, LegStatus, Signal, detect_no_basket, detect_yes_basket};

/// One test on the parent §5.7 (c) decision tree, in the order `classify` evaluates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Decided,
    NoBasketTried,
    NoBasketFewLegs,
    NoBasketCrossed,
    NoBasketNotPositive,
    NoBasketPositive,
    YesBasketTried,
    YesBasketNoLive,
    YesBasketCrossed,
    YesBasketMissingAsk,
    YesBasketNotPositive,
    YesBasketPositive,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone)]
pub struct Classification {
    pub path: Vec<Node>,
    pub signal: Option<Signal>,
}

/// `detect`'s `decided`: an event with a leg already resolved YES yields nothing.
fn decided(g: &Group) -> bool {
    g.legs.iter().any(|l| l.status == LegStatus::ClosedYes)
}

/// The legs D2a buys: every live leg.
fn yes_live(g: &Group) -> Vec<usize> {
    (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live)
        .collect()
}

/// The legs D2b buys: every live leg with a YES bid.
fn no_used(g: &Group) -> Vec<usize> {
    (0..g.legs.len())
        .filter(|&i| g.legs[i].status == LegStatus::Live && g.legs[i].book.top().best_bid.is_some())
        .collect()
}

fn any_crossed(g: &Group, legs: &[usize]) -> bool {
    legs.iter().any(|&i| g.legs[i].book.is_crossed())
}

/// D2b first; when it is positive D2a is not tried (L7: they are never both positive).
/// `post_end` is the scanner's (from event dates), never part of the kind.
pub fn classify(g: &Group) -> Classification {
    let mut path = Vec::new();
    if decided(g) {
        path.push(Node::Decided);
        return Classification { path, signal: None };
    }
    path.push(Node::NoBasketTried);
    let used = no_used(g);
    if used.len() < 2 {
        path.push(Node::NoBasketFewLegs);
    } else if any_crossed(g, &used) {
        path.push(Node::NoBasketCrossed);
    } else if let Some(sig) = detect_no_basket(g) {
        path.push(Node::NoBasketPositive);
        return Classification {
            path,
            signal: Some(sig),
        };
    } else {
        path.push(Node::NoBasketNotPositive);
    }
    path.push(Node::YesBasketTried);
    let live = yes_live(g);
    if live.is_empty() {
        path.push(Node::YesBasketNoLive);
        return Classification { path, signal: None };
    }
    if any_crossed(g, &live) {
        path.push(Node::YesBasketCrossed);
        return Classification { path, signal: None };
    }
    if live
        .iter()
        .any(|&i| g.legs[i].book.top().best_ask.is_none())
    {
        path.push(Node::YesBasketMissingAsk);
        return Classification { path, signal: None };
    }
    match detect_yes_basket(g) {
        Some(sig) => {
            path.push(Node::YesBasketPositive);
            path.push(if sig.kind == Kind::YesBasket {
                Node::Complete
            } else {
                Node::Incomplete
            });
            Classification {
                path,
                signal: Some(sig),
            }
        }
        None => {
            path.push(Node::YesBasketNotPositive);
            Classification { path, signal: None }
        }
    }
}
