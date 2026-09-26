use clap::{Parser, Subcommand};
use scanner::config::Config;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "scanner",
    about = "read-only Polymarket arbitrage scanner (paper trading only)"
)]
struct Cli {
    #[arg(long, global = true, default_value = "/etc/cute-trading/scanner.toml")]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the live scanner.
    Run,
    /// Fetch the universe and print the shard plan (no websocket connections).
    Census,
    /// Apply database migrations (needs OWNER_DATABASE_URL).
    Migrate,
    /// Replay a capture directory through the engine.
    Replay {
        dir: PathBuf,
        /// Print counts only; write nothing to Postgres.
        #[arg(long)]
        dry_run: bool,
    },
    /// Crash-loop breaker controls.
    Breaker {
        #[command(subcommand)]
        action: BreakerCmd,
    },
}

#[derive(Subcommand)]
enum BreakerCmd {
    /// Forget recent starts so the next `run` goes live again.
    Reset,
}

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scanner: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    scanner::guard::check_env()?;
    let cli = Cli::parse();
    let (cfg, hash) = Config::load(&cli.config)?;
    match cli.cmd {
        Cmd::Run => scanner::run::run(cfg, hash).await,
        Cmd::Census => scanner::run::census(&cfg).await,
        Cmd::Migrate => scanner::run::migrate().await,
        Cmd::Replay { dir, dry_run } => {
            let (policies, hash) = scanner::paper::with_policies(&cfg.paper, &hash)?;
            scanner::run::replay_cmd(&cfg, &hash, &dir, dry_run, policies.as_ref()).await
        }
        Cmd::Breaker {
            action: BreakerCmd::Reset,
        } => scanner::run::breaker_reset().await,
    }
}
