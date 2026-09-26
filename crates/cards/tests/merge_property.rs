//! Spec §9 merge property: for random episode sets, the sum of the hourly cells equals the direct
//! computation over the same episodes (and Kaplan–Meier on the merged cells equals arb-core's
//! independent implementation on the raw durations).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::*;
use cards::stats::{self, CountMap, DurMap};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const HOURS: i64 = 6;

fn arb_ep() -> impl Strategy<Value = Ep> {
    (
        (0..HOURS * H, 0..3 * H, any::<bool>(), any::<bool>()),
        (
            any::<[bool; 5]>(),
            -2i64..8,
            0i64..9_000,
            proptest::option::of(0i64..1_200),
        ),
    )
        .prop_map(|((opened, dur, yes, closed), (f, pnl_c, edge, gap))| {
            let end = opened + dur;
            Ep {
                id: format!("e{opened}-{dur}"),
                run_id: 1,
                side: if yes { Side::Yes } else { Side::No },
                opened_ms: opened,
                closed_ms: closed.then_some(end),
                end_ms: if closed { end } else { HOURS * H },
                duration_ms: closed.then_some(dur),
                censor_left: f[0],
                censor_right: f[1],
                censor_gap: f[2],
                edge_net: Decimal::new(edge, 6),
                edge_gross: Decimal::new(edge + 100, 6),
                pnl_net: Decimal::new(pnl_c, 2),
                pnl_gross: Decimal::new(pnl_c + 1, 2),
                cost: dec!(80),
                budget100: Some(Decimal::new(pnl_c, 2)),
                suspect: f[3],
                stale: f[4],
                prev_gap_ms: gap,
            }
        })
}

/// What the store query returns for hour h: opened before its end, still open or closed at/after its start.
fn store_view(eps: &[Ep], h: i64) -> Vec<Ep> {
    eps.iter()
        .filter(|e| e.opened_ms < h + H && e.closed_ms.is_none_or(|c| c >= h))
        .cloned()
        .collect()
}

fn summed<C: CellData>(eps: &[Ep]) -> C {
    let mut sum = C::default();
    for i in 0..HOURS {
        let h = i * H;
        let view = store_view(eps, h);
        sum.add(&C::build(&HourInput {
            hour_ms: h,
            episodes: &view,
            latency_ms: &[],
            equity_deltas: &[],
        }));
    }
    sum
}

fn side_count(eps: &[Ep], s: Side, f: impl Fn(&Ep) -> bool) -> i64 {
    i64::try_from(eps.iter().filter(|e| e.side == s && f(e)).count()).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn g1_cells_sum_to_the_direct_counts(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: G1Cell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            prop_assert_eq!(*c.their.get(s), side_count(&eps, s, |e| e.pnl_net > Decimal::ZERO));
            prop_assert_eq!(*c.gross.get(s), side_count(&eps, s, |e| e.pnl_gross > Decimal::ZERO));
            let ours = |e: &Ep| e.pnl_net > Decimal::ZERO && !e.suspect && !e.stale;
            prop_assert_eq!(*c.ours.get(s), side_count(&eps, s, ours));
            for (i, g) in MERGE_GAPS_MS.iter().enumerate() {
                let m = side_count(&eps, s, |e| ours(e) && e.prev_gap_ms.is_some_and(|p| p <= *g));
                prop_assert_eq!(c.merged_away.get(s)[i], m);
            }
        }
    }

    #[test]
    fn dur_cells_sum_to_the_direct_maps_and_km_matches_arb_core(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: DurCell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            let mut their = CountMap::new();
            let mut ours = DurMap::new();
            let mut obs = Vec::new();
            for e in eps.iter().filter(|e| e.side == s && e.pnl_net > Decimal::ZERO) {
                let Some(d) = e.duration_ms else { continue };
                if !e.censor_left && !e.censor_right && !e.censor_gap {
                    *their.entry(d).or_insert(0) += 1;
                }
                if e.censor_left || e.censor_gap || e.suspect || e.stale {
                    continue;
                }
                let slot = ours.entry(d).or_insert((0, 0));
                if e.censor_right { slot.1 += 1 } else { slot.0 += 1 }
                obs.push((d, !e.censor_right));
            }
            prop_assert_eq!(c.their.get(s), &their);
            prop_assert_eq!(c.ours.get(s), &ours);
            let times: Vec<i64> = ours.keys().copied().collect();
            let k = stats::km(c.ours.get(s).iter().map(|(t, v)| (*t, *v)), &times);
            let arb: std::collections::BTreeMap<i64, Decimal> = arb_core::stats::kaplan_meier(&obs).into_iter().collect();
            let mut prev = Decimal::ONE;
            for (i, t) in times.iter().enumerate() {
                let expect = arb.get(t).copied().unwrap_or(prev);
                prop_assert_eq!(stats::fp_to_dec(k.at[i]).round_dp(12), expect.round_dp(12));
                prev = expect;
            }
        }
        // G3, hour by hour, straight from the episodes
        let (mut observed, mut long, mut clipped) = (0i64, 0i64, CountMap::new());
        for i in 0..HOURS {
            let h = i * H;
            for e in eps.iter().filter(|e| e.pnl_net > Decimal::ZERO) {
                let (s, t) = (e.opened_ms.max(h), e.end_ms.min(h + H));
                if s >= t { continue; }
                observed += 1;
                if e.opened_ms < h + 300_000 && e.end_ms > h + H - 300_000 { long += 1; }
                if e.closed_ms.is_some() && !e.censor_left && !e.censor_right && !e.censor_gap {
                    *clipped.entry(t - s).or_insert(0) += 1;
                }
            }
        }
        prop_assert_eq!((c.hour_observed, c.hour_long), (observed, long));
        prop_assert_eq!(&c.clipped, &clipped);
    }

    #[test]
    fn g4_cells_sum_to_the_direct_edge_maps(eps in proptest::collection::vec(arb_ep(), 0..60)) {
        let c: G4Cell = summed(&eps);
        for s in [Side::Yes, Side::No] {
            let mut net = CountMap::new();
            for e in eps.iter().filter(|e| e.side == s && e.pnl_net > Decimal::ZERO && !e.suspect && !e.stale) {
                *net.entry(stats::centi_bps(e.edge_net)).or_insert(0) += 1;
            }
            prop_assert_eq!(c.net.get(s), &net);
        }
    }
}
