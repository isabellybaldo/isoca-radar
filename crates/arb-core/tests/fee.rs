#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::fee::FeeSchedule;
use arb_core::money::Price;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn p(s: &str) -> Price {
    Price::parse(s).unwrap()
}

#[test]
fn golden_values_from_polymarket_fee_tables_100_shares() {
    let crypto = FeeSchedule::new(dec!(0.07), 1).unwrap();
    let politics = FeeSchedule::new(dec!(0.04), 1).unwrap();
    let sports = FeeSchedule::new(dec!(0.05), 1).unwrap();
    let legacy = FeeSchedule::new(dec!(0.25), 2).unwrap();
    assert_eq!(crypto.fee_exact(p("0.50"), dec!(100)), dec!(1.75));
    assert_eq!(politics.fee_exact(p("0.30"), dec!(100)), dec!(0.84));
    assert_eq!(sports.fee_exact(p("0.95"), dec!(100)), dec!(0.2375)); // docs table shows 0.24 (cents)
    assert_eq!(legacy.fee_exact(p("0.50"), dec!(100)), dec!(1.5625));
    assert_eq!(crypto.fee_exact(p("0"), dec!(100)), Decimal::ZERO);
    assert_eq!(crypto.fee_exact(p("1"), dec!(100)), Decimal::ZERO);
}

#[test]
fn phi_is_symmetric() {
    let f = FeeSchedule::new(dec!(0.05), 1).unwrap();
    for i in 0..=1000u32 {
        let x = Price::new(Decimal::new(i.into(), 3)).unwrap();
        assert_eq!(f.phi(x), f.phi(x.complement()));
    }
}

#[test]
fn venue_rounding_zeroes_dust_but_conservative_never_does() {
    let f = FeeSchedule::new(dec!(0.04), 1).unwrap();
    // 0.04 * 0.01 * 0.99 * 0.01 = 0.00000396
    assert_eq!(f.fee_exact(p("0.01"), dec!(0.01)), dec!(0.00000396));
    assert_eq!(f.fee_venue(p("0.01"), dec!(0.01)), Decimal::ZERO);
    assert_eq!(f.fee_conservative(p("0.01"), dec!(0.01)), dec!(0.00001));
    assert_eq!(f.fee_conservative(p("0.30"), dec!(100)), dec!(0.84)); // exact already
}

#[test]
fn l8_cost_and_proceeds_strictly_increase_for_live_schedules() {
    let schedules = [
        (dec!(0.07), 1),
        (dec!(0.05), 1),
        (dec!(0.04), 1),
        (dec!(0.03), 1),
        (dec!(0.25), 2),
    ];
    for (rate, e) in schedules {
        let f = FeeSchedule::new(rate, e).unwrap();
        let grid: Vec<Price> = (0..=1000u32)
            .map(|i| Price::new(Decimal::new(i.into(), 3)).unwrap())
            .collect();
        for w in grid.windows(2) {
            assert!(
                f.buy_cost(w[0]) < f.buy_cost(w[1]),
                "buy_cost not increasing at {:?}",
                w
            );
            assert!(
                f.sell_proceeds(w[0]) < f.sell_proceeds(w[1]),
                "sell_proceeds not increasing at {:?}",
                w
            );
        }
    }
}

#[test]
fn invalid_schedules_are_rejected() {
    assert!(FeeSchedule::new(dec!(-0.01), 1).is_err());
    assert!(FeeSchedule::new(dec!(0.05), 0).is_err());
    assert!(FeeSchedule::new(dec!(0.05), 5).is_err());
}
