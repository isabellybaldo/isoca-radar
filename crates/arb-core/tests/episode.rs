#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::episode::{EpisodeEvent, EpisodeTracker};
use rust_decimal_macros::dec;

#[test]
fn opens_tracks_one_shot_peak_and_closes() {
    let mut t = EpisodeTracker::new(30_000);
    assert!(t.on_state("ev1", 1_000, None).is_empty()); // first sight, not positive
    let e = t.on_state("ev1", 2_000, Some(dec!(0.40)));
    assert!(matches!(e[0], EpisodeEvent::Open { ts_ms: 2_000, .. }));
    let e = t.on_state("ev1", 3_000, Some(dec!(1.10)));
    assert!(matches!(e[0], EpisodeEvent::Peak { ts_ms: 3_000, .. }));
    assert!(t.on_state("ev1", 4_000, Some(dec!(0.20))).is_empty()); // lower: not a new peak
    let e = t.on_state("ev1", 5_000, None);
    match &e[0] {
        EpisodeEvent::Close {
            duration_ms,
            peak_pnl,
            censor,
            ..
        } => {
            assert_eq!(*duration_ms, 3_000);
            assert_eq!(*peak_pnl, dec!(1.10)); // one-shot peak, never summed
            assert!(!censor.left && !censor.right && !censor.gap);
        }
        other => panic!("expected close, got {other:?}"),
    }
}

#[test]
fn positive_on_first_sight_is_left_censored() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    let e = t.on_state("ev1", 2_000, None);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, .. } if censor.left));
}

#[test]
fn disconnect_right_censors_and_next_sight_is_left_censored() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, None);
    t.on_state("ev1", 2_000, Some(dec!(0.5)));
    let e = t.on_disconnect("ev1", 2_500);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, duration_ms: 500, .. } if censor.right));
    t.on_state("ev1", 9_000, Some(dec!(0.5)));
    let e = t.on_state("ev1", 9_500, None);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, .. } if censor.left));
}

#[test]
fn long_feed_gap_inside_an_episode_is_flagged() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 0, None);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    t.on_state("ev1", 40_000, Some(dec!(0.5))); // 39 s since the last update
    let e = t.on_state("ev1", 41_000, None);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, .. } if censor.gap));
}

#[test]
fn gap_before_the_closing_update_is_flagged() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 0, None);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    // no update for 39 s, then the state turns non-positive
    let e = t.on_state("ev1", 40_000, None);
    assert!(
        matches!(&e[0], EpisodeEvent::Close { censor, duration_ms: 39_000, .. } if censor.gap && !censor.left && !censor.right)
    );
}

#[test]
fn groups_are_independent() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("a", 0, None);
    t.on_state("b", 0, None);
    t.on_state("a", 1, Some(dec!(1)));
    assert!(t.on_state("b", 2, None).is_empty());
}
