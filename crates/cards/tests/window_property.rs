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

/// `apply`'s fingerprint check trips a `debug_assert!` on a mismatch (so a debug build — every
/// build on the dev machine — fails loudly and immediately, per the project's convention), which
/// `cargo test` also runs under (`debug_assertions` is on). Exercising the mismatch path here
/// therefore has to survive that panic with `catch_unwind`, exactly as a `#[should_panic]` test
/// would, but so the state left behind (the flag, the untouched sums) can still be inspected —
/// that surviving state is what a release build (where `debug_assert!` compiles out) sees instead
/// of a panic, and is what Task 9's rebuild path reads.
fn apply_expecting_a_debug_panic(
    w: &mut Windows<G1Cell>,
    hour: i64,
    old: Option<&G1Cell>,
    new: &G1Cell,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.apply(hour, old, new)));
    assert!(
        result.is_err(),
        "a fingerprint mismatch must trip apply()'s debug_assert!"
    );
}

/// Carried Task 5 finding, fix round 1: a subtraction that removes a cell that was never actually
/// added (or removes the wrong value) would drive a count negative, and `km` would silently
/// produce a negative Ŝ. `apply`'s `old` is caller-supplied, so a caller bug could pass a phantom
/// `old` for an hour this window never added. `apply` must flag it (`needs_rebuild()`) and leave
/// every sum exactly as it was, instead of touching anything.
#[test]
fn apply_flags_a_phantom_old_for_a_never_applied_hour_and_touches_nothing() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    let hour = 100 * H; // never applied before this call
    let phantom_old = cell(999); // large, nonzero: subtracting it from zero would go negative
    let new = cell(3);

    apply_expecting_a_debug_panic(&mut w, hour, Some(&phantom_old), &new);

    assert!(
        w.needs_rebuild(),
        "a phantom `old` for a never-applied hour must be flagged"
    );
    assert_eq!(
        w.all,
        G1Cell::default(),
        "nothing must be touched once the mismatch is detected: `new` is not applied either"
    );
    for span in &w.spans {
        assert_eq!(span, &G1Cell::default());
    }

    // The window still isn't tracking `hour`, so a genuine first application (the correct
    // `old = None`) succeeds normally — the dirty flag stays set (only a rebuild clears it).
    w.apply(hour, None, &new);
    assert!(
        w.needs_rebuild(),
        "the flag is sticky until the caller rebuilds"
    );
    assert_eq!(w.all, new);
    for span in &w.spans {
        assert_eq!(span, &new);
    }
}

/// A wrong-value `old` for an hour that genuinely was applied before (e.g. a partial failure left
/// a caller's cache of the previous cell stale) must be flagged and ignored, not subtracted.
#[test]
fn apply_flags_a_wrong_value_old_for_an_already_applied_hour() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    let hour = 100 * H;
    let first = cell(3);
    w.apply(hour, None, &first);
    assert!(!w.needs_rebuild());

    let wrong_old = cell(4); // not what was actually applied (`first`)
    let second = cell(7);
    apply_expecting_a_debug_panic(&mut w, hour, Some(&wrong_old), &second);

    assert!(
        w.needs_rebuild(),
        "a wrong-value `old` must be flagged, not trusted"
    );
    assert_eq!(
        w.all, first,
        "the sums must stay exactly as they were before the mismatched call"
    );
    for span in &w.spans {
        assert_eq!(span, &first);
    }
}

/// `old = None` for an hour that already has a fingerprint on record must be flagged too: trusting
/// it would double-count (both the never-removed first value and the new one end up summed).
#[test]
fn apply_flags_a_missing_old_for_an_already_applied_hour() {
    let mut w: Windows<G1Cell> = Windows::new(100 * H);
    let hour = 100 * H;
    let first = cell(3);
    w.apply(hour, None, &first);

    let second = cell(7);
    // bug: claims this is the first application, but it isn't
    apply_expecting_a_debug_panic(&mut w, hour, None, &second);

    assert!(
        w.needs_rebuild(),
        "`old = None` for an hour that already has a fingerprint must be flagged"
    );
    assert_eq!(
        w.all, first,
        "the sums must stay exactly as they were before the mismatched call (no double-count)"
    );
    for span in &w.spans {
        assert_eq!(span, &first);
    }
}
