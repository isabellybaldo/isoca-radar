//! The paper trader (spec §5.6, §11): one account per `policies.toml` entry, each with its own
//! consumed-liquidity ledger. Pure: time arrives on the inputs (see the call contract in the plan).
use crate::SimError;
use crate::account::{Account, Valuation};
use crate::config::{PolicyKind, SimConfig};
use crate::consumed::ConsumedLiquidity;
use crate::latency::{FrameRate, exec_delay_ms};
use crate::policy::{Decision, Hold, OpenCtx, Planned, SkipReason, decide, outcome_for};
use crate::rows::{AccountRow, ExecRow, FillRow, IntentRow, MarkoutRow, SimOut};
use crate::types::{BookSide, LegKey, Outcome};
use crate::venue::{FillLeg, basket_buy, outcome_levels, unwind_sell, yes_level};
use arb_core::book::Book;
use arb_core::detect::Kind;
use arb_core::fee::FeeSchedule;
use arb_core::fill::{BasketRule, BuyLeg, walk};
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Read access to the engine's YES books: defined in `venue` (7b §3.2), re-exported here so
/// `sim::trader::Books` keeps working for the scanner.
pub use crate::venue::Books;

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

/// One item of due work, however it was queued. `due` (S3c fix round 1) keys these by
/// `(due_ms, Rank, seq)` in a single `BTreeMap`, so the earliest item across every kind is always
/// its first entry — no per-call scan of four separate queues, and no `Vec::remove(i)` shifting.
#[derive(Debug, Clone)]
enum Due {
    Pending(Pending),
    Unwind(Unwind),
    Conversion(Conversion),
    Markout(Markout),
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
    /// Every not-yet-processed pending intent, unwind, conversion and markout, keyed by
    /// `(due_ms, Rank, seq)`: the map's own key order IS the global due-time order (S3c fix
    /// round 1). `seq` alone is already unique across every kind (one shared counter), so no two
    /// entries ever collide on a key.
    due: BTreeMap<(i64, Rank, u64), Due>,
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

/// `levels` minus what `consumed` already used against them (the same mapping `pnl_now` and
/// `filtered_ctx` both need): drops any level with nothing left.
fn available_levels(
    consumed: &ConsumedLiquidity,
    key: &LegKey,
    levels: &[(Price, Shares)],
) -> Vec<(Price, Shares)> {
    levels
        .iter()
        .filter_map(|&(px, s)| {
            let avail = consumed.available(&yes_level(key, true, px), s.get());
            Shares::new(avail)
                .ok()
                .filter(|a| a.get() > Decimal::ZERO)
                .map(|a| (px, a))
        })
        .collect()
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
            due: BTreeMap::new(),
            next_mark_ms: None,
            next_seq: 0,
        })
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// Markets any account holds a position or returned YES in: what the live resolver asks
    /// Gamma about (S3c). Sorted, so callers can compare snapshots cheaply.
    pub fn held_markets(&self) -> BTreeSet<String> {
        self.accounts
            .iter()
            .flat_map(|a| a.positions.keys().chain(a.returned.keys()))
            .map(|k| k.market_id.clone())
            .collect()
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
            match self.decide_for(a, ctx) {
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
                    self.due.insert(
                        (t_exec, Rank::Pending, seq),
                        Due::Pending(Pending {
                            acct: a,
                            episode_id: ctx.episode_id.clone(),
                            kind: ctx.signal.kind,
                            planned,
                            t_exec,
                        }),
                    );
                }
            }
        }
        out
    }

    /// The earliest key due at or before `recv_ms`, across `due` and the pending minute mark
    /// (S3c fix round 1): `due`'s own first key already is its earliest item (one global
    /// timeline instead of queue-by-queue, spec §5.6; review finding on Task 8), so this is just
    /// comparing that one key against the mark's virtual `(at, Rank::Mark, 0)` — O(log n), never
    /// a scan. `None` once nothing more is due. The mark can never tie a real key: `Rank::Mark`
    /// is never a real item's rank.
    fn next_due_key(&self, recv_ms: i64, mark_at: Option<i64>) -> Option<(i64, Rank, u64)> {
        let due_key = self.due.keys().next().copied();
        let mark_key = mark_at.map(|at| (at, Rank::Mark, 0u64));
        let best = match (due_key, mark_key) {
            (Some(a), Some(b)) => Some(if a <= b { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        best.filter(|k| k.0 <= recv_ms)
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
    /// global due-time order (see `next_due_key`). Loops so that anything queued while draining
    /// (e.g. a 0 ms delay) is picked up in its correct place within this same call.
    pub fn on_frame(&mut self, recv_ms: i64, books: &impl Books) -> Vec<SimOut> {
        self.rate.push(recv_ms);
        let mut out = Vec::new();

        if self.next_mark_ms.is_none() {
            self.next_mark_ms = Some(mark_boundary_after(self.cfg.mark_every_ms, recv_ms));
        }
        let mut mark_at = self.mark_due_at(recv_ms);
        let mark_was_due = mark_at.is_some();

        while let Some(key) = self.next_due_key(recv_ms, mark_at) {
            if key.1 == Rank::Mark {
                if let Some(at) = mark_at.take() {
                    self.mark_all(at, books, &mut out);
                }
                continue;
            }
            let Some(item) = self.due.remove(&key) else {
                break; // unreachable: `key` was just read from `due` itself, above
            };
            match item {
                Due::Pending(p) => self.execute(p, books, &mut out),
                Due::Unwind(u) => self.unwind(u, books, &mut out),
                Due::Conversion(c) => self.convert(c, &mut out),
                Due::Markout(m) => {
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
            }
        }

        if mark_was_due {
            self.next_mark_ms = Some(mark_boundary_after(self.cfg.mark_every_ms, recv_ms));
        }
        out
    }

    /// A market resolved with this payout vector: settle it in every account (spec §5.6, Q19).
    /// The row's equity values whatever OTHER positions the account still holds (in markets that
    /// haven't resolved yet) at the normal bid, same as any other `AccountRow` — resolving one
    /// market must not zero out unrelated open positions in the row.
    pub fn on_resolution(
        &mut self,
        t_ms: i64,
        market_id: &str,
        payout_yes: Decimal,
        payout_no: Decimal,
        books: &impl Books,
    ) -> Vec<SimOut> {
        let mut out = Vec::new();
        for a in 0..self.accounts.len() {
            let entries = self.accounts[a].settle(t_ms, market_id, payout_yes, payout_no);
            if entries.is_empty() {
                continue;
            }
            out.extend(entries.into_iter().map(SimOut::Ledger));
            out.push(SimOut::Account(self.account_row(a, t_ms, books)));
        }
        out
    }

    /// Shutdown at `t_ms`: complete pending conversions (the on-chain step would have happened),
    /// drop pending intents (releasing their reservations), unwinds and markouts, then mark.
    /// Callers must call `on_frame(t_ms, books)` first, as usual, so anything due at or before
    /// `t_ms` has already executed at its own due time; `close` only disposes of what's still
    /// outstanding after that — it does not itself run work that `on_frame` would have.
    pub fn close(&mut self, t_ms: i64, books: &impl Books) -> Vec<SimOut> {
        let mut out = Vec::new();
        // One pass over `due` in key order: every Conversion's Rank is constant, so their
        // relative order within this pass is exactly (due_ms, seq) — the same sort the old
        // per-queue `Vec` used. Releasing pending reservations and dropping unwinds/markouts in
        // the same pass is equivalent to doing them separately: only conversions push to `out`.
        for (_, item) in std::mem::take(&mut self.due) {
            match item {
                Due::Conversion(mut c) => {
                    c.due_ms = t_ms;
                    self.convert(c, &mut out);
                }
                Due::Pending(p) => {
                    let a = &mut self.accounts[p.acct];
                    debug_assert!(
                        a.reserved >= p.planned.reserve,
                        "account {}: reserved {} < this intent's reserve {}",
                        a.id,
                        a.reserved,
                        p.planned.reserve
                    );
                    a.reserved = (a.reserved - p.planned.reserve).max(Decimal::ZERO);
                }
                Due::Unwind(_) | Due::Markout(_) => {}
            }
        }
        self.mark_all(t_ms, books, &mut out);
        out
    }

    /// Spec 7a §4.2: every account back to its bankroll IN PLACE (a paper reset). Cash,
    /// reservations, realized, fees, positions, returned YES, peak equity and drawdown restart
    /// from the account's spec; consumed liquidity, due work and the minute-mark schedule are
    /// cleared. Kept: `cfg`, `scenario_ix`, `rate` (a market measure), and each account's ledger
    /// `seq` (review C1: `paper_ledger`'s key is (run_id, account_id, seq) with ON CONFLICT DO
    /// NOTHING, so a restarted seq would silently drop the new epoch's ledger). `next_seq` only
    /// orders the now-empty due queue and carries on too. Callers `close` first.
    pub fn reset(&mut self) {
        for a in &mut self.accounts {
            let seq = a.seq;
            *a = Account::new(a.spec.clone());
            a.seq = seq;
        }
        for c in &mut self.consumed {
            *c = ConsumedLiquidity::default();
        }
        self.due.clear();
        self.next_mark_ms = None;
    }

    /// Spec 7a §4.2: mark every account at `t_ms` now (a reset's start), exactly as a minute mark
    /// would: one `mark` ledger row and one `AccountRow` each. The minute schedule is untouched.
    pub fn mark_now(&mut self, t_ms: i64, books: &impl Books) -> Vec<SimOut> {
        let mut out = Vec::new();
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
            .map(|spec| match saved.remove(&spec.id()) {
                // Same id, unchanged settings: keep the saved account, reservations released.
                Some(mut a) if a.spec == *spec => {
                    a.reserved = Decimal::ZERO;
                    a
                }
                // No saved account, or its settings changed (e.g. bankroll, per_trade): start
                // fresh, consistent with the S3c plan's ruling for a changed account.
                _ => Account::new(spec.clone()),
            })
            .collect();
        Self::with_accounts(cfg, accounts)
    }

    /// `yes_token` had a delta applied: does any account hold anything consumed on it? A single
    /// `&str`-keyed, allocation-free check per account (S3c fix round 1) before building a
    /// `LevelKey` or touching any account's ledger — replaces a global "anything consumed
    /// anywhere" flag, which stayed true for the rest of the run as soon as any account bought
    /// anything on ANY token, gating nothing per-token.
    fn any_consumed(&self, yes_token: &str) -> bool {
        self.consumed.iter().any(|c| c.has_token(yes_token))
    }

    pub fn on_level(&mut self, yes_token: &str, side: BookSide, price: Decimal, new_size: Decimal) {
        if !self.any_consumed(yes_token) {
            return;
        }
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
        if !self.any_consumed(yes_token) {
            return;
        }
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
            let levels =
                available_levels(consumed, &l.key, &outcome_levels(book, l.key.outcome, true));
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

    /// `decide` on the raw feed first, then, only for a raw `Take`, narrowed to what account
    /// `a`'s own consumed liquidity leaves available (spec §5.6, controller rulings): a real bot
    /// can't refill an order it already took unless the feed shows genuine turnover. Consumption
    /// can only explain a skip the raw feed would have taken, so every policy verdict
    /// (`NotMine`, `NotNetPositive`, P1's lock-up gates, `PairNotApproved`, `NoCash`,
    /// `BelowMinSize`, `BadInput`) keeps its own reason (S3b final review carry-over).
    fn decide_for(&self, a: usize, ctx: &OpenCtx) -> Decision {
        let acct = &self.accounts[a];
        let free = acct.free_cash();
        let raw = decide(&self.cfg, &acct.spec, free, ctx);
        if !matches!(raw, Decision::Take(_)) {
            return raw;
        }
        let Some(fctx) = self.filtered_ctx(a, ctx) else {
            // Unreachable after a raw Take: `decide` rejects the same malformed shapes
            // (mismatched lengths, a used leg missing from `ctx.legs`) as BadInput.
            return Decision::Skip(SkipReason::BadInput);
        };
        let naive = acct.spec.policy == PolicyKind::P3Naive;
        let quote = if naive {
            &fctx.signal.gross
        } else {
            &fctx.signal.quote
        };
        let edge = if naive { quote.pnl_gross } else { quote.pnl };
        let depth = quote
            .legs
            .iter()
            .map(|l| l.filled)
            .min()
            .unwrap_or(Decimal::ZERO);
        if edge <= Decimal::ZERO || depth <= Decimal::ZERO {
            return Decision::Skip(SkipReason::Consumed);
        }
        decide(&self.cfg, &acct.spec, free, &fctx)
    }

    /// `ctx.open_levels` and `signal.{quote,gross}` narrowed to what account `a`'s own consumed
    /// liquidity leaves available (the mapping `pnl_now` uses). `None` for a malformed `ctx`
    /// (mismatched lengths, or a used leg missing from `ctx.legs`).
    fn filtered_ctx(&self, a: usize, ctx: &OpenCtx) -> Option<OpenCtx> {
        let consumed = &self.consumed[a];
        let kind = ctx.signal.kind;
        let used = &ctx.signal.legs_used;
        if ctx.open_levels.len() != used.len() {
            return None;
        }
        let rule = match kind {
            Kind::NoBasket => BasketRule::NestedNo,
            _ => BasketRule::AllLegs {
                payoff: Decimal::ONE,
            },
        };
        let mut open_levels = Vec::with_capacity(used.len());
        let mut net_legs = Vec::with_capacity(used.len());
        let mut gross_legs = Vec::with_capacity(used.len());
        for (pos, (&gi, levels)) in used.iter().zip(&ctx.open_levels).enumerate() {
            let info = ctx.legs.get(gi)?;
            let key = LegKey {
                market_id: info.market_id.clone(),
                yes_token: info.yes_token.clone(),
                outcome: outcome_for(kind, pos),
            };
            let filtered = available_levels(consumed, &key, levels);
            net_legs.push(BuyLeg {
                levels: filtered.clone(),
                fee: info.fee,
            });
            gross_legs.push(BuyLeg {
                levels: filtered.clone(),
                fee: FeeSchedule::ZERO,
            });
            open_levels.push(filtered);
        }
        let mut signal = ctx.signal.clone();
        signal.quote = walk(&net_legs, rule, None);
        signal.gross = walk(&gross_legs, rule, None);
        let mut out = ctx.clone();
        out.signal = signal;
        out.open_levels = open_levels;
        Some(out)
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
        let fill_legs: Vec<FillLeg> = p
            .planned
            .legs
            .iter()
            .map(|l| FillLeg {
                key: l.key.clone(),
                fee: l.fee,
                limit: l.limit,
            })
            .collect();
        // Account::buy never touches consumed liquidity, so buying every leg first and then
        // booking each leg's rows in order is the same as the old buy-then-book loop.
        let buy = basket_buy(books, &fill_legs, p.planned.q_target, &mut self.consumed[a]);
        for (leg, f) in p.planned.legs.iter().zip(&buy.per_leg) {
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
            if let Some(e) = self.accounts[a].buy(p.t_exec, &p.episode_id, &leg.key, f) {
                out.push(SimOut::Ledger(e));
            }
        }
        let q_eff = buy.q_eff;
        let mut leg_risk = Decimal::ZERO;
        for ((leg, f), &excess) in p.planned.legs.iter().zip(&buy.per_leg).zip(&buy.excess) {
            if excess > Decimal::ZERO {
                leg_risk += excess;
                let due_ms = p.t_exec + self.delay(a, leg.seconds_delay);
                let seq = self.alloc_seq();
                self.due.insert(
                    (due_ms, Rank::Unwind, seq),
                    Due::Unwind(Unwind {
                        acct: a,
                        episode_id: p.episode_id.clone(),
                        leg: leg.key.clone(),
                        fee: leg.fee,
                        qty: excess,
                        due_ms,
                    }),
                );
            }
            if let Some(vwap) = f.vwap() {
                let offsets = self.cfg.markout_offsets_ms.clone();
                for off in offsets {
                    let seq = self.alloc_seq();
                    let due_ms = p.t_exec + off;
                    self.due.insert(
                        (due_ms, Rank::Markout, seq),
                        Due::Markout(Markout {
                            acct: a,
                            episode_id: p.episode_id.clone(),
                            leg: leg.key.clone(),
                            vwap,
                            offset_ms: off,
                        }),
                    );
                }
            }
        }
        if q_eff > Decimal::ZERO
            && let Hold::Convert { complements } = &p.planned.hold
        {
            let seq = self.alloc_seq();
            let due_ms = p.t_exec + self.cfg.settle_delay_ms;
            self.due.insert(
                (due_ms, Rank::Conversion, seq),
                Due::Conversion(Conversion {
                    acct: a,
                    episode_id: p.episode_id.clone(),
                    s_legs: p.planned.legs.iter().map(|l| l.key.clone()).collect(),
                    complements: complements.clone(),
                    q: q_eff,
                    due_ms,
                }),
            );
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
        debug_assert!(
            self.accounts[a].cash >= self.accounts[a].reserved,
            "account {}: cash {} < reserved {} after execution",
            self.accounts[a].id,
            self.accounts[a].cash,
            self.accounts[a].reserved
        );
    }

    fn unwind(&mut self, u: Unwind, books: &impl Books, out: &mut Vec<SimOut>) {
        // Capped at what we actually hold (unwind_sell rounds both): a stale unwind (e.g. a
        // position already reduced by another event) can't sell shares we don't have. Below the
        // venue minimum, or the book gone: no order goes out, but the held inventory still gets a
        // zero-fill row (side "sell", filled 0) so it stays visible — a real bot would see the
        // same held-not-sold leg risk, not silence.
        let held = self.accounts[u.acct]
            .positions
            .get(&u.leg)
            .map_or(Decimal::ZERO, |p| p.qty);
        let (qty, f) = unwind_sell(
            books.yes_book(&u.leg.yes_token),
            &u.leg,
            u.fee,
            u.qty,
            held,
            self.cfg.min_order_shares,
            &mut self.consumed[u.acct],
        );
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
        Self::row_from(acct, t_ms, v)
    }

    /// Builds an `AccountRow` from an account and an already-computed valuation. `account_row`
    /// is its only caller: marks, resolutions and the close all value open positions at the bid.
    fn row_from(acct: &Account, t_ms: i64, v: Valuation) -> AccountRow {
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
