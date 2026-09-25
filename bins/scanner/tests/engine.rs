#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::Side;
use common::*;
use pm_api::ws::MarketEvent;
use rust_decimal_macros::dec;
use scanner::engine::Out;
use scanner::universe::Universe;
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn yes_basket_opens_then_closes_with_exact_timing() {
    let mut e = engine();
    assert!(episodes(&seed(&mut e)).is_empty());
    // y0 ask 0.40 → 0.30: ΣA = 0.95 → +0.05/share × 10
    let outs = e.on_frame(
        0,
        2_150,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    let open = episodes(&outs);
    assert_eq!(open.len(), 1);
    let r = &open[0];
    assert_eq!(r.episode_id, "live-7-00000001");
    assert_eq!(
        (r.kind.as_str(), r.realization.as_str()),
        ("yes_basket", "settlement")
    );
    assert_eq!(r.opened_server_ms, 2_000);
    assert_eq!(r.closed_server_ms, None);
    assert!(!r.censor_left, "the group was seen non-positive first");
    assert_eq!(r.peak_pnl_net_usd, dec!(0.5));
    assert_eq!(r.peak_max_size, dec!(10));
    assert_eq!((r.n_outcomes, r.n_legs, r.legs_dropped), (3, 3, 0));
    assert_eq!(r.detect_lag_ms, Some(150));
    assert!(!r.stale_transport);
    assert_eq!(r.pnl_net_budget100_usd, Some(dec!(0.5)));
    assert_eq!(r.below_min_size, Some(false));
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Recheck(q) if q.episode_id == "live-7-00000001"))
    );

    // y1 ask 0.35 → 0.40: ΣA = 1.00 → closes
    let outs = e.on_frame(
        0,
        3_600,
        &[
            ask("e1-y1", 3_500, "0.40", "10"),
            ask("e1-y1", 3_500, "0.35", "0"),
        ],
    );
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].closed_server_ms, Some(3_500));
    assert_eq!(closed[0].duration_ms, Some(1_500));
    assert!(!closed[0].censor_right && !closed[0].censor_gap);
    assert_eq!(e.open_episodes(), 0);
}

#[test]
fn a_frame_is_evaluated_once_after_all_its_events() {
    let mut e = engine();
    seed(&mut e);
    // intermediate state (after y0 moves) would be ΣA = 0.95, but the frame ends at ΣA = 1.05
    let outs = e.on_frame(
        0,
        2_100,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
            ask("e1-y1", 2_000, "0.45", "10"),
            ask("e1-y1", 2_000, "0.35", "0"),
        ],
    );
    assert!(episodes(&outs).is_empty());
}

#[test]
fn server_top_that_disagrees_with_ours_is_an_integrity_event_and_a_resnapshot() {
    let mut e = engine();
    seed(&mut e);
    let outs = e.on_frame(
        0,
        2_100,
        &[MarketEvent::Delta {
            token: "e1-y2".into(),
            server_ts_ms: 2_000,
            side: Side::Ask,
            price: p("0.50"),
            size: sh("5"),
            best_bid: None,
            best_ask: Some(p("0.29")), // ours is 0.30
        }],
    );
    assert!(outs.iter().any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch" && i.token.as_deref() == Some("e1-y2"))));
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Resnapshot { token } if token == "e1-y2"))
    );
}

#[test]
fn crossed_book_is_flagged_and_never_an_opportunity() {
    let mut e = engine();
    let outs = e.on_frame(
        0,
        1_100,
        &[
            snap("e1-y0", 1_000, &[("0.60", "10")], &[("0.20", "10")]),
            snap("e1-y1", 1_000, &[], &[("0.20", "10")]),
            snap("e1-y2", 1_000, &[], &[("0.20", "10")]),
        ],
    );
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "crossed_book"))
    );
    assert!(episodes(&outs).is_empty());
}

#[test]
fn disconnect_right_censors_and_the_next_positive_state_is_left_censored() {
    let mut e = engine();
    seed(&mut e);
    e.on_frame(
        0,
        2_100,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    let outs = e.disconnect(0, 2_500);
    assert!(outs.iter().any(
        |o| matches!(o, Out::Integrity(i) if i.kind == "reconnect_gap" && i.shard == Some(0))
    ));
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1);
    assert!(closed[0].censor_right);
    assert_eq!(closed[0].closed_server_ms, Some(2_500));
    // after reconnect the initial dump shows the basket still positive: a NEW, left-censored episode
    let outs = e.on_frame(
        0,
        3_100,
        &[
            snap("e1-y0", 3_000, &[], &[("0.30", "10")]),
            snap("e1-y1", 3_000, &[], &[("0.35", "10")]),
            snap("e1-y2", 3_000, &[], &[("0.30", "10")]),
        ],
    );
    let reopened = episodes(&outs);
    assert_eq!(reopened.len(), 1);
    assert_eq!(reopened[0].episode_id, "live-7-00000002");
    assert!(reopened[0].censor_left);
}

#[test]
fn reapplying_the_universe_keeps_books() {
    let mut e = engine();
    seed(&mut e);
    let u = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![meta("e1", 3)],
        canary: vec![],
    });
    let conn: HashMap<String, usize> = [("e1".to_string(), 0)].into_iter().collect();
    assert!(e.apply_universe(u, &conn, 1_500).is_empty());
    // only y0 moves; y1/y2 must still be known from before the refresh
    let outs = e.on_frame(
        0,
        2_100,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    assert_eq!(episodes(&outs).len(), 1);
}

#[test]
fn an_event_leaving_the_universe_closes_its_episode_right_censored() {
    let mut e = engine();
    seed(&mut e);
    e.on_frame(
        0,
        2_100,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    let empty = Arc::new(Universe {
        taken_ms: 2,
        groups: vec![],
        canary: vec![],
    });
    let outs = e.apply_universe(empty, &HashMap::new(), 2_600);
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1);
    assert!(closed[0].censor_right);
    assert_eq!(e.open_episodes(), 0);
}
