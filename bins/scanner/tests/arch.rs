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
    for pure in ["arb-core"] {
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
}
