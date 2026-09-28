#![allow(clippy::unwrap_used, clippy::expect_used)]
mod fixture;
use compact::archive::*;
use compact::pq::{PartWriter, Table, read_all};
use compact::reduce::Reducer;
use compact::rows::*;
use compact::verify::verify_day;
use fixture::*;

/// A checkpoint past every test day: merging is allowed.
const ALL: Option<i64> = Some(i64::MAX);
use std::path::{Path, PathBuf};

fn good_day(name: &str) -> (PathBuf, PathBuf) {
    let (raw, arch) = (tmp(&format!("{name}-raw")), tmp(&format!("{name}-arch")));
    build_day(&raw, &arch, T0, &[3]);
    let merged = merge_ready(&arch, &cfg(&raw, &arch), "s", ALL);
    assert!(
        merged.failed.is_empty(),
        "merge failed: {:?}",
        merged.failed
    );
    (raw, arch)
}

/// Rewrite one daily table (tob, l2 or gaps) through `f`, keeping the manifest's totals in step
/// (all of this fixture's tob and l2 rows are in hour 0), so only `f`'s change is wrong.
fn rewrite<R: Table + Clone>(arch: &Path, f: impl FnOnce(&mut Vec<R>)) {
    let day = day_dir(arch, T0);
    let p = daily_path(&day, R::NAME);
    let mut rows: Vec<R> = read_all(&p).unwrap();
    f(&mut rows);
    let mut w = PartWriter::<R>::create(p).unwrap();
    for r in &rows {
        w.push(r.clone()).unwrap();
    }
    w.close().unwrap().publish().unwrap();
    let mp = day.join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    let n = u64::try_from(rows.len()).unwrap();
    let (total, hour0) = match R::NAME {
        "tob" => (&mut m.totals.tob, &mut m.hours[0].rows.tob),
        "l2" => (&mut m.totals.l2, &mut m.hours[0].rows.l2),
        _ => (&mut m.totals.gaps, &mut m.hours[0].rows.gaps),
    };
    *hour0 = *hour0 + n - *total; // every rewrite here adds rows or keeps the count
    *total = n;
    std::fs::write(&mp, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
}

fn fails(arch: &Path, needle: &str) {
    let p = verify_day(arch, "2026-09-23").unwrap();
    assert!(
        p.iter().any(|x| x.contains(needle)),
        "expected a problem containing {needle:?}, got {p:?}"
    );
}

#[test]
fn verify_passes_on_a_good_day() {
    let (raw, arch) = good_day("vgood");
    assert_eq!(
        verify_day(&arch, "2026-09-23").unwrap(),
        Vec::<String>::new()
    );
    assert!(
        verify_day(&arch, "2026-9-23").is_err(),
        "a bad day name is an error, not a pass"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_fails_on_each_kind_of_corruption() {
    // a manifest that disagrees with a file
    let (raw, arch) = good_day("vcount");
    let mp = day_dir(&arch, T0).join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    m.totals.depth += 1;
    std::fs::write(&mp, serde_json::to_vec(&m).unwrap()).unwrap();
    fails(&arch, "depth:");
    // a tob row that is not a change
    let (raw2, arch2) = good_day("vdup");
    rewrite::<TobRow>(&arch2, |rows| {
        let r = rows[0];
        rows.insert(
            1,
            TobRow {
                recv_ms: r.recv_ms,
                ..r
            },
        );
    });
    fails(&arch2, "repeats its top");
    // a ts_ms that goes back
    let (raw3, arch3) = good_day("vts");
    rewrite::<TobRow>(&arch3, |rows| {
        let i = (1..rows.len())
            .find(|&i| rows[i].token_idx == rows[0].token_idx)
            .unwrap();
        rows[i].ts_ms = rows[0].ts_ms - 1;
    });
    fails(&arch3, "ts_ms goes back");
    // an l2 window that does not start with a snapshot
    let (raw4, arch4) = good_day("vl2");
    rewrite::<L2Row>(&arch4, |rows| rows[0].snapshot = false);
    fails(&arch4, "does not start with a snapshot");
    // a row inside a whole-feed gap
    let (raw5, arch5) = good_day("vgap");
    let t = read_all::<TobRow>(&daily_path(&day_dir(&arch5, T0), "tob")).unwrap()[0].recv_ms;
    rewrite::<GapRow>(&arch5, |rows| {
        rows.push(GapRow {
            start_ms: t,
            end_ms: t + 1,
            shard: None,
            reason: reason::SCANNER_STOPPED.into(),
        })
    });
    fails(&arch5, "inside a gap");
    for d in [
        raw, arch, raw2, arch2, raw3, arch3, raw4, arch4, raw5, arch5,
    ] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn verify_allows_a_leading_zero_before_the_l2_snapshot() {
    // A stream that restarts onto an empty book zeroes the levels shown earlier (snapshot =
    // false, new_size = 0) before its next real snapshot (reduce.rs: `l2_restart`). Such a
    // leading size-0 row must not be reported as "does not start with a snapshot".
    let (raw, arch) = good_day("vl2lead");
    rewrite::<L2Row>(&arch, |rows| {
        let r = rows[0].clone();
        rows.insert(
            0,
            L2Row {
                snapshot: false,
                new_size: 0,
                ..r
            },
        );
    });
    assert_eq!(
        verify_day(&arch, "2026-09-23").unwrap(),
        Vec::<String>::new()
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_fails_on_an_l2_ts_that_goes_back() {
    let (raw, arch) = good_day("vl2ts");
    rewrite::<L2Row>(&arch, |rows| {
        let i = (1..rows.len())
            .find(|&i| rows[i].token_idx == rows[0].token_idx)
            .unwrap();
        rows[i].ts_ms = rows[0].ts_ms - 1;
    });
    let rows = read_all::<L2Row>(&daily_path(&day_dir(&arch, T0), "l2")).unwrap();
    fails(
        &arch,
        &format!(
            "l2: token_idx {} ts_ms goes back at {}",
            rows[0].token_idx,
            rows[0].ts_ms - 1
        ),
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_fails_on_a_tob_token_idx_outside_the_dictionary() {
    let (raw, arch) = good_day("vidx");
    rewrite::<TobRow>(&arch, |rows| {
        rows[0].token_idx = 9_999;
    });
    fails(&arch, "is not in tokens");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_checks_every_hours_l2_stream_starts_with_a_snapshot() {
    // Every hour restarts its windows with snapshots (R5): an hour whose stream for a token
    // starts with a change (here the day's last l2 row, moved to hour 1 in the manifest) fails.
    let (raw, arch) = good_day("vl2hour");
    let l2 = read_all::<L2Row>(&daily_path(&day_dir(&arch, T0), "l2")).unwrap();
    let last = l2.last().unwrap();
    assert!(!last.snapshot && last.new_size > 0, "{l2:?}");
    assert_eq!(
        verify_day(&arch, "2026-09-23").unwrap(),
        Vec::<String>::new()
    );
    let mp = day_dir(&arch, T0).join(MANIFEST);
    let mut m: Manifest = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    m.hours[0].rows.l2 -= 1;
    m.hours[1].rows.l2 += 1;
    std::fs::write(&mp, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    fails(
        &arch,
        &format!(
            "l2: episode {} token_idx {} does not start with a snapshot",
            last.episode_id, last.token_idx
        ),
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_checks_the_l2_stream_restarts_with_a_snapshot_after_a_whole_feed_gap() {
    let (raw, arch) = good_day("vl2gap");
    let l2 = read_all::<L2Row>(&daily_path(&day_dir(&arch, T0), "l2")).unwrap();
    let last = l2.last().unwrap().clone();
    let prev = l2
        .iter()
        .rev()
        .skip(1)
        .find(|r| r.token_idx == last.token_idx)
        .unwrap()
        .recv_ms;
    assert!(last.recv_ms - prev > 2, "{l2:?}");
    // a whole-feed gap between the token's last two rows: the last one must be a snapshot
    rewrite::<GapRow>(&arch, |rows| {
        rows.push(GapRow {
            start_ms: prev + 1,
            end_ms: last.recv_ms - 1,
            shard: None,
            reason: reason::RAW_HOUR_LOST.into(),
        })
    });
    fails(&arch, "does not start with a snapshot");
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}

#[test]
fn verify_passes_on_two_days_and_the_second_stands_alone() {
    use compact::hour::{HourJob, Source, process_hour};
    let (raw, arch) = (tmp("v2day-raw"), tmp("v2day-arch"));
    let d = T0 + 24 * H;
    let h23 = d - H;
    let r = rec("u1", T0, &[("e1", &["y1", "y2"])], &[("e1", 0)]);
    write_record(&raw, &r);
    write_hour(
        &raw,
        h23,
        &[
            uni(h23 + 1_000, &r),
            frame(
                h23 + 2_000,
                0,
                book("y1", h23 + 1_990, &[("0.40", "1")], &[("0.45", "3")]),
            ),
            frame(
                h23 + 3_000,
                0,
                book("y2", h23 + 2_990, &[("0.20", "1")], &[]),
            ),
            frame(
                d - 30_000,
                0,
                change("y1", d - 30_005, "BUY", "0.41", "1", "0.41", "0.45"),
            ),
        ],
    );
    write_hour(
        &raw,
        d,
        &[frame(
            d + 1_000,
            0,
            change("y1", d + 995, "BUY", "0.42", "1", "0.42", "0.45"),
        )],
    );
    let mut red = Reducer::new(rcfg());
    for i in 0..48 {
        let hour_ms = T0 + i * H;
        let source = if hour_ms == h23 || hour_ms == d {
            Source::File(tape::files::hour_path(&raw, hour_ms))
        } else {
            Source::Stopped
        };
        let job = HourJob {
            hour_ms,
            raw_dir: raw.clone(),
            archive_dir: arch.clone(),
            source,
            windows: vec![],
            stop_before_publish: false,
        };
        process_hour(&mut red, &job).unwrap().unwrap();
    }
    let merged = merge_ready(&arch, &cfg(&raw, &arch), "s", ALL);
    assert!(merged.failed.is_empty(), "{:?}", merged.failed);
    for day in ["2026-09-23", "2026-09-24"] {
        assert_eq!(
            verify_day(&arch, day).unwrap(),
            Vec::<String>::new(),
            "{day}"
        );
    }
    let day2 = day_dir(&arch, d);
    let toks: Vec<TokenRow> = read_all(&daily_path(&day2, TokenRow::NAME)).unwrap();
    let y2 = toks
        .iter()
        .find(|t| t.token_id == "y2")
        .expect("y2 opens day 2");
    let tob: Vec<TobRow> = read_all(&daily_path(&day2, TobRow::NAME)).unwrap();
    assert!(
        tob.iter()
            .any(|t| t.token_idx == y2.token_idx && t.ts_ms == d && t.bid == Some(m("0.20"))),
        "{tob:?}"
    );
    let depth: Vec<DepthRow> = read_all(&daily_path(&day2, DepthRow::NAME)).unwrap();
    assert!(
        depth
            .iter()
            .any(|x| x.token_idx == y2.token_idx && x.ts_ms == d),
        "{depth:?}"
    );
    std::fs::remove_dir_all(&raw).ok();
    std::fs::remove_dir_all(&arch).ok();
}
