#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::Book;
use arb_core::detect::detect_no_basket;
use arb_core::fee::FeeSchedule;
use common::{ctx, d, no_group};
use rust_decimal::Decimal;
use sim::config::{AccountSpec, P1Params, PolicyKind, Scenario, SimConfig};
use sim::rows::SimOut;
use sim::trader::{PaperTrader, TraderState};
use sim::types::Outcome;
use std::collections::HashMap;

fn cfg(accounts: &[PolicyKind]) -> SimConfig {
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
            .map(|p| AccountSpec {
                policy: *p,
                scenario: "t".into(),
                bankroll: d("1000"),
                per_trade: d("100"),
                fee_bips: Decimal::ZERO,
            })
            .collect(),
    }
}

const NO3: [Outcome; 3] = [Outcome::No, Outcome::No, Outcome::No];

fn books_of(g: &arb_core::detect::Group) -> HashMap<String, Book> {
    g.legs
        .iter()
        .enumerate()
        .map(|(i, l)| (format!("y{i}"), l.book.clone()))
        .collect()
}

/// open → execute → convert; returns the trader holding 51.2 returned YES of m3.
fn converted() -> (PaperTrader, HashMap<String, Book>) {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[PolicyKind::P0Converter])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books);
    t.on_frame(3100, &books);
    (t, books)
}

#[test]
fn resolution_settles_every_account() {
    let (mut t, _) = converted();
    let outs = t.on_resolution(9_000, "m3", d("1"), d("0"));
    assert!(outs.iter().any(|o| matches!(o, SimOut::Ledger(e) if e.kind == sim::account::EntryKind::Settle && e.cash_delta == d("51.2"))));
    assert!(outs.iter().any(|o| matches!(o, SimOut::Account(_))));
    assert_eq!(t.accounts()[0].realized, d("53.76")); // 2.56 conversion + 51.2 windfall
    assert!(
        t.on_resolution(9_001, "unknown", d("1"), d("0"))
            .iter()
            .all(|o| !matches!(o, SimOut::Ledger(_)))
    );
    // Q19: a 50-50 vector on a fresh holding settles at 0.5
    let (mut t, _) = converted();
    t.on_resolution(9_000, "m3", d("0.5"), d("0.5"));
    assert_eq!(t.accounts()[0].realized, d("28.16")); // 2.56 + 25.6
}

#[test]
fn marks_are_emitted_on_minute_boundaries_of_input_time() {
    let (mut t, books) = converted(); // first frame at 1100 → next mark at 60 000
    assert!(
        t.on_frame(59_999, &books)
            .iter()
            .all(|o| !matches!(o, SimOut::Account(_)))
    );
    let outs = t.on_frame(60_000, &books);
    let rows: Vec<_> = outs
        .iter()
        .filter_map(|o| {
            if let SimOut::Account(r) = o {
                Some(r.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].t_ms, rows[0].cash, rows[0].equity),
        (60_000, d("1002.56"), d("1002.56"))
    );
    assert_eq!(
        rows[0].cash + (rows[0].equity - rows[0].cash),
        d("1000") + rows[0].realized + rows[0].unrealized
    );
}

#[test]
fn a_long_gap_emits_one_mark() {
    let (mut t, books) = converted();
    let outs = t.on_frame(10 * 3_600_000 + 5, &books); // ten hours later
    let marks: Vec<i64> = outs
        .iter()
        .filter_map(|o| {
            if let SimOut::Account(r) = o {
                Some(r.t_ms)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(marks, vec![10 * 3_600_000]);
}

#[test]
fn close_runs_pending_conversions_and_drops_the_rest() {
    let g = no_group(FeeSchedule::ZERO);
    let books = books_of(&g);
    let mut t = PaperTrader::new(cfg(&[PolicyKind::P0Converter])).unwrap();
    t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3));
    t.on_frame(1100, &books); // executed; conversion due at 3100, markouts at 2100
    let outs = t.close(1500, &books);
    assert!(outs.iter().any(|o| matches!(o, SimOut::Ledger(e) if e.kind == sim::account::EntryKind::Convert && e.t_ms == 1500)));
    assert!(
        outs.iter()
            .any(|o| matches!(o, SimOut::Account(r) if r.t_ms == 1500))
    );
    assert!(
        t.on_frame(40_000, &books)
            .iter()
            .all(|o| !matches!(o, SimOut::Markout(_))),
        "pending markouts dropped"
    );
}

#[test]
fn state_round_trips_and_restores_by_account_id() {
    let (t, _) = converted();
    let json = serde_json::to_string(&t.state()).unwrap();
    let state: TraderState = serde_json::from_str(&json).unwrap();
    let r =
        PaperTrader::restore(cfg(&[PolicyKind::P0Converter, PolicyKind::P3Naive]), state).unwrap();
    assert_eq!(r.accounts()[0], t.accounts()[0], "restored exactly");
    assert_eq!(
        r.accounts()[1].cash,
        d("1000"),
        "a new account in the config starts fresh"
    );
    let dropped = PaperTrader::restore(
        cfg(&[PolicyKind::P3Naive]),
        serde_json::from_str(&json).unwrap(),
    )
    .unwrap();
    assert_eq!(dropped.accounts().len(), 1);
}

#[test]
fn same_inputs_same_outputs() {
    let run = || {
        let g = no_group(FeeSchedule::ZERO);
        let books = books_of(&g);
        let mut t = PaperTrader::new(cfg(&[PolicyKind::P0Converter, PolicyKind::P3Naive])).unwrap();
        let mut all = Vec::new();
        all.extend(t.on_open(&ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3)));
        for ms in [1100, 2100, 3100, 60_000, 61_000] {
            all.extend(t.on_frame(ms, &books));
        }
        all.extend(t.on_resolution(70_000, "m3", d("1"), d("0")));
        all.extend(t.close(80_000, &books));
        all
    };
    assert_eq!(run(), run());
}
