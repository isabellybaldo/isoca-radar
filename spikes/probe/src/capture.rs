//! `capture`: event-affine shards → websocket sessions → raw zstd capture + per-frame CSV.
use crate::shards::pack_shards;
use crate::universe::Universe;
use pm_api::backoff::{Backoff, ConnectBudget};
use pm_api::ws::{Frame, MarketEvent, now_us, parse_frame, run_connection};
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Wall-clock milliseconds, for `Backoff`/`ConnectBudget` (both take `i64` ms).
fn now_ms() -> i64 {
    now_us() / 1_000
}

/// `ConnectBudget::try_take`, recovering from a poisoned mutex instead of panicking:
/// a stuck lock must not take the whole capture down.
fn take_budget(budget: &Mutex<ConnectBudget>, now: i64) -> bool {
    match budget.lock() {
        Ok(mut g) => g.try_take(now),
        Err(poisoned) => poisoned.into_inner().try_take(now),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub recv_us: i64,
    pub conn: usize,
    pub shard_tokens: usize,
    pub bytes: usize,
    pub ok: bool,
    pub events: usize,
    pub server_ms: Option<i64>,
    pub kinds: String,
}

impl Row {
    pub const HEADER: &'static str = "recv_us,conn,shard_tokens,bytes,ok,events,server_ms,kinds";

    pub fn to_csv(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{}",
            self.recv_us,
            self.conn,
            self.shard_tokens,
            self.bytes,
            u8::from(self.ok),
            self.events,
            self.server_ms.map_or(String::new(), |v| v.to_string()),
            self.kinds
        )
    }

    #[allow(dead_code)] // first non-test caller is `report` (Task 4); binaries warn on unused pub fns
    pub fn from_csv(line: &str) -> Option<Row> {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() != 8 {
            return None;
        }
        Some(Row {
            recv_us: f[0].parse().ok()?,
            conn: f[1].parse().ok()?,
            shard_tokens: f[2].parse().ok()?,
            bytes: f[3].parse().ok()?,
            ok: f[4] == "1",
            events: f[5].parse().ok()?,
            server_ms: if f[6].is_empty() {
                None
            } else {
                Some(f[6].parse().ok()?)
            },
            kinds: f[7].to_string(),
        })
    }
}

fn kind_and_ts(e: &MarketEvent) -> (&'static str, Option<i64>) {
    match e {
        MarketEvent::Snapshot { server_ts_ms, .. } => ("book", Some(*server_ts_ms)),
        MarketEvent::Delta { server_ts_ms, .. } => ("price_change", Some(*server_ts_ms)),
        MarketEvent::LastTrade { server_ts_ms, .. } => ("last_trade_price", Some(*server_ts_ms)),
        MarketEvent::TickSize { server_ts_ms, .. } => ("tick_size_change", Some(*server_ts_ms)),
        MarketEvent::Resolved { server_ts_ms, .. } => ("market_resolved", *server_ts_ms),
        MarketEvent::NewMarket { server_ts_ms } => ("new_market", *server_ts_ms),
        MarketEvent::Other { .. } => ("other", None),
    }
}

/// One CSV row per frame; the parse error (if any) is returned for sampling.
pub fn summarize_frame(shard_tokens: usize, f: &Frame) -> (Row, Option<String>) {
    let base = Row {
        recv_us: f.recv_us,
        conn: f.conn,
        shard_tokens,
        bytes: f.text.len(),
        ok: false,
        events: 0,
        server_ms: None,
        kinds: String::new(),
    };
    match parse_frame(&f.text) {
        Ok(evs) => {
            let mut kinds: Vec<&str> = Vec::new();
            let mut server_ms: Option<i64> = None;
            for e in &evs {
                let (k, t) = kind_and_ts(e);
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
                server_ms = match (server_ms, t) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
            kinds.sort_unstable();
            (
                Row {
                    ok: true,
                    events: evs.len(),
                    server_ms,
                    kinds: kinds.join("|"),
                    ..base
                },
                None,
            )
        }
        Err(e) => (base, Some(e.to_string())),
    }
}

pub async fn capture(
    u: &Universe,
    shard_tokens: usize,
    max_conns: usize,
    minutes: u64,
    out: &Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let all_shards: Vec<Vec<String>> = pack_shards(&u.token_groups(), shard_tokens);
    let dropped_tokens: usize = all_shards.iter().skip(max_conns).map(Vec::len).sum();
    println!("dropped_tokens={dropped_tokens}");
    let shards: Vec<Vec<String>> = all_shards.into_iter().take(max_conns).collect();
    let sizes: Vec<usize> = shards.iter().map(Vec::len).collect();
    let mut shard_csv = String::from("conn,tokens\n");
    for (i, n) in sizes.iter().enumerate() {
        shard_csv.push_str(&format!("{i},{n}\n"));
    }
    shard_csv.push_str(&format!("# dropped_tokens={dropped_tokens}\n"));
    std::fs::write(out.join("shards.csv"), shard_csv)?;
    println!(
        "shards={} tokens={}",
        shards.len(),
        sizes.iter().sum::<usize>()
    );

    // Global connect budget shared by every shard task, across all reconnects (≤ 2/s, ≤ 60/min).
    let budget = Arc::new(Mutex::new(ConnectBudget::new(2, 60)));
    let (tx, mut rx) = mpsc::channel::<Frame>(20_000);
    for (i, tokens) in shards.into_iter().enumerate() {
        let tx = tx.clone();
        let budget = Arc::clone(&budget);
        tokio::spawn(async move {
            let mut backoff = Backoff::new(5_000, 60_000, 30_000);
            loop {
                while !take_budget(&budget, now_ms()) {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                backoff.on_connected(now_ms());
                let r = run_connection(i, tokens.clone(), tx.clone()).await;
                if tx.is_closed() {
                    break;
                }
                let delay_ms = backoff.on_disconnected(now_ms());
                eprintln!("conn {i} ended: {r:?}; reconnecting in {delay_ms} ms");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        });
    }
    drop(tx);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(minutes * 60);
    let mut raw = zstd::Encoder::new(File::create(out.join("raw.jsonl.zst"))?, 3)?.auto_finish();
    let mut rows = BufWriter::new(File::create(out.join("frames.csv"))?);
    writeln!(rows, "{}", Row::HEADER)?;
    let mut errors = BufWriter::new(File::create(out.join("errors.jsonl"))?);
    let mut n_err: usize = 0;
    loop {
        let frame = tokio::select! {
            f = rx.recv() => match f { Some(f) => f, None => break },
            _ = tokio::time::sleep_until(deadline) => break,
        };
        writeln!(
            raw,
            "{}",
            json!({ "recv_us": frame.recv_us, "conn": frame.conn, "raw": frame.text })
        )?;
        let shard = sizes.get(frame.conn).copied().unwrap_or(0);
        let (row, err) = summarize_frame(shard, &frame);
        writeln!(rows, "{}", row.to_csv())?;
        if let Some(e) = err {
            if n_err < 100 {
                writeln!(errors, "{}", json!({ "error": e, "raw": frame.text }))?;
            }
            n_err += 1;
        }
    }
    rx.close();
    rows.flush()?;
    errors.flush()?;
    println!("parse_errors={n_err}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_api::ws::Frame;

    #[test]
    fn summarizes_a_price_change_frame() {
        let text = r#"{"event_type":"price_change","market":"m","timestamp":"1790121493000","price_changes":[
            {"asset_id":"1","price":"0.5","size":"1","side":"BUY"},{"asset_id":"2","price":"0.5","size":"0","side":"SELL"}]}"#;
        let f = Frame {
            conn: 3,
            recv_us: 1_790_121_493_250_000,
            text: text.into(),
        };
        let (row, err) = summarize_frame(50, &f);
        assert!(err.is_none());
        assert_eq!(
            row,
            Row {
                recv_us: 1_790_121_493_250_000,
                conn: 3,
                shard_tokens: 50,
                bytes: text.len(),
                ok: true,
                events: 2,
                server_ms: Some(1_790_121_493_000),
                kinds: "price_change".into()
            }
        );
        assert_eq!(Row::from_csv(&row.to_csv()), Some(row));
    }

    #[test]
    fn unparseable_frame_is_counted_not_fatal() {
        let f = Frame {
            conn: 0,
            recv_us: 1,
            text: "{broken".into(),
        };
        let (row, err) = summarize_frame(5, &f);
        assert!(!row.ok && err.is_some() && row.server_ms.is_none());
        assert_eq!(Row::from_csv(&row.to_csv()), Some(row));
    }
}
