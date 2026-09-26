//! Oracle: the 6-MP arm of the Freireich et al. (1963) leukemia remission trial (Blood 21:699),
//! remission in weeks, + = right-censored, as tabulated in Kleinbaum & Klein, *Survival Analysis:
//! A Self-Learning Text* (3rd ed., 2012), ch. 2, and in Klein & Moeschberger, *Survival Analysis*
//! (2nd ed., 2003), §1.2 / Table 4.1A. It has tied events (three at 6), a censored time tied with
//! events (6+, 10+) and heavy censoring. Published KM: 0.857, 0.807, 0.753, 0.690, 0.627, 0.538,
//! 0.448 at 6, 7, 10, 13, 16, 22, 23; median 23. Exact values: 6/7, 96/119, 64/85, 176/255, 32/51,
//! 64/119, 160/357. The placebo arm (no censoring) has median 8.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::stats::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

fn durmap(obs: &[(i64, bool)]) -> DurMap {
    let mut m = DurMap::new();
    for (t, event) in obs {
        let e = m.entry(*t).or_insert((0, 0));
        if *event { e.0 += 1 } else { e.1 += 1 }
    }
    m
}

fn six_mp() -> Vec<(i64, bool)> {
    let events = [6, 6, 6, 7, 10, 13, 16, 22, 23];
    let censored = [6, 9, 10, 11, 17, 19, 20, 25, 32, 32, 34, 35];
    events
        .iter()
        .map(|t| (*t, true))
        .chain(censored.iter().map(|t| (*t, false)))
        .collect()
}

fn counts(vals: &[i64]) -> CountMap {
    let mut m = CountMap::new();
    for v in vals {
        *m.entry(*v).or_insert(0) += 1;
    }
    m
}

#[test]
fn kaplan_meier_matches_the_6mp_leukemia_textbook_table() {
    let m = durmap(&six_mp());
    assert_eq!(dur_total(&m), (9, 12));
    let grid = [5, 6, 7, 10, 13, 16, 22, 23, 35];
    let s = km(m.iter().map(|(t, v)| (*t, *v)), &grid);
    let got: Vec<Decimal> = s.at.iter().map(|v| fp_to_dec(*v).round_dp(4)).collect();
    assert_eq!(
        got,
        vec![
            dec!(1),
            dec!(0.8571),
            dec!(0.8067),
            dec!(0.7529),
            dec!(0.6902),
            dec!(0.6275),
            dec!(0.5378),
            dec!(0.4482),
            dec!(0.4482)
        ]
    );
    // exact to 20 places against the fractions
    assert_eq!(
        fp_to_dec(s.at[1]).round_dp(20),
        (dec!(6) / dec!(7)).round_dp(20)
    );
    assert_eq!(
        fp_to_dec(s.at[7]).round_dp(20),
        (dec!(160) / dec!(357)).round_dp(20)
    );
    assert_eq!(s.median, Some(23));
    assert_eq!((s.events, s.censored), (9, 12));
}

#[test]
fn km_median_without_censoring_and_not_reached() {
    let placebo = [
        1, 1, 2, 2, 3, 4, 4, 5, 5, 8, 8, 8, 8, 11, 11, 12, 12, 15, 17, 22, 23,
    ];
    let m = durmap(&placebo.iter().map(|t| (*t, true)).collect::<Vec<_>>());
    assert_eq!(km(m.iter().map(|(t, v)| (*t, *v)), &[]).median, Some(8));
    assert_eq!(median_of(counts(&placebo).into_iter(), 21), Some(dec!(8)));
    // most censored early: S never falls to 1/2
    let m = durmap(&[(1, true), (2, false), (3, false), (4, false)]);
    let s = km(m.iter().map(|(t, v)| (*t, *v)), &[10]);
    assert_eq!(s.median, None);
    assert_eq!(fp_to_dec(s.at[0]), dec!(0.75));
    // empty input: S = 1 everywhere, no median, no division
    let empty = DurMap::new();
    let s = km(empty.iter().map(|(t, v)| (*t, *v)), &[1, 2]);
    assert_eq!((s.at, s.median), (vec![FP_ONE, FP_ONE], None));
}

#[test]
fn empirical_survival_is_the_share_strictly_greater() {
    let m = counts(&[1, 2, 2, 3]);
    assert_eq!(
        emp_at(&m, &[0, 1, 2, 3]),
        vec![
            Some(dec!(1)),
            Some(dec!(0.75)),
            Some(dec!(0.25)),
            Some(dec!(0))
        ]
    );
    assert_eq!(emp_at(&CountMap::new(), &[1]), vec![None]);
}

#[test]
fn sample_median_averages_the_middle_pair_and_nearest_rank_is_ceil() {
    assert_eq!(
        median_of(counts(&[1, 2, 3, 4]).into_iter(), 4),
        Some(dec!(2.5))
    );
    assert_eq!(median_of(counts(&[5, 1, 3]).into_iter(), 3), Some(dec!(3)));
    assert_eq!(median_of(CountMap::new().into_iter(), 0), None);
    let ten = counts(&(1..=10).collect::<Vec<_>>());
    assert_eq!(nearest_rank(ten.clone().into_iter(), 10, 90), Some(9));
    assert_eq!(nearest_rank(ten.clone().into_iter(), 10, 50), Some(5));
    assert_eq!(nearest_rank(ten.into_iter(), 10, 100), Some(10));
    assert_eq!(nearest_rank(counts(&[7]).into_iter(), 1, 90), Some(7));
    assert_eq!(nearest_rank(CountMap::new().into_iter(), 0, 90), None);
}

#[test]
fn bins_are_fixed_width_with_an_open_top_and_negatives_apart() {
    // centi-bps: 0.0049 → 4900 (49 bps)
    assert_eq!(centi_bps(dec!(0.0049)), 4_900);
    assert_eq!(centi_bps(dec!(0.000012345)), 12); // 0.12345 bp → 0.12 (half-even)
    assert_eq!(centi_bps(dec!(0.0000125)), 12); // 0.125 bp → 0.12 (tie to even)
    assert_eq!(centi_bps(dec!(-0.001)), -1_000);
    let m = counts(&[-5, 0, 2_499, 2_500, 4_900, 49_999, 50_000, 123_456]);
    let (below, b) = bins(&m, 2_500, 50_000);
    assert_eq!(below, 1);
    assert_eq!(b.len(), 21);
    assert_eq!(b[0], (0, 2));
    assert_eq!(b[1], (2_500, 2));
    assert_eq!(b[19], (47_500, 1));
    assert_eq!(b[20], (50_000, 2));
    assert_eq!(b.iter().map(|x| x.1).sum::<i64>() + below, 8);
}

#[test]
fn count_maps_add_and_subtract_back_to_empty() {
    let a = counts(&[1, 1, 5]);
    let mut m = CountMap::new();
    add_counts(&mut m, &a, 1);
    add_counts(&mut m, &a, 1);
    add_counts(&mut m, &a, -1);
    assert_eq!(m, a);
    add_counts(&mut m, &a, -1);
    assert!(m.is_empty(), "zero entries are removed: {m:?}");
    let d = durmap(&[(3, true), (3, false), (4, true)]);
    let mut dm = DurMap::new();
    add_durs(&mut dm, &d, 1);
    add_durs(&mut dm, &d, -1);
    assert!(dm.is_empty());
    let merged: Vec<(i64, i64)> =
        merge_sorted(&counts(&[1, 3, 3]), &counts(&[2, 3]), |x, y| x + y).collect();
    assert_eq!(merged, vec![(1, 1), (2, 1), (3, 3)]);
    assert_eq!(total(&counts(&[1, 3, 3])), 3);
}

#[test]
fn formatting_helpers_are_exact() {
    assert_eq!(secs(dec!(16150)), "16.15");
    assert_eq!(secs(dec!(7990)), "7.99");
    assert_eq!(secs(dec!(1000)), "1");
    assert_eq!(secs(dec!(4194.5)), "4.19");
    assert_eq!(pct(366, 624), Some(dec!(58.7)));
    assert_eq!(pct(1, 0), None);
    assert_eq!(dec(dec!(-0.000)), "0");
    assert_eq!(dec(dec!(0.4100)), "0.41");
    assert_eq!(cards::hour_of(3_600_000 * 5 + 1), 3_600_000 * 5);
    assert_eq!(cards::hour_of(-1), -3_600_000);
}

proptest! {
    #[test]
    fn km_agrees_with_arb_core_to_12_places(obs in proptest::collection::vec((0i64..500, any::<bool>()), 0..300)) {
        let m = durmap(&obs);
        let times: Vec<i64> = m.keys().copied().collect();
        let ours = km(m.iter().map(|(t, v)| (*t, *v)), &times);
        let theirs: BTreeMap<i64, Decimal> = arb_core::stats::kaplan_meier(&obs).into_iter().collect();
        let mut s_prev = Decimal::ONE;
        for (i, t) in times.iter().enumerate() {
            let expect = theirs.get(t).copied().unwrap_or(s_prev);
            prop_assert_eq!(fp_to_dec(ours.at[i]).round_dp(12), expect.round_dp(12), "t = {}", t);
            s_prev = expect;
        }
    }
}
