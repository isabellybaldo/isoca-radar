//! 09-27 spec §8: any input up to 16 KiB returns a model or {"error"}, never a panic.
//! Every function gets both garbage and structurally valid input (random values around and across
//! the limits), so the deep paths behind the serde layer are reached.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_wasm::api::{
    basket_json, classify_json, depth_walk_json, episode_json, mirror_json, payoff_json,
};
use proptest::prelude::*;

const ALL: [fn(&str) -> String; 6] = [
    mirror_json,
    payoff_json,
    classify_json,
    depth_walk_json,
    episode_json,
    basket_json,
];

/// Mostly on the 0.01 grid, now and then off it.
fn price() -> impl Strategy<Value = String> {
    prop_oneof![
        18 => (1u32..=99).prop_map(|c| format!("0.{c:02}")),
        1 => r#"(0\.[0-9]{1,3}|1\.00|0|-0\.5|abc|)"#,
    ]
}

fn size() -> impl Strategy<Value = String> {
    prop_oneof![
        18 => (1u32..=2000).prop_map(|n| n.to_string()),
        1 => r#"([0-9]{1,8}|0\.0000001|0|-1|1000000|1000001|x)"#,
    ]
}

fn level() -> impl Strategy<Value = String> {
    (price(), size()).prop_map(|(p, s)| format!(r#"{{"p":"{p}","s":"{s}"}}"#))
}

/// Levels in random order (ascending AND descending); `max` may exceed the 8-level limit.
fn levels(max: usize) -> impl Strategy<Value = Vec<String>> {
    prop_oneof![
        3 => prop::collection::vec(level(), 1..=3),
        1 => prop::collection::vec(level(), 0..=max),
    ]
}

fn fee() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => Just("null".to_string()),
        2 => (prop_oneof![Just("0.07"), Just("0.04"), Just("0"), Just("-1"), Just("2")], 0u8..=5)
            .prop_map(|(r, e)| format!(r#"{{"rate":"{r}","exponent":{e}}}"#)),
    ]
}

fn book() -> impl Strategy<Value = String> {
    (levels(9), levels(9))
        .prop_map(|(b, a)| format!(r#"{{"bids":[{}],"asks":[{}]}}"#, b.join(","), a.join(",")))
}

fn basket() -> impl Strategy<Value = String> {
    (
        prop_oneof![4 => Just("no"), 4 => Just("yes"), 1 => Just("x")],
        // mostly 2..=4 legs, the accepting range
        prop_oneof![
            6 => prop::collection::vec((book(), book(), book(), fee()), 2..=4),
            1 => prop::collection::vec((book(), book(), book(), fee()), 0..=6),
        ],
        prop_oneof![
            9 => (1u32..=5000).prop_map(|n| n.to_string()),
            1 => r#"([0-9]{1,7}(\.[0-9]{1,7})?|-1)"#,
        ],
        prop_oneof![
            Just(""),
            Just(r#","convert_fee_bips":"50""#),
            Just(r#","convert_fee_bips":"10001""#),
            Just(r#","min_order":"0""#),
            Just(r#","min_order":"5""#)
        ],
    )
        .prop_map(|(kind, legs, q, extra)| {
            let legs: Vec<String> = legs
                .into_iter()
                .map(|(o, t, u, fee)| {
                    format!(r#"{{"open":{o},"t_x":{t},"unwind":{u},"fee":{fee}}}"#)
                })
                .collect();
            format!(
                r#"{{"kind":"{kind}","legs":[{}],"q_request":"{q}"{extra}}}"#,
                legs.join(",")
            )
        })
}

/// The accepting shapes: everything on the grid, inside the limits, a book that can trade.
/// One YES leg: asks from `a` up, bids just under `a`, 1..=3 levels a side.
fn grid_leg(
    a: std::ops::RangeInclusive<u32>,
) -> impl Strategy<Value = (u32, usize, usize, u32, bool)> {
    (
        a,
        1usize..=3,
        1usize..=3,
        1u32..=1000,
        prop::bool::weighted(0.9),
    )
}

fn render_leg((a, na, nb, size, live): (u32, usize, usize, u32, bool)) -> String {
    let asks: Vec<String> = (0..na as u32)
        .map(|k| format!(r#"{{"p":"0.{:02}","s":"{size}"}}"#, a + k))
        .collect();
    let bids: Vec<String> = (1..=nb as u32)
        .map(|k| {
            format!(
                r#"{{"p":"0.{:02}","s":"{size}"}}"#,
                a.saturating_sub(k).max(1)
            )
        })
        .collect();
    let st = if live { "live" } else { "other" };
    format!(
        r#"{{"status":"{st}","bids":[{}],"asks":[{}],"fee":null}}"#,
        bids.join(","),
        asks.join(",")
    )
}

fn render_book(leg: (u32, usize, usize, u32, bool)) -> String {
    let v: serde_json::Value = serde_json::from_str(&render_leg(leg)).unwrap();
    format!(r#"{{"bids":{},"asks":{}}}"#, v["bids"], v["asks"])
}

fn ok_classify() -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        prop::collection::vec(grid_leg(5..=95), 2..=4),
    )
        .prop_map(|(aug, legs)| {
            let legs: Vec<String> = legs.into_iter().map(render_leg).collect();
            format!(r#"{{"augmented":{aug},"legs":[{}]}}"#, legs.join(","))
        })
}

fn ok_basket() -> impl Strategy<Value = String> {
    // the open book is cheap enough to have an edge: low asks for YES, high bids for NO
    prop_oneof![Just(true), Just(false)].prop_flat_map(|yes| {
        let open = if yes { 5..=33 } else { 62..=95 };
        (
            prop::collection::vec((grid_leg(open), grid_leg(5..=95), grid_leg(5..=95)), 2..=4),
            1u32..=500,
            prop_oneof![Just(""), Just(r#","min_order":"1""#)],
            prop_oneof![Just(""), Just(r#","convert_fee_bips":"50""#)],
        )
            .prop_map(move |(legs, q, extra, conv)| {
                let legs: Vec<String> = legs
                    .into_iter()
                    .map(|(o, t, u)| {
                        format!(
                            r#"{{"open":{},"t_x":{},"unwind":{},"fee":null}}"#,
                            render_book(o),
                            render_book(t),
                            render_book(u)
                        )
                    })
                    .collect();
                let (kind, conv) = if yes { ("yes", "") } else { ("no", conv) };
                format!(
                    r#"{{"kind":"{kind}","legs":[{}],"q_request":"{q}"{extra}{conv}}}"#,
                    legs.join(",")
                )
            })
    })
}

fn ok_book() -> impl Strategy<Value = String> {
    grid_leg(5..=95).prop_map(render_book)
}

fn ok_depth() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no")],
        prop::collection::vec(grid_leg(5..=95), 2..=4),
    )
        .prop_map(|(rule, legs)| {
            let legs: Vec<String> = legs
                .into_iter()
                .map(|l| {
                    let v: serde_json::Value = serde_json::from_str(&render_book(l)).unwrap();
                    format!(r#"{{"levels":{},"fee":null}}"#, v["asks"])
                })
                .collect();
            format!(r#"{{"rule":"{rule}","legs":[{}]}}"#, legs.join(","))
        })
}

fn ok_payoff() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no"), Just("implication")],
        2usize..=6,
        any::<bool>(),
        any::<bool>(),
        prop::collection::vec((1u32..=99, 1u32..=99), 6),
    )
        .prop_map(|(kind, s, other, with_prices, ps)| {
            let want = if kind == "implication" { 2 } else { s };
            let prices = if with_prices {
                let v: Vec<String> = ps
                    .iter()
                    .take(want)
                    .map(|(b, a)| format!(r#"{{"bid":"0.{b:02}","ask":"0.{a:02}"}}"#))
                    .collect();
                format!("[{}]", v.join(","))
            } else {
                "null".to_string()
            };
            format!(r#"{{"kind":"{kind}","s":{s},"other_listed":{other},"prices":{prices}}}"#)
        })
}

fn ok_episode() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no")],
        1i64..=600_000,
        (2usize..=3).prop_flat_map(|n| {
            prop::collection::vec(
                (1i64..=20_000, prop::collection::vec(grid_leg(5..=95), n)),
                2..=8,
            )
        }),
        (0i64..=5000, 0i64..=5000, 0i64..=5000, any::<bool>()),
        prop_oneof![
            Just("null".to_string()),
            (0i32..=10).prop_map(|n| n.to_string())
        ],
    )
        .prop_map(|(kind, gap, ups, (p, n, m, honor), sd)| {
            let mut t = 0i64;
            let ups: Vec<String> = ups
                .into_iter()
                .map(|(dt, legs)| {
                    t += dt;
                    let legs: Vec<String> = legs.into_iter().map(render_leg).collect();
                    format!(r#"{{"t_ms":{t},"legs":[{}]}}"#, legs.join(","))
                })
                .collect();
            format!(
                r#"{{"kind":"{kind}","gap_ms":{gap},"updates":[{}],"scenario":{{"proc_ms":{p},"net_ms":{n},"match_ms":{m},"honor_seconds_delay":{honor}}},"seconds_delay":{sd}}}"#,
                ups.join(",")
            )
        })
}

fn payoff() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no"), Just("implication"), Just("x")],
        0usize..=8,
        any::<bool>(),
        prop_oneof![
            Just(None),
            prop::collection::vec((price(), price()), 0..=7).prop_map(Some)
        ],
    )
        .prop_map(|(kind, s, other, prices)| {
            let prices = match prices {
                None => "null".to_string(),
                Some(v) => format!(
                    "[{}]",
                    v.iter()
                        .map(|(b, a)| format!(r#"{{"bid":"{b}","ask":"{a}"}}"#))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            };
            format!(r#"{{"kind":"{kind}","s":{s},"other_listed":{other},"prices":{prices}}}"#)
        })
}

fn leg_in() -> impl Strategy<Value = String> {
    (
        prop_oneof![
            4 => Just("live"),
            1 => Just("placeholder"),
            1 => Just("other"),
            1 => Just("closed_no"),
            1 => Just("closed_yes"),
            1 => Just("halted"),
            1 => Just("unknown")
        ],
        levels(9),
        levels(9),
        fee(),
    )
        .prop_map(|(st, b, a, fee)| {
            format!(
                r#"{{"status":"{st}","bids":[{}],"asks":[{}],"fee":{fee}}}"#,
                b.join(","),
                a.join(",")
            )
        })
}

fn legs_in(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(leg_in(), 0..=max).prop_map(|v| v.join(","))
}

fn classify() -> impl Strategy<Value = String> {
    (any::<bool>(), legs_in(7))
        .prop_map(|(aug, legs)| format!(r#"{{"augmented":{aug},"legs":[{legs}]}}"#))
}

fn depth_walk() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no"), Just("x")],
        prop::collection::vec((levels(9), fee()), 0..=7),
    )
        .prop_map(|(rule, legs)| {
            let legs: Vec<String> = legs
                .into_iter()
                .map(|(l, f)| format!(r#"{{"levels":[{}],"fee":{f}}}"#, l.join(",")))
                .collect();
            format!(r#"{{"rule":"{rule}","legs":[{}]}}"#, legs.join(","))
        })
}

fn ms() -> impl Strategy<Value = i64> {
    prop_oneof![
        3 => 0i64..=5000,
        1 => Just(5001),
        1 => Just(-1),
        1 => any::<i64>(),
    ]
}

fn episode() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("yes"), Just("no"), Just("x")],
        prop_oneof![
            Just(String::new()),
            Just(r#""gap_ms":0,"#.to_string()),
            Just(r#""gap_ms":1,"#.to_string()),
            Just(r#""gap_ms":600000,"#.to_string()),
            Just(r#""gap_ms":600001,"#.to_string()),
            (-5i64..=700_000).prop_map(|g| format!(r#""gap_ms":{g},"#)),
        ],
        prop::collection::vec(
            (
                prop_oneof![0i64..=10, 599_990i64..=600_005, 0i64..=600_000, Just(-1)],
                legs_in(5),
            ),
            0..=9,
        ),
        (ms(), ms(), ms(), any::<bool>()),
        prop_oneof![
            Just("null".to_string()),
            (0i32..=11).prop_map(|n| n.to_string())
        ],
    )
        .prop_map(|(kind, gap, ups, (p, n, m, honor), sd)| {
            let ups: Vec<String> = ups
                .into_iter()
                .map(|(t, legs)| format!(r#"{{"t_ms":{t},"legs":[{legs}]}}"#))
                .collect();
            format!(
                r#"{{"kind":"{kind}",{gap}"updates":[{}],"scenario":{{"proc_ms":{p},"net_ms":{n},"match_ms":{m},"honor_seconds_delay":{honor}}},"seconds_delay":{sd}}}"#,
                ups.join(",")
            )
        })
}

/// Hostile input (weight 1 each) beside the accepting shapes (weight 5 each), so every function
/// reaches its deep paths as well as its rejections.
fn json_ish() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => ".{0,300}",
        1 => r#"\{"(bids|asks|legs|kind|s|rule|updates|q_request)":\[?\{?"?[a-z0-9.\-]{0,8}"?\]?\}"#,
        1 => book(),
        1 => basket(),
        1 => payoff(),
        1 => classify(),
        1 => depth_walk(),
        1 => episode(),
        5 => ok_book(),
        5 => ok_basket(),
        5 => ok_payoff(),
        5 => ok_classify(),
        5 => ok_depth(),
        5 => ok_episode(),
    ]
}

fn assert_model_or_error(out: &str) -> Result<(), TestCaseError> {
    let v: serde_json::Value = serde_json::from_str(out).unwrap();
    prop_assert!(v.is_object());
    if let Some(e) = v.get("error") {
        prop_assert!(e.as_str().is_some_and(|m| !m.contains('\n')));
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]
    #[test]
    fn never_panics(s in json_ish()) {
        for f in ALL {
            assert_model_or_error(&f(&s))?;
        }
    }
}

/// `n` bytes: a valid book padded with trailing spaces.
fn padded(n: usize) -> String {
    let mut s = String::from(r#"{"bids":[{"p":"0.50","s":"10"}],"asks":[]}"#);
    s.push_str(&" ".repeat(n - s.len()));
    assert_eq!(s.len(), n);
    s
}

#[test]
fn the_size_limit_is_16_kib_exactly() {
    let over = padded(16_385);
    for f in ALL {
        let v: serde_json::Value = serde_json::from_str(&f(&over)).unwrap();
        assert_eq!(v["error"], "input is larger than 16 KiB");
    }
    let at = padded(16_384);
    for f in ALL {
        assert_model_or_error(&f(&at)).unwrap();
    }
    // mirror_json takes that book, so the limit itself is not what rejects it
    let v: serde_json::Value = serde_json::from_str(&mirror_json(&at)).unwrap();
    assert!(v.get("error").is_none(), "{v}");
}
