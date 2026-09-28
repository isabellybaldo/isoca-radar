#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::reduce::Reducer;
use compact::rows::*;
use fixture::*;

fn tob(
    ts: i64,
    recv: i64,
    idx: u32,
    bid: Option<(&str, &str)>,
    ask: Option<(&str, &str)>,
) -> TobRow {
    TobRow {
        ts_ms: ts,
        recv_ms: recv,
        token_idx: idx,
        bid: bid.map(|b| m(b.0)),
        bid_sz: bid.map(|b| m(b.1)),
        ask: ask.map(|a| m(a.0)),
        ask_sz: ask.map(|a| m(a.1)),
    }
}

fn gap(s: i64, e: i64, shard: Option<i32>, why: &str) -> GapRow {
    GapRow {
        start_ms: s,
        end_ms: e,
        shard,
        reason: why.into(),
    }
}

fn depth(ts: i64, idx: u32, side: u8, level: u16, p: &str, s: &str) -> DepthRow {
    DepthRow {
        ts_ms: ts,
        token_idx: idx,
        side,
        level,
        price: m(p),
        size: m(s),
    }
}

#[test]
fn the_golden_hour_produces_exactly_the_expected_rows() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book(
                    "y1",
                    T0 + 1_990,
                    &[("0.40", "100"), ("0.39", "50")],
                    &[("0.42", "30")],
                ),
            ),
            frame(
                T0 + 3_000,
                0,
                book("y2", T0 + 2_995, &[("0.55", "10")], &[]),
            ),
            frame(
                T0 + 4_000,
                0,
                change("y1", T0 + 3_995, "SELL", "0.42", "0", "0.40", "1"),
            ),
            frame(
                T0 + 5_000,
                0,
                change("y1", T0 + 4_995, "BUY", "0.40", "100", "0.40", "1"),
            ), // same top: no row
            frame(
                T0 + 301_000,
                0,
                change("y2", T0 + 300_995, "BUY", "0.56", "5", "0.56", "1"),
            ), // crosses the 5-min boundary first
            down(T0 + 400_000, 0),
            frame(
                T0 + 401_000,
                0,
                change("y1", T0 + 400_995, "BUY", "0.41", "7", "0.41", "1"),
            ), // untrusted: no row
            frame(
                T0 + 402_000,
                0,
                book("y1", T0 + 401_995, &[("0.41", "7")], &[("0.43", "9")]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let stats = red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob,
        vec![
            tob(
                T0 + 1_990,
                T0 + 2_000,
                0,
                Some(("0.40", "100")),
                Some(("0.42", "30"))
            ),
            tob(T0 + 2_995, T0 + 3_000, 1, Some(("0.55", "10")), None),
            tob(T0 + 3_995, T0 + 4_000, 0, Some(("0.40", "100")), None),
            tob(T0 + 300_995, T0 + 301_000, 1, Some(("0.56", "5")), None),
            tob(
                T0 + 401_995,
                T0 + 402_000,
                0,
                Some(("0.41", "7")),
                Some(("0.43", "9"))
            ),
        ]
    );
    assert_eq!(
        s.depth,
        vec![
            depth(T0 + 300_000, 0, SIDE_BID, 0, "0.40", "100"),
            depth(T0 + 300_000, 0, SIDE_BID, 1, "0.39", "50"),
            depth(T0 + 300_000, 1, SIDE_BID, 0, "0.55", "10"),
        ]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 3_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 400_000, T0 + H, Some(0), reason::DOWN),
        ]
    );
    assert!(s.l2.is_empty());
    let toks = red.tokens();
    assert_eq!(toks.len(), 2);
    assert_eq!(
        (
            toks[0].token_id.as_str(),
            toks[0].market_id.as_str(),
            toks[0].event_id.as_str(),
            toks[0].outcome.as_str(),
            toks[0].neg_risk
        ),
        ("y1", "m-y1", "e1", "yes", true)
    );
    assert_eq!((stats.lines, stats.bad_lines, stats.ts_clamped), (9, 0, 0));
}

#[test]
fn a_lost_hour_becomes_a_raw_hour_lost_gap_and_untrusts_every_book() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "1")], &[])),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let mut s1 = VecSink::default();
    red.begin_hour(T0 + H, vec![]);
    red.missing_hour(reason::RAW_HOUR_LOST, &mut s1).unwrap();
    red.end_hour(&mut s1).unwrap();
    assert_eq!(
        s1.gaps,
        vec![gap(T0 + H, T0 + 2 * H, None, reason::RAW_HOUR_LOST)]
    );
    let mut s2 = VecSink::default();
    red.begin_hour(T0 + 2 * H, vec![]);
    run(
        &mut red,
        &[
            frame(
                T0 + 2 * H + 500,
                0,
                change("y1", T0 + 2 * H + 495, "BUY", "0.41", "1", "0.41", "1"),
            ), // stale book: no row
            frame(
                T0 + 2 * H + 1_000,
                0,
                book("y1", T0 + 2 * H + 995, &[("0.42", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s2,
    );
    red.end_hour(&mut s2).unwrap();
    assert_eq!(s2.tob.len(), 1);
    assert_eq!(s2.tob[0].bid, Some(m("0.42")));
    assert_eq!(
        s2.gaps,
        vec![
            gap(T0 + 2 * H, T0 + 2 * H + 500, None, reason::RAW_HOUR_LOST),
            gap(T0 + 2 * H + 500, T0 + 3 * H, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn shutdown_is_scanner_stopped_until_the_next_line_then_books_wait_for_snapshots() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            shutdown(T0 + 10_000),
            uni(T0 + 20_000, &r),
            frame(
                T0 + 21_000,
                0,
                change("y1", T0 + 20_995, "BUY", "0.41", "1", "0.41", "1"),
            ),
            frame(
                T0 + 22_000,
                0,
                book("y1", T0 + 21_995, &[("0.45", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.40")), Some(m("0.45"))]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            // fix round 1 (ruling 4): the shard gaps and scanner_stopped meet at the shutdown line
            gap(T0 + 10_000, T0 + 20_000, None, reason::SCANNER_STOPPED),
            gap(T0 + 20_000, T0 + 22_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn a_group_losing_its_connection_stops_rows_until_a_new_snapshot() {
    let with = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let without = rec("u2", T0 + 60_000, &[("e1", &["y1"])], &[]);
    let back = rec("u3", T0 + 120_000, &[("e1", &["y1"])], &[("e1", 3)]);
    let recs = [with.clone(), without.clone(), back.clone()];
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &with),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            uni(T0 + 60_000, &without),
            frame(
                T0 + 61_000,
                0,
                change("y1", T0 + 60_995, "BUY", "0.41", "1", "0.41", "1"),
            ), // reset book: no row
            uni(T0 + 120_000, &back),
            frame(
                T0 + 121_000,
                3,
                change("y1", T0 + 120_995, "BUY", "0.42", "1", "0.42", "1"),
            ), // still untrusted
            frame(
                T0 + 122_000,
                3,
                book("y1", T0 + 121_995, &[("0.43", "1")], &[]),
            ),
        ],
        &recs,
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.40")), Some(m("0.43"))]
    );
    assert!(
        s.gaps.contains(&gap(
            T0 + 120_000,
            T0 + 122_000,
            Some(3),
            reason::NO_BOOK_YET
        )),
        "{:?}",
        s.gaps
    );
    // fix round 1 (ruling 3): y1 is untrusted from 60 000 (no connection) to 122 000, covered
    // on its last shard as `down`
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 60_000, T0 + 122_000, Some(0), reason::DOWN),
            gap(T0 + 120_000, T0 + 122_000, Some(3), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn depth_covers_only_tokens_changed_since_their_last_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0), ("zz", 1)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "1")], &[])),
            frame(
                T0 + 300_500,
                0,
                change("y1", T0 + 300_495, "BUY", "0.41", "1", "0.41", "1"),
            ),
            frame(
                T0 + 600_500,
                0,
                change("y1", T0 + 600_495, "BUY", "0.42", "1", "0.42", "1"),
            ),
            frame(T0 + 900_500, 1, trade("other", T0 + 900_495)), // y1 changed at 600 500: snapshot at 900 000
            frame(T0 + 1_200_500, 1, trade("other", T0 + 1_200_495)), // nothing changed since: no snapshot
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let mut at: Vec<(i64, u32)> = s
        .depth
        .iter()
        .map(|d| (d.ts_ms - T0, d.token_idx))
        .collect();
    at.dedup(); // one entry per (snapshot, token); a token has up to depth_levels rows per side
    assert_eq!(
        at,
        vec![(300_000, 0), (300_000, 1), (600_000, 0), (900_000, 0)]
    );
}

#[test]
fn tob_ts_is_clamped_non_decreasing_per_token() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 6_000, 0, book("y1", T0 + 5_000, &[("0.40", "1")], &[])),
            frame(
                T0 + 7_000,
                0,
                change("y1", T0 + 4_000, "BUY", "0.41", "1", "0.41", "1"),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let st = red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob.iter().map(|t| t.ts_ms).collect::<Vec<_>>(),
        vec![T0 + 5_000, T0 + 5_000]
    );
    assert_eq!(st.ts_clamped, 1);
}

#[test]
fn the_token_dictionary_restarts_each_utc_day() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0 + 23 * H, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 23 * H + 1_000, &r),
            frame(
                T0 + 23 * H + 2_000,
                0,
                book("y2", T0 + 23 * H + 1_990, &[("0.20", "1")], &[]),
            ),
            frame(
                T0 + 23 * H + 3_000,
                0,
                book("y1", T0 + 23 * H + 2_990, &[("0.40", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        red.tokens()
            .iter()
            .map(|t| t.token_id.as_str())
            .collect::<Vec<_>>(),
        vec!["y2", "y1"]
    );
    let mut s2 = VecSink::default();
    red.begin_hour(T0 + 24 * H, vec![]);
    assert!(red.tokens().is_empty(), "a new day, a new dictionary");
    run(
        &mut red,
        &[frame(
            T0 + 24 * H + 1_000,
            0,
            change("y1", T0 + 24 * H + 995, "BUY", "0.41", "1", "0.41", "1"),
        )],
        std::slice::from_ref(&r),
        &mut s2,
    );
    red.end_hour(&mut s2).unwrap();
    assert_eq!(s2.tob[0].token_idx, 0);
    assert_eq!(red.tokens()[0].token_id, "y1");
}

#[test]
fn a_token_listed_in_two_events_makes_one_row_per_change_and_one_depth_snapshot() {
    // FeedBooks keeps one book per leg slot; a token live in two events resolves to its last
    // live slot. The reducer works per token, so it must not emit twice for it.
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["y1", "y2"])],
        &[("e1", 0), ("e2", 0)],
    );
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(T0 + 3_000, 0, book("y2", T0 + 2_990, &[("0.20", "1")], &[])),
            frame(
                T0 + 4_000,
                0,
                change("y1", T0 + 3_995, "BUY", "0.41", "2", "0.41", "1"),
            ),
            frame(T0 + 300_500, 0, trade("y2", T0 + 300_495)),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob
            .iter()
            .map(|t| (t.token_idx, t.bid))
            .collect::<Vec<_>>(),
        vec![
            (0, Some(m("0.40"))),
            (1, Some(m("0.20"))),
            (0, Some(m("0.41")))
        ]
    );
    assert_eq!(red.tokens().len(), 2);
    assert_eq!(
        red.tokens()[0].event_id,
        "e2",
        "the last live listing's metadata, as the engine resolves it"
    );
    let at: Vec<(i64, u32, u16)> = s
        .depth
        .iter()
        .map(|d| (d.ts_ms - T0, d.token_idx, d.level))
        .collect();
    assert_eq!(at, vec![(300_000, 0, 0), (300_000, 0, 1), (300_000, 1, 0)]);
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 3_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

// ---- fix round 1 ----

/// A universe line whose record cannot be read.
fn uni_missing(t: i64) -> tape::line::Line {
    tape::line::Line {
        recv_ts_us: t * 1000,
        shard: 0,
        raw: None,
        ctl: Some(tape::line::Ctl::Universe {
            taken_ms: T0,
            file: "missing".into(),
        }),
    }
}

/// (recv offset, bid, bid size, ask) of each tob row.
type Top4 = (i64, Option<i64>, Option<i64>, Option<i64>);

fn tops(s: &VecSink) -> Vec<Top4> {
    s.tob
        .iter()
        .map(|t| (t.recv_ms - T0, t.bid, t.bid_sz, t.ask))
        .collect()
}

#[test]
fn a_universe_refresh_that_empties_a_duplicated_tokens_book_untrusts_it() {
    // The reviewer's probe: FeedBooks (like the engine) hands the last live slot of a token
    // listed twice an empty book on every refresh; a delta on it must not become a row.
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["y1"])],
        &[("e1", 0), ("e2", 0)],
    );
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.60", "10")]),
            ),
            uni(T0 + 3_000, &r),
            frame(
                T0 + 4_000,
                0,
                change("y1", T0 + 3_995, "BUY", "0.30", "5", "0.40", "0.60"),
            ), // no row
            frame(
                T0 + 5_000,
                0,
                book("y1", T0 + 4_995, &[("0.40", "12")], &[("0.60", "10")]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        tops(&s),
        vec![
            (2_000, Some(m("0.40")), Some(m("10")), Some(m("0.60"))),
            (5_000, Some(m("0.40")), Some(m("12")), Some(m("0.60"))),
        ]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 3_000, T0 + 5_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn a_token_moving_from_canary_to_a_group_leg_waits_for_a_new_snapshot() {
    let mut canary = rec("u1", T0, &[], &[]);
    canary.universe.canary = vec![tape::universe_view::CanaryView {
        market_id: "c1".into(),
        event_id: "ec".into(),
        yes_token: "y1".into(),
        no_token: "n1".into(),
    }];
    canary.conn_of.insert("canary:c1".into(), 0);
    let group = rec("u2", T0 + 3_000, &[("e1", &["y1"])], &[("e1", 0)]);
    let recs = [canary.clone(), group.clone()];
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &canary),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            uni(T0 + 3_000, &group), // the group leg starts from an empty book
            frame(
                T0 + 4_000,
                0,
                change("y1", T0 + 3_995, "BUY", "0.30", "5", "0.40", "1"),
            ), // no row
            frame(T0 + 5_000, 0, book("y1", T0 + 4_995, &[("0.41", "1")], &[])),
        ],
        &recs,
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob
            .iter()
            .map(|t| (t.recv_ms - T0, t.bid))
            .collect::<Vec<_>>(),
        vec![(2_000, Some(m("0.40"))), (5_000, Some(m("0.41")))]
    );
    // n1 (never snapshotted) leaves with the canary: its shard gap closes then (gone → forget)
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 3_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 3_000, T0 + 5_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn a_value_too_large_for_the_archive_is_a_bad_line_and_the_token_waits_for_a_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 1_500, 0, book("y2", T0 + 1_490, &[("0.20", "1")], &[])),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "99999999999999")], &[]),
            ), // top unfit
            frame(T0 + 3_000, 0, book("y1", T0 + 2_990, &[("0.41", "1")], &[])),
            frame(
                T0 + 4_000,
                0,
                book(
                    "y1",
                    T0 + 3_990,
                    &[("0.40", "1"), ("0.39", "99999999999999")],
                    &[],
                ),
            ), // level 1 unfit
            frame(T0 + 300_500, 0, trade("other", T0 + 300_495)), // depth: y1 unfit, y2 fine
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let st = red.end_hour(&mut s).unwrap();
    assert_eq!(st.bad_lines, 2);
    assert_eq!(
        s.tob
            .iter()
            .map(|t| (t.recv_ms - T0, t.token_idx, t.bid))
            .collect::<Vec<_>>(),
        vec![
            (1_500, 0, Some(m("0.20"))),
            (3_000, 1, Some(m("0.41"))),
            (4_000, 1, Some(m("0.40"))),
        ]
    );
    assert_eq!(
        s.depth,
        vec![depth(T0 + 300_000, 0, SIDE_BID, 0, "0.20", "1")]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 2_000, T0 + 3_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 300_500, T0 + H, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn a_missing_universe_record_is_a_whole_feed_gap_until_the_next_record() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            uni_missing(T0 + 10_000),
            frame(
                T0 + 11_000,
                0,
                book("y1", T0 + 10_995, &[("0.43", "1")], &[]),
            ), // universe unknown: no row
            uni(T0 + 20_000, &r),
            frame(
                T0 + 21_000,
                0,
                book("y1", T0 + 20_995, &[("0.45", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let st = red.end_hour(&mut s).unwrap();
    assert_eq!(st.universe_missing, 1);
    assert_eq!(
        s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.40")), Some(m("0.45"))]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 10_000, T0 + 20_000, None, reason::RAW_HOUR_LOST),
            gap(T0 + 20_000, T0 + 21_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn a_missing_hour_with_the_scanner_stopped_is_one_scanner_stopped_gap() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let mut s1 = VecSink::default();
    red.begin_hour(T0 + H, vec![]);
    red.missing_hour(reason::SCANNER_STOPPED, &mut s1).unwrap();
    red.end_hour(&mut s1).unwrap();
    assert_eq!(
        s1.gaps,
        vec![gap(T0 + H, T0 + 2 * H, None, reason::SCANNER_STOPPED)]
    );
    let mut s2 = VecSink::default();
    red.begin_hour(T0 + 2 * H, vec![]);
    run(
        &mut red,
        &[
            frame(
                T0 + 2 * H + 500,
                0,
                change("y1", T0 + 2 * H + 495, "BUY", "0.41", "1", "0.41", "1"),
            ), // stale: no row
            frame(
                T0 + 2 * H + 1_000,
                0,
                book("y1", T0 + 2 * H + 995, &[("0.42", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s2,
    );
    red.end_hour(&mut s2).unwrap();
    assert_eq!(
        s2.tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.42"))]
    );
    assert_eq!(
        s2.gaps,
        vec![
            gap(T0 + 2 * H, T0 + 2 * H + 500, None, reason::SCANNER_STOPPED),
            gap(
                T0 + 2 * H + 500,
                T0 + 2 * H + 1_000,
                Some(0),
                reason::NO_BOOK_YET
            ),
        ]
    );
}

#[test]
fn a_token_leaving_the_universe_closes_its_shard_gap_and_carried_books_stay_trusted() {
    let r1 = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let r2 = rec("u2", T0 + 5_000, &[("e1", &["y1"])], &[("e1", 0)]);
    let recs = [r1.clone(), r2.clone()];
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r1),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            uni(T0 + 5_000, &r2), // y2 (still waiting) is gone
            frame(
                T0 + 6_000,
                0,
                change("y1", T0 + 5_995, "BUY", "0.41", "1", "0.41", "1"),
            ), // carried: a row
        ],
        &recs,
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.tob.iter().map(|t| t.bid).collect::<Vec<_>>(),
        vec![Some(m("0.40")), Some(m("0.41"))]
    );
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 5_000, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn depth_skips_a_changed_token_that_is_no_longer_trusted() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0), ("zz", 1)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(
                T0 + 10_000,
                0,
                change("y1", T0 + 9_995, "BUY", "0.41", "1", "0.41", "1"),
            ),
            down(T0 + 20_000, 0),
            frame(T0 + 300_500, 1, trade("other", T0 + 300_495)),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert!(s.depth.is_empty(), "{:?}", s.depth);
}

#[test]
fn a_shard_gap_continues_into_the_next_hour() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            down(T0 + 400_000, 0),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.gaps.last(),
        Some(&gap(T0 + 400_000, T0 + H, Some(0), reason::DOWN))
    );
    let mut s1 = VecSink::default();
    red.begin_hour(T0 + H, vec![]);
    run(
        &mut red,
        &[frame(
            T0 + H + 1_000,
            0,
            book("y1", T0 + H + 995, &[("0.42", "1")], &[]),
        )],
        std::slice::from_ref(&r),
        &mut s1,
    );
    red.end_hour(&mut s1).unwrap();
    assert_eq!(
        s1.gaps,
        vec![gap(T0 + H, T0 + H + 1_000, Some(0), reason::DOWN)]
    );
    assert_eq!(s1.tob.len(), 1);
}

#[test]
fn skipped_depth_boundaries_stamp_the_first_boundary_after_the_change() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(
                T0 + 300_500,
                0,
                change("y1", T0 + 300_495, "BUY", "0.41", "1", "0.41", "1"),
            ),
            frame(
                T0 + 1_200_500,
                0,
                change("y1", T0 + 1_200_495, "BUY", "0.42", "1", "0.42", "1"),
            ),
            frame(T0 + 1_500_500, 0, trade("y1", T0 + 1_500_495)),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let mut at: Vec<i64> = s.depth.iter().map(|d| d.ts_ms - T0).collect();
    at.dedup();
    assert_eq!(at, vec![300_000, 600_000, 1_500_000]);
}

#[test]
fn cut_makes_the_rest_of_the_hour_raw_hour_lost() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])), // y2 still waiting
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.cut(reason::RAW_HOUR_LOST, &mut s).unwrap();
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_001, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 2_001, T0 + H, None, reason::RAW_HOUR_LOST),
        ]
    );
    let mut s1 = VecSink::default();
    red.begin_hour(T0 + H, vec![]);
    run(
        &mut red,
        &[
            frame(
                T0 + H + 500,
                0,
                change("y1", T0 + H + 495, "BUY", "0.41", "1", "0.41", "1"),
            ), // untrusted: no row
        ],
        std::slice::from_ref(&r),
        &mut s1,
    );
    red.end_hour(&mut s1).unwrap();
    assert!(s1.tob.is_empty());
    assert_eq!(
        s1.gaps,
        vec![
            gap(T0 + H, T0 + H + 500, None, reason::RAW_HOUR_LOST),
            gap(T0 + H + 500, T0 + 2 * H, Some(0), reason::NO_BOOK_YET),
        ]
    );
}

#[test]
fn cut_after_a_shutdown_is_raw_hour_lost_from_the_cut() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            shutdown(T0 + 10_000),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.cut(reason::RAW_HOUR_LOST, &mut s).unwrap();
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.gaps,
        vec![
            gap(T0, T0 + 1_000, None, reason::NO_BOOK_YET),
            gap(T0 + 1_000, T0 + 2_000, Some(0), reason::NO_BOOK_YET),
            gap(T0 + 10_000, T0 + 10_001, None, reason::SCANNER_STOPPED),
            gap(T0 + 10_001, T0 + H, None, reason::RAW_HOUR_LOST),
        ]
    );
}

/// Three hours exercising carried state: open shard gaps, a lost universe across an hour end,
/// a group losing and regaining its connection, a shutdown, depth boundaries.
fn three_hours() -> (
    Vec<Vec<tape::line::Line>>,
    Vec<tape::universe_view::RecordView>,
) {
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1", "y2"]), ("e2", &["y3"])],
        &[("e1", 0), ("e2", 1)],
    );
    let r2 = rec(
        "u2",
        T0 + H,
        &[("e1", &["y1", "y2"]), ("e2", &["y3"])],
        &[("e1", 0)],
    );
    let hours = vec![
        vec![
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book(
                    "y1",
                    T0 + 1_990,
                    &[("0.40", "1"), ("0.39", "2")],
                    &[("0.45", "3")],
                ),
            ),
            frame(T0 + 3_000, 0, book("y2", T0 + 2_990, &[("0.20", "1")], &[])),
            frame(
                T0 + 4_000,
                1,
                book("y3", T0 + 3_990, &[("0.30", "1")], &[("0.35", "1")]),
            ),
            frame(
                T0 + 301_000,
                0,
                change("y1", T0 + 300_995, "BUY", "0.41", "1", "0.41", "0.45"),
            ),
            down(T0 + 400_000, 0),
            frame(
                T0 + 3_000_000,
                1,
                change("y3", T0 + 2_999_995, "BUY", "0.31", "1", "0.31", "0.35"),
            ),
            uni_missing(T0 + 3_500_000),
        ],
        vec![
            frame(
                T0 + H + 1_000,
                1,
                change("y3", T0 + H + 995, "BUY", "0.32", "1", "0.32", "0.35"),
            ),
            uni(T0 + H + 2_000, &r2),
            frame(
                T0 + H + 3_000,
                0,
                book("y1", T0 + H + 2_995, &[("0.42", "1")], &[("0.44", "1")]),
            ),
            frame(
                T0 + H + 4_000,
                0,
                book("y2", T0 + H + 3_995, &[("0.21", "1")], &[]),
            ),
            frame(
                T0 + H + 400_000,
                0,
                change("y1", T0 + H + 399_995, "SELL", "0.44", "0", "0.42", "1"),
            ),
        ],
        vec![
            uni(T0 + 2 * H + 1_000, &r),
            frame(
                T0 + 2 * H + 2_000,
                1,
                book("y3", T0 + 2 * H + 1_995, &[("0.33", "1")], &[]),
            ),
            shutdown(T0 + 2 * H + 10_000),
            uni(T0 + 2 * H + 20_000, &r),
            frame(
                T0 + 2 * H + 21_000,
                0,
                book("y1", T0 + 2 * H + 20_995, &[("0.43", "1")], &[]),
            ),
            frame(
                T0 + 2 * H + 700_000,
                0,
                change("y1", T0 + 2 * H + 699_995, "BUY", "0.44", "1", "0.44", "1"),
            ),
            frame(T0 + 2 * H + 1_000_000, 1, trade("y3", T0 + 2 * H + 999_995)),
        ],
    ];
    (hours, vec![r, r2])
}

type HourOut = (VecSink, Vec<TokenRow>, compact::reduce::HourStats);

fn run_hours(split: bool) -> Vec<HourOut> {
    run_hours_with(split, &[])
}

/// `eps`: the episodes whose l2 windows each hour gets (as the service reads them).
fn run_hours_with(split: bool, eps: &[store::CompactEpisode]) -> Vec<HourOut> {
    let (hours, recs) = three_hours();
    let c = cfg(std::path::Path::new("/r"), std::path::Path::new("/a"));
    let mut red = Reducer::new(rcfg());
    let mut out = Vec::new();
    for (i, lines) in hours.iter().enumerate() {
        if split {
            let json = serde_json::to_string(&red.state()).unwrap();
            red = Reducer::from_state(rcfg(), serde_json::from_str(&json).unwrap()).unwrap();
        }
        let mut s = VecSink::default();
        let hour = T0 + i as i64 * H;
        red.begin_hour(hour, compact::windows::windows_for_hour(eps, hour, &c));
        run(&mut red, lines, &recs, &mut s);
        let st = red.end_hour(&mut s).unwrap();
        out.push((s, red.tokens().to_vec(), st));
    }
    out
}

#[test]
fn a_checkpoint_round_trip_at_every_hour_gives_the_same_rows_as_one_run() {
    let whole = run_hours(false);
    let split = run_hours(true);
    assert_eq!(whole, split);
    // not vacuous: carried state matters in every hour
    assert!(
        whole
            .iter()
            .all(|(s, _, _)| !s.tob.is_empty() && !s.gaps.is_empty()),
        "{whole:?}"
    );
    assert!(whole.iter().any(|(s, _, _)| !s.depth.is_empty()));
    assert!(
        whole[1]
            .0
            .gaps
            .contains(&gap(T0 + H, T0 + H + 2_000, None, reason::RAW_HOUR_LOST)),
        "{:?}",
        whole[1].0.gaps
    );
    assert!(
        whole[1]
            .0
            .gaps
            .iter()
            .any(|g| g.shard == Some(1) && g.reason == reason::DOWN),
        "{:?}",
        whole[1].0.gaps
    );
}

#[test]
fn a_checkpoint_round_trip_keeps_l2_windows_that_span_hours() {
    let ep = |id: &str, event: &str, opened: i64, closed: Option<i64>| store::CompactEpisode {
        episode_id: id.into(),
        event_id: event.into(),
        opened_ms: opened,
        closed_ms: closed,
    };
    let eps = [
        // [T0 + 3 000 000, T0 + H + 600 000): y3 is trusted in hour 0, lost to the missing
        // universe record, and never snapshotted again inside the window
        ep("a", "e2", T0 + H - 300_000, Some(T0 + H + 300_000)),
        // [T0 + H − 200 000, T0 + H + 1 600 000), still open (capped): y1, y2 wait in hour 0
        // (down), restart with snapshots in hour 1
        ep("c", "e1", T0 + H + 100_000, None),
        // [T0 + 2H − 500 000, …): y3 has no book in hour 1 (no connection), a snapshot in hour 2
        ep("b", "e2", T0 + 2 * H - 200_000, None),
    ];
    let whole = run_hours_with(false, &eps);
    let split = run_hours_with(true, &eps);
    assert_eq!(whole, split);
    let l2: Vec<Vec<(i64, u32, bool, &str)>> = whole
        .iter()
        .map(|(s, _, _)| {
            s.l2.iter()
                .map(|x| {
                    (
                        x.recv_ms - T0,
                        x.token_idx,
                        x.snapshot,
                        x.episode_id.as_str(),
                    )
                })
                .collect()
        })
        .collect();
    let (y1, y2, y3) = (0, 1, 2);
    assert_eq!(
        l2,
        vec![
            vec![
                (3_000_000, y3, true, "a"),
                (3_000_000, y3, true, "a"),
                (3_000_000, y3, false, "a"),
            ],
            vec![
                (H + 3_000, y1, true, "c"),
                (H + 3_000, y1, true, "c"),
                (H + 4_000, y2, true, "c"),
                (H + 400_000, y1, false, "c"),
            ],
            vec![(2 * H + 2_000, y3, true, "b")],
        ]
    );
}
