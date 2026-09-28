#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8: every tob row the compactor emits equals the scanner engine's top of book for that
//! token after that very line (the Task 4 fixture, and #[ignore] a real capture: TAPE_AGREEMENT_DIR),
//! the day-opening rows too (a UTC day's first line gives every trusted token a row).
mod agreement;
use agreement::*;
use arb_core::money::{Price, Shares};
use compact::reduce::{ReduceCfg, Reducer};
use compact::rows::{VecSink, micros};
use scanner::engine::Engine;
use scanner::replay::{ReplayState, apply_line};
use std::collections::BTreeSet;
use std::path::Path;
use tape::universe_view::load_record;

type Side = Option<(i64, i64)>;

fn side(x: Option<(Price, Shares)>) -> Side {
    x.map(|(p, s)| (micros(p.get()).unwrap(), micros(s.get()).unwrap()))
}

/// The Task 4 fixture's tokens that ever receive an explicit book (ws) snapshot: `a3` never does
/// (it only ever gets price_change deltas), so it can never be trusted and never produces a tob
/// row (Task 11: no row is ever made from an untrusted book). Every other fixture token does.
const SNAPSHOTTED: [&str; 10] = ["a1", "a2", "b1", "b2", "cy", "cn", "d1", "d2", "c8y", "c8n"];

/// Drives the engine and the reducer side by side over `dir`. Returns the number of tob rows
/// checked and which tokens produced at least one.
fn check(dir: &Path) -> (usize, BTreeSet<String>) {
    let (n, seen, _) = check_split(dir, None);
    (n, seen)
}

/// `check`, with a UTC day starting for the reducer just before line `midnight` (0-based over the
/// whole run): its hour ends and the next day's hour begins there. Also returns the tokens given
/// a tob row at that line, after checking they are exactly the tokens trusted after it.
fn check_split(dir: &Path, midnight: Option<usize>) -> (usize, BTreeSet<String>, BTreeSet<String>) {
    const DAY: i64 = 86_400_000;
    let mut engine = Engine::new(ctx());
    let mut st = ReplayState::default();
    let mut red = Reducer::new(ReduceCfg {
        depth_every_ms: 300_000,
        depth_levels: 10,
    });
    let mut load = |taken: i64, file: &str| load_record(dir, taken, file);
    let mut rows = 0usize;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let (mut n_line, mut opened) = (0usize, BTreeSet::new());
    let mut shift = 0i64;
    for h in tape::files::listing(dir).hours {
        red.begin_hour(h.start_ms + shift, vec![]);
        tape::files::for_each_line(&h.path, |l| {
            apply_line(&mut engine, dir, &l, &mut st, &mut |_| {})?;
            let at_midnight = midnight == Some(n_line);
            n_line += 1;
            if at_midnight {
                red.end_hour(&mut VecSink::default())?;
                shift = DAY;
                red.begin_hour(h.start_ms + shift, vec![]);
            }
            let mut sink = VecSink::default();
            red.line(&l, &mut load, &mut sink)?;
            if at_midnight {
                for row in &sink.tob {
                    opened.insert(
                        red.tokens()[usize::try_from(row.token_idx)?]
                            .token_id
                            .clone(),
                    );
                }
                assert_eq!(
                    opened,
                    red.state().trusted,
                    "the day's first line gives every trusted token a row"
                );
            }
            for row in &sink.tob {
                let tok = red.tokens()[usize::try_from(row.token_idx)?]
                    .token_id
                    .clone();
                let top = engine
                    .book_of(&tok)
                    .map(|b| b.top())
                    .unwrap_or_else(|| panic!("the engine holds no book for {tok}"));
                assert_eq!(
                    (row.bid.zip(row.bid_sz), row.ask.zip(row.ask_sz)),
                    (side(top.best_bid), side(top.best_ask)),
                    "tob row for {tok} at recv_ms {} differs from the engine's top",
                    row.recv_ms
                );
                seen.insert(tok);
            }
            rows += sink.tob.len();
            Ok(())
        })
        .unwrap();
        red.end_hour(&mut VecSink::default()).unwrap();
    }
    (rows, seen, opened)
}

#[test]
fn every_tob_row_equals_the_engines_top_at_that_line() {
    let dir = tmp("tob");
    fixture(&dir);
    let (n, seen) = check(&dir);
    assert!(n > 100, "{n} tob rows checked");
    for tok in SNAPSHOTTED {
        assert!(
            seen.contains(tok),
            "token {tok} has a book snapshot in the fixture but produced no tob row"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_day_opening_rows_equal_the_engines_top_at_that_line() {
    let dir = tmp("tob-midnight");
    fixture(&dir);
    let lines = tape::files::listing(&dir)
        .hours
        .iter()
        .map(|h| {
            let mut n = 0usize;
            tape::files::for_each_line(&h.path, |_| {
                n += 1;
                Ok(())
            })
            .unwrap();
            n
        })
        .sum::<usize>();
    let (n, _, opened) = check_split(&dir, Some(lines / 2));
    assert!(n > 100, "{n} tob rows checked");
    assert!(
        opened.len() >= 3,
        "several tokens are trusted mid-fixture and open the day: {opened:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore]
fn every_tob_row_equals_the_engines_top_on_a_real_capture() {
    let Ok(dir) = std::env::var("TAPE_AGREEMENT_DIR") else {
        eprintln!("SKIPPED: TAPE_AGREEMENT_DIR not set");
        return;
    };
    let (n, _) = check(Path::new(&dir));
    eprintln!("tob agreement: {n} rows equal the engine's top");
}
