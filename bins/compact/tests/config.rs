#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::config::Config;

const REPO: &str = include_str!("../../../config/compact.toml");

#[test]
fn the_repo_config_has_the_spec_defaults() {
    let (c, sha) = Config::parse(REPO).unwrap();
    assert_eq!(
        (c.raw_dir.as_str(), c.archive_dir.as_str()),
        ("/data/capture", "/archive")
    );
    assert_eq!((c.depth_every_secs, c.depth_levels), (300, 10));
    assert_eq!(
        (c.l2_before_secs, c.l2_after_secs, c.l2_max_window_secs),
        (300, 300, 1800)
    );
    assert_eq!((c.archive_budget_gb, c.lag_alarm_hours), (40, 3));
    assert_eq!(c.budget_bytes(), 40_000_000_000);
    assert_eq!(sha.len(), 64);
    assert!(
        !REPO.contains('\r'),
        "config/compact.toml must be LF (.gitattributes)"
    );
}

#[test]
fn bad_values_are_refused_at_startup() {
    for (from, to) in [
        ("depth_every_secs = 300", "depth_every_secs = 7"),
        ("depth_every_secs = 300", "depth_every_secs = 420"),
        ("depth_levels = 10", "depth_levels = 0"),
        ("l2_before_secs = 300", "l2_before_secs = 3601"),
        ("l2_max_window_secs = 1800", "l2_max_window_secs = 30"),
        ("archive_budget_gb = 40", "archive_budget_gb = 71"),
        ("lag_alarm_hours = 3", "lag_alarm_hours = 0"),
    ] {
        assert!(REPO.contains(from), "{from}");
        assert!(
            Config::parse(&REPO.replace(from, to)).is_err(),
            "{to} must be refused"
        );
    }
    assert!(
        Config::parse(&format!("{REPO}\nsurprise = 1\n")).is_err(),
        "unknown keys refused"
    );
    let (_, a) = Config::parse(REPO).unwrap();
    let (_, b) =
        Config::parse(&REPO.replace("lag_alarm_hours = 3", "lag_alarm_hours = 4")).unwrap();
    assert_ne!(a, b, "the manifest's config sha changes with the file");
}
