//! Pure algebraic lemmas from the spec that don't hang off a single module.
use proptest::prelude::*;
use rust_decimal::Decimal;

fn cents(c: u32) -> Decimal {
    Decimal::new(c.into(), 2)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// L6: for random top-of-book YES bids B_i and every non-empty subset S, Gebele's
    /// convert-and-sell edge for buying NO on S and holding YES on the rest,
    /// (|S|−1) + Σ_{j∉S} B_j − Σ_{k∈S} (1 − B_k), collapses to Σ_i B_i − 1 regardless of S.
    #[test]
    fn l6_subset_collapse_is_invariant(bids in prop::collection::vec(1u32..=98, 2..=7)) {
        let n = bids.len();
        let b: Vec<Decimal> = bids.iter().map(|&c| cents(c)).collect();
        let sum_all: Decimal = b.iter().copied().sum();
        let rhs = sum_all - Decimal::ONE;

        for mask in 1u32..(1u32 << n) {
            let s_size = i64::from(mask.count_ones());
            let mut sum_in_s = Decimal::ZERO;
            let mut sum_not_s = Decimal::ZERO;
            for (i, &bi) in b.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    sum_in_s += bi;
                } else {
                    sum_not_s += bi;
                }
            }
            let lhs = Decimal::from(s_size - 1) + sum_not_s - (Decimal::from(s_size) - sum_in_s);
            prop_assert_eq!(lhs, rhs);
        }
    }
}
