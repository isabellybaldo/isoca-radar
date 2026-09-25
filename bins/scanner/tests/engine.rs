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

/// y0 ask 0.40 → 0.30 opens a REAL basket (ΣA = 0.96 < 1) at 2_000; the server's reported top ask
/// is 0.29, a level we LACK (0.30 > 0.29 is not proof of a phantom): nothing gets trimmed, so the
/// deferred check can only be resolved by a resnapshot, and the basket stays open until then.
fn open_on_a_wrong_book(e: &mut scanner::engine::Engine) -> Vec<Out> {
    seed(e);
    e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.29")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.29")),
        ],
    )
}

#[test]
fn a_late_top_mismatch_flags_the_episode_already_open_in_its_group() {
    let mut e = engine();
    let open = episodes(&open_on_a_wrong_book(&mut e));
    assert_eq!(open.len(), 1);
    assert!(!open[0].engine_suspect, "nothing is known to be wrong yet");
    // a later frame on the same conn (> 1 s past 2_000) resolves the check as a real mismatch: a
    // missing level (ours 0.30 is above the server's 0.29), so nothing is trimmed and the real,
    // still-open basket is only flagged suspect, not closed.
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
fn a_phantom_trim_that_empties_the_ask_side_closes_the_real_episode_early() {
    let mut e = engine();
    seed(&mut e); // e1 legs known at ts 1_000: ΣA = 1.05, no basket
    // y0 ask 0.40 → 0.30 opens a basket (ΣA = 0.95) at 2_000, but the server's own final top ask
    // is 0.31: our 0.30 is a phantom the stream itself disproves (Rule 1), not a missing level.
    let outs = e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.31")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.31")),
        ],
    );
    let open = episodes(&outs);
    assert_eq!(open.len(), 1);
    assert!(!open[0].engine_suspect);
    // a later frame on the same conn resolves the deferred check: trimming the phantom leaves
    // y0's ask side empty (the true 0.31 level isn't ours to show until a resnapshot lands), so
    // the basket it created is exposed as fake and closes in the SAME pass as the suspect flag —
    // first the still-open, now-suspect row, then its close, both stamped with the mismatch's
    // own ts (2_000), never the resolving frame's.
    let outs = e.on_frame(0, 3_200, &[ask("e1-y1", 3_001, "0.20", "10")]);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
    let mine: Vec<_> = episodes(&outs)
        .into_iter()
        .filter(|r| r.episode_id == "live-7-00000001")
        .collect();
    assert_eq!(mine.len(), 2, "{mine:?}");
    assert!(
        mine[0].engine_suspect && mine[0].closed_server_ms.is_none(),
        "{mine:?}"
    );
    assert!(
        mine[1].engine_suspect && mine[1].closed_server_ms == Some(2_000),
        "{mine:?}"
    );
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

#[test]
fn a_phantom_level_is_trimmed_from_the_streams_top_without_a_resnapshot() {
    let mut e = engine();
    seed(&mut e); // e1-y0 ask 0.40
    // the server removed our 0.40 ask without us seeing it; its final top ask is now 0.45
    e.on_frame(
        0,
        2_100,
        &[delta(
            "e1-y0",
            2_000,
            Side::Ask,
            "0.45",
            "5",
            None,
            Some("0.45"),
        )],
    );
    let outs = e.on_frame(0, 3_200, &[ask("e1-y1", 3_001, "0.20", "10")]); // > 1 s later: resolves
    let rows: Vec<_> = outs
        .iter()
        .filter_map(|o| match o {
            Out::Integrity(i) if i.kind == "top_mismatch" => Some(i.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 1, "{outs:?}");
    assert!(
        rows[0]
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("repaired from stream"),
        "{rows:?}"
    );
    assert!(
        !outs.iter().any(|o| matches!(o, Out::Resnapshot { .. })),
        "{outs:?}"
    );
    assert_eq!(
        e.book_of("e1-y0").unwrap().top().best_ask.map(|(x, _)| x),
        Some(p("0.45"))
    );
}

#[test]
fn a_persistent_mismatch_is_reported_at_most_once_per_throttle_window() {
    let mut e = engine(); // common::ctx(): issue_throttle_ms = 5_000
    seed(&mut e); // e1-y2 ask 0.30
    // the server keeps saying 0.29: a level we LACK, so only a resnapshot could fix it
    let mut details = Vec::new();
    let mut resnaps = 0;
    for k in 0..10_i64 {
        let t = 2_000 + k * 1_100; // each later delta resolves the previous pending check
        let outs = e.on_frame(
            0,
            t + 100,
            &[MarketEvent::Delta {
                token: "e1-y2".into(),
                server_ts_ms: t,
                side: Side::Ask,
                price: p("0.50"),
                size: sh("5"),
                best_bid: None,
                best_ask: Some(p("0.29")),
            }],
        );
        for o in &outs {
            match o {
                Out::Integrity(i) if i.kind == "top_mismatch" => {
                    details.push(i.detail.clone().unwrap_or_default())
                }
                Out::Resnapshot { token } if token == "e1-y2" => resnaps += 1,
                _ => {}
            }
        }
    }
    // resolutions at ts 2000, 3100, …, 10800: emitted at 2000 and 7500 only (window 5000)
    assert_eq!(details.len(), 2, "{details:?}");
    assert_eq!(resnaps, 2);
    assert!(details[1].ends_with("(+4 suppressed)"), "{details:?}");
    assert!(!details[0].contains("repaired"));
}

#[test]
fn a_non_repairing_trim_still_forces_the_groups_close_even_from_another_groups_frame() {
    let mut e = engine();
    seed(&mut e); // e1 legs known at ts 1_000: ΣA = 1.05, no basket
    // add a second, independent group on the SAME connection: its own traffic must be able to
    // resolve e1's deferred check without ever putting e1's group in that frame's `touched` set
    let u = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![meta("e1", 3), meta("e2", 1)],
        canary: vec![],
    });
    let conn: HashMap<String, usize> = [("e1".to_string(), 0), ("e2".to_string(), 0)]
        .into_iter()
        .collect();
    assert!(e.apply_universe(u, &conn, 0).is_empty());
    // y0 ask 0.40 → 0.30 opens a basket (ΣA = 0.95) at 2_000, but the server's final top ask is
    // 0.31: our 0.30 is a phantom (Rule 1), and trimming it leaves the ask side empty (a missing
    // level, not a repair).
    let outs = e.on_frame(
        0,
        2_100,
        &[
            delta("e1-y0", 2_000, Side::Ask, "0.30", "10", None, Some("0.31")),
            delta("e1-y0", 2_000, Side::Ask, "0.40", "0", None, Some("0.31")),
        ],
    );
    assert_eq!(episodes(&outs).len(), 1);
    // resolved by a frame that touches ONLY e2, on the same conn/shard: without the review-round-1
    // fix, e1's group would never be re-evaluated here (it isn't in this frame's `touched` set)
    // and the fake basket would linger until e1's own next frame.
    let outs = e.on_frame(0, 3_200, &[ask("e2-y0", 3_001, "0.99", "5")]);
    let closed = episodes(&outs)
        .into_iter()
        .rev()
        .find(|r| r.episode_id == "live-7-00000001")
        .expect("e1's episode closes even though this frame never touches group e1");
    // stamped with the mismatch's own ts (2_000, == group_last_ts at the time), not the resolving
    // frame's ts (3_001): the close time never depends on unrelated traffic sharing the frame.
    assert_eq!(closed.closed_server_ms, Some(2_000));
    assert!(closed.engine_suspect);
}

#[test]
fn close_all_flushes_suppressed_throttle_counts_in_sorted_order_then_resets_them() {
    let mut e = engine();
    seed(&mut e); // e1-y1 ask 0.35, e1-y2 ask 0.30
    // for each token: 3 resolutions inside the 5_000 ms throttle window — the first emits (n=0),
    // the second is suppressed and never gets its own row. 0.01 is far below anything we hold, so
    // it's always a missing level: no trim, no interference with the count itself.
    for token in ["e1-y2", "e1-y1"] {
        for k in 0..3_i64 {
            let t = 2_000 + k * 1_100;
            e.on_frame(
                0,
                t + 100,
                &[MarketEvent::Delta {
                    token: token.into(),
                    server_ts_ms: t,
                    side: Side::Ask,
                    price: p("0.50"),
                    size: sh("5"),
                    best_bid: None,
                    best_ask: Some(p("0.01")),
                }],
            );
        }
    }
    let outs = e.close_all(9_999);
    let rows: Vec<_> = outs
        .iter()
        .filter_map(|o| match o {
            Out::Integrity(i) if i.kind == "top_mismatch" => Some(i.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "a suppressed count is a measurement, never dropped: {rows:?}"
    );
    // sorted by (kind, token): deterministic regardless of the throttle map's iteration order
    assert_eq!(rows[0].token.as_deref(), Some("e1-y1"), "{rows:?}");
    assert_eq!(rows[1].token.as_deref(), Some("e1-y2"), "{rows:?}");
    for r in &rows {
        assert_eq!(r.detail.as_deref(), Some("+1 suppressed (at close)"));
        assert_eq!(r.ts_ms, 9_999);
        assert_eq!(r.shard, None);
    }
    // flushed exactly once: reset, so a second close_all has nothing left to report
    assert!(
        !e.close_all(20_000)
            .iter()
            .any(|o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch"))
    );
}

#[test]
fn a_repair_that_uncrosses_the_book_opens_with_the_resolving_frames_recv_ms() {
    let mut e = engine();
    seed(&mut e); // e1: ΣA = 1.05, unrelated to this test
    // a second, independent group with only 2 legs, so its own ΣA is easy to reason about.
    let u = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![meta("e1", 3), meta("e2", 2)],
        canary: vec![],
    });
    let conn: HashMap<String, usize> = [("e1".to_string(), 0), ("e2".to_string(), 0)]
        .into_iter()
        .collect();
    assert!(e.apply_universe(u, &conn, 0).is_empty());
    e.on_frame(
        0,
        1_100,
        &[
            snap("e2-y0", 1_000, &[], &[("0.40", "10")]),
            snap("e2-y1", 1_000, &[], &[("0.65", "10")]),
        ],
    ); // ΣA = 1.05, no basket
    // ts 2_000: e2-y0 gets a real bid at 0.10, then a phantom bid at 0.50 that crosses its own
    // 0.40 ask; the exchange's own final top for this burst is bid 0.10 / ask 0.40 (server-final,
    // per every item), so 0.50 is a level we must not have. e2-y1's ask also drops to 0.45 in the
    // same frame: ΣA = 0.40 + 0.45 = 0.85 < 1, a real opportunity — but e2-y0 is crossed right
    // now, so `detect_yes_basket` (D1) refuses to open anything.
    let outs = e.on_frame(
        0,
        2_100,
        &[
            delta("e2-y0", 2_000, Side::Bid, "0.10", "10", None, None),
            delta(
                "e2-y0",
                2_000,
                Side::Bid,
                "0.50",
                "10",
                Some("0.10"),
                Some("0.40"),
            ),
            ask("e2-y1", 2_000, "0.45", "10"),
            ask("e2-y1", 2_000, "0.65", "0"),
        ],
    );
    assert!(
        episodes(&outs).is_empty(),
        "e2-y0 is crossed: nothing opens yet, {outs:?}"
    );
    // > 1 s later, an UNRELATED e1 frame resolves e2-y0's deferred check (same conn): our top
    // (bid 0.50 / ask 0.40) disagrees with the server's (bid 0.10 / ask 0.40); trimming the
    // phantom 0.50 leaves bid 0.10 / ask 0.40, which matches the server exactly — repaired — and
    // uncrosses the book. The group is re-evaluated right there, opening the basket the phantom
    // had been hiding. `recv_ms` here (9_999) is THIS resolving frame's own receipt time, wholly
    // unrelated to the mismatch's ts (2_000, still == group_last_ts): a fabricated recv_ms == ts
    // would give a false open_lag_ms of 0 / stale = false (S3a final review item 2).
    let outs = e.on_frame(0, 9_999, &[ask("e1-y2", 9_500, "0.29", "1")]);
    assert!(
        outs.iter().any(
            |o| matches!(o, Out::Integrity(i) if i.kind == "top_mismatch" && i.token.as_deref() == Some("e2-y0")
                && i.detail.as_deref().unwrap_or("").contains("repaired from stream"))
        ),
        "{outs:?}"
    );
    assert!(
        !outs
            .iter()
            .any(|o| matches!(o, Out::Resnapshot { token } if token == "e2-y0")),
        "repaired, so no resnapshot: {outs:?}"
    );
    let rows = episodes(&outs);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let opened = &rows[0];
    assert_eq!(opened.opened_server_ms, 2_000, "{opened:?}");
    assert_eq!(opened.closed_server_ms, None, "{opened:?}");
    assert_eq!(
        opened.open_lag_ms,
        Some(9_999 - 2_000),
        "open_lag_ms must come from the resolving frame's own recv_ms, not its ts: {opened:?}"
    );
    assert!(opened.stale_transport, "{opened:?}");
}

#[test]
fn the_threshold_gate_skips_detectors_until_the_top_sums_cross() {
    let mut e = engine();
    seed(&mut e); // ΣA = 1.05 and no bids: both gates shut
    assert_eq!(e.detector_calls(), 0);
    // ΣA = 0.95: the YES gate opens (one call). There are still no bids, so D2b is skipped.
    let outs = e.on_frame(
        0,
        2_150,
        &[
            ask("e1-y0", 2_000, "0.30", "10"),
            ask("e1-y0", 2_000, "0.40", "0"),
        ],
    );
    assert_eq!(episodes(&outs).len(), 1);
    assert_eq!(e.detector_calls(), 1);
    // ΣA = 1.00: the gate is shut, but the open YES episode still runs D2a, which closes it at
    // this frame's own ts.
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
    assert_eq!(e.detector_calls(), 2);
    // ΣA = 1.01 with nothing open: fully gated. Only the tracker sees this state.
    let outs = e.on_frame(
        0,
        4_600,
        &[
            ask("e1-y2", 4_500, "0.31", "10"),
            ask("e1-y2", 4_500, "0.30", "0"),
        ],
    );
    assert!(episodes(&outs).is_empty());
    assert_eq!(e.detector_calls(), 2);
}

#[test]
fn the_threshold_gate_never_changes_a_single_output() {
    let mut s = Synth::new(200, 42);
    let frames = s.script(20_000);
    let run = |gate: bool| {
        let mut e = synth_engine(&s, gate);
        let mut outs: Vec<Out> = Vec::new();
        drive(&mut e, &frames, 4_000, &mut |o| outs.push(o));
        (outs, e.detector_calls())
    };
    let (on, calls_on) = run(true);
    let (off, calls_off) = run(false);
    // Debug prints Decimal with its scale, so equal strings mean equal bytes, not only equal values.
    let on_txt: Vec<String> = on.iter().map(|o| format!("{o:?}")).collect();
    let off_txt: Vec<String> = off.iter().map(|o| format!("{o:?}")).collect();
    assert_eq!(on_txt.len(), off_txt.len(), "output counts differ");
    for (i, (a, b)) in on_txt.iter().zip(&off_txt).enumerate() {
        assert_eq!(a, b, "output {i} differs with the gate on");
    }
    // The fixture must exercise what it claims to: both basket kinds open, feed drops
    // right-censor, and the gate really skips work.
    let rows = episodes(&on);
    assert!(
        rows.iter().any(|r| r.kind == "yes_basket"),
        "no YES basket opened"
    );
    assert!(
        rows.iter().any(|r| r.kind == "no_basket"),
        "no NO basket opened"
    );
    assert!(
        rows.iter().any(|r| r.censor_right),
        "no right-censored close"
    );
    assert!(
        calls_on < calls_off / 2,
        "gate skipped too little: {calls_on} calls vs {calls_off}"
    );
}

// ---- S3a-2 Task 4: pending top-check indexes stay consistent with `pending_tops` ----

#[test]
fn pending_top_indexes_stay_consistent_through_every_mutation() {
    use arb_core::money::Price;
    use scanner::engine::Engine;
    use scanner::universe::CanaryPair;
    // Deterministic LCG (MMIX constants), as in common::Lcg, local so the seed is visible.
    let mut state: u64 = 0x5eed_0004;
    let mut below = |n: usize| -> usize {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap() % n
    };
    let canary = vec![CanaryPair {
        market_id: "cm".into(),
        event_id: "ce".into(),
        yes_token: "c-y".into(),
        no_token: "c-n".into(),
    }];
    // u1: a (3 legs) on conn 0, b (2 legs) on conn 1, plus a canary pair. u2 reorders the groups
    // (group indices change), drops a's third leg and the canary, and adds d on conn 2.
    let u1 = Arc::new(Universe {
        taken_ms: 1,
        groups: vec![meta("a", 3), meta("b", 2)],
        canary: canary.clone(),
    });
    let mut a2 = meta("a", 3);
    a2.legs.truncate(2);
    let u2 = Arc::new(Universe {
        taken_ms: 2,
        groups: vec![meta("d", 2), meta("b", 2), a2],
        canary: vec![],
    });
    let conn_of: HashMap<String, usize> = [("a", 0), ("b", 1), ("d", 2)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let tokens = [
        "a-y0", "a-y1", "a-y2", "b-y0", "b-y1", "d-y0", "d-y1", "c-y", "c-n", "zz",
    ];
    let mut e = Engine::new(ctx());
    let _ = e.apply_universe(u1.clone(), &conn_of, 0);
    let mut ts: i64 = 1_000;
    let mut on_u1 = true;
    let mut max_pending = 0;
    let mut kinds_seen = [0usize; 6];
    for step in 0..20_000 {
        let tok = tokens[below(tokens.len())];
        let roll = below(100);
        // ts advances 0..=1 500 ms: same-ts replaces, earlier-ts resolves and stale sweeps all occur
        ts += i64::try_from(below(4)).unwrap() * 500;
        let kind = match roll {
            0..=69 => {
                let conn = below(3);
                let c = 10 + i64::try_from(below(80)).unwrap();
                let top = if below(2) == 0 {
                    Some(Price::new(rust_decimal::Decimal::new(c, 2)).unwrap())
                } else {
                    None
                };
                let size = if below(4) == 0 { "0" } else { "10" };
                let ev = MarketEvent::Delta {
                    token: tok.into(),
                    server_ts_ms: ts,
                    side: Side::Ask,
                    price: Price::new(rust_decimal::Decimal::new(c, 2)).unwrap(),
                    size: sh(size),
                    best_bid: None,
                    best_ask: top,
                };
                let _ = e.on_frame(conn, ts + 100, &[ev]);
                0
            }
            70..=79 => {
                let _ = e.on_frame(
                    below(3),
                    ts + 100,
                    &[snap(tok, ts, &[("0.10", "5")], &[("0.90", "5")])],
                );
                1
            }
            80..=89 => {
                let mut b = Book::default();
                let bts = ts - i64::try_from(below(3)).unwrap() * 700;
                b.apply_snapshot(&[(p("0.10"), sh("5"))], &[(p("0.90"), sh("5"))], bts);
                let _ = e.rest_snapshot(tok, b);
                2
            }
            90..=95 => {
                let _ = e.disconnect(below(3), ts);
                3
            }
            96..=98 => {
                let u = if on_u1 { u2.clone() } else { u1.clone() };
                on_u1 = !on_u1;
                let _ = e.apply_universe(u, &conn_of, ts);
                4
            }
            _ => {
                let _ = e.close_all(ts);
                5
            }
        };
        kinds_seen[kind] += 1;
        match e.debug_check_pending_indexes() {
            Ok(n) => max_pending = max_pending.max(n),
            Err(m) => panic!("step {step} (op {kind}): {m}"),
        }
    }
    assert!(
        max_pending >= 4,
        "fixture kept too few pending checks: {max_pending}"
    );
    assert!(kinds_seen.iter().all(|&n| n > 0), "{kinds_seen:?}");
}
