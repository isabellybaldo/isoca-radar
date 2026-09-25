#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::detect::{
    Group, GroupLeg, Kind, LegStatus, detect_implication, detect_no_basket, detect_yes_basket,
};
use arb_core::fee::FeeSchedule;
use arb_core::stats::annualized;
use common::{ctx, d, fee, live, no_group, p};
use rust_decimal::Decimal;
use sim::config::{AccountSpec, P1Params, PolicyKind, SimConfig};
use sim::policy::{Decision, Hold, SkipReason, decide, walk_cost};
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
    // 1.95 × 51.2 = 99.84, plus the fee-rounding margin: 3 legs × 1 level walked each × 0.00001,
    // plus the 0.001 floor = 99.84 + 0.00103 = 99.84103.
    assert_eq!(pl.reserve, d("99.84103"));
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

/// A thin leg (depth 3, below `min_order_shares` 5) used to be misread as `NoCash`: `q_open` (3)
/// passed the old `<= 0` check, so `decide` fell through to `at(cfg.min_order_shares)`, which
/// failed to quote 5 shares on the thin leg (only 3 available) and landed on the generic `NoCash`
/// fallback — even with abundant free cash. The size problem must be reported as `BelowMinSize`.
#[test]
fn a_thin_leg_is_below_min_size_not_a_cash_problem() {
    let g = Group {
        legs: vec![
            live(Some("0.40"), Some("0.45"), FeeSchedule::ZERO),
            live(Some("0.35"), Some("0.40"), FeeSchedule::ZERO),
            GroupLeg {
                status: LegStatus::Live,
                book: common::book(&[("0.30", "3")], &[("0.35", "100")]),
                fee: FeeSchedule::ZERO,
            },
            live(None, Some("0.05"), FeeSchedule::ZERO),
        ],
        augmented: false,
    };
    let sig = detect_no_basket(&g).unwrap();
    let c = ctx("e1", 1000, &g, sig, &NO3);
    assert!(matches!(
        decide(&cfg(), &acct(PolicyKind::P0Converter, "100"), d("1000"), &c),
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

#[test]
fn multi_level_books_never_size_past_the_budget() {
    // Build a 4-leg NO basket with leg0 having multi-level books.
    // leg0: YES bids [("0.90","50"), ("0.36","950")], asks [("0.95","100")]
    // leg1: YES bids [("0.35","1000")], asks [("0.40","100")]
    // leg2: YES bids [("0.30","1000")], asks [("0.35","100")]
    // leg3: YES asks [("0.05","100")], no bids
    // NO asks: 0.10×50 then 0.64×950, 0.65×1000, 0.70×1000.
    // Walked cost for q > 50: 1.99q − 27.
    let g = Group {
        legs: vec![
            GroupLeg {
                status: LegStatus::Live,
                book: common::book(&[("0.90", "50"), ("0.36", "950")], &[("0.95", "100")]),
                fee: FeeSchedule::ZERO,
            },
            GroupLeg {
                status: LegStatus::Live,
                book: common::book(&[("0.35", "1000")], &[("0.40", "100")]),
                fee: FeeSchedule::ZERO,
            },
            GroupLeg {
                status: LegStatus::Live,
                book: common::book(&[("0.30", "1000")], &[("0.35", "100")]),
                fee: FeeSchedule::ZERO,
            },
            live(None, Some("0.05"), FeeSchedule::ZERO),
        ],
        augmented: false,
    };
    let sig = detect_no_basket(&g).unwrap();
    let c = ctx(
        "e1",
        1000,
        &g,
        sig,
        &[Outcome::No, Outcome::No, Outcome::No],
    );

    // Case A: per_trade binds. per_trade=100, free_cash=1000.
    // Bisection: 1.99q − 27 ≤ 100 → q ≤ 63.8 → q_target = 63 on the whole-share grid.
    let Decision::Take(pl_a) = decide(&cfg(), &acct(PolicyKind::P0Converter, "100"), d("1000"), &c)
    else {
        panic!("expected take in case A (per_trade binds)")
    };
    assert_eq!(pl_a.q_target, d("63"), "case A: q_target should be 63");

    // Verify walked cost for q=63 is ≤ 100.
    let mut walked_cost_a = Decimal::ZERO;
    for (levels, leg) in c.open_levels.iter().zip(&pl_a.legs) {
        let (cost, _) = walk_cost(levels, pl_a.q_target, leg.fee, false)
            .expect("walked cost should succeed for taken q");
        walked_cost_a += cost;
    }
    assert!(
        walked_cost_a <= d("100"),
        "case A: walked cost {} should be <= per_trade 100",
        walked_cost_a
    );
    // Walked cost should be 1.99*63 − 27 = 98.37.
    assert!(
        walked_cost_a >= d("98"),
        "case A: walked cost {} should be >= 98",
        walked_cost_a
    );

    // Reserve base: 1.99 × 63 = 125.37. Margin: leg0 walks 2 levels at q=63 (50 + 13), legs 1–2
    // walk 1 each → 4 levels total × 0.00001 + the 0.001 floor = 125.37 + 0.00104 = 125.37104.
    assert_eq!(
        pl_a.reserve,
        d("125.37104"),
        "case A: reserve should be 125.37104"
    );
    assert!(
        pl_a.reserve <= d("1000"),
        "case A: reserve {} should be <= free_cash 1000",
        pl_a.reserve
    );

    // Case B: reserve binds. per_trade=1000, free_cash=110.
    // Bisection: 1.99q − 27 ≤ 110 → q ≤ 68.84 → q_target = 68.
    // Reserve at q=68: 1.99*68 + 0.001 = 135.321 > 110, so reserve correction kicks in.
    // C = 1.99, q = (110 − 0.001) / 1.99 = 109.999 / 1.99 ≈ 55.27 → q_target = 55.
    let Decision::Take(pl_b) = decide(&cfg(), &acct(PolicyKind::P0Converter, "1000"), d("110"), &c)
    else {
        panic!("expected take in case B (reserve binds)")
    };
    assert_eq!(pl_b.q_target, d("55"), "case B: q_target should be 55");

    // Reserve base: 1.99 * 55 = 109.45. Margin: leg0 walks 2 levels at q=55 (50 + 5), legs 1–2
    // walk 1 each → 4 levels total * 0.00001 + the 0.001 floor = 109.45 + 0.00104 = 109.45104.
    assert_eq!(
        pl_b.reserve,
        d("109.45104"),
        "case B: reserve should be 109.45104"
    );
    assert!(
        pl_b.reserve <= d("110"),
        "case B: reserve {} should be <= free_cash 110",
        pl_b.reserve
    );

    // Case C: limits refresh. per_trade=1000, free_cash=80.
    // Bisection: 1.99q − 27 ≤ 80 → q ≤ 53.77 → q_target = 53 (grid: whole shares, still >50).
    // Reserve at q=53: 1.99*53 + 0.001 = 105.471 > 80, so reserve correction kicks in.
    // C = 1.99, q_shrunk = (80 − 0.001) / 1.99 = 79.999 / 1.99 ≈ 40.2015.
    // Settling q_shrunk re-quotes FIRST (q<=50 → leg0's cheap 0.10 level, grid multiples of
    // 0.2) before rounding, so it lands on 40.2 — not on 40.2's whole-number floor — because
    // 40.2 itself already satisfies every limit's 2dp rule (0.10, 0.65, 0.70 all reduce a
    // multiple of 0.2 to <= 2 decimal places). Settling again at q=40.2 is stable (limits
    // unchanged), so q_target = 40.2, with leg0's limit refreshed down to 0.1.
    // C = 1.45, reserve = 1.45*40.2 + 0.001 = 58.291.
    let Decision::Take(pl_c) = decide(&cfg(), &acct(PolicyKind::P0Converter, "1000"), d("80"), &c)
    else {
        panic!("expected take in case C (limits refresh)")
    };
    assert_eq!(pl_c.q_target, d("40.2"), "case C: q_target should be 40.2");

    // leg0 limit should be refreshed to 0.1 (cheaper level after shrink).
    assert_eq!(
        pl_c.legs[0].limit,
        p("0.1"),
        "case C: leg0 limit should be 0.1 after refresh"
    );
    assert_eq!(
        pl_c.legs[1].limit,
        p("0.65"),
        "case C: leg1 limit should be 0.65"
    );
    assert_eq!(
        pl_c.legs[2].limit,
        p("0.70"),
        "case C: leg2 limit should be 0.70"
    );

    // Reserve base: 1.45 * 40.2 = 58.29. Margin: at q=40.2 (< 50) leg0 walks only its first
    // level, so all 3 legs walk 1 level each → 3 * 0.00001 + the 0.001 floor = 58.29 + 0.00103
    // = 58.29103.
    assert_eq!(
        pl_c.reserve,
        d("58.29103"),
        "case C: reserve should be 58.29103"
    );
    assert!(
        pl_c.reserve <= d("80"),
        "case C: reserve {} should be <= free_cash 80",
        pl_c.reserve
    );

    // Case D: the primary path's own rounding crosses a book breakpoint. per_trade="72.50796",
    // free_cash=1000.
    // Bisection: cost(q) = 1.45q for q<=50, then 72.5 + 1.99*(q-50) for q>50.
    // budget=72.50796 → q_want = 50 + (72.50796 - 72.5) / 1.99 ≈ 50.004.
    // at(50.004) touches leg0's second level, so its limit is 0.64 (grid: whole shares) → 50.
    // Re-settling at q=50 lands exactly on leg0's first-level breakpoint: its limit drops to
    // 0.1 (grid: multiples of 0.2, and 50 is valid) → q_target stays 50, but with fresh limits.
    let Decision::Take(pl_d) = decide(
        &cfg(),
        &acct(PolicyKind::P0Converter, "72.50796"),
        d("1000"),
        &c,
    ) else {
        panic!("expected take in case D (primary-path breakpoint)")
    };
    assert_eq!(pl_d.q_target, d("50"), "case D: q_target should be 50");
    assert_eq!(
        pl_d.legs[0].limit,
        p("0.1"),
        "case D: leg0 limit should settle to 0.1, not the stale 0.64"
    );
    assert_eq!(
        pl_d.legs[1].limit,
        p("0.65"),
        "case D: leg1 limit should be 0.65"
    );
    assert_eq!(
        pl_d.legs[2].limit,
        p("0.70"),
        "case D: leg2 limit should be 0.70"
    );
    // Reserve base: 1.45 * 50 = 72.5. Margin: at q=50 leg0 exactly exhausts its first level
    // (50 shares) without touching the second, so all 3 legs walk 1 level each → 3 * 0.00001
    // + the 0.001 floor = 72.5 + 0.00103 = 72.50103.
    assert_eq!(
        pl_d.reserve,
        d("72.50103"),
        "case D: reserve should be 72.50103"
    );
}
