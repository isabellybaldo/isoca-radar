//! `run` orchestration (spec §8.1.2, §8.2) and the small commands.
use crate::capture::{self, CaptureMsg, Ctl, Line, UniverseRecord, book_ctl};
use crate::checks::{self, CheckCmd, Limiter};
use crate::config::Config;
use crate::engine::{Engine, EngineCtx, Out, RecheckReq};
use crate::feed::{FeedMsg, Supervisor};
use crate::metrics::Metrics;
use crate::replay::replay_dir;
use crate::shards::{self, Plan};
use crate::universe::{ConnMap, Universe};
use crate::writer::{self, WriterMsg};
use crate::{now_ms, version};
use anyhow::Context;
use pm_api::backoff::ConnectBudget;
use pm_api::clob::ClobClient;
use pm_api::gamma::GammaClient;
use pm_api::ws::parse_frame;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use store::{Batch, IntegrityRow, NewRun, Store};
use tokio::sync::{mpsc, oneshot, watch};

pub fn breaker_trips(recent_starts: i64, max_starts: i64) -> bool {
    recent_starts >= max_starts
}

pub fn warm_start_usable(taken_ms: i64, now_ms: i64, max_age_ms: i64) -> bool {
    taken_ms <= now_ms && now_ms - taken_ms < max_age_ms
}

/// Graceful-stop budget for `run_live` from the shutdown signal (S3a). With END_RUN_TIMEOUT for
/// the `runs` exit row, the whole stop fits compose's 30 s stop_grace_period.
pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(20);
pub const END_RUN_TIMEOUT: Duration = Duration::from_secs(5);

/// Run one shutdown step, but never past `deadline` (a ready step still completes). An overrun
/// is logged and the step abandoned.
pub async fn by_deadline<F: std::future::Future>(
    deadline: tokio::time::Instant,
    step: &str,
    f: F,
) -> Option<F::Output> {
    match tokio::time::timeout_at(deadline, f).await {
        Ok(v) => Some(v),
        Err(_) => {
            eprintln!("shutdown: {step} overran the deadline; skipped");
            None
        }
    }
}

/// Wait for a std thread to finish without joining it, never past `deadline`. Returns whether it
/// finished (then `join` returns at once). An unfinished thread doesn't keep the process alive.
pub async fn wait_thread<T>(
    h: &std::thread::JoinHandle<T>,
    deadline: tokio::time::Instant,
) -> bool {
    while !h.is_finished() {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

#[derive(Debug)]
pub enum EngineCmd {
    Apply {
        universe: Arc<Universe>,
        conn_of: HashMap<String, usize>,
        ack: oneshot::Sender<()>,
    },
    Rates(oneshot::Sender<HashMap<String, u64>>),
}

/// Integrity row for a unit above the Phase 0 per-connection limit (never subscribed).
pub fn oversize_row(run_id: i64, ts_ms: i64, key: &str, tokens: usize) -> IntegrityRow {
    IntegrityRow {
        run_id,
        ts_ms,
        kind: "unit_oversize".into(),
        shard: None,
        token: None,
        event_id: Some(key.to_string()),
        detail: Some(format!(
            "{tokens} tokens > {} per connection (Phase 0); not subscribed",
            shards::MAX_UNIT_TOKENS
        )),
    }
}

fn new_run(cfg_hash: &str, mode: &str) -> NewRun {
    NewRun {
        service: "scanner".into(),
        started_ms: now_ms(),
        version: version().to_string(),
        config_hash: cfg_hash.to_string(),
        mode: mode.to_string(),
    }
}

fn engine_ctx(cfg: &Config, cfg_hash: &str, run_id: i64, source: &str) -> EngineCtx {
    EngineCtx {
        run_id,
        source: source.to_string(),
        scanner_version: version().to_string(),
        config_hash: cfg_hash.to_string(),
        gap_ms: cfg.engine.gap_ms,
        stale_lag_ms: cfg.engine.stale_lag_ms,
        suspect_window_ms: cfg.engine.suspect_window_ms,
        l1_suspect_window_ms: cfg.engine.l1_suspect_window_ms,
        drift_settle_ms: cfg.engine.drift_settle_ms,
        issue_throttle_ms: cfg.engine.issue_throttle_ms,
    }
}

async fn fetch_universe(cfg: &Config) -> anyhow::Result<Universe> {
    let events = GammaClient::new()?
        .all_active_events()
        .await
        .context("gamma universe")?;
    Ok(Universe::build(
        &events,
        now_ms(),
        cfg.universe.canary_pairs,
    ))
}

async fn persist_universe(store: &Store, u: &Universe) -> anyhow::Result<()> {
    let n = i32::try_from(u.groups.len()).unwrap_or(i32::MAX);
    store
        .save_universe(u.taken_ms, n, &u.to_json()?)
        .await
        .context("save_universe")?;
    let (events, legs) = u.store_rows();
    store
        .upsert_universe(&events, &legs, u.taken_ms)
        .await
        .context("upsert_universe")?;
    store.mark_gone(u.taken_ms).await.context("mark_gone")?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub async fn run(cfg: Config, cfg_hash: String) -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL (role scanner_rw) is not set")?;
    let store = Store::connect(&url, 5).await?;
    let starts = store
        .starts_since("scanner", now_ms() - cfg.breaker.window_secs * 1000)
        .await?;
    if breaker_trips(starts, cfg.breaker.max_starts) {
        // Safe mode must always exit 0 (compose `on-failure` must not restart a clean exit), so
        // nothing after this point may use `?`: a failure recording the run is logged, not raised.
        let reason = format!(
            "safe mode: {starts} starts in the last {} s (crash-loop breaker); no connections made. \
             Fix the cause, then `scanner breaker reset`",
            cfg.breaker.window_secs
        );
        match store.start_run(&new_run(&cfg_hash, "safe")).await {
            Ok(id) => {
                if let Err(e) = store.end_run(id, now_ms(), &reason).await {
                    eprintln!("scanner: failed to record safe-mode run exit: {e:#}");
                }
            }
            Err(e) => eprintln!("scanner: failed to record safe-mode run start: {e:#}"),
        }
        eprintln!("scanner: {reason}");
        return Ok(()); // exit 0: compose `on-failure` does not restart a clean exit
    }
    let run_id = store.start_run(&new_run(&cfg_hash, "live")).await?;
    let result = run_live(&cfg, &cfg_hash, &store, run_id).await;
    let reason = match &result {
        Ok(()) => "shutdown".to_string(),
        Err(e) => format!("error: {e:#}"),
    };
    let _ = tokio::time::timeout(END_RUN_TIMEOUT, store.end_run(run_id, now_ms(), &reason)).await;
    result
}

/// Bounded per-minute samples for the frame-timing log (S3a-2 final review item 1). Keeps at
/// most `CAP` values; once full, the stride doubles so later samples are thinned instead of
/// growing the vector, keeping memory bounded over a whole busy minute.
struct SampleBuf {
    buf: Vec<u32>,
    stride: u32,
    skip: u32,
}

impl SampleBuf {
    const CAP: usize = 200_000;

    fn new() -> Self {
        Self {
            buf: Vec::new(),
            stride: 1,
            skip: 0,
        }
    }

    fn push(&mut self, v: u32) {
        if self.skip == 0 {
            self.buf.push(v);
            if self.buf.len() >= Self::CAP {
                self.stride *= 2;
            }
        }
        self.skip = (self.skip + 1) % self.stride;
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.stride = 1;
        self.skip = 0;
    }
}

/// p50/p99/max of `xs` (µs), by integer index after a sort; no floats. Empty input → all zero.
/// A pure function over a slice, per the S3a-2 final review brief (item 1).
fn percentiles_us(xs: &[u32]) -> (u32, u32, u32) {
    if xs.is_empty() {
        return (0, 0, 0);
    }
    let mut v = xs.to_vec();
    v.sort_unstable();
    let at = |p: usize| v[(v.len() - 1) * p / 100];
    (at(50), at(99), v[v.len() - 1])
}

/// Per-frame `parse_frame`/`engine.on_frame` timing (`Instant`, live loop only; the engine stays
/// pure). Accumulates in integer µs, and once a minute has passed since the last log, prints one
/// summary line and resets.
struct FrameTiming {
    parse: SampleBuf,
    on_frame: SampleBuf,
    frames: u64,
    last_log_ms: i64,
}

impl FrameTiming {
    fn new(now: i64) -> Self {
        Self {
            parse: SampleBuf::new(),
            on_frame: SampleBuf::new(),
            frames: 0,
            last_log_ms: now,
        }
    }

    /// Record one frame's parse time and, if parsing succeeded, its `on_frame` time.
    fn record(&mut self, parse_us: u32, on_frame_us: Option<u32>, now: i64) {
        self.parse.push(parse_us);
        if let Some(v) = on_frame_us {
            self.on_frame.push(v);
        }
        self.frames += 1;
        if now - self.last_log_ms < 60_000 {
            return;
        }
        let (p50p, p99p, maxp) = percentiles_us(&self.parse.buf);
        let (p50o, p99o, maxo) = percentiles_us(&self.on_frame.buf);
        eprintln!(
            "engine: frames={} parse_us p50={p50p} p99={p99p} max={maxp} on_frame_us p50={p50o} p99={p99o} max={maxo}",
            self.frames
        );
        self.parse.clear();
        self.on_frame.clear();
        self.frames = 0;
        self.last_log_ms = now;
    }
}

struct Io {
    writer: mpsc::Sender<WriterMsg>,
    capture: mpsc::Sender<CaptureMsg>,
    recheck: mpsc::Sender<RecheckReq>,
    resnap: mpsc::Sender<String>,
    refresh: Arc<tokio::sync::Notify>,
    metrics: Arc<Metrics>,
    run_id: i64,
    l1_window_ms: i64,
    suspect_window_ms: i64,
}

impl Io {
    fn capture(&self, line: Line) {
        if self.capture.try_send(CaptureMsg::Line(line)).is_err() {
            self.metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Capture a control input stamped `t_ms`, the SAME instant the engine is given for it,
    /// so replay reproduces closes and censoring to the millisecond.
    fn ctl(&self, shard: usize, ctl: Ctl, t_ms: i64) {
        self.capture(Line {
            recv_ts_us: t_ms * 1000,
            shard,
            raw: None,
            ctl: Some(ctl),
        });
    }

    fn integrity(
        &self,
        kind: &str,
        token: Option<String>,
        event_id: Option<String>,
        detail: String,
    ) -> IntegrityRow {
        IntegrityRow {
            run_id: self.run_id,
            ts_ms: now_ms(),
            kind: kind.into(),
            shard: None,
            token,
            event_id,
            detail: Some(detail),
        }
    }

    async fn route(&self, engine: &mut Engine, outs: Vec<Out>) -> anyhow::Result<()> {
        for o in outs {
            match o {
                // DEVIATION: Out::Episode carries Box<EpisodeWrite>, but WriterMsg::Episode takes an
                // owned EpisodeWrite, so unbox it here.
                Out::Episode(w) => self
                    .writer
                    .send(WriterMsg::Episode(*w))
                    .await
                    .context("writer gone")?,
                Out::Integrity(i) => self
                    .writer
                    .send(WriterMsg::Integrity(i))
                    .await
                    .context("writer gone")?,
                Out::Recheck(r) => {
                    let _ = self.recheck.try_send(r);
                }
                Out::Resnapshot { token } => {
                    let _ = self.resnap.try_send(token);
                }
                Out::UniverseChanged => self.refresh.notify_one(),
                Out::Lag(ms) => self.metrics.push_lag(ms),
                Out::Drift {
                    token,
                    event_id,
                    detail,
                } => {
                    self.writer
                        .send(WriterMsg::Integrity(self.integrity(
                            "drift",
                            Some(token.clone()),
                            event_id.clone(),
                            detail,
                        )))
                        .await
                        .context("writer gone")?;
                    let _ = self.resnap.try_send(token);
                    if let Some(ev) = event_id {
                        let t = now_ms();
                        let until = t + self.suspect_window_ms;
                        let flagged = engine.mark_suspect(&ev, until, t);
                        self.ctl(
                            0,
                            Ctl::Suspect {
                                event_id: ev,
                                until_ms: until,
                            },
                            t,
                        );
                        // `mark_suspect` returns only re-flagged episode rows
                        for f in flagged {
                            if let Out::Episode(w) = f {
                                self.writer
                                    .send(WriterMsg::Episode(*w))
                                    .await
                                    .context("writer gone")?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

async fn run_live(cfg: &Config, cfg_hash: &str, store: &Store, run_id: i64) -> anyhow::Result<()> {
    let metrics = Arc::new(Metrics::default());
    std::fs::create_dir_all(&cfg.capture.dir)
        .with_context(|| format!("create {}", cfg.capture.dir))?;

    // Universe: warm start or fresh (spec §8.1.2)
    let now = now_ms();
    let warm = match store.latest_universe().await? {
        Some(s)
            if warm_start_usable(
                s.taken_ms,
                now,
                i64::try_from(cfg.universe.warm_start_max_age_secs).unwrap_or(0) * 1000,
            ) =>
        {
            Universe::from_json(&s.body).ok()
        }
        _ => None,
    };
    let universe = match warm {
        Some(u) => u,
        None => {
            let u = fetch_universe(cfg).await?;
            persist_universe(store, &u).await?;
            u
        }
    };
    let universe = Arc::new(universe);
    let plan = shards::plan(shards::units(&universe, &HashMap::new()), &cfg.shards);

    // Channels
    let (feed_tx, mut feed_rx) = mpsc::channel::<FeedMsg>(cfg.engine.channel_capacity);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<EngineCmd>(1024);
    let (check_tx, mut check_rx) = mpsc::channel::<CheckCmd>(4096);
    let (writer_tx, writer_rx) = mpsc::channel::<WriterMsg>(65_536);
    let (capture_tx, capture_rx) = mpsc::channel::<CaptureMsg>(65_536);
    let (recheck_tx, recheck_rx) = mpsc::channel::<RecheckReq>(4096);
    let (resnap_tx, resnap_rx) = mpsc::channel::<String>(4096);
    let (plan_tx, mut plan_rx) = mpsc::channel::<Plan>(8);
    let (u_tx, u_rx) = watch::channel(universe.clone());
    let refresh = Arc::new(tokio::sync::Notify::new());

    // Tasks
    let writer_task = tokio::spawn(writer::run_writer(
        store.clone(),
        writer_rx,
        cfg.store.flush_ms,
        cfg.store.max_pending_rows,
        metrics.clone(),
    ));
    for (key, count) in plan.new_oversize(&Plan::default()) {
        writer_tx
            .send(WriterMsg::Integrity(oversize_row(
                run_id,
                now_ms(),
                &key,
                count,
            )))
            .await
            .context("writer gone")?;
    }
    let capture_thread = capture::spawn(cfg.capture.clone(), capture_rx, metrics.clone());
    let clob = Arc::new(ClobClient::new()?);
    let limiter = Arc::new(Limiter::new(
        cfg.recheck.rest_per_sec,
        cfg.recheck.resnapshot_reserved_per_sec,
    ));
    let rechecks_task = tokio::spawn(checks::run_rechecks(
        recheck_rx,
        u_rx.clone(),
        clob.clone(),
        limiter.clone(),
        cfg.recheck.offsets_ms.clone(),
        cfg.recheck.max_in_flight,
        writer_tx.clone(),
    ));
    let resnapshots_task = tokio::spawn(checks::run_resnapshots(
        resnap_rx,
        clob.clone(),
        limiter.clone(),
        check_tx.clone(),
    ));
    let samplers_task = tokio::spawn(checks::run_samplers(
        u_rx.clone(),
        clob.clone(),
        limiter.clone(),
        cfg.recheck.clone(),
        check_tx.clone(),
        metrics.clone(),
    ));
    let health_task = {
        // health: one row per minute
        let (metrics, writer_tx) = (metrics.clone(), writer_tx.clone());
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(60));
            t.tick().await;
            loop {
                t.tick().await;
                let minute = now_ms().div_euclid(60_000) * 60_000;
                if writer_tx
                    .send(WriterMsg::Health(metrics.minute_row(run_id, minute)))
                    .await
                    .is_err()
                {
                    return;
                }
                // Drop/delete counters otherwise never reach Postgres (F2): drain each to 0 and
                // surface a non-zero count as an integrity row.
                let now = now_ms();
                for (kind, counter) in [
                    ("capture_drop", &metrics.capture_drops),
                    ("writer_drop", &metrics.writer_drops),
                    ("capture_budget_delete", &metrics.capture_deletes),
                ] {
                    let n = counter.swap(0, Ordering::Relaxed);
                    if n == 0 {
                        continue;
                    }
                    let row = IntegrityRow {
                        run_id,
                        ts_ms: now,
                        kind: kind.to_string(),
                        shard: None,
                        token: None,
                        event_id: None,
                        detail: Some(n.to_string()),
                    };
                    if writer_tx.send(WriterMsg::Integrity(row)).await.is_err() {
                        return;
                    }
                }
            }
        })
    };
    let budget = Arc::new(Mutex::new(ConnectBudget::new(
        cfg.connect.per_sec,
        cfg.connect.per_min,
    )));
    let supervisor_task = {
        let mut sup = Supervisor::new(
            feed_tx.clone(),
            budget,
            cfg.connect.clone(),
            cfg.shards.per_shard_capacity,
            metrics.clone(),
        );
        tokio::spawn(async move {
            while let Some(p) = plan_rx.recv().await {
                sup.apply(&p).await;
            }
            sup.abort_all();
        })
    };
    let refresher_task = {
        // refresher (spec §7: every 5 min, and on new_market / market_resolved)
        let (cfg, store, cmd_tx, plan_tx, u_tx, refresh, writer_tx) = (
            cfg.clone(),
            store.clone(),
            cmd_tx.clone(),
            plan_tx.clone(),
            u_tx.clone(),
            refresh.clone(),
            writer_tx.clone(),
        );
        // Sticky plans (S3a): a refresh keeps every unit on its connection; only a retier (every
        // `retier_every_refreshes`) re-plans from scratch.
        let mut prev = plan.clone();
        tokio::spawn(async move {
            let mut rates: HashMap<String, u64> = HashMap::new();
            let mut n: u32 = 0;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(cfg.universe.refresh_secs)) => {}
                    _ = refresh.notified() => {}
                }
                n = n.wrapping_add(1);
                let u = match fetch_universe(&cfg).await {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("refresh: {e:#}");
                        continue;
                    }
                };
                let retier = n.is_multiple_of(cfg.shards.retier_every_refreshes);
                if retier {
                    let (tx, rx) = oneshot::channel();
                    if cmd_tx.send(EngineCmd::Rates(tx)).await.is_err() {
                        return;
                    }
                    rates = rx.await.unwrap_or_default();
                }
                let plan = shards::replan(&prev, shards::units(&u, &rates), &cfg.shards, retier);
                for (key, count) in plan.new_oversize(&prev) {
                    let row = oversize_row(run_id, now_ms(), &key, count);
                    if writer_tx.send(WriterMsg::Integrity(row)).await.is_err() {
                        return;
                    }
                }
                prev = plan.clone();
                if let Err(e) = persist_universe(&store, &u).await {
                    eprintln!("refresh persist: {e:#}");
                }
                let u = Arc::new(u);
                let (ack_tx, ack_rx) = oneshot::channel();
                if cmd_tx
                    .send(EngineCmd::Apply {
                        universe: u.clone(),
                        conn_of: plan.conn_of(),
                        ack: ack_tx,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                let _ = ack_rx.await; // mapping applied BEFORE any new shard can deliver frames
                let _ = u_tx.send(u);
                if plan_tx.send(plan).await.is_err() {
                    return;
                }
            }
        })
    };

    // Engine loop (this future)
    let io = Io {
        writer: writer_tx.clone(),
        capture: capture_tx.clone(),
        recheck: recheck_tx,
        resnap: resnap_tx,
        refresh: refresh.clone(),
        metrics: metrics.clone(),
        run_id,
        l1_window_ms: cfg.engine.l1_suspect_window_ms,
        suspect_window_ms: cfg.engine.suspect_window_ms,
    };
    let mut engine = Engine::new(engine_ctx(cfg, cfg_hash, run_id, "live"));
    // Unique across runs AND within a run (F3): run ids are unique, so a warm start that reuses
    // an earlier `taken_ms` in a new process can never overwrite a previous run's record.
    let mut universe_seq: u64 = u64::try_from(run_id).unwrap_or(0).saturating_mul(1_000_000);
    let mut apply =
        |engine: &mut Engine, u: Arc<Universe>, conn_of: &HashMap<String, usize>, io: &Io| {
            universe_seq += 1;
            let file = capture::universe_file_name(u.taken_ms, universe_seq);
            let rec = UniverseRecord {
                universe: (*u).clone(),
                conn_of: conn_of
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect::<ConnMap>(),
                file: file.clone(),
            };
            if io
                .capture
                .try_send(CaptureMsg::Universe(Box::new(rec)))
                .is_err()
            {
                io.metrics.capture_drops.fetch_add(1, Ordering::Relaxed);
            }
            let t = now_ms();
            io.ctl(
                0,
                Ctl::Universe {
                    taken_ms: u.taken_ms,
                    file,
                },
                t,
            );
            engine.apply_universe(u, conn_of, t)
        };
    let outs = apply(&mut engine, universe.clone(), &plan.conn_of(), &io);
    io.route(&mut engine, outs).await?;
    plan_tx.send(plan).await.context("supervisor gone")?;

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut timing = FrameTiming::new(now_ms());
    let result: anyhow::Result<()> = loop {
        tokio::select! {
            _ = &mut shutdown => break Ok(()),
            m = feed_rx.recv() => {
                let Some(m) = m else { break Err(anyhow::anyhow!("feed channel closed")) };
                match m {
                    FeedMsg::Frame(f) => {
                        let ms = f.recv_us.div_euclid(1000);
                        metrics.msgs.fetch_add(1, Ordering::Relaxed);
                        metrics.bytes.fetch_add(f.text.len() as u64, Ordering::Relaxed);
                        let t0 = Instant::now();
                        let parsed = parse_frame(&f.text);
                        let parse_us = u32::try_from(t0.elapsed().as_micros()).unwrap_or(u32::MAX);
                        let (outs, on_frame_us) = match parsed {
                            Ok(evs) => {
                                let t1 = Instant::now();
                                let outs = engine.on_frame(f.conn, ms, &evs);
                                let on_frame_us = u32::try_from(t1.elapsed().as_micros()).unwrap_or(u32::MAX);
                                engine.alive(f.conn, ms);
                                (outs, Some(on_frame_us))
                            }
                            Err(e) => (vec![Out::Integrity(IntegrityRow {
                                run_id, ts_ms: ms, kind: "parse_error".into(), shard: i32::try_from(f.conn).ok(),
                                token: None, event_id: None, detail: Some(e.to_string()),
                            })], None),
                        };
                        timing.record(parse_us, on_frame_us, now_ms());
                        io.capture(Line { recv_ts_us: f.recv_us, shard: f.conn, raw: Some(f.text), ctl: None });
                        if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
                    }
                    FeedMsg::Up { .. } => {}
                    FeedMsg::Down { conn, reason } => {
                        let t = now_ms();
                        io.ctl(conn, Ctl::Down, t);
                        let mut outs = engine.disconnect(conn, t);
                        if let Some(Out::Integrity(i)) = outs.first_mut() {
                            i.detail = Some(reason);
                        }
                        if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
                    }
                    FeedMsg::Parked { conn } => {
                        let row = IntegrityRow { run_id, ts_ms: now_ms(), kind: "shard_parked".into(), shard: i32::try_from(conn).ok(), token: None, event_id: None, detail: None };
                        if writer_tx.send(WriterMsg::Integrity(row)).await.is_err() { break Err(anyhow::anyhow!("writer gone")) }
                    }
                }
            }
            c = cmd_rx.recv() => {
                let Some(c) = c else { break Err(anyhow::anyhow!("command channel closed")) };
                match c {
                    EngineCmd::Apply { universe, conn_of, ack } => {
                        let outs = apply(&mut engine, universe, &conn_of, &io);
                        let _ = ack.send(());
                        if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
                    }
                    EngineCmd::Rates(tx) => { let _ = tx.send(engine.rates()); }
                }
            }
            Some(k) = check_rx.recv() => {
                let outs = match k {
                    CheckCmd::RestSnapshot { token, book } => {
                        io.ctl(0, book_ctl(&token, &book), now_ms());
                        engine.rest_snapshot(&token, book)
                    }
                    CheckCmd::Drift { token, book } => { engine.drift_check(&token, book, now_ms()); Vec::new() }
                    CheckCmd::L1Mismatch { event_id, detail } => {
                        let t = now_ms();
                        let until = t + io.l1_window_ms;
                        let mut outs = engine.mark_suspect(&event_id, until, t);
                        io.ctl(0, Ctl::Suspect { event_id: event_id.clone(), until_ms: until }, t);
                        outs.push(Out::Integrity(io.integrity("mirror_mismatch", None, Some(event_id), detail)));
                        outs
                    }
                };
                if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
            }
            _ = tick.tick() => {
                let outs = engine.tick(now_ms());
                if let Err(e) = io.route(&mut engine, outs).await { break Err(e) }
            }
        }
    };

    // Shutdown: ONE deadline for every step (SHUTDOWN_BUDGET), so the whole stop plus `run`'s
    // exit row fits compose's 30 s stop_grace_period. Order:
    // 1. stop taking new shards/universes; 2. close every open episode; 3. flush the writer;
    // 4. THEN stop everything still holding a `writer_tx` clone (the re-checker owns its tasks),
    //    so the writer's final join can return; 5. give the capture thread what time is left.
    let deadline = tokio::time::Instant::now() + SHUTDOWN_BUDGET;
    drop(plan_tx);
    supervisor_task.abort();
    refresher_task.abort();
    let t = now_ms();
    io.ctl(0, Ctl::Shutdown, t);
    let outs = engine.close_all(t);
    let _ = by_deadline(deadline, "close episodes", io.route(&mut engine, outs)).await;
    let (done_tx, done_rx) = oneshot::channel();
    let flush = async {
        if writer_tx.send(WriterMsg::Flush(done_tx)).await.is_ok() {
            let _ = done_rx.await;
        }
    };
    let _ = by_deadline(deadline, "writer flush", flush).await;
    health_task.abort();
    rechecks_task.abort();
    resnapshots_task.abort();
    samplers_task.abort();
    drop(io);
    drop(capture_tx);
    drop(writer_tx);
    let _ = by_deadline(deadline, "writer exit", writer_task).await;
    if wait_thread(&capture_thread, deadline).await {
        let _ = capture_thread.join();
    } else {
        eprintln!("shutdown: capture thread still busy at the deadline; exiting without it");
    }
    result
}

pub async fn census(cfg: &Config) -> anyhow::Result<()> {
    let u = fetch_universe(cfg).await?;
    let plan = shards::plan(shards::units(&u, &HashMap::new()), &cfg.shards);
    let mut by_status: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for g in &u.groups {
        for l in &g.legs {
            *by_status.entry(l.status.as_str()).or_insert(0) += 1;
        }
    }
    println!("negrisk groups: {}", u.groups.len());
    println!("legs by status: {by_status:?}");
    println!("canary pairs:   {}", u.canary.len());
    println!("subscribed tokens (all): {}", u.subscribed_tokens());
    println!(
        "shard plan: {} shards ({} hot), {} tokens, max {} tokens/shard",
        plan.live_shards(),
        plan.shards.iter().filter(|s| s.hot).count(),
        plan.tokens(),
        plan.shards
            .iter()
            .map(|s| s.tokens.len())
            .max()
            .unwrap_or(0)
    );
    println!(
        "dropped (connection budget): {} units, {} tokens",
        plan.dropped.len(),
        plan.dropped_tokens
    );
    println!(
        "oversize units (> {} tokens, never subscribed): {:?}",
        shards::MAX_UNIT_TOKENS,
        plan.oversize
    );
    if let Ok(url) = std::env::var("DATABASE_URL") {
        match Store::connect(&url, 1).await {
            Ok(s) => println!("database tables: {:?}", s.table_names().await?),
            Err(e) => println!("database: unreachable ({e})"),
        }
    }
    Ok(())
}

pub async fn migrate() -> anyhow::Result<()> {
    let url = std::env::var("OWNER_DATABASE_URL")
        .context("OWNER_DATABASE_URL (owner role) is not set")?;
    let s = Store::connect(&url, 1).await?;
    s.migrate().await?;
    println!("migrations applied");
    Ok(())
}

pub async fn breaker_reset() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    Store::connect(&url, 1)
        .await?
        .record_breaker_reset("scanner", now_ms())
        .await?;
    println!("breaker reset: the next `run` starts live");
    Ok(())
}

pub async fn replay_cmd(
    cfg: &Config,
    cfg_hash: &str,
    dir: &Path,
    dry_run: bool,
) -> anyhow::Result<()> {
    if dry_run {
        let r = replay_dir(dir, engine_ctx(cfg, cfg_hash, 0, "replay"))?;
        let eps: std::collections::HashSet<&str> = r
            .outs
            .iter()
            .filter_map(|o| match o {
                Out::Episode(w) => Some(w.row.episode_id.as_str()),
                _ => None,
            })
            .collect();
        println!(
            "frames {} parse_errors {} episodes {}",
            r.frames,
            r.parse_errors,
            eps.len()
        );
        return Ok(());
    }
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    let store = Store::connect(&url, 2).await?;
    let run_id = store.start_run(&new_run(cfg_hash, "replay")).await?;
    let r = replay_dir(dir, engine_ctx(cfg, cfg_hash, run_id, "replay"))?;
    let mut batch = Batch::default();
    for o in r.outs {
        match o {
            // DEVIATION: unbox, same reason as in Io::route.
            Out::Episode(w) => batch.episodes.push(*w),
            Out::Integrity(i) => batch.integrity.push(i),
            _ => {}
        }
        if batch.episodes.len() + batch.integrity.len() >= 2_000 {
            store.write_batch(&std::mem::take(&mut batch)).await?;
        }
    }
    store.write_batch(&batch).await?;
    store
        .end_run(
            run_id,
            now_ms(),
            &format!("replay of {} ({} frames)", dir.display(), r.frames),
        )
        .await?;
    println!(
        "replay run {run_id}: {} frames, {} parse errors",
        r.frames, r.parse_errors
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_us_sorts_then_indexes_with_integer_math() {
        assert_eq!(percentiles_us(&[]), (0, 0, 0));
        assert_eq!(percentiles_us(&[5]), (5, 5, 5));
        let xs: Vec<u32> = (1..=100).collect();
        assert_eq!(percentiles_us(&xs), (50, 99, 100));
        // unsorted input: same result
        let mut shuffled = xs;
        shuffled.reverse();
        assert_eq!(percentiles_us(&shuffled), (50, 99, 100));
    }

    #[test]
    fn breaker_trips_at_max_starts() {
        assert!(!breaker_trips(4, 5));
        assert!(breaker_trips(5, 5));
        assert!(breaker_trips(9, 5));
    }

    #[test]
    fn warm_start_only_when_fresh() {
        assert!(warm_start_usable(1_000, 200_000, 300_000));
        assert!(!warm_start_usable(1_000, 400_000, 300_000));
        assert!(
            !warm_start_usable(500_000, 400_000, 300_000),
            "a snapshot from the future is not trusted"
        );
    }

    #[test]
    fn an_oversize_unit_becomes_an_integrity_row_naming_it() {
        let r = oversize_row(3, 9, "e1", 1_234);
        assert_eq!(
            (r.run_id, r.ts_ms, r.kind.as_str(), r.event_id.as_deref()),
            (3, 9, "unit_oversize", Some("e1"))
        );
        assert!(
            r.detail
                .as_deref()
                .is_some_and(|d| d.contains("1234 tokens"))
        );
    }

    #[test]
    fn the_whole_stop_fits_the_compose_grace_period() {
        // compose.yml: stop_grace_period: 30s
        assert!(SHUTDOWN_BUDGET + END_RUN_TIMEOUT < Duration::from_secs(30));
    }

    #[tokio::test]
    async fn shutdown_steps_never_outlive_the_deadline() {
        let start = tokio::time::Instant::now();
        let deadline = start + Duration::from_millis(100);
        assert_eq!(by_deadline(deadline, "ready", async { 1 }).await, Some(1));
        assert_eq!(
            by_deadline(deadline, "stuck", std::future::pending::<u8>()).await,
            None
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        // past the deadline: ready work still completes, stuck work returns at once
        assert_eq!(
            by_deadline(deadline, "late ready", async { 2 }).await,
            Some(2)
        );
        let t = tokio::time::Instant::now();
        assert_eq!(
            by_deadline(deadline, "late stuck", std::future::pending::<u8>()).await,
            None
        );
        assert!(t.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn waiting_for_a_busy_thread_stops_at_the_deadline() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let busy = std::thread::spawn(move || {
            let _ = rx.recv();
        });
        let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
        assert!(
            !wait_thread(&busy, deadline).await,
            "still busy at the deadline"
        );
        drop(tx); // lets the thread finish
        let later = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(wait_thread(&busy, later).await);
        let _ = busy.join();
    }
}
