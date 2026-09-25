#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use pm_api::ws::MarketEvent;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::engine::{Engine, EngineCtx, Out};
use scanner::universe::{GroupMeta, LegMeta, Status, Universe};
use std::collections::HashMap;
use std::sync::Arc;
use store::EpisodeRow;

pub fn ctx() -> EngineCtx {
    EngineCtx {
        run_id: 7,
        source: "live".into(),
        scanner_version: "test".into(),
        config_hash: "cfg".into(),
        gap_ms: 30_000,
        stale_lag_ms: 500,
        suspect_window_ms: 60_000,
        l1_suspect_window_ms: 3_600_000,
        drift_settle_ms: 2_000,
    }
}

pub fn meta(event: &str, n: usize) -> GroupMeta {
    GroupMeta {
        event_id: event.into(),
        slug: None,
        title: None,
        neg_risk_market_id: None,
        augmented: false,
        legs: (0..n)
            .map(|i| LegMeta {
                market_id: format!("{event}-m{i}"),
                yes_token: format!("{event}-y{i}"),
                no_token: format!("{event}-n{i}"),
                status: Status::Live,
                fee_rate: Decimal::ZERO,
                fee_exponent: 1,
                tick: Some(dec!(0.01)),
                min_size: Some(dec!(5)),
                seconds_delay: Some(0),
                end_date: None,
            })
            .collect(),
    }
}

pub fn engine() -> Engine {
    let mut e = Engine::new(ctx());
    let u = Arc::new(Universe {
        taken_ms: 0,
        groups: vec![meta("e1", 3)],
        canary: vec![],
    });
    let conn: HashMap<String, usize> = [("e1".to_string(), 0)].into_iter().collect();
    let _ = e.apply_universe(u, &conn, 0);
    e
}

pub fn snap(token: &str, ts: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> MarketEvent {
    MarketEvent::Snapshot {
        token: token.into(),
        server_ts_ms: ts,
        bids: bids.iter().map(|(a, b)| (p(a), sh(b))).collect(),
        asks: asks.iter().map(|(a, b)| (p(a), sh(b))).collect(),
        hash: None,
    }
}

pub fn ask(token: &str, ts: i64, price: &str, size: &str) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side: Side::Ask,
        price: p(price),
        size: sh(size),
        best_bid: None,
        best_ask: None,
    }
}

pub fn episodes(outs: &[Out]) -> Vec<EpisodeRow> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Episode(w) => Some(w.row.clone()),
            _ => None,
        })
        .collect()
}

/// ΣA = 0.40 + 0.35 + 0.30 = 1.05: no basket.
pub fn seed(e: &mut Engine) -> Vec<Out> {
    e.on_frame(
        0,
        1_100,
        &[
            snap("e1-y0", 1_000, &[], &[("0.40", "10")]),
            snap("e1-y1", 1_000, &[], &[("0.35", "10")]),
            snap("e1-y2", 1_000, &[], &[("0.30", "10")]),
        ],
    )
}

pub fn p(s: &str) -> Price {
    Price::parse(s).unwrap()
}

pub fn sh(s: &str) -> Shares {
    Shares::parse(s).unwrap()
}
