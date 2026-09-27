#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::{PREMISES, code_version, effective_version};
use scanner::config::Config;
use scanner::paper::parse_policies;
use scanner::premises::{HOST_BAND_MINUTES, HostBand, host_label, run_rows, section};
use std::path::PathBuf;
use store::HealthRow;

fn repo(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(p)
}

fn repo_config() -> Config {
    Config::load(&repo("config/scanner.toml")).unwrap().0
}

fn minute(lag: Option<i32>, rtt: Option<i32>) -> HealthRow {
    minute_clock(lag, rtt, None)
}

fn minute_clock(lag: Option<i32>, rtt: Option<i32>, clock_offset_ms: Option<i32>) -> HealthRow {
    HealthRow {
        run_id: 7,
        minute_ms: 0,
        tokens: 0,
        shards: 0,
        msgs: 0,
        bytes: 0,
        drops: 0,
        reconnects: 0,
        rest_p50_ms: None,
        rest_p99_ms: None,
        lag_p50_ms: lag,
        lag_p99_ms: None,
        clock_offset_ms,
        ok: None,
        capture_paused: false,
        parked_shards: 0,
        dropped_groups: 0,
        rest_rtt_p50_ms: rtt,
        rest_rtt_p99_ms: None,
    }
}

#[test]
fn run_rows_cover_every_area_but_host_with_config_hashed_versions() {
    let cfg = repo_config();
    let text = std::fs::read_to_string(repo("config/policies.toml")).unwrap();
    let pol = parse_policies(&text).unwrap();
    let rows = run_rows(9, &cfg, Some(&pol));
    let areas: Vec<&str> = rows.iter().map(|r| r.area.as_str()).collect();
    assert_eq!(
        areas,
        [
            "censoring",
            "depth",
            "detector_no",
            "detector_yes",
            "episode_def",
            "fees",
            "sim"
        ]
    );
    assert!(rows.iter().all(|r| r.run_id == 9));
    let censoring = rows.iter().find(|r| r.area == "censoring").unwrap();
    assert_eq!(censoring.version, effective_version("1", "gap_ms=30000"));
    let sim = rows.iter().find(|r| r.area == "sim").unwrap();
    assert_eq!(
        sim.version,
        effective_version("1", &format!("policies_sha256={}", pol.sha256))
    );
    // paper trading off: no sim row, so Edge capture never counts the run
    assert!(run_rows(9, &cfg, None).iter().all(|r| r.area != "sim"));
}

#[test]
fn the_backfill_literals_in_0006_are_todays_premises() {
    let sql = std::fs::read_to_string(repo("crates/store/migrations/0006_cards.sql")).unwrap();
    let block = sql
        .split("-- backfill:begin")
        .nth(1)
        .and_then(|s| s.split("-- backfill:end").next())
        .expect("backfill markers");
    let tuples: Vec<(String, String, String)> = block
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("('"))
        .map(|l| {
            let inner = l
                .trim_end_matches(',')
                .trim_start_matches("('")
                .trim_end_matches("')");
            let parts: Vec<&str> = inner.split("', '").collect();
            assert_eq!(parts.len(), 3, "{l}");
            (
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
            )
        })
        .collect();
    let cfg = repo_config();
    let expected: Vec<(String, String, String)> = PREMISES
        .iter()
        .filter(|p| p.0 != "sim" && p.0 != "host")
        .map(|p| {
            (
                p.0.to_string(),
                code_version(p.0).unwrap().to_string(),
                section(p.0, &cfg, None).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        tuples, expected,
        "0006's backfill must name exactly today's premises (a version bump after 0006 needs no backfill change: \
         backfilled runs simply stop matching the new key)"
    );
}

#[test]
fn host_labels_default_to_unknown_and_reject_bad_input() {
    assert_eq!(host_label(None).unwrap(), "unknown");
    assert_eq!(host_label(Some(String::new())).unwrap(), "unknown");
    assert_eq!(host_label(Some("vps-kvm2".into())).unwrap(), "vps-kvm2");
    assert!(host_label(Some("VPS KVM2".into())).is_err());
    assert!(host_label(Some("a|b".into())).is_err());
}

#[test]
fn the_host_row_comes_once_after_ten_minutes_with_the_median_band() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    // minutes 1..=9: no row yet; lag + rtt/2 = 80+65=145, one minute unmeasured
    for i in 1..HOST_BAND_MINUTES {
        let row = if i == 3 {
            minute(Some(80), None)
        } else {
            minute(Some(80), Some(130))
        };
        assert_eq!(band.observe(&row), None, "minute {i}");
    }
    let host = band.observe(&minute(Some(10), Some(10))).unwrap();
    assert_eq!((host.run_id, host.area.as_str()), (7, "host"));
    assert_eq!(host.version, "1:vps-kvm2|100-250");
    assert_eq!(
        band.observe(&minute(Some(1), Some(1))),
        None,
        "written once"
    );
    let mut quiet = HostBand::new(8, "pc-dev".into());
    let mut last = None;
    for _ in 0..HOST_BAND_MINUTES {
        last = quiet.observe(&minute(None, None));
    }
    assert_eq!(last.unwrap().version, "1:pc-dev|unmeasured");
}

#[test]
fn skewed_minutes_are_skipped_but_still_count_toward_the_window() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    let mut last = None;
    for i in 1..=HOST_BAND_MINUTES {
        // Half the minutes are skewed and report a tiny (0, 0) => 0 latency, which the feed-lag
        // clock had frozen or run backward during a host freeze; if wrongly included it would
        // pull the median down to "lt25" (index (10-1)/2 = 4, the 5th-smallest of 10 sorted
        // values). The other half are the same normal (80, 130) => 145 as the baseline test.
        // Skipping the skewed half leaves 5 real samples, whose own median (index (5-1)/2 = 2)
        // is 145 → "100-250".
        last = if i % 2 == 0 {
            band.observe(&minute_clock(Some(0), Some(0), Some(300)))
        } else {
            band.observe(&minute(Some(80), Some(130)))
        };
    }
    assert_eq!(last.unwrap().version, "1:vps-kvm2|100-250");
}

#[test]
fn mostly_skewed_minutes_leave_the_band_unmeasured() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    let mut last = None;
    for i in 1..=HOST_BAND_MINUTES {
        // Only 3 of 10 minutes are unskewed: fewer than half, so the band is unmeasured even
        // though those 3 have perfectly good, consistent samples.
        last = if i <= 3 {
            band.observe(&minute(Some(80), Some(130)))
        } else {
            band.observe(&minute_clock(Some(80), Some(130), Some(-260)))
        };
    }
    assert_eq!(last.unwrap().version, "1:vps-kvm2|unmeasured");
}

#[test]
fn an_incident_shaped_minute_is_excluded_like_the_2026_09_27_freeze() {
    // The actual incident this brief fixes: a host freeze left the VM clock ~2 s behind, which
    // made feed lag read around -2000 ms. If minutes shaped like that were counted, the median
    // would collapse into "lt25" (5 of them, each contributing lag + rtt/2 = -2000+65 = -1935,
    // land on the 5th-smallest of 10 sorted values, index (10-1)/2 = 4). Because their
    // clock_offset_ms is skewed, observe() must exclude them, leaving only the 5 normal (80, 130)
    // => 145 minutes and the untouched "100-250" band.
    let mut band = HostBand::new(7, "vps-kvm2".into());
    let mut last = None;
    for i in 1..=HOST_BAND_MINUTES {
        last = if i % 2 == 0 {
            band.observe(&minute_clock(Some(-2_000), Some(130), Some(2_000)))
        } else {
            band.observe(&minute(Some(80), Some(130)))
        };
    }
    assert_eq!(last.unwrap().version, "1:vps-kvm2|100-250");
}

#[test]
fn clock_offset_of_exactly_250_still_counts_but_251_does_not() {
    let mut at_threshold = HostBand::new(7, "vps-kvm2".into());
    let mut last = None;
    for _ in 0..HOST_BAND_MINUTES {
        last = at_threshold.observe(&minute_clock(Some(80), Some(130), Some(250)));
    }
    assert_eq!(
        last.unwrap().version,
        "1:vps-kvm2|100-250",
        "250 is not > CLOCK_SKEW_MS: still counted"
    );

    let mut over_threshold = HostBand::new(8, "vps-kvm2".into());
    let mut last2 = None;
    for _ in 0..HOST_BAND_MINUTES {
        last2 = over_threshold.observe(&minute_clock(Some(80), Some(130), Some(251)));
    }
    assert_eq!(
        last2.unwrap().version,
        "1:vps-kvm2|unmeasured",
        "251 is > CLOCK_SKEW_MS: excluded, and 0 of 10 is fewer than half"
    );
}

#[test]
fn a_none_clock_offset_behaves_as_today() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    let mut last = None;
    for _ in 0..HOST_BAND_MINUTES {
        last = band.observe(&minute_clock(Some(80), Some(130), None));
    }
    assert_eq!(last.unwrap().version, "1:vps-kvm2|100-250");
}

#[test]
fn the_host_row_stays_pending_until_an_insert_succeeds() {
    let mut band = HostBand::new(7, "vps-kvm2".into());
    for _ in 1..HOST_BAND_MINUTES {
        band.observe(&minute(Some(80), Some(130)));
        assert_eq!(
            band.pending(),
            None,
            "nothing to insert before the band is known"
        );
    }
    let host = band.observe(&minute(Some(80), Some(130))).unwrap();
    assert_eq!(band.pending(), Some(&host));
    // the insert failed: later minutes keep offering the same computed row, never a new one
    for _ in 0..3 {
        assert_eq!(band.observe(&minute(Some(1), Some(1))), None);
        assert_eq!(band.pending(), Some(&host), "retried each minute");
    }
    band.recorded();
    assert_eq!(band.pending(), None, "one success ends the retries");
    band.observe(&minute(Some(80), Some(130)));
    assert_eq!(band.pending(), None);
}
