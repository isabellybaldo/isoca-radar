//! Golden fixtures (09-27 spec §4.3, 7b §3.4). The cases live here; `fixtures/<name>_json.json`
//! holds `[{ "name", "input", "output" }]` with `input` and `output` as the exact strings passed and
//! returned, so the web's parity test (plan 7b-B) runs the same files through the WASM build and
//! compares byte for byte. `UPDATE_FIXTURES=1` rewrites the files from these cases; a reviewer
//! reads the diff. Without it, a file whose names, inputs or outputs differ from here fails.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{mirror_json, payoff_json};
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
