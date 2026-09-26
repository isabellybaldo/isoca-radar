//! Paper-trading glue for the scanner (S3c):
//! - the frozen policies file and its hash (spec §11 pre-registration);
//! - `SimOut` → `store` rows (one mapping for live and replay);
//! - the live `paper_state` rows (restore at start);
//! - the live resolver loop.
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
    Batch, PaperAccountRow, PaperExecRow, PaperFillRow, PaperIntentRow, PaperLedgerRow,
    PaperMarkRow, PaperMarkoutRow, PaperStateRow,
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

/// The live accounts as `paper_state` rows (exact JSON: decimals serialize as strings).
pub fn state_batch(st: &TraderState, updated_ms: i64) -> anyhow::Result<Batch> {
    let paper_state = st
        .accounts
        .iter()
        .map(|a| {
            Ok(PaperStateRow {
                account_id: a.id.clone(),
                state: serde_json::to_string(a).context("serialize paper account")?,
                updated_ms,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Batch {
        paper_state,
        ..Batch::default()
    })
}

/// Rebuild the live trader from `paper_state`. A saved account whose spec differs from the
/// policies file (same id, e.g. a new per_trade) is a different pre-registered experiment: it
/// starts fresh. `PaperTrader::restore` (S3b) already does that on its own; filtering here only
/// adds the log line. A row that isn't a saved account is an error, never a silent reset.
pub fn restore_trader(p: &Policies, rows: &[PaperStateRow]) -> anyhow::Result<PaperTrader> {
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

/// Live resolver: every `every`, and `hint_delay` after each ws hint, ask `fetch` about the
/// markets the trader holds and forward each resolution found. The engine loop captures it as
/// `Ctl::Resolved` BEFORE applying it, so replay settles identically. Nothing held → no call.
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
    loop {
        tokio::select! {
            _ = tokio::time::sleep(every) => {}
            _ = hint.notified() => { tokio::time::sleep(hint_delay).await; }
        }
        let markets = held.borrow().clone();
        if markets.is_empty() {
            continue;
        }
        match fetch(markets).await {
            Ok(found) => {
                for r in found {
                    if out.send(r).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => eprintln!("resolver: {e}"),
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

    #[test]
    fn a_saved_account_whose_spec_changed_starts_fresh() {
        let p = parse_policies(POLICIES).unwrap();
        let mut st = PaperTrader::new(p.sim.clone()).unwrap().state();
        st.accounts[0].cash = d("900");
        st.accounts[1].cash = d("800");
        let rows = state_batch(&st, 5).unwrap().paper_state;
        assert_eq!(rows.len(), st.accounts.len());
        assert!(rows.iter().all(|r| r.updated_ms == 5));
        let same = restore_trader(&p, &rows).unwrap();
        assert_eq!(
            (same.accounts()[0].cash, same.accounts()[1].cash),
            (d("900"), d("800"))
        );
        // the first account's per_trade changed: a different pre-registered experiment
        let changed =
            parse_policies(&POLICIES.replacen("per_trade = \"100\"", "per_trade = \"50\"", 1))
                .unwrap();
        let t = restore_trader(&changed, &rows).unwrap();
        assert_eq!(
            (t.accounts()[0].cash, t.accounts()[1].cash),
            (d("1000"), d("800"))
        );
        let bad = vec![store::PaperStateRow {
            account_id: "x".into(),
            state: "{not json".into(),
            updated_ms: 1,
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
}
