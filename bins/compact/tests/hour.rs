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
