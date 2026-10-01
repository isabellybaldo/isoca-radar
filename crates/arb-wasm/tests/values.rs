//! Hand-computed expectations for the facade (09-27 spec §4.1, 7b §3.4) and its limits.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{
    basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json,
};
use serde_json::Value;

fn v(out: &str) -> Value {
    serde_json::from_str(out).unwrap()
}

fn err(out: &str) -> String {
    v(out)["error"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an error, got {out}"))
        .to_string()
}

#[test]
fn mirror_reflects_the_fed_book() {
    let out = mirror_json(
        r#"{"bids":[{"p":"0.54","s":"5603.74"},{"p":"0.53","s":"85887.16"}],"asks":[{"p":"0.55","s":"32356.03"}]}"#,
    );
    assert_eq!(
        out,
        r#"{"no_asks":[{"p":"0.46","s":"5603.74"},{"p":"0.47","s":"85887.16"}],"no_bids":[{"p":"0.45","s":"32356.03"}],"best_bid":{"p":"0.54","s":"5603.74"},"best_ask":{"p":"0.55","s":"32356.03"},"spread":"0.01","d1_long":"1.01","d1_short":"0.99","crossed":false}"#
    );
}

#[test]
fn payoff_top_edges_follow_l5() {
    // YES: 1 − (0.45 + 0.52) = 0.03
    let yes = payoff_json(
        r#"{"kind":"yes","s":2,"other_listed":true,"prices":[{"bid":"0.40","ask":"0.45"},{"bid":"0.50","ask":"0.52"}]}"#,
    );
    assert_eq!(v(&yes)["top_edge_gross"], "0.03");
    // NO: Σ bids − 1 = (0.60 + 0.50) − 1 = 0.10
    let no = payoff_json(
        r#"{"kind":"no","s":2,"other_listed":true,"prices":[{"bid":"0.60","ask":"0.61"},{"bid":"0.50","ask":"0.51"}]}"#,
    );
    assert_eq!(v(&no)["top_edge_gross"], "0.1");
}

#[test]
fn bad_mirror_and_payoff_inputs_are_one_sentence_errors() {
    let cases = [
        (
            r#"{"bids":[{"p":0.55,"s":"1"}],"asks":[]}"#,
            "not valid input",
        ),
        (
            r#"{"bids":[{"p":"0.555","s":"1"}],"asks":[]}"#,
            "steps of 0.01",
        ),
        (
            r#"{"bids":[{"p":"1.00","s":"1"}],"asks":[]}"#,
            "steps of 0.01",
        ),
        (r#"{"bids":[{"p":"0","s":"1"}],"asks":[]}"#, "steps of 0.01"),
        (
            r#"{"bids":[{"p":"0.50","s":"1"},{"p":"0.50","s":"2"}],"asks":[]}"#,
            "same price",
        ),
        (r#"{"bids":[{"p":"0.50","s":"0"}],"asks":[]}"#, "size"),
        (r#"{"bids":[{"p":"0.50","s":"1000001"}],"asks":[]}"#, "size"),
    ];
    for (input, needle) in cases {
        let e = err(&mirror_json(input));
        assert!(e.contains(needle), "{input} → {e}");
        assert!(!e.contains('\n'));
    }
    let s1 = payoff_json(r#"{"kind":"yes","s":1,"other_listed":true,"prices":null}"#);
    assert!(err(&s1).contains("2 to 6"));
    let big = format!(r#"{{"bids":[],"asks":[]}}{}"#, " ".repeat(16 * 1024));
    assert!(err(&mirror_json(&big)).contains("16 KiB"));
}

#[test]
fn an_s_past_32_bits_is_the_same_parse_error_on_every_target() {
    // `s` is a u32, so 5000000000 fails in serde on native and on wasm32 alike. As a `usize` it
    // parsed on 64-bit ("s must be 2 to 6", and an implication accepted it) and failed on wasm32.
    for (kind, column) in [("yes", 28), ("no", 27), ("implication", 36)] {
        let out = payoff_json(&format!(
            r#"{{"kind":"{kind}","s":5000000000,"other_listed":true,"prices":null}}"#
        ));
        assert_eq!(
            err(&out),
            format!(
                "not valid input: invalid value: integer `5000000000`, expected u32 at line 1 column {column}"
            ),
            "{kind}"
        );
    }
}

#[test]
fn deeply_nested_input_under_16_kib_is_one_error_not_a_crash() {
    // What protects this is the typed input structs: they expect an object of known fields, so
    // `[[[…` and `{"bids":{"bids":…` are rejected at the first wrong token, long before any deep
    // recursion. serde_json's own 128-level recursion limit is only the backstop. Either way a
    // stack overflow (which would abort the WASM instance) must never be reachable.
    for f in [mirror_json, payoff_json] {
        for bomb in ["[".repeat(16 * 1024), r#"{"bids":"#.repeat(1_000)] {
            let e = err(&f(&bomb));
            assert!(e.starts_with("not valid input"), "{e}");
        }
    }
}

#[test]
fn duplicate_price_error_has_no_newline_even_when_the_input_has_one() {
    // Price::parse trims whitespace, so "0.50\n" duplicates "0.50"; the message prints the
    // normalized price, never the raw string.
    let e = err(&mirror_json(
        r#"{"bids":[{"p":"0.50","s":"1"},{"p":"0.50\n","s":"2"}],"asks":[]}"#,
    ));
    assert_eq!(e, "two levels at the same price 0.5");
    assert!(!e.contains('\n'));
}

#[test]
fn classify_names_its_path() {
    let out = classify_json(
        r#"{"augmented":false,"legs":[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.35","s":"10"}],"asks":[{"p":"0.36","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.30","s":"10"}],"asks":[{"p":"0.31","s":"10"}],"fee":null}]}"#,
    );
    assert!(
        out.starts_with(
            r#"{"path":["no_basket_tried","no_basket_positive"],"kind":"no_basket","realization":"converter""#
        ),
        "{out}"
    );
}

#[test]
fn depth_walk_draws_past_q_star() {
    let out = depth_walk_json(
        r#"{"rule":"yes","legs":[{"levels":[{"p":"0.30","s":"10"},{"p":"0.40","s":"10"}],"fee":null},{"levels":[{"p":"0.60","s":"15"}],"fee":null}]}"#,
    );
    assert_eq!(
        out,
        r#"{"steps":[{"q_from":"0","q_to":"10","cost":"0.9","payoff":"1","profitable":true},{"q_from":"10","q_to":"15","cost":"1","payoff":"1","profitable":false}],"q_star":"10","pi":"1","pi_gross":"1"}"#
    );
    let seven = format!(
        r#"{{"rule":"yes","legs":[{}]}}"#,
        [r#"{"levels":[{"p":"0.10","s":"1"}],"fee":null}"#; 7].join(",")
    );
    assert!(err(&depth_walk_json(&seven)).contains("at most 6 legs"));
}

#[test]
fn a_slow_trader_misses_a_short_episode() {
    let open = r#"[{"status":"live","bids":[{"p":"0.60","s":"10"}],"asks":[{"p":"0.61","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let shut = r#"[{"status":"live","bids":[{"p":"0.40","s":"10"}],"asks":[{"p":"0.41","s":"10"}],"fee":null},{"status":"live","bids":[{"p":"0.50","s":"10"}],"asks":[{"p":"0.51","s":"10"}],"fee":null}]"#;
    let input = format!(
        r#"{{"kind":"no","gap_ms":30000,"updates":[{{"t_ms":0,"legs":{shut}}},{{"t_ms":1000,"legs":{open}}},{{"t_ms":1200,"legs":{shut}}}],"scenario":{{"proc_ms":0,"net_ms":145,"match_ms":150,"honor_seconds_delay":false}},"seconds_delay":null}}"#
    );
    let out = episode_json(&input);
    assert!(out.contains(r#""open_ms":1000,"close_ms":1200"#), "{out}");
    assert!(
        out.ends_with(r#""t_x":1295,"captured":"0","caught":false}"#),
        "{out}"
    );
}
/// Section f's three NO legs (YES bids 0.40/0.35/0.30, asks a tick up, 100 deep), with leg 1's
/// t_x bid ("" = no bid) and the three unwind books' asks given.
fn f_input(tx1_bids: &str, unwind_asks: [&str; 3]) -> String {
    let book = |b: &str, a: &str| {
        let bids = if b.is_empty() {
            String::new()
        } else {
            format!(r#"{{"p":"{b}","s":"100"}}"#)
        };
        format!(r#"{{"bids":[{bids}],"asks":[{{"p":"{a}","s":"100"}}]}}"#)
    };
    let (b, a) = (["0.40", "0.35", "0.30"], ["0.41", "0.36", "0.31"]);
    let legs: Vec<String> = (0..3)
        .map(|i| {
            let tx = if i == 1 {
                book(tx1_bids, a[i])
            } else {
                book(b[i], a[i])
            };
            format!(
                r#"{{"open":{},"t_x":{tx},"unwind":{},"fee":null}}"#,
                book(b[i], a[i]),
                book(b[i], unwind_asks[i])
            )
        })
        .collect();
    format!(
        r#"{{"kind":"no","legs":[{}],"q_request":"50"}}"#,
        legs.join(",")
    )
}

#[test]
fn the_default_no_basket_fills_and_converts_for_a_small_gain() {
    // NO asks 0.60 + 0.65 + 0.70 = 1.95 per set, 3 legs convert for 2: 50 sets spend 97.5, get 100
    let o = v(&basket_json(&f_input("0.35", ["0.41", "0.36", "0.31"])));
    assert_eq!(
        (o["q_open"].clone(), o["q_target"].clone()),
        ("100".into(), "50".into())
    );
    assert_eq!(o["q_eff"], "50");
    assert_eq!(o["legs"][0]["limit"], "0.6");
    assert_eq!(o["cash"]["spent"], "97.5");
    assert_eq!(o["cash"]["convert_back"], "100");
    assert_eq!(o["cash"]["net_cash"], "2.5");
    assert!(o["value_at_resolution"].is_null());
}

#[test]
fn the_usual_burst_is_caught_and_loses() {
    // leg 1 empty at t_x: q_eff 0, legs 0 and 2 unwind 50 each into YES asks 5 ticks up:
    // NO bids 1 − 0.46 = 0.54 and 1 − 0.36 = 0.64 → back 27 + 32 = 59; spent 30 + 35 = 65
    let o = v(&basket_json(&f_input("", ["0.46", "0.41", "0.36"])));
    assert_eq!(o["q_eff"], "0");
    assert_eq!(
        (
            o["legs"][0]["excess"].clone(),
            o["legs"][1]["excess"].clone()
        ),
        ("50".into(), "0".into())
    );
    assert_eq!(o["legs"][0]["unwind"]["vwap"], "0.54");
    assert_eq!(o["cash"]["unwind_back"], "59");
    assert_eq!(o["cash"]["net_cash"], "-6");
    assert_eq!(o["cash"]["convert_back"], "0");
}

#[test]
fn a_request_below_the_minimum_is_an_error_not_a_clamp() {
    let input = f_input("0.35", ["0.41", "0.36", "0.31"])
        .replace(r#""kind":"no""#, r#""kind":"no","min_order":"60""#);
    assert_eq!(
        err(&basket_json(&input)),
        "q_request must be at least the minimum order of 60 shares"
    );
}

#[test]
fn a_request_past_the_open_depth_plans_q_open_and_sizes_round_like_the_policy() {
    // NO asks 1 − 0.33 = 0.67 and 1 − 0.70 = 0.30: Σ 0.97 < 1, 100 deep on each leg
    let input = |q: &str| {
        let book = |b: &str, a: &str| {
            format!(r#"{{"bids":[{{"p":"{b}","s":"100"}}],"asks":[{{"p":"{a}","s":"100"}}]}}"#)
        };
        let leg = |b: &str, a: &str| {
            let k = book(b, a);
            format!(r#"{{"open":{k},"t_x":{k},"unwind":{k},"fee":null}}"#)
        };
        format!(
            r#"{{"kind":"no","legs":[{},{}],"q_request":"{q}"}}"#,
            leg("0.33", "0.34"),
            leg("0.70", "0.71")
        )
    };
    let past = v(&basket_json(&input("500")));
    assert_eq!(
        (past["q_open"].clone(), past["q_target"].clone()),
        ("100".into(), "100".into()),
        "the policy's min(budget, depth at open): q_open is shown beside it"
    );
    // 0.67 × 7.5 = 5.025 has 3 dp: `settle` rounds the size down to 7 (0.67 × 7 = 4.69)
    let odd = v(&basket_json(&input("7.5")));
    assert_eq!(odd["q_target"], "7");
    assert_eq!(odd["legs"][0]["buy"]["filled"], "7");
}
