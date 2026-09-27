//! One analyze tick (spec §7):
//!  1. read the live episodes whose updated_ms passed the cursor (a 120 s lookback covers commit
//!     order) and mark every hour from their opening to now dirty, for each family whose runs
//!     they belong to (a late close also clears the hours it no longer spans);
//!  2. a family whose premise key changed (or first start) loads its stored cells and catches up
//!     the rest from history, oldest first, 24 hours per batch, pausing between batches, status
//!     under_review meanwhile; a run that newly matches (a late `host` row) dirties its hours;
//!  3. advance the windows to the current hour, rebuild the dirty cells and the current partial
//!     hour, store them, swap them into the windows;
//!  4. write every card for 24h / 7d / 30d / all (+ today for Edge capture), the cursor and meta.
//!
//! Failure: the caller logs one line, records it in card_meta and calls `reset`, so the next tick
//! reloads the windows from card_cells (the stored cells are the truth). Results keep their values.
use cards::body::{BodyCtx, Card, Parts, ScenarioLat, base_status, make};
use cards::cells::{CellData, DurCell, Ep, G1Cell, G4Cell, HourInput, Side};
use cards::edge::EdgeCell;
use cards::family::{
    Family, LONG_WINDOWS, RunPremises, RunSpan, contiguous, current_key, hours_between,
};
use cards::sweet::SweetInput;
use cards::window::SPANS;
use cards::{DAY_MS, HOUR_MS, hour_of};
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use store::{
    CardCellRow, CardEpisode, CardResultRow, ChangedSpan, MAX_CHANGED_PAGE, MarkPoint, Store,
};

/// Re-read this much before the cursor: a transaction stamped earlier may commit later.
pub const LOOKBACK_MS: i64 = 120_000;
pub const BATCH_HOURS: i64 = 24;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    pub ms: u128,
    pub dirty_hours: usize,
    pub catch_up_hours: usize,
    pub results: usize,
}

pub struct Analyzer {
    store: Store,
    pause: Duration,
    version: String,
    scenarios: Vec<ScenarioLat>,
    g1: Family<G1Cell>,
    dur: Family<DurCell>,
    g4: Family<G4Cell>,
    edge: Family<EdgeCell>,
    cursor: Option<i64>,
    last_hour: Option<i64>,
    /// When the last successful tick ran (for late paper marks and health rows).
    last_tick_ms: Option<i64>,
}

/// The newest live run's P0 accounts: its run and each account's starting bankroll.
struct Bankrolls {
    run_id: Option<i64>,
    by_account: BTreeMap<String, Decimal>,
}

struct Env<'a> {
    store: &'a Store,
    now_ms: i64,
    now_hour: i64,
    last_hour: Option<i64>,
    last_tick_ms: Option<i64>,
    pause: Duration,
    version: &'a str,
    scenarios: &'a [ScenarioLat],
    accounts: &'a [(String, String)],
    /// Starting bankrolls, for an account's first mark (see `bankroll_for`).
    bankrolls: &'a Bankrolls,
    runs: &'a [RunPremises],
    changed: &'a [ChangedSpan],
}

impl Analyzer {
    pub async fn new(
        store: Store,
        pause: Duration,
        version: &str,
        scenarios: Vec<ScenarioLat>,
    ) -> anyhow::Result<Self> {
        let cursor = match store.card_meta_get("cursor").await? {
            Some(text) => serde_json::from_str::<serde_json::Value>(&text)?["updated_ms"].as_i64(),
            None => None,
        };
        Ok(Self {
            store,
            pause,
            version: version.to_string(),
            scenarios,
            g1: Family::new(0),
            dur: Family::new(0),
            g4: Family::new(0),
            edge: Family::new(0),
            cursor,
            last_hour: None,
            last_tick_ms: None,
        })
    }

    /// Forget the in-memory windows: the next tick reloads them from card_cells.
    pub fn reset(&mut self) {
        self.g1 = Family::new(0);
        self.dur = Family::new(0);
        self.g4 = Family::new(0);
        self.edge = Family::new(0);
        self.last_hour = None;
    }

    pub async fn write_static_meta(&self) -> anyhow::Result<()> {
        self.store
            .card_results_put(
                &[],
                &[
                    ("papers".to_string(), cards::papers::papers().to_string()),
                    (
                        "not_replicated".to_string(),
                        cards::papers::not_replicated().to_string(),
                    ),
                ],
            )
            .await?;
        Ok(())
    }

    /// Best effort: a failed tick is recorded for the page ("last tick failed …").
    pub async fn record_error(&self, now_ms: i64, text: &str) {
        let v = json!({ "ms": now_ms, "text": text }).to_string();
        if let Err(e) = self.store.card_meta_put("last_error", &v).await {
            eprintln!("analyze: could not record the error: {e}");
        }
    }

    async fn changed(&self) -> anyhow::Result<(Vec<ChangedSpan>, Option<i64>)> {
        let Some(cursor) = self.cursor else {
            // first start: catch-up reads the whole history; start the cursor at its end
            return Ok((
                Vec::new(),
                Some(self.store.max_episode_updated_ms().await?.unwrap_or(0)),
            ));
        };
        let (mut after_ms, mut after_id) = (cursor - LOOKBACK_MS, String::new());
        let (mut out, mut max) = (Vec::new(), cursor);
        loop {
            let page = self
                .store
                .changed_spans(after_ms, &after_id, MAX_CHANGED_PAGE)
                .await?;
            let Some(last) = page.last() else { break };
            after_ms = last.updated_ms;
            after_id = last.episode_id.clone();
            max = max.max(after_ms);
            let full = i64::try_from(page.len()).unwrap_or(0) >= MAX_CHANGED_PAGE;
            out.extend(page);
            if !full {
                break;
            }
        }
        Ok((out, Some(max)))
    }

    pub async fn tick(&mut self, now_ms: i64) -> anyhow::Result<TickReport> {
        let started = Instant::now();
        let runs: Vec<RunPremises> = self
            .store
            .premise_runs()
            .await?
            .into_iter()
            .map(|r| RunPremises {
                span: RunSpan {
                    run_id: r.run_id,
                    started_ms: r.started_ms,
                    end_ms: r.end_ms,
                },
                versions: r.premises.into_iter().collect(),
            })
            .collect();
        let (changed, cursor) = self.changed().await?;
        let accounts = self.store.p0_accounts().await?;
        let now_accounts = self.store.paper_accounts_now().await?;
        let bankrolls = Bankrolls {
            run_id: now_accounts.first().map(|a| a.run_id),
            by_account: now_accounts
                .into_iter()
                .map(|a| (a.account_id, a.bankroll))
                .collect(),
        };
        let env = Env {
            store: &self.store,
            now_ms,
            now_hour: hour_of(now_ms),
            last_hour: self.last_hour,
            last_tick_ms: self.last_tick_ms,
            pause: self.pause,
            version: &self.version,
            scenarios: &self.scenarios,
            accounts: &accounts,
            bankrolls: &bankrolls,
            runs: &runs,
            changed: &changed,
        };
        let mut results = Vec::new();
        let mut meta = Vec::new();
        let mut report = TickReport::default();
        sync(&env, &mut self.g1, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.dur, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.g4, &mut results, &mut meta, &mut report).await?;
        sync(&env, &mut self.edge, &mut results, &mut meta, &mut report).await?;
        report.ms = started.elapsed().as_millis();
        meta.push((
            "tick".to_string(),
            json!({ "last_tick_ms": now_ms, "analyze_version": self.version, "tick_ms": report.ms }).to_string(),
        ));
        meta.push(("last_error".to_string(), "null".to_string()));
        if let Some(c) = cursor {
            meta.push(("cursor".to_string(), json!({ "updated_ms": c }).to_string()));
        }
        self.store.card_results_put(&results, &meta).await?;
        report.results = results.len();
        self.cursor = cursor;
        self.last_hour = Some(env.now_hour);
        self.last_tick_ms = Some(now_ms);
        Ok(report)
    }
}

async fn sync<C: Card>(
    env: &Env<'_>,
    fam: &mut Family<C>,
    results: &mut Vec<CardResultRow>,
    meta: &mut Vec<(String, String)>,
    report: &mut TickReport,
) -> anyhow::Result<()> {
    let Some((key, matched)) = current_key(C::AREAS, env.runs) else {
        fam.key = None; // no run recorded these premises yet: no result row (the page says so)
        return Ok(());
    };
    let matched: BTreeMap<i64, RunSpan> = matched.into_iter().map(|r| (r.run_id, r)).collect();
    let mut dirty: BTreeSet<i64> = BTreeSet::new();
    if fam.key.as_deref() != Some(key.as_str()) {
        start_key(env, fam, &key, matched, report).await?;
    } else {
        let since = fam.since_ms();
        let mut earlier = false;
        for r in matched
            .values()
            .filter(|r| !fam.runs.contains_key(&r.run_id))
        {
            dirty.extend(hours_between(
                r.started_ms,
                r.end_ms.unwrap_or(env.now_ms).min(env.now_ms),
            ));
            earlier |= since.is_none_or(|s| r.started_ms < s);
        }
        if earlier {
            // a late-matching run that starts before the key's data did: the windows never loaded
            // its hours, so reload them from card_cells (same key: never under_review, F17)
            start_key(env, fam, &key, matched, report).await?;
        } else {
            fam.runs = matched;
        }
    }
    let needed = fam.windows.needed_ranges(env.now_hour);
    if !needed.is_empty() {
        let stored = load_ranges::<C>(env, &key, &needed).await?;
        fam.windows.advance(env.now_hour, &stored);
    }
    for s in env
        .changed
        .iter()
        .filter(|s| fam.runs.contains_key(&s.run_id))
    {
        dirty.extend(hours_between(s.opened_ms, env.now_ms));
    }
    // the current partial hour, and every hour since the last tick (the one it left partial, and
    // any a slow or failed tick skipped)
    dirty.insert(env.now_hour);
    if let Some(last) = env.last_hour.filter(|l| *l < env.now_hour) {
        dirty.extend(hours_between(last, env.now_hour));
    }
    // paper marks and health rows have no updated_ms cursor, and a row stamped late in the hour
    // that just closed may commit after its rebuild: rebuild that hour again while within the
    // lookback of the hour, and on the first tick after one that ran within it
    if C::EDGE_INPUTS
        && (env.now_ms - env.now_hour < LOOKBACK_MS
            || env
                .last_tick_ms
                .is_some_and(|t| t < env.now_hour + LOOKBACK_MS))
    {
        dirty.insert(env.now_hour - HOUR_MS);
    }
    let since_hour = fam.since_ms().map_or(env.now_hour, hour_of);
    dirty.retain(|h| *h >= since_hour && *h <= env.now_hour);
    report.dirty_hours += dirty.len();
    for group in contiguous(&dirty, BATCH_HOURS) {
        let built = build_hours(env, fam, &group).await?;
        let olds = applied_cells(env, fam, &key, &group).await?;
        put_cells(env, &key, &built).await?;
        for (h, c) in &built {
            fam.windows.apply(*h, olds.get(h), c);
        }
    }
    if fam.windows.needs_rebuild() {
        // a window saw a cell that did not match what it had applied (release builds only; a
        // debug build asserts): rebuild this key's windows from card_cells, the stored truth
        eprintln!(
            "analyze: {} windows disagreed with card_cells; rebuilding them",
            C::FAMILY
        );
        let runs = fam.runs.clone();
        start_key(env, fam, &key, runs, report).await?;
    }
    let long = fam.long.due(env.now_hour);
    results.extend(family_results(env, fam, &key, long)?);
    fam.long.mark(long, env.now_hour);
    meta.push((
        format!("fam:{}", C::FAMILY),
        json!({ "key": key, "next_hour": env.now_hour, "done": true }).to_string(),
    ));
    Ok(())
}

/// (Re)load a key's windows: load what is stored for it, then build the hours after that. The
/// status is under_review only while a changed (or first-seen) key catches up, or while a catch-up
/// that was cut short resumes; a restart under the same key rebuilds its downtime quietly (F17).
async fn start_key<C: Card>(
    env: &Env<'_>,
    fam: &mut Family<C>,
    key: &str,
    runs: BTreeMap<i64, RunSpan>,
    report: &mut TickReport,
) -> anyhow::Result<()> {
    *fam = Family::new(env.now_hour);
    fam.key = Some(key.to_string());
    fam.runs = runs;
    let since_hour = fam.since_ms().map_or(env.now_hour, hour_of);
    let meta_key = format!("fam:{}", C::FAMILY);
    let stored: Option<serde_json::Value> = match env.store.card_meta_get(&meta_key).await? {
        Some(text) => Some(serde_json::from_str(&text)?),
        None => None,
    };
    let (resume, catching_up) = match stored {
        Some(m) if m["key"].as_str() == Some(key) => (
            // max then min, never clamp: a clock that went back puts since_hour past now (F10)
            m["next_hour"]
                .as_i64()
                .unwrap_or(since_hour)
                .max(since_hour)
                .min(env.now_hour + HOUR_MS),
            m["done"].as_bool() != Some(true),
        ),
        _ => (since_hour, true),
    };
    let mut h = since_hour;
    while h < resume {
        let end = (h + BATCH_HOURS * HOUR_MS).min(resume);
        for (hh, text) in env.store.card_cells_range(C::FAMILY, key, h, end).await? {
            fam.windows.apply(hh, None, &C::from_json(&text)?);
        }
        h = end;
    }
    if resume > env.now_hour {
        return Ok(());
    }
    fam.catching_up = catching_up;
    if catching_up {
        // every window, under_review, and not marked rendered: the tick's end renders them again
        env.store
            .card_results_put(&family_results(env, fam, key, [true, true])?, &[])
            .await?;
    }
    let mut h = resume;
    while h <= env.now_hour {
        let end = (h + BATCH_HOURS * HOUR_MS).min(env.now_hour + HOUR_MS);
        let hours: Vec<i64> = (0..)
            .map(|i| h + i * HOUR_MS)
            .take_while(|x| *x < end)
            .collect();
        let built = build_hours(env, fam, &hours).await?;
        put_cells(env, key, &built).await?;
        for (hh, c) in &built {
            // never applied before (only hours < resume were loaded): old = None
            fam.windows.apply(*hh, None, c);
        }
        report.catch_up_hours += built.len();
        h = end;
        if catching_up {
            env.store
                .card_meta_put(
                    &meta_key,
                    &json!({ "key": key, "next_hour": h, "done": false }).to_string(),
                )
                .await?;
        }
        if h <= env.now_hour && !env.pause.is_zero() {
            tokio::time::sleep(env.pause).await;
        }
    }
    fam.catching_up = false;
    Ok(())
}

async fn load_ranges<C: CellData>(
    env: &Env<'_>,
    key: &str,
    ranges: &[(i64, i64)],
) -> anyhow::Result<BTreeMap<i64, C>> {
    let mut out = BTreeMap::new();
    for (from, to) in ranges {
        for (h, text) in env
            .store
            .card_cells_range(C::FAMILY, key, *from, *to)
            .await?
        {
            out.insert(h, C::from_json(&text)?);
        }
    }
    Ok(out)
}

/// The versions of `hours` currently in the windows (= stored, for hours ≤ now_hour).
async fn applied_cells<C: CellData>(
    env: &Env<'_>,
    fam: &Family<C>,
    key: &str,
    hours: &[i64],
) -> anyhow::Result<BTreeMap<i64, C>> {
    let mut out = BTreeMap::new();
    let mut missing = Vec::new();
    for h in hours.iter().filter(|h| **h <= fam.windows.now_hour) {
        match fam.windows.recent.get(h) {
            Some(c) => {
                out.insert(*h, c.clone());
            }
            None => missing.push(*h),
        }
    }
    for (h, text) in env.store.card_cells_at(C::FAMILY, key, &missing).await? {
        out.insert(h, C::from_json(&text)?);
    }
    Ok(out)
}

async fn put_cells<C: CellData>(
    env: &Env<'_>,
    key: &str,
    built: &[(i64, C)],
) -> anyhow::Result<()> {
    let mut rows = Vec::with_capacity(built.len());
    for (h, c) in built {
        rows.push(CardCellRow {
            card: C::FAMILY.to_string(),
            premise_key: key.to_string(),
            hour_ms: *h,
            data: c.to_json()?,
            computed_ms: env.now_ms,
        });
    }
    env.store.card_cells_put(&rows).await?;
    Ok(())
}

fn to_ep<C: CellData>(e: CardEpisode, fam: &Family<C>, now_ms: i64) -> Option<Ep> {
    let side = Side::from_kind(&e.kind)?;
    let end_ms = e
        .closed_ms
        .unwrap_or_else(|| fam.run_end(e.run_id, now_ms))
        .max(e.opened_ms);
    Some(Ep {
        id: e.episode_id,
        run_id: e.run_id,
        side,
        opened_ms: e.opened_ms,
        closed_ms: e.closed_ms,
        end_ms,
        duration_ms: e.duration_ms,
        censor_left: e.censor_left,
        censor_right: e.censor_right,
        censor_gap: e.censor_gap,
        edge_net: e.edge_net,
        edge_gross: e.edge_gross,
        pnl_net: e.pnl_net,
        pnl_gross: e.pnl_gross,
        cost: e.cost,
        budget100: e.budget100,
        suspect: e.suspect,
        stale: e.stale,
        prev_gap_ms: e.prev_gap_ms,
    })
}

type Latency = BTreeMap<i64, Vec<i64>>;
type Deltas = BTreeMap<i64, Vec<(String, Decimal)>>;

/// Edge capture's inputs for [from, to): measured latency per minute (lag p50 + RTT p50 / 2, both
/// present) and the equity change between consecutive live marks of each P0 account whose current
/// mark is in a run of this key. The previous mark may belong to another key's run: when both runs
/// have the same policies the account was restored, so the delta telescopes across keys (booking
/// the downtime revaluation into this key's first hour); other policies count from the bankroll;
/// unknown policies skip the mark. An account's very first mark counts against its bankroll.
async fn edge_inputs<C: CellData>(
    env: &Env<'_>,
    fam: &Family<C>,
    run_ids: &[i64],
    from: i64,
    to: i64,
) -> anyhow::Result<(Latency, Deltas)> {
    let mut lat = Latency::new();
    for m in env.store.health_latency(run_ids, from, to).await? {
        if let (Some(l), Some(r)) = (m.lag_p50_ms, m.rest_rtt_p50_ms) {
            lat.entry(hour_of(m.minute_ms))
                .or_default()
                .push(i64::from(l) + i64::from(r) / 2);
        }
    }
    let scenario: BTreeMap<&str, &str> = env
        .accounts
        .iter()
        .map(|(a, s)| (a.as_str(), s.as_str()))
        .collect();
    let ids: Vec<String> = env.accounts.iter().map(|(a, _)| a.clone()).collect();
    let marks = env.store.marks_with_prev(&ids, from, to).await?;
    // each run's policies, as the config part of its `sim` premise version (see `policies_of`)
    let sim: BTreeMap<i64, &str> = env
        .runs
        .iter()
        .filter_map(|r| {
            r.versions
                .get("sim")
                .and_then(|v| policies_of(v))
                .map(|p| (r.span.run_id, p))
        })
        .collect();
    let mut deltas = Deltas::new();
    let mut prev: Option<&MarkPoint> = None;
    for m in &marks {
        let p = prev.filter(|p| p.account_id == m.account_id);
        prev = Some(m);
        if m.t_ms < from || !fam.runs.contains_key(&m.run_id) {
            continue;
        }
        let Some(s) = scenario.get(m.account_id.as_str()) else {
            continue;
        };
        let fresh = || bankroll_for(env, &sim, m.run_id, &m.account_id).map(|b| m.equity - b);
        let delta = match p {
            Some(p) if fam.runs.contains_key(&p.run_id) => Some(m.equity - p.equity),
            // the previous mark is another premise key's run. Scanner `restore_trader` starts
            // every account fresh only when the policies file changed: other policies -> this
            // first mark counts from the bankroll; the same policies (e.g. a `sim` code bump) ->
            // the account was restored, so it telescopes from its previous mark; unknown -> skip
            Some(p) => match (sim.get(&p.run_id), sim.get(&m.run_id)) {
                (Some(a), Some(b)) if a != b => fresh(),
                (Some(_), Some(_)) => Some(m.equity - p.equity),
                _ => None,
            },
            // no mark before this one, ever: the account began inside the window, so its first
            // delta is against its starting bankroll (never the absolute equity)
            None => fresh(),
        };
        let Some(delta) = delta else { continue };
        deltas
            .entry(hour_of(m.t_ms))
            .or_default()
            .push((s.to_string(), delta));
    }
    Ok((lat, deltas))
}

/// A `sim` premise version is `{code}+{sha8(sim_section(policies_sha256))}` (arb-core
/// `effective_version`, scanner `premises::section`): the part after the last `+` changes with
/// the policies file and only with it, while the code part also moves on a `sim` code bump that
/// keeps the policies (and the accounts, which `restore_trader` then restores). The store exposes
/// no run's policies_sha256 to analyze, so this suffix is the exact comparison available
/// (a 32-bit hash: two different policies files colliding is ~2^-32).
fn policies_of(sim_version: &str) -> Option<&str> {
    sim_version.rsplit_once('+').map(|(_, config)| config)
}

/// The starting bankroll of `account_id` in run `run_id`. A run's paper_accounts rows come from
/// its policies file (scanner `account_rows`), so two runs with the same policies (`policies_of`
/// their `sim` versions) have the same bankroll per account id. The store gives the newest live
/// run's rows, so this is known exactly when `run_id` is that run or has its policies; otherwise
/// None (the delta is skipped, never guessed).
fn bankroll_for(
    env: &Env<'_>,
    sim: &BTreeMap<i64, &str>,
    run_id: i64,
    account_id: &str,
) -> Option<Decimal> {
    let newest = env.bankrolls.run_id?;
    let same = run_id == newest
        || sim
            .get(&run_id)
            .is_some_and(|v| sim.get(&newest) == Some(v));
    if !same {
        return None;
    }
    env.bankrolls.by_account.get(account_id).copied()
}

async fn build_hours<C: CellData>(
    env: &Env<'_>,
    fam: &Family<C>,
    hours: &[i64],
) -> anyhow::Result<Vec<(i64, C)>> {
    let (Some(first), Some(last)) = (hours.first(), hours.last()) else {
        return Ok(Vec::new());
    };
    let (from, to) = (*first, *last + HOUR_MS);
    let run_ids = fam.run_ids();
    let eps: Vec<Ep> = env
        .store
        .card_episodes(&run_ids, from, to)
        .await?
        .into_iter()
        .filter_map(|e| to_ep(e, fam, env.now_ms))
        .collect();
    let (lat, deltas) = if C::EDGE_INPUTS {
        edge_inputs(env, fam, &run_ids, from, to).await?
    } else {
        (Latency::new(), Deltas::new())
    };
    let (no_lat, no_deltas): (Vec<i64>, Vec<(String, Decimal)>) = (Vec::new(), Vec::new());
    Ok(hours
        .iter()
        .map(|h| {
            let inp = HourInput {
                hour_ms: *h,
                episodes: &eps,
                latency_ms: lat.get(h).map_or(&no_lat[..], |v| &v[..]),
                equity_deltas: deltas.get(h).map_or(&no_deltas[..], |v| &v[..]),
                // Task 8 fills the real SWEET inputs; SWEET isn't in LONG_WINDOWS/Family yet.
                sweet: SweetInput::default(),
            };
            (*h, C::build(&inp))
        })
        .collect())
}

/// The result rows of one family: 24h and 7d (and Edge capture's today) every time; 30d and all
/// only where `long` says (ruling T9 (c): at most hourly, see `LongRender`). A window left out
/// keeps its previous row and that row's own computed_ms. `all` reuses 30d's parts when no
/// applied hour precedes the 30d window (the two sums are then the same cells).
fn family_results<C: Card>(
    env: &Env<'_>,
    fam: &Family<C>,
    key: &str,
    long: [bool; 2],
) -> anyhow::Result<Vec<CardResultRow>> {
    let since = fam.since_ms();
    let mut out = Vec::new();
    let ctx_for = |win: &'static str, start: Option<i64>| {
        let from = match (start, since) {
            (Some(s), Some(k)) => Some(s.max(k)),
            (None, k) => k,
            (s, None) => s,
        };
        BodyCtx {
            win,
            base: base_status(fam.catching_up, since, start),
            premise_key: key,
            analyze_version: env.version,
            from_ms: from,
            to_ms: Some(env.now_ms),
            since_ms: since,
            computed_ms: env.now_ms,
            scenarios: env.scenarios,
            // Task 8 wires the real YES hurdle (P1's min_annualized from policies.toml).
            hurdle: None,
        }
    };
    let mut emit = |parts: Vec<Parts>, ctx: &BodyCtx<'_>| -> anyhow::Result<()> {
        for p in parts {
            let body = make(p, ctx);
            out.push(CardResultRow {
                card: body.card.clone(),
                win: ctx.win.to_string(),
                premise_key: key.to_string(),
                status: body.status.as_str().to_string(),
                body: serde_json::to_string(&body)?,
                computed_ms: env.now_ms,
            });
        }
        Ok(())
    };
    let mut parts_30d: Option<Vec<Parts>> = None;
    for (i, (name, span)) in SPANS.iter().enumerate() {
        let is_30d = *name == LONG_WINDOWS[0];
        let start = fam.windows.span_start(*span);
        // nothing applied before the 30d window: all's sum is these very cells
        let all_is_30d = is_30d && fam.windows.maxes.range(..start).next().is_none();
        if is_30d && !long[0] && !(long[1] && all_is_30d) {
            continue;
        }
        let ctx = ctx_for(name, Some(start));
        let parts = C::parts(&fam.windows.spans[i], &fam.windows.max_from(start), &ctx);
        if is_30d {
            if long[0] {
                emit(parts.clone(), &ctx)?;
            }
            if all_is_30d {
                parts_30d = Some(parts);
            }
        } else {
            emit(parts, &ctx)?;
        }
    }
    if long[1] {
        let ctx = ctx_for("all", None);
        let parts = match parts_30d {
            Some(p) => p,
            None => C::parts(&fam.windows.all, &fam.windows.max_from(i64::MIN), &ctx),
        };
        emit(parts, &ctx)?;
    }
    if C::EDGE_INPUTS {
        let day = env.now_ms - env.now_ms.rem_euclid(DAY_MS);
        let today = fam.windows.today(day);
        let ctx = ctx_for("today", Some(day));
        emit(C::parts(&today, &fam.windows.max_from(day), &ctx), &ctx)?;
    }
    Ok(out)
}
