//! Parsing and limits (09-27 spec §4.1, 7b §3.4). Every check runs before any arb-core or sim
//! call; nothing is clamped. Decimals arrive as JSON strings only: a JSON number where a decimal
//! is expected fails deserialization ("not valid input: …").
use arb_core::book::Book;
use arb_core::detect::{Group, GroupLeg, LegStatus};
use arb_core::fee::FeeSchedule;
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::Deserialize;

pub const MAX_INPUT: usize = 16 * 1024;
pub const MAX_LEGS: usize = 6;
pub const MAX_LEVELS: usize = 8;
pub const MAX_SIZE: u32 = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelIn {
    pub p: String,
    pub s: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeIn {
    pub rate: String,
    pub exponent: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegIn {
    pub status: String,
    #[serde(default)]
    pub bids: Vec<LevelIn>,
    #[serde(default)]
    pub asks: Vec<LevelIn>,
    #[serde(default)]
    pub fee: Option<FeeIn>,
}

/// One YES book: bids and asks, each at most 8 levels.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookIn {
    #[serde(default)]
    pub bids: Vec<LevelIn>,
    #[serde(default)]
    pub asks: Vec<LevelIn>,
}

/// A decimal written as a JSON string.
pub fn decimal(s: &str, what: &str) -> Result<Decimal, String> {
    Decimal::from_str_exact(s.trim()).map_err(|_| format!("{what} {s:?} is not a decimal"))
}

pub fn price(s: &str) -> Result<Price, String> {
    let bad = || format!("price {s:?} must be 0.01 to 0.99 in steps of 0.01");
    let p = Price::parse(s).map_err(|_| bad())?;
    let d = p.get();
    if d < Decimal::new(1, 2)
        || d > Decimal::new(99, 2)
        || (d * Decimal::ONE_HUNDRED).fract() != Decimal::ZERO
    {
        return Err(bad());
    }
    Ok(p)
}

/// A share quantity above 0 and at most 1,000,000, with at most 6 decimals. `what` names it.
pub fn quantity(s: &str, what: &str) -> Result<Decimal, String> {
    let bad =
        || format!("{what} {s:?} must be above 0 and at most 1,000,000, with at most 6 decimals");
    let q = Shares::parse(s).map_err(|_| bad())?.get();
    if q <= Decimal::ZERO || q > Decimal::from(MAX_SIZE) {
        return Err(bad());
    }
    Ok(q)
}

pub fn size(s: &str) -> Result<Shares, String> {
    let q = quantity(s, "size")?;
    Shares::new(q).map_err(|e| e.to_string())
}

pub fn levels(v: &[LevelIn]) -> Result<Vec<(Price, Shares)>, String> {
    if v.len() > MAX_LEVELS {
        return Err(format!("at most {MAX_LEVELS} levels per side"));
    }
    let mut out: Vec<(Price, Shares)> = Vec::with_capacity(v.len());
    for l in v {
        let p = price(&l.p)?;
        if out.iter().any(|(q, _)| *q == p) {
            return Err(format!("two levels at the same price {}", d(p.get())));
        }
        out.push((p, size(&l.s)?));
    }
    Ok(out)
}

pub fn fee(f: &Option<FeeIn>) -> Result<FeeSchedule, String> {
    match f {
        None => Ok(FeeSchedule::ZERO),
        Some(f) => {
            let rate = decimal(&f.rate, "fee rate")?;
            FeeSchedule::new(rate, f.exponent).map_err(|e| format!("fee: {e}"))
        }
    }
}

pub fn status(s: &str) -> Result<LegStatus, String> {
    Ok(match s {
        "live" => LegStatus::Live,
        "placeholder" => LegStatus::Placeholder,
        "other" => LegStatus::Other,
        "closed_no" => LegStatus::ClosedNo,
        "closed_yes" => LegStatus::ClosedYes,
        "halted" => LegStatus::Halted,
        _ => return Err(format!("unknown leg status {s:?}")),
    })
}

pub fn book(bids: &[LevelIn], asks: &[LevelIn]) -> Result<Book, String> {
    let mut b = Book::default();
    b.apply_snapshot(&levels(bids)?, &levels(asks)?, 0);
    Ok(b)
}

pub fn group(legs: &[LegIn], augmented: bool) -> Result<Group, String> {
    if legs.is_empty() || legs.len() > MAX_LEGS {
        return Err(format!("1 to at most {MAX_LEGS} legs"));
    }
    let legs = legs
        .iter()
        .map(|l| {
            Ok(GroupLeg {
                status: status(&l.status)?,
                book: book(&l.bids, &l.asks)?,
                fee: fee(&l.fee)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Group { legs, augmented })
}

/// Decimal → JSON string: normalized, and zero is always "0" (never "-0").
pub fn d(x: Decimal) -> String {
    if x.is_zero() {
        "0".to_string()
    } else {
        x.normalize().to_string()
    }
}
