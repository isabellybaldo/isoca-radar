//! Archive rows (spec 5b §3). Integers only: prices and sizes in micro-units (value × 10⁶).
use anyhow::Context;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

pub const MICRO: i64 = 1_000_000;
pub const SIDE_BID: u8 = 0;
pub const SIDE_ASK: u8 = 1;

/// `gaps.reason` values (spec §3).
pub mod reason {
    pub const DOWN: &str = "down";
    pub const RAW_HOUR_LOST: &str = "raw_hour_lost";
    pub const NO_BOOK_YET: &str = "no_book_yet";
    pub const SCANNER_STOPPED: &str = "scanner_stopped";
}

/// value × 10⁶, exactly: an error when `d` has more than 6 decimals or does not fit.
pub fn micros(d: Decimal) -> anyhow::Result<i64> {
    let m = d
        .checked_mul(Decimal::from(MICRO))
        .with_context(|| format!("{d} × 10⁶ overflows"))?;
    anyhow::ensure!(m.fract().is_zero(), "{d} has more than 6 decimal places");
    m.to_i64()
        .with_context(|| format!("{d} × 10⁶ does not fit an i64"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRow {
    pub token_idx: u32,
    pub token_id: String,
    pub market_id: String,
    pub event_id: String,
    /// "yes" or "no".
    pub outcome: String,
    pub neg_risk: bool,
}

/// A change of best bid or best ask (price or size); a side is null when it is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TobRow {
    pub ts_ms: i64,
    pub recv_ms: i64,
    pub token_idx: u32,
    pub bid: Option<i64>,
    pub bid_sz: Option<i64>,
    pub ask: Option<i64>,
    pub ask_sz: Option<i64>,
}

/// One level of a periodic snapshot (level 0 = best).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthRow {
    pub ts_ms: i64,
    pub token_idx: u32,
    pub side: u8,
    pub level: u16,
    pub price: i64,
    pub size: i64,
}

/// An exact level change (or, with `snapshot`, a level of a full book) inside an episode window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2Row {
    pub ts_ms: i64,
    pub recv_ms: i64,
    pub token_idx: u32,
    pub side: u8,
    pub price: i64,
    pub new_size: i64,
    pub snapshot: bool,
    pub episode_id: String,
}

/// A stretch with missing or untrustworthy data. `shard` null: the whole feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapRow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub shard: Option<i32>,
    pub reason: String,
}

/// Where the reducer puts rows (Parquet parts in production, vectors in tests).
pub trait Sink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()>;
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()>;
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()>;
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VecSink {
    pub tob: Vec<TobRow>,
    pub depth: Vec<DepthRow>,
    pub l2: Vec<L2Row>,
    pub gaps: Vec<GapRow>,
}

impl Sink for VecSink {
    fn tob(&mut self, r: TobRow) -> anyhow::Result<()> {
        self.tob.push(r);
        Ok(())
    }
    fn depth(&mut self, r: DepthRow) -> anyhow::Result<()> {
        self.depth.push(r);
        Ok(())
    }
    fn l2(&mut self, r: L2Row) -> anyhow::Result<()> {
        self.l2.push(r);
        Ok(())
    }
    fn gap(&mut self, r: GapRow) -> anyhow::Result<()> {
        self.gaps.push(r);
        Ok(())
    }
}
