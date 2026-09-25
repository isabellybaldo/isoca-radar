#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use arb_core::book::{Book, Side};
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
    // the check is deferred (spec 2026-09-25): not yet reported in this same frame
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    // a later frame on the same conn, > 1 s past, resolves it
    let outs = e.on_frame(0, 3_200, &[ask("e1-y1", 3_001, "0.20", "10")]);
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

#[test]
fn a_server_top_of_zero_means_an_empty_side_not_a_mismatch() {
    let mut e = engine();
    seed(&mut e); // asks only: our bid side is empty
    let outs = e.on_frame(
        0,
        2_100,
        &[MarketEvent::Delta {
            token: "e1-y0".into(),
            server_ts_ms: 2_000,
            side: Side::Ask,
            price: p("0.40"),
            size: sh("12"),
            best_bid: Some(p("0")), // Polymarket's encoding of "no bids"
            best_ask: Some(p("0.40")),
        }],
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
}

fn delta(
    token: &str,
    ts: i64,
    side: Side,
    price: &str,
    size: &str,
    bb: Option<&str>,
    ba: Option<&str>,
) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side,
        price: p(price),
        size: sh(size),
        best_bid: bb.map(p),
        best_ask: ba.map(p),
    }
}

#[test]
fn the_top_check_uses_the_servers_top_after_the_whole_frame() {
    let mut e = engine();
    seed(&mut e); // e1-y0 ask 0.40
    // one message: add 0.42, then remove 0.40. Each item carries the FINAL top (0.42): after the
    // first item alone ours is still 0.40, which must not be flagged.
    let outs = e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.42", "10", None, Some("0.42")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.42")),
        ],
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "{outs:?}"
    );
    // a real disagreement: deferred (spec 2026-09-25) until a later update of the SAME token
    // proves the ts=3_000 update is over, at which point it's reported.
    let outs = e.on_frame(
        0,
        3_100,
        &[delta(
            "e1-y0",
            3_000,
            Side::Ask,
            "0.45",
            "5",
            None,
            Some("0.41"),
        )],
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "not yet resolved: {outs:?}"
    );
    let outs = e.on_frame(
        0,
        3_600,
        &[delta(
            "e1-y0",
            3_500,
            Side::Ask,
            "0.45",
            "0",
            None,
            Some("0.42"),
        )],
    );
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
}

#[test]
fn split_ws_messages_sharing_one_server_ts_do_not_false_mismatch() {
    let mut e = engine();
    // e1-y0: a bid at 0.30 plus two ask levels, 0.40 and 0.42
    e.on_frame(
        0,
        1_100,
        &[snap(
            "e1-y0",
            1_000,
            &[("0.30", "10")],
            &[("0.40", "5"), ("0.42", "5")],
        )],
    );
    // two SEPARATE websocket frames sharing ONE server ts: each carries the FINAL top (bb None,
    // ba 0.42), even though right after frame1 alone our book still shows the stale ask top
    // (0.40) — the false-positive pattern from the live smoke test.
    let outs1 = e.on_frame(
        0,
        2_100,
        &[delta(
            "e1-y0",
            2_000,
            Side::Bid,
            "0.30",
            "0",
            None,
            Some("0.42"),
        )],
    );
    assert!(
        !outs1
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "{outs1:?}"
    );
    let outs2 = e.on_frame(
        0,
        2_101,
        &[delta(
            "e1-y0",
            2_000,
            Side::Ask,
            "0.40",
            "0",
            None,
            Some("0.42"),
        )],
    );
    assert!(
        !outs2
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "{outs2:?}"
    );
    // a later frame on the same conn, > 1 s past, resolves cleanly: still none
    let outs3 = e.on_frame(0, 4_200, &[ask("e1-y1", 4_000, "0.20", "10")]);
    assert!(
        !outs3
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "{outs3:?}"
    );
}

#[test]
fn a_real_top_mismatch_is_reported_once_a_later_frame_resolves_it() {
    let mut e = engine();
    seed(&mut e); // e1-y0 ask 0.40
    let outs = e.on_frame(
        0,
        2_100,
        &[delta(
            "e1-y0",
            2_000,
            Side::Ask,
            "0.45",
            "5",
            None,
            Some("0.41"), // wrong: our top ask stays 0.40
        )],
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "deferred: {outs:?}"
    );
    // not yet 1 s past the pending entry's ts (2_000): still not reported
    let outs = e.on_frame(0, 2_600, &[ask("e1-y1", 2_500, "0.20", "10")]);
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch")),
        "{outs:?}"
    );
    // now > 1 s past: resolves and reports the real disagreement
    let outs = e.on_frame(0, 3_200, &[ask("e1-y1", 3_001, "0.20", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
}

#[test]
fn a_server_ask_of_one_means_an_empty_ask_side() {
    let mut e = engine();
    // bids only on y0: our ask side is empty
    e.on_frame(0, 1_100, &[snap("e1-y0", 1_000, &[("0.30", "10")], &[])]);
    let outs = e.on_frame(
        0,
        2_100,
        &[delta(
            "e1-y0",
            2_000,
            Side::Bid,
            "0.30",
            "12",
            Some("0.30"),
            Some("1"),
        )],
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
}

#[test]
fn a_book_crossed_only_mid_frame_is_not_flagged() {
    let mut e = engine();
    seed(&mut e); // e1-y0 ask 0.40
    // bid 0.45 appears (crossed vs ask 0.40) but the same frame lifts the ask to 0.50
    let outs = e.on_frame(
        0,
        2_100,
        &[
            delta(
                "e1-y0",
                2_000,
                Side::Bid,
                "0.45",
                "5",
                Some("0.45"),
                Some("0.50"),
            ),
            delta(
                "e1-y0",
                2_000,
                Side::Ask,
                "0.50",
                "10",
                Some("0.45"),
                Some("0.50"),
            ),
            delta(
                "e1-y0",
                2_000,
                Side::Ask,
                "0.40",
                "0",
                Some("0.45"),
                Some("0.50"),
            ),
        ],
    );
    assert!(
        !outs.iter().any(|o| matches!(o, Out::Integrity(i) if i.kind == "crossed_book" || i.kind == "top_mismatch")),
        "{outs:?}"
    );
}

#[test]
fn a_group_that_loses_its_connection_on_refresh_closes_right_censored_and_stays_dark() {
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
    assert_eq!(e.open_episodes(), 1);
    // refresh: the event stays in the universe but gets no connection this time (dropped by the
    // connection budget, or simply unassigned)
    let u = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![meta("e1", 3)],
        canary: vec![],
    });
    let outs = e.apply_universe(u, &HashMap::new(), 2_600);
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1, "{outs:?}");
    assert!(closed[0].censor_right);
    assert_eq!(closed[0].closed_server_ms, Some(2_600));
    assert_eq!(e.open_episodes(), 0);
    // a later REST snapshot for one of its legs opens nothing: the group has no connection
    let mut b = Book::default();
    b.apply_snapshot(&[], &[(p("0.10"), sh("10"))], 3_000);
    let outs = e.rest_snapshot("e1-y0", b);
    assert!(episodes(&outs).is_empty(), "{outs:?}");
    assert_eq!(e.open_episodes(), 0);
}

#[test]
fn a_leg_removed_from_an_open_episodes_event_closes_it_right_censored_without_panicking() {
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
    assert_eq!(
        e.open_episodes(),
        1,
        "the open episode's legs_used spans all 3 legs"
    );
    // refresh: same event, same connection, but the leg-list sequence changed (one leg gone)
    let mut m = meta("e1", 3);
    m.legs.truncate(2);
    let u = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![m],
        canary: vec![],
    });
    let conn: HashMap<String, usize> = [("e1".to_string(), 0)].into_iter().collect();
    let outs = e.apply_universe(u, &conn, 2_600);
    let closed = episodes(&outs);
    assert_eq!(closed.len(), 1, "{outs:?}");
    assert!(closed[0].censor_right);
    assert_eq!(closed[0].closed_server_ms, Some(2_600));
    assert_eq!(e.open_episodes(), 0);
}

/// y0 ask 0.40 → 0.30 opens a basket (ΣA = 0.95) at 2_000, but the server says the final top
/// ask is 0.31: our book is wrong, and the deferred check only finds out a frame later.
fn open_on_a_wrong_book(e: &mut scanner::engine::Engine) -> Vec<Out> {
    seed(e);
    e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.31")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.31")),
        ],
    )
}

#[test]
fn a_late_top_mismatch_flags_the_episode_already_open_in_its_group() {
    let mut e = engine();
    let open = episodes(&open_on_a_wrong_book(&mut e));
    assert_eq!(open.len(), 1);
    assert!(!open[0].engine_suspect, "nothing is known to be wrong yet");
    // a later frame on the same conn (> 1 s past 2_000) resolves the check as a real mismatch
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    let rows = episodes(&outs);
    let last = rows
        .iter()
        .rev()
        .find(|r| r.episode_id == "live-7-00000001")
        .expect("the open episode is re-emitted");
    assert!(last.engine_suspect);
    assert_eq!(last.closed_server_ms, None, "still open");
    let outs = e.close_all(4_000);
    assert!(episodes(&outs)[0].engine_suspect, "and it stays flagged");
}

#[test]
fn a_late_top_mismatch_also_flags_an_episode_that_closed_before_the_check_resolved() {
    let mut e = engine();
    open_on_a_wrong_book(&mut e);
    // y1 ask 0.35 → 0.40 at 2_500: ΣA = 1.00, the episode closes before y0's check is due
    let outs = e.on_frame(
        0,
        2_600,
        &[
            ask("e1-y1", 2_500, "0.40", "10"),
            ask("e1-y1", 2_500, "0.35", "0"),
        ],
    );
    let closed = episodes(&outs);
    assert_eq!(closed[0].closed_server_ms, Some(2_500));
    assert!(!closed[0].engine_suspect);
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    let r = episodes(&outs)
        .into_iter()
        .rev()
        .find(|r| r.episode_id == "live-7-00000001")
        .expect("the closed row is re-emitted with the flag");
    assert!(r.engine_suspect);
    assert_eq!(
        (r.closed_server_ms, r.duration_ms),
        (Some(2_500), Some(500))
    );
}

#[test]
fn a_clean_resolution_re_emits_nothing() {
    let mut e = engine();
    seed(&mut e);
    e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.30")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.30")),
        ],
    );
    let outs = e.on_frame(0, 3_200, &[ask("e1-y2", 3_001, "0.30", "10")]);
    assert!(episodes(&outs).is_empty(), "{outs:?}");
}
