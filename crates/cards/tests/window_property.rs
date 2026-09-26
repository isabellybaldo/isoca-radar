//! The incremental windows equal a direct sum of the stored cells in each window, after any mix
//! of rebuilt hours (late closes, future hours) and clock advances (spec §7, Review Focus 1 and 5).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::{CellData, G1Cell, PerSide};
use cards::window::{SPANS, Windows};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn cell(seed: i64) -> G1Cell {
    G1Cell {
        their: PerSide {
            yes: seed % 7,
            no: seed % 3,
        },
        gross: PerSide {
            yes: seed % 5,
            no: 1,
        },
        ours: PerSide {
            yes: seed % 4,
            no: seed % 2,
        },
        flagged: PerSide {
            yes: 0,
            no: seed % 2,
        },
        merged_away: PerSide {
            yes: [seed % 2, seed % 3, seed % 4],
            no: [0, 0, 1],
        },
    }
}

fn direct(stored: &BTreeMap<i64, G1Cell>, from: i64, to: i64) -> G1Cell {
    let mut c = G1Cell::default();
    for (_, v) in stored.range(from..=to) {
        c.add(v);
    }
    c
}

#[derive(Debug, Clone)]
enum Op {
    /// Rebuild the cell of (now + offset hours) with a new value (offset ≤ 0: past, > 0: future).
    Put(i64, i64),
    /// Advance the clock by n hours.
    Advance(i64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (-800i64..3, 0i64..1000).prop_map(|(o, s)| Op::Put(o, s)),
        1 => prop_oneof![1i64..3, 20i64..30, 700i64..800].prop_map(Op::Advance),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn windows_equal_the_direct_sum_after_any_ops(ops in proptest::collection::vec(op(), 1..60)) {
        let start = 1_000 * H;
        let mut stored: BTreeMap<i64, G1Cell> = BTreeMap::new();
        let mut w: Windows<G1Cell> = Windows::new(start);
        for o in ops {
            match o {
                Op::Put(off, seed) => {
                    let h = w.now_hour + off * H;
                    let new = cell(seed);
                    // the applied version of an hour ≤ now is the stored one (the analyze invariant)
                    let old = if h <= w.now_hour { stored.get(&h).cloned() } else { None };
                    w.apply(h, old.as_ref(), &new);
                    stored.insert(h, new);
                }
                Op::Advance(n) => {
                    let to = w.now_hour + n * H;
                    let mut need = BTreeMap::new();
                    for (from, until) in w.needed_ranges(to) {
                        for (h, c) in stored.range(from..until) {
                            need.insert(*h, c.clone());
                        }
                    }
                    w.advance(to, &need);
                }
            }
            for (i, (_, span)) in SPANS.iter().enumerate() {
                let from = w.now_hour - (span - 1) * H;
                prop_assert_eq!(&w.spans[i], &direct(&stored, from, w.now_hour), "span {}", span);
            }
            prop_assert_eq!(&w.all, &direct(&stored, i64::MIN, w.now_hour));
            let day = w.now_hour - 5 * H;
            prop_assert_eq!(&w.today(day), &direct(&stored, day, w.now_hour));
        }
    }
}

#[test]
fn a_clock_that_goes_back_changes_nothing() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    w.apply(100 * H, None, &cell(5));
    let before = w.all.clone();
    w.advance(90 * H, &BTreeMap::new());
    assert_eq!((w.now_hour, w.all.clone()), (100 * H, before));
    assert!(w.needed_ranges(90 * H).is_empty());
}

/// Carried Task 5 finding: a subtraction that removes a cell that was never actually added would
/// drive a count negative (and `km` would silently produce a negative Ŝ). `apply`'s `old` is
/// caller-supplied, so a caller bug could pass `Some(..)` for an hour this window never added.
/// The window must ignore that phantom `old` instead of subtracting it, in every sum it feeds.
#[test]
fn apply_ignores_a_cell_that_was_never_added() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    let hour = 100 * H; // never applied before this call
    let phantom_old = cell(999); // large, nonzero: subtracting it from zero would go negative
    let new = cell(3);

    w.apply(hour, Some(&phantom_old), &new);

    // Had `phantom_old` been subtracted, every sum below would be `new` minus a nonzero cell,
    // which for G1Cell's plain i64 counts would go negative. Instead every sum is exactly `new`.
    assert_eq!(
        w.all, new,
        "a phantom `old` for a never-applied hour must not be subtracted from `all`"
    );
    for span in &w.spans {
        assert_eq!(
            span, &new,
            "a phantom `old` for a never-applied hour must not be subtracted from a span"
        );
    }

    // A second, legitimate rebuild of the same hour behaves normally: the real previous value
    // (`new`) is subtracted and the fresh one added, since the window now genuinely tracks it.
    let rebuilt = cell(7);
    w.apply(hour, Some(&new), &rebuilt);
    assert_eq!(w.all, rebuilt);
    for span in &w.spans {
        assert_eq!(span, &rebuilt);
    }
}
