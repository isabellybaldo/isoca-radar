//! Spec §4 enforcement: a premise area's files may not change while its version stays the same.
//! Refresh (after bumping a version in src/premises.rs):
//!     PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock
#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::premises::{PREMISES, files_hash, lock_problems, parse_lock, refreshed_lock};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn lock_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("premises.lock")
}

fn current() -> Vec<(String, String, String)> {
    PREMISES
        .iter()
        .map(|(area, version, files)| {
            let texts: Vec<(String, String)> = files
                .iter()
                .map(|f| {
                    let text = std::fs::read_to_string(root().join(f))
                        .unwrap_or_else(|e| panic!("read {f}: {e}"));
                    (f.to_string(), text)
                })
                .collect();
            let refs: Vec<(&str, &str)> = texts
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str()))
                .collect();
            (area.to_string(), version.to_string(), files_hash(&refs))
        })
        .collect()
}

#[test]
fn premises_lock_matches_the_sources() {
    let lock = parse_lock(&std::fs::read_to_string(lock_path()).unwrap_or_default());
    let now = current();
    if std::env::var("PREMISES_LOCK_WRITE").is_ok() {
        match refreshed_lock(&now, &lock) {
            Ok(text) => {
                std::fs::write(lock_path(), text).unwrap();
                println!("premises.lock refreshed");
                return;
            }
            Err(p) => panic!("refusing to refresh premises.lock:\n{}", p.join("\n")),
        }
    }
    let problems = lock_problems(&now, &lock);
    assert!(
        problems.is_empty(),
        "premises.lock is out of step:\n{}\nBump the area's version in crates/arb-core/src/premises.rs when \
         the change alters what the area means, then run: \
         PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock",
        problems.join("\n")
    );
}
