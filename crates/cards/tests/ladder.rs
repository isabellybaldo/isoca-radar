#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::ladder::{BreakEven, Direction, Rung, break_even, measured};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn rung(latency_ms: i64, net: Decimal) -> Rung {
    Rung {
        latency_ms: Some(latency_ms),
        extra: false,
        n_exec: 30,
        net,
    }
}

#[test]
fn too_few_trades_on_any_rung_is_thin() {
    let mut r = vec![rung(0, dec!(1)), rung(25, dec!(-1))];
    r[1].n_exec = 29;
    assert_eq!(break_even(&r), BreakEven::Thin);
    assert_eq!(break_even(&[]), BreakEven::Thin);
}

#[test]
fn every_rung_paying_is_all_and_every_rung_losing_is_none() {
    assert_eq!(
        break_even(&[rung(0, dec!(0)), rung(25, dec!(3))]),
        BreakEven::PaysEverywhere
    );
    assert_eq!(
        break_even(&[rung(0, dec!(-0.01)), rung(25, dec!(-3))]),
        BreakEven::PaysNowhere
    );
}

#[test]
fn the_first_sign_change_is_interpolated_and_rounded_to_5_ms() {
    // 25 + 27 × 5 / (5 + 5) = 38.5 → 40
    assert_eq!(
        break_even(&[rung(0, dec!(10)), rung(25, dec!(5)), rung(52, dec!(-5))]),
        BreakEven::At {
            latency_ms: 40,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }
    );
    // the faster rungs lose: 0 + 100 × 4 / 8 = 50
    assert_eq!(
        break_even(&[rung(100, dec!(4)), rung(0, dec!(-4))]),
        BreakEven::At {
            latency_ms: 50,
            direction: Direction::PaysBeyond,
            not_monotone: false
        },
        "rungs are sorted by latency first"
    );
    // 52 + 48 / 101 = 52.475 → rounds to 50, clamped into [52, 100]
    assert_eq!(
        break_even(&[rung(52, dec!(1)), rung(100, dec!(-100))]),
        BreakEven::At {
            latency_ms: 52,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }
    );
}

#[test]
fn more_than_one_sign_change_is_flagged_not_monotone() {
    // first change between 0 and 25 at 12.5 → 15 (half away from zero)
    assert_eq!(
        break_even(&[rung(0, dec!(1)), rung(25, dec!(-1)), rung(52, dec!(1))]),
        BreakEven::At {
            latency_ms: 15,
            direction: Direction::LosesBeyond,
            not_monotone: true
        }
    );
}

#[test]
fn extras_never_enter_the_computation() {
    let extra = Rung {
        latency_ms: Some(450),
        extra: true,
        n_exec: 0,
        net: dec!(-100),
    };
    assert_eq!(
        break_even(&[rung(0, dec!(1)), rung(25, dec!(2)), extra]),
        BreakEven::PaysEverywhere
    );
}

#[test]
fn the_json_shape_matches_the_spec() {
    let j = |b: BreakEven| serde_json::to_string(&b).unwrap();
    assert_eq!(j(BreakEven::Thin), r#"{"kind":"thin"}"#);
    assert_eq!(j(BreakEven::PaysEverywhere), r#"{"kind":"all"}"#);
    assert_eq!(j(BreakEven::PaysNowhere), r#"{"kind":"none"}"#);
    assert_eq!(
        j(BreakEven::At {
            latency_ms: 40,
            direction: Direction::LosesBeyond,
            not_monotone: false
        }),
        r#"{"kind":"at","latency_ms":40,"direction":"loses_beyond"}"#
    );
    assert_eq!(
        j(BreakEven::At {
            latency_ms: 15,
            direction: Direction::PaysBeyond,
            not_monotone: true
        }),
        r#"{"kind":"at","latency_ms":15,"direction":"pays_beyond","not_monotone":true}"#
    );
}

#[test]
fn measured_is_the_nearest_rank_median_of_lag_plus_half_the_rtt() {
    let minutes = [
        (Some(78), Some(150)), // 153
        (Some(80), None),      // not measured
        (None, Some(10)),      // not measured
        (Some(70), Some(20)),  // 80
    ];
    assert_eq!(measured(minutes.into_iter()), Some(80));
    assert_eq!(measured(std::iter::empty()), None);
}

const LAT: [i64; 6] = [0, 25, 52, 100, 200, 300];

proptest! {
    #[test]
    fn every_sign_pattern_has_a_defined_answer_between_the_rungs_it_names(
        nets in proptest::collection::vec(-100i64..100, 1..=6),
    ) {
        let rungs: Vec<Rung> = nets.iter().zip(LAT).map(|(&n, l)| rung(l, Decimal::from(n))).collect();
        let pays: Vec<bool> = nets.iter().map(|&n| n >= 0).collect();
        match break_even(&rungs) {
            BreakEven::Thin => prop_assert!(false, "30 execs each: never thin"),
            BreakEven::PaysEverywhere => prop_assert!(pays.iter().all(|&p| p)),
            BreakEven::PaysNowhere => prop_assert!(pays.iter().all(|&p| !p)),
            BreakEven::At { latency_ms, direction, not_monotone } => {
                let i = pays.windows(2).position(|w| w[0] != w[1]).unwrap();
                prop_assert!(LAT[i] <= latency_ms && latency_ms <= LAT[i + 1]);
                prop_assert_eq!(direction == Direction::LosesBeyond, pays[i]);
                let changes = pays.windows(2).filter(|w| w[0] != w[1]).count();
                prop_assert_eq!(not_monotone, changes > 1);
            }
        }
    }
}
