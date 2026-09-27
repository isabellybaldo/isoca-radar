//! Identity proofs for the single-pass statistics (Task 9 fix rounds 1–2, Review Focus 5): each
//! fused or early-exiting function must return exactly what the plain definitions return, on
//! random count maps with many ties, even and odd n, empty maps, non-ascending grids and
//! Kaplan–Meier inputs whose median is never reached.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::stats::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// Kaplan–Meier as first written (Task 5): the totals fold, then the full walk, no early exit.
fn km_reference(entries: &[(i64, (i64, i64))], grid: &[i64]) -> KmSummary {
    let (events, censored) = entries
        .iter()
        .fold((0i64, 0i64), |a, (_, v)| (a.0 + v.0, a.1 + v.1));
    let mut at_risk = events + censored;
    let mut s = FP_ONE;
    let mut at = Vec::new();
    let mut gi = 0usize;
    let mut median = None;
    for (t, (e, c)) in entries {
        while gi < grid.len() && grid[gi] < *t {
            at.push(s);
            gi += 1;
        }
        if *e > 0 && at_risk > 0 {
            s = s * i128::from(at_risk - e) / i128::from(at_risk);
            if median.is_none() && s * 2 <= FP_ONE {
                median = Some(*t);
            }
        }
        at_risk -= e + c;
    }
    while gi < grid.len() {
        at.push(s);
        gi += 1;
    }
    KmSummary {
        at,
        median,
        events,
        censored,
    }
}

/// Gebele's Ŝ(τ) as first written: one range sum per grid point.
fn emp_reference(m: &CountMap, grid: &[i64]) -> Vec<Option<Decimal>> {
    let n = total(m);
    if n <= 0 {
        return vec![None; grid.len()];
    }
    grid.iter()
        .map(|tau| {
            let above: i64 = m.range(tau.saturating_add(1)..).map(|(_, c)| c).sum();
            Some(Decimal::from(above) / Decimal::from(n))
        })
        .collect()
}

/// Values from a narrow range, so ties are the rule (several entries collapse on one key); a
/// zero count is allowed, as a window subtraction can leave one.
fn countmap() -> impl Strategy<Value = CountMap> {
    proptest::collection::vec((-5i64..40, 0i64..4), 0..40).prop_map(|v| {
        let mut m = CountMap::new();
        for (k, c) in v {
            *m.entry(k).or_insert(0) += c;
        }
        m
    })
}

/// Durations with (events, censored) counts, events below `event_max`: with `event_max` 2 an entry
/// has at most one event against up to three censored, so the median is often never reached.
fn durmap(event_max: i64) -> impl Strategy<Value = DurMap> {
    proptest::collection::vec((0i64..30, 0i64..event_max, 0i64..4), 0..40).prop_map(|v| {
        let mut m = DurMap::new();
        // (0, 0) entries allowed: a window subtraction can leave them behind
        for (t, e, c) in v {
            let x = m.entry(t).or_insert((0, 0));
            x.0 += e;
            x.1 += c;
        }
        m
    })
}

fn ascending_grid() -> impl Strategy<Value = Vec<i64>> {
    proptest::collection::vec(-6i64..45, 0..12).prop_map(|mut g| {
        g.sort_unstable();
        g
    })
}

fn entries(m: &DurMap) -> Vec<(i64, (i64, i64))> {
    m.iter().map(|(t, v)| (*t, *v)).collect()
}

fn merged(a: &DurMap, b: &DurMap) -> DurMap {
    let mut m = a.clone();
    add_durs(&mut m, b, 1);
    m
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn km_and_km_totals_equal_the_full_walk(m in prop_oneof![durmap(3), durmap(2)], grid in ascending_grid()) {
        let want = km_reference(&entries(&m), &grid);
        prop_assert_eq!(&km(m.iter().map(|(t, v)| (*t, *v)), &grid), &want);
        let (ev, ce) = dur_total(&m);
        prop_assert_eq!(&km_totals(m.iter().map(|(t, v)| (*t, *v)), ev, ce, &grid), &want);
    }

    #[test]
    fn km_totals_over_the_lazy_merge_equals_the_full_walk(a in durmap(3), b in durmap(2), grid in ascending_grid()) {
        let want = km_reference(&entries(&merged(&a, &b)), &grid);
        let (ea, ca) = dur_total(&a);
        let (eb, cb) = dur_total(&b);
        let got = km_totals(merge_sorted(&a, &b, |x, y| (x.0 + y.0, x.1 + y.1)), ea + eb, ca + cb, &grid);
        prop_assert_eq!(got, want);
    }

    #[test]
    fn km_sides_and_merged_equals_three_full_walks(
        a in prop_oneof![durmap(3), durmap(2)],
        b in prop_oneof![durmap(3), durmap(2)],
        grid in ascending_grid(),
        merged_grid in ascending_grid(),
    ) {
        let got = km_sides_and_merged(&a, &b, &grid, &merged_grid);
        prop_assert_eq!(&got[0], &km_reference(&entries(&a), &grid));
        prop_assert_eq!(&got[1], &km_reference(&entries(&b), &grid));
        prop_assert_eq!(&got[2], &km_reference(&entries(&merged(&a, &b)), &merged_grid));
    }

    #[test]
    fn emp_and_median_equals_emp_at_and_median_of(m in countmap(), grid in ascending_grid()) {
        let n = total(&m);
        let want = (emp_reference(&m, &grid), median_of(m.iter().map(|(v, c)| (*v, *c)), n));
        prop_assert_eq!(&emp_at(&m, &grid), &want.0);
        prop_assert_eq!(emp_and_median(&m, n, &grid), want);
    }

    #[test]
    fn emp_and_median_on_an_unsorted_grid(m in countmap(), grid in proptest::collection::vec(-6i64..45, 0..12)) {
        let n = total(&m);
        let want = (emp_reference(&m, &grid), median_of(m.iter().map(|(v, c)| (*v, *c)), n));
        prop_assert_eq!(&emp_at(&m, &grid), &want.0);
        prop_assert_eq!(emp_and_median(&m, n, &grid), want);
    }

    #[test]
    fn ranks_and_bins_equals_nearest_rank_twice_and_bins(
        m in countmap(),
        p in (1i64..=100, 1i64..=100),
        width in -2i64..30,
        top in 0i64..60,
    ) {
        let n = total(&m);
        let want = RanksAndBins {
            ranks: [p.0, p.1].map(|pct| nearest_rank(m.iter().map(|(v, c)| (*v, *c)), n, pct)),
            bins: bins(&m, width, top),
        };
        prop_assert_eq!(ranks_and_bins(&m, n, [p.0, p.1], width, top), want);
    }
}

#[test]
fn the_named_edge_cases() {
    let grid = [0, 1, 2, 3, 4, 5, 10];
    // median never reached: one event among many censored
    let never: DurMap = [(1, (1, 5)), (3, (0, 4)), (8, (0, 2))]
        .into_iter()
        .collect();
    let k = km(never.iter().map(|(t, v)| (*t, *v)), &grid);
    assert_eq!(k.median, None);
    assert_eq!(k, km_reference(&entries(&never), &grid));
    // even n, the middle pair straddling a tie: [2, 2, 3, 3, 3, 7] -> 3; [1, 2, 3, 4] -> 2.5
    let even: CountMap = [(2, 2), (3, 3), (7, 1)].into_iter().collect();
    assert_eq!(emp_and_median(&even, 6, &grid).1, Some(Decimal::from(3)));
    let four: CountMap = [(1, 1), (2, 1), (3, 1), (4, 1)].into_iter().collect();
    let (emp, med) = emp_and_median(&four, 4, &grid);
    assert_eq!(med, Some(Decimal::new(25, 1)));
    assert_eq!(emp, emp_reference(&four, &grid));
    // all ties on one value, and the empty map
    let ties: CountMap = [(5, 9)].into_iter().collect();
    assert_eq!(
        emp_and_median(&ties, 9, &grid),
        (emp_reference(&ties, &grid), Some(Decimal::from(5)))
    );
    assert_eq!(
        emp_and_median(&CountMap::new(), 0, &grid),
        (vec![None; grid.len()], None)
    );
    let rb = ranks_and_bins(&ties, 9, [50, 90], 2, 6);
    assert_eq!(rb.ranks, [Some(5), Some(5)]);
    assert_eq!(rb.bins, bins(&ties, 2, 6));
    let empty = ranks_and_bins(&BTreeMap::new(), 0, [50, 90], 2, 6);
    assert_eq!(empty.ranks, [None, None]);
}
