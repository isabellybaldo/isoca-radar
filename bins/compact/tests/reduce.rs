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
            gap(T0 + 10_001, T0 + 20_000, None, reason::SCANNER_STOPPED),
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
