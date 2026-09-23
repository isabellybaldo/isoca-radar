//! Taker fee model (spec §3, §5.1, L8): fee = shares × rate × (p(1−p))^exponent.
use crate::money::Price;
use rust_decimal::{Decimal, RoundingStrategy};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeeError {
    #[error("fee rate must be >= 0, got {0}")]
    NegativeRate(Decimal),
    #[error("fee exponent must be 1..=4, got {0}")]
    BadExponent(u8),
}

/// A market's taker fee schedule (Gamma `feeSchedule.rate` / `.exponent`). We are always the taker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeSchedule {
    rate: Decimal,
    exponent: u8,
}

impl FeeSchedule {
    pub const ZERO: FeeSchedule = FeeSchedule {
        rate: Decimal::ZERO,
        exponent: 1,
    };

    pub fn new(rate: Decimal, exponent: u8) -> Result<Self, FeeError> {
        if rate < Decimal::ZERO {
            return Err(FeeError::NegativeRate(rate));
        }
        if !(1..=4).contains(&exponent) {
            return Err(FeeError::BadExponent(exponent));
        }
        Ok(Self { rate, exponent })
    }

    /// Per-share fee φ(p) = rate · (p(1−p))^exponent. Symmetric: φ(p) = φ(1−p).
    pub fn phi(&self, p: Price) -> Decimal {
        let x = p.get() * (Decimal::ONE - p.get());
        let mut acc = Decimal::ONE;
        for _ in 0..self.exponent {
            acc *= x;
        }
        self.rate * acc
    }

    /// c(p) = p + φ(p): cost of buying one share at p (L8).
    pub fn buy_cost(&self, p: Price) -> Decimal {
        p.get() + self.phi(p)
    }

    /// v(b) = b − φ(b): proceeds of selling one share at b (L8).
    pub fn sell_proceeds(&self, b: Price) -> Decimal {
        b.get() - self.phi(b)
    }

    /// Unrounded fee for qty shares at p (for math checks).
    pub fn fee_exact(&self, p: Price, qty: Decimal) -> Decimal {
        self.phi(p) * qty
    }

    /// The docs' rule: rounded to 5 dp, anything below 0.00001 becomes 0 (rounding mode unverified).
    pub fn fee_venue(&self, p: Price, qty: Decimal) -> Decimal {
        self.fee_exact(p, qty)
            .round_dp_with_strategy(5, RoundingStrategy::MidpointAwayFromZero)
    }

    /// What Π books: always round up to 5 dp (never 0 for a positive fee).
    pub fn fee_conservative(&self, p: Price, qty: Decimal) -> Decimal {
        self.fee_exact(p, qty)
            .round_dp_with_strategy(5, RoundingStrategy::AwayFromZero)
    }
}
