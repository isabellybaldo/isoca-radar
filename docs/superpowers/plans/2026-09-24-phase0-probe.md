# Phase 0 Probe Implementation Plan (measurement spike)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A small `probe` binary, run in Docker on the owner's machine for about 1–1.5 h. It measures what S2c needs: message rate and bytes/day for the NegRisk universe, the per-IP websocket connection cap, lag and stalls versus shard size, and the WSL2 clock offset. It also re-checks the YES/NO mirror (L1) at scale and validates `pm-api`'s parsers on live traffic.

**Architecture:** A workspace binary `spikes/probe` built on `pm-api` (S2a must be done first) with five subcommands: `universe`, `conncap`, `capture`, `mirror`, `report`. Pure logic (universe selection, event-affine shard packing, CSV rows, statistics, mirror comparison) is unit-tested. Network paths are exercised by the actual run. The outputs are CSV/Markdown files plus a hand-filled `RESULTS.md` that answers Q7 and Q17.

**Tech Stack:** Rust 1.98.1, pm-api, arb-core, tokio, zstd 0.13, clap 4, anyhow 1, serde/serde_json. Docker Desktop (WSL2).

**Spec:** `docs/superpowers/specs/2026-09-22-arb-scanner-design.md` (§7 capture, §8.1.2, §18 Q7/Q17, §19 phase 0).

## Global Constraints

- Read-only: only `pm-api` routes (Gamma `/events/keyset`, CLOB `POST /books`, market websocket). No credentials.
- **Be polite to the venue.** At most 2 new websocket connections per second everywhere. `conncap` spaces attempts 3 s apart, stops at the **first** refusal, and never exceeds 40 connections. `capture` uses at most 16 connections.
- Event-affine sharding: all legs of one event, or both tokens of a canary pair, go on the same connection. A group is never split.
- No floats: statistics use integer milliseconds/bytes and sorted-index percentiles.
- This binary is a spike. It may be deleted after S2c lands; its measurements live on in `spikes/probe/RESULTS.md`.
- Commit messages are plain, with no co-author or attribution trailer.

## Review Focus

1. **A connection that drops mid-capture** must reconnect (5 s pause) and keep its shard's rows attributed to the same `conn`. The run must not end early (pinned by the reconnect loop in Task 2 and checked in the run's `errors.jsonl` and conn row counts).
2. **Frames that `pm-api` can't parse** must be counted and sampled, never crash the capture (pinned in Task 2's `summarize_frame` test).
3. **Negative lag** (the server clock ahead of WSL2) must be reported as-is, not clamped, because it *is* the clock-offset measurement (pinned in Task 4).
4. **A NegRisk event with more legs than the shard size** gets its own over-size shard rather than being split (pinned in Task 2).
5. **A mirror book missing from the `/books` response** (a delisted token) is reported as `missing`, not as a mismatch (pinned in Task 3).

## File Structure

```
Cargo.toml                         add member spikes/probe; workspace deps zstd, clap, anyhow
.dockerignore                      keep the build context small
spikes/probe/
  Cargo.toml
  Dockerfile
  RUNBOOK.md                       exact commands for the owner
  RESULTS.md                       template, filled after the run
  src/main.rs                      clap subcommands
  src/universe.rs                  select(): NegRisk groups + canary pairs
  src/shards.rs                    pack_shards(): event-affine first-fit-decreasing
  src/capture.rs                   Row (CSV), summarize_frame(), capture()
  src/conncap.rs                   conncap()
  src/mirror.rs                    mirror_matches(), mirror()
  src/report.rs                    percentile(), bucket_stats(), render()
```

---

### Task 1: Crate, universe selection, `universe` subcommand

**Files:**
- Modify: `Cargo.toml` (root)
- Create: `spikes/probe/Cargo.toml`, `spikes/probe/src/main.rs`, `spikes/probe/src/universe.rs`

**Interfaces:**
- Consumes (from S2a):
  - `pm_api::gamma::{GammaClient, GammaEvent, leg_status, token_ids}`
  - `GammaEvent::{is_neg_risk, is_augmented, markets}`
  - `GammaClient::all_active_events()`
  - `arb_core::detect::LegStatus`
- Produces:
  - `universe::{Group { event_id, title, yes_tokens: Vec<String> }, CanaryPair { market_id, yes, no }, Universe { groups, canary }}` (all serde)
  - `universe::select(&[GammaEvent], canary_n: usize) -> Universe`
  - `Universe::token_groups(&self) -> Vec<Vec<String>>` (each NegRisk group's YES tokens, then each canary pair as `[yes, no]`)
  - `Universe::load(&Path) -> anyhow::Result<Universe>` (added in Task 2)

- [ ] **Step 1: Workspace.** Root `Cargo.toml`: members `["crates/arb-core", "crates/pm-api", "spikes/probe"]`; add to `[workspace.dependencies]`:
```toml
zstd = "0.13"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
```
`spikes/probe/Cargo.toml`:
```toml
[package]
name = "probe"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
arb-core = { path = "../../crates/arb-core" }
pm-api = { path = "../../crates/pm-api" }
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
zstd.workspace = true
clap.workspace = true
anyhow.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing test.** At the bottom of `spikes/probe/src/universe.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pm_api::gamma::parse_page;

    const PAGE: &str = r#"{"events":[
      {"id":"2","title":"B","negRisk":true,"markets":[
        {"id":"21","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"b1y\",\"b1n\"]"},
        {"id":"22","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"b2y\",\"b2n\"]"},
        {"id":"23","active":true,"closed":true,"outcomePrices":"[\"0\",\"1\"]","clobTokenIds":"[\"b3y\",\"b3n\"]"}]},
      {"id":"1","title":"A","negRisk":true,"markets":[
        {"id":"11","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"a1y\",\"a1n\"]"}]},
      {"id":"3","title":"binary","negRisk":false,"markets":[
        {"id":"32","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"c2y\",\"c2n\"]"},
        {"id":"31","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"c1y\",\"c1n\"]"}]}
    ],"next_cursor":null}"#;

    #[test]
    fn selects_live_negrisk_legs_and_sorted_canaries() {
        let events = parse_page(PAGE).unwrap().events;
        let u = select(&events, 1);
        // event 1 has a single live leg → not a group; event 2 keeps its 2 live legs, not the closed one
        assert_eq!(u.groups.len(), 1);
        assert_eq!(u.groups[0].event_id, "2");
        assert_eq!(u.groups[0].yes_tokens, vec!["b1y", "b2y"]);
        // canaries sorted by market id, truncated to 1
        assert_eq!(u.canary, vec![CanaryPair { market_id: "31".into(), yes: "c1y".into(), no: "c1n".into() }]);
        assert_eq!(u.token_groups(), vec![vec!["b1y".to_string(), "b2y".to_string()], vec!["c1y".to_string(), "c1n".to_string()]]);
    }
}
```

- [ ] **Step 3: Run to verify it fails.** Run `cargo test -p probe`. Expected: FAIL to compile (`select` missing).

- [ ] **Step 4: Implement** `spikes/probe/src/universe.rs` (above the tests):

```rust
//! Universe for the probe: NegRisk groups (YES tokens of live legs) + canary binary pairs.
use arb_core::detect::LegStatus;
use pm_api::gamma::{leg_status, token_ids, GammaEvent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub event_id: String,
    pub title: String,
    pub yes_tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryPair {
    pub market_id: String,
    pub yes: String,
    pub no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Universe {
    pub groups: Vec<Group>,
    pub canary: Vec<CanaryPair>,
}

pub fn select(events: &[GammaEvent], canary_n: usize) -> Universe {
    let mut groups = Vec::new();
    let mut binaries = Vec::new();
    for e in events {
        if e.is_neg_risk() {
            let aug = e.is_augmented();
            let yes: Vec<String> = e
                .markets()
                .iter()
                .filter(|m| leg_status(m, aug) == LegStatus::Live)
                .filter_map(|m| token_ids(m).ok().flatten().map(|(y, _)| y))
                .collect();
            if yes.len() >= 2 {
                groups.push(Group { event_id: e.id.clone(), title: e.title.clone().unwrap_or_default(), yes_tokens: yes });
            }
        } else {
            for m in e.markets() {
                if leg_status(m, false) == LegStatus::Live {
                    if let Ok(Some((y, n))) = token_ids(m) {
                        binaries.push(CanaryPair { market_id: m.id.clone(), yes: y, no: n });
                    }
                }
            }
        }
    }
    binaries.sort_by(|a, b| a.market_id.cmp(&b.market_id));
    binaries.dedup_by(|a, b| a.market_id == b.market_id);
    binaries.truncate(canary_n);
    groups.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    Universe { groups, canary: binaries }
}

impl Universe {
    pub fn token_groups(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = self.groups.iter().map(|g| g.yes_tokens.clone()).collect();
        out.extend(self.canary.iter().map(|c| vec![c.yes.clone(), c.no.clone()]));
        out
    }
}
```
(`Universe::load` arrives in Task 2, where it is first used, so clippy's dead-code check stays green here.)

`spikes/probe/src/main.rs`:
```rust
//! Phase 0 measurement probe (throwaway). Read-only; see spikes/probe/RUNBOOK.md.
#![forbid(unsafe_code)]
mod universe;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch active events from Gamma and write the probe universe.
    Universe {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 100)]
        canary: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Universe { out, canary } => {
            let events = pm_api::gamma::GammaClient::new()?.all_active_events().await?;
            let u = universe::select(&events, canary);
            let tokens: usize = u.token_groups().iter().map(Vec::len).sum();
            std::fs::write(&out, serde_json::to_string_pretty(&u)?)?;
            println!(
                "events={} negrisk_groups={} canary_pairs={} tokens={} → {}",
                events.len(),
                u.groups.len(),
                u.canary.len(),
                tokens,
                out.display()
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Run the tests and gates.** Run `cargo test -p probe && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS, clean.

- [ ] **Step 6: Commit.**
```bash
git add Cargo.toml Cargo.lock spikes/probe
git commit -m "add phase 0 probe with universe selection"
```

---

### Task 2: Event-affine shards and the `capture` subcommand

**Files:**
- Create: `spikes/probe/src/shards.rs`, `spikes/probe/src/capture.rs`
- Modify: `spikes/probe/src/main.rs`

**Interfaces:**
- Consumes: `Universe::token_groups` (Task 1); `pm_api::ws::{parse_frame, run_connection, Frame, MarketEvent}`.
- Produces:
  - `shards::pack_shards(&[Vec<String>], max_tokens: usize) -> Vec<Vec<String>>`
  - `capture::Row { recv_us: i64, conn: usize, shard_tokens: usize, bytes: usize, ok: bool, events: usize, server_ms: Option<i64>, kinds: String }` with `HEADER`, `to_csv(&self) -> String`, `from_csv(&str) -> Option<Row>`
  - `capture::summarize_frame(shard_tokens: usize, &Frame) -> (Row, Option<String>)`
  - `capture::capture(&Universe, shard_tokens: usize, max_conns: usize, minutes: u64, out: &Path) -> anyhow::Result<()>`

- [ ] **Step 1: Write the failing tests.** Bottom of `spikes/probe/src/shards.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn g(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    #[test]
    fn never_splits_a_group_and_respects_the_limit() {
        let groups = vec![g("a", 3), g("b", 2), g("c", 2), g("d", 7)];
        let shards = pack_shards(&groups, 5);
        // d (7 > 5) gets its own over-size shard; the rest fit in ≤ 5
        assert!(shards.iter().any(|s| s.len() == 7));
        for s in &shards {
            assert!(s.len() <= 5 || s.len() == 7);
        }
        for grp in &groups {
            let homes = shards.iter().filter(|s| s.contains(&grp[0])).count();
            assert_eq!(homes, 1);
            let home = shards.iter().find(|s| s.contains(&grp[0])).unwrap();
            assert!(grp.iter().all(|t| home.contains(t)), "group split across shards");
        }
        assert_eq!(shards.iter().map(Vec::len).sum::<usize>(), 14);
    }
}
```
Bottom of `spikes/probe/src/capture.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pm_api::ws::Frame;

    #[test]
    fn summarizes_a_price_change_frame() {
        let text = r#"{"event_type":"price_change","market":"m","timestamp":"1790121493000","price_changes":[
            {"asset_id":"1","price":"0.5","size":"1","side":"BUY"},{"asset_id":"2","price":"0.5","size":"0","side":"SELL"}]}"#;
        let f = Frame { conn: 3, recv_us: 1_790_121_493_250_000, text: text.into() };
        let (row, err) = summarize_frame(50, &f);
        assert!(err.is_none());
        assert_eq!(row, Row { recv_us: 1_790_121_493_250_000, conn: 3, shard_tokens: 50, bytes: text.len(), ok: true,
                              events: 2, server_ms: Some(1_790_121_493_000), kinds: "price_change".into() });
        assert_eq!(Row::from_csv(&row.to_csv()), Some(row));
    }

    #[test]
    fn unparseable_frame_is_counted_not_fatal() {
        let f = Frame { conn: 0, recv_us: 1, text: "{broken".into() };
        let (row, err) = summarize_frame(5, &f);
        assert!(!row.ok && err.is_some() && row.server_ms.is_none());
        assert_eq!(Row::from_csv(&row.to_csv()), Some(row));
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Add `mod shards; mod capture;` to `main.rs`, then run `cargo test -p probe`. Expected: FAIL to compile.

- [ ] **Step 3: Implement.**

`spikes/probe/src/shards.rs`:
```rust
//! Event-affine shard packing (spec §7): first-fit decreasing; a group is never split.
pub fn pack_shards(groups: &[Vec<String>], max_tokens: usize) -> Vec<Vec<String>> {
    let mut sorted: Vec<&Vec<String>> = groups.iter().collect();
    sorted.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut shards: Vec<Vec<String>> = Vec::new();
    for g in sorted {
        match shards.iter_mut().find(|s| s.len() + g.len() <= max_tokens) {
            Some(s) => s.extend(g.iter().cloned()),
            None => shards.push(g.clone()),
        }
    }
    shards
}
```

`spikes/probe/src/capture.rs`:
```rust
//! `capture`: event-affine shards → websocket sessions → raw zstd capture + per-frame CSV.
use crate::shards::pack_shards;
use crate::universe::Universe;
use pm_api::ws::{parse_frame, run_connection, Frame, MarketEvent};
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

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
            server_ms: if f[6].is_empty() { None } else { Some(f[6].parse().ok()?) },
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
            (Row { ok: true, events: evs.len(), server_ms, kinds: kinds.join("|"), ..base }, None)
        }
        Err(e) => (base, Some(e.to_string())),
    }
}

pub async fn capture(u: &Universe, shard_tokens: usize, max_conns: usize, minutes: u64, out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let shards: Vec<Vec<String>> = pack_shards(&u.token_groups(), shard_tokens).into_iter().take(max_conns).collect();
    let sizes: Vec<usize> = shards.iter().map(Vec::len).collect();
    let mut shard_csv = String::from("conn,tokens\n");
    for (i, n) in sizes.iter().enumerate() {
        shard_csv.push_str(&format!("{i},{n}\n"));
    }
    std::fs::write(out.join("shards.csv"), shard_csv)?;
    println!("shards={} tokens={}", shards.len(), sizes.iter().sum::<usize>());

    let (tx, mut rx) = mpsc::channel::<Frame>(20_000);
    for (i, tokens) in shards.into_iter().enumerate() {
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                let r = run_connection(i, tokens.clone(), tx.clone()).await;
                if tx.is_closed() {
                    break;
                }
                eprintln!("conn {i} ended: {r:?}; reconnecting in 5 s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        tokio::time::sleep(Duration::from_millis(500)).await; // ≤ 2 connects per second
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
        writeln!(raw, "{}", json!({ "recv_us": frame.recv_us, "conn": frame.conn, "raw": frame.text }))?;
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
```

Add to `impl Universe` in `spikes/probe/src/universe.rs` (and `use std::path::Path;` at the top):
```rust
    pub fn load(path: &Path) -> anyhow::Result<Universe> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
```

Add to the `Cmd` enum in `main.rs`:
```rust
    /// Capture live frames for N minutes with event-affine shards of ≤ shard_tokens.
    Capture {
        #[arg(long)]
        universe: PathBuf,
        #[arg(long)]
        shard_tokens: usize,
        #[arg(long, default_value_t = 16)]
        max_conns: usize,
        #[arg(long, default_value_t = 20)]
        minutes: u64,
        #[arg(long)]
        out: PathBuf,
    },
```
and the match arm:
```rust
        Cmd::Capture { universe, shard_tokens, max_conns, minutes, out } => {
            let u = universe::Universe::load(&universe)?;
            capture::capture(&u, shard_tokens, max_conns.min(16), minutes, &out).await?;
        }
```

- [ ] **Step 4: Run the tests and gates.** Run `cargo test -p probe && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS, clean.

- [ ] **Step 5: Commit.**
```bash
git add spikes/probe
git commit -m "add probe capture with event-affine shards and per-frame csv"
```

---

### Task 3: `conncap` and `mirror` subcommands

**Files:**
- Create: `spikes/probe/src/conncap.rs`, `spikes/probe/src/mirror.rs`
- Modify: `spikes/probe/src/main.rs`

**Interfaces:**
- Consumes:
  - `pm_api::routes::WS_MARKET`
  - `pm_api::ws::subscribe_message`
  - `pm_api::clob::ClobClient`
  - `BookWire::to_book`
  - `arb_core::book::Book::{no_asks_best_first, no_bids_best_first, asks_best_first, bids_best_first}`
- Produces:
  - `conncap::conncap(token: &str, max: usize, spacing_ms: u64, hold_secs: u64, out: &Path) -> anyhow::Result<()>`
  - `mirror::mirror_matches(yes: &Book, no: &Book) -> bool`
  - `mirror::mirror(&Universe, out: &Path) -> anyhow::Result<()>`

- [ ] **Step 1: Write the failing test.** Bottom of `spikes/probe/src/mirror.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use arb_core::money::{Price, Shares};

    fn lv(p: &str, s: &str) -> (Price, Shares) {
        (Price::parse(p).unwrap(), Shares::parse(s).unwrap())
    }

    #[test]
    fn detects_mirror_and_its_absence() {
        let mut yes = Book::default();
        yes.apply_snapshot(&[lv("0.54", "5603.74"), lv("0.53", "85887.16")], &[lv("0.55", "32356.03")], 1);
        let mut no = Book::default();
        no.apply_snapshot(&[lv("0.45", "32356.03")], &[lv("0.46", "5603.74"), lv("0.47", "85887.16")], 1);
        assert!(mirror_matches(&yes, &no));
        no.apply_snapshot(&[lv("0.45", "32356.03")], &[lv("0.46", "5603.74")], 1); // one level missing
        assert!(!mirror_matches(&yes, &no));
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Add `mod conncap; mod mirror;` to `main.rs`, then run `cargo test -p probe`. Expected: FAIL to compile.

- [ ] **Step 3: Implement.**

`spikes/probe/src/mirror.rs`:
```rust
//! L1 at scale: are NO books the exact mirror of YES books? (spec L1, Gebele App. A)
use crate::universe::Universe;
use arb_core::book::Book;
use pm_api::clob::{BookWire, ClobClient};
use std::collections::HashMap;
use std::path::Path;

pub fn mirror_matches(yes: &Book, no: &Book) -> bool {
    yes.no_asks_best_first() == no.asks_best_first() && yes.no_bids_best_first() == no.bids_best_first()
}

pub async fn mirror(u: &Universe, out: &Path) -> anyhow::Result<()> {
    let clob = ClobClient::new()?;
    let tokens: Vec<String> = u.canary.iter().flat_map(|c| [c.yes.clone(), c.no.clone()]).collect();
    let books: HashMap<String, BookWire> = clob.books(&tokens).await?.into_iter().map(|b| (b.asset_id.clone(), b)).collect();
    let mut lines = vec!["market_id,status,yes_levels".to_string()];
    let (mut ok, mut bad, mut missing) = (0usize, 0usize, 0usize);
    for c in &u.canary {
        let status = match (books.get(&c.yes), books.get(&c.no)) {
            (Some(y), Some(n)) => match (y.to_book(), n.to_book()) {
                (Ok(yb), Ok(nb)) if mirror_matches(&yb, &nb) => {
                    ok += 1;
                    "mirror"
                }
                (Ok(_), Ok(_)) => {
                    bad += 1;
                    "MISMATCH"
                }
                _ => {
                    bad += 1;
                    "parse_error"
                }
            },
            _ => {
                missing += 1;
                "missing"
            }
        };
        let levels = books.get(&c.yes).map_or(0, |b| b.bids.len() + b.asks.len());
        lines.push(format!("{},{status},{levels}", c.market_id));
    }
    lines.push(format!("# mirror={ok} mismatch_or_error={bad} missing={missing}"));
    std::fs::write(out, lines.join("\n") + "\n")?;
    println!("mirror={ok} mismatch_or_error={bad} missing={missing}");
    Ok(())
}
```

`spikes/probe/src/conncap.rs`:
```rust
//! Per-IP websocket connection cap, measured politely: 1 token each, spaced, stop at first refusal.
//! Sockets are opened only through pm-api's `run_connection` (deny.toml: only pm-api may use tungstenite).
use pm_api::ApiError;
use pm_api::ws::{run_connection, Frame};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

type Conn = (JoinHandle<Result<(), ApiError>>, mpsc::Receiver<Frame>);

pub async fn conncap(token: &str, max: usize, spacing_ms: u64, hold_secs: u64, out: &Path) -> anyhow::Result<()> {
    let mut conns: Vec<Conn> = Vec::new();
    let mut lines = vec!["n,ok,detail".to_string()];
    for n in 1..=max {
        let (tx, mut rx) = mpsc::channel::<Frame>(1024);
        let handle = tokio::spawn(run_connection(n, vec![token.to_string()], tx));
        // A connection counts as open once it delivers its first frame (the initial book dump).
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(_)) => {
                lines.push(format!("{n},1,"));
                conns.push((handle, rx));
            }
            Ok(None) => {
                let detail = match handle.await {
                    Ok(Err(e)) => e.to_string(),
                    Ok(Ok(())) => "closed before first frame".to_string(),
                    Err(e) => e.to_string(),
                };
                lines.push(format!("{n},0,{}", detail.replace(',', ";")));
                break;
            }
            Err(_) => {
                handle.abort();
                lines.push(format!("{n},0,no frame within 15 s"));
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(spacing_ms.max(500))).await;
    }
    // Hold, draining every receiver so no connection ends on a full channel (ApiError::Dropped).
    // run_connection pings every 10 s and returns ApiError::Stale after 30 s of silence,
    // so a connection still running after the hold is alive.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(hold_secs);
    while tokio::time::Instant::now() < deadline {
        for (_, rx) in &mut conns {
            while rx.try_recv().is_ok() {}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let alive = conns.iter().filter(|(h, _)| !h.is_finished()).count();
    for (h, _) in &conns {
        h.abort();
    }
    lines.push(format!("# opened={} alive_after_hold={alive}", conns.len()));
    std::fs::write(out, lines.join("
") + "
")?;
    println!("opened={} alive_after_hold={alive}", conns.len());
    Ok(())
}
```

Add to `Cmd` in `main.rs`:
```rust
    /// Measure the per-IP connection cap (≤ 40, ≥ 0.5 s apart, stops at first refusal).
    Conncap {
        #[arg(long)]
        universe: PathBuf,
        #[arg(long, default_value_t = 40)]
        max: usize,
        #[arg(long, default_value_t = 3000)]
        spacing_ms: u64,
        #[arg(long, default_value_t = 60)]
        hold_secs: u64,
        #[arg(long)]
        out: PathBuf,
    },
    /// L1 check: compare YES/NO books of the canary pairs level by level.
    Mirror {
        #[arg(long)]
        universe: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
```
and the match arms:
```rust
        Cmd::Conncap { universe, max, spacing_ms, hold_secs, out } => {
            let u = universe::Universe::load(&universe)?;
            let token = u.canary.first().map(|c| c.yes.clone()).ok_or_else(|| anyhow::anyhow!("universe has no canary pairs"))?;
            conncap::conncap(&token, max.min(40), spacing_ms, hold_secs, &out).await?;
        }
        Cmd::Mirror { universe, out } => {
            let u = universe::Universe::load(&universe)?;
            mirror::mirror(&u, &out).await?;
        }
```

- [ ] **Step 4: Run the tests and gates.** Run `cargo test -p probe && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS, clean.

- [ ] **Step 5: Commit.**
```bash
git add spikes/probe
git commit -m "add probe connection-cap and mirror checks"
```

---

### Task 4: `report` subcommand (integer statistics)

**Files:**
- Create: `spikes/probe/src/report.rs`
- Modify: `spikes/probe/src/main.rs`

**Interfaces:**
- Consumes: `capture::Row::{HEADER, from_csv}` (Task 2).
- Produces:
  - `report::percentile(&[i64], pct: usize) -> Option<i64>` (input sorted ascending; index `(len−1)·pct/100`)
  - `report::Stats { shard_tokens: usize, conns: usize, frames: usize, parse_errors: usize, frames_per_s: i64, bytes_per_day: i64, lag_p50: Option<i64>, lag_p95: Option<i64>, lag_p99: Option<i64>, lag_min: Option<i64>, stalls: usize }`
  - `report::bucket_stats(&[Row]) -> Vec<Stats>` (one entry per distinct `shard_tokens`, ascending)
  - `report::render(&[Stats]) -> String` (Markdown table)
  - `report::report(dir: &Path) -> anyhow::Result<()>` (reads `frames.csv`, writes `report.md`)

- [ ] **Step 1: Write the failing test.** Bottom of `spikes/probe/src/report.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn row(recv_ms: i64, conn: usize, shard: usize, server_ms: Option<i64>, ok: bool) -> Row {
        Row { recv_us: recv_ms * 1_000, conn, shard_tokens: shard, bytes: 100, ok, events: 1, server_ms, kinds: "book".into() }
    }

    #[test]
    fn percentile_by_sorted_index() {
        let v = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50), Some(5));
        assert_eq!(percentile(&v, 100), Some(10));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn stats_per_shard_size_with_negative_lag_kept() {
        // shard 50: 3 frames over 10 s; lags (recv − server) = −5, 20, 900 ms
        let rows = vec![
            row(1_000, 0, 50, Some(1_005), true),
            row(6_000, 0, 50, Some(5_980), true),
            row(11_000, 1, 50, Some(10_100), true),
            row(11_000, 2, 500, None, false),
        ];
        let s = bucket_stats(&rows);
        assert_eq!(s.len(), 2);
        let a = &s[0];
        assert_eq!((a.shard_tokens, a.conns, a.frames, a.parse_errors), (50, 2, 3, 0));
        assert_eq!(a.lag_min, Some(-5), "negative lag is the clock-offset signal; never clamp it");
        assert_eq!(a.lag_p50, Some(20));
        assert_eq!(a.stalls, 1, "900 ms is > p5 (−5) + 500");
        assert_eq!(a.frames_per_s, 0, "3 frames / 10 s rounds down");
        assert_eq!(a.bytes_per_day, 300 * 86_400 / 10);
        assert_eq!((s[1].shard_tokens, s[1].parse_errors, s[1].lag_p50), (500, 1, None));
        assert!(render(&s).contains("| 50 |"));
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Add `mod report;` to `main.rs`, then run `cargo test -p probe`. Expected: FAIL to compile.

- [ ] **Step 3: Implement** `spikes/probe/src/report.rs` (above the tests):
```rust
//! Integer statistics over frames.csv (no floats): rates, bytes/day, lag percentiles, stalls.
use crate::capture::Row;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn percentile(sorted: &[i64], pct: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = (sorted.len() - 1) * pct.min(100) / 100;
    sorted.get(idx).copied()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub shard_tokens: usize,
    pub conns: usize,
    pub frames: usize,
    pub parse_errors: usize,
    pub frames_per_s: i64,
    pub bytes_per_day: i64,
    pub lag_p50: Option<i64>,
    pub lag_p95: Option<i64>,
    pub lag_p99: Option<i64>,
    pub lag_min: Option<i64>,
    /// Frames whose lag exceeds this bucket's p5 lag + 500 ms (p5 absorbs the clock offset).
    pub stalls: usize,
}

pub fn bucket_stats(rows: &[Row]) -> Vec<Stats> {
    let mut by: BTreeMap<usize, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by.entry(r.shard_tokens).or_default().push(r);
    }
    by.into_iter()
        .map(|(shard_tokens, rs)| {
            let conns: BTreeSet<usize> = rs.iter().map(|r| r.conn).collect();
            let t0 = rs.iter().map(|r| r.recv_us).min().unwrap_or(0);
            let t1 = rs.iter().map(|r| r.recv_us).max().unwrap_or(0);
            let secs = ((t1 - t0) / 1_000_000).max(1);
            let bytes: i64 = rs.iter().map(|r| i64::try_from(r.bytes).unwrap_or(0)).sum();
            let frames = rs.len();
            let mut lags: Vec<i64> = rs.iter().filter_map(|r| r.server_ms.map(|s| r.recv_us / 1_000 - s)).collect();
            lags.sort_unstable();
            let p5 = percentile(&lags, 5);
            let stalls = p5.map_or(0, |p| lags.iter().filter(|&&l| l > p + 500).count());
            Stats {
                shard_tokens,
                conns: conns.len(),
                frames,
                parse_errors: rs.iter().filter(|r| !r.ok).count(),
                frames_per_s: i64::try_from(frames).unwrap_or(0) / secs,
                bytes_per_day: bytes * 86_400 / secs,
                lag_p50: percentile(&lags, 50),
                lag_p95: percentile(&lags, 95),
                lag_p99: percentile(&lags, 99),
                lag_min: lags.first().copied(),
                stalls,
            }
        })
        .collect()
}

fn o(v: Option<i64>) -> String {
    v.map_or("-".to_string(), |x| x.to_string())
}

pub fn render(stats: &[Stats]) -> String {
    let mut s = String::from(
        "| shard tokens | conns | frames | parse errors | frames/s | bytes/day | lag p50 ms | p95 | p99 | min | stalls (>p5+500ms) |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for x in stats {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            x.shard_tokens, x.conns, x.frames, x.parse_errors, x.frames_per_s, x.bytes_per_day,
            o(x.lag_p50), o(x.lag_p95), o(x.lag_p99), o(x.lag_min), x.stalls
        ));
    }
    s
}

pub fn report(dir: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(dir.join("frames.csv"))?;
    let rows: Vec<Row> = text.lines().filter(|l| *l != Row::HEADER).filter_map(Row::from_csv).collect();
    let md = render(&bucket_stats(&rows));
    std::fs::write(dir.join("report.md"), &md)?;
    println!("{md}");
    Ok(())
}
```
Add to `Cmd`:
```rust
    /// Summarize a capture directory (frames.csv → report.md).
    Report {
        #[arg(long)]
        dir: PathBuf,
    },
```
and the match arm `Cmd::Report { dir } => report::report(&dir)?,`.

- [ ] **Step 4: Run the tests and gates.** Run `cargo test -p probe && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`. Expected: PASS, clean.

- [ ] **Step 5: Commit.**
```bash
git add spikes/probe
git commit -m "add probe report with integer lag and stall statistics"
```

---

### Task 5: Docker image, runbook and results template

**Files:**
- Create: `.dockerignore`, `spikes/probe/Dockerfile`, `spikes/probe/RUNBOOK.md`, `spikes/probe/RESULTS.md`
- Modify: `.gitignore` (add `spikes/probe/results/`)

**Interfaces:**
- Produces: the image `cute-probe`, and the owner's runbook.

- [ ] **Step 1: Write the files.**

`.dockerignore`:
```
target
frontend/node_modules
frontend/dist
backend
.superpowers
.git
spikes/probe/results
```

`spikes/probe/Dockerfile`:
```dockerfile
# Build context: repository root. docker build -f spikes/probe/Dockerfile -t cute-probe .
FROM rust:1.98.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p probe

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/probe /usr/local/bin/probe
WORKDIR /data
ENTRYPOINT ["probe"]
```

`spikes/probe/RUNBOOK.md`:
````markdown
# Phase 0 probe: runbook (≈ 1.5 h, mostly waiting)

Everything runs in Docker Desktop (WSL2), so it measures the same environment S2c will run in.
Keep the PC awake for the whole run.

```powershell
# from the repo root
docker build -f spikes/probe/Dockerfile -t cute-probe .
docker volume create probe-data

# 1. universe (≈ 1 min)
docker run --rm -v probe-data:/data cute-probe universe --out /data/universe.json

# 2. connection cap (≈ 3 min: up to 40 connections, 3 s apart, stops at the first refusal)
docker run --rm -v probe-data:/data cute-probe conncap --universe /data/universe.json --out /data/conncap.csv

# 3. captures, 20 min each; shard size is the variable (max 16 connections)
docker run --rm -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 50   --minutes 20 --out /data/run-50
docker run --rm -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 500  --minutes 20 --out /data/run-500
docker run --rm -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 5000 --minutes 20 --out /data/run-5000

# 4. mirror check (≈ 10 s)
docker run --rm -v probe-data:/data cute-probe mirror --universe /data/universe.json --out /data/mirror.csv

# 5. reports
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-50
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-500
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-5000

# 6. copy the small outputs to the repo (raw captures stay in the volume)
docker run --rm -v probe-data:/data -v "${PWD}/spikes/probe/results:/out" debian:bookworm-slim sh -c "cp /data/universe.json /data/conncap.csv /data/mirror.csv /out/ && for d in /data/run-*; do mkdir -p /out/\$(basename \$d); cp \$d/report.md \$d/shards.csv \$d/errors.jsonl /out/\$(basename \$d)/; done"
```

Then fill in `spikes/probe/RESULTS.md` (or paste the outputs to Claude, which will fill it in).
````

`spikes/probe/RESULTS.md`:
```markdown
# Phase 0 results (fill in after the run)

Run date / machine: …

| Question | Answer | Evidence |
|---|---|---|
| Q7a: per-IP websocket connection cap | opened = …, first refusal at n = … (or none up to 40) | conncap.csv |
| Q7b: message rate for the NegRisk universe (run-5000 covers it) | … frames/s | run-5000/report.md |
| Q7c: bytes/day of raw capture (uncompressed) | … | run-5000/report.md |
| Q7d: stall rate vs shard size (50 / 500 / 5000) | … / … / … stalls | run-*/report.md |
| Q7e: chosen shard size + connection budget for S2c | … tokens/conn, … connections | reasoning: … |
| Q17: WSL2 clock offset (min lag; negative = server ahead) | … ms | run-*/report.md `min` |
| L1 at scale: mirror on canary pairs | mirror = … / mismatch = … / missing = … | mirror.csv |
| pm-api on live traffic: parse errors | … per … frames; shapes to fix: … | run-*/errors.jsonl |
```

`.gitignore`: add a line `spikes/probe/results/`.

- [ ] **Step 2: Verify the image builds.** Run `docker build -f spikes/probe/Dockerfile -t cute-probe .`. Expected: build succeeds. If the `rust:1.98.1-bookworm` tag doesn't exist, use the closest `rust:1.98-bookworm` tag and note it.

- [ ] **Step 3: Smoke-test in Docker** (≈ 2 min): `docker run --rm -v probe-data:/data cute-probe universe --out /data/universe.json`, then `docker run --rm -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 50 --max-conns 2 --minutes 1 --out /data/smoke`, then `docker run --rm -v probe-data:/data cute-probe report --dir /data/smoke`. Expected: a report table with frames > 0; `parse_errors` printed.

- [ ] **Step 4: Commit and push.**
```bash
git add .dockerignore .gitignore spikes/probe
git commit -m "add probe docker image, runbook and results template"
git push
```

- [ ] **Step 5 (owner): Run `RUNBOOK.md`** and fill in `RESULTS.md`, or paste the outputs. That unlocks the S2b and S2c plans.
