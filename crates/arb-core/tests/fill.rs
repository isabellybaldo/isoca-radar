#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

fn leg(levels: &[(&str, &str)], fee: FeeSchedule) -> BuyLeg {
    BuyLeg {
        levels: levels
            .iter()
            .map(|(p, s)| (Price::parse(p).unwrap(), Shares::parse(s).unwrap()))
            .collect(),
        fee,
    }
}
const ONE: BasketRule = BasketRule::AllLegs {
    payoff: Decimal::ONE,
};

#[test]
fn yes_basket_walks_merged_breakpoints() {
    // leg A: 0.30×10 then 0.32×10; leg B: 0.60×5 then 0.65×20. Zero fees.
    let a = leg(&[("0.30", "10"), ("0.32", "10")], FeeSchedule::ZERO);
    let b = leg(&[("0.60", "5"), ("0.65", "20")], FeeSchedule::ZERO);
    let q = walk(&[a, b], ONE, None);
    // seg1: 5 @ (0.30+0.60)=0.90 → +0.10×5 = 0.50
    // seg2: 5 @ (0.30+0.65)=0.95 → +0.05×5 = 0.25
    // seg3: 10 @ (0.32+0.65)=0.97 → +0.03×10 = 0.30 ; then A exhausted → stop
    assert_eq!(q.q_max, dec!(20));
    assert_eq!(q.pnl, dec!(1.05));
    assert_eq!(q.top_edge, dec!(0.10));
    assert_eq!(q.legs[0].levels_used, 2);
    assert_eq!(q.legs[1].levels_used, 2);
}

#[test]
fn exactly_one_is_not_an_opportunity() {
    let a = leg(&[("0.40", "10")], FeeSchedule::ZERO);
    let b = leg(&[("0.60", "10")], FeeSchedule::ZERO);
    let q = walk(&[a, b], ONE, None);
    assert_eq!(q.q_max, Decimal::ZERO);
    assert_eq!(q.top_edge, Decimal::ZERO);
}

#[test]
fn six_decimal_boundary_is_an_opportunity() {
    let l = |s: &str| leg(&[(s, "7")], FeeSchedule::ZERO);
    let q = walk(&[l("0.333333"), l("0.333333"), l("0.333333")], ONE, None);
    assert_eq!(q.q_max, dec!(7));
    assert_eq!(q.pnl, dec!(0.000007));
}

#[test]
fn budget_caps_cost_mid_level() {
    let a = leg(&[("0.40", "1000")], FeeSchedule::ZERO);
    let b = leg(&[("0.50", "1000")], FeeSchedule::ZERO);
    let q = walk(&[a, b], ONE, Some(dec!(100)));
    // room = 100 / 0.90 = 111.111111 (rounded down to 6 dp); cost = 111.111111 × 0.90
    assert_eq!(q.q_max, dec!(111.111111));
    assert_eq!(q.cost, dec!(99.9999999));
    assert!(q.cost <= dec!(100));
}

#[test]
fn reviewer_counterexample_nested_no_basket_is_10() {
    // YES bids 0.60×100, 0.45×100, 0.10×50 → NO asks 0.40, 0.55, 0.90 (spec L11)
    let legs = vec![
        leg(&[("0.40", "100")], FeeSchedule::ZERO),
        leg(&[("0.55", "100")], FeeSchedule::ZERO),
        leg(&[("0.90", "50")], FeeSchedule::ZERO),
    ];
    let q = walk(&legs, BasketRule::NestedNo, None);
    assert_eq!(q.pnl, dec!(10));
    assert_eq!(q.q_max, dec!(100));
}

#[test]
fn l5_no_basket_profit_is_n_minus_1_minus_sum_no() {
    // YES bids 0.40, 0.35, 0.30 (Σ = 1.05) → NO asks 0.60, 0.65, 0.70; 100 each; zero fees
    let legs = vec![
        leg(&[("0.60", "100")], FeeSchedule::ZERO),
        leg(&[("0.65", "100")], FeeSchedule::ZERO),
        leg(&[("0.70", "100")], FeeSchedule::ZERO),
    ];
    let q = walk(&legs, BasketRule::NestedNo, None);
    assert_eq!(q.payout, dec!(200)); // (n−1)·100
    assert_eq!(q.cost, dec!(195));
    assert_eq!(q.pnl, dec!(5)); // (n−1) − ΣNO = ΣYES_bid − 1 = 0.05 per basket
    let n = dec!(3);
    let sum_no = dec!(1.95);
    assert_eq!(n - sum_no, dec!(1.05), "the paper's LHS is off by one");
    assert_eq!((n - Decimal::ONE) - sum_no, dec!(0.05));
}

#[test]
fn fees_are_booked_conservatively() {
    let f = FeeSchedule::new(dec!(0.04), 1).unwrap();
    let q = walk(
        &[leg(&[("0.40", "3")], f), leg(&[("0.50", "3")], f)],
        ONE,
        None,
    );
    // exact fees: 3×0.04×(0.24+0.25) = 0.0588 (already 5 dp) → pnl = 0.30 − 0.0588
    assert_eq!(q.fees_exact, dec!(0.0588));
    assert_eq!(q.fees, dec!(0.0588));
    assert_eq!(q.pnl, dec!(0.2412));
    assert_eq!(q.pnl_gross, dec!(0.30));
}

// ---------- brute-force checks (exact fees, unit-size levels) ----------

fn cents(c: u32) -> Price {
    Price::new(Decimal::new(c.into(), 2)).unwrap()
}
fn unit_leg(asks: &[u32], fee: FeeSchedule) -> BuyLeg {
    BuyLeg {
        levels: asks
            .iter()
            .map(|&c| (cents(c), Shares::parse("1").unwrap()))
            .collect(),
        fee,
    }
}

/// L9 reference: unit-by-unit, stop at the first unit whose exact marginal is ≤ 0.
fn brute_all(legs: &[BuyLeg]) -> Decimal {
    let depth = legs.iter().map(|l| l.levels.len()).min().unwrap_or(0);
    let mut total = Decimal::ZERO;
    for k in 0..depth {
        let m = Decimal::ONE
            - legs
                .iter()
                .map(|l| l.fee.buy_cost(l.levels[k].0))
                .sum::<Decimal>();
        if m <= Decimal::ZERO {
            break;
        }
        total += m;
    }
    total
}

/// L11 reference: exhaustive search over every collection of NO baskets.
fn brute_nested(
    legs: &[BuyLeg],
    used: Vec<usize>,
    memo: &mut HashMap<Vec<usize>, Decimal>,
) -> Decimal {
    if let Some(v) = memo.get(&used) {
        return *v;
    }
    let avail: Vec<usize> = (0..legs.len())
        .filter(|&i| used[i] < legs[i].levels.len())
        .collect();
    let mut best = Decimal::ZERO;
    for mask in 1u32..(1 << avail.len()) {
        let s: Vec<usize> = avail
            .iter()
            .enumerate()
            .filter(|(j, _)| mask & (1 << j) != 0)
            .map(|(_, &i)| i)
            .collect();
        let payoff = Decimal::from(s.len() as u64) - Decimal::ONE;
        let cost: Decimal = s
            .iter()
            .map(|&i| legs[i].fee.buy_cost(legs[i].levels[used[i]].0))
            .sum();
        let mut next = used.clone();
        for &i in &s {
            next[i] += 1;
        }
        let v = payoff - cost + brute_nested(legs, next, memo);
        if v > best {
            best = v;
        }
    }
    memo.insert(used, best);
    best
}

fn ladders() -> impl Strategy<Value = (Vec<Vec<u32>>, u32)> {
    let ladder = prop::collection::vec(1u32..=99, 0..=3).prop_map(|mut v| {
        v.sort_unstable();
        v
    });
    (
        prop::collection::vec(ladder, 2..=4),
        prop::sample::select(vec![0u32, 4, 5]),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn l9_walk_equals_unit_brute_force((ls, rate) in ladders()) {
        let fee = FeeSchedule::new(Decimal::new(rate.into(), 2), 1).unwrap();
        let legs: Vec<BuyLeg> = ls.iter().map(|l| unit_leg(l, fee)).collect();
        let q = walk(&legs, BasketRule::AllLegs { payoff: Decimal::ONE }, None);
        prop_assert_eq!(q.pnl_exact.max(Decimal::ZERO), brute_all(&legs));
    }

    #[test]
    fn l11_nested_walk_equals_exhaustive_optimum((ls, rate) in ladders()) {
        let fee = FeeSchedule::new(Decimal::new(rate.into(), 2), 1).unwrap();
        let legs: Vec<BuyLeg> = ls.iter().map(|l| unit_leg(l, fee)).collect();
        let q = walk(&legs, BasketRule::NestedNo, None);
        let mut memo = HashMap::new();
        prop_assert_eq!(q.pnl_exact.max(Decimal::ZERO), brute_nested(&legs, vec![0; legs.len()], &mut memo));
    }
}
