//! The capture thread's housekeeping (disk-pause guard, retention, the capture byte budget,
//! stale universe files) must run on its very first message and then every 30 s. Soak run 3
//! (2026-09-26) found it never ran in release: the timer started at `i64::MIN`, so
//! `now - last_check` wrapped negative and the 10 GB cap was never enforced (24 GB captured).
use scanner::capture::{HOUSEKEEPING_EVERY_MS, housekeeping_due};

const NOW: i64 = 1_790_430_000_000; // a real 2026 wall-clock ms

#[test]
fn housekeeping_runs_on_the_first_message() {
    assert!(housekeeping_due(None, NOW));
    assert!(housekeeping_due(None, 0));
    assert!(housekeeping_due(None, i64::MIN));
}

#[test]
fn housekeeping_then_runs_every_30_s() {
    assert_eq!(HOUSEKEEPING_EVERY_MS, 30_000);
    assert!(!housekeeping_due(Some(NOW), NOW));
    assert!(!housekeeping_due(Some(NOW), NOW + 29_999));
    assert!(housekeeping_due(Some(NOW), NOW + 30_000));
}

#[test]
fn a_clock_step_backwards_runs_housekeeping_instead_of_stalling() {
    // Wall clock stepped back an hour: run now (and restart the 30 s cadence from the new
    // clock) rather than waiting an hour for the clock to catch up. No overflow at extremes.
    assert!(housekeeping_due(Some(NOW), NOW - 3_600_000));
    assert!(housekeeping_due(Some(i64::MAX), i64::MIN));
    assert!(housekeeping_due(Some(i64::MIN), i64::MAX));
}
