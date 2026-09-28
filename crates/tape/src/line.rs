//! Capture lines (parent spec §7): frames {recv_ts_us, shard, raw} and control lines
//! {recv_ts_us, shard, ctl}. The scanner writes them; replay and the compactor read them.
use arb_core::book::Book;
use arb_core::money::{Price, Shares};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub recv_ts_us: i64,
    pub shard: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctl: Option<Ctl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ctl {
    Down,
    Universe {
        taken_ms: i64,
        /// The exact universe record file name (no directory), e.g. `universe-<taken_ms>-<seq>.json.zst`.
        file: String,
    },
    RestSnapshot {
        token: String,
        server_ts_ms: i64,
        bids: Vec<(String, String)>,
        asks: Vec<(String, String)>,
    },
    Suspect {
        event_id: String,
        until_ms: i64,
    },
    /// A market resolution the live resolver found (S3c), with the recv instant the engine got.
    /// Payouts are exact decimal strings, like `RestSnapshot` levels.
    Resolved {
        market_id: String,
        payout_yes: String,
        payout_no: String,
    },
    Shutdown,
}

pub fn book_ctl(token: &str, book: &Book) -> Ctl {
    let lv = |v: Vec<(Price, Shares)>| -> Vec<(String, String)> {
        v.into_iter()
            .map(|(p, s)| (p.get().to_string(), s.get().to_string()))
            .collect()
    };
    Ctl::RestSnapshot {
        token: token.to_string(),
        server_ts_ms: book.server_ts_ms(),
        bids: lv(book.bids_best_first()),
        asks: lv(book.asks_best_first()),
    }
}

pub fn ctl_book(
    bids: &[(String, String)],
    asks: &[(String, String)],
    ts: i64,
) -> anyhow::Result<Book> {
    let parse = |v: &[(String, String)]| -> anyhow::Result<Vec<(Price, Shares)>> {
        v.iter()
            .map(|(p, s)| -> anyhow::Result<(Price, Shares)> {
                Ok((Price::parse(p)?, Shares::parse(s)?))
            })
            .collect()
    };
    let mut b = Book::default();
    b.apply_snapshot(&parse(bids)?, &parse(asks)?, ts);
    Ok(b)
}
