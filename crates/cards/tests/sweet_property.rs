//! Spec §9 merge property for SWEET (Task 6), in the style of `merge_property.rs`: random attempts
//! over up to 6 hours and 4 events, with random captured cash (`-2.00..=2.00` in cents) and a
//! random side-scenario pairing drawn from `scen()`.
//!
//! Merge: the sum of the per-hour cells equals a direct computation over the whole range, done
//! here by grouping attempts by row key and clusters by `(event_id, hour_of(opened_ms))`.
//! Expiry: subtracting one hour's cell from the merged sum equals the direct computation without
//! that hour.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::cells::{CellData, HourInput, Side};
use cards::hour_of;
use cards::sweet::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{BTreeMap, BTreeSet};

const HOURS: i64 = 6;
const EVENTS: i64 = 4;
/// Past every exec (0) and every hour of the range: nothing is in-flight.
const NOW: i64 = HOURS * H + IN_FLIGHT_MS + 1;

type RowStats = (
    i64,
    i64,
    i64,
    Decimal,
    Decimal,
    Decimal,
    i64,
    Decimal,
    Decimal,
    i64,
);
/// row key → (event_id, hour) → (executed n, Σx).
type ClusterMap = BTreeMap<String, BTreeMap<(String, i64), (i64, Decimal)>>;

const ZERO_STATS: RowStats = (
    0,
    0,
    0,
    Decimal::ZERO,
    Decimal::ZERO,
    Decimal::ZERO,
    0,
    Decimal::ZERO,
    Decimal::ZERO,
    0,
);

fn scen() -> Vec<(Side, String)> {
    vec![
        (Side::No, "sp-idle".into()),
        (Side::No, "sp-idle@10".into()),
        (Side::Yes, "sp-idle".into()),
    ]
}

fn sep(id: String, event_id: String, side: Side, opened_ms: i64) -> SweetEp {
    SweetEp {
        id,
        event_id,
        side,
        opened_ms,
        pnl_net: dec!(0.41),
        budget100: Some(dec!(0.33)),
        flagged: false,
        days: Some(dec!(3)),
        annualized: Some(dec!(0.5)),
        peak_cost: dec!(99),
        s_legs: 2,
        tags: Some(vec!["nba".into()]),
    }
}

fn buy_line(m: &str, o: &str, q: Decimal, cash: Decimal) -> Line {
    Line {
        kind: LineKind::Buy,
        leg: Some((m.to_string(), o.to_string())),
        qty: q,
        cash_delta: cash,
    }
}

fn convert_line(q: Decimal, cash: Decimal) -> Line {
    Line {
        kind: LineKind::Convert,
        leg: None,
        qty: q,
        cash_delta: cash,
    }
}

/// An executed attempt whose captured x is exactly `x_cash`: two legs bought for a total cost of
/// 10.00, cleared to net zero (NO: a real conversion; YES: two full legs give s_legs = 2 complete
/// sets), plus a convert line of quantity 0 that adds `x_cash` to cash without touching net or
/// sets — so varying `x_cash` alone varies the captured value.
fn att(side: Side, episode_id: &str, scenario: &str, x_cash: Decimal) -> Attempt {
    let outcome = if side == Side::Yes { "yes" } else { "no" };
    let mut lines = vec![
        buy_line("m0", outcome, dec!(10), dec!(-5.00)),
        buy_line("m1", outcome, dec!(10), dec!(-5.00)),
    ];
    match side {
        Side::No => lines.push(convert_line(dec!(10), dec!(10) + x_cash)),
        Side::Yes => lines.push(convert_line(dec!(0), x_cash)),
    }
    Attempt {
        episode_id: episode_id.to_string(),
        side,
        scenario: scenario.to_string(),
        s_legs: 2,
        skip: None,
        exec: Some((0, dec!(10))),
        pending: false,
        lines,
        mid30: BTreeMap::new(),
    }
}

fn summed(eps: &[SweetEp], atts: &[Attempt], scen: &[(Side, String)]) -> SweetCell {
    let mut sum = SweetCell::default();
    for i in 0..HOURS {
        let h = i * H;
        sum.add(&SweetCell::build(&HourInput {
            hour_ms: h,
            episodes: &[],
            latency_ms: &[],
            equity_deltas: &[],
            sweet: SweetInput {
                now_ms: NOW,
                eps,
                attempts: atts,
                scenarios: scen,
            },
        }));
    }
    sum
}

/// The direct computation: grouping attempts by row key and clusters by `(event_id, hour)`, over
/// episodes whose hour passes `hour_ok`.
fn direct(
    eps: &[SweetEp],
    atts: &[Attempt],
    scen: &[(Side, String)],
    hour_ok: impl Fn(i64) -> bool,
) -> BTreeMap<String, RowStats> {
    let mut rows: BTreeMap<String, (i64, i64, i64, Decimal, Decimal, Decimal)> = BTreeMap::new();
    let mut clus: ClusterMap = BTreeMap::new();
    for e in eps {
        let h = hour_of(e.opened_ms);
        if !hour_ok(h) || !e.positive() || e.flagged {
            continue;
        }
        for (_, scenario) in scen.iter().filter(|(sd, _)| *sd == e.side) {
            let fee_row = scenario.contains('@');
            let a = atts.iter().find(|a| {
                a.episode_id == e.id && a.scenario.as_str() == scenario.as_str() && a.side == e.side
            });
            let Some(a) = a else { continue };
            let in_flight = a.exec.is_some_and(|(t, _)| t > NOW - IN_FLIGHT_MS);
            if a.pending
                || in_flight
                || a.skip.as_deref() == Some(NOT_NET_POSITIVE)
                || a.skip.is_some()
            {
                continue;
            }
            let Some((_, q)) = a.exec else { continue };
            let cp = captured(a);
            for (dim, bucket) in e.buckets(fee_row) {
                let key = row_key(e.side, scenario, dim, bucket);
                let entry = rows.entry(key.clone()).or_insert((
                    0,
                    0,
                    0,
                    Decimal::ZERO,
                    Decimal::ZERO,
                    Decimal::ZERO,
                ));
                entry.0 += 1;
                if q > Decimal::ZERO {
                    entry.1 += 1;
                } else {
                    entry.2 += 1;
                }
                entry.3 += cp.x;
                entry.4 += cp.x * cp.x;
                entry.5 += cp.x.abs();
                let cl = clus
                    .entry(key)
                    .or_default()
                    .entry((e.event_id.clone(), h))
                    .or_insert((0, Decimal::ZERO));
                cl.0 += 1;
                cl.1 += cp.x;
            }
        }
    }
    rows.into_iter()
        .map(|(key, (attempts, filled, missed, sx, sx2, sabs))| {
            let (mut clusters, mut sc2, mut scn, mut sn2) =
                (0i64, Decimal::ZERO, Decimal::ZERO, 0i64);
            if let Some(groups) = clus.get(&key) {
                for (n, cx) in groups.values() {
                    clusters += 1;
                    sc2 += *cx * *cx;
                    scn += *cx * Decimal::from(*n);
                    sn2 += n * n;
                }
            }
            (
                key,
                (
                    attempts, filled, missed, sx, sx2, sabs, clusters, sc2, scn, sn2,
                ),
            )
        })
        .collect()
}

fn actual_stats(c: &SweetCell, key: &str) -> RowStats {
    match c.rows.get(key) {
        Some(a) => (
            a.attempts, a.filled, a.missed, a.sx.0, a.sx2.0, a.sabs.0, a.clusters, a.sc2.0,
            a.scn.0, a.sn2,
        ),
        None => ZERO_STATS,
    }
}

fn build_case(
    cases: &[(i64, i64, i64, usize, i64)],
    scen: &[(Side, String)],
) -> (Vec<SweetEp>, Vec<Attempt>) {
    let mut eps = Vec::new();
    let mut atts = Vec::new();
    for (i, (event_idx, hour, offset, scen_idx, cents)) in cases.iter().enumerate() {
        let id = format!("e{i}");
        let event_id = format!("ev{event_idx}");
        let (side, scenario) = &scen[*scen_idx];
        let opened_ms = hour * H + offset;
        eps.push(sep(id.clone(), event_id, *side, opened_ms));
        atts.push(att(*side, &id, scenario, Decimal::new(*cents, 2)));
    }
    (eps, atts)
}

fn assert_rows_match(merged: &SweetCell, expected: &BTreeMap<String, RowStats>) {
    let mut keys: BTreeSet<String> = merged.rows.keys().cloned().collect();
    keys.extend(expected.keys().cloned());
    for k in keys {
        assert_eq!(
            actual_stats(merged, &k),
            *expected.get(&k).unwrap_or(&ZERO_STATS),
            "row {k}"
        );
    }
}

fn arb_case() -> impl Strategy<Value = (i64, i64, i64, usize, i64)> {
    (0..EVENTS, 0..HOURS, 0..H, 0..3usize, -200i64..=200i64)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn sweet_cells_merge_to_the_direct_computation(cases in proptest::collection::vec(arb_case(), 0..40)) {
        let scen = scen();
        let (eps, atts) = build_case(&cases, &scen);
        let merged = summed(&eps, &atts, &scen);
        let expected = direct(&eps, &atts, &scen, |_| true);
        assert_rows_match(&merged, &expected);
    }

    #[test]
    fn sweet_cell_expiry_subtracts_exactly(
        cases in proptest::collection::vec(arb_case(), 0..40),
        drop_hour in 0..HOURS,
    ) {
        let scen = scen();
        let (eps, atts) = build_case(&cases, &scen);
        let mut merged = summed(&eps, &atts, &scen);
        let dropped = SweetCell::build(&HourInput {
            hour_ms: drop_hour * H,
            episodes: &[],
            latency_ms: &[],
            equity_deltas: &[],
            sweet: SweetInput {
                now_ms: NOW,
                eps: &eps,
                attempts: &atts,
                scenarios: &scen,
            },
        });
        merged.sub(&dropped);
        let expected = direct(&eps, &atts, &scen, |h| h != drop_hour * H);
        assert_rows_match(&merged, &expected);
    }
}
