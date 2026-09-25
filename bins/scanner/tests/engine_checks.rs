#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use common::*;
use scanner::engine::Out;

fn book(ts: i64, ask: &str) -> Book {
    let mut b = Book::default();
    b.apply_snapshot(
        &[],
        &[(Price::parse(ask).unwrap(), Shares::parse("10").unwrap())],
        ts,
    );
    b
}

fn open(e: &mut scanner::engine::Engine) {
    seed(e);
    e.on_frame(
        0,
        2_100,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
}

#[test]
fn alive_frames_prevent_a_false_gap() {
    let mut quiet = engine();
    open(&mut quiet);
    let outs = quiet.on_frame(
        0,
        41_100,
        &[
            ask("e1-y1", 41_000, "0.40", "10"),
            ask("e1-y1", 41_000, "0.35", "0"),
        ],
    );
    assert!(episodes(&outs)[0].censor_gap, "no liveness for 39 s → gap");

    let mut live = engine();
    open(&mut live);
    live.alive(0, 20_000);
    live.alive(0, 40_000);
    let outs = live.on_frame(
        0,
        41_100,
        &[
            ask("e1-y1", 41_000, "0.40", "10"),
            ask("e1-y1", 41_000, "0.35", "0"),
        ],
    );
    assert!(!episodes(&outs)[0].censor_gap);
}

#[test]
fn rest_snapshot_never_regresses_a_newer_book() {
    let mut e = engine();
    seed(&mut e); // y0 ask 0.40 @ 1000
    assert!(e.rest_snapshot("e1-y0", book(500, "0.10")).is_empty());
    assert_eq!(e.book_of("e1-y0").unwrap().server_ts_ms(), 1_000);
    // a newer REST book applies and is evaluated: ΣA = 0.30 + 0.35 + 0.30 = 0.95 → opens
    let outs = e.rest_snapshot("e1-y0", book(1_500, "0.30"));
    assert_eq!(episodes(&outs).len(), 1);
}

#[test]
fn drift_is_reported_only_after_settling_and_only_if_we_did_not_move() {
    let mut e = engine();
    seed(&mut e); // y0 ask 0.40 @ 1000
    e.drift_check("e1-y0", book(1_500, "0.41"), 10_000);
    assert!(e.tick(11_999).is_empty(), "not due yet");
    let outs = e.tick(12_000);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Drift { token, .. } if token == "e1-y0"))
    );

    // our book updated after the REST snapshot → inconclusive, nothing reported
    let mut e = engine();
    seed(&mut e);
    e.drift_check("e1-y0", book(1_500, "0.41"), 10_000);
    e.on_frame(0, 1_700, &[ask("e1-y0", 1_600, "0.39", "10")]);
    assert!(e.tick(12_000).is_empty());
}

#[test]
fn suspect_mark_flags_the_next_episode() {
    let mut e = engine();
    e.mark_suspect("e1", 10_000);
    open(&mut e);
    let outs = e.close_all(3_000);
    let r = &episodes(&outs)[0];
    assert!(r.engine_suspect);
    assert!(r.censor_right, "close_all right-censors");
}

#[test]
fn rates_count_frames_per_event_and_reset() {
    let mut e = engine();
    seed(&mut e);
    e.on_frame(0, 1_300, &[ask("e1-y0", 1_200, "0.41", "10")]);
    let r = e.rates();
    assert_eq!(r.get("e1"), Some(&4)); // 3 snapshots + 1 delta
    assert!(e.rates().is_empty());
}

#[test]
fn drift_is_never_reported_for_a_book_that_never_received_data() {
    let mut e = engine(); // no snapshot for e1-y2 yet (e.g. its group was dropped by the budget)
    e.drift_check("e1-y2", book(1_500, "0.41"), 10_000);
    assert!(e.tick(12_000).is_empty());
}
