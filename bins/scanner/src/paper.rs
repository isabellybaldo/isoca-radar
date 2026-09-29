//! Paper-trading glue for the scanner (S3c):
//! - the frozen policies file and its hash (spec §11 pre-registration);
//! - `SimOut` → `store` rows (one mapping for live and replay);
//! - the live `paper_state` rows (restore at start);
//! - the live resolver loop;
//! - Phase 7a paper epochs: the start-time epoch, the reset-request poller and the epoch batch.
//!
//! PAPER ONLY: nothing here can place an order. `sim` produces ledger rows and nothing else.
use crate::config::PaperCfg;
use anyhow::Context;
use pm_api::gamma::Resolution;
use sha2::{Digest, Sha256};
use sim::account::Account;
use sim::config::SimConfig;
use sim::rows::SimOut;
use sim::trader::{PaperTrader, TraderState};
use std::fmt::Write as _;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use store::{
    Batch, PaperAccountRow, PaperEpochRow, PaperExecRow, PaperFillRow, PaperIntentRow,
    PaperLedgerRow, PaperMarkRow, PaperMarkoutRow, PaperStateRow,
};
use tokio::sync::{Notify, mpsc, watch};

/// After a ws `market_resolved`, wait this long before asking Gamma (its payout lags the event).
pub const RESOLVE_HINT_DELAY: Duration = Duration::from_secs(30);

/// The parsed policies file and the sha256 of its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policies {
    pub sim: SimConfig,
    pub sha256: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

pub fn parse_policies(text: &str) -> anyhow::Result<Policies> {
    let sim: SimConfig = toml::from_str(text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(Policies {
        sim,
        sha256: sha256_hex(text.as_bytes()),
    })
}

/// None when paper trading is off.
pub fn load_policies(cfg: &PaperCfg) -> anyhow::Result<Option<Policies>> {
    if !cfg.enabled {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&cfg.policies).with_context(|| format!("read {}", cfg.policies))?;
    parse_policies(&text).map(Some)
}

/// `runs.config_hash` (and every episode's): the scanner config hash, plus `+<policies sha256>`
/// when paper trading is on, so a run names the exact policies it was evaluated under.
pub fn run_hash(cfg_hash: &str, p: Option<&Policies>) -> String {
    match p {
        Some(p) => format!("{cfg_hash}+{}", p.sha256),
        None => cfg_hash.to_string(),
    }
}

pub fn with_policies(cfg: &PaperCfg, cfg_hash: &str) -> anyhow::Result<(Option<Policies>, String)> {
    let p = load_policies(cfg)?;
    let h = run_hash(cfg_hash, p.as_ref());
    Ok((p, h))
}

/// One row per configured account for this run (written before any other paper row).
pub fn account_rows(run_id: i64, p: &Policies) -> Vec<PaperAccountRow> {
    p.sim
        .accounts
        .iter()
        .map(|a| PaperAccountRow {
            run_id,
            account_id: a.id(),
            policy: a.policy.as_str().to_string(),
            scenario: a.scenario.clone(),
            fee_bips: a.fee_bips,
            bankroll: a.bankroll,
            per_trade: a.per_trade,
            policies_sha256: p.sha256.clone(),
        })
        .collect()
}

/// Map one paper-trader output onto its table (live: `Io::route`; replay: `replay_cmd`).
pub fn push_sim(run_id: i64, o: SimOut, b: &mut Batch) {
    match o {
        SimOut::Intent(r) => b.paper_intents.push(PaperIntentRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            decided_ms: r.decided_ms,
            exec_ms: r.exec_ms,
            skip_reason: r.skip_reason,
            q_target: r.q_target,
            detected_q: r.detected_q,
            detected_pnl: r.detected_pnl,
            reserve: r.reserve,
        }),
        SimOut::Exec(r) => b.paper_execs.push(PaperExecRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            exec_ms: r.exec_ms,
            q_eff: r.q_eff,
            leg_risk_qty: r.leg_risk_qty,
            pnl_at_tx: r.pnl_at_tx,
            hit: r.hit,
        }),
        SimOut::Fill(r) => b.paper_fills.push(PaperFillRow {
            run_id,
            account_id: r.account,
            episode_id: r.episode_id,
            outcome: r.leg.outcome.as_str().to_string(),
            market_id: r.leg.market_id,
            yes_token: r.leg.yes_token,
            side: r.side.to_string(),
            t_ms: r.t_ms,
            limit_price: r.limit,
            requested: r.requested,
            filled: r.filled,
            vwap: r.vwap,
            fees: r.fees,
            levels_used: i32::try_from(r.levels_used).unwrap_or(i32::MAX),
        }),
        SimOut::Ledger(e) => {
            let (market_id, yes_token, outcome) = match e.leg {
                Some(l) => (
                    Some(l.market_id),
                    Some(l.yes_token),
                    Some(l.outcome.as_str().to_string()),
                ),
                None => (None, None, None),
            };
            b.paper_ledger.push(PaperLedgerRow {
                run_id,
                account_id: e.account,
                seq: i64::try_from(e.seq).unwrap_or(i64::MAX),
                t_ms: e.t_ms,
                kind: e.kind.as_str().to_string(),
                episode_id: e.episode_id,
                market_id,
                yes_token,
                outcome,
                qty: e.qty,
                price: e.price,
                cash_delta: e.cash_delta,
                fees: e.fees,
                realized_delta: e.realized_delta,
                cash_after: e.cash_after,
                equity: e.equity,
            });
        }
        SimOut::Markout(m) => b.paper_markouts.push(PaperMarkoutRow {
            run_id,
            account_id: m.account,
            episode_id: m.episode_id,
            outcome: m.leg.outcome.as_str().to_string(),
            market_id: m.leg.market_id,
            yes_token: m.leg.yes_token,
            fill_vwap: m.fill_vwap,
            offset_ms: m.offset_ms,
            mid: m.mid,
            markout: m.markout,
        }),
        SimOut::Account(a) => b.paper_marks.push(PaperMarkRow {
            run_id,
            account_id: a.account,
            t_ms: a.t_ms,
            cash: a.cash,
            equity: a.equity,
            realized: a.realized,
            unrealized: a.unrealized,
            fees_paid: a.fees_paid,
            locked: a.locked,
            max_drawdown: a.max_drawdown,
            n_positions: a.n_positions,
        }),
    }
}

/// The live accounts as `paper_state` rows (exact JSON: decimals serialize as strings). Every save
/// carries the current policies sha256, so a restore can tell a genuine restart from a
/// policies.toml edit (ruling 26).
pub fn state_batch(
    st: &TraderState,
    updated_ms: i64,
    policies_sha256: &str,
) -> anyhow::Result<Batch> {
    let paper_state = st
        .accounts
        .iter()
        .map(|a| {
            Ok(PaperStateRow {
                account_id: a.id.clone(),
                state: serde_json::to_string(a).context("serialize paper account")?,
                updated_ms,
                policies_sha256: Some(policies_sha256.to_string()),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Batch {
        paper_state,
        ..Batch::default()
    })
}

/// Rebuild the live trader from `paper_state`. Ruling 26: the policies sha gates the whole
/// restore, not just each account's spec. Any change anywhere in policies.toml (a scenario's
/// net_ms/match_ms/burst, `[p1]` thresholds, min_order_shares, settle_delay_ms, markout offsets,
/// ...) changes the sha over the per-account AccountSpec check alone would miss, so a NULL or
/// mismatched sha on ANY loaded row starts every account fresh under the new sha, logged once.
/// Only when every row's sha matches does the narrower per-account check run: a saved account
/// whose spec differs from the policies file (same id, e.g. a new per_trade) is a different
/// pre-registered experiment and starts fresh on its own. `PaperTrader::restore` (S3b) already
/// does that; filtering here only adds the log line. A row that isn't a saved account is an
/// error, never a silent reset.
pub fn restore_trader(p: &Policies, rows: &[PaperStateRow]) -> anyhow::Result<PaperTrader> {
    if let Some(old) = rows
        .iter()
        .find(|r| r.policies_sha256.as_deref() != Some(p.sha256.as_str()))
    {
        eprintln!(
            "paper: policies changed (saved {} != current {}); all accounts start fresh",
            old.policies_sha256.as_deref().unwrap_or("none"),
            p.sha256
        );
        return PaperTrader::restore(
            p.sim.clone(),
            TraderState {
                accounts: Vec::new(),
            },
        )
        .context("restore paper trader");
    }
    let mut accounts = Vec::new();
    for r in rows {
        let a: Account = serde_json::from_str(&r.state)
            .with_context(|| format!("paper_state row {} is not a saved account", r.account_id))?;
        match p.sim.accounts.iter().find(|s| s.id() == a.id) {
            Some(spec) if *spec == a.spec => accounts.push(a),
            Some(_) => eprintln!(
                "paper: account {} changed in policies.toml since it was saved; it starts fresh",
                a.id
            ),
            None => {} // no longer configured: PaperTrader::restore would drop it anyway
        }
    }
    PaperTrader::restore(p.sim.clone(), TraderState { accounts }).context("restore paper trader")
}

/// One poll: ask `fetch` about the currently-held markets and forward each resolution found.
/// Nothing held → no call, not an error (an empty `held` watch at startup, before the first
/// `tick`, is a normal no-op). Returns `false` when `out` is gone (the caller should stop).
async fn poll_once<F, Fut>(
    fetch: &mut F,
    held: &watch::Receiver<Vec<String>>,
    out: &mpsc::Sender<Resolution>,
) -> bool
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<Vec<Resolution>, pm_api::ApiError>>,
{
    let markets = held.borrow().clone();
    if markets.is_empty() {
        return true;
    }
    match fetch(markets).await {
        Ok(found) => {
            for r in found {
                if out.send(r).await.is_err() {
                    return false;
                }
            }
        }
        Err(e) => eprintln!("resolver: {e}"),
    }
    true
}

/// Live resolver: every `every`, and `hint_delay` after each ws hint, ask `fetch` about the
/// markets the trader holds and forward each resolution found. The engine loop captures it as
/// `Ctl::Resolved` BEFORE applying it, so replay settles identically. Nothing held → no call.
///
/// Polls once immediately at startup, before waiting on `every` (S3c fix round 2): a position
/// restored from `paper_state` in a market that resolved during downtime then settles right away
/// instead of waiting up to `every`. After that it keeps the normal interval. The first poll with
/// an empty `held` watch is a no-op like every other poll, never an error, and this never spins:
/// the immediate poll runs exactly once, then every later poll is gated by the same
/// sleep/hint-notify select as before.
pub async fn run_resolver<F, Fut>(
    mut fetch: F,
    held: watch::Receiver<Vec<String>>,
    hint: Arc<Notify>,
    every: Duration,
    hint_delay: Duration,
    out: mpsc::Sender<Resolution>,
) where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<Vec<Resolution>, pm_api::ApiError>>,
{
    if !poll_once(&mut fetch, &held, &out).await {
        return;
    }
    loop {
        tokio::select! {
            _ = tokio::time::sleep(every) => {}
            _ = hint.notified() => { tokio::time::sleep(hint_delay).await; }
        }
        if !poll_once(&mut fetch, &held, &out).await {
            return;
        }
    }
}

/// Spec 7a §4.2: how often the poller asks for new reset requests (~30 s to apply one).
pub const RESET_POLL_EVERY: Duration = Duration::from_secs(30);

/// What the scanner does to the paper accounts at start (spec 7a §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartEpoch {
    /// A restart under the same policies: restore the saved accounts, no epoch row.
    Continue,
    /// An empty paper_state (a fresh database) or a policies sha mismatch (ruling 26).
    Policies,
    /// A reset request pending at start (the highest one): a fresh trader (review C3).
    Reset { request_id: i64 },
}

/// A pending request wins (one epoch, cause `reset`, even when the policies changed too); then an
/// empty `paper_state` or any row whose sha isn't the current one is a policies epoch.
pub fn start_epoch(p: &Policies, saved: &[PaperStateRow], pending: &[i64]) -> StartEpoch {
    if let Some(&request_id) = pending.iter().max() {
        return StartEpoch::Reset { request_id };
    }
    if saved.is_empty()
        || saved
            .iter()
            .any(|r| r.policies_sha256.as_deref() != Some(p.sha256.as_str()))
    {
        return StartEpoch::Policies;
    }
    StartEpoch::Continue
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Policies,
    Reset,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Policies => "policies",
            Cause::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochStart {
    pub epoch: i32,
    pub start_ms: i64,
    pub cause: Cause,
    pub request_id: Option<i64>,
}

/// One `WriterMsg::PaperReset` (spec 7a §4.2): the rows the epoch closes and opens with, its
/// `paper_epochs` row and a `paper_state` save stamped `updated_ms` — the WALL clock (the upsert
/// guard `paper_state.updated_ms <= EXCLUDED.updated_ms` would reject a paper-clock stamp behind
/// a later save tick). Never split: a crash can't leave an epoch row next to pre-reset state.
pub fn epoch_batch(
    run_id: i64,
    p: &Policies,
    e: &EpochStart,
    rows: Vec<SimOut>,
    st: &TraderState,
    updated_ms: i64,
) -> anyhow::Result<Batch> {
    let mut b = state_batch(st, updated_ms, &p.sha256)?;
    for o in rows {
        push_sim(run_id, o, &mut b);
    }
    b.paper_epochs.push(PaperEpochRow {
        epoch: e.epoch,
        start_ms: e.start_ms,
        cause: e.cause.as_str().to_string(),
        request_id: e.request_id,
        run_id,
        policies_sha256: p.sha256.clone(),
    });
    Ok(b)
}

/// The engine loop's epoch state, read ONCE at start and then kept in memory: re-reading
/// `paper_epochs` while the writer is backlogged would re-apply a request (spec 7a §4.2). Epoch
/// numbers only ever come from `next_epoch`, one past the last, so this process never builds two
/// different rows with the same epoch (paper_epochs inserts are ON CONFLICT DO NOTHING).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetTracker {
    current: i32,
    last_applied: Option<i64>,
}

impl ResetTracker {
    pub fn new(current: i32, last_applied: Option<i64>) -> Self {
        Self {
            current,
            last_applied,
        }
    }

    /// The request to apply now: the highest id above `last_applied` (several collapse into
    /// one epoch), which becomes `last_applied`. None: nothing new.
    pub fn accept(&mut self, ids: &[i64]) -> Option<i64> {
        let max = ids
            .iter()
            .copied()
            .filter(|&i| self.last_applied.is_none_or(|l| i > l))
            .max()?;
        self.last_applied = Some(max);
        Some(max)
    }

    /// The number of the epoch about to start.
    pub fn next_epoch(&mut self) -> i32 {
        self.current += 1;
        self.current
    }

    /// The one place an epoch is opened, at start or live: numbers it and logs its one line.
    /// `at_start` only changes the log line.
    pub fn open(
        &mut self,
        start_ms: i64,
        cause: Cause,
        request_id: Option<i64>,
        at_start: bool,
    ) -> EpochStart {
        let epoch = self.next_epoch();
        match (request_id, at_start) {
            (Some(r), false) => eprintln!("paper: reset → epoch {epoch} (request {r})"),
            (Some(r), true) => {
                eprintln!("paper: reset → epoch {epoch} (request {r}, pending at start)")
            }
            (None, _) => eprintln!("paper: {} → epoch {epoch}", cause.as_str()),
        }
        EpochStart {
            epoch,
            start_ms,
            cause,
            request_id,
        }
    }

    pub fn last_applied(&self) -> Option<i64> {
        self.last_applied
    }

    pub fn current(&self) -> i32 {
        self.current
    }
}

/// Spec 7a §4.2: every `every`, ask `fetch` for reset request ids above the last one seen and
/// forward them (one message per poll that found any). A spawned task, like the resolver: the
/// engine loop has no database awaits. A failed poll logs one line and waits for the next.
/// `last_seen` is seeded with the start's `last_applied` (a start-time epoch included), so a
/// request applied at start is never forwarded again. Ends when `out` is gone.
pub async fn run_reset_poller<F, Fut, E>(
    mut fetch: F,
    mut last_seen: i64,
    every: Duration,
    out: mpsc::Sender<Vec<i64>>,
) where
    F: FnMut(i64) -> Fut,
    Fut: Future<Output = Result<Vec<i64>, E>>,
    E: std::fmt::Display,
{
    loop {
        tokio::time::sleep(every).await;
        match fetch(last_seen).await {
            Ok(ids) => {
                if let Some(&max) = ids.iter().max() {
                    last_seen = last_seen.max(max);
                    if out.send(ids).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => eprintln!("paper: reset poll failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use rust_decimal::Decimal;
    use sim::account::{EntryKind, LedgerEntry};
    use sim::rows::{AccountRow, ExecRow, FillRow, IntentRow, MarkoutRow};
    use sim::types::{LegKey, Outcome};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const POLICIES: &str = include_str!("../../../config/policies.toml");

    fn d(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    #[test]
    fn repo_policies_parse_and_join_the_run_hash() {
        let p = parse_policies(POLICIES).unwrap();
        assert_eq!(p.sha256.len(), 64);
        assert_eq!(p.sha256, parse_policies(POLICIES).unwrap().sha256, "stable");
        assert_eq!(run_hash("abc", Some(&p)), format!("abc+{}", p.sha256));
        assert_eq!(run_hash("abc", None), "abc");
        let rows = account_rows(7, &p);
        assert_eq!(rows.len(), p.sim.accounts.len());
        assert!(
            rows.iter()
                .all(|r| r.run_id == 7 && r.policies_sha256 == p.sha256)
        );
        assert!(
            rows.iter()
                .any(|r| r.account_id == "P0-converter/sp-idle/fee10" && r.fee_bips == d("10"))
        );
        let zero = POLICIES.replacen("bankroll = \"1000\"", "bankroll = \"0\"", 1);
        assert!(parse_policies(&zero).is_err(), "validated, not just parsed");
    }

    fn leg() -> LegKey {
        LegKey {
            market_id: "m0".into(),
            yes_token: "y0".into(),
            outcome: Outcome::No,
        }
    }

    #[test]
    fn every_sim_row_maps_to_its_table() {
        let mut b = Batch::default();
        let rows = [
            SimOut::Intent(IntentRow {
                account: "a".into(),
                episode_id: "e".into(),
                decided_ms: 1,
                exec_ms: None,
                skip_reason: Some("no_cash".into()),
                q_target: None,
                detected_q: None,
                detected_pnl: None,
                reserve: None,
            }),
            SimOut::Exec(ExecRow {
                account: "a".into(),
                episode_id: "e".into(),
                exec_ms: 2,
                q_eff: d("0"),
                leg_risk_qty: d("0"),
                pnl_at_tx: None,
                hit: false,
            }),
            SimOut::Fill(FillRow {
                account: "a".into(),
                episode_id: "e".into(),
                leg: leg(),
                t_ms: 2,
                side: "buy",
                limit: Some(d("0.6")),
                requested: d("51.2"),
                filled: d("51.2"),
                vwap: Some(d("0.6")),
                fees: d("0"),
                levels_used: 1,
            }),
            SimOut::Ledger(LedgerEntry {
                account: "a".into(),
                seq: 4,
                t_ms: 2,
                kind: EntryKind::Buy,
                episode_id: Some("e".into()),
                leg: Some(leg()),
                qty: d("51.2"),
                price: Some(d("0.6")),
                cash_delta: d("-30.72"),
                fees: d("0"),
                realized_delta: d("0"),
                cash_after: d("969.28"),
                equity: None,
            }),
            SimOut::Markout(MarkoutRow {
                account: "a".into(),
                episode_id: "e".into(),
                leg: leg(),
                fill_vwap: d("0.6"),
                offset_ms: 1000,
                mid: None,
                markout: None,
            }),
            SimOut::Account(AccountRow {
                account: "a".into(),
                t_ms: 60_000,
                cash: d("969.28"),
                equity: d("1000"),
                realized: d("0"),
                unrealized: d("0"),
                fees_paid: d("0"),
                locked: d("30.72"),
                max_drawdown: d("0"),
                n_positions: 1,
            }),
        ];
        for r in rows {
            push_sim(3, r, &mut b);
        }
        assert_eq!(b.rows(), 6);
        assert_eq!(b.paper_intents[0].skip_reason.as_deref(), Some("no_cash"));
        assert_eq!(b.paper_execs[0].pnl_at_tx, None, "unmeasured stays NULL");
        assert_eq!(
            (
                b.paper_fills[0].outcome.as_str(),
                b.paper_fills[0].side.as_str()
            ),
            ("no", "buy")
        );
        let l = &b.paper_ledger[0];
        assert_eq!(
            (
                l.run_id,
                l.seq,
                l.kind.as_str(),
                l.outcome.as_deref(),
                l.realized_delta
            ),
            (3, 4, "buy", Some("no"), d("0"))
        );
        assert_eq!(
            (b.paper_markouts[0].offset_ms, b.paper_marks[0].n_positions),
            (1000, 1)
        );
    }

    fn saved_rows(p: &Policies) -> Vec<store::PaperStateRow> {
        let mut st = PaperTrader::new(p.sim.clone()).unwrap().state();
        st.accounts[0].cash = d("900");
        st.accounts[1].cash = d("800");
        let rows = state_batch(&st, 5, &p.sha256).unwrap().paper_state;
        assert_eq!(rows.len(), st.accounts.len());
        assert!(
            rows.iter()
                .all(|r| r.updated_ms == 5
                    && r.policies_sha256.as_deref() == Some(p.sha256.as_str()))
        );
        rows
    }

    #[test]
    fn a_matching_sha_restores_the_saved_accounts() {
        let p = parse_policies(POLICIES).unwrap();
        let rows = saved_rows(&p);
        let same = restore_trader(&p, &rows).unwrap();
        assert_eq!(
            (same.accounts()[0].cash, same.accounts()[1].cash),
            (d("900"), d("800"))
        );
    }

    #[test]
    fn a_matching_sha_with_one_changed_spec_leaves_only_that_account_fresh() {
        let p = parse_policies(POLICIES).unwrap();
        let rows = saved_rows(&p);
        // The sha still matches (a real policies.toml edit would also change it; this simulates,
        // at the unit level, the narrower per-account guard ruling 26 keeps for when the sha
        // matches), but account 0's spec no longer matches what was saved: a different
        // pre-registered experiment starts fresh on its own, account 1 is untouched.
        let mut changed = p.clone();
        changed.sim.accounts[0].per_trade = d("50");
        let t = restore_trader(&changed, &rows).unwrap();
        assert_eq!(
            (t.accounts()[0].cash, t.accounts()[1].cash),
            (d("1000"), d("800"))
        );
    }

    #[test]
    fn a_policies_sha_mismatch_starts_every_account_fresh() {
        let p = parse_policies(POLICIES).unwrap();
        let rows = saved_rows(&p);
        // Every AccountSpec is untouched: only a [p1] threshold changed. Ruling 26: the old
        // per-account-only check would have restored both accounts' cash under the new sha; it
        // must not.
        let changed = parse_policies(&POLICIES.replacen(
            "min_annualized = \"0.10\"",
            "min_annualized = \"0.20\"",
            1,
        ))
        .unwrap();
        assert_ne!(changed.sha256, p.sha256);
        assert_eq!(
            changed.sim.accounts, p.sim.accounts,
            "no AccountSpec changed"
        );
        let fresh = restore_trader(&changed, &rows).unwrap();
        assert_eq!(
            (fresh.accounts()[0].cash, fresh.accounts()[1].cash),
            (d("1000"), d("1000")),
            "a policies change anywhere starts every account fresh, not just the one whose spec moved"
        );
    }

    #[test]
    fn a_null_sha_on_any_row_starts_every_account_fresh() {
        let p = parse_policies(POLICIES).unwrap();
        let mut rows = saved_rows(&p);
        rows[0].policies_sha256 = None; // e.g. a row saved before migration 0005
        let fresh = restore_trader(&p, &rows).unwrap();
        assert_eq!(
            (fresh.accounts()[0].cash, fresh.accounts()[1].cash),
            (d("1000"), d("1000"))
        );
    }

    #[test]
    fn a_corrupt_save_is_loud_never_a_silent_reset() {
        let p = parse_policies(POLICIES).unwrap();
        let bad = vec![store::PaperStateRow {
            account_id: "x".into(),
            state: "{not json".into(),
            updated_ms: 1,
            policies_sha256: Some(p.sha256.clone()),
        }];
        assert!(
            restore_trader(&p, &bad).is_err(),
            "a corrupt save is loud, never a silent reset"
        );
    }

    fn resolved(id: &str) -> Resolution {
        Resolution {
            market_id: id.into(),
            payout_yes: Decimal::ONE,
            payout_no: Decimal::ZERO,
        }
    }

    #[tokio::test]
    async fn resolver_asks_about_held_markets_and_forwards_what_resolved() {
        let (_held_tx, held_rx) = watch::channel(vec!["m1".to_string(), "m2".to_string()]);
        let (tx, mut rx) = mpsc::channel(8);
        let asked = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
        let seen = asked.clone();
        let task = tokio::spawn(run_resolver(
            move |ids: Vec<String>| {
                seen.lock().unwrap().push(ids);
                async { Ok::<_, pm_api::ApiError>(vec![resolved("m1")]) }
            },
            held_rx,
            Arc::new(Notify::new()),
            Duration::from_millis(20),
            Duration::ZERO,
            tx,
        ));
        let r = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r, resolved("m1"));
        assert_eq!(
            asked.lock().unwrap()[0],
            vec!["m1".to_string(), "m2".to_string()]
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_ws_hint_polls_soon_without_waiting_for_the_period() {
        let (_held_tx, held_rx) = watch::channel(vec!["m1".to_string()]);
        let (tx, mut rx) = mpsc::channel(8);
        let hint = Arc::new(Notify::new());
        let task = tokio::spawn(run_resolver(
            |_ids: Vec<String>| async { Ok::<_, pm_api::ApiError>(vec![resolved("m1")]) },
            held_rx,
            hint.clone(),
            Duration::from_secs(3_600),
            Duration::from_millis(10),
            tx,
        ));
        hint.notify_one();
        let r = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.market_id, "m1");
        task.abort();
    }

    #[tokio::test]
    async fn nothing_held_means_no_gamma_calls() {
        let (_held_tx, held_rx) = watch::channel(Vec::<String>::new());
        let (tx, _rx) = mpsc::channel(8);
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let task = tokio::spawn(run_resolver(
            move |_ids: Vec<String>| {
                c.fetch_add(1, Ordering::Relaxed);
                async { Ok::<_, pm_api::ApiError>(Vec::new()) }
            },
            held_rx,
            Arc::new(Notify::new()),
            Duration::from_millis(10),
            Duration::ZERO,
            tx,
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        task.abort();
    }

    #[test]
    fn start_epoch_names_a_policies_epoch_a_pending_request_or_nothing() {
        let p = parse_policies(POLICIES).unwrap();
        let rows = saved_rows(&p);
        assert_eq!(
            start_epoch(&p, &[], &[]),
            StartEpoch::Policies,
            "a fresh database"
        );
        assert_eq!(
            start_epoch(&p, &rows, &[]),
            StartEpoch::Continue,
            "a restart; also the start after a policies epoch's first save committed, so a crash \
             loop never writes a second one"
        );
        let changed = parse_policies(&POLICIES.replacen(
            "min_annualized = \"0.10\"",
            "min_annualized = \"0.20\"",
            1,
        ))
        .unwrap();
        assert_eq!(start_epoch(&changed, &rows, &[]), StartEpoch::Policies);
        assert_eq!(
            start_epoch(&changed, &rows, &[3, 5]),
            StartEpoch::Reset { request_id: 5 },
            "new policies and a pending request at once: one reset epoch"
        );
        assert_eq!(
            start_epoch(&p, &rows, &[2]),
            StartEpoch::Reset { request_id: 2 }
        );
    }

    #[test]
    fn the_tracker_applies_each_request_once_and_collapses_a_backlog() {
        let mut r = ResetTracker::new(1, Some(4));
        assert_eq!(r.accept(&[5]), Some(5));
        assert_eq!(r.accept(&[5]), None, "never twice, even if sent again");
        assert_eq!(
            r.accept(&[]),
            None,
            "a tick with no new request does nothing"
        );
        assert_eq!(r.accept(&[3, 4]), None, "older than last_applied");
        assert_eq!(
            r.accept(&[6, 8, 7]),
            Some(8),
            "several collapse into one epoch naming the highest"
        );
        assert_eq!(r.last_applied(), Some(8));
        assert_eq!((r.next_epoch(), r.next_epoch(), r.current()), (2, 3, 3));
        let mut fresh = ResetTracker::new(0, None);
        assert_eq!(fresh.accept(&[1]), Some(1));
    }

    #[test]
    fn an_epoch_batch_carries_the_rows_the_epoch_row_and_the_wall_clock_save() {
        let p = parse_policies(POLICIES).unwrap();
        let st = PaperTrader::new(p.sim.clone()).unwrap().state();
        let mark = SimOut::Account(AccountRow {
            account: "P0-converter/rung-0".into(),
            t_ms: 11,
            cash: d("1000"),
            equity: d("1000"),
            realized: d("0"),
            unrealized: d("0"),
            fees_paid: d("0"),
            locked: d("0"),
            max_drawdown: d("0"),
            n_positions: 0,
        });
        let e = EpochStart {
            epoch: 2,
            start_ms: 11,
            cause: Cause::Reset,
            request_id: Some(9),
        };
        let b = epoch_batch(3, &p, &e, vec![mark], &st, 77).unwrap();
        assert_eq!(
            b.paper_epochs,
            vec![store::PaperEpochRow {
                epoch: 2,
                start_ms: 11,
                cause: "reset".into(),
                request_id: Some(9),
                run_id: 3,
                policies_sha256: p.sha256.clone(),
            }]
        );
        assert_eq!((b.paper_marks.len(), b.paper_marks[0].run_id), (1, 3));
        assert!(
            b.paper_state.len() == st.accounts.len()
                && b.paper_state.iter().all(|s| s.updated_ms == 77)
        );
    }

    #[tokio::test]
    async fn the_poller_forwards_new_ids_and_asks_after_the_last_seen() {
        let asked = Arc::new(std::sync::Mutex::new(Vec::<i64>::new()));
        let seen = asked.clone();
        let (tx, mut rx) = mpsc::channel(8);
        let task = tokio::spawn(run_reset_poller(
            move |after: i64| {
                seen.lock().unwrap().push(after);
                let found = if after < 6 { vec![5, 6] } else { Vec::new() };
                async move { Ok::<_, String>(found) }
            },
            4, // seeded with the request a start-time epoch already applied
            Duration::from_millis(10),
            tx,
        ));
        let ids = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ids, vec![5, 6]);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let asked = asked.lock().unwrap().clone();
        assert_eq!(
            asked[0], 4,
            "the seed: a request applied at start is never asked for again"
        );
        assert!(asked[1..].iter().all(|&a| a == 6), "{asked:?}");
        assert!(rx.try_recv().is_err(), "nothing new: nothing sent");
        task.abort();
    }

    #[tokio::test]
    async fn a_failed_poll_logs_and_the_next_one_still_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let (tx, mut rx) = mpsc::channel(8);
        let task = tokio::spawn(run_reset_poller(
            move |_after: i64| {
                let n = c.fetch_add(1, Ordering::Relaxed);
                async move {
                    if n == 0 {
                        Err("database unreachable".to_string())
                    } else {
                        Ok(vec![3])
                    }
                }
            },
            0,
            Duration::from_millis(10),
            tx,
        ));
        let ids = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ids, vec![3]);
        task.abort();
    }
}
