#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::detect::{Group, Kind, detect_implication, detect_no_basket, detect_yes_basket};
use arb_core::fee::FeeSchedule;
use arb_core::stats::annualized;
use common::{ctx, d, fee, live, no_group, p};
use rust_decimal::Decimal;
use sim::config::{AccountSpec, P1Params, PolicyKind, SimConfig};
use sim::policy::{Decision, Hold, SkipReason, decide};
use sim::types::{LegKey, Outcome};

fn cfg() -> SimConfig {
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
        scenarios: vec![],
        accounts: vec![],
    }
}

fn acct(policy: PolicyKind, per_trade: &str) -> AccountSpec {
    AccountSpec {
        policy,
        scenario: "t".into(),
        bankroll: d("1000"),
        per_trade: d(per_trade),
        fee_bips: Decimal::ZERO,
    }
}

const NO3: [Outcome; 3] = [Outcome::No, Outcome::No, Outcome::No];

#[test]
fn p0_takes_a_net_positive_no_basket_sized_to_the_budget() {
    let g = no_group(FeeSchedule::ZERO);
    let c = ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3);
    let Decision::Take(pl) = decide(&cfg(), &acct(PolicyKind::P0Converter, "100"), d("1000"), &c)
    else {
        panic!("expected take")
    };
    // unit cost 1.95; 100 / 1.95 = 51.28 → sizes valid for limits 0.60/0.65/0.70 are multiples of 0.2
    assert_eq!(pl.q_target, d("51.2"));
    assert_eq!(pl.detected_q, d("100"));
    assert_eq!(pl.reserve, d("99.841"));
    assert_eq!(
        pl.legs.iter().map(|l| l.limit).collect::<Vec<_>>(),
        vec![p("0.6"), p("0.65"), p("0.7")]
    );
    assert!(pl.legs.iter().all(|l| l.key.outcome == Outcome::No));
    let Hold::Convert { complements } = &pl.hold else {
        panic!("NO basket converts")
    };
    assert_eq!(
        complements,
        &vec![LegKey {
            market_id: "m3".into(),
            yes_token: "y3".into(),
            outcome: Outcome::Yes
        }]
    );
}

#[test]
fn only_the_matching_policies_look_at_a_kind() {
    let g = no_group(FeeSchedule::ZERO);
    let c = ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3);
    assert!(matches!(
        decide(
            &cfg(),
            &acct(PolicyKind::P1Settlement, "100"),
            d("1000"),
            &c
        ),
        Decision::NotMine
    ));
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P2Combo, "100"), d("1000"), &c),
        Decision::NotMine
    ));
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P3Naive, "100"), d("1000"), &c),
        Decision::Take(_)
    ));
}

#[test]
fn fees_turn_p0_away_but_not_the_naive_control() {
    let g = no_group(fee("0.25", 1)); // Σφ ≈ 0.169 > gross edge 0.05
    let sig = detect_no_basket(&g).unwrap(); // gross edge > 0, so the detector still reports it
    assert!(
        sig.quote.pnl <= Decimal::ZERO,
        "the net walk stops before its first segment"
    );
    let c = ctx("e1", 1000, &g, sig, &NO3);
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P0Converter, "100"), d("1000"), &c),
        Decision::Skip(SkipReason::NotNetPositive)
    ));
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P3Naive, "100"), d("1000"), &c),
        Decision::Take(_)
    ));
}

#[test]
fn cash_and_minimum_size_limit_the_order() {
    let g = no_group(FeeSchedule::ZERO);
    let c = ctx("e1", 1000, &g, detect_no_basket(&g).unwrap(), &NO3);
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P0Converter, "100"), d("5"), &c),
        Decision::Skip(SkipReason::NoCash)
    ));
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P0Converter, "5"), d("1000"), &c),
        Decision::Skip(SkipReason::BelowMinSize)
    ));
}

fn yes_group() -> Group {
    Group {
        legs: vec![
            live(None, Some("0.45"), FeeSchedule::ZERO),
            live(None, Some("0.50"), FeeSchedule::ZERO),
        ],
        augmented: false,
    }
}

#[test]
fn p1_needs_lockup_figures_within_limits() {
    let g = yes_group();
    let sig = detect_yes_basket(&g).unwrap();
    assert_eq!(sig.kind, Kind::YesBasket);
    let r = sig.quote.pnl / sig.quote.cost;
    let yes2 = [Outcome::Yes, Outcome::Yes];
    let p1 = acct(PolicyKind::P1Settlement, "100");

    let c = ctx("e1", 0, &g, sig.clone(), &yes2);
    assert!(matches!(
        decide(&cfg(), &p1, d("1000"), &c),
        Decision::Skip(SkipReason::NoLockupFigures)
    ));

    let mut c = ctx("e1", 0, &g, sig.clone(), &yes2);
    c.days_to_end = Some(d("10"));
    c.annualized = annualized(r, d("10"));
    let Decision::Take(pl) = decide(&cfg(), &p1, d("1000"), &c) else {
        panic!("expected take")
    };
    assert_eq!(pl.hold, Hold::ToResolution);

    c.days_to_end = Some(d("40"));
    assert!(matches!(
        decide(&cfg(), &p1, d("1000"), &c),
        Decision::Skip(SkipReason::TooLong)
    ));

    c.days_to_end = Some(d("10"));
    c.annualized = Some(d("0.05"));
    assert!(matches!(
        decide(&cfg(), &p1, d("1000"), &c),
        Decision::Skip(SkipReason::AnnualizedTooLow)
    ));
}

#[test]
fn incomplete_yes_baskets_are_for_the_naive_control_only() {
    let mut g = yes_group();
    g.augmented = true;
    let sig = detect_yes_basket(&g).unwrap();
    assert_eq!(sig.kind, Kind::YesBasketIncomplete);
    let c = ctx("e1", 0, &g, sig, &[Outcome::Yes, Outcome::Yes]);
    assert!(matches!(
        decide(
            &cfg(),
            &acct(PolicyKind::P1Settlement, "100"),
            d("1000"),
            &c
        ),
        Decision::NotMine
    ));
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P3Naive, "100"), d("1000"), &c),
        Decision::Take(_)
    ));
}

#[test]
fn p2_needs_an_approved_pair_and_buys_no_a_yes_b() {
    let a = live(Some("0.60"), None, FeeSchedule::ZERO); // NO_A ask 0.40
    let b = live(None, Some("0.55"), FeeSchedule::ZERO); // YES_B ask 0.55
    let sig = detect_implication(&a, &b).unwrap();
    let g = Group {
        legs: vec![a, b],
        augmented: false,
    };
    let mut c = ctx("e1", 0, &g, sig, &[Outcome::No, Outcome::Yes]);
    let p2 = acct(PolicyKind::P2Combo, "100");
    assert!(matches!(
        decide(&cfg(), &p2, d("1000"), &c),
        Decision::Skip(SkipReason::PairNotApproved)
    ));
    c.pair_approved = true;
    let Decision::Take(pl) = decide(&cfg(), &p2, d("1000"), &c) else {
        panic!("expected take")
    };
    assert_eq!(
        pl.legs.iter().map(|l| l.key.outcome).collect::<Vec<_>>(),
        vec![Outcome::No, Outcome::Yes]
    );
}
