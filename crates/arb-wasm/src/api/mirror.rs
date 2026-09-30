//! `mirror_json`: one YES book reflected (L1), its D1 sums and the canary (09-27 §4.1).
use super::{LevelOut, lv, run};
use crate::input::{LevelIn, d, levels};
use arb_core::book::{Book, d1_sums};
use arb_core::detect::canary;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MirrorIn {
    bids: Vec<LevelIn>,
    asks: Vec<LevelIn>,
}

#[derive(Serialize)]
struct MirrorOut {
    no_asks: Vec<LevelOut>,
    no_bids: Vec<LevelOut>,
    best_bid: Option<LevelOut>,
    best_ask: Option<LevelOut>,
    spread: Option<String>,
    d1_long: Option<String>,
    d1_short: Option<String>,
    crossed: bool,
}

/// One YES book reflected (L1) with its D1 sums and the canary.
pub fn mirror_json(input: &str) -> String {
    run(input, |i: MirrorIn| {
        let mut b = Book::default();
        b.apply_snapshot(&levels(&i.bids)?, &levels(&i.asks)?, 0);
        let top = b.top();
        let sums = d1_sums(&top);
        Ok(MirrorOut {
            no_asks: b.no_asks_best_first().into_iter().map(lv).collect(),
            no_bids: b.no_bids_best_first().into_iter().map(lv).collect(),
            best_bid: top.best_bid.map(lv),
            best_ask: top.best_ask.map(lv),
            spread: match (top.best_bid, top.best_ask) {
                (Some((bp, _)), Some((ap, _))) => Some(d(ap.get() - bp.get())),
                _ => None,
            },
            d1_long: sums.map(|s| d(s.0)),
            d1_short: sums.map(|s| d(s.1)),
            crossed: canary(&b),
        })
    })
}
