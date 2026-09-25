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

// ---------- F1: non-positive Some is not positive ----------

#[test]
fn f1_non_positive_some_never_opens_an_episode() {
    let mut t = EpisodeTracker::new(30_000);
    assert!(t.on_state("ev1", 1_000, Some(dec!(0))).is_empty());
    assert!(t.on_state("ev1", 2_000, Some(dec!(-0.5))).is_empty());
}

#[test]
fn f1_open_episode_closes_on_some_zero() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    let e = t.on_state("ev1", 2_000, Some(dec!(0)));
    assert!(matches!(&e[0], EpisodeEvent::Close { .. }));
}

// ---------- F2: heartbeat + shutdown ----------

#[test]
fn f2_on_alive_advances_gap_clock_without_flagging_a_gap() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    let mut ts = 1_000;
    while ts < 61_000 {
        ts += 10_000;
        t.on_alive("ev1", ts);
    }
    let e = t.close_all(62_000);
    assert_eq!(e.len(), 1);
    match &e[0] {
        EpisodeEvent::Close {
            duration_ms,
            censor,
            ..
        } => {
            assert_eq!(*duration_ms, 61_000);
            assert!(!censor.gap);
            assert!(censor.right);
        }
        other => panic!("expected close, got {other:?}"),
    }
}

#[test]
fn f2_on_alive_flags_a_gap_after_long_silence() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, Some(dec!(0.5)));
    t.on_alive("ev1", 40_000); // 39 s of silence
    let e = t.close_all(41_000);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, .. } if censor.gap));
}

#[test]
fn f2_on_alive_is_a_noop_with_nothing_open() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_alive("ev1", 1_000); // nothing open: no panic, no episode created
    assert!(t.on_state("ev1", 2_000, None).is_empty());
}

#[test]
fn f2_close_all_closes_every_open_episode_right_censored_ids_ascending() {
    let mut t = EpisodeTracker::new(30_000);
    t.on_state("a", 0, Some(dec!(1)));
    t.on_state("b", 0, Some(dec!(1)));
    let e = t.close_all(5_000);
    assert_eq!(e.len(), 2);
    let ids: Vec<u64> = e
        .iter()
        .map(|ev| match ev {
            EpisodeEvent::Close { id, censor, .. } => {
                assert!(censor.right);
                *id
            }
            other => panic!("expected close, got {other:?}"),
        })
        .collect();
    assert!(ids[0] < ids[1]);
    // after close_all, the next positive state is a first sight again: left-censored.
    let e = t.on_state("a", 6_000, Some(dec!(1)));
    assert!(matches!(&e[0], EpisodeEvent::Open { .. }));
    let e = t.on_state("a", 7_000, None);
    assert!(matches!(&e[0], EpisodeEvent::Close { censor, .. } if censor.left));
}

#[test]
fn open_censor_reads_left_at_open_time_without_a_close() {
    let mut t = EpisodeTracker::new(30_000);
    assert!(t.open_censor("ev1").is_none()); // nothing open yet
    t.on_state("ev1", 1_000, Some(dec!(0.5))); // first sight, positive: left-censored
    assert!(t.open_censor("ev1").unwrap().left);

    let mut t = EpisodeTracker::new(30_000);
    t.on_state("ev1", 1_000, None); // first sight, not positive
    t.on_state("ev1", 2_000, Some(dec!(0.5))); // opens on the second, non-first sight
    assert!(!t.open_censor("ev1").unwrap().left);
}
