//! `policies.toml` (spec §11): accounts = policy × latency scenario (× sensitivities).
use crate::SimError;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyKind {
    #[serde(rename = "P0-converter")]
    P0Converter,
    #[serde(rename = "P1-settlement")]
    P1Settlement,
    #[serde(rename = "P2-combo")]
    P2Combo,
    #[serde(rename = "P3-naive")]
    P3Naive,
}

impl PolicyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyKind::P0Converter => "P0-converter",
            PolicyKind::P1Settlement => "P1-settlement",
            PolicyKind::P2Combo => "P2-combo",
            PolicyKind::P3Naive => "P3-naive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BurstStep {
    pub frames_per_s_above: u64,
    pub multiplier: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub proc_ms: i64,
    pub net_ms: i64,
    pub match_ms: i64,
    pub honor_seconds_delay: bool,
    #[serde(default)]
    pub burst: Vec<BurstStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountSpec {
    pub policy: PolicyKind,
    pub scenario: String,
    pub bankroll: Decimal,
    pub per_trade: Decimal,
    /// NegRisk adapter conversion fee in basis points (Q4 sensitivity; 0 = V1 measurement).
    #[serde(default)]
    pub fee_bips: Decimal,
}

impl AccountSpec {
    /// Stable account id, e.g. `P0-converter/sp-idle` or `P0-converter/sp-idle/fee10`.
    pub fn id(&self) -> String {
        if self.fee_bips.is_zero() {
            format!("{}/{}", self.policy.as_str(), self.scenario)
        } else {
            format!(
                "{}/{}/fee{}",
                self.policy.as_str(),
                self.scenario,
                self.fee_bips.normalize()
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P1Params {
    pub min_annualized: Decimal,
    pub max_days_to_end: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    pub settle_delay_ms: i64,
    pub markout_offsets_ms: Vec<i64>,
    pub mark_every_ms: i64,
    pub burst_window_ms: i64,
    pub min_order_shares: Decimal,
    pub p1: P1Params,
    pub scenarios: Vec<Scenario>,
    pub accounts: Vec<AccountSpec>,
}

fn bad(msg: impl Into<String>) -> SimError {
    SimError::Config(msg.into())
}

impl SimConfig {
    pub fn scenario(&self, name: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.name == name)
    }

    pub fn validate(&self) -> Result<(), SimError> {
        if self.settle_delay_ms < 0 || self.mark_every_ms <= 0 || self.burst_window_ms <= 0 {
            return Err(bad(
                "settle_delay_ms ≥ 0, mark_every_ms > 0, burst_window_ms > 0",
            ));
        }
        if self.min_order_shares <= Decimal::ZERO {
            return Err(bad("min_order_shares must be > 0"));
        }
        if self.markout_offsets_ms.iter().any(|&o| o <= 0)
            || self.markout_offsets_ms.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(bad(
                "markout_offsets_ms must be positive and strictly increasing",
            ));
        }
        let mut names = HashSet::new();
        for s in &self.scenarios {
            if !names.insert(s.name.as_str()) {
                return Err(bad(format!("duplicate scenario {}", s.name)));
            }
            if s.proc_ms < 0 || s.net_ms < 0 || s.match_ms < 0 {
                return Err(bad(format!("scenario {}: latencies must be ≥ 0", s.name)));
            }
            if s.burst.iter().any(|b| b.multiplier < 1)
                || s.burst
                    .windows(2)
                    .any(|w| w[0].frames_per_s_above >= w[1].frames_per_s_above)
            {
                return Err(bad(format!(
                    "scenario {}: burst steps need multiplier ≥ 1 and increasing thresholds",
                    s.name
                )));
            }
        }
        let mut ids = HashSet::new();
        for a in &self.accounts {
            if self.scenario(&a.scenario).is_none() {
                return Err(bad(format!("account {}: unknown scenario", a.id())));
            }
            if a.bankroll <= Decimal::ZERO || a.per_trade <= Decimal::ZERO {
                return Err(bad(format!(
                    "account {}: bankroll and per_trade must be > 0",
                    a.id()
                )));
            }
            if a.fee_bips < Decimal::ZERO || a.fee_bips > Decimal::from(10_000) {
                return Err(bad(format!(
                    "account {}: fee_bips must be in [0, 10000]",
                    a.id()
                )));
            }
            if !ids.insert(a.id()) {
                return Err(bad(format!("duplicate account {}", a.id())));
            }
        }
        if self.accounts.is_empty() {
            return Err(bad("no accounts"));
        }
        Ok(())
    }
}
