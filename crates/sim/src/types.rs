//! Identifiers shared by the sim modules.
use arb_core::book::Side;
use serde::{Deserialize, Serialize};

/// The token a position or order is on. We subscribe YES books only; NO is the mirror (L1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Yes,
    No,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Yes => "yes",
            Outcome::No => "no",
        }
    }
}

/// One tradeable token: a market's YES or NO. Books are looked up by `yes_token`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LegKey {
    pub market_id: String,
    pub yes_token: String,
    pub outcome: Outcome,
}

/// A side of the YES book. Consumed liquidity is tracked in YES-book coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookSide {
    Bid,
    Ask,
}

impl BookSide {
    pub fn from_core(s: Side) -> Self {
        match s {
            Side::Bid => BookSide::Bid,
            Side::Ask => BookSide::Ask,
        }
    }
}

/// Serde helper: a `BTreeMap` as a list of pairs (JSON object keys must be strings; ours are structs).
pub mod vecmap {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer, K: Serialize, V: Serialize>(
        m: &BTreeMap<K, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter())
    }

    pub fn deserialize<
        'de,
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    >(
        d: D,
    ) -> Result<BTreeMap<K, V>, D::Error> {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}
