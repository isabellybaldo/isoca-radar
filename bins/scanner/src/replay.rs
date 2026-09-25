//! Replay a capture directory through the engine (spec §8.3). Same engine, same inputs, same outputs.
use crate::capture::{Ctl, Line, UniverseRecord, ctl_book};
use crate::engine::{Engine, EngineCtx, Out};
use anyhow::Context;
use pm_api::ws::parse_frame;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct ReplayOut {
    pub outs: Vec<Out>,
    pub frames: u64,
    pub parse_errors: u64,
}

fn walk(dir: &Path, hours: &mut Vec<PathBuf>, universes: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let p = entry?.path();
        if p.is_dir() {
            walk(&p, hours, universes)?;
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".jsonl.zst") {
                hours.push(p);
            } else if name.starts_with("universe-") && name.ends_with(".json") {
                universes.push(p);
            }
        }
    }
    Ok(())
}

pub fn replay_dir(dir: &Path, ctx: EngineCtx) -> anyhow::Result<ReplayOut> {
    let (mut hours, mut ufiles) = (Vec::new(), Vec::new());
    walk(dir, &mut hours, &mut ufiles)?;
    hours.sort();
    let mut universes: HashMap<i64, UniverseRecord> = HashMap::new();
    for f in ufiles {
        let rec: UniverseRecord = serde_json::from_slice(&std::fs::read(&f)?)
            .with_context(|| format!("parse {}", f.display()))?;
        universes.insert(rec.universe.taken_ms, rec);
    }
    let mut engine = Engine::new(ctx);
    let mut out = ReplayOut {
        outs: Vec::new(),
        frames: 0,
        parse_errors: 0,
    };
    let mut last_ms = 0_i64;
    let mut shut_down = false;
    for h in hours {
        let reader = BufReader::new(zstd::Decoder::new(std::fs::File::open(&h)?)?);
        for text in reader.lines() {
            let text = text?;
            if text.trim().is_empty() {
                continue;
            }
            let line: Line = serde_json::from_str(&text)
                .with_context(|| format!("bad capture line in {}", h.display()))?;
            let ms = line.recv_ts_us.div_euclid(1000);
            last_ms = last_ms.max(ms);
            if let Some(raw) = &line.raw {
                out.frames += 1;
                match parse_frame(raw) {
                    Ok(evs) => {
                        out.outs.extend(engine.on_frame(line.shard, ms, &evs));
                        engine.alive(line.shard, ms);
                    }
                    Err(_) => out.parse_errors += 1,
                }
            }
            match line.ctl {
                None => {}
                Some(Ctl::Down) => out.outs.extend(engine.disconnect(line.shard, ms)),
                Some(Ctl::Universe { taken_ms }) => {
                    let rec = universes
                        .get(&taken_ms)
                        .with_context(|| format!("missing universe-{taken_ms}.json"))?;
                    let conn: HashMap<String, usize> =
                        rec.conn_of.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    out.outs.extend(engine.apply_universe(
                        Arc::new(rec.universe.clone()),
                        &conn,
                        ms,
                    ));
                }
                Some(Ctl::RestSnapshot {
                    token,
                    server_ts_ms,
                    bids,
                    asks,
                }) => {
                    let b = ctl_book(&bids, &asks, server_ts_ms)?;
                    out.outs.extend(engine.rest_snapshot(&token, b));
                }
                Some(Ctl::Suspect { event_id, until_ms }) => {
                    engine.mark_suspect(&event_id, until_ms)
                }
                Some(Ctl::Shutdown) => {
                    out.outs.extend(engine.close_all(ms));
                    shut_down = true;
                }
            }
        }
    }
    if !shut_down {
        out.outs.extend(engine.close_all(last_ms)); // crash: close at the last captured instant
    }
    Ok(out)
}
