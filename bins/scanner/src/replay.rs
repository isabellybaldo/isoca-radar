//! Replay a capture directory through the engine (spec §8.3). Same engine, same inputs, same outputs.
//! Lines are read by `tape::files::for_each_line`, the reader the compactor shares (spec 5b §2).
use crate::capture::{Ctl, Line, UniverseRecord, ctl_book, universe_path};
use crate::engine::{Engine, EngineCtx, Out};
use anyhow::Context;
use pm_api::ws::parse_frame;
use rust_decimal::Decimal;
use sim::trader::PaperTrader;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct ReplayOut {
    pub outs: Vec<Out>,
    pub frames: u64,
    pub parse_errors: u64,
}

#[derive(Debug)]
pub struct ReplayStats {
    pub frames: u64,
    pub parse_errors: u64,
}

/// What a replay carries from one line to the next.
#[derive(Debug, Default)]
pub struct ReplayState {
    pub frames: u64,
    pub parse_errors: u64,
    pub last_ms: i64,
    pub shut_down: bool,
}

fn walk(dir: &Path, hours: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let p = entry?.path();
        if p.is_dir() {
            walk(&p, hours)?;
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".jsonl.zst") {
                hours.push(p);
            }
        }
    }
    Ok(())
}

/// Load and decompress the universe record `Ctl::Universe` names, at its exact path (F3: the
/// ctl line carries the file name, so replay never has to search for it by `taken_ms`).
fn load_universe(dir: &Path, taken_ms: i64, file: &str) -> anyhow::Result<UniverseRecord> {
    tape::files::read_zst_json(&universe_path(dir, taken_ms, file))
}

/// Feed one capture line to the engine exactly as replay does. The tape agreement test
/// (bins/scanner/tests/tape_agreement.rs) drives the engine line by line through this.
pub fn apply_line(
    engine: &mut Engine,
    dir: &Path,
    line: &Line,
    st: &mut ReplayState,
    sink: &mut impl FnMut(Out),
) -> anyhow::Result<()> {
    let ms = line.recv_ts_us.div_euclid(1000);
    st.last_ms = st.last_ms.max(ms);
    if let Some(raw) = &line.raw {
        st.frames += 1;
        match parse_frame(raw) {
            Ok(evs) => {
                for o in engine.on_frame(line.shard, ms, &evs) {
                    sink(o);
                }
                engine.alive(line.shard, ms);
            }
            Err(_) => st.parse_errors += 1,
        }
    }
    match &line.ctl {
        None => {}
        Some(Ctl::Down) => {
            for o in engine.disconnect(line.shard, ms) {
                sink(o);
            }
        }
        Some(Ctl::Universe { taken_ms, file }) => {
            let rec = load_universe(dir, *taken_ms, file)?;
            let conn: HashMap<String, usize> =
                rec.conn_of.iter().map(|(k, v)| (k.clone(), *v)).collect();
            for o in engine.apply_universe(Arc::new(rec.universe), &conn, ms) {
                sink(o);
            }
        }
        Some(Ctl::RestSnapshot {
            token,
            server_ts_ms,
            bids,
            asks,
        }) => {
            let b = ctl_book(bids, asks, *server_ts_ms)?;
            for o in engine.rest_snapshot(token, b) {
                sink(o);
            }
        }
        Some(Ctl::Suspect { event_id, until_ms }) => {
            for o in engine.mark_suspect(event_id, *until_ms, ms) {
                sink(o);
            }
        }
        Some(Ctl::Resolved {
            market_id,
            payout_yes,
            payout_no,
        }) => {
            let y = Decimal::from_str_exact(payout_yes)
                .with_context(|| format!("bad payout_yes {payout_yes:?}"))?;
            let n = Decimal::from_str_exact(payout_no)
                .with_context(|| format!("bad payout_no {payout_no:?}"))?;
            for o in engine.on_resolution(ms, market_id, y, n) {
                sink(o);
            }
        }
        Some(Ctl::Shutdown) => {
            for o in engine.close_all(ms) {
                sink(o);
            }
            for o in engine.paper_close(ms) {
                sink(o);
            }
            st.shut_down = true;
        }
    }
    Ok(())
}

/// Replay a capture directory, delivering every engine output to `sink` in order instead of
/// collecting them, so a long replay does not have to hold millions of `Out`s in memory. With a
/// paper trader (S3c) the trader starts FRESH (never from `paper_state`), so two replays of one
/// capture produce identical paper rows; resolutions come only from `Ctl::Resolved` lines.
pub fn replay_dir_paper_with(
    dir: &Path,
    ctx: EngineCtx,
    paper: Option<PaperTrader>,
    mut sink: impl FnMut(Out),
) -> anyhow::Result<ReplayStats> {
    let mut hours = Vec::new();
    walk(dir, &mut hours)?;
    hours.sort();
    let mut engine = Engine::new(ctx);
    if let Some(t) = paper {
        engine.set_paper(t);
    }
    let mut st = ReplayState::default();
    for h in hours {
        tape::files::for_each_line(&h, |line| {
            apply_line(&mut engine, dir, &line, &mut st, &mut sink)
        })?;
    }
    if !st.shut_down {
        // crash: close at the last captured instant
        for o in engine.close_all(st.last_ms) {
            sink(o);
        }
        for o in engine.paper_close(st.last_ms) {
            sink(o);
        }
    }
    Ok(ReplayStats {
        frames: st.frames,
        parse_errors: st.parse_errors,
    })
}

/// Replay without a paper trader (the S2 scanner outputs only).
pub fn replay_dir_with(
    dir: &Path,
    ctx: EngineCtx,
    sink: impl FnMut(Out),
) -> anyhow::Result<ReplayStats> {
    replay_dir_paper_with(dir, ctx, None, sink)
}

pub fn replay_dir(dir: &Path, ctx: EngineCtx) -> anyhow::Result<ReplayOut> {
    let mut outs = Vec::new();
    let stats = replay_dir_with(dir, ctx, |o| outs.push(o))?;
    Ok(ReplayOut {
        outs,
        frames: stats.frames,
        parse_errors: stats.parse_errors,
    })
}
