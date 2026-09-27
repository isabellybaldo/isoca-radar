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
        issue_throttle_ms: 5_000,
    }
}

pub fn meta(event: &str, n: usize) -> GroupMeta {
    GroupMeta {
        event_id: event.into(),
        slug: None,
        title: None,
        neg_risk_market_id: None,
        augmented: false,
        tags: None,
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

// ---- S3a-2: deterministic synthetic high-rate script (equivalence test + bench) ----

/// Deterministic 64-bit LCG (MMIX constants). Fixtures only: no rand crate, no clock.
pub struct Lcg(u64);

impl Lcg {
    /// Uniform-enough in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % n
    }
}

fn cents(c: i64) -> Price {
    Price::new(Decimal::new(c, 2)).unwrap()
}

/// Levels in 1..=99 cents, 100 shares each.
fn ladder(cs: impl Iterator<Item = i64>) -> Vec<(Price, Shares)> {
    cs.filter(|c| (1..=99).contains(c))
        .map(|c| (cents(c), Shares::new(dec!(100)).unwrap()))
        .collect()
}

fn level(
    token: &str,
    ts: i64,
    side: Side,
    c: i64,
    size: Shares,
    best_bid: Option<Price>,
    best_ask: Option<Price>,
) -> MarketEvent {
    MarketEvent::Delta {
        token: token.into(),
        server_ts_ms: ts,
        side,
        price: cents(c),
        size,
        best_bid,
        best_ask,
    }
}

struct SynthLeg {
    token: String,
    a0: i64,
    b0: i64,
    ask: i64,
    bid: i64,
}

pub struct SynthFrame {
    pub conn: usize,
    pub recv_ms: i64,
    pub events: Vec<MarketEvent>,
}

pub struct Synth {
    pub universe: Universe,
    pub conn_of: HashMap<String, usize>,
    legs: Vec<Vec<SynthLeg>>,
    rng: Lcg,
    ts: i64,
}

impl Synth {
    /// `groups` events. Event g has 2 + g % 11 legs (2..=12), and every third event has a 2 %
    /// taker fee. Even events hover around ΣA = 1 (YES baskets open now and then), odd ones
    /// around ΣB = 1 (NO baskets). Each token has a static ladder (≤ 15 levels a side) that never
    /// moves, plus one dynamic top level per side. The dynamic ask stays in [a0 − 2, a0 + 1] and
    /// the dynamic bid in [b0 − 1, b0 + 2], cents. These ranges don't overlap the static levels
    /// (asks ≥ a0 + 2, bids ≤ b0 − 2) or each other (b0 + 2 < a0 − 2), so no book is ever crossed
    /// and, with no server top on the deltas, no top check ever fires.
    pub fn new(groups: usize, seed: u64) -> Synth {
        let mut metas = Vec::with_capacity(groups);
        let mut legs = Vec::with_capacity(groups);
        for g in 0..groups {
            let n = 2 + g % 11;
            let event = format!("s{g}");
            let mut m = meta(&event, n);
            if g % 3 == 0 {
                for l in &mut m.legs {
                    l.fee_rate = dec!(0.02);
                }
            }
            let n_i = i64::try_from(n).unwrap();
            let yes_side = g % 2 == 0;
            let total = if yes_side { 100 + n_i } else { 100 - n_i }; // Σ a0 or Σ b0, cents
            let ls = (0..n_i)
                .map(|i| {
                    let share = total / n_i + i64::from(i < total % n_i);
                    let (a0, b0) = if yes_side {
                        (share, share - 5)
                    } else {
                        (share + 5, share)
                    };
                    SynthLeg {
                        token: format!("{event}-y{i}"),
                        a0,
                        b0,
                        ask: a0,
                        bid: b0,
                    }
                })
                .collect();
            metas.push(m);
            legs.push(ls);
        }
        let conn_of = metas
            .iter()
            .enumerate()
            .map(|(g, m)| (m.event_id.clone(), g % 40))
            .collect();
        Synth {
            universe: Universe {
                taken_ms: 0,
                groups: metas,
                canary: vec![],
            },
            conn_of,
            legs,
            rng: Lcg(seed),
            ts: 1_000,
        }
    }

    /// One snapshot frame per event at ts 1 000, then `n` delta frames. Each delta frame moves
    /// 1–3 legs of one event (top ask and top bid: add the new level, remove the old one), all
    /// at one server ts. The ts advances 1–3 ms per frame; recv = ts + 150 (under stale_lag_ms).
    ///
    /// `wrong_top_every`: 0 = never; otherwise about 1 in that many deltas carry a deliberately
    /// WRONG server `best_bid`/`best_ask` (offset from the leg's true new top), so the
    /// top-mismatch path (trim, repair, throttle, suspect, Resnapshot) runs too. Bench passes 0
    /// to keep its script's own invariant that no top check ever fires.
    pub fn script(&mut self, n: usize, wrong_top_every: usize) -> Vec<SynthFrame> {
        let mut out: Vec<SynthFrame> = Vec::with_capacity(self.legs.len() + n);
        for (g, ls) in self.legs.iter().enumerate() {
            let events = ls
                .iter()
                .map(|l| MarketEvent::Snapshot {
                    token: l.token.clone(),
                    server_ts_ms: self.ts,
                    bids: ladder(std::iter::once(l.bid).chain((0..15).map(|k| l.b0 - 2 - k))),
                    asks: ladder(std::iter::once(l.ask).chain((0..15).map(|k| l.a0 + 2 + k))),
                    hash: None,
                })
                .collect();
            out.push(SynthFrame {
                conn: g % 40,
                recv_ms: self.ts + 150,
                events,
            });
        }
        for _ in 0..n {
            out.push(self.next_frame(wrong_top_every));
        }
        out
    }

    fn next_frame(&mut self, wrong_top_every: usize) -> SynthFrame {
        self.ts += 1 + i64::try_from(self.rng.below(3)).unwrap();
        let ts = self.ts;
        let g = self.rng.below(self.legs.len());
        let moves = 1 + self.rng.below(3);
        let mut events = Vec::new();
        for _ in 0..moves {
            let l = self.rng.below(self.legs[g].len());
            let da = i64::try_from(self.rng.below(4)).unwrap();
            let db = i64::try_from(self.rng.below(4)).unwrap();
            let size = Shares::new(Decimal::from(1 + self.rng.below(200))).unwrap();
            let gone = Shares::new(Decimal::ZERO).unwrap();
            let leg = &mut self.legs[g][l];
            let (na, nb) = (leg.a0 - 2 + da, leg.b0 - 1 + db);
            // About 1 in `wrong_top_every` deltas (0 = never) report a server top that is
            // deliberately WRONG (offset from the leg's true new top on both sides), so the
            // top-mismatch resolution path runs for both the on and off gate engines.
            let wrong = wrong_top_every > 0 && self.rng.below(wrong_top_every) == 0;
            let off = 1 + i64::try_from(self.rng.below(3)).unwrap();
            // Clamped to the script's own 1..=99 cent range (see `ladder`), so the wrong top is
            // always a valid `Price`.
            let (bb, ba) = if wrong {
                (
                    Some(cents((nb - off).clamp(1, 99))),
                    Some(cents((na + off).clamp(1, 99))),
                )
            } else {
                (None, None)
            };
            for (side, old, new) in [(Side::Ask, leg.ask, na), (Side::Bid, leg.bid, nb)] {
                events.push(level(&leg.token, ts, side, new, size, bb, ba));
                if old != new {
                    events.push(level(&leg.token, ts, side, old, gone, bb, ba));
                }
            }
            leg.ask = na;
            leg.bid = nb;
        }
        SynthFrame {
            conn: g % 40,
            recv_ms: ts + 150,
            events,
        }
    }
}

/// A fresh engine (common `ctx()`) on the synthetic universe, with the gate on or off.
pub fn synth_engine(s: &Synth, gate: bool) -> Engine {
    let mut e = Engine::new(ctx());
    e.set_threshold_gate(gate);
    let _ = e.apply_universe(Arc::new(s.universe.clone()), &s.conn_of, 0);
    e
}

/// Feed `frames` to `e` the way replay does (`on_frame`, then `alive`). Every `drop_every`
/// frames (0 = never) conn 0's feed drops (`disconnect`), and `close_all` runs after the last
/// frame. Every output goes to `sink`, in order.
pub fn drive(e: &mut Engine, frames: &[SynthFrame], drop_every: usize, sink: &mut dyn FnMut(Out)) {
    let mut last = 0;
    for (i, f) in frames.iter().enumerate() {
        for o in e.on_frame(f.conn, f.recv_ms, &f.events) {
            sink(o);
        }
        e.alive(f.conn, f.recv_ms);
        if drop_every > 0 && i % drop_every == drop_every - 1 {
            for o in e.disconnect(0, f.recv_ms) {
                sink(o);
            }
        }
        last = f.recv_ms;
    }
    for o in e.close_all(last) {
        sink(o);
    }
}
