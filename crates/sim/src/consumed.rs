//! Consumed liquidity (spec §5.6). Our fills aren't real, so the orders we "took" are still in the
//! feed. Each account keeps what it consumed per YES-book level; the amount only clears once the
//! feed shows that level shrink by at least that amount (someone else plausibly took it) or vanish.
//! Otherwise every flicker of the same episode could "fill" the same resting orders again.
use crate::types::{BookSide, vecmap};
use arb_core::book::Book;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LevelKey {
    pub yes_token: String,
    pub side: BookSide,
    pub price: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consumed {
    pub amount: Decimal,
    /// The feed's aggregate size at the level when we last consumed from it.
    pub ref_size: Decimal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumedLiquidity {
    #[serde(with = "vecmap")]
    levels: BTreeMap<LevelKey, Consumed>,
}

impl ConsumedLiquidity {
    pub fn available(&self, key: &LevelKey, feed_size: Decimal) -> Decimal {
        let used = self.levels.get(key).map_or(Decimal::ZERO, |c| c.amount);
        (feed_size - used).max(Decimal::ZERO)
    }

    pub fn consume(&mut self, key: LevelKey, feed_size: Decimal, qty: Decimal) {
        if qty <= Decimal::ZERO {
            return;
        }
        let e = self.levels.entry(key).or_insert(Consumed {
            amount: Decimal::ZERO,
            ref_size: feed_size,
        });
        e.amount += qty;
        e.ref_size = feed_size;
    }

    /// The feed's new aggregate size at one level (0 = the level was removed).
    pub fn on_level(&mut self, key: &LevelKey, new_size: Decimal) {
        let clear = self
            .levels
            .get(key)
            .is_some_and(|c| new_size <= Decimal::ZERO || new_size <= c.ref_size - c.amount);
        if clear {
            self.levels.remove(key);
        }
    }

    /// A full book for `yes_token` arrived: re-check every consumed level of that token.
    pub fn on_snapshot(&mut self, yes_token: &str, book: &Book) {
        let keys: Vec<LevelKey> = self
            .levels
            .keys()
            .filter(|k| k.yes_token == yes_token)
            .cloned()
            .collect();
        if keys.is_empty() {
            return; // nothing consumed on this token: don't copy its book
        }
        let side_map = |v: Vec<(arb_core::money::Price, arb_core::money::Shares)>| -> BTreeMap<Decimal, Decimal> {
            v.into_iter().map(|(p, s)| (p.get(), s.get())).collect()
        };
        let bids = side_map(book.bids_best_first());
        let asks = side_map(book.asks_best_first());
        for k in keys {
            let side = match k.side {
                BookSide::Bid => &bids,
                BookSide::Ask => &asks,
            };
            let size = side.get(&k.price).copied().unwrap_or(Decimal::ZERO);
            self.on_level(&k, size);
        }
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
}
