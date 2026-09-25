//! Phase 0 measurement probe (throwaway). Read-only; see spikes/probe/RUNBOOK.md.
#![forbid(unsafe_code)]
mod capture;
mod conncap;
mod mirror;
mod shards;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Universe { out, canary } => {
            let events = pm_api::gamma::GammaClient::new()?
                .all_active_events()
                .await?;
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
        Cmd::Capture {
            universe,
            shard_tokens,
            max_conns,
            minutes,
            out,
        } => {
            let u = universe::Universe::load(&universe)?;
            capture::capture(&u, shard_tokens, max_conns.min(16), minutes, &out).await?;
        }
        Cmd::Conncap {
            universe,
            max,
            spacing_ms,
            hold_secs,
            out,
        } => {
            let u = universe::Universe::load(&universe)?;
            let token = u
                .canary
                .first()
                .map(|c| c.yes.clone())
                .ok_or_else(|| anyhow::anyhow!("universe has no canary pairs"))?;
            conncap::conncap(&token, max.min(40), spacing_ms, hold_secs, &out).await?;
        }
        Cmd::Mirror { universe, out } => {
            let u = universe::Universe::load(&universe)?;
            mirror::mirror(&u, &out).await?;
        }
    }
    Ok(())
}
