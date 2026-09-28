#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::{HashMap, HashSet};

fn deps() -> HashMap<String, HashSet<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let mut m = HashMap::new();
    for p in v["packages"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap().to_string();
        let ds: HashSet<String> = p["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"].is_null()) // normal deps only
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect();
        m.insert(name, ds);
    }
    m
}

#[test]
fn crate_graph_matches_spec_9_2() {
    let d = deps();
    let io = [
        "tokio",
        "reqwest",
        "tokio-tungstenite",
        "sqlx",
        "pm-api",
        "store",
    ];
    for pure in ["arb-core", "sim", "cards", "tape"] {
        for bad in io {
            assert!(!d[pure].contains(bad), "{pure} must stay pure (no {bad})");
        }
    }
    assert!(
        !d["store"].contains("pm-api") && !d["store"].contains("reqwest"),
        "store has no network"
    );
    assert!(
        !d["pm-api"].contains("store") && !d["pm-api"].contains("sqlx"),
        "pm-api has no database"
    );
    for bad in ["reqwest", "tokio-tungstenite", "sqlx"] {
        assert!(
            !d["scanner"].contains(bad),
            "scanner reaches the network/db only via pm-api/store ({bad})"
        );
    }
    for bad in [
        "reqwest",
        "tokio-tungstenite",
        "sqlx",
        "pm-api",
        "sim",
        "arb-core",
    ] {
        assert!(
            !d["api"].contains(bad),
            "api reads Postgres only via store and has no network client or trading code ({bad})"
        );
    }
    for bad in ["reqwest", "tokio-tungstenite", "sqlx", "pm-api"] {
        assert!(
            !d["analyze"].contains(bad),
            "analyze reads Postgres only via store and has no network client ({bad})"
        );
    }
    for bad in ["reqwest", "tokio-tungstenite", "sqlx", "pm-api"] {
        assert!(
            !d["compact"].contains(bad),
            "compact reads Postgres only via store and has no network client ({bad})"
        );
    }
    for (name, ds) in &d {
        for c in ["parquet", "arrow"] {
            assert!(
                name == "compact" || !ds.contains(c),
                "{c} is compact-only (owner approval 2026-09-27), but {name} depends on it"
            );
        }
    }
    assert!(
        !d["cards"].contains("store"),
        "cards is pure: analyze does the IO"
    );
}

/// Pulls the `["a", "b", ...]` string list assigned to `const NAME: [&str; N] = [...]` out of
/// a guard.rs source file, regardless of whether it's written on one line or several.
fn denylist(src: &str, name: &str) -> Vec<String> {
    let after_name = src
        .split(&format!("const {name}"))
        .nth(1)
        .unwrap_or_else(|| panic!("{name} not found"));
    let after_eq = &after_name[after_name.find('=').expect("=")..];
    let open = after_eq.find('[').expect("[");
    let close = open + after_eq[open..].find(']').expect("]");
    after_eq[open + 1..close]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// bins/api/src/guard.rs is a hand-kept copy of bins/scanner/src/guard.rs's denylist (the api
/// can't import scanner's guard without pulling in pm-api). Keep the two in step.
#[test]
fn scanner_and_api_guard_denylists_are_identical() {
    let scanner_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/guard.rs"))
        .expect("scanner guard.rs");
    let api_src =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../api/src/guard.rs"))
            .expect("api guard.rs");
    let scanner_contains = denylist(&scanner_src, "CONTAINS");
    let api_contains = denylist(&api_src, "CONTAINS");
    let scanner_prefixes = denylist(&scanner_src, "PREFIXES");
    let api_prefixes = denylist(&api_src, "PREFIXES");
    let analyze_src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../analyze/src/guard.rs"
    ))
    .expect("analyze guard.rs");
    assert_eq!(
        scanner_contains,
        denylist(&analyze_src, "CONTAINS"),
        "CONTAINS drifted in analyze"
    );
    assert_eq!(
        scanner_prefixes,
        denylist(&analyze_src, "PREFIXES"),
        "PREFIXES drifted in analyze"
    );
    // A parity check that silently passed on two empty lists (a botched parser, both files
    // gutted) would be worse than useless, so pin both lists non-empty at their real sizes.
    assert!(!scanner_contains.is_empty(), "CONTAINS parsed as empty");
    assert!(!scanner_prefixes.is_empty(), "PREFIXES parsed as empty");
    assert_eq!(scanner_contains.len(), 5, "CONTAINS should have 5 entries");
    assert_eq!(scanner_prefixes.len(), 2, "PREFIXES should have 2 entries");
    assert_eq!(
        scanner_contains, api_contains,
        "CONTAINS denylist drifted between scanner and api guard.rs"
    );
    assert_eq!(
        scanner_prefixes, api_prefixes,
        "PREFIXES denylist drifted between scanner and api guard.rs"
    );
    let compact_src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../compact/src/guard.rs"
    ))
    .expect("compact guard.rs");
    assert_eq!(
        scanner_contains,
        denylist(&compact_src, "CONTAINS"),
        "CONTAINS drifted in compact"
    );
    assert_eq!(
        scanner_prefixes,
        denylist(&compact_src, "PREFIXES"),
        "PREFIXES drifted in compact"
    );
}
