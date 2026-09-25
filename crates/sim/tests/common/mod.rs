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

use arb_core::detect::{Group, GroupLeg, LegStatus};
use sim::policy::{LegInfo, OpenCtx};
use sim::types::Outcome;
use sim::venue::bought_levels;

/// A live leg whose YES book has one bid and one ask level of size 100.
pub fn live(bid: Option<&str>, ask: Option<&str>, f: FeeSchedule) -> GroupLeg {
    let bids: Vec<(&str, &str)> = bid.map(|b| vec![(b, "100")]).unwrap_or_default();
    let asks: Vec<(&str, &str)> = ask.map(|a| vec![(a, "100")]).unwrap_or_default();
    GroupLeg {
        status: LegStatus::Live,
        book: book(&bids, &asks),
        fee: f,
    }
}

pub fn info(i: usize, f: FeeSchedule) -> LegInfo {
    LegInfo {
        market_id: format!("m{i}"),
        yes_token: format!("y{i}"),
        fee: f,
        seconds_delay: None,
    }
}

/// Builds the open context the scanner will build (S3c): levels of the bought outcome per used leg.
pub fn ctx(
    ep: &str,
    r_open_ms: i64,
    g: &Group,
    sig: arb_core::detect::Signal,
    outcomes: &[Outcome],
) -> OpenCtx {
    let open_levels = sig
        .legs_used
        .iter()
        .zip(outcomes)
        .map(|(&i, &o)| bought_levels(&g.legs[i].book, o))
        .collect();
    let legs = g
        .legs
        .iter()
        .enumerate()
        .map(|(i, l)| info(i, l.fee))
        .collect();
    OpenCtx {
        episode_id: ep.into(),
        event_id: "ev".into(),
        r_open_ms,
        signal: sig,
        legs,
        open_levels,
        days_to_end: None,
        annualized: None,
        pair_approved: false,
    }
}

/// NO basket: YES bids 0.40/0.35/0.30 (NO asks 0.60/0.65/0.70, Σ 1.95 < 2), YES asks 0.45/0.40/0.35,
/// plus a 4th live leg with no bid (not in S: its YES comes back on conversion).
pub fn no_group(f: FeeSchedule) -> Group {
    Group {
        legs: vec![
            live(Some("0.40"), Some("0.45"), f),
            live(Some("0.35"), Some("0.40"), f),
            live(Some("0.30"), Some("0.35"), f),
            live(None, Some("0.05"), f),
        ],
        augmented: false,
    }
}
