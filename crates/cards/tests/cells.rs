#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const M: i64 = 60_000;

/// A closed, uncensored, positive episode.
pub fn ep(id: &str, side: Side, opened: i64, end: i64) -> Ep {
    Ep {
        id: id.into(),
        run_id: 1,
        side,
        opened_ms: opened,
        closed_ms: Some(end),
        end_ms: end,
        duration_ms: Some(end - opened),
        censor_left: false,
        censor_right: false,
        censor_gap: false,
        edge_net: dec!(0.0049),
        edge_gross: dec!(0.0137),
        pnl_net: dec!(0.41),
        pnl_gross: dec!(1.37),
        cost: dec!(99.59),
        budget100: Some(dec!(0.33)),
        suspect: false,
        stale: false,
        prev_gap_ms: None,
    }
}

fn input(hour: i64, eps: &[Ep]) -> HourInput<'_> {
    HourInput {
        hour_ms: hour,
        episodes: eps,
        latency_ms: &[],
        equity_deltas: &[],
    }
}

#[test]
fn g1_counts_positive_episodes_opened_in_the_hour_per_side() {
    let mut neg = ep("n", Side::Yes, 10, 20);
    neg.pnl_net = dec!(-0.01);
    let mut flag = ep("f", Side::No, 30, 40);
    flag.suspect = true;
    let mut close1 = ep("c1", Side::No, 50, 60);
    close1.prev_gap_ms = Some(200);
    let mut close2 = ep("c2", Side::No, 70, 80);
    close2.prev_gap_ms = Some(0);
    let eps = vec![
        ep("y1", Side::Yes, 0, 5),
        ep("y2", Side::Yes, H - 1, H + 10),
        ep("later", Side::Yes, H, H + 5), // next hour
        ep("earlier", Side::Yes, -5, 5),  // previous hour
        neg,
        flag,
        close1,
        close2,
    ];
    let c = G1Cell::build(&input(0, &eps));
    assert_eq!((c.their.yes, c.their.no), (2, 3));
    assert_eq!(
        (c.gross.yes, c.gross.no),
        (3, 3),
        "gross counts every episode with Π_gross > 0"
    );
    assert_eq!((c.ours.yes, c.ours.no), (2, 2));
    assert_eq!((c.flagged.yes, c.flagged.no), (0, 1));
    assert_eq!(
        c.merged_away.no,
        [1, 2, 2],
        "≤0 ms: c2; ≤250 ms and ≤1 s: c1 and c2"
    );
    assert_eq!(c.merged_away.yes, [0, 0, 0]);
}

#[test]
fn dur_cell_splits_uncensored_right_left_gap_flagged_and_open() {
    let mut right = ep("r", Side::Yes, 10, 1_010);
    right.censor_right = true;
    let mut left = ep("l", Side::Yes, 20, 520);
    left.censor_left = true;
    let mut gap = ep("g", Side::No, 30, 90_030);
    gap.censor_gap = true;
    let mut flag = ep("f", Side::Yes, 40, 140);
    flag.stale = true;
    let mut open = ep("o", Side::No, 50, H);
    open.closed_ms = None;
    open.duration_ms = None;
    let eps = vec![
        ep("a", Side::Yes, 0, 2_000),
        ep("b", Side::Yes, 5, 2_005),
        right,
        left,
        gap,
        flag,
        open,
    ];
    let c = DurCell::build(&input(0, &eps));
    assert_eq!(
        c.their.yes.get(&2_000),
        Some(&2),
        "a and b: uncensored, THEIR includes flagged"
    );
    assert_eq!(
        c.their.yes.get(&100),
        Some(&1),
        "the flagged one is uncensored too"
    );
    assert_eq!(c.ours.yes.get(&2_000), Some(&(2, 0)));
    assert_eq!(
        c.ours.yes.get(&1_000),
        Some(&(0, 1)),
        "right-censored enters KM as censored"
    );
    assert_eq!(c.ours.yes.get(&100), None, "OUR METHOD drops flagged");
    assert_eq!((c.left.yes, c.flagged.yes), (1, 1));
    assert_eq!(c.gap.no.get(&90_000), Some(&1));
    assert_eq!(c.open.no, 1);
}

#[test]
fn an_episode_spanning_three_hours_is_observed_in_each_hour() {
    // opened 00:30, closed 03:10: touches hours 0, 1, 2 and 3
    let long = ep("long", Side::No, 30 * M, 3 * H + 10 * M);
    let short = ep("short", Side::Yes, H + 20 * M, H + 25 * M);
    let eps = vec![long, short];
    let cells: Vec<DurCell> = (0..4)
        .map(|i| DurCell::build(&input(i * H, &eps)))
        .collect();
    assert_eq!(
        cells.iter().map(|c| c.hour_observed).collect::<Vec<_>>(),
        vec![1, 2, 1, 1]
    );
    assert_eq!(
        cells.iter().map(|c| c.hour_long).collect::<Vec<_>>(),
        vec![0, 1, 1, 0],
        "long-lived = present in the first and the last 5 minutes of the hour"
    );
    assert_eq!(cells[0].clipped.get(&(30 * M)), Some(&1));
    assert_eq!(cells[1].clipped.get(&H), Some(&1));
    assert_eq!(cells[1].clipped.get(&(5 * M)), Some(&1));
    assert_eq!(cells[3].clipped.get(&(10 * M)), Some(&1));
    assert_eq!(
        cells[0].ours.no.get(&(2 * H + 40 * M)),
        Some(&(1, 0)),
        "its duration counts once, in its opening hour"
    );
    assert!(cells[1..].iter().all(|c| c.ours.no.is_empty()));
}

#[test]
fn g4_bins_peak_edges_in_centi_bps_and_drops_flagged() {
    let mut big = ep("b", Side::Yes, 1, 2);
    big.edge_net = dec!(0.0512);
    big.edge_gross = dec!(0.06);
    let mut flag = ep("f", Side::Yes, 3, 4);
    flag.suspect = true;
    let eps = vec![ep("a", Side::Yes, 0, 1), big, flag];
    let c = G4Cell::build(&input(0, &eps));
    assert_eq!(c.their.yes, 3);
    assert_eq!(c.net.yes.get(&4_900), Some(&1));
    assert_eq!(c.net.yes.get(&51_200), Some(&1));
    assert_eq!(c.gross.yes.get(&13_700), Some(&1));
    assert_eq!(cards::stats::total(&c.net.yes), 2);
}

#[test]
fn cells_add_subtract_and_round_trip_through_json() {
    let eps = vec![ep("a", Side::Yes, 0, 2_000), ep("b", Side::No, 10, 20)];
    let a = DurCell::build(&input(0, &eps));
    let mut sum = DurCell::default();
    sum.add(&a);
    sum.add(&a);
    sum.sub(&a);
    assert_eq!(sum, a);
    sum.sub(&a);
    assert_eq!(sum, DurCell::default(), "subtraction removes zero entries");
    let json = a.to_json().unwrap();
    assert!(
        !json.contains('.'),
        "cells carry integer counts only: {json}"
    );
    assert_eq!(DurCell::from_json(&json).unwrap(), a);
    let g1 = G1Cell::build(&input(0, &eps));
    assert_eq!(G1Cell::from_json(&g1.to_json().unwrap()).unwrap(), g1);
    let g4 = G4Cell::build(&input(0, &eps));
    assert_eq!(
        G4Cell::from_json(&g4.to_json().unwrap()).unwrap(),
        g4,
        "G4Cell round-trips through JSON too"
    );
    assert_eq!(
        serde_json::to_string(&Num(dec!(0.410))).unwrap(),
        "\"0.41\"",
        "Num drops trailing zeros"
    );
    assert_eq!(
        serde_json::to_string(&Num(dec!(-0))).unwrap(),
        "\"0\"",
        "Num writes -0 as \"0\""
    );
    assert_eq!(
        serde_json::from_str::<Num>("\"0.410\"").unwrap(),
        Num(dec!(0.41)),
        "deserializing still accepts trailing zeros"
    );
    assert_eq!(
        serde_json::from_str::<Num>("\"12345678901234567.89\"").unwrap(),
        Num(dec!(12345678901234567.89))
    );
    let _: Decimal = Num::default().0;
    assert_eq!(
        (G1Cell::FAMILY, DurCell::FAMILY, G4Cell::FAMILY),
        ("g1", "dur", "g4")
    );
    assert!(DurCell::AREAS.contains(&"censoring") && !G1Cell::AREAS.contains(&"censoring"));
    assert!(G4Cell::AREAS.contains(&"depth"));
}
