#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::{day_dir, part_path, sidecar_path};
use compact::hour::*;
use compact::pq::read_all;
use compact::reduce::Reducer;
use compact::rows::*;
use fixture::*;

#[test]
fn an_hour_writes_five_parts_and_a_sidecar_that_match_the_reducer() {
    let (raw, arch) = (tmp("hour-raw"), tmp("hour-arch"));
    let (r, h) = three_hours(&raw);
    let mut red = Reducer::new(rcfg());
    let report = process_hour(&mut red, &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    let mut want = VecSink::default();
    let mut red2 = Reducer::new(rcfg());
    red2.begin_hour(T0, vec![]);
    run(&mut red2, &h[0], &[r], &mut want);
    red2.end_hour(&mut want).unwrap();
    let day = day_dir(&arch, T0);
    assert_eq!(
        read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap(),
        want.tob
    );
    assert_eq!(
        read_all::<DepthRow>(&part_path(&day, 0, "depth")).unwrap(),
        want.depth
    );
    assert_eq!(
        read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap(),
        want.gaps
    );
    assert_eq!(
        read_all::<TokenRow>(&part_path(&day, 0, "tokens")).unwrap(),
        red2.tokens().to_vec()
    );
    assert!(
        read_all::<L2Row>(&part_path(&day, 0, "l2"))
            .unwrap()
            .is_empty()
    );
    let side: HourReport =
        serde_json::from_slice(&std::fs::read(sidecar_path(&day, 0)).unwrap()).unwrap();
    assert_eq!(side, report);
    let bytes = std::fs::read(tape::files::hour_path(&raw, T0)).unwrap();
    assert_eq!(
        report.source_bytes,
        Some(u64::try_from(bytes.len()).unwrap())
    );
    assert_eq!(
        report.source_sha256.as_deref(),
        Some(compact::config::sha256_hex(&bytes).as_str())
    );
    assert_eq!(
        report.source_file.as_deref(),
        Some("2026-09-23/00.jsonl.zst")
    );
    assert_eq!(
        (report.status.as_str(), report.rows.tob, report.rows.tokens),
        ("done", u64::try_from(want.tob.len()).unwrap(), 2)
    );
    assert!(
        std::fs::read_dir(&day)
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp"))
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_crash_before_the_rename_leaves_no_final_file_and_the_rerun_matches() {
    let raw = tmp("crash-raw");
    three_hours(&raw);
    let (a, b) = (tmp("crash-a"), tmp("crash-b"));
    let mut j = hour_job(&raw, &a, 0);
    j.stop_before_publish = true;
    assert!(
        process_hour(&mut Reducer::new(rcfg()), &j)
            .unwrap()
            .is_none()
    );
    let day = day_dir(&a, T0);
    assert!(
        !part_path(&day, 0, "tob").exists() && !sidecar_path(&day, 0).exists(),
        "nothing published"
    );
    assert!(
        compact::pq::tmp_of(&part_path(&day, 0, "tob")).exists(),
        "only temporary files"
    );
    process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &a, 0))
        .unwrap()
        .unwrap();
    process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &b, 0))
        .unwrap()
        .unwrap();
    assert_eq!(
        day_files(&a),
        day_files(&b),
        "the re-run equals a clean run"
    );
    for d in [&raw, &a, &b] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn a_truncated_hour_keeps_its_rows_and_gaps_the_rest() {
    let (raw, arch) = (tmp("trunc-raw"), tmp("trunc-arch"));
    let (r, h) = three_hours(&raw);
    // the scanner restarted and appended a second zstd frame, then was killed mid-write
    let p = tape::files::hour_path(&raw, T0);
    let mut bytes = std::fs::read(&p).unwrap();
    let mut second = Vec::new();
    for l in [
        uni(T0 + 500_000, &r),
        frame(
            T0 + 501_000,
            0,
            book("y1", T0 + 500_995, &[("0.30", "1")], &[]),
        ),
    ] {
        second.extend(serde_json::to_vec(&l).unwrap());
        second.push(b'\n');
    }
    let z = zstd::encode_all(second.as_slice(), 3).unwrap();
    bytes.extend_from_slice(&z[..z.len() - 6]);
    std::fs::write(&p, &bytes).unwrap();
    let report = process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    assert!(report.read_error.is_some(), "{report:?}");
    assert_eq!(
        report.status, "done",
        "the hour is recorded; compaction does not stall on it"
    );
    assert_eq!(
        report.source_bytes,
        Some(u64::try_from(bytes.len()).unwrap())
    );
    assert_eq!(
        report.source_sha256.as_deref(),
        Some(compact::config::sha256_hex(&bytes).as_str())
    );
    let day = day_dir(&arch, T0);
    let tob = read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap();
    let last_recv = h[0].last().unwrap().recv_ts_us / 1000;
    assert!(
        tob.iter().all(|t| t.recv_ms <= last_recv) && tob.len() >= 3,
        "rows before the break are kept: {tob:?}"
    );
    let gaps = read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap();
    assert!(
        gaps.contains(&GapRow {
            start_ms: last_recv + 1,
            end_ms: T0 + H,
            shard: None,
            reason: reason::RAW_HOUR_LOST.into()
        }),
        "{gaps:?}"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

fn encoded(lines: &[tape::line::Line]) -> Vec<u8> {
    let mut plain = Vec::new();
    for l in lines {
        plain.extend(serde_json::to_vec(l).unwrap());
        plain.push(b'\n');
    }
    zstd::encode_all(plain.as_slice(), 3).unwrap()
}

#[test]
fn a_frame_after_a_truncated_one_is_still_read() {
    let (raw, arch) = (tmp("resync-raw"), tmp("resync-arch"));
    let (r, h) = three_hours(&raw);
    let p = tape::files::hour_path(&raw, T0);
    // frame A (the fixture's hour), then frame B cut short by an unclean exit, a stray frame
    // magic (a false positive: not a frame), then frame C from the restarted scanner
    let mut bytes = std::fs::read(&p).unwrap();
    let b = encoded(&[
        uni(T0 + 500_000, &r),
        frame(
            T0 + 501_000,
            0,
            book("y1", T0 + 500_995, &[("0.30", "1")], &[]),
        ),
    ]);
    bytes.extend_from_slice(&b[..b.len() - 6]);
    bytes.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD, 0xff, 0xff, 0xff, 0xff, 0x00, 0x01]);
    bytes.extend(encoded(&[
        uni(T0 + 600_000, &r),
        frame(
            T0 + 601_000,
            0,
            book("y1", T0 + 600_995, &[("0.35", "4")], &[("0.38", "2")]),
        ),
        frame(
            T0 + 601_100,
            0,
            book("y2", T0 + 601_095, &[("0.22", "3")], &[("0.27", "1")]),
        ),
    ]));
    std::fs::write(&p, &bytes).unwrap();
    let report = process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    assert!(report.read_error.is_some(), "{report:?}");
    assert_eq!(report.status, "done");
    assert_eq!(report.stats.lines, 7, "frames A and C are read: {report:?}");
    assert_eq!(
        report.source_bytes,
        Some(u64::try_from(bytes.len()).unwrap())
    );
    assert_eq!(
        report.source_sha256.as_deref(),
        Some(compact::config::sha256_hex(&bytes).as_str())
    );
    let day = day_dir(&arch, T0);
    let tob = read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap();
    let last_a = h[0].last().unwrap().recv_ts_us / 1000;
    assert!(tob.iter().any(|t| t.recv_ms == last_a), "A: {tob:?}");
    let c: Vec<i64> = tob
        .iter()
        .filter(|t| t.recv_ms > last_a)
        .map(|t| t.recv_ms)
        .collect();
    assert_eq!(c, vec![T0 + 601_000, T0 + 601_100], "C's books: {tob:?}");
    assert!(
        tob.iter()
            .all(|t| t.bid != Some(m("0.30")) && t.recv_ms != T0 + 501_000),
        "nothing from the broken frame: {tob:?}"
    );
    let gaps = read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap();
    assert!(
        gaps.contains(&GapRow {
            start_ms: last_a + 1,
            end_ms: T0 + 600_000,
            shard: None,
            reason: reason::RAW_HOUR_LOST.into()
        }),
        "the unreadable stretch, up to C's first line: {gaps:?}"
    );
    assert!(
        gaps.iter().all(|g| g.end_ms <= T0 + 601_100),
        "books wait for C's snapshots, then nothing is open: {gaps:?}"
    );
    assert!(
        gaps.iter()
            .any(|g| g.start_ms == T0 + 600_000 && g.reason == reason::NO_BOOK_YET),
        "books are untrusted across the break: {gaps:?}"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn reorder_sorts_within_the_window_and_counts_late_rows() {
    let mut r: Reorder<char> = Reorder::default();
    let mut out = Vec::new();
    for (recv, v) in [
        (100, 'a'),
        (90, 'b'),
        (95, 'c'),
        (20_000, 'd'),
        (50, 'e'),
        (20_001, 'f'),
    ] {
        r.push(recv, v, &mut |x| {
            out.push(x);
            Ok(())
        })
        .unwrap();
    }
    r.drain(&mut |x| {
        out.push(x);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        out,
        vec!['b', 'c', 'a', 'e', 'd', 'f'],
        "b, c, a sorted; e arrived after 100 was written: kept, late"
    );
    assert_eq!(r.late(), 1);
}

#[test]
fn reorder_keeps_push_order_for_equal_recv_ms() {
    // l2 readers apply rows in file order: zeros then a snapshot set, or a snapshot set then a
    // diff, can all share one recv_ms, so equal recv_ms must leave in the order they came.
    let mut r: Reorder<char> = Reorder::default();
    let mut out = Vec::new();
    let push = |r: &mut Reorder<char>, recv: i64, v: char, out: &mut Vec<char>| {
        r.push(recv, v, &mut |x| {
            out.push(x);
            Ok(())
        })
        .unwrap();
    };
    for (recv, v) in [(100, 'a'), (100, 'b'), (50, 'c'), (100, 'd'), (100, 'e')] {
        push(&mut r, recv, v, &mut out);
    }
    push(&mut r, 20_200, 'x', &mut out); // flushes everything below 10_200
    assert_eq!(out, vec!['c', 'a', 'b', 'd', 'e']);
    // one more at the recv_ms just written is not late: it follows the others with that recv_ms
    push(&mut r, 100, 'f', &mut out);
    for v in ['g', 'h'] {
        push(&mut r, 20_200, v, &mut out);
    }
    r.drain(&mut |x| {
        out.push(x);
        Ok(())
    })
    .unwrap();
    assert_eq!(out, vec!['c', 'a', 'b', 'd', 'e', 'f', 'x', 'g', 'h']);
    assert_eq!(r.late(), 0);
}

fn job_at(
    raw: &std::path::Path,
    arch: &std::path::Path,
    source: Source,
    windows: Vec<compact::windows::Window>,
) -> HourJob {
    HourJob {
        hour_ms: T0,
        raw_dir: raw.into(),
        archive_dir: arch.into(),
        source,
        windows,
        stop_before_publish: false,
    }
}

fn file_job(
    raw: &std::path::Path,
    arch: &std::path::Path,
    windows: Vec<compact::windows::Window>,
) -> HourJob {
    job_at(
        raw,
        arch,
        Source::File(tape::files::hour_path(raw, T0)),
        windows,
    )
}

/// The reducer's rows for one hour file, in emission order.
fn reducer_rows(
    lines: &[tape::line::Line],
    r: &tape::universe_view::RecordView,
    windows: Vec<compact::windows::Window>,
) -> VecSink {
    let mut want = VecSink::default();
    let mut red = Reducer::new(rcfg());
    red.begin_hour(T0, windows);
    run(&mut red, lines, std::slice::from_ref(r), &mut want);
    red.end_hour(&mut want).unwrap();
    want
}

/// Per token, ts_ms never goes back in file order.
fn ts_non_decreasing(rows: impl Iterator<Item = (u32, i64)>) -> bool {
    let mut last = std::collections::HashMap::new();
    rows.into_iter()
        .all(|(t, ts)| last.insert(t, ts).is_none_or(|p| p <= ts))
}

#[test]
fn a_cross_shard_window_restart_keeps_one_tokens_l2_in_reducer_order() {
    let (raw, arch) = (tmp("l2ord-raw"), tmp("l2ord-arch"));
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["z1"])],
        &[("e1", 0), ("e2", 1)],
    );
    write_record(&raw, &r);
    let lines = vec![
        uni(T0 + 1_000, &r),
        frame(
            T0 + 2_000,
            0,
            book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
        ),
        frame(
            T0 + 2_100,
            1,
            book("z1", T0 + 2_090, &[("0.20", "3")], &[("0.25", "1")]),
        ),
        // a shard 1 line crosses the window start: y1 restarts with snapshot rows stamped 100_005
        frame(
            T0 + 100_005,
            1,
            change("z1", T0 + 100_000, "BUY", "0.21", "1", "0.21", "0.25"),
        ),
        // a shard 0 line received earlier, processed after: y1's diff stamped 100_001
        frame(
            T0 + 100_001,
            0,
            change("y1", T0 + 100_000, "BUY", "0.41", "1", "0.41", "0.45"),
        ),
    ];
    write_hour(&raw, T0, &lines);
    let w = vec![compact::windows::Window {
        episode_id: "ep".into(),
        event_id: "e1".into(),
        opened_ms: T0 + 400_000,
        start_ms: T0 + 100_000,
        end_ms: T0 + 900_000,
        truncated: false,
    }];
    let rep = process_hour(&mut Reducer::new(rcfg()), &file_job(&raw, &arch, w.clone()))
        .unwrap()
        .unwrap();
    let want = reducer_rows(&lines, &r, w);
    let got = read_all::<L2Row>(&part_path(&day_dir(&arch, T0), 0, "l2")).unwrap();
    assert_eq!(got, want.l2);
    let shape: Vec<(bool, u8, i64, i64, i64)> = got
        .iter()
        .map(|x| (x.snapshot, x.side, x.price, x.new_size, x.recv_ms))
        .collect();
    assert_eq!(
        shape,
        vec![
            (true, SIDE_BID, m("0.40"), m("10"), T0 + 100_005),
            (true, SIDE_ASK, m("0.45"), m("5"), T0 + 100_005),
            (false, SIDE_BID, m("0.41"), m("1"), T0 + 100_001),
        ],
        "the snapshot set comes first; the recv_ms column is kept as stamped"
    );
    assert!(ts_non_decreasing(
        got.iter().map(|x| (x.token_idx, x.ts_ms))
    ));
    let tob = read_all::<TobRow>(&part_path(&day_dir(&arch, T0), 0, "tob")).unwrap();
    // across tokens the file is in recv_ms order; per token it is the reducer's order
    let by_token = |v: &[TobRow]| {
        let mut v = v.to_vec();
        v.sort_by_key(|x| x.token_idx); // stable
        v
    };
    assert_eq!(by_token(&tob), by_token(&want.tob));
    assert!(ts_non_decreasing(
        tob.iter().map(|x| (x.token_idx, x.ts_ms))
    ));
    assert_eq!(rep.late_rows, 0);
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_token_on_two_shards_keeps_its_tob_order() {
    let (raw, arch) = (tmp("tob2-raw"), tmp("tob2-arch"));
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["y1"])],
        &[("e1", 0), ("e2", 1)],
    );
    write_record(&raw, &r);
    let lines = vec![
        uni(T0 + 1_000, &r),
        frame(
            T0 + 2_000,
            1,
            book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
        ),
        frame(
            T0 + 3_005,
            1,
            change("y1", T0 + 3_000, "BUY", "0.41", "1", "0.41", "0.45"),
        ),
        frame(
            T0 + 3_001,
            0,
            change("y1", T0 + 3_002, "BUY", "0.42", "1", "0.42", "0.45"),
        ),
    ];
    write_hour(&raw, T0, &lines);
    process_hour(&mut Reducer::new(rcfg()), &file_job(&raw, &arch, vec![]))
        .unwrap()
        .unwrap();
    let want = reducer_rows(&lines, &r, vec![]);
    let got = read_all::<TobRow>(&part_path(&day_dir(&arch, T0), 0, "tob")).unwrap();
    assert_eq!(got, want.tob);
    let shape: Vec<(i64, i64, Option<i64>)> =
        got.iter().map(|x| (x.ts_ms, x.recv_ms, x.bid)).collect();
    assert_eq!(
        shape,
        vec![
            (T0 + 1_990, T0 + 2_000, Some(m("0.40"))),
            (T0 + 3_000, T0 + 3_005, Some(m("0.41"))),
            (T0 + 3_002, T0 + 3_001, Some(m("0.42"))),
        ],
        "the last row is the true final top"
    );
    assert!(ts_non_decreasing(
        got.iter().map(|x| (x.token_idx, x.ts_ms))
    ));
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_late_row_never_jumps_ahead_of_its_own_tokens_buffered_rows() {
    let (raw, arch) = (tmp("late-raw"), tmp("late-arch"));
    let r = rec(
        "u1",
        T0,
        &[("e1", &["y1"]), ("e2", &["y1"]), ("e3", &["z1"])],
        &[("e1", 0), ("e2", 1), ("e3", 2)],
    );
    write_record(&raw, &r);
    let lines = vec![
        uni(T0 + 1_000, &r),
        frame(
            T0 + 2_000,
            1,
            book("y1", T0 + 1_990, &[("0.40", "10")], &[("0.45", "5")]),
        ),
        frame(
            T0 + 2_100,
            2,
            book("z1", T0 + 2_090, &[("0.20", "3")], &[("0.25", "1")]),
        ),
        frame(
            T0 + 20_000,
            2,
            change("z1", T0 + 19_995, "BUY", "0.21", "1", "0.21", "0.25"),
        ),
        // y1 is buffered at 25_000 ...
        frame(
            T0 + 25_000,
            1,
            change("y1", T0 + 24_995, "BUY", "0.41", "1", "0.41", "0.45"),
        ),
        // ... while z1's 20_000 row is written
        frame(
            T0 + 32_000,
            2,
            change("z1", T0 + 31_995, "BUY", "0.22", "1", "0.22", "0.25"),
        ),
        // y1 from its other shard, received before what was written: late by recv_ms alone
        frame(
            T0 + 19_000,
            0,
            change("y1", T0 + 25_000, "BUY", "0.42", "1", "0.42", "0.45"),
        ),
    ];
    write_hour(&raw, T0, &lines);
    process_hour(&mut Reducer::new(rcfg()), &file_job(&raw, &arch, vec![]))
        .unwrap()
        .unwrap();
    let want = reducer_rows(&lines, &r, vec![]);
    let got = read_all::<TobRow>(&part_path(&day_dir(&arch, T0), 0, "tob")).unwrap();
    let y1_idx = want.tob[0].token_idx;
    let y1 = |v: &[TobRow]| -> Vec<Option<i64>> {
        v.iter()
            .filter(|x| x.token_idx == y1_idx)
            .map(|x| x.bid)
            .collect()
    };
    assert_eq!(
        y1(&got),
        vec![Some(m("0.40")), Some(m("0.41")), Some(m("0.42"))]
    );
    assert_eq!(
        y1(&got),
        y1(&want.tob),
        "one token's rows keep the reducer's order"
    );
    assert!(ts_non_decreasing(
        got.iter().map(|x| (x.token_idx, x.ts_ms))
    ));
    let mut sorted = got.clone();
    sorted.sort_by_key(|x| x.token_idx);
    let mut want_sorted = want.tob.clone();
    want_sorted.sort_by_key(|x| x.token_idx);
    assert_eq!(sorted, want_sorted, "every row is kept");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn an_invalid_utf8_line_is_a_bad_line_not_a_break() {
    let (raw, arch) = (tmp("utf8-raw"), tmp("utf8-arch"));
    let (r, h) = three_hours(&raw);
    let mut plain = Vec::new();
    for (i, l) in h[0].iter().enumerate() {
        if i == 2 {
            plain.extend_from_slice(&[0xff, 0xfe, b' ', b'x', b'\n']);
        }
        plain.extend(serde_json::to_vec(l).unwrap());
        plain.push(b'\n');
    }
    std::fs::write(
        tape::files::hour_path(&raw, T0),
        zstd::encode_all(plain.as_slice(), 3).unwrap(),
    )
    .unwrap();
    let rep = process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    assert_eq!(rep.read_error, None);
    assert_eq!(
        (rep.stats.lines, rep.stats.bad_lines),
        (4, 1),
        "the rest of the hour is still read"
    );
    let want = reducer_rows(&h[0], &r, vec![]);
    let day = day_dir(&arch, T0);
    assert_eq!(
        read_all::<TobRow>(&part_path(&day, 0, "tob")).unwrap(),
        want.tob
    );
    assert_eq!(
        read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap(),
        want.gaps
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn a_missing_hour_is_lost_or_stopped_with_its_own_gap() {
    for (source, status, why) in [
        (Source::Lost, "lost", reason::RAW_HOUR_LOST),
        (Source::Stopped, "done", reason::SCANNER_STOPPED),
    ] {
        let (raw, arch) = (tmp("miss-raw"), tmp("miss-arch"));
        let rep = process_hour(
            &mut Reducer::new(rcfg()),
            &job_at(&raw, &arch, source.clone(), vec![]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(rep.status, status, "{source:?}");
        assert_eq!(
            (
                rep.source_file,
                rep.source_bytes,
                rep.source_sha256,
                rep.read_error
            ),
            (None, None, None, None)
        );
        let day = day_dir(&arch, T0);
        assert_eq!(
            read_all::<GapRow>(&part_path(&day, 0, "gaps")).unwrap(),
            vec![GapRow {
                start_ms: T0,
                end_ms: T0 + H,
                shard: None,
                reason: why.into()
            }],
            "{source:?}"
        );
        assert!(
            read_all::<TobRow>(&part_path(&day, 0, "tob"))
                .unwrap()
                .is_empty()
        );
        assert!(sidecar_path(&day, 0).exists());
        std::fs::remove_dir_all(&raw).ok();
        std::fs::remove_dir_all(&arch).ok();
    }
}

#[test]
fn a_rerun_that_fails_to_publish_leaves_no_old_sidecar() {
    let (raw, arch) = (tmp("side-raw"), tmp("side-arch"));
    three_hours(&raw);
    process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    let day = day_dir(&arch, T0);
    assert!(sidecar_path(&day, 0).exists());
    // a part's final name is now a non-empty folder: its publish (rename) fails
    let gaps = part_path(&day, 0, "gaps");
    std::fs::remove_file(&gaps).unwrap();
    std::fs::create_dir_all(gaps.join("x")).unwrap();
    assert!(process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0)).is_err());
    assert!(
        !sidecar_path(&day, 0).exists(),
        "the old sidecar must not describe the new parts"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn bytes_past_a_break_are_hashed_too() {
    let (raw, arch) = (tmp("tail-raw"), tmp("tail-arch"));
    three_hours(&raw);
    let p = tape::files::hour_path(&raw, T0);
    let mut bytes = std::fs::read(&p).unwrap();
    // not a zstd frame, and longer than the decoder's read-ahead
    bytes.extend((0..400_000u32).map(|i| u8::try_from(i % 251).unwrap() | 1));
    std::fs::write(&p, &bytes).unwrap();
    let rep = process_hour(&mut Reducer::new(rcfg()), &hour_job(&raw, &arch, 0))
        .unwrap()
        .unwrap();
    assert!(rep.read_error.is_some(), "{rep:?}");
    assert_eq!(rep.stats.lines, 4, "the frame before the break is read");
    assert_eq!(rep.source_bytes, Some(u64::try_from(bytes.len()).unwrap()));
    assert_eq!(
        rep.source_sha256.as_deref(),
        Some(compact::config::sha256_hex(&bytes).as_str())
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
