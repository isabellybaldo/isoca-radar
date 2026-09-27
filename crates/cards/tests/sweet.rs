#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::cells::Side;
use cards::sweet::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

const H: i64 = 3_600_000;

fn leg(m: &str, o: &str) -> Option<Leg> {
    Some((m.to_string(), o.to_string()))
}
fn buy(m: &str, o: &str, q: Decimal, cash: Decimal) -> Line {
    Line {
        kind: LineKind::Buy,
        leg: leg(m, o),
        qty: q,
        cash_delta: cash,
    }
}
fn sell(m: &str, o: &str, q: Decimal, cash: Decimal) -> Line {
    Line {
        kind: LineKind::Sell,
        leg: leg(m, o),
        qty: q,
        cash_delta: cash,
    }
}
fn convert(q: Decimal, cash: Decimal) -> Line {
    Line {
        kind: LineKind::Convert,
        leg: None,
        qty: q,
        cash_delta: cash,
    }
}
fn attempt(
    side: Side,
    q_eff: Decimal,
    lines: Vec<Line>,
    mids: &[(&str, &str, Decimal)],
) -> Attempt {
    Attempt {
        episode_id: "e1".into(),
        side,
        scenario: "sp-idle".into(),
        skip: None,
        exec: Some((0, q_eff)),
        pending: false,
        lines,
        mid30: mids
            .iter()
            .map(|(m, o, v)| ((m.to_string(), o.to_string()), *v))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn days_buckets_close_on_the_right_at_p1s_thirty_days() {
    assert_eq!(days_bucket(None), "unknown");
    assert_eq!(days_bucket(Some(dec!(-0.5))), "unknown");
    assert_eq!(days_bucket(Some(dec!(0))), "lt1");
    assert_eq!(days_bucket(Some(dec!(0.999))), "lt1");
    assert_eq!(days_bucket(Some(dec!(1))), "1-7");
    assert_eq!(days_bucket(Some(dec!(7))), "7-30");
    assert_eq!(
        days_bucket(Some(dec!(30))),
        "7-30",
        "P1 takes days <= 30 (policy.rs:258)"
    );
    assert_eq!(days_bucket(Some(dec!(30.0001))), "30-90");
    assert_eq!(days_bucket(Some(dec!(90))), "90-365");
    assert_eq!(days_bucket(Some(dec!(365))), "ge365");
}

#[test]
fn s_and_hour_buckets_are_half_open() {
    assert_eq!(
        [1, 2, 3, 4, 5, 6, 10, 11].map(s_bucket),
        ["2", "2", "3", "4-5", "4-5", "6-10", "6-10", "gt10"]
    );
    let day = 20_000 * 24 * H;
    assert_eq!(hour_bucket(day), "00-04");
    assert_eq!(hour_bucket(day + 4 * H - 1), "00-04");
    assert_eq!(hour_bucket(day + 4 * H), "04-08");
    assert_eq!(hour_bucket(day + 23 * H + 59 * 60_000), "20-24");
    assert_eq!(
        hour_bucket(-1),
        "20-24",
        "rem_euclid: before the epoch still lands in a day"
    );
}

#[test]
fn category_takes_the_first_bucket_in_order_then_other_then_untagged() {
    let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(category_bucket(None), UNTAGGED);
    assert_eq!(category_bucket(Some(&t(&[]))), UNTAGGED);
    assert_eq!(category_bucket(Some(&t(&["featured"]))), OTHER);
    assert_eq!(
        category_bucket(Some(&t(&["featured", "nba", "politics"]))),
        "politics",
        "bucket order wins, not tag order"
    );
    assert_eq!(category_bucket(Some(&t(&["nba"]))), "sports");
    // every slug belongs to exactly one bucket
    let mut seen = std::collections::BTreeSet::new();
    for (_, slugs) in CATEGORIES {
        for s in *slugs {
            assert!(seen.insert(*s), "{s} is in two buckets");
            assert_eq!(*s, s.to_lowercase(), "{s} must be lower-case");
        }
    }
}

#[test]
fn accounts_map_to_row_scenarios() {
    assert_eq!(
        row_scenario("P0-converter", "sp-idle", dec!(0)),
        Some((Side::No, "sp-idle".into()))
    );
    assert_eq!(
        row_scenario("P0-converter", "sp-idle", dec!(10)),
        Some((Side::No, "sp-idle@10".into()))
    );
    assert_eq!(
        row_scenario("P0-converter", "sp-idle", dec!(50)),
        Some((Side::No, "sp-idle@50".into()))
    );
    assert_eq!(
        row_scenario("P0-converter", "colo-idle", dec!(10)),
        None,
        "fee rows only at the headline scenario"
    );
    assert_eq!(
        row_scenario("P1-settlement", "sp-burst", dec!(0)),
        Some((Side::Yes, "sp-burst".into()))
    );
    assert_eq!(row_scenario("P1-settlement", "sp-idle", dec!(10)), None);
    assert_eq!(row_scenario("P3-naive", "sp-idle", dec!(0)), None);
    assert_eq!(row_scenario("P2-combo", "sp-idle", dec!(0)), None);
    assert_eq!(fee_of("sp-idle@10"), ("sp-idle".to_string(), 10));
    assert_eq!(fee_of("colo-idle"), ("colo-idle".to_string(), 0));
}

#[test]
fn intents_classify_as_skipped_pending_executed_or_dropped() {
    let now = 10 * H;
    assert_eq!(
        classify_intent(Some("no_cash".into()), None, false, now),
        (Some("no_cash".into()), false)
    );
    assert_eq!(
        classify_intent(None, Some(now - 1_000), true, now),
        (None, false)
    );
    assert_eq!(
        classify_intent(None, Some(now - 1_000), false, now),
        (None, true),
        "not executed yet"
    );
    assert_eq!(
        classify_intent(None, Some(now - IN_FLIGHT_MS - 1), false, now),
        (Some(DROPPED.into()), false),
        "a restart dropped it"
    );
    assert_eq!(
        classify_intent(None, None, false, now),
        (Some(DROPPED.into()), false)
    );
}

#[test]
fn a_no_attempt_converts_and_realizes_its_edge() {
    let a = attempt(
        Side::No,
        dec!(10),
        vec![
            buy("m0", "no", dec!(10), dec!(-4.00)),
            buy("m1", "no", dec!(10), dec!(-5.10)),
            convert(dec!(10), dec!(10.00)),
        ],
        &[],
    );
    assert_eq!(
        captured(&a),
        Captured {
            x: dec!(0.90),
            x0: dec!(0.90),
            cost: dec!(9.10),
            unpriced: 0,
            pooled: 0
        }
    );
}

#[test]
fn a_yes_attempt_counts_its_complete_sets_at_one_each() {
    let a = attempt(
        Side::Yes,
        dec!(10),
        vec![
            buy("a", "yes", dec!(10), dec!(-3.00)),
            buy("b", "yes", dec!(10), dec!(-3.50)),
            buy("c", "yes", dec!(10), dec!(-3.40)),
        ],
        &[],
    );
    let c = captured(&a);
    assert_eq!((c.x, c.cost), (dec!(0.10), dec!(9.90)));
}

#[test]
fn a_missed_attempt_with_a_partial_buy_and_unwind_is_negative() {
    // q_eff = 0: one leg bought 10, the other none; 6 unwound; 4 left, valued at the 30 s mid.
    let a = attempt(
        Side::No,
        dec!(0),
        vec![
            buy("m0", "no", dec!(10), dec!(-4.00)),
            sell("m0", "no", dec!(6), dec!(2.30)),
        ],
        &[("m0", "no", dec!(0.39))],
    );
    let c = captured(&a);
    assert_eq!(
        (c.x, c.x0, c.cost, c.unpriced),
        (dec!(-0.14), dec!(-1.70), dec!(4.00), 0)
    );
}

#[test]
fn a_yes_leftover_beyond_the_sets_is_valued_at_its_mid_or_zero_when_unpriced() {
    let lines = vec![
        buy("a", "yes", dec!(12), dec!(-3.60)),
        buy("b", "yes", dec!(10), dec!(-3.50)),
        buy("c", "yes", dec!(10), dec!(-3.40)),
    ];
    let priced = captured(&attempt(
        Side::Yes,
        dec!(10),
        lines.clone(),
        &[("a", "yes", dec!(0.30))],
    ));
    assert_eq!(
        (priced.x, priced.x0, priced.unpriced),
        (dec!(0.10), dec!(-0.50), 0)
    );
    let unpriced = captured(&attempt(Side::Yes, dec!(10), lines, &[]));
    assert_eq!(
        (unpriced.x, unpriced.x0, unpriced.unpriced),
        (dec!(-0.50), dec!(-0.50), 1)
    );
}

#[test]
fn pooled_shares_follow_each_rows_episode_and_never_count_twice() {
    // two NO episodes on the same legs in one account; the second's conversion was capped (held
    // short after a settlement took shares without an episode id): its leftover stays on it.
    let e1 = attempt(
        Side::No,
        dec!(10),
        vec![
            buy("m0", "no", dec!(10), dec!(-4.00)),
            buy("m1", "no", dec!(10), dec!(-5.10)),
            convert(dec!(10), dec!(10.00)),
        ],
        &[],
    );
    let e2 = attempt(
        Side::No,
        dec!(5),
        vec![
            buy("m0", "no", dec!(5), dec!(-2.00)),
            buy("m1", "no", dec!(5), dec!(-2.50)),
            convert(dec!(2), dec!(2.00)),
        ],
        &[("m0", "no", dec!(0.40)), ("m1", "no", dec!(0.55))],
    );
    let (c1, c2) = (captured(&e1), captured(&e2));
    assert_eq!(c1.x, dec!(0.90));
    assert_eq!(c2.x, dec!(0.35)); // -4.50 + 2.00 + 3 x 0.40 + 3 x 0.55
    // additive: the two attempts together equal one attempt holding every row (NO side is linear)
    let mut both = e2.clone();
    both.lines = e1.lines.iter().chain(&e2.lines).cloned().collect();
    assert_eq!(
        captured(&both).x,
        c1.x + c2.x,
        "attribution by episode is additive on the NO side"
    );
    // a row that sells more than the attempt bought (a data anomaly) is clamped, counted, never priced
    let odd = attempt(
        Side::No,
        dec!(5),
        vec![
            buy("m0", "no", dec!(5), dec!(-2.00)),
            sell("m0", "no", dec!(8), dec!(3.20)),
        ],
        &[("m0", "no", dec!(0.40))],
    );
    let c = captured(&odd);
    assert_eq!((c.x, c.pooled, c.unpriced), (dec!(1.20), 1, 0));
}

#[test]
fn the_key_suffix_is_a_semicolon_segment_over_every_parameter() {
    let s = key_suffix();
    assert!(s.starts_with(";sweet=1+"), "{s}");
    assert_eq!(s.len(), ";sweet=1+".len() + 8);
    let p = params_text();
    for needle in [
        "min_exec=30",
        "min_clusters=30",
        "in_flight_ms=60000",
        "late_ms=600000",
        "markout_ms=30000",
        "few_eff_n=5",
        "p1_max_days=30",
        "1:1960",
        "500:3891",
        "days=lt1,1-7,7-30,30-90,90-365,ge365,unknown",
    ] {
        assert!(p.contains(needle), "params_text lacks {needle}: {p}");
    }
    assert_eq!(key_suffix(), key_suffix(), "deterministic");
}
