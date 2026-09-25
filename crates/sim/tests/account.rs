#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::d;
use proptest::prelude::*;
use rust_decimal::Decimal;
use sim::account::{Account, EntryKind};
use sim::config::{AccountSpec, PolicyKind};
use sim::types::{LegKey, Outcome};
use sim::venue::OrderFill;

fn spec(fee_bips: &str) -> AccountSpec {
    AccountSpec {
        policy: PolicyKind::P0Converter,
        scenario: "t".into(),
        bankroll: d("1000"),
        per_trade: d("100"),
        fee_bips: d(fee_bips),
    }
}

fn k(m: &str, o: Outcome) -> LegKey {
    LegKey {
        market_id: m.into(),
        yes_token: format!("y-{m}"),
        outcome: o,
    }
}

fn fill(q: &str, notional: &str, fees: &str) -> OrderFill {
    OrderFill {
        filled: d(q),
        notional: d(notional),
        fees: d(fees),
        levels_used: 1,
    }
}

fn identity_holds(a: &Account, bid: impl Fn(&LegKey) -> Option<Decimal>) {
    let v = a.valuation(bid);
    assert_eq!(
        a.cash + v.marks,
        a.spec.bankroll + a.realized + v.unrealized(),
        "equity identity"
    );
}

#[test]
fn buy_then_partial_sell_books_realized_on_proportional_basis() {
    let mut a = Account::new(spec("0"));
    let leg = k("m0", Outcome::Yes);
    a.buy(0, "e1", &leg, &fill("10", "4", "0.1")).unwrap();
    assert_eq!(a.cash, d("995.9"));
    let e = a.sell(1, "e1", &leg, &fill("4", "2.4", "0.05")).unwrap();
    assert_eq!(e.kind, EntryKind::Sell);
    assert_eq!(e.realized_delta, d("0.71")); // 2.35 − 4.1·4/10
    assert_eq!(a.cash, d("998.25"));
    assert_eq!(a.positions[&leg].qty, d("6"));
    identity_holds(&a, |_| Some(d("0.5")));
}

#[test]
fn conversion_pays_set_minus_one_and_returns_complements_marked_zero() {
    let mut a = Account::new(spec("0"));
    let s: Vec<LegKey> = ["m0", "m1", "m2"]
        .iter()
        .map(|m| k(m, Outcome::No))
        .collect();
    for l in &s {
        a.buy(0, "e1", l, &fill("10", "6", "0")).unwrap();
    }
    let comp = vec![k("m3", Outcome::Yes)];
    let e = a.convert(2000, "e1", &s, &comp, d("10")).unwrap();
    assert_eq!((e.cash_delta, e.realized_delta), (d("20"), d("2")));
    assert_eq!(a.cash, d("1002"));
    assert!(a.positions.is_empty());
    assert_eq!(a.returned[&comp[0]], d("10"));
    assert_eq!(
        a.valuation(|_| Some(d("0.9"))).marks,
        d("0"),
        "returned YES is marked at 0"
    );
    identity_holds(&a, |_| Some(d("0.9")));
}

#[test]
fn conversion_fee_bips_are_charged() {
    let mut a = Account::new(spec("50"));
    let s: Vec<LegKey> = ["m0", "m1", "m2"]
        .iter()
        .map(|m| k(m, Outcome::No))
        .collect();
    for l in &s {
        a.buy(0, "e1", l, &fill("10", "6", "0")).unwrap();
    }
    let e = a.convert(2000, "e1", &s, &[], d("10")).unwrap();
    assert_eq!((e.cash_delta, e.fees), (d("19.9"), d("0.1")));
}

#[test]
fn settlement_pays_positions_and_windfalls() {
    let mut a = Account::new(spec("0"));
    a.buy(0, "e1", &k("m0", Outcome::Yes), &fill("10", "4.5", "0"))
        .unwrap();
    a.returned.insert(k("m0", Outcome::Yes), d("3"));
    let es = a.settle(9, "m0", d("1"), d("0"));
    assert_eq!(es.len(), 2);
    assert_eq!(a.realized, d("8.5")); // 10 − 4.5, + 3 windfall
    assert_eq!(a.cash, d("1008.5"));
    assert!(a.positions.is_empty() && a.returned.is_empty());
    assert!(
        a.settle(10, "m0", d("1"), d("0")).is_empty(),
        "nothing left to settle"
    );
}

#[test]
fn non_binary_payout_settles_at_the_vector() {
    let mut a = Account::new(spec("0"));
    a.buy(0, "e1", &k("m0", Outcome::Yes), &fill("10", "4", "0"))
        .unwrap();
    a.buy(0, "e1", &k("m0", Outcome::No), &fill("10", "4", "0"))
        .unwrap();
    a.settle(9, "m0", d("0.5"), d("0.5"));
    assert_eq!(a.realized, d("2")); // (5 − 4) + (5 − 4)
}

#[test]
fn marks_track_drawdown() {
    let mut a = Account::new(spec("0"));
    let leg = k("m0", Outcome::Yes);
    a.buy(0, "e1", &leg, &fill("100", "50", "0")).unwrap();
    let e = a.mark(60_000, |_| Some(d("0.6")));
    assert_eq!(e.equity, Some(d("1010")));
    a.mark(120_000, |_| Some(d("0.4")));
    assert_eq!(a.peak_equity, d("1010"));
    assert_eq!(a.max_drawdown, d("20"));
    assert_eq!(
        a.valuation(|_| None).marks,
        d("0"),
        "no bid: marked at 0 (conservative)"
    );
}

proptest! {
    #[test]
    fn equity_identity_holds_for_any_sequence(ops in proptest::collection::vec((0u8..4, 1u32..500, 1u32..99), 1..40)) {
        let mut a = Account::new(spec("25"));
        let legs: Vec<LegKey> = ["m0", "m1"].iter().map(|m| k(m, Outcome::No)).collect();
        for (i, (op, q, px)) in ops.into_iter().enumerate() {
            let t = i64::try_from(i).unwrap();
            let q = Decimal::from(q) / Decimal::from(10);
            let px = Decimal::from(px) / Decimal::from(100);
            let leg = &legs[usize::from(op % 2)];
            match op {
                0 | 1 => { a.buy(t, "e", leg, &OrderFill { filled: q, notional: q * px, fees: d("0.01"), levels_used: 1 }); }
                2 => {
                    let held = a.positions.get(leg).map_or(Decimal::ZERO, |p| p.qty);
                    let s = q.min(held);
                    if s > Decimal::ZERO { a.sell(t, "e", leg, &OrderFill { filled: s, notional: s * px, fees: d("0.01"), levels_used: 1 }); }
                }
                _ => { a.convert(t, "e", &legs, &[k("m9", Outcome::Yes)], q); }
            }
            let v = a.valuation(|_| Some(d("0.37")));
            prop_assert_eq!(a.cash + v.marks, a.spec.bankroll + a.realized + v.unrealized());
        }
    }
}
