//! Phase 0 measurement probe (throwaway). Read-only; see spikes/probe/RUNBOOK.md.
#![forbid(unsafe_code)]
mod capture;
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
    }
    Ok(())
}
