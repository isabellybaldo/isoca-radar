#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::cells::{CellData, HourInput, Side};
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
    s_legs: i64,
    lines: Vec<Line>,
    mids: &[(&str, &str, Decimal)],
) -> Attempt {
    Attempt {
        episode_id: "e1".into(),
        side,
        scenario: "sp-idle".into(),
        s_legs,
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
        classify_intent(None, Some(now - IN_FLIGHT_MS), false, now),
        (None, true),
        "exactly IN_FLIGHT_MS old is still pending; dropped only past it"
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
        2,
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
        3,
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
    // s_legs = 2: the episode has two legs, but the other filled 0 and wrote no ledger row.
    let a = attempt(
        Side::No,
        dec!(0),
        2,
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
        3,
        lines.clone(),
        &[("a", "yes", dec!(0.30))],
    ));
    assert_eq!(
        (priced.x, priced.x0, priced.unpriced),
        (dec!(0.10), dec!(-0.50), 0)
    );
    let unpriced = captured(&attempt(Side::Yes, dec!(10), 3, lines, &[]));
    assert_eq!(
        (unpriced.x, unpriced.x0, unpriced.unpriced),
        (dec!(-0.50), dec!(-0.50), 1)
    );
}

#[test]
fn a_yes_attempt_missing_a_whole_leg_pays_no_set_at_all() {
    // S = 3, but leg c filled 0 and wrote no ledger row at all (sim/account.rs:172-174): a and b
    // are fully bought and nothing was unwound, yet this is not two complete sets — it is zero.
    let a = attempt(
        Side::Yes,
        dec!(0),
        3,
        vec![
            buy("a", "yes", dec!(10), dec!(-3.00)),
            buy("b", "yes", dec!(10), dec!(-3.50)),
        ],
        &[("a", "yes", dec!(0.30)), ("b", "yes", dec!(0.35))],
    );
    let c = captured(&a);
    // x0 = cash_back - cash_out with no set credit (sets = 0, not min(10, 10) = 10).
    assert_eq!(c.x0, dec!(-6.50), "no set credit: leg c never filled");
    // x = x0 + leftovers at their mids: all of a and b are leftover now, priced at 0.30 and 0.35.
    assert_eq!(c.x, dec!(0.00), "x = x0 + leftovers at their mids");
    assert_eq!(c.unpriced, 0);
    assert_eq!(c.pooled, 0);
}

#[test]
fn pooled_shares_follow_each_rows_episode_and_never_count_twice() {
    // two NO episodes on the same legs in one account; the second's conversion was capped (held
    // short after a settlement took shares without an episode id): its leftover stays on it.
    let e1 = attempt(
        Side::No,
        dec!(10),
        2,
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
        2,
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
        1,
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
        "fee_bips=10,50",
        "filter_skips=annualized_too_low,no_lockup_figures,too_long",
    ] {
        assert!(p.contains(needle), "params_text lacks {needle}: {p}");
    }
    assert_eq!(key_suffix(), key_suffix(), "deterministic");
}

const T: i64 = 20_000 * 24 * H;

fn sep(id: &str, event: &str, side: Side, opened: i64) -> SweetEp {
    SweetEp {
        id: id.into(),
        event_id: event.into(),
        side,
        opened_ms: opened,
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

/// buys 10 + 10 NO for 9.10, converts 10 for 9.10 + x_cash. s_legs matches `sep`'s episode (2).
fn no_att(ep: &str, scen: &str, x_cash: Decimal) -> Attempt {
    let mut a = attempt(
        Side::No,
        dec!(10),
        2,
        vec![
            buy("m0", "no", dec!(10), dec!(-4.00)),
            buy("m1", "no", dec!(10), dec!(-5.10)),
            convert(dec!(10), dec!(9.10) + x_cash),
        ],
        &[],
    );
    a.episode_id = ep.into();
    a.scenario = scen.into();
    a
}

fn build(now: i64, eps: &[SweetEp], atts: &[Attempt], scen: &[(Side, String)]) -> SweetCell {
    SweetCell::build(&HourInput {
        hour_ms: T,
        episodes: &[],
        latency_ms: &[],
        equity_deltas: &[],
        sweet: SweetInput {
            now_ms: now,
            eps,
            attempts: atts,
            scenarios: scen,
        },
    })
}

fn scen() -> Vec<(Side, String)> {
    vec![
        (Side::No, "sp-idle".into()),
        (Side::No, "sp-idle@10".into()),
        (Side::Yes, "sp-idle".into()),
    ]
}

#[test]
fn a_cell_counts_the_table_the_attempts_and_the_clusters_per_row() {
    let eps = vec![
        sep("e1", "ev1", Side::No, T + 60_000),
        sep("e2", "ev1", Side::No, T + 120_000),
        sep("e3", "ev2", Side::No, T + 180_000),
    ];
    let atts = vec![
        no_att("e1", "sp-idle", dec!(1)),
        no_att("e2", "sp-idle", dec!(-0.5)),
        no_att("e3", "sp-idle", dec!(2)),
    ];
    let c = build(T + 2 * H, &eps, &atts, &scen());
    let all = &c.rows[&row_key(Side::No, "sp-idle", "all", "all")];
    assert_eq!(
        (all.eps, all.attempts, all.filled, all.missed),
        (3, 3, 3, 0)
    );
    assert_eq!(all.on_table.0, dec!(0.99));
    assert_eq!(all.sx.0, dec!(2.5));
    assert_eq!(all.sx2.0, dec!(5.25)); // 1 + 0.25 + 4
    assert_eq!(all.sabs.0, dec!(3.5));
    assert_eq!(all.cost.0, dec!(27.30));
    // clusters: ev1 (n=2, c=0.5) and ev2 (n=1, c=2)
    assert_eq!((all.clusters, all.sn2), (2, 5));
    assert_eq!(all.sc2.0, dec!(4.25)); // 0.25 + 4
    assert_eq!(all.scn.0, dec!(3)); // 0.5*2 + 2*1
    // the fee scenario has only the all row, on the table but with no attempts
    let fee = &c.rows[&row_key(Side::No, "sp-idle@10", "all", "all")];
    assert_eq!((fee.eps, fee.attempts), (3, 0));
    assert!(
        !c.rows
            .contains_key(&row_key(Side::No, "sp-idle@10", "days", "1-7"))
    );
    // other dimensions carry the same attempt
    assert_eq!(
        c.rows[&row_key(Side::No, "sp-idle", "days", "1-7")].attempts,
        3
    );
    assert_eq!(c.rows[&row_key(Side::No, "sp-idle", "s", "2")].attempts, 3);
    assert_eq!(
        c.rows[&row_key(Side::No, "sp-idle", "hour", "00-04")].attempts,
        3
    );
    assert_eq!(
        c.rows
            .contains_key(&row_key(Side::No, "sp-idle", "category", "sports")),
        CATEGORY_AVAILABLE
    );
    // YES scenario: no YES episode, so no YES row at all
    assert!(c.rows.keys().all(|k| !k.starts_with("yes|")));
}

#[test]
fn in_flight_pending_non_positive_and_flagged_never_become_attempts() {
    let mut flagged = sep("e4", "ev4", Side::No, T + 1);
    flagged.flagged = true;
    let mut losing = sep("e5", "ev5", Side::No, T + 2);
    losing.pnl_net = dec!(-0.01);
    let eps = vec![
        sep("e1", "ev1", Side::No, T + 1),
        sep("e2", "ev2", Side::No, T + 1),
        flagged,
        losing,
    ];
    let now = T + H;
    let mut in_flight = no_att("e1", "sp-idle", dec!(1));
    in_flight.exec = Some((now - 10_000, dec!(10)));
    let mut pending = no_att("e2", "sp-idle", dec!(1));
    pending.exec = None;
    pending.pending = true;
    let atts = vec![
        in_flight,
        pending,
        no_att("e4", "sp-idle", dec!(1)),
        no_att("e5", "sp-idle", dec!(1)),
    ];
    let c = build(now, &eps, &atts, &scen());
    let all = &c.rows[&row_key(Side::No, "sp-idle", "all", "all")];
    assert_eq!((all.eps, all.attempts, all.flagged), (2, 0, 1));
}

#[test]
fn p1_filter_skips_count_and_leave_the_eligible_table() {
    let eps = vec![
        sep("y1", "ev1", Side::Yes, T + 1),
        sep("y2", "ev2", Side::Yes, T + 2),
    ];
    let mut skip = attempt(Side::Yes, dec!(0), 2, vec![], &[]);
    skip.episode_id = "y1".into();
    skip.exec = None;
    skip.skip = Some("too_long".into());
    let mut cash = skip.clone();
    cash.episode_id = "y2".into();
    cash.skip = Some("no_cash".into());
    let c = build(T + H, &eps, &[skip, cash], &scen());
    let all = &c.rows[&row_key(Side::Yes, "sp-idle", "all", "all")];
    assert_eq!((all.attempts, all.filter_skips), (2, 1));
    assert_eq!(all.skipped.get("too_long"), Some(&1));
    assert_eq!(all.skipped.get("no_cash"), Some(&1));
    assert_eq!(
        (all.on_table.0, all.on_table_elig.0),
        (dec!(0.66), dec!(0.33))
    );
    assert_eq!((all.up_w.0, all.up_c.0), (dec!(99), dec!(198))); // 0.5 x 99 twice
}

#[test]
fn add_then_sub_is_the_identity_and_empty_rows_disappear() {
    let eps = vec![sep("e1", "ev1", Side::No, T + 1)];
    let c = build(T + H, &eps, &[no_att("e1", "sp-idle", dec!(1))], &scen());
    let mut sum = SweetCell::default();
    sum.add(&c);
    sum.add(&c);
    sum.sub(&c);
    assert_eq!(sum, c);
    sum.sub(&c);
    assert_eq!(sum, SweetCell::default());
    assert_eq!(SweetCell::from_json(&c.to_json().unwrap()).unwrap(), c);
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn sweet_keys_its_family_with_the_suffix_and_rebuilds_late_hours() {
    assert_eq!(SweetCell::FAMILY, "sweet");
    assert!(SweetCell::SWEET_INPUTS);
    assert_eq!(SweetCell::LATE_MS, LATE_MS);
    assert_eq!(SweetCell::key_suffix(), key_suffix());
    assert_eq!(cards::edge::EdgeCell::key_suffix(), "");
    assert_eq!(cards::cells::G1Cell::key_suffix(), "");
    assert_eq!(cards::edge::EdgeCell::LATE_MS, 0);
}
