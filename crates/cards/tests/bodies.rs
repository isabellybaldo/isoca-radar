#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::*;
use cards::cells::*;
use cards::gebele::*;
use cards::stats::{CountMap, DurMap};
use serde_json::Value;

fn ctx(base: Status) -> BodyCtx<'static> {
    BodyCtx {
        win: "7d",
        base,
        premise_key: "detector_no=1+e3b0c442;detector_yes=1+e3b0c442;episode_def=1+aa;fees=1+e3b0c442",
        analyze_version: "abc1234",
        from_ms: Some(1_000 * 3_600_000),
        to_ms: Some(1_167 * 3_600_000 + 5),
        since_ms: Some(900 * 3_600_000),
        computed_ms: 1_167 * 3_600_000 + 5,
        scenarios: &[],
    }
}

fn col<'a>(b: &'a CardBody, label: &str) -> &'a Row {
    b.columns
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", b.columns))
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn g1_quotes_the_paper_and_counts_both_methods() {
    let c = G1Cell {
        their: PerSide { yes: 1_200, no: 40 },
        gross: PerSide { yes: 1_300, no: 44 },
        ours: PerSide { yes: 1_150, no: 38 },
        flagged: PerSide { yes: 50, no: 2 },
        merged_away: PerSide {
            yes: [0, 100, 150],
            no: [0, 3, 5],
        },
    };
    let b = g1(&c, &ctx(Status::Live));
    assert_eq!(
        (b.card.as_str(), b.paper.as_str(), b.status),
        ("G1", "gebele-2026", Status::Live)
    );
    let yes = col(&b, "YES-side episodes");
    assert_eq!(
        (yes.paper.clone(), yes.their.clone(), yes.ours.clone()),
        (s("2098"), s("1200"), s("1150"))
    );
    let ratio = col(&b, "YES : NO");
    assert_eq!(
        (ratio.paper.clone(), ratio.their.clone(), ratio.ours.clone()),
        (s("58.28"), s("30"), s("30.26"))
    );
    assert_eq!(col(&b, "YES, flicker-merged ≤ 250 ms").ours, s("1050"));
    assert_eq!(col(&b, "NO, flicker-merged ≤ 1000 ms").ours, s("33"));
    assert_eq!(col(&b, "NO, gross Π > 0 (context)").their, s("44"));
    assert_eq!(b.n, 1_240);
    assert_eq!(
        b.series.bars.iter().map(|x| x.value).collect::<Vec<_>>(),
        vec![1_150, 38]
    );
    assert!(
        b.notes.iter().any(|n| n.contains("52 flagged")),
        "{:?}",
        b.notes
    );
}

#[test]
fn thin_and_empty_samples_never_divide_by_zero() {
    // NO side empty, YES side small: ratios are null (not "0"), the card is thin with reasons
    let c = G1Cell {
        their: PerSide { yes: 4, no: 0 },
        ours: PerSide { yes: 4, no: 0 },
        ..G1Cell::default()
    };
    let b = g1(&c, &ctx(Status::Live));
    assert_eq!(b.status, Status::Thin);
    let ratio = col(&b, "YES : NO");
    assert_eq!((ratio.their.clone(), ratio.ours.clone()), (None, None));
    assert_eq!(
        col(&b, "NO-side episodes").ours,
        s("0"),
        "a measured zero stays \"0\""
    );
    assert!(
        b.thin_reasons
            .iter()
            .any(|r| r.starts_with("NO side: n = 0 < 10")),
        "{:?}",
        b.thin_reasons
    );
    assert!(
        b.thin_reasons
            .iter()
            .any(|r| r.starts_with("YES side: n = 4 < 10"))
    );
    // every Gebele card over an empty window
    let empty_d = DurCell::default();
    for body in [
        g2(&empty_d, &ctx(Status::Live)),
        g3(&empty_d, &ctx(Status::Live)),
        g4(&G4Cell::default(), &ctx(Status::Live)),
    ] {
        assert_eq!(body.status, Status::Thin, "{}", body.card);
        assert!(!body.thin_reasons.is_empty());
        for r in &body.columns {
            if r.unit == "s" || r.unit == "%" || r.unit == "bps" {
                assert_eq!(r.ours, None, "{} {}", body.card, r.label);
            }
        }
    }
    // warming beats thin: the window is not covered yet, and the reasons still show
    let w = g1(&c, &ctx(Status::Warming));
    assert_eq!(w.status, Status::Warming);
    assert!(!w.thin_reasons.is_empty());
}

#[test]
fn g2_medians_and_survival_curves() {
    let mut their = CountMap::new();
    let mut ours = DurMap::new();
    for i in 1..=40 {
        their.insert(i * 1_000, 1);
        ours.insert(i * 1_000, (1, 0));
    }
    ours.insert(100_000, (0, 3)); // three right-censored at 100 s
    let c = DurCell {
        their: PerSide {
            yes: their,
            no: CountMap::new(),
        },
        ours: PerSide {
            yes: ours,
            no: DurMap::new(),
        },
        left: PerSide { yes: 2, no: 1 },
        ..DurCell::default()
    };
    let b = g2(&c, &ctx(Status::Live));
    let yes = col(&b, "YES median duration");
    assert_eq!(
        (yes.paper.clone(), yes.their.clone()),
        (s("16.15"), s("20.5"))
    );
    // KM: 43 at risk; S(t_k) = (43 − k)/43; first S ≤ 1/2 at k = 22 → 22 s
    assert_eq!(yes.ours, s("22"));
    assert_eq!(col(&b, "NO median duration").paper, s("7.99"));
    assert_eq!(col(&b, "NO median duration").ours, None);
    assert_eq!(col(&b, "left-censored, excluded").ours, s("3"));
    assert_eq!(col(&b, "right-censored, kept in Kaplan–Meier").ours, s("3"));
    let km_yes = b
        .series
        .curves
        .iter()
        .find(|c| c.label.starts_with("YES · Kaplan"))
        .unwrap();
    assert_eq!(km_yes.points.len(), GRID_MS.len());
    assert_eq!(
        km_yes.points[0],
        CurvePoint {
            t_ms: 100,
            s: "1".into()
        }
    );
    assert!(
        b.notes
            .iter()
            .any(|n| n.contains("does not support a reliable distributional comparison"))
    );
    assert!(
        b.thin_reasons.iter().any(|r| r.starts_with("NO side")),
        "NO side has no durations"
    );
}

#[test]
fn g3_share_and_the_one_hour_window_bias() {
    let mut ours = DurMap::new();
    for i in 0..30 {
        ours.insert(60_000 + i, (1, 0)); // 30 short episodes
    }
    for i in 0..10 {
        ours.insert(4_000_000 + i, (1, 0)); // 10 of 50+ minutes
    }
    let mut their = CountMap::new();
    for (k, v) in &ours {
        their.insert(*k, v.0);
    }
    let mut clipped = their.clone();
    clipped.insert(3_600_000, 5);
    let c = DurCell {
        their: PerSide {
            yes: their,
            no: CountMap::new(),
        },
        ours: PerSide {
            yes: ours,
            no: DurMap::new(),
        },
        hour_observed: 624,
        hour_long: 366,
        clipped,
        flagged: PerSide { yes: 7, no: 0 },
        ..DurCell::default()
    };
    let b = g3(&c, &ctx(Status::Live));
    let share = col(&b, "long-lived share");
    assert_eq!(
        (share.paper.clone(), share.their.clone(), share.ours.clone()),
        (s("58.7"), s("58.7"), s("25"))
    );
    assert_eq!(col(&b, "episodes observed (per hour)").their, s("624"));
    assert!(col(&b, "median bias from one-hour windows").ours.is_some());
    assert_eq!(b.status, Status::Live);
    assert!(
        b.notes
            .iter()
            .any(|n| n.contains("drops 7 flagged episodes")),
        "{:?}",
        b.notes
    );
}

#[test]
fn g4_median_p90_and_bins() {
    let mut net = CountMap::new();
    for i in 1..=40 {
        net.insert(i * 250, 1); // 2.5 bp … 100 bp
    }
    net.insert(60_000, 1); // 600 bp: the open top bin
    let c = G4Cell {
        their: PerSide { yes: 45, no: 0 },
        net: PerSide {
            yes: net.clone(),
            no: CountMap::new(),
        },
        gross: PerSide {
            yes: net,
            no: CountMap::new(),
        },
        flagged: PerSide { yes: 4, no: 0 },
    };
    let b = g4(&c, &ctx(Status::Live));
    assert_eq!(col(&b, "YES positive episodes").paper, s("2098"));
    assert_eq!(col(&b, "YES positive episodes").ours, s("41"));
    assert_eq!(col(&b, "YES median net edge").ours, s("52.5")); // rank 21 of 41 → 21 · 2.5 bp
    assert_eq!(col(&b, "YES p90 net edge").ours, s("92.5")); // rank ⌈36.9⌉ = 37
    assert_eq!(col(&b, "NO median net edge").ours, None);
    let yes_bins: Vec<&Bin> = b.series.bins.iter().filter(|x| x.side == "YES").collect();
    assert_eq!(yes_bins.len(), 21);
    assert_eq!(
        (yes_bins[0].lo_bps, yes_bins[0].hi_bps, yes_bins[0].net),
        (0, Some(25), 9)
    );
    assert_eq!(
        (yes_bins[20].lo_bps, yes_bins[20].hi_bps, yes_bins[20].net),
        (500, None, 1)
    );
    assert!(b.notes.iter().any(|n| n.contains("figure only")));
    assert!(
        b.notes.iter().any(|n| n.starts_with("THEIR METHOD:")),
        "{:?}",
        b.notes
    );
    assert!(
        b.notes
            .iter()
            .any(|n| n.contains("drops 4 flagged episodes")),
        "{:?}",
        b.notes
    );
}

#[test]
fn status_precedence_and_the_hash_inputs() {
    assert_eq!(base_status(true, Some(0), Some(10)), Status::UnderReview);
    assert_eq!(base_status(false, Some(20), Some(10)), Status::Warming);
    assert_eq!(base_status(false, None, Some(10)), Status::Warming);
    assert_eq!(base_status(false, Some(10), Some(10)), Status::Live);
    assert_eq!(
        base_status(false, Some(99), None),
        Status::Live,
        "all is never warming"
    );
    assert_eq!(final_status(Status::Live, &["x".into()]), Status::Thin);
    assert_eq!(
        final_status(Status::UnderReview, &["x".into()]),
        Status::UnderReview
    );
    let h = repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "p");
    assert_eq!(h.len(), 64);
    assert_eq!(
        h,
        repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "p")
    );
    for other in [
        repro_hash("k2", "v1", Some(0), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v2", Some(0), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v1", Some(3_600_000), Some(3_600_000), "G1", "7d", "p"),
        repro_hash("k", "v1", Some(0), Some(3_600_000), "G1", "7d", "q"),
    ] {
        assert_ne!(h, other);
    }
    let b = g1(&G1Cell::default(), &ctx(Status::Live));
    // the data range enters the hash by hour
    assert_eq!(
        b.hash,
        repro_hash(
            ctx(Status::Live).premise_key,
            "abc1234",
            Some(1_000 * 3_600_000),
            Some(1_167 * 3_600_000),
            "G1",
            "7d",
            "merge_gaps_ms=0,250,1000;thin_ratio_n=10"
        )
    );
}

#[test]
fn params_strings_are_built_from_the_constants_so_a_constant_change_moves_the_hash() {
    let c = ctx(Status::Live);
    let g1 = g1(&G1Cell::default(), &c);
    let gaps: Vec<String> = MERGE_GAPS_MS.iter().map(i64::to_string).collect();
    assert_eq!(
        g1.hash,
        repro_hash(
            c.premise_key,
            c.analyze_version,
            c.from_ms,
            c.to_ms,
            "G1",
            c.win,
            &format!(
                "merge_gaps_ms={};thin_ratio_n={THIN_RATIO_N}",
                gaps.join(",")
            )
        ),
        "G1's params must be built from MERGE_GAPS_MS and THIN_RATIO_N, not a hardcoded literal"
    );

    let g3 = g3(&DurCell::default(), &c);
    assert_eq!(
        g3.hash,
        repro_hash(
            c.premise_key,
            c.analyze_version,
            c.from_ms,
            c.to_ms,
            "G3",
            c.win,
            &format!(
                "long_ms={LONG_MS};edge_min_ms={EDGE_MINUTES_MS};thin_median_n={THIN_MEDIAN_N}"
            )
        ),
        "G3's params must be built from LONG_MS, EDGE_MINUTES_MS and THIN_MEDIAN_N"
    );

    let g4 = g4(&G4Cell::default(), &c);
    assert_eq!(
        g4.hash,
        repro_hash(
            c.premise_key,
            c.analyze_version,
            c.from_ms,
            c.to_ms,
            "G4",
            c.win,
            &format!(
                "bin_bps={};top_bps={};quantum_bp=0.01;thin_n={THIN_MEDIAN_N}",
                BIN_CBPS / 100,
                TOP_CBPS / 100
            )
        ),
        "G4's params must be built from BIN_CBPS and TOP_CBPS"
    );
}

#[test]
fn bodies_carry_decimals_as_strings_or_null_never_numbers() {
    let d = DurCell::default();
    for b in G1Cell::bodies(&G1Cell::default(), &(), &ctx(Status::Live))
        .into_iter()
        .chain(DurCell::bodies(&d, &(), &ctx(Status::Live)))
        .chain(G4Cell::bodies(&G4Cell::default(), &(), &ctx(Status::Live)))
    {
        let v = serde_json::to_value(&b).unwrap();
        for r in v["columns"].as_array().unwrap() {
            for k in ["paper", "their", "ours"] {
                assert!(
                    r[k].is_string() || r[k].is_null(),
                    "{} {}: {}",
                    b.card,
                    k,
                    r[k]
                );
            }
        }
        assert_eq!(v["status"], "thin");
        let back: CardBody = serde_json::from_value(v).unwrap();
        assert_eq!(back, b);
    }
}

#[test]
fn the_papers_list_and_the_not_replicated_box() {
    let p = cards::papers::papers();
    let arr = p.as_array().unwrap();
    let find = |id: &str| arr.iter().find(|x| x["id"] == id).unwrap().clone();
    assert_eq!(find("gebele-2026")["status"], "live");
    assert_eq!(find("saguillo")["status"], "coming_later");
    assert_eq!(
        find("saguillo")["note"],
        Value::String("coming later: needs the on-chain dataset and somewhere to put it that is not your laptop".into())
    );
    assert_eq!(find("cheng")["status"], "dropped");
    let nr = cards::papers::not_replicated();
    assert_eq!(nr.as_array().unwrap().len(), 2);
    assert!(nr.to_string().contains("308,416,666"));
}
