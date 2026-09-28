#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::BTreeMap;
use tape::feed::*;
use tape::line::{Ctl, Line};
use tape::universe_view::*;

const T: i64 = 1_790_121_600_000;

fn rec(conn: &[(&str, usize)]) -> RecordView {
    RecordView {
        universe: UniverseView {
            taken_ms: T,
            groups: vec![GroupView {
                event_id: "e1".into(),
                neg_risk_market_id: Some("nr".into()),
                legs: ["a1", "a2"]
                    .iter()
                    .map(|t| LegView {
                        market_id: format!("m-{t}"),
                        yes_token: t.to_string(),
                        no_token: format!("n-{t}"),
                        status: "live".into(),
                    })
                    .collect(),
            }],
            canary: vec![CanaryView {
                market_id: "m9".into(),
                event_id: "e9".into(),
                yes_token: "cy".into(),
                no_token: "cn".into(),
            }],
        },
        conn_of: conn
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<BTreeMap<_, _>>(),
        file: "u.json.zst".into(),
    }
}

fn frame(t: i64, shard: usize, raw: String) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard,
        raw: Some(raw),
        ctl: None,
    }
}

fn ctl(t: i64, shard: usize, c: Ctl) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard,
        raw: None,
        ctl: Some(c),
    }
}

fn uni(t: i64) -> Line {
    ctl(
        t,
        0,
        Ctl::Universe {
            taken_ms: T,
            file: "u.json.zst".into(),
        },
    )
}

fn book(tok: &str, ts: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> String {
    let lv = |v: &[(&str, &str)]| {
        v.iter()
            .map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"event_type":"book","asset_id":"{tok}","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#,
        lv(bids),
        lv(asks)
    )
}

fn change(tok: &str, ts: i64, side: &str, price: &str, size: &str, bb: &str, ba: &str) -> String {
    format!(
        r#"{{"event_type":"price_change","timestamp":"{ts}","price_changes":[{{"asset_id":"{tok}","side":"{side}","price":"{price}","size":"{size}","best_bid":"{bb}","best_ask":"{ba}"}}]}}"#
    )
}

fn run(f: &mut FeedBooks, r: &RecordView, lines: &[Line]) -> Vec<Applied> {
    let mut load = |_: i64, _: &str| Ok(r.clone());
    lines
        .iter()
        .map(|l| f.apply(l, &mut load, &|_| true))
        .collect()
}

fn asks(f: &FeedBooks, t: &str) -> Vec<String> {
    f.book(t)
        .unwrap()
        .asks_best_first()
        .iter()
        .map(|(p, s)| format!("{}x{}", p.get(), s.get()))
        .collect()
}

#[test]
fn a_universe_makes_tokens_visible_with_their_metadata_and_shard() {
    let r = rec(&[("e1", 0), ("canary:m9", 1)]);
    let mut f = FeedBooks::new();
    let ap = run(&mut f, &r, &[uni(T)]);
    assert_eq!(
        ap[0].event,
        LineEvent::Universe(UniverseChange {
            appeared: vec!["a1".into(), "a2".into(), "cn".into(), "cy".into()],
            reset: vec![],
            gone: vec![],
        })
    );
    assert_eq!(f.conn("a1"), Some(0));
    assert_eq!(f.conn("cy"), Some(1));
    assert_eq!(f.tokens_on(1), vec!["cn".to_string(), "cy".to_string()]);
    assert_eq!(
        f.event_tokens("e1"),
        vec!["a1".to_string(), "a2".to_string()]
    );
    let m = f.meta("cn").unwrap();
    assert_eq!(
        (m.outcome.as_str(), m.neg_risk, m.event_id.as_str()),
        ("no", false, "e9")
    );
    assert!(f.meta("a1").unwrap().neg_risk);
    assert!(f.book("zz").is_none());
}

#[test]
fn a_phantom_level_is_trimmed_once_its_update_is_complete() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let ap = run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(
                T + 10,
                0,
                book("a1", T + 5, &[("0.40", "10")], &[("0.45", "10")]),
            ),
            // the server says the best ask is 0.46: our 0.45 is a phantom, but only once this update is complete
            frame(
                T + 20,
                0,
                change("a1", T + 15, "BUY", "0.41", "3", "0.41", "0.46"),
            ),
            // same server ts: part of the same update, the trim waits
            frame(
                T + 21,
                0,
                change("a1", T + 15, "BUY", "0.41", "4", "0.41", "0.46"),
            ),
            // a later ts on the same token settles the pending check first
            frame(
                T + 30,
                0,
                change("a1", T + 25, "BUY", "0.39", "1", "0.41", "0.46"),
            ),
        ],
    );
    assert_eq!(asks(&f, "a1"), Vec::<String>::new(), "0.45 trimmed");
    assert_eq!(ap[1].snapshotted, vec!["a1".to_string()]);
    assert!(ap[4].touched.contains(&"a1".to_string()));
    assert_eq!(
        ap[4].before_of("a1").unwrap().asks.len(),
        1,
        "levels before this line's first change"
    );
}

#[test]
fn a_down_line_drops_pending_checks_without_trimming() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let ap = run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
            frame(
                T + 20,
                0,
                change("a1", T + 15, "BUY", "0.30", "1", "0.30", "0.50"),
            ),
            ctl(T + 30, 0, Ctl::Down),
            frame(
                T + 5_000,
                0,
                change("a2", T + 4_990, "BUY", "0.20", "1", "0.20", "1"),
            ),
        ],
    );
    assert_eq!(ap[3].event, LineEvent::Down(0));
    assert_eq!(
        asks(&f, "a1"),
        vec!["0.45x10".to_string()],
        "no trim after the connection died"
    );
}

#[test]
fn a_group_without_a_connection_is_reset_and_never_trimmed() {
    let with = rec(&[("e1", 0)]);
    let without = rec(&[]);
    let mut f = FeedBooks::new();
    run(
        &mut f,
        &with,
        &[
            uni(T),
            frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
        ],
    );
    let ap = run(&mut f, &without, &[uni(T + 20)]);
    assert_eq!(
        ap[0].event,
        LineEvent::Universe(UniverseChange {
            appeared: vec![],
            reset: vec!["a1".into(), "a2".into()],
            gone: vec![],
        })
    );
    assert!(f.book("a1").unwrap().asks_best_first().is_empty(), "reset");
    run(
        &mut f,
        &without,
        &[
            frame(T + 30, 0, book("a1", T + 25, &[], &[("0.45", "10")])),
            frame(
                T + 40,
                0,
                change("a1", T + 35, "BUY", "0.30", "1", "0.30", "0.50"),
            ),
            frame(
                T + 5_000,
                0,
                change("a1", T + 4_990, "BUY", "0.29", "1", "0.30", "0.50"),
            ),
        ],
    );
    assert_eq!(
        asks(&f, "a1"),
        vec!["0.45x10".to_string()],
        "no check without a connection (F4)"
    );
}

#[test]
fn an_older_rest_snapshot_is_ignored_and_a_newer_one_replaces_the_book() {
    let r = rec(&[("e1", 0)]);
    let mut f = FeedBooks::new();
    let rest = |ts: i64, a: &str| Ctl::RestSnapshot {
        token: "a1".into(),
        server_ts_ms: ts,
        bids: vec![],
        asks: vec![(a.into(), "5".into())],
    };
    let ap = run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
            ctl(T + 20, 0, rest(T + 4, "0.50")),
            ctl(T + 30, 0, rest(T + 6, "0.55")),
        ],
    );
    assert!(ap[2].touched.is_empty());
    assert_eq!(ap[3].snapshotted, vec!["a1".to_string()]);
    assert_eq!(asks(&f, "a1"), vec!["0.55x5".to_string()]);
}

#[test]
fn a_missing_universe_record_changes_nothing_and_says_so() {
    let mut f = FeedBooks::new();
    let mut load = |_: i64, _: &str| -> anyhow::Result<RecordView> { anyhow::bail!("gone") };
    let ap = f.apply(&uni(T), &mut load, &|_| false);
    assert!(matches!(ap.event, LineEvent::UniverseMissing(ref m) if m.contains("gone")));
    assert!(f.visible_tokens().is_empty());
}

#[test]
fn state_round_trips_and_behaves_the_same_afterwards() {
    let r = rec(&[("e1", 0), ("canary:m9", 1)]);
    let mut f = FeedBooks::new();
    run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(
                T + 10,
                0,
                book("a1", T + 5, &[("0.40", "10")], &[("0.45", "10")]),
            ),
            frame(
                T + 20,
                0,
                change("a1", T + 15, "BUY", "0.41", "3", "0.41", "0.46"),
            ),
        ],
    );
    let mut g = FeedBooks::from_state(&f.to_state()).unwrap();
    assert_eq!(g.to_state(), f.to_state());
    let more = [frame(
        T + 3_000,
        0,
        change("a2", T + 2_990, "BUY", "0.1", "1", "0.1", "1"),
    )];
    run(&mut f, &r, &more);
    run(&mut g, &r, &more);
    assert_eq!(asks(&g, "a1"), asks(&f, "a1"));
    assert_eq!(
        asks(&g, "a1"),
        Vec::<String>::new(),
        "the restored pending check still trims"
    );
}

/// The engine keeps a book per leg slot and indexes only live legs (engine.rs `apply_universe`):
/// a token listed twice is visible through its live leg, and on the next refresh it carries the
/// book of its LAST listing (here the halted one, never fed), exactly as the engine does.
#[test]
fn a_token_listed_twice_follows_the_engines_leg_slots() {
    let leg = |t: &str, status: &str| LegView {
        market_id: format!("m-{t}"),
        yes_token: t.into(),
        no_token: format!("n-{t}"),
        status: status.into(),
    };
    let group = |e: &str, legs: Vec<LegView>| GroupView {
        event_id: e.into(),
        neg_risk_market_id: None,
        legs,
    };
    let r = RecordView {
        universe: UniverseView {
            taken_ms: T,
            groups: vec![
                group("e1", vec![leg("a1", "live")]),
                group("e2", vec![leg("a1", "halted")]),
            ],
            canary: vec![],
        },
        conn_of: [("e1".to_string(), 0), ("e2".to_string(), 1)]
            .into_iter()
            .collect(),
        file: "u.json.zst".into(),
    };
    let mut f = FeedBooks::new();
    run(
        &mut f,
        &r,
        &[
            uni(T),
            frame(T + 10, 0, book("a1", T + 5, &[], &[("0.45", "10")])),
        ],
    );
    assert_eq!(f.conn("a1"), Some(0), "visible through its live leg");
    assert_eq!(asks(&f, "a1"), vec!["0.45x10".to_string()]);
    run(&mut f, &r, &[uni(T + 20)]);
    assert!(
        asks(&f, "a1").is_empty(),
        "the halted listing's empty book is the one carried"
    );
    let g = FeedBooks::from_state(&f.to_state()).unwrap();
    assert_eq!(g.to_state(), f.to_state());
    assert_eq!(g.conn("a1"), Some(0));
}
