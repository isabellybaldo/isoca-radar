//! Hand-computed expectations for the facade (09-27 spec §4.1, 7b §3.4) and its limits.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{classify_json, depth_walk_json, episode_json, mirror_json, payoff_json};
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
