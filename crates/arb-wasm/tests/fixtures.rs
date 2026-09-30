//! Golden fixtures (09-27 spec §4.3, 7b §3.4). The cases live here; `fixtures/<name>_json.json`
//! holds `[{ "name", "input", "output" }]` with `input` and `output` as the exact strings passed and
//! returned, so the web's parity test (plan 7b-B) runs the same files through the WASM build and
//! compares byte for byte. `UPDATE_FIXTURES=1` rewrites the files from these cases; a reviewer
//! reads the diff. Without it, a file whose names, inputs or outputs differ from here fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{
    basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;

#[derive(Serialize, Deserialize)]
struct Case {
    name: String,
    input: String,
    output: String,
}

/// `s` padded with trailing spaces (valid JSON whitespace) to exactly `len` bytes.
fn pad(s: &str, len: usize) -> String {
    assert!(s.len() <= len, "{} > {len}", s.len());
    format!("{s}{}", " ".repeat(len - s.len()))
}

fn check(func: &str, f: fn(&str) -> String, cases: Vec<(&str, String)>) {
    let path = format!("{}/fixtures/{func}.json", env!("CARGO_MANIFEST_DIR"));
    let want: Vec<Case> = cases
        .into_iter()
        .map(|(name, input)| Case {
            name: name.to_string(),
            output: f(&input),
            input,
        })
        .collect();
    if std::env::var("UPDATE_FIXTURES").is_ok_and(|v| v == "1") {
        fs::write(&path, serde_json::to_string_pretty(&want).unwrap() + "\n").unwrap();
        return;
    }
    let have: Vec<Case> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let names = |v: &[Case]| v.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&have),
        names(&want),
        "{func}: case names (run with UPDATE_FIXTURES=1)"
    );
    for (h, w) in have.iter().zip(&want) {
        assert_eq!(
            h.input, w.input,
            "{func}/{}: the fixture's input is not this test's",
            w.name
        );
        assert_eq!(h.output, w.output, "{func}/{}: output drifted", w.name);
    }
}

fn lv(p: &str, s: &str) -> Value {
    json!({ "p": p, "s": s })
}

// ---- mirror ----
fn fed_bids() -> Value {
    json!([
        lv("0.54", "5603.74"),
        lv("0.53", "85887.16"),
        lv("0.52", "42239.53"),
        lv("0.51", "28133.09")
    ])
}

fn fed_asks() -> Value {
    json!([
        lv("0.55", "32356.03"),
        lv("0.56", "252281.27"),
        lv("0.57", "23963.36"),
        lv("0.58", "5637.30")
    ])
}

fn mirror_in(bids: Value, asks: Value) -> String {
    json!({ "bids": bids, "asks": asks }).to_string()
}

fn eight(p0: u32) -> Value {
    Value::Array(
        (0..8)
            .map(|i| lv(&format!("0.{:02}", p0 + i), "1"))
            .collect(),
    )
}

#[test]
fn mirror() {
    let fed = mirror_in(fed_bids(), fed_asks());
    check(
        "mirror_json",
        mirror_json,
        vec![
            ("fed-book", fed.clone()),
            (
                "crossed",
                mirror_in(json!([lv("0.56", "10")]), json!([lv("0.55", "10")])),
            ),
            ("bids-only", mirror_in(json!([lv("0.54", "10")]), json!([]))),
            ("empty", mirror_in(json!([]), json!([]))),
            (
                "json-number-price",
                r#"{"bids":[{"p":0.55,"s":"1"}],"asks":[]}"#.to_string(),
            ),
            (
                "unknown-field",
                r#"{"bids":[],"asks":[],"depth":3}"#.to_string(),
            ),
            ("off-grid", mirror_in(json!([lv("0.555", "1")]), json!([]))),
            ("price-0.01", mirror_in(json!([lv("0.01", "1")]), json!([]))),
            ("price-0.00", mirror_in(json!([lv("0.00", "1")]), json!([]))),
            ("price-0.99", mirror_in(json!([]), json!([lv("0.99", "1")]))),
            ("price-1.00", mirror_in(json!([]), json!([lv("1.00", "1")]))),
            (
                "duplicate-price",
                mirror_in(json!([lv("0.50", "1"), lv("0.50", "2")]), json!([])),
            ),
            ("size-zero", mirror_in(json!([lv("0.50", "0")]), json!([]))),
            (
                "size-1000000",
                mirror_in(json!([lv("0.50", "1000000")]), json!([])),
            ),
            (
                "size-1000001",
                mirror_in(json!([lv("0.50", "1000001")]), json!([])),
            ),
            (
                "size-7dp",
                mirror_in(json!([lv("0.50", "0.0000001")]), json!([])),
            ),
            ("eight-levels", mirror_in(eight(10), json!([]))),
            (
                "nine-levels",
                mirror_in(
                    json!([
                        lv("0.01", "1"),
                        lv("0.02", "1"),
                        lv("0.03", "1"),
                        lv("0.04", "1"),
                        lv("0.05", "1"),
                        lv("0.06", "1"),
                        lv("0.07", "1"),
                        lv("0.08", "1"),
                        lv("0.09", "1")
                    ]),
                    json!([]),
                ),
            ),
            ("input-16384", pad(&fed, 16 * 1024)),
            ("input-16385", pad(&fed, 16 * 1024 + 1)),
        ],
    );
}

// ---- payoff ----
fn pr(bid: &str, ask: &str) -> Value {
    json!({ "bid": bid, "ask": ask })
}

fn payoff_in(kind: &str, s: u32, other_listed: bool, prices: Value) -> String {
    json!({ "kind": kind, "s": s, "other_listed": other_listed, "prices": prices }).to_string()
}

#[test]
fn payoff() {
    check(
        "payoff_json",
        payoff_json,
        vec![
            ("yes-3-other", payoff_in("yes", 3, false, Value::Null)),
            ("yes-3-listed", payoff_in("yes", 3, true, Value::Null)),
            ("no-3-other", payoff_in("no", 3, false, Value::Null)),
            (
                "no-2-listed-prices",
                payoff_in(
                    "no",
                    2,
                    true,
                    json!([pr("0.60", "0.61"), pr("0.50", "0.51")]),
                ),
            ),
            (
                "yes-2-prices",
                payoff_in(
                    "yes",
                    2,
                    true,
                    json!([pr("0.40", "0.45"), pr("0.50", "0.52")]),
                ),
            ),
            (
                "implication-prices",
                payoff_in(
                    "implication",
                    2,
                    true,
                    json!([pr("0.70", "0.72"), pr("0.40", "0.42")]),
                ),
            ),
            ("yes-s1", payoff_in("yes", 1, true, Value::Null)),
            ("yes-s2", payoff_in("yes", 2, false, Value::Null)),
            ("no-s6", payoff_in("no", 6, false, Value::Null)),
            ("no-s7", payoff_in("no", 7, false, Value::Null)),
            (
                "prices-wrong-count",
                payoff_in(
                    "no",
                    3,
                    true,
                    json!([pr("0.60", "0.61"), pr("0.50", "0.51")]),
                ),
            ),
            ("bad-kind", payoff_in("maybe", 3, true, Value::Null)),
        ],
    );
}

// ---- classify ----
fn leg(status: &str, bid: Option<&str>, ask: Option<&str>, fee: Value) -> Value {
    let side = |p: Option<&str>| p.map_or_else(|| json!([]), |p| json!([lv(p, "10")]));
    json!({ "status": status, "bids": side(bid), "asks": side(ask), "fee": fee })
}

fn live(bid: &str, ask: &str) -> Value {
    leg("live", Some(bid), Some(ask), Value::Null)
}

fn fee7() -> Value {
    json!({ "rate": "0.07", "exponent": 1 })
}

fn classify_in(augmented: bool, legs: Vec<Value>) -> String {
    json!({ "augmented": augmented, "legs": legs }).to_string()
}

fn no_edge_3() -> Vec<Value> {
    vec![
        live("0.40", "0.41"),
        live("0.35", "0.36"),
        live("0.30", "0.31"),
    ]
}

#[test]
fn classify() {
    let fees_on: Vec<Value> = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")]
        .iter()
        .map(|(b, a)| leg("live", Some(b), Some(a), fee7()))
        .collect();
    check(
        "classify_json",
        classify_json,
        vec![
            ("no-edge-3", classify_in(false, no_edge_3())),
            (
                "yes-incomplete-other",
                classify_in(
                    false,
                    vec![
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                        leg("other", None, None, Value::Null),
                    ],
                ),
            ),
            (
                "decided",
                classify_in(
                    false,
                    vec![
                        leg("closed_yes", None, None, Value::Null),
                        live("0.20", "0.30"),
                    ],
                ),
            ),
            (
                "missing-ask",
                classify_in(
                    false,
                    vec![
                        live("0.20", "0.30"),
                        leg("live", Some("0.20"), None, Value::Null),
                    ],
                ),
            ),
            (
                "crossed-leg",
                classify_in(false, vec![live("0.56", "0.55"), live("0.30", "0.31")]),
            ),
            (
                "nothing",
                classify_in(false, vec![live("0.40", "0.52"), live("0.40", "0.52")]),
            ),
            ("fees-on", classify_in(false, fees_on)),
            (
                "augmented-complete",
                classify_in(
                    true,
                    vec![
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                        live("0.20", "0.30"),
                    ],
                ),
            ),
            (
                "six-legs",
                classify_in(false, vec![live("0.10", "0.12"); 6]),
            ),
            (
                "seven-legs",
                classify_in(false, vec![live("0.10", "0.12"); 7]),
            ),
            ("zero-legs", classify_in(false, vec![])),
            (
                "bad-status",
                classify_in(
                    false,
                    vec![leg("won", None, None, Value::Null), live("0.20", "0.30")],
                ),
            ),
            (
                "fee-rate-1",
                classify_in(
                    false,
                    vec![
                        leg(
                            "live",
                            Some("0.40"),
                            Some("0.41"),
                            json!({ "rate": "1", "exponent": 1 }),
                        ),
                        live("0.35", "0.36"),
                    ],
                ),
            ),
        ],
    );
}

// ---- depth walk ----
fn wleg(levels: Value, fee: Value) -> Value {
    json!({ "levels": levels, "fee": fee })
}

fn walk_in(rule: &str, legs: Vec<Value>) -> String {
    json!({ "rule": rule, "legs": legs }).to_string()
}

#[test]
fn depth_walk() {
    let yes_two = vec![
        wleg(json!([lv("0.30", "10"), lv("0.40", "10")]), Value::Null),
        wleg(json!([lv("0.60", "15")]), Value::Null),
    ];
    let no_three = vec![
        wleg(json!([lv("0.60", "10"), lv("0.62", "20")]), Value::Null),
        wleg(json!([lv("0.65", "15"), lv("0.70", "30")]), Value::Null),
        wleg(json!([lv("0.70", "5"), lv("0.72", "40")]), Value::Null),
    ];
    let three_levels = |a: &str, b: &str, c: &str| {
        wleg(json!([lv(a, "10"), lv(b, "10"), lv(c, "10")]), Value::Null)
    };
    check(
        "depth_walk_json",
        depth_walk_json,
        vec![
            ("yes-two-legs", walk_in("yes", yes_two.clone())),
            ("no-nested-three", walk_in("no", no_three)),
            (
                "exactly-one",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([lv("0.50", "10")]), Value::Null),
                        wleg(json!([lv("0.50", "10")]), Value::Null),
                    ],
                ),
            ),
            (
                "fees-on",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([lv("0.30", "10"), lv("0.40", "10")]), fee7()),
                        wleg(json!([lv("0.60", "15")]), fee7()),
                    ],
                ),
            ),
            (
                "zero-depth-leg",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([]), Value::Null),
                        wleg(json!([lv("0.60", "15")]), Value::Null),
                    ],
                ),
            ),
            (
                "four-legs-three-levels",
                walk_in(
                    "yes",
                    vec![
                        three_levels("0.20", "0.21", "0.25"),
                        three_levels("0.22", "0.23", "0.24"),
                        three_levels("0.24", "0.26", "0.28"),
                        three_levels("0.25", "0.27", "0.30"),
                    ],
                ),
            ),
            (
                "descending-levels",
                walk_in(
                    "yes",
                    vec![
                        wleg(json!([lv("0.40", "10"), lv("0.30", "10")]), Value::Null),
                        wleg(json!([lv("0.60", "15")]), Value::Null),
                    ],
                ),
            ),
            ("bad-rule", walk_in("maybe", yes_two.clone())),
            (
                "seven-legs",
                walk_in("yes", vec![wleg(json!([lv("0.10", "1")]), Value::Null); 7]),
            ),
        ],
    );
}

// ---- episode ----
fn two(b0: &str, a0: &str, b1: &str, a1: &str) -> Value {
    json!([live(b0, a0), live(b1, a1)])
}

fn open_no() -> Value {
    two("0.60", "0.61", "0.50", "0.51") // NO asks 0.40 + 0.50 = 0.90 < 1
}

fn shut_no() -> Value {
    two("0.40", "0.41", "0.50", "0.51")
}

fn episode_in(
    kind: &str,
    gap_ms: Option<i64>,
    updates: Vec<(i64, Value)>,
    net_ms: i64,
    match_ms: i64,
    honor: bool,
    seconds_delay: Value,
) -> String {
    let updates: Vec<Value> = updates
        .into_iter()
        .map(|(t, legs)| json!({ "t_ms": t, "legs": legs }))
        .collect();
    let mut v = json!({
        "kind": kind,
        "updates": updates,
        "scenario": { "proc_ms": 0, "net_ms": net_ms, "match_ms": match_ms, "honor_seconds_delay": honor },
        "seconds_delay": seconds_delay,
    });
    if let Some(g) = gap_ms {
        v["gap_ms"] = json!(g);
    }
    v.to_string()
}

#[test]
fn episode() {
    let short = || vec![(0, shut_no()), (1000, open_no()), (1200, shut_no())];
    let yes_open = two("0.20", "0.30", "0.30", "0.40"); // YES asks 0.70 < 1
    let shut_yes = two("0.50", "0.60", "0.40", "0.50"); // YES asks 1.10, YES bids 0.90: nothing
    let many = |n: i64| (0..n).map(|i| (i * 100, open_no())).collect::<Vec<_>>();
    check(
        "episode_json",
        episode_json,
        vec![
            (
                "slow-miss",
                episode_in("no", Some(30_000), short(), 145, 150, false, Value::Null),
            ),
            (
                "fast-catch",
                episode_in("no", Some(30_000), short(), 1, 50, false, Value::Null),
            ),
            (
                "never-opens",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (500, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "still-open-at-end",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (1000, open_no()), (5000, open_no())],
                    145,
                    150,
                    false,
                    Value::Null,
                ),
            ),
            (
                "seconds-delay-ignored",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (100, open_no()), (900, shut_no())],
                    1,
                    50,
                    false,
                    json!(1),
                ),
            ),
            (
                "seconds-delay-honored",
                episode_in(
                    "no",
                    None,
                    vec![(0, shut_no()), (100, open_no()), (900, shut_no())],
                    1,
                    50,
                    true,
                    json!(1),
                ),
            ),
            (
                "yes-kind",
                episode_in(
                    "yes",
                    None,
                    vec![(0, shut_yes.clone()), (100, yes_open), (2000, shut_yes)],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "gap-censor",
                episode_in(
                    "no",
                    Some(100),
                    vec![(0, open_no()), (500, open_no()), (600, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "one-update",
                episode_in("no", None, vec![(0, open_no())], 1, 50, false, Value::Null),
            ),
            (
                "eight-updates",
                episode_in("no", None, many(8), 1, 50, false, Value::Null),
            ),
            (
                "nine-updates",
                episode_in("no", None, many(9), 1, 50, false, Value::Null),
            ),
            (
                "non-increasing-t",
                episode_in(
                    "no",
                    None,
                    vec![(100, open_no()), (100, shut_no())],
                    1,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "latency-5000",
                episode_in("no", None, short(), 5000, 0, false, Value::Null),
            ),
            (
                "latency-5001",
                episode_in("no", None, short(), 5001, 0, false, Value::Null),
            ),
            (
                "tx-at-close",
                episode_in("no", None, short(), 100, 100, false, Value::Null),
            ),
            (
                "tx-at-update",
                episode_in(
                    "no",
                    None,
                    vec![
                        (0, shut_no()),
                        (1000, open_no()),
                        (1100, two("0.70", "0.71", "0.50", "0.51")),
                        (1500, shut_no()),
                    ],
                    50,
                    50,
                    false,
                    Value::Null,
                ),
            ),
            (
                "seconds-delay-10",
                episode_in("no", None, short(), 1, 50, true, json!(10)),
            ),
            (
                "seconds-delay-11",
                episode_in("no", None, short(), 1, 50, true, json!(11)),
            ),
        ],
    );
}
// ---- basket (7b §3.4) ----
fn bk(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> Value {
    let side = |v: &[(&str, &str)]| Value::Array(v.iter().map(|(p, s)| lv(p, s)).collect());
    json!({ "bids": side(bids), "asks": side(asks) })
}

fn bleg(open: Value, t_x: Value, unwind: Value, fee: Value) -> Value {
    json!({ "open": open, "t_x": t_x, "unwind": unwind, "fee": fee })
}

/// Section f's default: a 3-leg NO basket, YES bids 0.40/0.35/0.30 (NO asks 0.60/0.65/0.70,
/// Σ 1.95 < 2), YES asks one tick up; 100 deep everywhere; t_x and unwind books = open.
fn f_book(i: usize) -> Value {
    let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
    bk(&[(b, "100")], &[(a, "100")])
}

fn f_legs() -> Vec<Value> {
    (0..3)
        .map(|i| bleg(f_book(i), f_book(i), f_book(i), Value::Null))
        .collect()
}

fn basket_in(kind: &str, legs: Vec<Value>, q: &str, extra: &[(&str, &str)]) -> String {
    let mut v = json!({ "kind": kind, "legs": legs, "q_request": q });
    for (k, x) in extra {
        v[*k] = json!(x);
    }
    v.to_string()
}

/// "The usual burst": leg 1 empty at t_x, and the unwind books 5 ticks against us (for NO the
/// YES asks rise 0.05).
fn burst_legs() -> Vec<Value> {
    let against = [
        bk(&[("0.40", "100")], &[("0.46", "100")]),
        bk(&[("0.35", "100")], &[("0.41", "100")]),
        bk(&[("0.30", "100")], &[("0.36", "100")]),
    ];
    (0..3)
        .map(|i| {
            let tx = if i == 1 {
                bk(&[], &[("0.36", "100")])
            } else {
                f_book(i)
            };
            bleg(f_book(i), tx, against[i].clone(), Value::Null)
        })
        .collect()
}

fn yes_book(b: &str, a: &str) -> Value {
    bk(&[(b, "100")], &[(a, "100")])
}

#[test]
fn basket() {
    let f = f_legs();
    let mut partial = f_legs();
    partial[2]["t_x"] = bk(&[("0.30", "20")], &[("0.31", "100")]); // leg 2: 20 of 50
    let mut below_min = f_legs();
    below_min[0]["t_x"] = bk(&[("0.40", "47")], &[("0.41", "100")]); // excess 3 on legs 1, 2
    let mut unwind_empty = burst_legs();
    for l in &mut unwind_empty {
        l["unwind"] = bk(&[("0.40", "100")], &[]);
    }
    let fees_on: Vec<Value> = (0..3)
        .map(|i| bleg(f_book(i), f_book(i), f_book(i), fee7()))
        .collect();
    let yes: Vec<Value> = [("0.29", "0.30"), ("0.29", "0.30"), ("0.29", "0.30")]
        .iter()
        .map(|(b, a)| bleg(yes_book(b, a), yes_book(b, a), yes_book(b, a), Value::Null))
        .collect();
    let mut yes_empty = yes.clone();
    yes_empty[0]["t_x"] = bk(&[("0.29", "100")], &[]);
    yes_empty[1]["unwind"] = bk(&[("0.20", "100")], &[("0.30", "100")]);
    yes_empty[2]["unwind"] = bk(&[("0.20", "100")], &[("0.30", "100")]);
    let no_edge: Vec<Value> = (0..3)
        .map(|_| {
            bleg(
                yes_book("0.30", "0.31"),
                yes_book("0.30", "0.31"),
                yes_book("0.30", "0.31"),
                Value::Null,
            )
        })
        .collect();
    let thin: Vec<Value> = (0..3)
        .map(|i| {
            let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
            let t = bk(&[(b, "4")], &[(a, "100")]);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let two_legs: Vec<Value> = [("0.60", "0.61"), ("0.50", "0.51")]
        .iter()
        .map(|(b, a)| {
            let t = yes_book(b, a);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let four_legs: Vec<Value> = ["0.30", "0.30", "0.25", "0.25"]
        .iter()
        .map(|b| {
            let t = bk(&[(b, "100")], &[("0.99", "100")]);
            bleg(t.clone(), t.clone(), t, Value::Null)
        })
        .collect();
    let mut five_legs = four_legs.clone();
    five_legs.push(four_legs[0].clone());
    let one_leg = vec![f[0].clone()];
    let mut eight_levels = f_legs();
    eight_levels[0]["unwind"] = json!({ "bids": [], "asks": eight(41) });
    let mut nine_levels = f_legs();
    nine_levels[0]["unwind"] = json!({ "bids": [], "asks": eight(41) });
    nine_levels[0]["unwind"]["asks"]
        .as_array_mut()
        .unwrap()
        .push(lv("0.49", "1"));
    let mut penny = burst_legs();
    penny[0]["unwind"] = bk(&[], &[("0.99", "100")]); // NO sells at 0.01
    let mut penny_bad = burst_legs();
    penny_bad[0]["unwind"] = bk(&[], &[("1.00", "100")]);
    let mut top = burst_legs();
    top[2]["unwind"] = bk(&[], &[("0.01", "100")]); // NO sells at 0.99
    let mut top_bad = burst_legs();
    top_bad[2]["unwind"] = bk(&[], &[("0.00", "100")]);
    let big = |s: &str| -> Vec<Value> {
        (0..3)
            .map(|i| {
                let (b, a) = [("0.40", "0.41"), ("0.35", "0.36"), ("0.30", "0.31")][i];
                let t = bk(&[(b, s)], &[(a, s)]);
                bleg(t.clone(), t.clone(), t, Value::Null)
            })
            .collect()
    };
    let default = basket_in("no", f.clone(), "50", &[]);
    check(
        "basket_json",
        basket_json,
        vec![
            ("no-all-fill", default.clone()),
            (
                "no-one-leg-empty-burst",
                basket_in("no", burst_legs(), "50", &[]),
            ),
            ("no-one-leg-partial", basket_in("no", partial, "50", &[])),
            ("no-excess-below-min", basket_in("no", below_min, "50", &[])),
            (
                "no-unwind-book-empty",
                basket_in("no", unwind_empty, "50", &[]),
            ),
            (
                "no-convert-fee-0",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "0")]),
            ),
            (
                "no-convert-fee-50",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "50")]),
            ),
            (
                "no-convert-fee-10000",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "10000")]),
            ),
            (
                "no-convert-fee-10001",
                basket_in("no", f.clone(), "50", &[("convert_fee_bips", "10001")]),
            ),
            ("no-fees-on", basket_in("no", fees_on, "50", &[])),
            (
                "no-min-order-10",
                basket_in("no", f.clone(), "50", &[("min_order", "10")]),
            ),
            ("yes-all-fill", basket_in("yes", yes.clone(), "50", &[])),
            ("yes-one-leg-empty", basket_in("yes", yes_empty, "50", &[])),
            (
                "yes-convert-fee",
                basket_in("yes", yes, "50", &[("convert_fee_bips", "50")]),
            ),
            ("no-edge-at-open", basket_in("no", no_edge, "50", &[])),
            ("too-thin-at-open", basket_in("no", thin, "50", &[])),
            (
                "q-request-below-min",
                basket_in("no", f.clone(), "4.99", &[]),
            ),
            (
                "q-request-json-number",
                default.replace(r#""q_request":"50""#, r#""q_request":50"#),
            ),
            (
                "q-request-1000000",
                basket_in("no", big("1000000"), "1000000", &[]),
            ),
            (
                "q-request-1000001",
                basket_in("no", big("1000000"), "1000001", &[]),
            ),
            ("one-leg", basket_in("no", one_leg, "50", &[])),
            ("two-legs", basket_in("no", two_legs, "50", &[])),
            ("four-legs", basket_in("no", four_legs, "50", &[])),
            ("five-legs", basket_in("no", five_legs, "50", &[])),
            ("eight-levels", basket_in("no", eight_levels, "50", &[])),
            ("nine-levels", basket_in("no", nine_levels, "50", &[])),
            ("price-0.99", basket_in("no", penny, "50", &[])),
            ("price-1.00", basket_in("no", penny_bad, "50", &[])),
            ("price-0.01", basket_in("no", top, "50", &[])),
            ("price-0.00", basket_in("no", top_bad, "50", &[])),
            ("bad-kind", basket_in("maybe", f.clone(), "50", &[])),
            ("input-16384", pad(&default, 16 * 1024)),
            ("input-16385", pad(&default, 16 * 1024 + 1)),
        ],
    );
}
