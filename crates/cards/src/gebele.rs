//! Gebele, Mutzel, Matthes, "Executable Arbitrage and Market Efficiency in Prediction Markets",
//! arXiv 2608.00666 v1 (read 2026-09-26). Cards G1–G4 (spec §5): PAPER is their number, THEIR
//! METHOD their definitions on our data, OUR METHOD our headline definitions.
use crate::body::{
    Bar, Bin, BodyCtx, Card, CardBody, Curve, CurvePoint, Parts, Row, Series, make, row,
};
use crate::cells::{DurCell, G1Cell, G4Cell, LONG_MS, MERGE_GAPS_MS, Side};
use crate::stats::{
    CountMap, bins, dec, dur_total, emp_at, fp_to_dec, km, median_of, merge_sorted, nearest_rank,
    pct, secs, total,
};
use rust_decimal::Decimal;

pub const PAPER_ID: &str = "gebele-2026";
/// Spec §4: fewer than 30 uncensored durations for a median, fewer than 10 per side for a ratio.
pub const THIN_MEDIAN_N: i64 = 30;
pub const THIN_RATIO_N: i64 = 10;
/// Survival curve points (ms): 0.1 s to 24 h, roughly log-spaced.
pub const GRID_MS: [i64; 18] = [
    100, 250, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 30_000, 60_000, 120_000, 300_000, 600_000,
    1_800_000, 3_600_000, 7_200_000, 21_600_000, 86_400_000,
];
/// G4 bins: 25 bp wide in 0.01-bp units, the last one open from 500 bp.
pub const BIN_CBPS: i64 = 2_500;
pub const TOP_CBPS: i64 = 50_000;

const PAPER_SAMPLE: &str = "PAPER: CLOB sample, hour-stratified, 14 Apr 2026 00:00 UTC to 19 May 2026 06:00 UTC; \
     an episode is a maximal run with the fee-adjusted edge strictly > 0, taker execution, no flicker merge.";

fn count(v: i64) -> Option<String> {
    Some(v.to_string())
}

fn ratio(a: i64, b: i64) -> Option<String> {
    (b > 0).then(|| dec((Decimal::from(a) / Decimal::from(b)).round_dp(2)))
}

fn sides() -> [(&'static str, Side); 2] {
    [("YES", Side::Yes), ("NO", Side::No)]
}

/// `stats::bins` returns `None` only when `width <= 0`. Every call site here passes a positive
/// compile-time constant (`BIN_CBPS`), so the `None` arm can never actually run; it exists only
/// to avoid `unwrap`/`expect` (denied by clippy) while still surfacing a caller bug loudly if one
/// is ever introduced (e.g. `BIN_CBPS` changed to a non-positive value).
fn bins_or_panic(m: &CountMap, width: i64, top: i64) -> (i64, Vec<(i64, i64)>) {
    match bins(m, width, top) {
        Some(v) => v,
        None => unreachable!("cards::gebele: bins() called with non-positive width {width}"),
    }
}

pub fn g1(c: &G1Cell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    for (name, side) in sides() {
        let n = *c.ours.get(side);
        if n < THIN_RATIO_N {
            thin.push(format!(
                "{name} side: n = {n} < {THIN_RATIO_N}, so the ratio is not reliable"
            ));
        }
    }
    let mut columns = vec![
        row(
            "YES-side episodes",
            "count",
            Some("2098"),
            count(c.their.yes),
            count(c.ours.yes),
        ),
        row(
            "NO-side episodes",
            "count",
            Some("36"),
            count(c.their.no),
            count(c.ours.no),
        ),
        row(
            "YES : NO",
            "×",
            Some("58.28"),
            ratio(c.their.yes, c.their.no),
            ratio(c.ours.yes, c.ours.no),
        ),
    ];
    for (i, g) in MERGE_GAPS_MS.iter().enumerate() {
        for (name, side) in sides() {
            let merged = c.ours.get(side) - c.merged_away.get(side).get(i).copied().unwrap_or(0);
            columns.push(row(
                &format!("{name}, flicker-merged ≤ {g} ms"),
                "count",
                None,
                None,
                count(merged),
            ));
        }
    }
    for (name, side) in sides() {
        columns.push(row(
            &format!("{name}, gross Π > 0 (context)"),
            "count",
            None,
            count(*c.gross.get(side)),
            None,
        ));
    }
    let notes = vec![
        PAPER_SAMPLE.to_string(),
        format!(
            "THEIR METHOD: our raw live episodes, counted per side. OUR METHOD drops {} flagged episodes \
             (engine_suspect or stale_transport) and adds the merge-gap sensitivity.",
            c.flagged.yes + c.flagged.no
        ),
    ];
    let series = Series {
        bars: vec![
            Bar {
                label: "YES".into(),
                value: c.ours.yes,
            },
            Bar {
                label: "NO".into(),
                value: c.ours.no,
            },
        ],
        ..Series::default()
    };
    make(
        Parts {
            card: "G1",
            paper: PAPER_ID,
            title: "G1 · Side asymmetry",
            params: "merge_gaps_ms=0,250,1000;thin_ratio_n=10".into(),
            n: c.their.yes + c.their.no,
            columns,
            series,
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

fn grid_params() -> String {
    let g: Vec<String> = GRID_MS.iter().map(i64::to_string).collect();
    format!("grid_ms={};thin_median_n={THIN_MEDIAN_N}", g.join(","))
}

pub fn g2(c: &DurCell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let mut notes = vec![
        format!(
            "{PAPER_SAMPLE} Window-spanning episodes excluded; Ŝ(τ) = K⁻¹ Σ 𝟙{{D_k > τ}} over uncensored episodes. \
             On the NO side (n = 5) the paper says this “does not support a reliable distributional comparison”."
        ),
        "Their median: sample median of finished episodes. Ours: Kaplan–Meier median (first time survival ≤ 50%)."
            .to_string(),
    ];
    let mut columns: Vec<Row> = Vec::new();
    let mut curves = Vec::new();
    let mut n = 0;
    for ((name, side), (paper_med, paper_n)) in
        sides().into_iter().zip([("16.15", "253"), ("7.99", "5")])
    {
        let their = c.their.get(side);
        let ours = c.ours.get(side);
        let n_their = total(their);
        n += n_their;
        let (ev, ce) = dur_total(ours);
        let k = km(ours.iter().map(|(t, v)| (*t, *v)), &GRID_MS);
        let emp_med = median_of(their.iter().map(|(v, c)| (*v, *c)), n_their);
        if n_their < THIN_MEDIAN_N {
            thin.push(format!("{name} side: {n_their} uncensored durations < {THIN_MEDIAN_N}; THEIR median is not reliable"));
        }
        if ev < THIN_MEDIAN_N {
            thin.push(format!("{name} side: {ev} events < {THIN_MEDIAN_N}; the Kaplan–Meier median is not reliable"));
        }
        if ev > 0 && k.median.is_none() {
            notes.push(format!(
                "{name} side: the Kaplan–Meier median is not reached (survival never falls to 1/2)."
            ));
        }
        columns.push(row(
            &format!("{name} median duration"),
            "s",
            Some(paper_med),
            emp_med.map(secs),
            k.median.map(|m| secs(Decimal::from(m))),
        ));
        columns.push(row(
            &format!("{name} durations (n)"),
            "count",
            Some(paper_n),
            count(n_their),
            count(ev + ce),
        ));
        if ev + ce > 0 {
            curves.push(Curve {
                label: format!("{name} · Kaplan–Meier (ours)"),
                points: GRID_MS
                    .iter()
                    .zip(&k.at)
                    .map(|(t, v)| CurvePoint {
                        t_ms: *t,
                        s: dec(fp_to_dec(*v).round_dp(4)),
                    })
                    .collect(),
                median_ms: k.median.map(|m| m.to_string()),
            });
        }
        if n_their > 0 {
            curves.push(Curve {
                label: format!("{name} · empirical (theirs)"),
                points: GRID_MS
                    .iter()
                    .zip(emp_at(their, &GRID_MS))
                    .map(|(t, v)| CurvePoint {
                        t_ms: *t,
                        s: v.map(|x| dec(x.round_dp(4))).unwrap_or_default(),
                    })
                    .collect(),
                median_ms: emp_med.map(dec),
            });
        }
    }
    let right = dur_total(&c.ours.yes).1 + dur_total(&c.ours.no).1;
    let gap_n = total(&c.gap.yes) + total(&c.gap.no);
    let gap_med = median_of(merge_sorted(&c.gap.yes, &c.gap.no, |a, b| a + b), gap_n);
    columns.push(row(
        "right-censored, kept in Kaplan–Meier",
        "count",
        None,
        None,
        count(right),
    ));
    columns.push(row(
        "left-censored, excluded",
        "count",
        None,
        None,
        count(c.left.yes + c.left.no),
    ));
    columns.push(row(
        "gap-censored, reported apart",
        "count",
        None,
        None,
        count(gap_n),
    ));
    columns.push(row(
        "gap-censored median span (upper bound)",
        "s",
        None,
        None,
        gap_med.map(secs),
    ));
    notes.push(format!(
        "OUR METHOD: Kaplan–Meier with right-censored episodes; left-censored excluded and counted; gap-censored \
         reported apart (their recorded span is an upper bound). Drops {} flagged and {} still-open episodes.",
        c.flagged.yes + c.flagged.no,
        c.open.yes + c.open.no
    ));
    make(
        Parts {
            card: "G2",
            paper: PAPER_ID,
            title: "G2 · Durations and survival",
            params: grid_params(),
            n,
            columns,
            series: Series {
                curves,
                ..Series::default()
            },
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

pub fn g3(c: &DurCell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let their_share = pct(c.hour_long, c.hour_observed);
    if c.hour_observed == 0 {
        thin.push("no episode was observed in any hour of the window".to_string());
    }
    let all = merge_sorted(&c.ours.yes, &c.ours.no, |a, b| (a.0 + b.0, a.1 + b.1));
    let k = km(all, &[LONG_MS - 1]);
    if k.events < THIN_MEDIAN_N {
        thin.push(format!(
            "{} events < {THIN_MEDIAN_N}; the Kaplan–Meier share is not reliable",
            k.events
        ));
    }
    let ours_share = (k.events > 0)
        .then(|| {
            k.at.first()
                .map(|s| (fp_to_dec(*s) * Decimal::from(100)).round_dp(1))
        })
        .flatten();
    let clip_n = total(&c.clipped);
    let true_n = total(&c.their.yes) + total(&c.their.no);
    let clip_med = median_of(c.clipped.iter().map(|(v, n)| (*v, *n)), clip_n);
    let true_med = median_of(
        merge_sorted(&c.their.yes, &c.their.no, |a, b| a + b),
        true_n,
    );
    let bias = match (clip_med, true_med) {
        (Some(a), Some(b)) => Some(secs(a - b)),
        _ => None,
    };
    let columns = vec![
        row(
            "long-lived share",
            "%",
            Some("58.7"),
            their_share.map(dec),
            ours_share.map(dec),
        ),
        row(
            "episodes observed (per hour)",
            "count",
            Some("624"),
            count(c.hour_observed),
            None,
        ),
        row(
            "long-lived (first and last 5 min)",
            "count",
            Some("366"),
            count(c.hour_long),
            None,
        ),
        row("median bias from one-hour windows", "s", None, None, bias),
    ];
    let notes = vec![
        "PAPER: “Of the 624 observed episodes, 366 (58.7%)” were observed in both the first and the final five \
         minutes of their one-hour window (≥ 50 minutes). The paper does not state how its hours were chosen."
            .to_string(),
        "THEIR METHOD: our continuous record cut into clock hours; an episode spanning several hours is observed \
         in each."
            .to_string(),
        "OUR METHOD: the share of durations ≥ 50 min by Kaplan–Meier (right-censored kept), and the bias one-hour \
         windows introduce: the median of hour-clipped durations minus the median of true durations."
            .to_string(),
    ];
    make(
        Parts {
            card: "G3",
            paper: PAPER_ID,
            title: "G3 · Long-lived share",
            params: format!("long_ms={LONG_MS};edge_min_ms=300000;thin_median_n={THIN_MEDIAN_N}"),
            n: c.hour_observed,
            columns,
            series: Series::default(),
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

fn bps(centi: i64) -> String {
    dec(Decimal::new(centi, 2))
}

pub fn g4(c: &G4Cell, ctx: &BodyCtx<'_>) -> CardBody {
    let mut thin = Vec::new();
    let mut columns = vec![
        row(
            "YES positive episodes",
            "count",
            Some("2098"),
            count(c.their.yes),
            count(total(&c.net.yes)),
        ),
        row(
            "NO positive episodes",
            "count",
            Some("36"),
            count(c.their.no),
            count(total(&c.net.no)),
        ),
    ];
    let mut series = Series::default();
    let mut below_all = 0;
    for (name, side) in sides() {
        let net = c.net.get(side);
        let n = total(net);
        if n < THIN_MEDIAN_N {
            thin.push(format!(
                "{name} side: n = {n} < {THIN_MEDIAN_N}; the median and p90 are not reliable"
            ));
        }
        let q = |p: i64| nearest_rank(net.iter().map(|(v, k)| (*v, *k)), n, p).map(bps);
        columns.push(row(
            &format!("{name} median net edge"),
            "bps",
            None,
            None,
            q(50),
        ));
        columns.push(row(
            &format!("{name} p90 net edge"),
            "bps",
            None,
            None,
            q(90),
        ));
        let (below, net_bins) = bins_or_panic(net, BIN_CBPS, TOP_CBPS);
        let (_, gross_bins) = bins_or_panic(c.gross.get(side), BIN_CBPS, TOP_CBPS);
        below_all += below;
        for ((lo, k), (_, g)) in net_bins.into_iter().zip(gross_bins) {
            series.bins.push(Bin {
                side: name.to_string(),
                lo_bps: lo / 100,
                hi_bps: (lo < TOP_CBPS).then_some((lo + BIN_CBPS) / 100),
                net: k,
                gross: g,
            });
        }
    }
    let mut notes = vec![
        "PAPER: Figure 2 shows the distributions with insets of positive and negative counts; the text states no \
         units, bins or percentiles, so only the counts are quoted (figure only)."
            .to_string(),
        "OUR METHOD: peak top-of-book net edge per side, rounded to 0.01 bp; 25 bp bins, the last open from 500 bp; \
         gross as the overlay; nearest-rank median and p90; flagged episodes dropped."
            .to_string(),
    ];
    if below_all > 0 {
        notes.push(format!(
            "{below_all} episodes had a negative peak edge and sit below the first bin."
        ));
    }
    make(
        Parts {
            card: "G4",
            paper: PAPER_ID,
            title: "G4 · Net edge by side",
            params: format!("bin_bps=25;top_bps=500;quantum_bp=0.01;thin_n={THIN_MEDIAN_N}"),
            n: c.their.yes + c.their.no,
            columns,
            series,
            notes,
            thin,
            episode_link: None,
        },
        ctx,
    )
}

impl Card for G1Cell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g1(agg, ctx)]
    }
}

impl Card for DurCell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g2(agg, ctx), g3(agg, ctx)]
    }
}

impl Card for G4Cell {
    fn bodies(agg: &Self, _max: &Self::Max, ctx: &BodyCtx<'_>) -> Vec<CardBody> {
        vec![g4(agg, ctx)]
    }
}
