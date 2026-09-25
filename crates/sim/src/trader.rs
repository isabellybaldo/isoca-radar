//! The paper trader (spec §5.6, §11): one account per `policies.toml` entry, each with its own
//! consumed-liquidity ledger. Pure: time arrives on the inputs (see the call contract in the plan).
use crate::SimError;
use crate::account::Account;
use crate::config::SimConfig;
use crate::consumed::ConsumedLiquidity;
use crate::latency::{FrameRate, exec_delay_ms};
use crate::policy::{Decision, Hold, OpenCtx, Planned, decide};
use crate::rows::{AccountRow, ExecRow, FillRow, IntentRow, MarkoutRow, SimOut};
use crate::types::{BookSide, LegKey, Outcome};
use crate::venue::{OrderFill, fak_buy, fak_sell, outcome_levels, yes_level};
use arb_core::book::Book;
use arb_core::detect::Kind;
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::Shares;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Read access to the engine's YES books.
pub trait Books {
    fn yes_book(&self, yes_token: &str) -> Option<&Book>;
}

impl Books for HashMap<String, Book> {
    fn yes_book(&self, yes_token: &str) -> Option<&Book> {
        self.get(yes_token)
    }
}

#[derive(Debug, Clone)]
struct Pending {
    acct: usize,
    episode_id: String,
    kind: Kind,
    planned: Planned,
    t_exec: i64,
    seq: u64,
}

#[derive(Debug, Clone)]
struct Unwind {
    acct: usize,
    episode_id: String,
    leg: LegKey,
    fee: FeeSchedule,
    qty: Decimal,
    due_ms: i64,
    seq: u64,
}

#[derive(Debug, Clone)]
struct Conversion {
    acct: usize,
    episode_id: String,
    s_legs: Vec<LegKey>,
    complements: Vec<LegKey>,
    q: Decimal,
    due_ms: i64,
    seq: u64,
}

#[derive(Debug, Clone)]
struct Markout {
    acct: usize,
    episode_id: String,
    leg: LegKey,
    vwap: Decimal,
    offset_ms: i64,
    due_ms: i64,
    seq: u64,
}

/// Where an item sorts among others due at the same millisecond (spec §5.6): a mark always comes
/// last, so it never sees a same-ms execution that hasn't happened yet from the mark's own point of
/// view — but an execution due at an EARLIER ms than the mark's boundary still sorts before it by
/// `due_ms` alone, and a LATER one sorts after, which is the actual fix for the review finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Pending,
    Unwind,
    Conversion,
    Markout,
    Mark,
}

/// The next not-yet-processed item, identified by its queue + index (or the pending mark).
#[derive(Debug, Clone, Copy)]
enum Which {
    Pending(usize),
    Unwind(usize),
    Conversion(usize),
    Markout(usize),
    Mark,
}

/// What survives a restart: the accounts. Pending intents, unwinds and markouts are dropped at
/// shutdown (`close`); pending conversions are completed by `close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraderState {
    pub accounts: Vec<Account>,
}

pub struct PaperTrader {
    cfg: SimConfig,
    accounts: Vec<Account>,
    scenario_ix: Vec<usize>,
    consumed: Vec<ConsumedLiquidity>,
    pending: Vec<Pending>,
    unwinds: Vec<Unwind>,
    conversions: Vec<Conversion>,
    markouts: Vec<Markout>,
    rate: FrameRate,
    next_mark_ms: Option<i64>,
    next_seq: u64,
}

/// The next minute-mark boundary strictly after `t`.
fn mark_boundary_after(every: i64, t: i64) -> i64 {
    (t.div_euclid(every) + 1) * every
}

/// The bid we could sell `leg` into now (NO bid = 1 − YES ask).
pub fn bid_of(books: &impl Books, leg: &LegKey) -> Option<Decimal> {
    let top = books.yes_book(&leg.yes_token)?.top();
    match leg.outcome {
        Outcome::Yes => top.best_bid.map(|(p, _)| p.get()),
        Outcome::No => top.best_ask.map(|(p, _)| Decimal::ONE - p.get()),
    }
}

/// Mid of `leg`'s outcome (NO mid = 1 − YES mid); None unless both YES sides exist.
pub fn mid_of(books: &impl Books, leg: &LegKey) -> Option<Decimal> {
    let top = books.yes_book(&leg.yes_token)?.top();
    let m = (top.best_bid?.0.get() + top.best_ask?.0.get()) / Decimal::TWO;
    Some(match leg.outcome {
        Outcome::Yes => m,
        Outcome::No => Decimal::ONE - m,
    })
}

impl PaperTrader {
    pub fn new(cfg: SimConfig) -> Result<Self, SimError> {
        let accounts = cfg.accounts.iter().cloned().map(Account::new).collect();
        Self::with_accounts(cfg, accounts)
    }

    fn with_accounts(cfg: SimConfig, accounts: Vec<Account>) -> Result<Self, SimError> {
        cfg.validate()?;
        let scenario_ix = accounts
            .iter()
            .map(|a| {
                cfg.scenarios
                    .iter()
                    .position(|s| s.name == a.spec.scenario)
                    .ok_or_else(|| SimError::Config(format!("account {}: unknown scenario", a.id)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let n = accounts.len();
        Ok(Self {
            rate: FrameRate::new(cfg.burst_window_ms),
            cfg,
            accounts,
            scenario_ix,
            consumed: vec![ConsumedLiquidity::default(); n],
            pending: Vec::new(),
            unwinds: Vec::new(),
            conversions: Vec::new(),
            markouts: Vec::new(),
            next_mark_ms: None,
            next_seq: 0,
        })
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    fn delay(&self, acct: usize, seconds_delay: Option<i32>) -> i64 {
        let sc = &self.cfg.scenarios[self.scenario_ix[acct]];
        exec_delay_ms(sc, seconds_delay, self.rate.per_second())
    }

    /// A fresh, strictly increasing tie-breaker for the global due-work ordering.
    fn alloc_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    pub fn on_open(&mut self, ctx: &OpenCtx) -> Vec<SimOut> {
        let mut out = Vec::new();
        let max_delay = ctx
            .signal
            .legs_used
            .iter()
            .filter_map(|&i| ctx.legs.get(i).and_then(|l| l.seconds_delay))
            .max();
        for a in 0..self.accounts.len() {
            let t_exec = ctx.r_open_ms + self.delay(a, max_delay);
            let acct = &self.accounts[a];
            match decide(&self.cfg, &acct.spec, acct.free_cash(), ctx) {
                Decision::NotMine => {}
                Decision::Skip(r) => out.push(SimOut::Intent(IntentRow {
                    account: acct.id.clone(),
                    episode_id: ctx.episode_id.clone(),
                    decided_ms: ctx.r_open_ms,
                    exec_ms: None,
                    skip_reason: Some(r.as_str().to_string()),
                    q_target: None,
                    detected_q: None,
                    detected_pnl: None,
                    reserve: None,
                })),
                Decision::Take(planned) => {
                    out.push(SimOut::Intent(IntentRow {
                        account: acct.id.clone(),
                        episode_id: ctx.episode_id.clone(),
                        decided_ms: ctx.r_open_ms,
                        exec_ms: Some(t_exec),
                        skip_reason: None,
                        q_target: Some(planned.q_target),
                        detected_q: Some(planned.detected_q),
                        detected_pnl: Some(planned.detected_pnl),
                        reserve: Some(planned.reserve),
                    }));
                    self.accounts[a].reserved += planned.reserve;
                    let seq = self.alloc_seq();
                    self.pending.push(Pending {
                        acct: a,
                        episode_id: ctx.episode_id.clone(),
                        kind: ctx.signal.kind,
                        planned,
                        t_exec,
                        seq,
                    });
                }
            }
        }
        out
    }

    /// The earliest item due at or before `recv_ms`, across every queue and the pending minute
    /// mark, ordered by (due_ms, kind_rank, seq): one global timeline instead of queue-by-queue
    /// (spec §5.6; review finding on Task 8). `None` once nothing more is due.
    fn earliest_due(&self, recv_ms: i64, mark_at: Option<i64>) -> Option<Which> {
        let mut best: Option<(i64, Rank, u64, Which)> = None;
        macro_rules! consider {
            ($due:expr, $rank:expr, $seq:expr, $w:expr) => {
                if $due <= recv_ms {
                    let key = ($due, $rank, $seq);
                    if best.is_none_or(|(d, r, s, _)| key < (d, r, s)) {
                        best = Some(($due, $rank, $seq, $w));
                    }
                }
            };
        }
        if let Some((i, p)) = self
            .pending
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (p.t_exec, p.seq))
        {
            consider!(p.t_exec, Rank::Pending, p.seq, Which::Pending(i));
        }
        if let Some((i, u)) = self
            .unwinds
            .iter()
            .enumerate()
            .min_by_key(|(_, u)| (u.due_ms, u.seq))
        {
            consider!(u.due_ms, Rank::Unwind, u.seq, Which::Unwind(i));
        }
        if let Some((i, c)) = self
            .conversions
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| (c.due_ms, c.seq))
        {
            consider!(c.due_ms, Rank::Conversion, c.seq, Which::Conversion(i));
        }
        if let Some((i, m)) = self
            .markouts
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| (m.due_ms, m.seq))
        {
            consider!(m.due_ms, Rank::Markout, m.seq, Which::Markout(i));
        }
        if let Some(at) = mark_at {
            consider!(at, Rank::Mark, 0u64, Which::Mark);
        }
        best.map(|(_, _, _, w)| w)
    }

    /// If a minute mark is due at or before `now`, the boundary to stamp it at (spec §5.6: at most
    /// one mark per call, at the latest boundary ≤ `now`, however long the input gap was).
    fn mark_due_at(&self, now: i64) -> Option<i64> {
        let next = self.next_mark_ms?;
        if now < next {
            return None;
        }
        Some(now.div_euclid(self.cfg.mark_every_ms) * self.cfg.mark_every_ms)
    }

    /// Call BEFORE applying the frame at `recv_ms`: runs everything due at or before it, in one
    /// global due-time order (see `earliest_due`). Loops so that anything queued while draining
    /// (e.g. a 0 ms delay) is picked up in its correct place within this same call.
    pub fn on_frame(&mut self, recv_ms: i64, books: &impl Books) -> Vec<SimOut> {
        self.rate.push(recv_ms);
        let mut out = Vec::new();

        if self.next_mark_ms.is_none() {
            self.next_mark_ms = Some(mark_boundary_after(self.cfg.mark_every_ms, recv_ms));
        }
        let mut mark_at = self.mark_due_at(recv_ms);
        let mark_was_due = mark_at.is_some();

        loop {
            match self.earliest_due(recv_ms, mark_at) {
                None => break,
                Some(Which::Pending(i)) => {
                    let p = self.pending.remove(i);
                    self.execute(p, books, &mut out);
                }
                Some(Which::Unwind(i)) => {
                    let u = self.unwinds.remove(i);
                    self.unwind(u, books, &mut out);
                }
                Some(Which::Conversion(i)) => {
                    let c = self.conversions.remove(i);
                    self.convert(c, &mut out);
                }
                Some(Which::Markout(i)) => {
                    let m = self.markouts.remove(i);
                    let mid = mid_of(books, &m.leg);
                    out.push(SimOut::Markout(MarkoutRow {
                        account: self.accounts[m.acct].id.clone(),
                        episode_id: m.episode_id,
                        leg: m.leg,
                        fill_vwap: m.vwap,
                        offset_ms: m.offset_ms,
                        mid,
                        markout: mid.map(|x| x - m.vwap),
                    }));
                }
                Some(Which::Mark) => {
                    if let Some(at) = mark_at.take() {
                        self.mark_all(at, books, &mut out);
                    }
                }
            }
        }

        if mark_was_due {
            self.next_mark_ms = Some(mark_boundary_after(self.cfg.mark_every_ms, recv_ms));
        }
        out
    }

    /// A market resolved with this payout vector: settle it in every account (spec §5.6, Q19).
    pub fn on_resolution(
        &mut self,
        t_ms: i64,
        market_id: &str,
        payout_yes: Decimal,
        payout_no: Decimal,
    ) -> Vec<SimOut> {
        let mut out = Vec::new();
        for a in 0..self.accounts.len() {
            let entries = self.accounts[a].settle(t_ms, market_id, payout_yes, payout_no);
            if entries.is_empty() {
                continue;
            }
            out.extend(entries.into_iter().map(SimOut::Ledger));
            // No books here: remaining positions are valued at 0 in this row (conservative);
            // the next minute mark revalues them at the bid.
            let acct = &self.accounts[a];
            let v = acct.valuation(|_| None);
            out.push(SimOut::Account(AccountRow {
                account: acct.id.clone(),
                t_ms,
                cash: acct.cash,
                equity: acct.cash + v.marks,
                realized: acct.realized,
                unrealized: v.unrealized(),
                fees_paid: acct.fees_paid,
                locked: v.basis_open,
                max_drawdown: acct.max_drawdown,
                n_positions: i64::try_from(acct.positions.len() + acct.returned.len())
                    .unwrap_or(i64::MAX),
            }));
        }
        out
    }

    /// Shutdown at `t_ms`: complete pending conversions (the on-chain step would have happened),
    /// drop pending intents (releasing their reservations), unwinds and markouts, then mark.
    pub fn close(&mut self, t_ms: i64, books: &impl Books) -> Vec<SimOut> {
        let mut out = Vec::new();
        let mut due: Vec<Conversion> = std::mem::take(&mut self.conversions);
        due.sort_by_key(|c| (c.due_ms, c.seq));
        for mut c in due {
            c.due_ms = t_ms;
            self.convert(c, &mut out);
        }
        for p in std::mem::take(&mut self.pending) {
            let a = &mut self.accounts[p.acct];
            a.reserved = (a.reserved - p.planned.reserve).max(Decimal::ZERO);
        }
        self.unwinds.clear();
        self.markouts.clear();
        self.mark_all(t_ms, books, &mut out);
        out
    }

    pub fn state(&self) -> TraderState {
        TraderState {
            accounts: self.accounts.clone(),
        }
    }

    pub fn restore(cfg: SimConfig, state: TraderState) -> Result<Self, SimError> {
        let mut saved: HashMap<String, Account> = state
            .accounts
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        let accounts = cfg
            .accounts
            .iter()
            .map(|spec| {
                saved.remove(&spec.id()).map_or_else(
                    || Account::new(spec.clone()),
                    |mut a| {
                        a.reserved = Decimal::ZERO;
                        a
                    },
                )
            })
            .collect();
        Self::with_accounts(cfg, accounts)
    }

    pub fn on_level(&mut self, yes_token: &str, side: BookSide, price: Decimal, new_size: Decimal) {
        let key = crate::consumed::LevelKey {
            yes_token: yes_token.to_string(),
            side,
            price,
        };
        for c in &mut self.consumed {
            c.on_level(&key, new_size);
        }
    }

    pub fn on_snapshot(&mut self, yes_token: &str, book: &Book) {
        for c in &mut self.consumed {
            c.on_snapshot(yes_token, book);
        }
    }

    /// Π of the intended basket on the current books minus this account's consumed liquidity.
    fn pnl_now(&self, p: &Pending, books: &impl Books) -> Option<Decimal> {
        let consumed = &self.consumed[p.acct];
        let mut legs = Vec::with_capacity(p.planned.legs.len());
        for l in &p.planned.legs {
            let book = books.yes_book(&l.key.yes_token)?;
            let levels = outcome_levels(book, l.key.outcome, true)
                .into_iter()
                .filter_map(|(px, s)| {
                    let avail = consumed.available(&yes_level(&l.key, true, px), s.get());
                    Shares::new(avail)
                        .ok()
                        .filter(|a| a.get() > Decimal::ZERO)
                        .map(|a| (px, a))
                })
                .collect();
            legs.push(BuyLeg { levels, fee: l.fee });
        }
        let rule = match p.kind {
            Kind::NoBasket => BasketRule::NestedNo,
            _ => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
        };
        Some(walk(&legs, rule, None).pnl)
    }

    fn execute(&mut self, p: Pending, books: &impl Books, out: &mut Vec<SimOut>) {
        let a = p.acct;
        debug_assert!(
            self.accounts[a].reserved >= p.planned.reserve,
            "account {}: reserved {} < this intent's reserve {}",
            self.accounts[a].id,
            self.accounts[a].reserved,
            p.planned.reserve
        );
        self.accounts[a].reserved =
            (self.accounts[a].reserved - p.planned.reserve).max(Decimal::ZERO);
        let pnl_at_tx = self.pnl_now(&p, books);
        let mut filled = Vec::with_capacity(p.planned.legs.len());
        for leg in &p.planned.legs {
            let f = match books.yes_book(&leg.key.yes_token) {
                Some(b) => fak_buy(
                    b,
                    &leg.key,
                    leg.fee,
                    leg.limit,
                    p.planned.q_target,
                    &mut self.consumed[a],
                ),
                None => OrderFill::default(),
            };
            out.push(SimOut::Fill(FillRow {
                account: self.accounts[a].id.clone(),
                episode_id: p.episode_id.clone(),
                leg: leg.key.clone(),
                t_ms: p.t_exec,
                side: "buy",
                limit: Some(leg.limit.get()),
                requested: p.planned.q_target,
                filled: f.filled,
                vwap: f.vwap(),
                fees: f.fees,
                levels_used: f.levels_used,
            }));
            if let Some(e) = self.accounts[a].buy(p.t_exec, &p.episode_id, &leg.key, &f) {
                out.push(SimOut::Ledger(e));
            }
            filled.push(f);
        }
        let q_eff = filled
            .iter()
            .map(|f| f.filled)
            .min()
            .unwrap_or(Decimal::ZERO);
        let mut leg_risk = Decimal::ZERO;
        for (leg, f) in p.planned.legs.iter().zip(&filled) {
            let excess = f.filled - q_eff;
            if excess > Decimal::ZERO {
                leg_risk += excess;
                let due_ms = p.t_exec + self.delay(a, leg.seconds_delay);
                let seq = self.alloc_seq();
                self.unwinds.push(Unwind {
                    acct: a,
                    episode_id: p.episode_id.clone(),
                    leg: leg.key.clone(),
                    fee: leg.fee,
                    qty: excess,
                    due_ms,
                    seq,
                });
            }
            if let Some(vwap) = f.vwap() {
                let offsets = self.cfg.markout_offsets_ms.clone();
                for off in offsets {
                    let seq = self.alloc_seq();
                    self.markouts.push(Markout {
                        acct: a,
                        episode_id: p.episode_id.clone(),
                        leg: leg.key.clone(),
                        vwap,
                        offset_ms: off,
                        due_ms: p.t_exec + off,
                        seq,
                    });
                }
            }
        }
        if q_eff > Decimal::ZERO
            && let Hold::Convert { complements } = &p.planned.hold
        {
            let seq = self.alloc_seq();
            self.conversions.push(Conversion {
                acct: a,
                episode_id: p.episode_id.clone(),
                s_legs: p.planned.legs.iter().map(|l| l.key.clone()).collect(),
                complements: complements.clone(),
                q: q_eff,
                due_ms: p.t_exec + self.cfg.settle_delay_ms,
                seq,
            });
        }
        out.push(SimOut::Exec(ExecRow {
            account: self.accounts[a].id.clone(),
            episode_id: p.episode_id,
            exec_ms: p.t_exec,
            q_eff,
            leg_risk_qty: leg_risk,
            pnl_at_tx,
            hit: pnl_at_tx.is_some_and(|x| x > Decimal::ZERO),
        }));
    }

    fn unwind(&mut self, u: Unwind, books: &impl Books, out: &mut Vec<SimOut>) {
        let qty = u.qty.round_dp_with_strategy(2, RoundingStrategy::ToZero);
        // Below the venue minimum there is no order: the inventory is held, marked at the bid.
        if qty < self.cfg.min_order_shares {
            return;
        }
        let Some(book) = books.yes_book(&u.leg.yes_token) else {
            return;
        };
        let f = fak_sell(book, &u.leg, u.fee, qty, &mut self.consumed[u.acct]);
        out.push(SimOut::Fill(FillRow {
            account: self.accounts[u.acct].id.clone(),
            episode_id: u.episode_id.clone(),
            leg: u.leg.clone(),
            t_ms: u.due_ms,
            side: "sell",
            limit: None,
            requested: qty,
            filled: f.filled,
            vwap: f.vwap(),
            fees: f.fees,
            levels_used: f.levels_used,
        }));
        if let Some(e) = self.accounts[u.acct].sell(u.due_ms, &u.episode_id, &u.leg, &f) {
            out.push(SimOut::Ledger(e));
        }
    }

    fn convert(&mut self, c: Conversion, out: &mut Vec<SimOut>) {
        if let Some(e) =
            self.accounts[c.acct].convert(c.due_ms, &c.episode_id, &c.s_legs, &c.complements, c.q)
        {
            out.push(SimOut::Ledger(e));
        }
    }

    fn account_row(&self, a: usize, t_ms: i64, books: &impl Books) -> AccountRow {
        let acct = &self.accounts[a];
        let v = acct.valuation(|l| bid_of(books, l));
        AccountRow {
            account: acct.id.clone(),
            t_ms,
            cash: acct.cash,
            equity: acct.cash + v.marks,
            realized: acct.realized,
            unrealized: v.unrealized(),
            fees_paid: acct.fees_paid,
            locked: v.basis_open,
            max_drawdown: acct.max_drawdown,
            n_positions: i64::try_from(acct.positions.len() + acct.returned.len())
                .unwrap_or(i64::MAX),
        }
    }

    fn mark_all(&mut self, t_ms: i64, books: &impl Books, out: &mut Vec<SimOut>) {
        for a in 0..self.accounts.len() {
            let e = self.accounts[a].mark(t_ms, |l| bid_of(books, l));
            out.push(SimOut::Ledger(e));
            out.push(SimOut::Account(self.account_row(a, t_ms, books)));
        }
    }
}
