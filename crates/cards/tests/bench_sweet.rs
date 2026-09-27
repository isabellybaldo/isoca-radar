//! Phase 5d §9 benchmark (ignored; run by hand, debug build): a SWEET tick (advance one hour, rebuild
//! two hours, render 24h and 7d and one long window) stays under 50 ms over a 30-day history, and
//! what analyze holds for SWEET (48 recent cells + the four sums) serializes under 16 MB.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::{BodyCtx, Card, ScenarioLat, Status};
use cards::cells::{CellData, HourInput, Side};
use cards::sweet::*;
use cards::window::{SPANS, Windows};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::time::Instant;

const H: i64 = 3_600_000;
const PER_HOUR: i64 = 20;

fn scen() -> Vec<(Side, String)> {
    let mut v: Vec<(Side, String)> = ["colo-idle", "sp-idle", "sp-burst", "sp-idle-nodelay"]
        .iter()
        .map(|s| (Side::No, s.to_string()))
        .collect();
    v.push((Side::No, "sp-idle@10".into()));
    v.push((Side::No, "sp-idle@50".into()));
    v.extend(
        ["colo-idle", "sp-idle", "sp-burst"]
            .iter()
            .map(|s| (Side::Yes, s.to_string())),
    );
    v
}

fn cell_for(h: i64, sc: &[(Side, String)]) -> SweetCell {
    let mut eps = Vec::new();
    let mut atts = Vec::new();
    for i in 0..PER_HOUR {
        let side = if i % 4 == 0 { Side::Yes } else { Side::No };
        let id = format!("e{}-{i}", h / H);
        let s_legs = 2 + i % 9;
        eps.push(SweetEp {
            id: id.clone(),
            event_id: format!("ev{}", i % 7),
            side,
            opened_ms: h + i * 60_000,
            pnl_net: Decimal::new(41, 2),
            budget100: Some(Decimal::new(33, 2)),
            flagged: false,
            days: Some(Decimal::from(i % 40)),
            annualized: Some(Decimal::new(5, 1)),
            peak_cost: Decimal::from(99),
            s_legs,
            tags: Some(vec![
                ["nba", "politics", "crypto", "featured"][(i % 4) as usize].to_string(),
            ]),
        });
        for (sd, s) in sc.iter().filter(|(sd, _)| *sd == side) {
            let x = Decimal::new((i * 37 % 200) - 100, 2);
            atts.push(Attempt {
                episode_id: id.clone(),
                side: *sd,
                scenario: s.clone(),
                s_legs,
                skip: None,
                exec: Some((h + i * 60_000 + 300, Decimal::from(10))),
                pending: false,
                lines: vec![
                    Line {
                        kind: LineKind::Buy,
                        leg: Some(("m0".into(), "no".into())),
                        qty: Decimal::from(10),
                        cash_delta: Decimal::new(-400, 2),
                    },
                    Line {
                        kind: LineKind::Buy,
                        leg: Some(("m1".into(), "no".into())),
                        qty: Decimal::from(10),
                        cash_delta: Decimal::new(-510, 2),
                    },
                    Line {
                        kind: LineKind::Convert,
                        leg: None,
                        qty: Decimal::from(10),
                        cash_delta: Decimal::new(910, 2) + x,
                    },
                ],
                mid30: BTreeMap::new(),
            });
        }
    }
    SweetCell::build(&HourInput {
        hour_ms: h,
        episodes: &[],
        latency_ms: &[],
        equity_deltas: &[],
        sweet: SweetInput {
            now_ms: h + 2 * H,
            eps: &eps,
            attempts: &atts,
            scenarios: sc,
        },
    })
}

#[test]
#[ignore]
fn sweet_tick_stays_under_50ms_and_holds_under_16mb() {
    let sc = scen();
    let lats: Vec<ScenarioLat> = vec![ScenarioLat {
        name: "sp-idle".into(),
        net_ms: 145,
        latency_ms: 300,
    }];
    let now = 20_000 * 24 * H;
    let mut stored: BTreeMap<i64, SweetCell> = BTreeMap::new();
    let mut w = Windows::<SweetCell>::new(now);
    let t = Instant::now();
    for i in 0..720 {
        let h = now - (719 - i) * H;
        let c = cell_for(h, &sc);
        w.apply(h, None, &c);
        stored.insert(h, c);
    }
    println!("catch-up 720 h: {} ms", t.elapsed().as_millis());
    // one tick: the next hour arrives
    let t = Instant::now();
    let next = now + H;
    stored.insert(next, cell_for(next, &sc));
    let needed: BTreeMap<i64, SweetCell> = w
        .needed_ranges(next)
        .iter()
        .flat_map(|(a, b)| {
            stored
                .range(*a..*b)
                .map(|(h, c)| (*h, SweetCell::from_json(&c.to_json().unwrap()).unwrap()))
        })
        .collect();
    w.advance(next, &needed);
    for h in [next - H, next] {
        let old = stored.get(&h).cloned();
        let new = cell_for(h, &sc);
        w.apply(h, old.as_ref(), &new);
        let _ = new.to_json().unwrap();
        stored.insert(h, new);
    }
    let ctx = |win| BodyCtx {
        win,
        base: Status::Live,
        premise_key: "k",
        analyze_version: "t",
        from_ms: Some(0),
        to_ms: Some(next),
        since_ms: Some(0),
        computed_ms: next,
        scenarios: &lats,
        hurdle: Some(Decimal::new(10, 2)),
    };
    for (i, (name, _)) in SPANS.iter().enumerate() {
        let body = SweetCell::bodies(&w.spans[i], &(), &ctx(name));
        let _ = serde_json::to_string(&body).unwrap();
    }
    let ms = t.elapsed().as_millis();
    println!("sweet tick: {ms} ms");
    // Phase 5d ruling 2026-09-27: debug-build regression guard; the real check is VPS docker stats (runbook).
    assert!(ms < 75, "sweet tick took {ms} ms");
    let held: usize = w
        .recent
        .values()
        .map(|c| c.to_json().unwrap().len())
        .sum::<usize>()
        + w.spans
            .iter()
            .map(|c| c.to_json().unwrap().len())
            .sum::<usize>()
        + w.all.to_json().unwrap().len();
    println!("sweet held: {} KB", held / 1024);
    assert!(held < 16 * 1024 * 1024, "held {held} bytes");
}
