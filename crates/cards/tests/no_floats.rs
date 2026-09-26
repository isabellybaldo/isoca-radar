//! Phase 5a: cards must never use floating point (same scan as arb-core).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;
use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, hits);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let bytes = line.as_bytes();
                for needle in ["f32", "f64"] {
                    let mut start = 0;
                    while let Some(pos) = line[start..].find(needle) {
                        let at = start + pos;
                        let before_ok = at == 0
                            || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
                        let end = at + needle.len();
                        let after_ok = end >= bytes.len()
                            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
                        if before_ok && after_ok {
                            hits.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                        }
                        start = end;
                    }
                }
            }
        }
    }
}

#[test]
fn no_floating_point_in_src() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan(&src, &mut hits);
    assert!(
        hits.is_empty(),
        "floating point found:\n{}",
        hits.join("\n")
    );
}

#[test]
fn detector_sees_a_float() {
    let dir = std::env::temp_dir().join("cards_no_floats_probe");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.rs"), "let x: f64 = 1.0;\nlet buf64 = 1;\n").unwrap();
    let mut hits = Vec::new();
    scan(&dir, &mut hits);
    assert_eq!(hits.len(), 1, "{hits:?}");
}
