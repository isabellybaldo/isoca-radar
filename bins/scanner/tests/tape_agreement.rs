#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8 "agreement with the scanner": FeedBooks' books equal the engine's after every line.
mod agreement;
use agreement::*;
use std::path::Path;

#[test]
fn feed_books_equal_the_engines_after_every_line() {
    let dir = tmp("fixture");
    fixture(&dir);
    let n = compare(&dir, Some(&TOKENS));
    assert!(n > 1_500, "{n} lines compared");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore]
fn feed_books_equal_the_engines_on_a_real_capture() {
    let Ok(dir) = std::env::var("TAPE_AGREEMENT_DIR") else {
        eprintln!("SKIPPED: TAPE_AGREEMENT_DIR not set");
        return;
    };
    let n = compare(Path::new(&dir), None);
    eprintln!("tape agreement: {n} lines, books equal after every one");
}
