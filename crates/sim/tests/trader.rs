#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::{Book, Side};
use arb_core::detect::detect_no_basket;
use arb_core::fee::FeeSchedule;
use common::{ctx, d, no_group, p};
use rust_decimal::Decimal;
use sim::config::{AccountSpec, P1Params, PolicyKind, Scenario, SimConfig};
use sim::rows::SimOut;
use sim::trader::PaperTrader;
use sim::types::{BookSide, Outcome};
use std::collections::HashMap;

pub fn cfg(accounts: &[(PolicyKind, &str)]) -> SimConfig {
    SimConfig {
        settle_delay_ms: 2000,
        markout_offsets_ms: vec![1000],
        mark_every_ms: 60_000,
        burst_window_ms: 10_000,
        min_order_shares: d("5"),
        p1: P1Params {
            min_annualized: d("0.10"),
            max_days_to_end: d("30"),
        },
        scenarios: vec![Scenario {
            name: "t".into(),
            proc_ms: 0,
            net_ms: 0,
            match_ms: 100,
            honor_seconds_delay: true,
            burst: vec![],
        }],
        accounts: accounts
            .iter()
            .map(|(policy, bankroll)| AccountSpec {
                policy: *policy,
                scenario: "t".into(),
                bankroll: d(bankroll),
                per_trade: d("100"),
                fee_bips: Decimal::ZERO,
            })
            .collect(),
    }
}

fn books_of(g: &arb_core::detect::Group) -> HashMap<String, Book> {
    g.legs
        .iter()
        .enumerate()
        .map(|(i, l)| (format!("y{i}"), l.book.clone()))
        .collect()
}

const NO3: [Outcome; 3] = [Outcome::No, Outcome::No, Outcome::No];

fn fills(outs: &[SimOut]) -> Vec<(String, Decimal)> {
    outs.iter()
        .filter_map(|o| match o {
            SimOut::Fill(f) => Some((f.leg.market_id.clone(), f.filled)),
            _ => None,
        })
        .collect()
}

#[test]
fn no_basket_executes_at_t_x_then_converts() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    let outs = t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.is_none() && i.exec_ms == Some(1100))
    );
    assert!(fills(&t.on_frame(1050, &books)).is_empty(), "not due yet");
    let outs = t.on_frame(1100, &books);
    assert_eq!(
        fills(&outs),
        vec![
            ("m0".into(), d("51.2")),
            ("m1".into(), d("51.2")),
            ("m2".into(), d("51.2"))
        ]
    );
    let exec = outs
        .iter()
        .find_map(|o| {
            if let SimOut::Exec(e) = o {
                Some(e.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        (exec.q_eff, exec.leg_risk_qty, exec.hit),
        (d("51.2"), d("0"), true)
    );
    let a = &t.accounts()[0];
    assert_eq!(a.cash, d("900.16")); // 1000 − 51.2 × 1.95
    assert_eq!(a.reserved, d("0"), "reservation released at execution");

    let outs = t.on_frame(3100, &books); // settle delay 2000
    assert!(
        outs.iter()
            .any(|o| matches!(o, SimOut::Ledger(e) if e.kind == sim::account::EntryKind::Convert))
    );
    let a = &t.accounts()[0];
    assert_eq!(a.cash, d("1002.56")); // + 2 × 51.2
    assert_eq!(a.realized, d("2.56"));
    assert_eq!(
        a.returned.values().copied().collect::<Vec<_>>(),
        vec![d("51.2")]
    );
}

#[test]
fn a_second_episode_cannot_refill_consumed_liquidity() {
    let g = no_group(FeeSchedule::ZERO);
    let mut books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books);
    t.on_open(&ctx("e2", 1200, &g, detect_no_basket(&g).unwrap(), &NO3));
    let outs = t.on_frame(1300, &books);
    assert_eq!(
        fills(&outs),
        vec![
            ("m0".into(), d("48.8")),
            ("m1".into(), d("48.8")),
            ("m2".into(), d("48.8"))
        ]
    );
    // the feed then shows leg 0's YES bid shrink by ≥ what we took: it clears
    books.get_mut("y0").unwrap().apply_delta(
        Side::Bid,
        p("0.40"),
        arb_core::money::Shares::parse("0").unwrap(),
        1400,
    );
    t.on_level("y0", BookSide::Bid, d("0.40"), d("0"));
    books.get_mut("y0").unwrap().apply_delta(
        Side::Bid,
        p("0.40"),
        arb_core::money::Shares::parse("100").unwrap(),
        1401,
    );
    t.on_open(&ctx("e3", 1500, &g, detect_no_basket(&g).unwrap(), &NO3));
    let outs = t.on_frame(1600, &books);
    assert_eq!(fills(&outs)[0], ("m0".into(), d("51.2")));
}

#[test]
fn a_leg_that_thins_before_t_x_creates_leg_risk_which_is_unwound() {
    let g = no_group(FeeSchedule::ZERO);
    let mut books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    // between open and t_x, leg 2's YES bid (its NO ask 0.70) drops to 20
    books.get_mut("y2").unwrap().apply_delta(
        Side::Bid,
        p("0.30"),
        arb_core::money::Shares::parse("20").unwrap(),
        1050,
    );
    t.on_level("y2", BookSide::Bid, d("0.30"), d("20"));
    let outs = t.on_frame(1100, &books);
    assert_eq!(fills(&outs)[2], ("m2".into(), d("20")));
    let exec = outs
        .iter()
        .find_map(|o| {
            if let SimOut::Exec(e) = o {
                Some(e.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!((exec.q_eff, exec.leg_risk_qty), (d("20"), d("62.4")));
    // unwinds due at t_x + 100: sell 31.2 NO on legs 0 and 1 at NO bids 0.55 / 0.60
    let outs = t.on_frame(1200, &books);
    let sells: Vec<_> = outs
        .iter()
        .filter_map(|o| {
            if let SimOut::Fill(f) = o {
                Some((f.side, f.filled, f.vwap))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        sells,
        vec![
            ("sell", d("31.2"), Some(d("0.55"))),
            ("sell", d("31.2"), Some(d("0.60")))
        ]
    );
    t.on_frame(3100, &books); // conversion of the 20 complete sets
    let a = &t.accounts()[0];
    // unwinds: 31.2·(0.55 − 0.60) + 31.2·(0.60 − 0.65) = −3.12; conversion: 20·(2 − 1.95) = +1
    assert_eq!(a.realized, d("-2.12"));
    assert!(a.positions.is_empty());
}

#[test]
fn missing_book_at_execution_unwinds_the_other_legs() {
    let g = no_group(FeeSchedule::ZERO);
    let mut books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    books.remove("y2");
    let outs = t.on_frame(1100, &books);
    let exec = outs
        .iter()
        .find_map(|o| {
            if let SimOut::Exec(e) = o {
                Some(e.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        (exec.q_eff, exec.leg_risk_qty, exec.pnl_at_tx),
        (d("0"), d("102.4"), None)
    );
    let outs = t.on_frame(1200, &books);
    assert_eq!(
        outs.iter()
            .filter(|o| matches!(o, SimOut::Fill(f) if f.side == "sell"))
            .count(),
        2
    );
}

#[test]
fn accounts_do_not_share_consumed_liquidity() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[
        (PolicyKind::P0Converter, "1000"),
        (PolicyKind::P3Naive, "1000"),
    ]))
    .unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    let outs = t.on_frame(1100, &books);
    assert_eq!(
        fills(&outs).iter().filter(|(_, q)| *q == d("51.2")).count(),
        6,
        "both accounts fill fully"
    );
}

#[test]
fn pending_reservation_limits_the_next_intent() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "150")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    let outs = t.on_open(&ctx("e2", 1001, &g, detect_no_basket(&g).unwrap(), &NO3));
    // free cash 150 − 99.841 = 50.159 → 25.72 sets → multiple of 0.2 → 25.6
    assert!(matches!(&outs[..], [SimOut::Intent(i)] if i.q_target == Some(d("25.6"))));
    t.on_frame(1101, &books);
    assert_eq!(t.accounts()[0].reserved, d("0"));
}

#[test]
fn markouts_record_the_mid_after_each_offset() {
    let g = no_group(FeeSchedule::ZERO);
    let mut books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books);
    // leg 0 YES moves up: bid 0.50 ask 0.54 → NO mid 0.48; we bought NO at 0.60
    books.insert(
        "y0".into(),
        common::book(&[("0.50", "100")], &[("0.54", "100")]),
    );
    let outs = t.on_frame(2100, &books);
    let m = outs
        .iter()
        .find_map(|o| {
            if let SimOut::Markout(m) = o {
                (m.leg.market_id == "m0").then(|| m.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        (m.offset_ms, m.mid, m.markout),
        (1000, Some(d("0.48")), Some(d("-0.12")))
    );
}
