//! Exact money types (spec §5.1, §15). No floating point.
use rust_decimal::Decimal;
use thiserror::Error;

/// Polymarket prices and sizes never exceed 6 decimal places (pUSD has 6).
pub const MAX_DP: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MoneyError {
    #[error("not a decimal: {0:?}")]
    Parse(String),
    #[error("more than {MAX_DP} decimal places: {0}")]
    TooPrecise(Decimal),
    #[error("out of range: {0}")]
    OutOfRange(Decimal),
}

fn parse_decimal(s: &str) -> Result<Decimal, MoneyError> {
    Decimal::from_str_exact(s.trim()).map_err(|_| MoneyError::Parse(s.to_string()))
}

fn normalized_within_dp(d: Decimal) -> Result<Decimal, MoneyError> {
    let n = d.normalize();
    if n.scale() > MAX_DP {
        Err(MoneyError::TooPrecise(d))
    } else {
        Ok(n)
    }
}

/// A price in [0, 1], at most 6 decimal places, stored normalized (0.50 == 0.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(Decimal);

impl Price {
    pub fn new(d: Decimal) -> Result<Self, MoneyError> {
        if d < Decimal::ZERO || d > Decimal::ONE {
            return Err(MoneyError::OutOfRange(d));
        }
        Ok(Self(normalized_within_dp(d)?))
    }

    pub fn parse(s: &str) -> Result<Self, MoneyError> {
        Self::new(parse_decimal(s)?)
    }

    pub fn get(self) -> Decimal {
        self.0
    }

    /// L1 mirror: YES price p corresponds to NO price 1 − p.
    pub fn complement(self) -> Self {
        Self((Decimal::ONE - self.0).normalize())
    }
}

/// A non-negative share quantity, at most 6 decimal places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shares(Decimal);

impl Shares {
    pub fn new(d: Decimal) -> Result<Self, MoneyError> {
        if d < Decimal::ZERO {
            return Err(MoneyError::OutOfRange(d));
        }
        Ok(Self(normalized_within_dp(d)?))
    }

    pub fn parse(s: &str) -> Result<Self, MoneyError> {
        Self::new(parse_decimal(s)?)
    }

    pub fn get(self) -> Decimal {
        self.0
    }
}
