//! `episode_json`: the teaching model of one episode and one taker at t_x (09-27 §4.1).
use super::run;
use crate::input::{LegIn, d, group};
use arb_core::classify::classify;
use arb_core::detect::Kind;
use arb_core::episode::{EpisodeEvent, EpisodeTracker};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioIn {
    proc_ms: i64,
    net_ms: i64,
    match_ms: i64,
    honor_seconds_delay: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateIn {
    t_ms: i64,
    legs: Vec<LegIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeIn {
    kind: String,
    #[serde(default = "default_gap_ms")]
    gap_ms: i64,
    updates: Vec<UpdateIn>,
    scenario: ScenarioIn,
    seconds_delay: Option<i32>,
}

/// `config/scanner.toml`'s `gap_ms`.
fn default_gap_ms() -> i64 {
    30_000
}

#[derive(Serialize)]
struct UpdateOut {
    t_ms: i64,
    net: Option<String>,
}

#[derive(Serialize)]
struct EpisodeOut {
    updates: Vec<UpdateOut>,
    open_ms: Option<i64>,
    close_ms: Option<i64>,
    peak_ms: Option<i64>,
    peak_net: Option<String>,
    t_x: Option<i64>,
    captured: Option<String>,
    caught: bool,
}

/// A teaching model (09-27 §4.1): one taker at t_x gets the net Π of the latest update at or
/// before t_x, or 0 once the episode has closed.
pub fn episode_json(input: &str) -> String {
    run(input, |i: EpisodeIn| {
        let want = match i.kind.as_str() {
            "yes" => Kind::YesBasket,
            "no" => Kind::NoBasket,
            k => return Err(format!("unknown kind {k:?}")),
        };
        if !(1..=600_000).contains(&i.gap_ms) {
            return Err("gap_ms must be 1 to 600,000".to_string());
        }
        if !(2..=8).contains(&i.updates.len()) {
            return Err("2 to 8 updates".to_string());
        }
        if i.updates.iter().any(|u| !(0..=600_000).contains(&u.t_ms)) {
            return Err("update times must be 0 to 600,000 ms".to_string());
        }
        if i.updates.windows(2).any(|w| w[1].t_ms <= w[0].t_ms) {
            return Err("update times must strictly increase".to_string());
        }
        let sc = &i.scenario;
        for (n, v) in [
            ("proc_ms", sc.proc_ms),
            ("net_ms", sc.net_ms),
            ("match_ms", sc.match_ms),
        ] {
            if !(0..=5000).contains(&v) {
                return Err(format!("{n} must be 0 to 5,000"));
            }
        }
        if i.seconds_delay.is_some_and(|s| !(0..=10).contains(&s)) {
            return Err("seconds_delay must be 0 to 10".to_string());
        }
        let scenario = sim::config::Scenario {
            name: "playground".to_string(),
            proc_ms: sc.proc_ms,
            net_ms: sc.net_ms,
            match_ms: sc.match_ms,
            honor_seconds_delay: sc.honor_seconds_delay,
            burst: Vec::new(),
        };
        let mut tracker = EpisodeTracker::new(i.gap_ms);
        let mut nets: Vec<Option<Decimal>> = Vec::with_capacity(i.updates.len());
        let (mut open_ms, mut close_ms, mut peak_ms, mut peak_net) = (None, None, None, None);
        for u in &i.updates {
            let c = classify(&group(&u.legs, false)?);
            let net = c.signal.filter(|s| s.kind == want).map(|s| s.quote.pnl);
            nets.push(net);
            for ev in tracker.on_state("e", u.t_ms, net) {
                match ev {
                    EpisodeEvent::Open { ts_ms, pnl, .. } if open_ms.is_none() => {
                        open_ms = Some(ts_ms);
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Peak { ts_ms, pnl, .. } if close_ms.is_none() => {
                        peak_ms = Some(ts_ms);
                        peak_net = Some(pnl);
                    }
                    EpisodeEvent::Close { ts_ms, .. } if close_ms.is_none() => {
                        close_ms = Some(ts_ms)
                    }
                    _ => {}
                }
            }
        }
        let t_x = open_ms.map(|o| o + sim::latency::exec_delay_ms(&scenario, i.seconds_delay, 0));
        // The one decision here (09-27 §4.1): the latest update at or before t_x; 0 once closed.
        let captured = t_x.map(|t| {
            if close_ms.is_some_and(|c| c <= t) {
                return Decimal::ZERO;
            }
            i.updates
                .iter()
                .zip(&nets)
                .rev()
                .find(|(u, _)| u.t_ms <= t)
                .and_then(|(_, n)| *n)
                .unwrap_or(Decimal::ZERO)
        });
        Ok(EpisodeOut {
            updates: i
                .updates
                .iter()
                .zip(&nets)
                .map(|(u, n)| UpdateOut {
                    t_ms: u.t_ms,
                    net: n.map(d),
                })
                .collect(),
            open_ms,
            close_ms,
            peak_ms,
            peak_net: peak_net.map(d),
            t_x,
            caught: captured.is_some_and(|c| c > Decimal::ZERO),
            captured: captured.map(d),
        })
    })
}
