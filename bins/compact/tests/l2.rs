#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::reduce::Reducer;
use compact::rows::*;
use compact::windows::*;
use fixture::*;
use std::path::Path;
use store::CompactEpisode;

fn ep(id: &str, opened: i64, closed: Option<i64>) -> CompactEpisode {
    CompactEpisode {
        episode_id: id.into(),
        event_id: "e1".into(),
        opened_ms: opened,
        closed_ms: closed,
    }
}

fn win(id: &str, start: i64, end: i64) -> Window {
    Window {
        episode_id: id.into(),
        event_id: "e1".into(),
        opened_ms: start + 300_000,
        start_ms: start,
        end_ms: end,
        truncated: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn l2(ts: i64, recv: i64, idx: u32, side: u8, p: &str, s: &str, snapshot: bool, id: &str) -> L2Row {
    L2Row {
        ts_ms: ts,
        recv_ms: recv,
        token_idx: idx,
        side,
        price: m(p),
        new_size: m(s),
        snapshot,
        episode_id: id.into(),
    }
}

#[test]
fn window_math_follows_the_spec() {
    let (b, a, max) = (300_000, 300_000, 1_800_000);
    let w = window_of(&ep("x", T0 + 1_000_000, Some(T0 + 1_060_000)), b, a, max);
    assert_eq!(
        (w.start_ms, w.end_ms, w.truncated),
        (T0 + 700_000, T0 + 1_360_000, false)
    );
    let long = window_of(&ep("y", T0 + 1_000_000, Some(T0 + 3_000_000)), b, a, max);
    assert_eq!((long.end_ms, long.truncated), (T0 + 700_000 + max, true));
    let open = window_of(&ep("z", T0 + 1_000_000, None), b, a, max);
    assert_eq!((open.end_ms, open.truncated), (T0 + 700_000 + max, true));
    let c = cfg(Path::new("/r"), Path::new("/a"));
    assert_eq!(
        query_range(T0 + H, &c),
        (T0 + H - 1_800_000 + 300_000, T0 + 2 * H + 300_000)
    );
    let eps = [
        ep("late", T0 + H + 400_000, None), // window starts after this hour
        ep("b", T0 + 10_000, Some(T0 + 20_000)),
        ep("a", T0 + 10_000, Some(T0 + 20_000)),
        ep("before", T0 - 3_000_000, Some(T0 - 2_900_000)), // ended before this hour
    ];
    let ws = windows_for_hour(&eps, T0, &c);
    assert_eq!(
        ws.iter().map(|w| w.episode_id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn an_l2_window_starts_with_snapshot_rows_then_exact_changes() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 200_000)]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "10")]),
            ),
            frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[("0.20", "3")], &[])),
            frame(
                T0 + 50_000,
                0,
                change("y1", T0 + 49_995, "BUY", "0.39", "1", "0.40", "0.45"),
            ), // before the window
            frame(
                T0 + 100_500,
                0,
                change("y1", T0 + 100_495, "BUY", "0.41", "2", "0.41", "0.46"),
            ), // snapshots, then this change
            frame(
                T0 + 150_000,
                0,
                change("y1", T0 + 149_995, "BUY", "0.38", "4", "0.41", "0.46"),
            ), // settles 0.46: 0.45 trimmed
            frame(
                T0 + 200_500,
                0,
                change("y1", T0 + 200_495, "BUY", "0.37", "1", "0.41", "0.46"),
            ), // after the window
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let (t1, t2) = (T0 + 100_500, T0 + 150_000);
    assert_eq!(
        s.l2,
        vec![
            // snapshot rows: stamped max(book server ts, window start) (fix round 1, ruling 3)
            l2(T0 + 100_000, t1, 0, SIDE_BID, "0.40", "10", true, "ep"),
            l2(T0 + 100_000, t1, 0, SIDE_BID, "0.39", "1", true, "ep"),
            l2(T0 + 100_000, t1, 0, SIDE_ASK, "0.45", "10", true, "ep"),
            l2(T0 + 100_000, t1, 1, SIDE_BID, "0.20", "3", true, "ep"),
            l2(T0 + 100_495, t1, 0, SIDE_BID, "0.41", "2", false, "ep"),
            l2(T0 + 149_995, t2, 0, SIDE_BID, "0.38", "4", false, "ep"),
            l2(T0 + 149_995, t2, 0, SIDE_ASK, "0.45", "0", false, "ep"),
        ]
    );
}

#[test]
fn overlapping_windows_merge_and_rows_belong_to_the_first_episode() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(
        T0,
        vec![
            win("b", T0 + 150_000, T0 + 300_000),
            win("a", T0 + 100_000, T0 + 200_000),
        ],
    );
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[]),
            ),
            frame(
                T0 + 160_000,
                0,
                change("y1", T0 + 159_995, "BUY", "0.41", "1", "0.41", "1"),
            ),
            frame(
                T0 + 210_000,
                0,
                change("y1", T0 + 209_995, "BUY", "0.42", "1", "0.42", "1"),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let ids: Vec<(&str, bool)> =
        s.l2.iter()
            .map(|x| (x.episode_id.as_str(), x.snapshot))
            .collect();
    assert_eq!(
        ids,
        vec![
            ("a", true),  // a starts first: its snapshot at 160 000
            ("a", false), // 0.41 belongs to a (b overlaps, a opened first)
            ("b", true),
            ("b", true), // a ended at 200 000: b takes over with a fresh snapshot (0.41, 0.40)
            ("b", false), // 0.42
        ]
    );
}

#[test]
fn an_untrusted_or_empty_book_in_a_window_starts_with_its_first_real_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[]),
            ),
            frame(T0 + 2_100, 0, book("y2", T0 + 2_090, &[], &[])), // empty book
            down(T0 + 50_000, 0),                                   // y1, y2 untrusted
            frame(T0 + 100_500, 0, book("y2", T0 + 100_495, &[], &[])), // trusted, still empty: nothing
            frame(
                T0 + 101_000,
                0,
                change("y2", T0 + 100_995, "BUY", "0.20", "3", "0.20", "1"),
            ), // first level: a snapshot
            frame(
                T0 + 102_000,
                0,
                change("y1", T0 + 101_995, "BUY", "0.41", "1", "0.41", "1"),
            ), // untrusted: nothing
            frame(
                T0 + 103_000,
                0,
                book("y1", T0 + 102_995, &[("0.42", "1")], &[]),
            ), // its snapshot
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let rows: Vec<(u32, bool, i64)> =
        s.l2.iter()
            .map(|x| (x.token_idx, x.snapshot, x.price))
            .collect();
    let (y1, y2) = (0, 1);
    assert_eq!(rows, vec![(y2, true, m("0.20")), (y1, true, m("0.42"))]);
}

#[test]
fn a_window_across_midnight_restarts_with_snapshots_in_the_new_day() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let d = T0 + 23 * H;
    let w = Window {
        episode_id: "ep".into(),
        event_id: "e1".into(),
        opened_ms: d + 3_300_000,
        start_ms: d + 3_000_000,
        end_ms: d + H + 600_000,
        truncated: false,
    };
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(d, vec![w.clone()]);
    run(
        &mut red,
        &[
            uni(d + 1_000, &r),
            frame(d + 2_000, 0, book("y1", d + 1_990, &[("0.40", "10")], &[])),
            frame(
                d + 3_100_000,
                0,
                change("y1", d + 3_099_995, "BUY", "0.41", "1", "0.41", "1"),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        s.l2.iter().map(|x| x.snapshot).collect::<Vec<_>>(),
        vec![true, false]
    );
    let mut s2 = VecSink::default();
    red.begin_hour(d + H, vec![w]);
    run(
        &mut red,
        &[frame(
            d + H + 5_000,
            0,
            change("y1", d + H + 4_995, "BUY", "0.42", "1", "0.42", "1"),
        )],
        std::slice::from_ref(&r),
        &mut s2,
    );
    red.end_hour(&mut s2).unwrap();
    let rows: Vec<(u32, bool, i64)> = s2
        .l2
        .iter()
        .map(|x| (x.token_idx, x.snapshot, x.price))
        .collect();
    assert_eq!(
        rows,
        vec![
            (0, true, m("0.41")),
            (0, true, m("0.40")),
            (0, false, m("0.42"))
        ],
        "a new day: new dictionary, fresh snapshot first"
    );
}

/// (recv offset, token, snapshot, price, new_size, episode)
type Brief = (i64, u32, bool, i64, i64, String);

fn brief(s: &VecSink) -> Vec<Brief> {
    s.l2.iter()
        .map(|x| {
            (
                x.recv_ms - T0,
                x.token_idx,
                x.snapshot,
                x.price,
                x.new_size,
                x.episode_id.clone(),
            )
        })
        .collect()
}

fn row(recv: i64, idx: u32, snapshot: bool, p: &str, s: &str, id: &str) -> Brief {
    (recv, idx, snapshot, m(p), m(s), id.to_string())
}

#[test]
fn l2_follows_the_trust_rules_and_a_token_in_two_events_has_one_stream() {
    // y1 is live in e1 and e2: FeedBooks resolves it to its last slot, and a universe refresh
    // hands that slot an empty book (untrusted until a snapshot). One stream, owned by the
    // episode that opened first ("b", on e2), never two.
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["y1"])],
        &[("e1", 0), ("e2", 0)],
    );
    let mut b = win("b", T0 + 1_500, T0 + 900_000);
    b.event_id = "e2".into();
    let mut a = win("a", T0 + 1_600, T0 + 900_000);
    a.opened_ms = b.opened_ms + 1;
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![a, b]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.60", "10")]),
            ), // first snapshot: rows
            uni(T0 + 3_000, &r), // the book is replaced: untrusted
            frame(
                T0 + 4_000,
                0,
                change("y1", T0 + 3_995, "BUY", "0.30", "5", "0.40", "0.60"),
            ), // no row
            frame(
                T0 + 5_000,
                0,
                book("y1", T0 + 4_995, &[("0.40", "12")], &[("0.60", "10")]),
            ), // restarts with a snapshot
            down(T0 + 6_000, 0),
            frame(
                T0 + 7_000,
                0,
                change("y1", T0 + 6_995, "BUY", "0.30", "5", "0.40", "0.60"),
            ), // no row
            frame(T0 + 8_000, 0, book("y1", T0 + 7_995, &[("0.41", "1")], &[])),
            frame(
                T0 + 9_000,
                0,
                change("y1", T0 + 8_995, "SELL", "0.50", "2", "0.41", "0.50"),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    assert_eq!(
        brief(&s),
        vec![
            row(2_000, 0, true, "0.40", "10", "b"),
            row(2_000, 0, true, "0.60", "10", "b"),
            row(5_000, 0, true, "0.40", "12", "b"),
            row(5_000, 0, true, "0.60", "10", "b"),
            row(8_000, 0, true, "0.41", "1", "b"),
            row(9_000, 0, false, "0.50", "2", "b"),
        ]
    );
}

#[test]
fn an_unfit_value_in_a_window_is_a_bad_line_and_the_token_restarts_with_its_next_snapshot() {
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(T0 + 2_000, 0, book("y1", T0 + 1_990, &[("0.40", "1")], &[])),
            frame(
                T0 + 2_100,
                0,
                book(
                    "y2",
                    T0 + 2_090,
                    &[("0.20", "1"), ("0.19", "99999999999999")],
                    &[],
                ),
            ), // its top fits: trusted, a tob row
            // the window opens: y1's snapshot; y2's does not fit (untrusted, no row); then y1's
            // change does not fit either (untrusted, no row). One bad line.
            frame(
                T0 + 100_500,
                0,
                change(
                    "y1",
                    T0 + 100_495,
                    "BUY",
                    "0.39",
                    "99999999999999",
                    "0.40",
                    "1",
                ),
            ),
            frame(
                T0 + 101_000,
                0,
                change("y1", T0 + 100_995, "BUY", "0.38", "1", "0.40", "1"),
            ), // untrusted: nothing
            frame(
                T0 + 102_000,
                0,
                book("y1", T0 + 101_995, &[("0.41", "1")], &[]),
            ),
            frame(
                T0 + 103_000,
                0,
                book("y2", T0 + 102_995, &[("0.21", "2")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let st = red.end_hour(&mut s).unwrap();
    assert_eq!(st.bad_lines, 1);
    assert_eq!(
        brief(&s),
        vec![
            row(100_500, 0, true, "0.40", "1", "ep"),
            row(102_000, 0, true, "0.41", "1", "ep"),
            row(103_000, 1, true, "0.21", "2", "ep"),
        ]
    );
    assert!(
        s.gaps.contains(&GapRow {
            start_ms: T0 + 100_500,
            end_ms: T0 + 103_000,
            shard: Some(0),
            reason: reason::NO_BOOK_YET.into(),
        }),
        "{:?}",
        s.gaps
    );
}

/// y1 streams 0.40/10, 0.41/2 (bids) and 0.60/5 (ask) under "ep" from 100 500.
fn streaming_lines() -> (tape::universe_view::RecordView, Vec<tape::line::Line>) {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let lines = vec![
        uni(T0 + 1_000, &r),
        frame(
            T0 + 2_000,
            0,
            book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.60", "5")]),
        ),
        frame(
            T0 + 100_500,
            0,
            change("y1", T0 + 100_495, "BUY", "0.41", "2", "0.41", "0.60"),
        ),
    ];
    (r, lines)
}

fn streaming_rows() -> Vec<L2Row> {
    let t = T0 + 100_500;
    vec![
        l2(T0 + 100_000, t, 0, SIDE_BID, "0.40", "10", true, "ep"),
        l2(T0 + 100_000, t, 0, SIDE_ASK, "0.60", "5", true, "ep"),
        l2(T0 + 100_495, t, 0, SIDE_BID, "0.41", "2", false, "ep"),
    ]
}

#[test]
fn probe_empty_snapshot_while_streaming() {
    // A full book for a token already streaming is written as the exact difference from the
    // levels last written, as deltas would be: an empty book zeroes every level.
    let (r, mut lines) = streaming_lines();
    lines.extend([
        frame(T0 + 101_000, 0, book("y1", T0 + 100_995, &[], &[])),
        frame(
            T0 + 102_000,
            0,
            book("y1", T0 + 101_995, &[("0.30", "1")], &[]),
        ),
    ]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(&mut red, &lines, std::slice::from_ref(&r), &mut s);
    red.end_hour(&mut s).unwrap();
    let (a, b) = (T0 + 101_000, T0 + 102_000);
    let mut want = streaming_rows();
    want.extend([
        l2(T0 + 100_995, a, 0, SIDE_BID, "0.40", "0", false, "ep"),
        l2(T0 + 100_995, a, 0, SIDE_BID, "0.41", "0", false, "ep"),
        l2(T0 + 100_995, a, 0, SIDE_ASK, "0.60", "0", false, "ep"),
        l2(T0 + 101_995, b, 0, SIDE_BID, "0.30", "1", false, "ep"),
    ]);
    assert_eq!(s.l2, want);
}

#[test]
fn probe_down_then_empty_snapshot() {
    // A restart onto an empty book zeroes the levels written earlier this hour, then waits for
    // the first non-empty state, which is a snapshot.
    let (r, mut lines) = streaming_lines();
    lines.extend([
        down(T0 + 101_000, 0),
        frame(T0 + 102_000, 0, book("y1", T0 + 101_995, &[], &[])),
        frame(
            T0 + 103_000,
            0,
            change("y1", T0 + 102_995, "BUY", "0.30", "1", "0.30", "1"),
        ),
    ]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(&mut red, &lines, std::slice::from_ref(&r), &mut s);
    red.end_hour(&mut s).unwrap();
    let (a, b) = (T0 + 102_000, T0 + 103_000);
    let mut want = streaming_rows();
    want.extend([
        l2(T0 + 101_995, a, 0, SIDE_BID, "0.40", "0", false, "ep"),
        l2(T0 + 101_995, a, 0, SIDE_BID, "0.41", "0", false, "ep"),
        l2(T0 + 101_995, a, 0, SIDE_ASK, "0.60", "0", false, "ep"),
        l2(T0 + 102_995, b, 0, SIDE_BID, "0.30", "1", true, "ep"),
    ]);
    assert_eq!(s.l2, want);
}

#[test]
fn window_edges_are_exact() {
    let (b, a, max) = (300_000, 300_000, 1_800_000);
    // natural end exactly at the cap: not truncated; one past: truncated
    let at_cap = window_of(&ep("x", T0 + 1_000_000, Some(T0 + 2_200_000)), b, a, max);
    assert_eq!((at_cap.end_ms, at_cap.truncated), (T0 + 2_500_000, false));
    let past = window_of(&ep("y", T0 + 1_000_000, Some(T0 + 2_200_001)), b, a, max);
    assert_eq!((past.end_ms, past.truncated), (T0 + 2_500_000, true));
    let c = cfg(Path::new("/r"), Path::new("/a"));
    let h = T0 + H;
    let eps = [
        ep("ends_at_hour", h - 600_000, Some(h - 300_000)), // [h − 900 000, h): out
        ep("ends_after", h - 600_000, Some(h - 299_999)),   // ends at h + 1: in
        ep("starts_at_next", h + H + 300_000, None),        // starts at h + H: out
        ep("starts_before_next", h + H + 299_999, None),    // starts at h + H − 1: in
    ];
    let ws = windows_for_hour(&eps, h, &c);
    assert_eq!(
        ws.iter().map(|w| w.episode_id.as_str()).collect::<Vec<_>>(),
        vec!["ends_after", "starts_before_next"]
    );
}

#[test]
fn a_window_owns_lines_from_its_start_to_just_before_its_end() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 200_000)]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10")], &[]),
            ),
            frame(
                T0 + 100_000,
                0,
                change("y1", T0 + 99_995, "BUY", "0.41", "1", "0.41", "1"),
            ), // exactly at the start: in
            frame(
                T0 + 200_000,
                0,
                change("y1", T0 + 199_995, "BUY", "0.42", "1", "0.42", "1"),
            ), // exactly at the end: out
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    red.end_hour(&mut s).unwrap();
    let t = T0 + 100_000;
    assert_eq!(
        s.l2,
        vec![
            l2(T0 + 100_000, t, 0, SIDE_BID, "0.40", "10", true, "ep"),
            l2(T0 + 100_000, t, 0, SIDE_BID, "0.41", "1", false, "ep"),
        ]
    );
}

#[test]
fn a_tokens_l2_ts_never_goes_back() {
    let r = rec("u1", T0, &[("e1", &["y1"])], &[("e1", 0)]);
    let mut red = Reducer::new(rcfg());
    let mut s = VecSink::default();
    red.begin_hour(T0, vec![win("ep", T0 + 100_000, T0 + 900_000)]);
    run(
        &mut red,
        &[
            uni(T0 + 1_000, &r),
            frame(
                T0 + 2_000,
                0,
                book("y1", T0 + 1_990, &[("0.40", "10"), ("0.39", "1")], &[]),
            ),
            frame(
                T0 + 100_500,
                0,
                change("y1", T0 + 100_495, "BUY", "0.38", "1", "0.40", "1"),
            ),
            // a full book with an older server ts (same top: no tob row): 0.38 is gone
            frame(
                T0 + 101_000,
                0,
                book("y1", T0 + 100_200, &[("0.40", "10"), ("0.39", "1")], &[]),
            ),
        ],
        std::slice::from_ref(&r),
        &mut s,
    );
    let st = red.end_hour(&mut s).unwrap();
    let (a, b) = (T0 + 100_500, T0 + 101_000);
    assert_eq!(
        s.l2,
        vec![
            // the book's ts (1 990) is before the window: stamped at its start
            l2(T0 + 100_000, a, 0, SIDE_BID, "0.40", "10", true, "ep"),
            l2(T0 + 100_000, a, 0, SIDE_BID, "0.39", "1", true, "ep"),
            l2(T0 + 100_495, a, 0, SIDE_BID, "0.38", "1", false, "ep"),
            l2(T0 + 100_495, b, 0, SIDE_BID, "0.38", "0", false, "ep"), // raised from 100 200
        ]
    );
    assert_eq!(st.ts_clamped, 1);
}
