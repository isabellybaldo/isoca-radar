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
    }
    Ok(())
}
