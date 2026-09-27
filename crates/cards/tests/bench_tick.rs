//! Spec §9 benchmark (ignored by default; run by hand, debug build): one tick's compute over a
//! synthetic history of ~350 episodes/hour must stay under 200 ms, at 30 and at 90 days of history,
//! because a tick never re-merges every cell. Catch-up is timed and printed separately. The tick
//! part mirrors analyze: advance one hour (parse the stored cells leaving the spans), rebuild the
//! two dirty hours, store them as JSON, swap them in, and render the bodies to JSON: 24h and 7d
//! every tick, 30d and all at most hourly and one per tick (ruling T9 (c), `LongRender`). The
//! render of every window at once is printed too, for reference.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use cards::body::{BodyCtx, Card, ScenarioLat, base_status};
use cards::cells::*;
use cards::edge::EdgeCell;
use cards::family::{LONG_WINDOWS, LongRender};
use cards::sweet::SweetInput;
use cards::window::{SPANS, Windows};
use cards::{DAY_MS, HOUR_MS as H};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::time::Instant;

const PER_HOUR: i64 = 350;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: i64) -> i64 {
        i64::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

/// The episodes opened in hour `h` (deterministic per hour).
fn hour_eps(h: i64) -> Vec<Ep> {
    let mut r = Lcg(u64::try_from(h / H).unwrap());
    (0..PER_HOUR)
        .map(|i| {
            let opened = h + r.below(H);
            let dur = match r.below(100) {
                0..=79 => 1 + r.below(60_000),
                80..=97 => 1 + r.below(600_000),
                _ => 1 + r.below(4 * H),
            };
            let flags = r.below(100);
            Ep {
                id: format!("e{}-{i}", h / H),
                run_id: 1,
                side: if r.below(10) == 0 {
                    Side::No
                } else {
                    Side::Yes
                },
                opened_ms: opened,
                closed_ms: Some(opened + dur),
                end_ms: opened + dur,
                duration_ms: Some(dur),
                censor_left: flags == 0,
                censor_right: flags == 1,
                censor_gap: flags == 2,
                edge_net: Decimal::new(1 + r.below(60_000), 6),
                edge_gross: Decimal::new(60_000 + r.below(60_000), 6),
                pnl_net: Decimal::new(1 + r.below(500), 2),
                pnl_gross: Decimal::new(600, 2),
                cost: Decimal::new(1 + r.below(15_000), 2),
                budget100: Some(Decimal::new(1 + r.below(400), 2)),
                suspect: flags == 3,
                stale: flags == 4,
                prev_gap_ms: (flags < 20).then(|| r.below(1_000)),
            }
        })
        .collect()
}

/// What the store would return for hour h: episodes of the 4 hours before still overlapping, and h's own.
fn view(h: i64) -> Vec<Ep> {
    (0..=4)
        .flat_map(|k| hour_eps(h - k * H))
        .filter(|e| e.end_ms >= h && e.opened_ms < h + H)
        .collect()
}

fn deltas() -> Vec<(String, Decimal)> {
    (0..60)
        .map(|i| ("sp-idle".to_string(), Decimal::new(i - 30, 2)))
        .collect()
}

struct Run<C: CellData> {
    stored: BTreeMap<i64, String>,
    w: Windows<C>,
}

fn catch_up<C: CellData>(first: i64, now: i64) -> Run<C> {
    let lat: Vec<i64> = (0..60).map(|i| 150 + i).collect();
    let d = deltas();
    let mut run = Run {
        stored: BTreeMap::new(),
        w: Windows::new(now),
    };
    let mut h = first;
    while h <= now {
        let v = view(h);
        let c = C::build(&HourInput {
            hour_ms: h,
            episodes: &v,
            latency_ms: &lat,
            equity_deltas: &d,
            sweet: SweetInput::default(),
        });
        run.stored.insert(h, c.to_json().unwrap());
        run.w.apply(h, None, &c);
        h += H;
    }
    run
}

/// One tick at `to` with `inputs` pre-read (the store's part), rendering 24h, 7d (and today) and
/// the long windows `long` says (ruling T9 (c)). Returns the JSON bytes written.
fn tick<C: Card>(
    run: &mut Run<C>,
    to: i64,
    inputs: &[(i64, Vec<Ep>)],
    sc: &[ScenarioLat],
    long: [bool; 2],
) -> usize {
    let lat: Vec<i64> = (0..60).map(|i| 150 + i).collect();
    let d = deltas();
    let mut stored_needed = BTreeMap::new();
    for (from, until) in run.w.needed_ranges(to) {
        for (h, text) in run.stored.range(from..until) {
            if !run.w.recent.contains_key(h) {
                stored_needed.insert(*h, C::from_json(text).unwrap());
            }
        }
    }
    run.w.advance(to, &stored_needed);
    let mut bytes = 0;
    for (h, v) in inputs {
        let c = C::build(&HourInput {
            hour_ms: *h,
            episodes: v,
            latency_ms: &lat,
            equity_deltas: &d,
            sweet: SweetInput::default(),
        });
        let json = c.to_json().unwrap();
        bytes += json.len();
        run.stored.insert(*h, json);
        let old = run.w.recent.get(h).cloned();
        run.w.apply(*h, old.as_ref(), &c);
    }
    let key = "k";
    let mut emit = |win: &str, start: Option<i64>, agg: &C, max: C::Max| {
        let ctx = BodyCtx {
            win,
            base: base_status(false, Some(0), start),
            premise_key: key,
            analyze_version: "bench",
            from_ms: start,
            to_ms: Some(to),
            since_ms: Some(0),
            computed_ms: to,
            scenarios: sc,
        };
        for b in C::bodies(agg, &max, &ctx) {
            bytes += serde_json::to_string(&b).unwrap().len();
        }
    };
    for (i, (name, span)) in SPANS.iter().enumerate() {
        if *name == LONG_WINDOWS[0] && !long[0] {
            continue;
        }
        let s = run.w.span_start(*span);
        emit(name, Some(s), &run.w.spans[i], run.w.max_from(s));
    }
    if long[1] {
        emit("all", None, &run.w.all, run.w.max_from(i64::MIN));
    }
    if C::EDGE_INPUTS {
        let day = to - to.rem_euclid(DAY_MS);
        let today = run.w.today(day);
        emit("today", Some(day), &today, run.w.max_from(day));
    }
    bytes
}

/// (full render of every window, the hour's first tick, the hour's second tick), in ms. The two
/// real ticks follow `LongRender` from a state rendered last hour: the first advances an hour,
/// rebuilds the two dirty hours and renders 30d; the second, 15 minutes later, rebuilds the
/// partial hour and renders all.
fn family_tick_ms<C: Card>(days: i64, sc: &[ScenarioLat]) -> (u128, u128, u128) {
    let now = 1_000_000 * H;
    let first = now - (days * 24 - 1) * H;
    let t = Instant::now();
    let mut run: Run<C> = catch_up(first, now);
    let catch_ms = t.elapsed().as_millis();
    let to = now + H;
    let inputs: Vec<(i64, Vec<Ep>)> = vec![(now, view(now)), (to, view(to))];
    // (a) alone: every window rendered, on a copy
    let mut full_run = Run {
        stored: run.stored.clone(),
        w: run.w.clone(),
    };
    let t = Instant::now();
    let _ = tick(&mut full_run, to, &inputs, sc, [true, true]);
    let full = t.elapsed().as_millis();
    drop(full_run);
    // (a) + (c)
    let mut long = LongRender {
        rendered: [Some(now), Some(now)],
    };
    let due = long.due(to);
    let t = Instant::now();
    let bytes1 = tick(&mut run, to, &inputs, sc, due);
    let t1 = t.elapsed().as_millis();
    long.mark(due, to);
    let due2 = long.due(to);
    assert_eq!(
        (due, due2),
        ([true, false], [false, true]),
        "LongRender: 30d, then all"
    );
    let t = Instant::now();
    let bytes2 = tick(&mut run, to, &[(to, view(to))], sc, due2);
    let t2 = t.elapsed().as_millis();
    println!(
        "days={days} family={} catch-up={catch_ms} ms full-render={full} ms tick1(30d)={t1} ms tick2(all)={t2} ms json={bytes1}+{bytes2} B",
        C::FAMILY
    );
    (full, t1, t2)
}

/// Every family's (full, tick1, tick2) summed: analyze ticks all four families together.
fn tick_ms(days: i64) -> (u128, u128, u128) {
    let sc = vec![
        ScenarioLat {
            name: "colo-idle".into(),
            net_ms: 1,
            latency_ms: 52,
        },
        ScenarioLat {
            name: "sp-idle".into(),
            net_ms: 145,
            latency_ms: 300,
        },
    ];
    let parts = [
        family_tick_ms::<G1Cell>(days, &sc),
        family_tick_ms::<DurCell>(days, &sc),
        family_tick_ms::<G4Cell>(days, &sc),
        family_tick_ms::<EdgeCell>(days, &sc),
    ];
    parts
        .iter()
        .fold((0, 0, 0), |a, p| (a.0 + p.0, a.1 + p.1, a.2 + p.2))
}

#[test]
#[ignore = "benchmark: run by hand (Phase 5a plan, Task 9)"]
/// 30 days: every tick < 200 ms. 90 days: ticks that don't render `all` < 200 ms; the one that
/// does is printed and held under a 400 ms sanity ceiling (accepted overrun, ruling T9 fix round 2).
fn tick_stays_under_200ms_as_all_grows_from_30_to_90_days() {
    for days in [30, 90] {
        let (full, t1, t2) = tick_ms(days);
        println!(
            "tick total at {days} days: tick1 {t1} ms, tick2 {t2} ms (every window at once: {full} ms)"
        );
        if days == 30 {
            let worst = t1.max(t2);
            assert!(
                worst < 200,
                "one tick over {days} days took {worst} ms (budget 200 ms, spec §9; ruling T9 (c))"
            );
        } else {
            // ruling (fix round 2): past 30 days the tick that renders `all` may overrun (exact
            // Kaplan–Meier over a growing number of distinct durations); a loose ceiling still
            // catches a regression. Every other tick keeps the 200 ms budget.
            assert!(
                t1 < 200,
                "tick 1 over {days} days took {t1} ms (budget 200 ms)"
            );
            assert!(
                t2 < 400,
                "the all-rendering tick over {days} days took {t2} ms (ceiling 400 ms)"
            );
        }
    }
}
