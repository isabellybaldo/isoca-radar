#![allow(clippy::unwrap_used, clippy::expect_used)]
use sim::config::{BurstStep, Scenario};
use sim::latency::{FrameRate, burst_multiplier, exec_delay_ms};

fn sp(honor: bool) -> Scenario {
    Scenario {
        name: "sp".into(),
        proc_ms: 5,
        net_ms: 145,
        match_ms: 150,
        honor_seconds_delay: honor,
        burst: vec![
            BurstStep {
                frames_per_s_above: 2000,
                multiplier: 3,
            },
            BurstStep {
                frames_per_s_above: 6000,
                multiplier: 10,
            },
        ],
    }
}

#[test]
fn delay_is_proc_net_match_and_seconds_delay() {
    assert_eq!(exec_delay_ms(&sp(true), Some(3), 0), 5 + 145 + 150 + 3000);
    assert_eq!(
        exec_delay_ms(&sp(false), Some(3), 0),
        300,
        "Q1 sensitivity ignores secondsDelay"
    );
    assert_eq!(exec_delay_ms(&sp(true), None, 0), 300);
    assert_eq!(
        exec_delay_ms(&sp(true), Some(-1), 0),
        300,
        "a negative delay never shortens"
    );
}

#[test]
fn burst_multiplies_matching_time_strictly_above_thresholds() {
    assert_eq!(burst_multiplier(&sp(true), 2000), 1);
    assert_eq!(burst_multiplier(&sp(true), 2001), 3);
    assert_eq!(burst_multiplier(&sp(true), 7000), 10);
    assert_eq!(exec_delay_ms(&sp(true), None, 2001), 5 + 145 + 450);
    assert_eq!(exec_delay_ms(&sp(true), None, 7000), 5 + 145 + 1500);
}

#[test]
fn frame_rate_counts_the_trailing_window() {
    let mut r = FrameRate::new(1000);
    for t in [0, 500, 999] {
        r.push(t);
    }
    assert_eq!(r.per_second(), 3);
    r.push(1000); // evicts t=0 (outside (0, 1000])
    assert_eq!(r.per_second(), 3);
    r.push(2500);
    assert_eq!(r.per_second(), 1);
}
