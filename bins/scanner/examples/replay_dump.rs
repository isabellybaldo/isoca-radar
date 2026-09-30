//! 7b §3.3, the replay identity check: replay one capture directory with a policies file through
//! the engine and the paper trader, and write every paper fill, exec and ledger row, one per line,
//! in the order the trader emitted them. `scripts/replay-identity.sh` runs this same file at the
//! base commit and at the branch and compares the two dumps' sha256.
//!     cargo run -p scanner --example replay_dump -- <capture dir> <scanner.toml> <policies.toml> <out>
use anyhow::Context;
use scanner::config::Config;
use scanner::engine::{EngineCtx, Out};
use scanner::paper::parse_policies;
use scanner::replay::replay_dir_paper_with;
use sim::rows::SimOut;
use sim::trader::PaperTrader;
use std::fmt::Write as _;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, scanner_toml, policies_toml, out] = args.as_slice() else {
        anyhow::bail!("usage: replay_dump <capture dir> <scanner.toml> <policies.toml> <out file>");
    };
    let (cfg, hash) = Config::load(Path::new(scanner_toml))?;
    let text =
        std::fs::read_to_string(policies_toml).with_context(|| format!("read {policies_toml}"))?;
    let policies = parse_policies(&text)?;
    let ctx = EngineCtx {
        run_id: 0,
        source: "replay".to_string(),
        scanner_version: "replay-dump".to_string(),
        config_hash: hash,
        gap_ms: cfg.engine.gap_ms,
        stale_lag_ms: cfg.engine.stale_lag_ms,
        suspect_window_ms: cfg.engine.suspect_window_ms,
        l1_suspect_window_ms: cfg.engine.l1_suspect_window_ms,
        drift_settle_ms: cfg.engine.drift_settle_ms,
        issue_throttle_ms: cfg.engine.issue_throttle_ms,
    };
    let trader = PaperTrader::new(policies.sim).context("paper trader")?;
    let mut dump = String::new();
    let mut rows = 0u64;
    let stats = replay_dir_paper_with(Path::new(dir), ctx, Some(trader), |o| {
        if let Out::Paper(s) = o {
            let line = match *s {
                SimOut::Fill(f) => format!("fill {f:?}"),
                SimOut::Exec(e) => format!("exec {e:?}"),
                SimOut::Ledger(l) => format!("ledger {l:?}"),
                _ => return,
            };
            let _ = writeln!(dump, "{line}");
            rows += 1;
        }
    })?;
    std::fs::write(out, dump).with_context(|| format!("write {out}"))?;
    println!(
        "frames {} parse_errors {} paper_rows {rows}",
        stats.frames, stats.parse_errors
    );
    Ok(())
}
