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
}

#[derive(Debug, Clone)]
struct Unwind {
    acct: usize,
    episode_id: String,
    leg: LegKey,
    fee: FeeSchedule,
    qty: Decimal,
    due_ms: i64,
}

#[derive(Debug, Clone)]
struct Conversion {
    acct: usize,
    episode_id: String,
    s_legs: Vec<LegKey>,
    complements: Vec<LegKey>,
    q: Decimal,
    due_ms: i64,
}

#[derive(Debug, Clone)]
struct Markout {
    acct: usize,
    episode_id: String,
    leg: LegKey,
    vwap: Decimal,
    offset_ms: i64,
    due_ms: i64,
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
}

/// Remove the items due at `now`, ordered by due time (stable: insertion order breaks ties).
fn drain_due<T>(v: &mut Vec<T>, now: i64, due: impl Fn(&T) -> i64) -> Vec<T> {
    let (mut ready, rest): (Vec<T>, Vec<T>) =
        std::mem::take(v).into_iter().partition(|x| due(x) <= now);
    *v = rest;
    ready.sort_by_key(|x| due(x));
    ready
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
        })
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    fn delay(&self, acct: usize, seconds_delay: Option<i32>) -> i64 {
        let sc = &self.cfg.scenarios[self.scenario_ix[acct]];
        exec_delay_ms(sc, seconds_delay, self.rate.per_second())
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
                    self.pending.push(Pending {
                        acct: a,
                        episode_id: ctx.episode_id.clone(),
                        kind: ctx.signal.kind,
                        planned,
                        t_exec,
                    });
                }
            }
        }
        out
    }

    /// Call BEFORE applying the frame at `recv_ms`: runs everything due at or before it.
    pub fn on_frame(&mut self, recv_ms: i64, books: &impl Books) -> Vec<SimOut> {
        self.rate.push(recv_ms);
        let mut out = Vec::new();
        for p in drain_due(&mut self.pending, recv_ms, |p| p.t_exec) {
            self.execute(p, books, &mut out);
        }
        for u in drain_due(&mut self.unwinds, recv_ms, |u| u.due_ms) {
            self.unwind(u, books, &mut out);
        }
        for c in drain_due(&mut self.conversions, recv_ms, |c| c.due_ms) {
            self.convert(c, &mut out);
        }
        for m in drain_due(&mut self.markouts, recv_ms, |m| m.due_ms) {
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
        self.marks_due(recv_ms, books, &mut out);
        out
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
                self.unwinds.push(Unwind {
                    acct: a,
                    episode_id: p.episode_id.clone(),
                    leg: leg.key.clone(),
                    fee: leg.fee,
                    qty: excess,
                    due_ms,
                });
            }
            if let Some(vwap) = f.vwap() {
                for &off in &self.cfg.markout_offsets_ms {
                    self.markouts.push(Markout {
                        acct: a,
                        episode_id: p.episode_id.clone(),
                        leg: leg.key.clone(),
                        vwap,
                        offset_ms: off,
                        due_ms: p.t_exec + off,
                    });
                }
            }
        }
        if q_eff > Decimal::ZERO
            && let Hold::Convert { complements } = &p.planned.hold
        {
            self.conversions.push(Conversion {
                acct: a,
                episode_id: p.episode_id.clone(),
                s_legs: p.planned.legs.iter().map(|l| l.key.clone()).collect(),
                complements: complements.clone(),
                q: q_eff,
                due_ms: p.t_exec + self.cfg.settle_delay_ms,
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

    fn marks_due(&mut self, now: i64, books: &impl Books, out: &mut Vec<SimOut>) {
        let every = self.cfg.mark_every_ms;
        let aligned_after = |t: i64| (t.div_euclid(every) + 1) * every;
        let Some(next) = self.next_mark_ms else {
            self.next_mark_ms = Some(aligned_after(now));
            return;
        };
        if now < next {
            return;
        }
        // One mark at the latest boundary ≤ now, however long the input gap was.
        let at = now.div_euclid(every) * every;
        self.mark_all(at, books, out);
        self.next_mark_ms = Some(aligned_after(now));
    }
}
