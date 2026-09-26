#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::routes::*;
use std::fs;
use std::path::Path;

#[test]
fn route_table_is_exactly_the_read_only_set() {
    assert_eq!(
        ALLOWED,
        &[
            ("GET", "https://gamma-api.polymarket.com/events/keyset"),
            ("GET", "https://gamma-api.polymarket.com/markets"),
            ("GET", "https://clob.polymarket.com/book"),
            ("POST", "https://clob.polymarket.com/books"),
            ("GET", "https://clob.polymarket.com/ok"),
            ("WS", "wss://ws-subscriptions-clob.polymarket.com/ws/market"),
            ("WS", "wss://sports-api.polymarket.com/ws"),
        ]
    );
}

#[test]
fn table_agrees_with_the_constants() {
    assert_eq!(ALLOWED[0].1, format!("{GAMMA_BASE}{GAMMA_EVENTS_KEYSET}"));
    assert_eq!(ALLOWED[1].1, format!("{GAMMA_BASE}{GAMMA_MARKETS}"));
    assert_eq!(ALLOWED[2].1, format!("{CLOB_BASE}{CLOB_BOOK}"));
    assert_eq!(ALLOWED[3].1, format!("{CLOB_BASE}{CLOB_BOOKS}"));
    assert_eq!(ALLOWED[4].1, format!("{CLOB_BASE}{CLOB_OK}"));
    assert_eq!(ALLOWED[5].1, WS_MARKET);
    assert_eq!(ALLOWED[6].1, WS_SPORTS);
}

/// Recursively collect every file under `dir`.
fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn no_url_literals_outside_routes_rs() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    files_under(&src, &mut files);
    let mut hits = Vec::new();
    for path in files {
        if path.file_name().is_some_and(|n| n == "routes.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for scheme in ["http://", "https://", "ws://", "wss://"] {
            if text.contains(scheme) {
                hits.push(format!("{} contains {scheme}", path.display()));
            }
        }
    }
    assert!(hits.is_empty(), "{hits:?}");
}
