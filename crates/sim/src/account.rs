//! One paper account (spec §5.6): cash, positions at cost basis, an append-only ledger.
//! Identity (spec §15): cash + marks = bankroll + realized + unrealized, where
//! unrealized = marks − open basis. It holds exactly because basis leaves positions only through
//! `take_out`, whose output is the same number realized P&L subtracts.
use crate::config::AccountSpec;
use crate::types::{LegKey, Outcome, vecmap};
use crate::venue::OrderFill;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Buy,
    Sell,
    Convert,
    Settle,
    Mark,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Buy => "buy",
            EntryKind::Sell => "sell",
            EntryKind::Convert => "convert",
            EntryKind::Settle => "settle",
            EntryKind::Mark => "mark",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub account: String,
    pub seq: u64,
    pub t_ms: i64,
    pub kind: EntryKind,
    pub episode_id: Option<String>,
    pub leg: Option<LegKey>,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub cash_delta: Decimal,
    pub fees: Decimal,
    pub realized_delta: Decimal,
    pub cash_after: Decimal,
    pub equity: Option<Decimal>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub qty: Decimal,
    /// USDC paid for `qty`, fees included.
    pub basis: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Valuation {
    pub marks: Decimal,
    pub basis_open: Decimal,
}

impl Valuation {
    pub fn unrealized(&self) -> Decimal {
        self.marks - self.basis_open
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub spec: AccountSpec,
    pub cash: Decimal,
    /// Worst-case cost of intents decided but not yet executed.
    pub reserved: Decimal,
    pub realized: Decimal,
    pub fees_paid: Decimal,
    #[serde(with = "vecmap")]
    pub positions: BTreeMap<LegKey, Position>,
    /// YES returned by NO→collateral conversions: basis 0, marked at 0 until resolution (spec §5.6).
    #[serde(with = "vecmap")]
    pub returned: BTreeMap<LegKey, Decimal>,
    pub seq: u64,
    pub peak_equity: Decimal,
    pub max_drawdown: Decimal,
}

impl Account {
    pub fn new(spec: AccountSpec) -> Self {
        Self {
            id: spec.id(),
            cash: spec.bankroll,
            peak_equity: spec.bankroll,
            spec,
            reserved: Decimal::ZERO,
            realized: Decimal::ZERO,
            fees_paid: Decimal::ZERO,
            positions: BTreeMap::new(),
            returned: BTreeMap::new(),
            seq: 0,
            max_drawdown: Decimal::ZERO,
        }
    }

    pub fn free_cash(&self) -> Decimal {
        (self.cash - self.reserved).max(Decimal::ZERO)
    }

    #[allow(clippy::too_many_arguments)]
    fn entry(
        &mut self,
        t_ms: i64,
        kind: EntryKind,
        episode_id: Option<&str>,
        leg: Option<&LegKey>,
        qty: Decimal,
        price: Option<Decimal>,
        cash_delta: Decimal,
        fees: Decimal,
        realized_delta: Decimal,
    ) -> LedgerEntry {
        self.seq += 1;
        LedgerEntry {
            account: self.id.clone(),
            seq: self.seq,
            t_ms,
            kind,
            episode_id: episode_id.map(str::to_string),
            leg: leg.cloned(),
            qty,
            price,
            cash_delta,
            fees,
            realized_delta,
            cash_after: self.cash,
            equity: None,
        }
    }

    /// Remove up to `qty` from a position: (quantity removed, basis leaving with it).
    fn take_out(&mut self, leg: &LegKey, qty: Decimal) -> (Decimal, Decimal) {
        let Some(p) = self.positions.get_mut(leg) else {
            return (Decimal::ZERO, Decimal::ZERO);
        };
        let q = qty.min(p.qty);
        if q <= Decimal::ZERO {
            return (Decimal::ZERO, Decimal::ZERO);
        }
        // Rounded to 12 dp so every ledger figure keeps ≤ 12 dp: sums stay far inside Decimal's
        // 28 significant digits and the equity identity holds exactly (no silent rounding).
        let out = if q == p.qty {
            p.basis
        } else {
            (p.basis * q / p.qty)
                .round_dp_with_strategy(12, rust_decimal::RoundingStrategy::MidpointNearestEven)
        };
        p.qty -= q;
        p.basis -= out;
        if p.qty <= Decimal::ZERO {
            self.positions.remove(leg);
        }
        (q, out)
    }

    pub fn buy(
        &mut self,
        t_ms: i64,
        episode_id: &str,
        leg: &LegKey,
        f: &OrderFill,
    ) -> Option<LedgerEntry> {
        if f.filled <= Decimal::ZERO {
            return None;
        }
        let cost = f.notional + f.fees;
        self.cash -= cost;
        self.fees_paid += f.fees;
        let p = self.positions.entry(leg.clone()).or_default();
        p.qty += f.filled;
        p.basis += cost;
        Some(self.entry(
            t_ms,
            EntryKind::Buy,
            Some(episode_id),
            Some(leg),
            f.filled,
            f.vwap(),
            -cost,
            f.fees,
            Decimal::ZERO,
        ))
    }

    pub fn sell(
        &mut self,
        t_ms: i64,
        episode_id: &str,
        leg: &LegKey,
        f: &OrderFill,
    ) -> Option<LedgerEntry> {
        if f.filled <= Decimal::ZERO {
            return None;
        }
        let (q, basis_out) = self.take_out(leg, f.filled);
        let proceeds = f.notional - f.fees;
        self.cash += proceeds;
        self.fees_paid += f.fees;
        let realized = proceeds - basis_out;
        self.realized += realized;
        Some(self.entry(
            t_ms,
            EntryKind::Sell,
            Some(episode_id),
            Some(leg),
            q,
            f.vwap(),
            proceeds,
            f.fees,
            realized,
        ))
    }

    /// NO→collateral (NegRisk adapter): |S| NO sets of size q pay (|S| − 1)·q·(1 − feeBips) and
    /// return q YES of every leg outside S (marked 0). q is capped by what every S leg still holds.
    pub fn convert(
        &mut self,
        t_ms: i64,
        episode_id: &str,
        s_legs: &[LegKey],
        complements: &[LegKey],
        q: Decimal,
    ) -> Option<LedgerEntry> {
        if s_legs.len() < 2 {
            return None;
        }
        let held = s_legs
            .iter()
            .map(|l| self.positions.get(l).map_or(Decimal::ZERO, |p| p.qty))
            .min()
            .unwrap_or(Decimal::ZERO);
        let q = q.min(held);
        if q <= Decimal::ZERO {
            return None;
        }
        let mut basis_out = Decimal::ZERO;
        for l in s_legs {
            basis_out += self.take_out(l, q).1;
        }
        let sets = Decimal::from(u64::try_from(s_legs.len()).unwrap_or(u64::MAX)) - Decimal::ONE;
        let gross = sets * q;
        let fee = gross * self.spec.fee_bips / Decimal::from(10_000);
        let cash_in = gross - fee;
        self.cash += cash_in;
        self.fees_paid += fee;
        let realized = cash_in - basis_out;
        self.realized += realized;
        for c in complements {
            *self.returned.entry(c.clone()).or_default() += q;
        }
        Some(self.entry(
            t_ms,
            EntryKind::Convert,
            Some(episode_id),
            None,
            q,
            None,
            cash_in,
            fee,
            realized,
        ))
    }

    /// Settle every holding in `market_id` at its payout vector (never an assumed {0,1}; Q19).
    pub fn settle(
        &mut self,
        t_ms: i64,
        market_id: &str,
        payout_yes: Decimal,
        payout_no: Decimal,
    ) -> Vec<LedgerEntry> {
        let pay_of = |o: Outcome| match o {
            Outcome::Yes => payout_yes,
            Outcome::No => payout_no,
        };
        let mut out = Vec::new();
        let held: Vec<LegKey> = self
            .positions
            .keys()
            .filter(|k| k.market_id == market_id)
            .cloned()
            .collect();
        for leg in held {
            let (q, basis) = self.take_out(&leg, Decimal::MAX);
            let price = pay_of(leg.outcome);
            let pay = q * price;
            self.cash += pay;
            self.realized += pay - basis;
            out.push(self.entry(
                t_ms,
                EntryKind::Settle,
                None,
                Some(&leg),
                q,
                Some(price),
                pay,
                Decimal::ZERO,
                pay - basis,
            ));
        }
        let returned: Vec<LegKey> = self
            .returned
            .keys()
            .filter(|k| k.market_id == market_id)
            .cloned()
            .collect();
        for leg in returned {
            let q = self.returned.remove(&leg).unwrap_or(Decimal::ZERO);
            let price = pay_of(leg.outcome);
            let pay = q * price;
            self.cash += pay;
            self.realized += pay;
            out.push(self.entry(
                t_ms,
                EntryKind::Settle,
                None,
                Some(&leg),
                q,
                Some(price),
                pay,
                Decimal::ZERO,
                pay,
            ));
        }
        out
    }

    /// Positions at the current bid (none → 0, conservative); returned YES at 0 (spec §5.6).
    pub fn valuation(&self, bid: impl Fn(&LegKey) -> Option<Decimal>) -> Valuation {
        let mut v = Valuation {
            marks: Decimal::ZERO,
            basis_open: Decimal::ZERO,
        };
        for (leg, p) in &self.positions {
            v.marks += p.qty * bid(leg).unwrap_or(Decimal::ZERO);
            v.basis_open += p.basis;
        }
        v
    }

    pub fn mark(&mut self, t_ms: i64, bid: impl Fn(&LegKey) -> Option<Decimal>) -> LedgerEntry {
        let equity = self.cash + self.valuation(bid).marks;
        self.peak_equity = self.peak_equity.max(equity);
        self.max_drawdown = self.max_drawdown.max(self.peak_equity - equity);
        let mut e = self.entry(
            t_ms,
            EntryKind::Mark,
            None,
            None,
            Decimal::ZERO,
            None,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        e.equity = Some(equity);
        e
    }
}
