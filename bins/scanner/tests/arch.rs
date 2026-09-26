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
    for pure in ["arb-core", "sim"] {
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
    assert_eq!(
        denylist(&scanner_src, "CONTAINS"),
        denylist(&api_src, "CONTAINS"),
        "CONTAINS denylist drifted between scanner and api guard.rs"
    );
    assert_eq!(
        denylist(&scanner_src, "PREFIXES"),
        denylist(&api_src, "PREFIXES"),
        "PREFIXES denylist drifted between scanner and api guard.rs"
    );
}
