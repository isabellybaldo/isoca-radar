#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use tape::files::*;
use tape::line::{Ctl, Line};
use tape::time::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tape-test-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const T0: i64 = 1_790_121_600_000; // 2026-09-23 00:00Z

#[test]
fn civil_days_and_labels_match_known_values() {
    assert_eq!(civil(0), (1970, 1, 1, 0));
    assert_eq!(civil(T0), (2026, 9, 23, 0));
    assert_eq!(civil(1_790_200_799_999), (2026, 9, 23, 21));
    assert_eq!(civil(951_782_400_000), (2000, 2, 29, 0));
    assert_eq!(day_name(T0 + 5 * HOUR_MS), "2026-09-23");
    assert_eq!(hour_label(T0 + 13 * HOUR_MS + 59_999), "2026-09-23T13");
    assert_eq!(parse_day("2026-09-23"), Some(T0));
    assert_eq!(parse_day("2026-02-30"), None, "not a real date");
    assert_eq!(parse_day("_state"), None);
    assert_eq!(hour_of(T0 + 3_599_999), T0);
    assert_eq!(day_of(T0 + DAY_MS - 1), T0);
    assert_eq!(days_from_civil(2026, 9, 23) * DAY_MS, T0);
}

#[test]
fn hour_and_universe_paths_round_trip() {
    let p = hour_path(Path::new("/data/capture"), 1_790_200_799_999);
    assert!(p.ends_with("2026-09-23/21.jsonl.zst"), "{}", p.display());
    assert_eq!(
        hour_start_ms("2026-09-23", "21.jsonl.zst"),
        Some(T0 + 21 * HOUR_MS)
    );
    assert_eq!(hour_start_ms("2026-09-23", "universe-1-2.json.zst"), None);
    let name = universe_file_name(T0, 7);
    assert_eq!(name, format!("universe-{T0}-7.json.zst"));
    assert_eq!(universe_taken_ms(&name), Some(T0));
    assert!(universe_path(Path::new("/c"), T0, &name).ends_with(format!("2026-09-23/{name}")));
}

#[test]
fn listing_sorts_hours_and_universes_and_lines_round_trip() {
    let dir = tmp("listing");
    let lines = vec![
        Line {
            recv_ts_us: (T0 + 1_000) * 1000,
            shard: 2,
            raw: Some("PONG".into()),
            ctl: None,
        },
        Line {
            recv_ts_us: (T0 + 2_000) * 1000,
            shard: 0,
            raw: None,
            ctl: Some(Ctl::Down),
        },
    ];
    write_lines(&hour_path(&dir, T0 + HOUR_MS), &lines).unwrap();
    write_lines(&hour_path(&dir, T0), &lines[..1]).unwrap();
    write_zst_json(
        &universe_path(&dir, T0, &universe_file_name(T0, 1)),
        &serde_json::json!({"a": 1}),
    )
    .unwrap();
    let l = listing(&dir);
    assert_eq!(
        l.hours.iter().map(|h| h.start_ms).collect::<Vec<_>>(),
        vec![T0, T0 + HOUR_MS]
    );
    assert!(l.hours.iter().all(|h| h.bytes > 0));
    assert_eq!(l.universes.len(), 1);
    assert_eq!(l.universes[0].taken_ms, T0);
    assert!(l.universes[0].mtime_ms.is_some());
    let mut back = Vec::new();
    for_each_line(&l.hours[1].path, |x| {
        back.push(x);
        Ok(())
    })
    .unwrap();
    assert_eq!(back, lines);
    let v: serde_json::Value = read_zst_json(&l.universes[0].path).unwrap();
    assert_eq!(v["a"], 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_bad_capture_line_is_an_error_naming_the_file() {
    let dir = tmp("bad-line");
    let p = hour_path(&dir, T0);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, zstd::encode_all(&b"{not json}\n"[..], 3).unwrap()).unwrap();
    let e = for_each_line(&p, |_| Ok(())).unwrap_err();
    assert!(format!("{e:#}").contains("bad capture line in"), "{e:#}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_paper_reset_line_round_trips_with_its_payload() {
    let l = Line {
        recv_ts_us: 7_000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::PaperReset { t: 6 }),
    };
    let json = serde_json::to_string(&l).unwrap();
    assert_eq!(
        json,
        r#"{"recv_ts_us":7000,"shard":0,"ctl":{"kind":"paper_reset","t":6}}"#
    );
    assert_eq!(serde_json::from_str::<Line>(&json).unwrap(), l);
}
