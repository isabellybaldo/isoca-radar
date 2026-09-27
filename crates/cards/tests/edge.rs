#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::HOUR_MS as H;
use cards::body::{BodyCtx, Card, ScenarioLat, Status};
use cards::cells::*;
use cards::edge::*;
use cards::window::Windows;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const M: i64 = 60_000;

fn ep(id: &str, side: Side, opened: i64, end: i64, b100: Option<Decimal>, cost: Decimal) -> Ep {
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
        edge_net: dec!(0.004),
        edge_gross: dec!(0.01),
        pnl_net: dec!(0.5),
        pnl_gross: dec!(1),
        cost,
        budget100: b100,
        suspect: false,
        stale: false,
        prev_gap_ms: None,
    }
}

fn build(h: i64, eps: &[Ep], lat: &[i64], deltas: &[(String, Decimal)]) -> EdgeCell {
    EdgeCell::build(&HourInput {
        hour_ms: h,
        episodes: eps,
        latency_ms: lat,
        equity_deltas: deltas,
    })
}

fn scenarios() -> Vec<ScenarioLat> {
    [
        ("colo-idle", 1, 52),
        ("sp-idle", 145, 300),
        ("sp-burst", 145, 450),
        ("sp-idle-nodelay", 145, 300),
    ]
    .iter()
    .map(|(n, net, lat)| ScenarioLat {
        name: n.to_string(),
        net_ms: *net,
        latency_ms: *lat,
    })
    .collect()
}

fn ctx<'a>(sc: &'a [ScenarioLat]) -> BodyCtx<'a> {
    BodyCtx {
        win: "24h",
        base: Status::Live,
        premise_key: "depth=1+e3b0c442;detector_no=1+e3b0c442;detector_yes=1+e3b0c442;episode_def=1+aa;fees=1+e3b0c442;host=1:vps-kvm2|100-250;sim=1+bb",
        analyze_version: "abc1234",
        from_ms: Some(0),
        to_ms: Some(H),
        since_ms: Some(0),
        computed_ms: H,
        scenarios: sc,
    }
}

#[test]
fn the_split_the_sums_and_the_biggest_opportunity() {
    let mut flagged = ep("f", Side::Yes, 5, 6, Some(dec!(9)), dec!(50));
    flagged.stale = true;
    let mut negative = ep("neg", Side::No, 7, 8, Some(dec!(-1)), dec!(50));
    negative.pnl_net = dec!(-0.01);
    let eps = vec![
        ep("n1", Side::No, 0, 10, Some(dec!(0.33)), dec!(99.59)),
        ep("n2", Side::No, 1, 11, Some(dec!(1.2)), dec!(40)),
        ep("y1", Side::Yes, 2, 12, Some(dec!(2.5)), dec!(100)),
        ep("y0", Side::Yes, 3, 13, None, dec!(10)), // no capped P&L: not on the table
        flagged,
        negative,
    ];
    let c = build(0, &eps, &[], &[]);
    assert_eq!((c.table.no.0, c.table.yes.0), (dec!(1.53), dec!(2.5)));
    assert_eq!((c.n.no, c.n.yes), (2, 1));
    let b = c.biggest.clone().unwrap();
    assert_eq!((b.usd.0, b.episode_id.as_str()), (dec!(2.5), "y1"));
    let json = c.to_json().unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["table"]["no"], "1.53", "money is a JSON string: {json}");
    assert_eq!(EdgeCell::from_json(&json).unwrap(), c);
}

#[test]
fn capital_overlap_counts_an_episode_opened_hours_earlier() {
    // A: 00:30 → 03:10, $80. B: 02:05 → 02:20, $150 capped to $100.
    // C closes at 02:30 exactly when D opens: never both at once.
    let eps = vec![
        ep(
            "a",
            Side::No,
            30 * M,
            3 * H + 10 * M,
            Some(dec!(0.1)),
            dec!(80),
        ),
        ep(
            "b",
            Side::No,
            2 * H + 5 * M,
            2 * H + 20 * M,
            Some(dec!(0.2)),
            dec!(150),
        ),
        ep(
            "c",
            Side::No,
            2 * H + 25 * M,
            2 * H + 30 * M,
            Some(dec!(0.1)),
            dec!(15),
        ),
        ep(
            "d",
            Side::No,
            2 * H + 30 * M,
            2 * H + 40 * M,
            Some(dec!(0.1)),
            dec!(19),
        ),
    ];
    let cells: Vec<EdgeCell> = (0..4).map(|i| build(i * H, &eps, &[], &[])).collect();
    let caps: Vec<Decimal> = cells.iter().map(|c| c.max_capital.0).collect();
    assert_eq!(caps, vec![dec!(80), dec!(80), dec!(180), dec!(80)]);
    assert_eq!(cells[2].max_part().capital, dec!(180));
    let mut w: Windows<EdgeCell> = Windows::new(3 * H);
    for (i, c) in cells.iter().enumerate() {
        w.apply(i64::try_from(i).unwrap() * H, None, c);
    }
    let m = w.max_from(0);
    assert_eq!(m.capital, dec!(180));
    assert_eq!(m.biggest.unwrap().1, "b");
    assert_eq!(w.max_from(3 * H).capital, dec!(80));
}

#[test]
fn capture_rate_is_null_when_nothing_was_on_the_table() {
    let sc = scenarios();
    let c = build(
        0,
        &[],
        &[],
        &[
            ("sp-idle".to_string(), dec!(0)),
            ("sp-idle".to_string(), dec!(0)),
        ],
    );
    assert_eq!(c.captured.get("sp-idle"), Some(&(2, Num(dec!(0)))));
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    assert_eq!(b.status, Status::Thin);
    let rate = b
        .columns
        .iter()
        .find(|r| r.label == "capture rate, sp-idle")
        .unwrap();
    assert_eq!(rate.ours, None);
    let cap = b
        .columns
        .iter()
        .find(|r| r.label == "captured at sp-idle (145 ms)")
        .unwrap();
    assert_eq!(
        cap.ours.as_deref(),
        Some("0"),
        "marks existed: a measured zero"
    );
    let on = b.columns.iter().find(|r| r.label == ON_TABLE).unwrap();
    assert_eq!(on.ours.as_deref(), Some("0"));
    assert!(
        b.thin_reasons
            .iter()
            .any(|r| r.contains("nothing was on the table"))
    );
    let colo = b
        .series
        .decay
        .iter()
        .find(|p| p.scenario == "colo-idle")
        .unwrap();
    assert_eq!(
        (colo.captured.clone(), colo.rate_pct.clone()),
        (None, None),
        "no marks: not measured"
    );
}

#[test]
fn the_edge_body_has_rows_decay_points_and_the_measured_marker() {
    let sc = scenarios();
    let eps: Vec<Ep> = (0..12)
        .map(|i| {
            let b100 = if i < 10 { dec!(1) } else { dec!(1.25) }; // Σ = 12.5
            ep(
                &format!("e{i:02}"),
                if i % 3 == 0 { Side::Yes } else { Side::No },
                i * M,
                i * M + 1_000,
                Some(b100),
                dec!(50),
            )
        })
        .collect();
    let deltas = vec![
        ("sp-idle".to_string(), dec!(1)),
        ("sp-idle".to_string(), dec!(0.25)),
        ("colo-idle".to_string(), dec!(3.1)),
    ];
    let c = build(0, &eps, &[150, 210, 200], &deltas);
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    assert_eq!(
        (b.card.as_str(), b.paper.as_str(), b.status, b.n),
        ("EDGE", "edge", Status::Live, 12)
    );
    let get = |l: &str| {
        b.columns
            .iter()
            .find(|r| r.label == l)
            .unwrap()
            .ours
            .clone()
    };
    assert_eq!(get(ON_TABLE).as_deref(), Some("12.5"));
    assert_eq!(get("biggest single opportunity").as_deref(), Some("1.25"));
    assert_eq!(get("captured at sp-idle (145 ms)").as_deref(), Some("1.25"));
    assert_eq!(get("capture rate, sp-idle").as_deref(), Some("10"));
    assert_eq!(get("capital needed at once").as_deref(), Some("50"));
    assert_eq!(get("measured latency (estimate)").as_deref(), Some("200"));
    let head: Vec<&str> = b
        .series
        .decay
        .iter()
        .filter(|p| p.headline)
        .map(|p| p.scenario.as_str())
        .collect();
    assert_eq!(head, ["sp-idle"]);
    let colo = b
        .series
        .decay
        .iter()
        .find(|p| p.scenario == "colo-idle")
        .unwrap();
    assert_eq!(
        (colo.latency_ms, colo.rate_pct.as_deref()),
        (52, Some("24.8"))
    );
    let m = b.series.measured.clone().unwrap();
    assert_eq!((m.latency_ms, m.host.as_str()), (200, "vps-kvm2"));
    assert!(b.notes.iter().any(|n| n == UPPER_BOUND));
    assert_eq!(
        b.episode_link.as_deref(),
        Some("e10"),
        "ties go to the smaller id"
    );
    assert_eq!(
        host_from_key("fees=1+a;host=1:pc-dev|lt25;sim=1+b"),
        Some("pc-dev")
    );
    assert_eq!(host_from_key("fees=1+a"), None);
}

const HOURS: i64 = 5;

fn arb_ep() -> impl Strategy<Value = Ep> {
    (
        0..HOURS * H,
        0..2 * H,
        any::<bool>(),
        0i64..300,
        1i64..200,
        any::<bool>(),
        -3i64..5,
    )
        .prop_map(|(opened, dur, yes, b100_c, cost, flag, pnl)| {
            let mut e = ep(
                &format!("e{opened}-{dur}-{b100_c}"),
                if yes { Side::Yes } else { Side::No },
                opened,
                opened + dur,
                (b100_c > 0).then(|| Decimal::new(b100_c, 2)),
                Decimal::from(cost),
            );
            e.suspect = flag;
            e.pnl_net = Decimal::new(pnl, 1);
            e
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]
    #[test]
    fn edge_cells_sum_to_the_direct_figures_and_the_max_is_a_whole_window_sweep(
        eps in proptest::collection::vec(arb_ep(), 0..40),
        deltas in proptest::collection::vec((0..HOURS, 0i64..3, -500i64..500), 0..30),
    ) {
        let names = ["colo-idle", "sp-idle", "sp-burst"];
        let mut sum = EdgeCell::default();
        let mut maxes = EdgeMax::default();
        for i in 0..HOURS {
            let h = i * H;
            let view: Vec<Ep> = eps.iter().filter(|e| e.opened_ms < h + H && e.end_ms >= h).cloned().collect();
            let d: Vec<(String, Decimal)> = deltas.iter().filter(|x| x.0 == i).map(|x| (names[usize::try_from(x.1).unwrap()].to_string(), Decimal::new(x.2, 2))).collect();
            let c = build(h, &view, &[], &d);
            EdgeCell::merge_max(&mut maxes, &c.max_part());
            sum.add(&c);
        }
        let counted: Vec<&Ep> = eps.iter().filter(|e| e.pnl_net > Decimal::ZERO && !e.suspect && e.budget100.is_some()).collect();
        let table_no: Decimal = counted.iter().filter(|e| e.side == Side::No).map(|e| e.budget100.unwrap()).sum();
        prop_assert_eq!(sum.table.no.0, table_no);
        prop_assert_eq!(sum.n.yes + sum.n.no, i64::try_from(counted.len()).unwrap());
        for (k, name) in names.iter().enumerate() {
            let mine: Vec<Decimal> = deltas.iter().filter(|x| x.1 == i64::try_from(k).unwrap()).map(|x| Decimal::new(x.2, 2)).collect();
            let got = sum.captured.get(*name).map(|(n, v)| (*n, v.0));
            let want = (!mine.is_empty()).then(|| (i64::try_from(mine.len()).unwrap(), mine.iter().copied().sum::<Decimal>()));
            prop_assert_eq!(got, want);
        }
        // the whole-window sweep: capped costs of concurrently open counted episodes
        let mut ev: Vec<(i64, u8, Decimal)> = Vec::new();
        for e in &counted {
            let (s, t) = (e.opened_ms, e.end_ms.min(HOURS * H));
            if s < t {
                let cost = e.cost.min(Decimal::from(100));
                ev.push((s, 1, cost));
                ev.push((t, 0, -cost));
            }
        }
        ev.sort_by_key(|a| (a.0, a.1));
        let (mut run, mut max) = (Decimal::ZERO, Decimal::ZERO);
        for (_, _, d) in ev {
            run += d;
            max = max.max(run);
        }
        prop_assert_eq!(maxes.capital, max);
        let best = counted.iter().map(|e| (e.budget100.unwrap(), e.id.clone())).fold(None::<(Decimal, String)>, |acc, x| match acc {
            Some(a) if a.0 > x.0 || (a.0 == x.0 && a.1 <= x.1) => Some(a),
            _ => Some(x),
        });
        prop_assert_eq!(maxes.biggest, best);
    }
}

#[test]
fn a_negative_capture_shows_a_negative_amount_and_rate() {
    let sc = scenarios();
    let eps = vec![
        ep("n1", Side::No, 0, 10, Some(dec!(2)), dec!(50)),
        ep("n2", Side::No, 1, 11, Some(dec!(1)), dec!(50)),
    ];
    // sp-idle lost money over the window; 0.3125 / 3 = 10.41666…% → -10.4
    let deltas = vec![
        ("sp-idle".to_string(), dec!(-0.5)),
        ("sp-idle".to_string(), dec!(0.1875)),
    ];
    let c = build(0, &eps, &[], &deltas);
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    let get = |l: &str| {
        b.columns
            .iter()
            .find(|r| r.label == l)
            .unwrap()
            .ours
            .clone()
    };
    assert_eq!(get(ON_TABLE).as_deref(), Some("3"));
    assert_eq!(
        get("captured at sp-idle (145 ms)").as_deref(),
        Some("-0.3125")
    );
    assert_eq!(get("capture rate, sp-idle").as_deref(), Some("-10.4"));
    let p = b
        .series
        .decay
        .iter()
        .find(|p| p.scenario == "sp-idle")
        .unwrap();
    assert_eq!(
        (p.captured.as_deref(), p.rate_pct.as_deref()),
        (Some("-0.3125"), Some("-10.4"))
    );
}

#[test]
fn the_edge_card_counts_flagged_episodes_and_discloses_rounding_and_restarts() {
    let sc = scenarios();
    let mut stale = ep("s", Side::No, 5, 6, Some(dec!(9)), dec!(50));
    stale.stale = true;
    let mut suspect = ep("u", Side::Yes, 7, 8, Some(dec!(9)), dec!(50));
    suspect.suspect = true;
    let mut negative_flagged = ep("x", Side::No, 9, 10, Some(dec!(9)), dec!(50));
    negative_flagged.stale = true;
    negative_flagged.pnl_net = dec!(-0.01); // not net-positive: never on the table, not counted as dropped
    let eps = vec![
        ep("n1", Side::No, 0, 10, Some(dec!(1)), dec!(50)),
        stale,
        suspect,
        negative_flagged,
    ];
    let c = build(0, &eps, &[], &[]);
    assert_eq!((c.flagged.no, c.flagged.yes), (1, 1));
    let json = c.to_json().unwrap();
    assert_eq!(EdgeCell::from_json(&json).unwrap(), c);
    let mut sum = EdgeCell::default();
    sum.add(&c);
    sum.add(&c);
    sum.sub(&c);
    assert_eq!(
        sum.flagged, c.flagged,
        "the flagged count adds and subtracts exactly"
    );
    let b = EdgeCell::bodies(&c, &c.max_part(), &ctx(&sc)).remove(0);
    assert!(
        b.notes
            .iter()
            .any(|n| n.contains("drops 2 flagged episodes")),
        "{:?}",
        b.notes
    );
    assert!(
        b.notes.iter().any(|n| n.contains("banker's rounding")),
        "{:?}",
        b.notes
    );
    assert!(
        b.notes.iter().any(|n| n.contains("downtime")),
        "{:?}",
        b.notes
    );
    // cells stored before the flagged count existed still load
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v.as_object_mut().unwrap().remove("flagged");
    let old = EdgeCell::from_json(&v.to_string()).unwrap();
    assert_eq!(old.flagged, PerSide::default());
}
