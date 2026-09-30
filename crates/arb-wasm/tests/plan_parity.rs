//! `basket_json` re-implements `sim::policy::decide`'s planning (edge check, full-set size, minimum,
//! `settle`). This pins the two together: over random open books (2-4 legs, NO and YES baskets,
//! fees on and off, random `q_request`), the planned `q_open`, `q_target` and per-leg limits, or
//! the planning error, equal what `decide` plans for the same books. `decide` sizes by a cash
//! budget, not a share request, so `q_request` is driven as `per_trade` just above the walk's cost
//! at `q_request` (or unbounded when `q_request` is at least the open depth); free cash never binds.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::detect::{Group, GroupLeg, LegStatus, detect_no_basket, detect_yes_basket};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use arb_wasm::api::basket_json;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sim::config::{AccountSpec, P1Params, PolicyKind, SimConfig};
use sim::policy::{Decision, LegInfo, OpenCtx, SkipReason, decide, walk_cost};
use sim::types::Outcome;
use sim::venue::bought_levels;
use std::cell::Cell;

const NO_EDGE: &str = "the basket has no edge at open after fees, so the policy would not trade";

#[derive(Debug, Clone)]
struct LegSpec {
    bid: u32,
    gap: u32,
    n_bids: usize,
    n_asks: usize,
    sizes: Vec<u32>,
    fee: u8,
}

#[derive(Debug, Clone)]
struct Case {
    no: bool,
    legs: Vec<LegSpec>,
    q_request: u32,
    min_order: u32,
}

fn leg_strategy(no: bool) -> impl Strategy<Value = LegSpec> {
    let bid = if no { 15u32..=60 } else { 3u32..=40 };
    (
        bid,
        1u32..=8,
        1usize..=3,
        1usize..=3,
        prop::collection::vec(1u32..=120, 6),
        0u8..3,
    )
        .prop_map(|(bid, gap, n_bids, n_asks, sizes, fee)| LegSpec {
            bid,
            gap,
            n_bids,
            n_asks,
            sizes,
            fee,
        })
}

fn case_strategy() -> impl Strategy<Value = Case> {
    (
        any::<bool>(),
        2usize..=4,
        prop::sample::select(vec![5u32, 10]),
    )
        .prop_flat_map(|(no, n, min_order)| {
            (
                Just(no),
                prop::collection::vec(leg_strategy(no), n),
                min_order..=300,
                Just(min_order),
            )
                .prop_map(|(no, legs, q_request, min_order)| Case {
                    no,
                    legs,
                    q_request,
                    min_order,
                })
        })
}

fn px(cents: u32) -> String {
    format!("0.{cents:02}")
}

/// The YES book of a leg as (bids best first, asks best first) in cents and shares.
type Side = Vec<(u32, u32)>;

fn levels(l: &LegSpec) -> (Side, Side) {
    let bids = (0..l.n_bids)
        .map(|k| (l.bid - k as u32, l.sizes[k]))
        .collect();
    let asks = (0..l.n_asks)
        .map(|k| (l.bid + l.gap + k as u32, l.sizes[3 + k]))
        .collect();
    (bids, asks)
}

fn side_json(v: &[(u32, u32)]) -> Value {
    Value::Array(
        v.iter()
            .map(|&(p, s)| json!({ "p": px(p), "s": s.to_string() }))
            .collect(),
    )
}

/// 0 = no fee, 1 = rate 0.07, 2 = rate 0.25 (the live ceiling), exponent 1.
fn fee_rate(kind: u8) -> Option<(i64, u32)> {
    match kind {
        0 => None,
        1 => Some((7, 2)),
        _ => Some((25, 2)),
    }
}

fn fee_of(kind: u8) -> FeeSchedule {
    fee_rate(kind).map_or(FeeSchedule::ZERO, |(n, s)| {
        FeeSchedule::new(Decimal::new(n, s), 1).unwrap()
    })
}

fn dec(v: &Value) -> Decimal {
    v.as_str().unwrap().parse().unwrap()
}

fn cfg(min_order: u32) -> SimConfig {
    SimConfig {
        settle_delay_ms: 2000,
        markout_offsets_ms: vec![1000],
        mark_every_ms: 60_000,
        burst_window_ms: 10_000,
        min_order_shares: Decimal::from(min_order),
        p1: P1Params {
            min_annualized: Decimal::new(1, 1),
            max_days_to_end: Decimal::from(30),
        },
        scenarios: vec![],
        accounts: vec![],
    }
}

fn group_leg(l: &LegSpec) -> GroupLeg {
    let (b, a) = levels(l);
    let side = |v: &[(u32, u32)]| -> Vec<(Price, Shares)> {
        v.iter()
            .map(|&(p, s)| {
                (
                    Price::parse(&px(p)).unwrap(),
                    Shares::parse(&s.to_string()).unwrap(),
                )
            })
            .collect()
    };
    let mut book = arb_core::book::Book::default();
    book.apply_snapshot(&side(&b), &side(&a), 0);
    GroupLeg {
        status: LegStatus::Live,
        book,
        fee: fee_of(l.fee),
    }
}

fn facade_input(c: &Case) -> String {
    let legs: Vec<Value> = c
        .legs
        .iter()
        .map(|l| {
            let (b, a) = levels(l);
            let book = json!({ "bids": side_json(&b), "asks": side_json(&a) });
            let fee = if let Some((n, s)) = fee_rate(l.fee) {
                json!({ "rate": Decimal::new(n, s).to_string(), "exponent": 1 })
            } else {
                Value::Null
            };
            json!({ "open": book, "t_x": book, "unwind": book, "fee": fee })
        })
        .collect();
    json!({
        "kind": if c.no { "no" } else { "yes" },
        "legs": legs,
        "q_request": c.q_request.to_string(),
        "min_order": c.min_order.to_string(),
    })
    .to_string()
}

/// Returns 1 when `decide` took the basket (and the facade planned the same), 0 when both refused
/// or the case was skipped.
fn check_case(c: &Case) -> Result<usize, TestCaseError> {
    let out: Value = serde_json::from_str(&basket_json(&facade_input(c))).unwrap();

    // The sim's side: the real detector builds the signal, the real `decide` plans it.
    let group = Group {
        legs: c.legs.iter().map(group_leg).collect(),
        augmented: false,
    };
    let (outcome, policy) = if c.no {
        (Outcome::No, PolicyKind::P0Converter)
    } else {
        (Outcome::Yes, PolicyKind::P1Settlement)
    };
    let signal = if c.no {
        detect_no_basket(&group)
    } else {
        detect_yes_basket(&group)
    };
    let Some(signal) = signal else {
        // the detector saw no gross edge: the facade's edge check must say the same
        prop_assert_eq!(out["error"].as_str(), Some(NO_EDGE));
        return Ok(0);
    };
    let open_levels: Vec<_> = signal
        .legs_used
        .iter()
        .map(|&i| bought_levels(&group.legs[i].book, outcome))
        .collect();
    let q_open = signal
        .quote
        .legs
        .iter()
        .map(|l| l.filled)
        .min()
        .unwrap_or(Decimal::ZERO);
    let q_request = Decimal::from(c.q_request);
    // `q_request` as a budget: unbounded past the open depth, else the walk's cost at `q_request`
    // plus 5e-5 (above the at most 4 x 1e-5 the conservative fee rounding can add), so the
    // bisection lands just above `q_request` and `settle` rounds it back down to `q_request`.
    // A `q_request` exactly at a level boundary would let that sliver touch the next level: skip.
    let per_trade = if q_request >= q_open {
        Decimal::from(1_000_000_000_000u64)
    } else {
        let at_boundary = open_levels.iter().any(|lv| {
            let mut cum = Decimal::ZERO;
            lv.iter().any(|(_, s)| {
                cum += s.get();
                cum == q_request
            })
        });
        if at_boundary {
            return Ok(0);
        }
        let cost: Decimal = open_levels
            .iter()
            .zip(&c.legs)
            .map(|(lv, l)| walk_cost(lv, q_request, fee_of(l.fee), false).unwrap().0)
            .sum();
        cost + Decimal::new(5, 5)
    };
    let free_cash = Decimal::from(1_000_000_000_000u64);
    let ctx = OpenCtx {
        episode_id: "e".into(),
        event_id: "ev".into(),
        r_open_ms: 0,
        signal,
        legs: c
            .legs
            .iter()
            .enumerate()
            .map(|(i, l)| LegInfo {
                market_id: format!("m{i}"),
                yes_token: format!("y{i}"),
                fee: fee_of(l.fee),
                seconds_delay: None,
            })
            .collect(),
        open_levels,
        days_to_end: Some(Decimal::ZERO),
        annualized: Some(Decimal::from(100)),
        pair_approved: false,
    };
    let spec = AccountSpec {
        policy,
        scenario: "t".into(),
        bankroll: free_cash,
        per_trade,
        fee_bips: Decimal::ZERO,
    };
    match decide(&cfg(c.min_order), &spec, free_cash, &ctx) {
        Decision::Take(pl) => {
            prop_assert!(out.get("error").is_none(), "facade errored: {out}");
            prop_assert_eq!(dec(&out["q_open"]), pl.detected_q);
            prop_assert_eq!(dec(&out["q_target"]), pl.q_target);
            let limits: Vec<Decimal> = out["legs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| dec(&l["limit"]))
                .collect();
            let want: Vec<Decimal> = pl.legs.iter().map(|l| l.limit.get()).collect();
            prop_assert_eq!(limits, want);
            Ok(1)
        }
        Decision::Skip(SkipReason::NotNetPositive) => {
            prop_assert_eq!(out["error"].as_str(), Some(NO_EDGE));
            Ok(0)
        }
        Decision::Skip(SkipReason::BelowMinSize) => {
            let e = out["error"].as_str().unwrap_or_default();
            prop_assert!(e.contains("is too thin for"), "facade said {out}");
            Ok(0)
        }
        other => Err(TestCaseError::fail(format!(
            "decide gave {other:?}, facade {out}"
        ))),
    }
}

#[test]
fn basket_json_plans_what_the_policy_plans() {
    let mut runner = TestRunner::new(Config {
        cases: 1500,
        ..Config::default()
    });
    let takes = Cell::new(0usize);
    let cases = Cell::new(0usize);
    runner
        .run(&case_strategy(), |c| {
            cases.set(cases.get() + 1);
            takes.set(takes.get() + check_case(&c)?);
            Ok(())
        })
        .unwrap();
    // the generator must reach the take branch often, or the comparison proves little
    assert!(
        takes.get() * 5 >= cases.get(),
        "only {} of {} cases reached a planned basket",
        takes.get(),
        cases.get()
    );
}
