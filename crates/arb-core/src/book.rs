//! L2 book for one YES token, with the NO side derived by the mirror (L1).
use crate::money::{Price, Shares};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Top {
    pub best_bid: Option<(Price, Shares)>,
    pub best_ask: Option<(Price, Shares)>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    bids: BTreeMap<Price, Shares>,
    asks: BTreeMap<Price, Shares>,
    server_ts_ms: i64,
}

impl Book {
    pub fn apply_snapshot(
        &mut self,
        bids: &[(Price, Shares)],
        asks: &[(Price, Shares)],
        server_ts_ms: i64,
    ) {
        self.bids.clear();
        self.asks.clear();
        for &(p, s) in bids {
            if s.get() > Decimal::ZERO {
                self.bids.insert(p, s);
            }
        }
        for &(p, s) in asks {
            if s.get() > Decimal::ZERO {
                self.asks.insert(p, s);
            }
        }
        self.server_ts_ms = server_ts_ms;
    }

    /// `new_size` is the new aggregate size at `price` (0 removes the level). Returns true if the top changed.
    pub fn apply_delta(
        &mut self,
        side: Side,
        price: Price,
        new_size: Shares,
        server_ts_ms: i64,
    ) -> bool {
        let before = self.top();
        let map = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        if new_size.get() == Decimal::ZERO {
            map.remove(&price);
        } else {
            map.insert(price, new_size);
        }
        self.server_ts_ms = server_ts_ms;
        self.top() != before
    }

    pub fn top(&self) -> Top {
        Top {
            best_bid: self.bids.iter().next_back().map(|(p, s)| (*p, *s)),
            best_ask: self.asks.iter().next().map(|(p, s)| (*p, *s)),
        }
    }

    pub fn server_ts_ms(&self) -> i64 {
        self.server_ts_ms
    }

    /// L2: a single condition can only show arbitrage if the book is crossed (bid > ask).
    pub fn is_crossed(&self) -> bool {
        match (self.top().best_bid, self.top().best_ask) {
            (Some((b, _)), Some((a, _))) => b > a,
            _ => false,
        }
    }

    pub fn asks_best_first(&self) -> Vec<(Price, Shares)> {
        self.asks.iter().map(|(p, s)| (*p, *s)).collect()
    }

    pub fn bids_best_first(&self) -> Vec<(Price, Shares)> {
        self.bids.iter().rev().map(|(p, s)| (*p, *s)).collect()
    }

    /// L1: NO asks = 1 − YES bids, same sizes, best (lowest) first.
    pub fn no_asks_best_first(&self) -> Vec<(Price, Shares)> {
        self.bids_best_first()
            .into_iter()
            .map(|(p, s)| (p.complement(), s))
            .collect()
    }

    /// L1: NO bids = 1 − YES asks, same sizes, best (highest) first.
    pub fn no_bids_best_first(&self) -> Vec<(Price, Shares)> {
        self.asks_best_first()
            .into_iter()
            .map(|(p, s)| (p.complement(), s))
            .collect()
    }

    /// Drop levels the server's authoritative top proves are gone: bids strictly above
    /// `best_bid`, asks strictly below `best_ask` (a removal we never applied). `None` leaves that
    /// side alone: an empty or unreported side is not proof. Returns true if anything was removed.
    pub fn trim_to_top(&mut self, best_bid: Option<Price>, best_ask: Option<Price>) -> bool {
        let before = self.bids.len() + self.asks.len();
        if let Some(b) = best_bid {
            self.bids.retain(|p, _| *p <= b);
        }
        if let Some(a) = best_ask {
            self.asks.retain(|p, _| *p >= a);
        }
        self.bids.len() + self.asks.len() != before
    }
}

/// D1 sums (parent §5.7 (a), L2): long = A + (1 − B), short = B + (1 − A), from the best bid B and
/// best ask A; `None` without both sides. The book is crossed (B > A) exactly when long < 1 and
/// short > 1.
pub fn d1_sums(top: &Top) -> Option<(Decimal, Decimal)> {
    let (b, _) = top.best_bid?;
    let (a, _) = top.best_ask?;
    let (a, b) = (a.get(), b.get());
    Some((a + (Decimal::ONE - b), b + (Decimal::ONE - a)))
}
