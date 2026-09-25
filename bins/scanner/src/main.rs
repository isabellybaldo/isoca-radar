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
    let (_cfg, hash) = Config::load(&cli.config)?;
    let name = match cli.cmd {
        Cmd::Run => "run",
        Cmd::Census => "census",
        Cmd::Migrate => "migrate",
        Cmd::Replay { .. } => "replay",
        Cmd::Breaker { .. } => "breaker",
    };
    eprintln!(
        "scanner {} config {hash}: `{name}` is wired up in Task 10",
        scanner::version()
    );
    Ok(())
}
