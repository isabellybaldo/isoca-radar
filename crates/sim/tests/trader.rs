#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::{Book, Side};
use arb_core::detect::{Group, GroupLeg, LegStatus, detect_no_basket, detect_yes_basket};
use arb_core::fee::FeeSchedule;
use common::{book, ctx, d, no_group, p};
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
    // Legs 1 and 2 are still fully consumed (100/100): a real bot would see those orders gone, so
    // this episode is skipped as "consumed" — not sized against the raw, unfiltered feed — and
    // produces no fills at all, not even a lopsided partial fill on leg 0.
    let outs = t.on_open(&ctx("e3", 1500, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.as_deref() == Some("consumed"))
    );
    let outs = t.on_frame(1600, &books);
    assert!(
        fills(&outs).is_empty(),
        "a real bot would see those orders gone"
    );
}

/// The consumed-liquidity check is per account (like `ConsumedLiquidity` itself, spec §5.6): one
/// account fully exhausting a book's liquidity must not stop a SEPARATE account — its own ledger,
/// never touched by that depletion — from taking the same episode on the same tokens/prices.
/// (Both a `P0-converter` and a `P3-naive` account take `NoBasket`, so a single multi-account
/// trader can't keep one of them "fresh" through e1/e2 while both remain eligible for e3 — hence
/// two separate traders here, each standing in for one account's own ledger.)
#[test]
fn consumed_liquidity_is_not_shared_across_accounts() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);

    // Account A: the same depleting sequence as `a_second_episode_cannot_refill_consumed_liquidity`
    // (51.2 + 48.8 = 100 on every leg), then e3 on the SAME tokens is skipped as "consumed".
    let mut a = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    a.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    a.on_frame(1100, &books);
    a.on_open(&ctx("e2", 1200, &g, detect_no_basket(&g).unwrap(), &NO3));
    a.on_frame(1300, &books);
    let outs = a.on_open(&ctx("e3", 1500, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(
        matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.as_deref() == Some("consumed"))
    );

    // Account B: its own, never-touched ledger on the exact same tokens/prices — e3 is its FIRST
    // episode, so it sees full depth and takes it in full.
    let mut b = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    let outs = b.on_open(&ctx("e3", 1500, &g, detect_no_basket(&g).unwrap(), &NO3));
    assert!(matches!(&outs[..], [SimOut::Intent(i)] if i.skip_reason.is_none()));
    let outs = b.on_frame(1600, &books);
    assert_eq!(
        fills(&outs),
        vec![
            ("m0".into(), d("51.2")),
            ("m1".into(), d("51.2")),
            ("m2".into(), d("51.2"))
        ]
    );
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

/// An unwind whose (venue-rounded) quantity falls below `min_order_shares` no longer sells
/// silently: it still emits a zero-fill row (filled 0, vwap None, fees 0) so the held, unsold
/// leg-risk inventory stays visible — and, being unfilled, produces no ledger entry.
#[test]
fn unwind_below_the_venue_minimum_is_held_not_sold_but_stays_visible() {
    let g = no_group(FeeSchedule::ZERO);
    let mut books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    // leg 2's YES bid (its NO ask 0.70) thins to 48, just 3.2 short of the 51.2 planned fill:
    // legs 0 and 1's leg-risk (51.2 − 48 = 3.2 each) is below the 5-share venue minimum.
    books.get_mut("y2").unwrap().apply_delta(
        Side::Bid,
        p("0.30"),
        arb_core::money::Shares::parse("48").unwrap(),
        1050,
    );
    t.on_level("y2", BookSide::Bid, d("0.30"), d("48"));
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
    assert_eq!((exec.q_eff, exec.leg_risk_qty), (d("48"), d("6.4"))); // 3.2 on each of legs 0, 1

    let outs = t.on_frame(1200, &books); // unwinds due at t_x + 100
    let sells: Vec<_> = outs
        .iter()
        .filter_map(|o| match o {
            SimOut::Fill(f) if f.side == "sell" => {
                Some((f.leg.market_id.clone(), f.requested, f.filled, f.vwap))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        sells,
        vec![
            ("m0".into(), d("3.2"), d("0"), None),
            ("m1".into(), d("3.2"), d("0"), None),
        ]
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, SimOut::Ledger(e) if e.kind == sim::account::EntryKind::Sell)),
        "no ledger entry for a held, unsold leg"
    );
    // still held: the position never left the account.
    let a = &t.accounts()[0];
    let leg0 = sim::types::LegKey {
        market_id: "m0".into(),
        yes_token: "y0".into(),
        outcome: Outcome::No,
    };
    assert_eq!(a.positions.get(&leg0).unwrap().qty, d("51.2"));
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
    // free cash 150 − 99.84103 = 50.15897 → 25.722549 sets → multiple of 0.2 → 25.6
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

/// A YES-basket whose leg 0 has both a bid (0.40, 30 shares) and an ask (0.45), and leg 1 an ask
/// only (0.50, thinned to 40 later). Buying YES on both legs is net positive: payoff 1 − (0.45 +
/// 0.50) = 0.05 > 0.
fn yes_group_for_ordering() -> Group {
    Group {
        legs: vec![
            GroupLeg {
                status: LegStatus::Live,
                book: book(&[("0.40", "30")], &[("0.45", "100")]),
                fee: FeeSchedule::ZERO,
            },
            GroupLeg {
                status: LegStatus::Live,
                book: book(&[], &[("0.50", "100")]),
                fee: FeeSchedule::ZERO,
            },
        ],
        augmented: false,
    }
}

/// A standalone NO-basket group used only to build episode 2's `OpenCtx` (never fed to
/// `books_of`): leg 0 shares leg 0's bid above (0.40, 30) so its sizing matches the real shared
/// book; leg 1's bid (0.85) only needs to make the pair net positive on its own — nothing in the
/// shared books ever reads it, since episode 2's leg 1 buy targets the mirrored "y1" book, which
/// has no bid at all and so simply fills 0 there, harmlessly.
fn no_group_for_ordering() -> Group {
    Group {
        legs: vec![
            GroupLeg {
                status: LegStatus::Live,
                book: book(&[("0.40", "30")], &[]),
                fee: FeeSchedule::ZERO,
            },
            GroupLeg {
                status: LegStatus::Live,
                book: book(&[("0.85", "100")], &[]),
                fee: FeeSchedule::ZERO,
            },
        ],
        augmented: false,
    }
}

/// Review finding (Task 8, round 1): `on_frame` used to drain its four queues one at a time, so an
/// earlier-due unwind and a later-due pending buy competed for the SAME book level in reverse time
/// order whenever both were due by the same (delayed) frame. This reproduces it directly:
///
/// - Episode 1 (YES-basket, `y0`+`y1`) buys YES on both legs; `y1`'s ask thins from 100 to 40
///   before t_x, so leg 0 fully fills at 100 while leg 1 only gets 40, leaving 60 shares of
///   leg-risk. The unwind that flattens it SELLS YES on leg 0, hitting its YES-BID level (0.40,
///   30 shares) — due at t_x + 100 = 1200.
/// - Episode 2 (NO-basket, same account) later buys NO on leg 0 too: buying NO also hits the SAME
///   YES-BID level (0.40, 30) via the L1 mirror. Its t_x is 1250.
/// - A single `on_frame(1300, ..)` call must run both. Due-time order says the unwind (1200) goes
///   first and takes the level's only 30 shares; episode 2's buy then finds nothing left. The old
///   queue-by-queue order ran the pending buy first instead (wrongly, since 1250 > 1200), letting
///   it grab the 30 shares and leaving the earlier unwind with 0 — the reverse of what should
///   happen.
#[test]
fn an_earlier_due_unwind_wins_the_book_level_over_a_later_due_buy() {
    let g1 = yes_group_for_ordering();
    let g2 = no_group_for_ordering();
    let mut books = books_of(&g1);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P3Naive, "1000")])).unwrap();
    let yes2 = [Outcome::Yes, Outcome::Yes];
    let no2 = [Outcome::No, Outcome::No];

    t.on_open(&ctx(
        "e1",
        1000,
        &g1,
        detect_yes_basket(&g1).unwrap(),
        &yes2,
    ));
    // y1's ask thins from 100 to 40 before t_x (1100): leg 0 fills 100, leg 1 only 40.
    books.get_mut("y1").unwrap().apply_delta(
        Side::Ask,
        p("0.50"),
        arb_core::money::Shares::parse("40").unwrap(),
        1050,
    );
    t.on_level("y1", BookSide::Ask, d("0.50"), d("40"));
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
    // At open, both legs' books are still full (100 each): unit cost 0.95, so q_open(100)'s own
    // walked cost (95) already fits the $100 budget — q_target settles at exactly 100 (100 is
    // itself grid-valid: 0.45 × 100 and 0.50 × 100 both have ≤ 2 dp). Leg 1 then only fills 40
    // (thinned before t_x), so leg-risk on leg 0 is 100 − 40 = 60 — still far past the unwind
    // level's 30-share cap, which is all this test needs.
    assert_eq!((exec.q_eff, exec.leg_risk_qty), (d("40"), d("60")));

    // Episode 2 opens after episode 1's execution but before its unwind is due; its own t_x (1250)
    // is later than the unwind's due time (1200), so both are due by on_frame(1300, ..).
    t.on_open(&ctx("e2", 1150, &g2, detect_no_basket(&g2).unwrap(), &no2));
    let outs = t.on_frame(1300, &books);

    let sell_m0 = outs
        .iter()
        .find_map(|o| match o {
            SimOut::Fill(f) if f.side == "sell" && f.leg.market_id == "m0" => Some(f.filled),
            _ => None,
        })
        .unwrap();
    let buy_m0_e2 = outs
        .iter()
        .find_map(|o| match o {
            SimOut::Fill(f)
                if f.side == "buy" && f.leg.market_id == "m0" && f.episode_id == "e2" =>
            {
                Some(f.filled)
            }
            _ => None,
        })
        .unwrap();
    // The earlier-due unwind takes the level's only 30 shares fully; the later-due buy gets 0.
    assert_eq!(sell_m0, d("30"));
    assert_eq!(buy_m0_e2, d("0"));
}

/// Review finding (Task 8, round 1): the minute mark used to run AFTER the whole frame's due work,
/// so a mark stamped at boundary `at` could include cash changes from executions due strictly
/// after `at` but still within the same (delayed) call. This reproduces it: episode e1's t_x
/// (60_050) falls just after the 60_000 mark boundary, and both are due in one `on_frame(60_050,
/// ..)` call. The mark at 60_000 must see the PRE-execution cash; the execution still happens
/// within the same call, just correctly ordered after the mark.
#[test]
fn a_mark_at_its_boundary_does_not_see_a_later_due_execution_in_the_same_frame() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[(PolicyKind::P0Converter, "1000")])).unwrap();
    // Establishes next_mark_ms = 60_000 without firing anything (first-frame initialisation).
    t.on_frame(100, &books);
    t.on_open(&ctx("e1", 59_950, &g, detect_no_basket(&g).unwrap(), &NO3));
    let outs = t.on_frame(60_050, &books);

    let mark = outs
        .iter()
        .find_map(|o| match o {
            SimOut::Account(a) if a.t_ms == 60_000 => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(mark.cash, d("1000"), "the mark predates e1's execution");
    assert!(
        outs.iter()
            .any(|o| matches!(o, SimOut::Fill(f) if f.side == "buy")),
        "the execution still runs within the same call, just after the mark"
    );
    // Same fill as `no_basket_executes_at_t_x_then_converts`: 51.2 shares at 1.95 total.
    assert_eq!(t.accounts()[0].cash, d("900.16"));
}
