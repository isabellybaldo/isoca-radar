//! 09-27 spec §8: any input up to 16 KiB returns a model or {"error"}, never a panic.
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

fn level() -> impl Strategy<Value = String> {
    (
        r#"(0\.[0-9]{1,3}|1\.00|0|-0\.5)"#,
        r#"([0-9]{1,8}|0\.0000001|-1)"#,
    )
        .prop_map(|(p, s)| format!(r#"{{"p":"{p}","s":"{s}"}}"#))
}

fn book() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(level(), 0..10),
        prop::collection::vec(level(), 0..10),
    )
        .prop_map(|(b, a)| format!(r#"{{"bids":[{}],"asks":[{}]}}"#, b.join(","), a.join(",")))
}

fn basket() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("no"), Just("yes"), Just("x")],
        prop::collection::vec((book(), book(), book(), any::<bool>()), 0..6),
        r#"([0-9]{1,7}(\.[0-9]{1,7})?|-1)"#,
        prop_oneof![
            Just(""),
            Just(r#","convert_fee_bips":"50""#),
            Just(r#","min_order":"0""#)
        ],
    )
        .prop_map(|(kind, legs, q, extra)| {
            let legs: Vec<String> = legs
                .into_iter()
                .map(|(o, t, u, fee)| {
                    let fee = if fee {
                        r#"{"rate":"0.07","exponent":1}"#
                    } else {
                        "null"
                    };
                    format!(r#"{{"open":{o},"t_x":{t},"unwind":{u},"fee":{fee}}}"#)
                })
                .collect();
            format!(
                r#"{{"kind":"{kind}","legs":[{}],"q_request":"{q}"{extra}}}"#,
                legs.join(",")
            )
        })
}

fn json_ish() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,300}",
        r#"\{"(bids|asks|legs|kind|s|rule|updates|q_request)":\[?\{?"?[a-z0-9.\-]{0,8}"?\]?\}"#,
        book(),
        basket(),
    ]
}

proptest! {
    #[test]
    fn never_panics(s in json_ish()) {
        for f in ALL {
            let out = f(&s);
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            prop_assert!(v.is_object());
            if let Some(e) = v.get("error") {
                prop_assert!(e.as_str().is_some_and(|m| !m.contains('\n')));
            }
        }
    }
}
