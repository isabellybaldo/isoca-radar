#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::{Book, Side};
use arb_core::stats::{annualized, days_to_end};
use common::*;
use pm_api::ws::MarketEvent;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::engine::{Engine, Out};
use scanner::universe::Universe;
use sim::account::EntryKind;
use sim::config::{AccountSpec, P1Params, PolicyKind, Scenario, SimConfig};
use sim::rows::SimOut;
use sim::trader::PaperTrader;
use std::collections::HashMap;
use std::sync::Arc;

/// One account; t_x = r_open + 100 ms; settle delay 2 s; one markout at +1 s.
fn sim_cfg(policy: PolicyKind) -> SimConfig {
    SimConfig {
        settle_delay_ms: 2000,
        markout_offsets_ms: vec![1000],
        mark_every_ms: 60_000,
        burst_window_ms: 10_000,
        min_order_shares: dec!(5),
        p1: P1Params {
            min_annualized: dec!(0.10),
            max_days_to_end: dec!(30),
        },
        scenarios: vec![Scenario {
            name: "t".into(),
            proc_ms: 0,
            net_ms: 0,
            match_ms: 100,
            honor_seconds_delay: true,
            burst: vec![],
        }],
        accounts: vec![AccountSpec {
            policy,
            scenario: "t".into(),
            bankroll: dec!(1000),
            per_trade: dec!(100),
            fee_bips: Decimal::ZERO,
        }],
    }
}

fn paper_engine(policy: PolicyKind) -> Engine {
    let mut e = engine();
    e.set_paper(PaperTrader::new(sim_cfg(policy)).unwrap());
    e
}

fn sims(outs: &[Out]) -> Vec<SimOut> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Paper(s) => Some((**s).clone()),
            _ => None,
        })
        .collect()
}

fn fills(outs: &[Out]) -> Vec<(String, Decimal)> {
    sims(outs)
        .into_iter()
        .filter_map(|s| match s {
            SimOut::Fill(f) => Some((f.leg.market_id, f.filled)),
            _ => None,
        })
        .collect()
}

fn all3(a: Decimal, b: Decimal, c: Decimal) -> Vec<(String, Decimal)> {
    vec![
        ("e1-m0".to_string(), a),
        ("e1-m1".to_string(), b),
        ("e1-m2".to_string(), c),
    ]
}

/// YES books: bids `bids` (size 100), asks 0.45 / 0.40 / 0.35 (ΣA = 1.20: never a YES basket).
fn books(e: &mut Engine, recv_ms: i64, ts: i64, bids: [&str; 3]) -> Vec<Out> {
    let asks = ["0.45", "0.40", "0.35"];
    let evs: Vec<MarketEvent> = (0..3)
        .map(|i| {
            snap(
                &format!("e1-y{i}"),
                ts,
                &[(bids[i], "100")],
                &[(asks[i], "100")],
            )
        })
        .collect();
    e.on_frame(0, recv_ms, &evs)
}

/// A frame that changes no book: only time passes.
fn quiet(e: &mut Engine, recv_ms: i64) -> Vec<Out> {
    e.on_frame(
        0,
        recv_ms,
        &[MarketEvent::LastTrade {
            token: "e1-y0".into(),
            server_ts_ms: recv_ms,
        }],
    )
}

fn bid(token: &str, ts: i64, price: &str, size: &str) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side: Side::Bid,
        price: p(price),
        size: sh(size),
        best_bid: None,
        best_ask: None,
    }
}

/// ΣNO = 3 × 0.70 = 2.10 first (nothing), then NO asks 0.60/0.65/0.70 (Σ 1.95 < 2): a NO basket
/// opens at recv 2 100, so the account's execution is due at 2 200 (budget 100 / 1.95 → 51.2).
fn open_no_basket(e: &mut Engine) -> Vec<Out> {
    books(e, 1_100, 1_000, ["0.30", "0.30", "0.30"]);
    books(e, 2_100, 2_000, ["0.40", "0.35", "0.30"])
}

#[test]
fn a_no_basket_open_reaches_the_paper_trader_and_fills_at_t_x() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    let outs = open_no_basket(&mut e);
    let eps = episodes(&outs);
    assert_eq!((eps.len(), eps[0].kind.as_str()), (1, "no_basket"));
    let intents: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| {
            if let SimOut::Intent(i) = s {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(intents.len(), 1);
    assert_eq!(
        (intents[0].account.as_str(), intents[0].episode_id.as_str()),
        ("P0-converter/t", "live-7-00000001")
    );
    assert_eq!(
        (intents[0].exec_ms, intents[0].q_target),
        (Some(2_200), Some(dec!(51.2)))
    );
    assert!(fills(&quiet(&mut e, 2_150)).is_empty(), "not due yet");
    assert_eq!(
        fills(&quiet(&mut e, 2_300)),
        all3(dec!(51.2), dec!(51.2), dec!(51.2))
    );
    assert_eq!(e.paper_held_markets(), vec!["e1-m0", "e1-m1", "e1-m2"]);
}

#[test]
fn due_work_sees_the_book_before_the_frame_that_changes_it() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e); // execution due at recv 2 200
    // The frame received at 2 200 thins leg 2's YES bid (its NO ask 0.70) to 20. The execution
    // due at 2 200 must see the book as it stood BEFORE this frame (S3b call contract).
    let outs = e.on_frame(0, 2_200, &[bid("e1-y2", 2_150, "0.30", "20")]);
    assert_eq!(fills(&outs), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
}

#[test]
fn an_emptied_level_clears_consumed_liquidity() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    assert_eq!(
        fills(&quiet(&mut e, 2_300)),
        all3(dec!(51.2), dec!(51.2), dec!(51.2))
    );
    // leg 0's YES bid 0.40 empties (the basket closes) and comes back with 60 (episode 2 opens)
    let outs = e.on_frame(0, 2_400, &[bid("e1-y0", 2_400, "0.40", "0")]);
    assert!(episodes(&outs).iter().any(|r| r.closed_server_ms.is_some()));
    let outs = e.on_frame(0, 2_500, &[bid("e1-y0", 2_500, "0.40", "60")]);
    assert!(
        episodes(&outs)
            .iter()
            .any(|r| r.episode_id == "live-7-00000002")
    );
    // on_level cleared leg 0's consumed 51.2, so it offers all 60; legs 1 and 2 still carry ours
    // (100 − 51.2 = 48.8), which caps the second basket at 48.8. Had the removal not reached
    // on_level, leg 0 would offer only 60 − 51.2 = 8.8 (a re-added 60 is no shrink below 48.8)
    // and every leg would fill 8.8.
    assert_eq!(
        fills(&quiet(&mut e, 2_700)),
        all3(dec!(48.8), dec!(48.8), dec!(48.8))
    );
}

/// A YES-bid delta whose message reports the server's final best bid (S3a deferred top-check).
fn bid_top(token: &str, ts: i64, price: &str, size: &str, best_bid: &str) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side: Side::Bid,
        price: p(price),
        size: sh(size),
        best_bid: Some(p(best_bid)),
        best_ask: None,
    }
}

#[test]
fn a_trimmed_phantom_level_clears_consumed_liquidity() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    assert_eq!(
        fills(&quiet(&mut e, 2_300)),
        all3(dec!(51.2), dec!(51.2), dec!(51.2))
    );
    // The server says leg 0's best bid is 0.30: our 0.40 (the level we consumed) is a phantom,
    // a removal we never received. The check is deferred until the update is complete...
    let outs = e.on_frame(0, 2_400, &[bid_top("e1-y0", 2_400, "0.30", "100", "0.30")]);
    assert!(
        !outs.iter().any(|o| matches!(o, Out::Integrity(_))),
        "deferred, not yet checked"
    );
    // ...which the next update on the token proves: S3a trims 0.40 (the basket closes), then
    // this frame's delta re-adds 0.40 with 60 (episode 2 opens at recv 2 500).
    let outs = e.on_frame(0, 2_500, &[bid_top("e1-y0", 2_500, "0.40", "60", "0.40")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    assert!(episodes(&outs).iter().any(|r| r.closed_server_ms.is_some()));
    assert!(
        episodes(&outs)
            .iter()
            .any(|r| r.episode_id == "live-7-00000002")
    );
    // The trim reached on_level as a removal: leg 0's consumed 51.2 cleared, exactly as for a
    // size-0 delta, and legs 1 and 2 (48.8 left each) cap the basket. Without the trim report,
    // leg 0 would offer only 60 − 51.2 = 8.8 and every leg would fill 8.8.
    assert_eq!(
        fills(&quiet(&mut e, 2_700)),
        all3(dec!(48.8), dec!(48.8), dec!(48.8))
    );
}

#[test]
fn an_episode_opened_by_a_rest_snapshot_executes_after_the_latest_frame() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    quiet(&mut e, 5_000); // the engine has now seen a frame received at 5 000
    // A REST snapshot (server ts 1 500, accepted: newer than our 1 000) lifts leg 0's YES bid to
    // 0.50: NO asks 0.50 / 0.70 / 0.70, Σ 1.90 < 2, so a NO basket opens from rest_snapshot.
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_500);
    let outs = e.rest_snapshot("e1-y0", b);
    assert_eq!(episodes(&outs).len(), 1);
    let intents: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| {
            if let SimOut::Intent(i) = s {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(intents.len(), 1);
    // rest_snapshot evaluates with recv = the REST book's server ts (1 500); the trader's clock
    // is never behind the latest frame the engine received (5 000), plus match_ms 100.
    assert_eq!(intents[0].exec_ms, Some(5_100));
}

/// P3 opens a YES basket (asks 0.30/0.35/0.30, size 10): execution due at 2 250, not yet run.
fn yes_basket_open() -> Engine {
    let mut e = paper_engine(PolicyKind::P3Naive);
    seed(&mut e);
    e.on_frame(
        0,
        2_150,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    e
}

/// ...and holds it to resolution.
fn yes_basket_held() -> Engine {
    let mut e = yes_basket_open();
    assert_eq!(
        fills(&quiet(&mut e, 2_300)),
        all3(dec!(10), dec!(10), dec!(10))
    );
    e
}

fn settles(outs: &[Out]) -> Vec<(Decimal, Option<Decimal>, Decimal)> {
    sims(outs)
        .into_iter()
        .filter_map(|s| match s {
            SimOut::Ledger(l) if l.kind == EntryKind::Settle => {
                Some((l.qty, l.price, l.cash_delta))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn resolutions_settle_at_the_payout_vector_and_flag_non_binary_ones() {
    let mut e = yes_basket_held();
    assert_eq!(e.paper_held_markets(), vec!["e1-m0", "e1-m1", "e1-m2"]);
    let outs = e.on_resolution(9_000, "e1-m0", dec!(0.5), dec!(0.5));
    assert!(outs.iter().any(|o| matches!(o, Out::Integrity(i)
        if i.kind == "non_binary_payout" && i.event_id.as_deref() == Some("e1") && i.ts_ms == 9_000)));
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(0.5)), dec!(5))]);
    let outs = e.on_resolution(9_001, "e1-m1", dec!(1), dec!(0));
    assert!(
        !outs.iter().any(|o| matches!(o, Out::Integrity(_))),
        "a binary payout is not flagged"
    );
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(1)), dec!(10))]);
    assert_eq!(e.paper_held_markets(), vec!["e1-m2"]);
}

#[test]
fn a_repeated_resolution_settles_once() {
    let mut e = yes_basket_held();
    assert_eq!(
        settles(&e.on_resolution(9_000, "e1-m1", dec!(1), dec!(0))).len(),
        1
    );
    // the ws hint and the periodic poll both found it
    let again = e.on_resolution(9_500, "e1-m1", dec!(1), dec!(0));
    assert!(
        sims(&again).iter().all(|s| !matches!(s, SimOut::Ledger(_))),
        "{again:?}"
    );
}

#[test]
fn a_resolution_first_runs_the_work_due_before_it() {
    // No frame arrives between the open and the resolution: the execution due at 2 250 must
    // still run (on_frame(2 260) first), so the settlement includes the shares it bought.
    let mut e = yes_basket_open();
    let outs = e.on_resolution(2_260, "e1-m0", dec!(1), dec!(0));
    assert_eq!(fills(&outs), all3(dec!(10), dec!(10), dec!(10)));
    assert_eq!(settles(&outs), vec![(dec!(10), Some(dec!(1)), dec!(10))]);
}

#[test]
fn lockup_figures_use_the_latest_live_end_date() {
    let mut m = meta("e1", 3);
    m.legs[0].end_date = Some("1970-01-06T00:00:00Z".into()); // 5 days
    m.legs[1].end_date = Some("1970-01-11T00:00:00Z".into()); // 10 days: the max
    let mut e = Engine::new(ctx());
    let conn: HashMap<String, usize> = [("e1".to_string(), 0)].into_iter().collect();
    let _ = e.apply_universe(
        Arc::new(Universe {
            taken_ms: 0,
            groups: vec![m],
            canary: vec![],
        }),
        &conn,
        0,
    );
    e.set_paper(PaperTrader::new(sim_cfg(PolicyKind::P0Converter)).unwrap());
    let outs = open_no_basket(&mut e);
    let r = &episodes(&outs)[0];
    let days = days_to_end(864_000_000, 2_000); // relative to the opening server ts
    assert_eq!(r.days_to_end, Some(days));
    assert_eq!(
        r.annualized_return,
        annualized(r.peak_pnl_net_usd / r.peak_cost_usd, days)
    );
    assert_eq!(r.post_end, Some(false));
    // no end dates: unmeasured (NULL), never 0
    let mut plain = paper_engine(PolicyKind::P0Converter);
    let r = &episodes(&open_no_basket(&mut plain))[0];
    assert_eq!(
        (r.days_to_end, r.annualized_return, r.post_end),
        (None, None, None)
    );
}

#[test]
fn close_completes_conversions_marks_every_account_and_exposes_state() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    quiet(&mut e, 2_300); // executed; the conversion is due at 4 200
    let outs = e.paper_close(3_000);
    assert!(sims(&outs).iter().any(
        |s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Convert && l.t_ms == 3_000)
    ));
    let marks: Vec<_> = sims(&outs)
        .into_iter()
        .filter_map(|s| {
            if let SimOut::Account(a) = s {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(marks.len(), 1);
    assert_eq!(
        marks[0].equity,
        dec!(1000) + marks[0].realized + marks[0].unrealized,
        "equity identity"
    );
    assert_eq!(e.paper_state().unwrap().accounts.len(), 1);

    let mut bare = engine();
    assert!(bare.paper_state().is_none());
    assert!(bare.paper_close(1).is_empty());
    assert!(bare.paper_held_markets().is_empty());
}

#[test]
fn close_first_runs_the_work_due_before_the_close_instant() {
    // The stop comes at 2 250 with no frame since the open: the execution due at 2 200 runs at
    // its own time first (S3b: close follows on_frame), then close completes its conversion.
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e);
    let outs = e.paper_close(2_250);
    assert_eq!(fills(&outs), all3(dec!(51.2), dec!(51.2), dec!(51.2)));
    assert!(sims(&outs).iter().any(
        |s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Convert && l.t_ms == 2_250)
    ));
}

#[test]
fn a_ws_market_resolved_asks_for_a_resolver_poll() {
    let mut e = engine();
    let outs = e.on_frame(
        0,
        100,
        &[MarketEvent::Resolved {
            market: Some("0xabc".into()),
            server_ts_ms: Some(90),
        }],
    );
    assert!(outs.contains(&Out::ResolvedHint) && outs.contains(&Out::UniverseChanged));
}

#[test]
fn the_paper_trader_never_changes_the_scanners_own_outputs() {
    // The S3a-2 synthetic high-rate script: YES and NO baskets open and close, 1 in 50 deltas
    // carries a wrong server top (trims), conn 0 drops every 4 000 frames. With all 15 repo
    // accounts attached, every non-paper output must be byte-identical to the bare engine's.
    let mut s = Synth::new(200, 42);
    let frames = s.script(20_000, 50);
    let policies: SimConfig =
        toml::from_str(include_str!("../../../config/policies.toml")).unwrap();
    let run = |paper: bool| {
        let mut e = synth_engine(&s, true);
        if paper {
            e.set_paper(PaperTrader::new(policies.clone()).unwrap());
        }
        let mut outs: Vec<Out> = Vec::new();
        drive(&mut e, &frames, 4_000, &mut |o| outs.push(o));
        outs
    };
    let (bare, with) = (run(false), run(true));
    let scanner_only: Vec<String> = with
        .iter()
        .filter(|o| !matches!(o, Out::Paper(_)))
        .map(|o| format!("{o:?}"))
        .collect();
    let bare_txt: Vec<String> = bare.iter().map(|o| format!("{o:?}")).collect();
    assert_eq!(scanner_only.len(), bare_txt.len(), "output counts differ");
    for (i, (a, b)) in scanner_only.iter().zip(&bare_txt).enumerate() {
        assert_eq!(a, b, "scanner output {i} differs with the trader attached");
    }
    // the fixture exercises the trader: every open reaches the P3 accounts (they take every kind)
    assert!(
        with.iter()
            .any(|o| matches!(o, Out::Paper(s) if matches!(**s, SimOut::Intent(_)))),
        "no paper intent: the trader never saw an open"
    );
}

/// Every instant a paper row carries (spec 7a §4.2: none may fall before the epoch start).
fn row_times(s: &SimOut) -> Vec<i64> {
    match s {
        SimOut::Intent(i) => std::iter::once(i.decided_ms).chain(i.exec_ms).collect(),
        SimOut::Exec(x) => vec![x.exec_ms],
        SimOut::Fill(f) => vec![f.t_ms],
        SimOut::Ledger(l) => vec![l.t_ms],
        SimOut::Markout(_) => vec![],
        SimOut::Account(a) => vec![a.t_ms],
    }
}

fn zero_ms_engine() -> Engine {
    let mut c = sim_cfg(PolicyKind::P0Converter);
    c.scenarios[0].match_ms = 0;
    let mut e = engine();
    e.set_paper(PaperTrader::new(c).unwrap());
    e
}

#[test]
fn a_reset_closes_at_the_paper_clock_then_marks_every_account_at_its_bankroll() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    open_no_basket(&mut e); // execution due at 2 200
    quiet(&mut e, 2_300); // executed; its conversion is due at 4 200
    let (start, rows) = e.paper_reset(2_250).unwrap(); // an older t never moves the clock back
    assert_eq!((start, e.paper_clock()), (2_301, 2_301));
    let led: Vec<(EntryKind, i64)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Ledger(l) => Some((l.kind, l.t_ms)),
            _ => None,
        })
        .collect();
    assert_eq!(
        led,
        vec![
            (EntryKind::Convert, 2_300),
            (EntryKind::Mark, 2_300),
            (EntryKind::Mark, 2_301)
        ]
    );
    let marks: Vec<(i64, Decimal)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Account(a) => Some((a.t_ms, a.equity)),
            _ => None,
        })
        .collect();
    assert_eq!(marks.last(), Some(&(2_301, dec!(1000))));
    assert!(
        e.paper_state()
            .unwrap()
            .accounts
            .iter()
            .all(|a| a.cash == dec!(1000) && a.positions.is_empty())
    );
    let mut bare = engine();
    assert!(bare.paper_reset(1).is_none(), "no trader: nothing to reset");
}

#[test]
fn no_new_epoch_row_is_stamped_before_the_epoch_start() {
    let mut e = zero_ms_engine();
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    let (start, _) = e.paper_reset(1_500).unwrap();
    assert_eq!(start, 1_501);
    // frames received in the reset's own millisecond and before it (review C2): the NO basket
    // opens and the 0 ms account executes right away
    let mut after = books(&mut e, 1_500, 1_450, ["0.40", "0.35", "0.30"]);
    after.extend(quiet(&mut e, 1_500));
    after.extend(quiet(&mut e, 1_499));
    let rows = sims(&after);
    assert!(
        rows.iter()
            .any(|s| matches!(s, SimOut::Exec(x) if x.exec_ms == 1_501)),
        "{rows:?}"
    );
    for s in &rows {
        assert!(row_times(s).iter().all(|&t| t >= start), "{s:?}");
    }
}

#[test]
fn a_rest_snapshot_open_a_resolution_and_a_close_after_a_reset_are_stamped_at_or_after_its_start() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]); // ΣNO 2.10: nothing
    let (start, _) = e.paper_reset(1_200).unwrap();
    assert_eq!(start, 1_201);
    // a REST snapshot stamped with an older server time lifts leg 0's YES bid: NO asks
    // 0.50 / 0.70 / 0.70, Σ 1.90 < 2
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_150);
    let mut outs = e.rest_snapshot("e1-y0", b);
    outs.extend(quiet(&mut e, 1_400)); // executes at 1 301
    outs.extend(e.on_resolution(1_000, "e1-m0", dec!(0), dec!(1))); // an older instant
    outs.extend(e.paper_close(900)); // an older instant too
    let rows = sims(&outs);
    assert!(
        rows.iter().any(
            |s| matches!(s, SimOut::Intent(i) if i.decided_ms == 1_201 && i.exec_ms == Some(1_301))
        ),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(
            |s| matches!(s, SimOut::Ledger(l) if l.kind == EntryKind::Settle && l.t_ms == 1_400)
        ),
        "{rows:?}"
    );
    for s in &rows {
        assert!(row_times(s).iter().all(|&t| t >= start), "{s:?}");
    }
}

#[test]
fn a_seeded_paper_clock_stamps_the_first_rows_of_a_start_time_epoch() {
    let mut e = paper_engine(PolicyKind::P0Converter);
    e.seed_paper_clock(5_000); // review C3: never the i64::MIN clock
    assert_eq!(e.paper_clock(), 5_000);
    books(&mut e, 1_100, 1_000, ["0.30", "0.30", "0.30"]);
    let mut b = Book::default();
    b.apply_snapshot(&[(p("0.50"), sh("100"))], &[(p("0.55"), sh("100"))], 1_500);
    let rows = sims(&e.rest_snapshot("e1-y0", b));
    assert!(
        rows.iter().any(
            |s| matches!(s, SimOut::Intent(i) if i.decided_ms == 5_000 && i.exec_ms == Some(5_100))
        ),
        "{rows:?}"
    );
    e.seed_paper_clock(10);
    assert_eq!(e.paper_clock(), 5_000, "seeding never moves the clock back");
}

/// Every paper row of `outs` as (variant, the instants it carries): what the entry-point clamp
/// could change.
fn stamps(outs: &[Out]) -> Vec<(&'static str, Vec<i64>)> {
    sims(outs)
        .iter()
        .map(|s| {
            let v = match s {
                SimOut::Intent(_) => "intent",
                SimOut::Exec(_) => "exec",
                SimOut::Fill(_) => "fill",
                SimOut::Ledger(_) => "ledger",
                SimOut::Markout(_) => "markout",
                SimOut::Account(_) => "account",
            };
            (v, row_times(s))
        })
        .collect()
}

#[test]
fn the_entry_point_clamp_changes_nothing_without_a_reset() {
    // D11: with non-decreasing input times (every live and replay input outside a reset,
    // including ties), max(t, paper clock) == t at every entry point, so the rows are exactly
    // the ones the unclamped engine produced (pinned below from the pre-clamp code).
    let mut e = paper_engine(PolicyKind::P0Converter);
    let mut outs = open_no_basket(&mut e); // opens at 2 100, execution due at 2 200
    outs.extend(quiet(&mut e, 2_300)); // executes at 2 200; conversion due at 4 200
    outs.extend(e.on_resolution(2_300, "e1-m1", dec!(0), dec!(1))); // the frame's own ms
    outs.extend(quiet(&mut e, 5_000)); // the conversion completes at 4 200
    outs.extend(books(&mut e, 61_000, 60_900, ["0.30", "0.30", "0.30"])); // the minute mark
    outs.extend(e.paper_close(61_000)); // the frame's own ms again
    let intent = ("intent", vec![2_100, 2_200]);
    let buy = [("fill", vec![2_200]), ("ledger", vec![2_200])];
    let mut want = vec![intent];
    for _ in 0..3 {
        want.extend(buy.clone());
    }
    want.extend([
        ("exec", vec![2_200]),
        ("ledger", vec![2_300]),
        ("account", vec![2_300]),
        ("markout", vec![]),
        ("markout", vec![]),
        ("markout", vec![]),
        ("ledger", vec![60_000]),
        ("account", vec![60_000]),
        ("ledger", vec![61_000]),
        ("account", vec![61_000]),
    ]);
    assert_eq!(stamps(&outs), want);
    let rows = sims(&outs);
    let led: Vec<(EntryKind, i64, Decimal)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Ledger(l) => Some((l.kind, l.t_ms, l.cash_delta)),
            _ => None,
        })
        .collect();
    assert_eq!(
        led,
        vec![
            (EntryKind::Buy, 2_200, dec!(-30.72)),
            (EntryKind::Buy, 2_200, dec!(-33.28)),
            (EntryKind::Buy, 2_200, dec!(-35.84)),
            (EntryKind::Settle, 2_300, dec!(51.2)),
            (EntryKind::Mark, 60_000, dec!(0)),
            (EntryKind::Mark, 61_000, dec!(0)),
        ]
    );
    let eq: Vec<(i64, Decimal)> = rows
        .iter()
        .filter_map(|s| match s {
            SimOut::Account(a) => Some((a.t_ms, a.equity)),
            _ => None,
        })
        .collect();
    assert_eq!(
        eq,
        vec![
            (2_300, dec!(1012.8)),
            (60_000, dec!(1012.8)),
            (61_000, dec!(1012.8))
        ]
    );
    assert_eq!(e.paper_clock(), 61_000);
}
