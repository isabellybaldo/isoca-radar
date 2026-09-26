#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::*;
use std::collections::BTreeMap;

fn t(a: &str, v: &str, h: &str) -> (String, String, String) {
    (a.to_string(), v.to_string(), h.to_string())
}

#[test]
fn sha8_and_effective_versions_use_the_first_8_hex_of_sha256() {
    // sha256("") = e3b0c442…, sha256("abc") = ba7816bf… (FIPS 180-2 test vectors)
    assert_eq!(sha8(""), "e3b0c442");
    assert_eq!(sha8("abc"), "ba7816bf");
    assert_eq!(effective_version("1", ""), "1+e3b0c442");
    assert_eq!(censoring_section(30_000), "gap_ms=30000");
    assert_eq!(
        episode_def_section(500, 60_000, 3_600_000, 2_000),
        "stale_lag_ms=500;suspect_window_ms=60000;l1_suspect_window_ms=3600000;drift_settle_ms=2000"
    );
    assert_eq!(sim_section("ab"), "policies_sha256=ab");
}

#[test]
fn host_bands_follow_the_spec_edges() {
    assert_eq!(host_band(None), "unmeasured");
    assert_eq!(host_band(Some(0)), "lt25");
    assert_eq!(host_band(Some(24)), "lt25");
    assert_eq!(host_band(Some(25)), "25-100");
    assert_eq!(host_band(Some(99)), "25-100");
    assert_eq!(host_band(Some(100)), "100-250");
    assert_eq!(host_band(Some(250)), "100-250");
    assert_eq!(host_band(Some(251)), "gt250");
    assert_eq!(host_version("vps-kvm2", Some(120)), "1:vps-kvm2|100-250");
    assert_eq!(host_version("pc-dev", None), "1:pc-dev|unmeasured");
    assert!(valid_host_label("vps-kvm2") && valid_host_label("pc-dev") && valid_host_label("a"));
    assert!(!valid_host_label("") && !valid_host_label("VPS") && !valid_host_label("a b"));
    assert!(!valid_host_label("a|b") && !valid_host_label(&"x".repeat(33)));
}

#[test]
fn premise_keys_are_sorted_and_need_every_area() {
    let mut v = BTreeMap::new();
    v.insert("fees".to_string(), "1+aa".to_string());
    v.insert("episode_def".to_string(), "2+bb".to_string());
    v.insert("sim".to_string(), "1+cc".to_string());
    assert_eq!(
        premise_key(&["fees", "episode_def"], &v).as_deref(),
        Some("episode_def=2+bb;fees=1+aa")
    );
    assert_eq!(premise_key(&["fees", "depth"], &v), None);
}

#[test]
fn the_premises_table_is_sorted_unique_and_its_versions_are_plain_tokens() {
    let areas: Vec<&str> = PREMISES.iter().map(|p| p.0).collect();
    let mut sorted = areas.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(areas, sorted);
    for (area, version, _) in PREMISES {
        assert!(!version.is_empty(), "{area}");
        assert!(
            !version.contains(['=', ';', '+', '|', ':', ' ']),
            "{area}: version {version:?} must be a plain token"
        );
        assert_eq!(code_version(area), Some(*version));
    }
    assert_eq!(code_version("nope"), None);
}

#[test]
fn file_hashes_ignore_carriage_returns_but_not_paths() {
    let lf = files_hash(&[("a.rs", "x\ny\n")]);
    assert_eq!(files_hash(&[("a.rs", "x\r\ny\r\n")]), lf);
    assert_ne!(files_hash(&[("b.rs", "x\ny\n")]), lf);
    assert_eq!(lf.len(), 64);
    assert_eq!(files_hash(&[]).len(), 64);
}

#[test]
fn an_unbumped_change_fails_and_refresh_refuses_it() {
    let lock = vec![t("fees", "1", "aaa"), t("depth", "1", "bbb")];
    // fee.rs changed, version still 1
    let now = vec![t("fees", "1", "ccc"), t("depth", "1", "bbb")];
    let p = lock_problems(&now, &lock);
    assert_eq!(p.len(), 1);
    assert!(
        p[0].contains("fees: its files changed but its version is still 1"),
        "{p:?}"
    );
    assert!(refreshed_lock(&now, &lock).is_err());
    // bumped: the check still asks for a refresh, and the refresh is allowed
    let bumped = vec![t("fees", "2", "ccc"), t("depth", "1", "bbb")];
    let p = lock_problems(&bumped, &lock);
    assert_eq!(p, vec!["fees: version 1 → 2; refresh the lock".to_string()]);
    let text = refreshed_lock(&bumped, &lock).unwrap();
    assert_eq!(parse_lock(&text), bumped);
    assert!(lock_problems(&bumped, &parse_lock(&text)).is_empty());
}

#[test]
fn new_and_removed_areas_ask_for_a_refresh_and_the_lock_parses_crlf() {
    let lock = parse_lock("# header\r\nfees 1 aaa\r\n\r\nold 1 zzz\r\n");
    assert_eq!(lock, vec![t("fees", "1", "aaa"), t("old", "1", "zzz")]);
    let now = vec![t("fees", "1", "aaa"), t("sim", "1", "ddd")];
    let p = lock_problems(&now, &lock);
    assert!(
        p.iter().any(|x| x.starts_with("sim: not in premises.lock")),
        "{p:?}"
    );
    assert!(
        p.iter()
            .any(|x| x.starts_with("old: in premises.lock but no longer")),
        "{p:?}"
    );
    assert!(refreshed_lock(&now, &lock).is_ok());
}
