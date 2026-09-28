use clap::{Parser, Subcommand};
use compact::config::Config;
use compact::run::{POLL_SECS, STATEMENT_TIMEOUT_MS, Service, lock_archive};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::{PgConnectOptions, Store};

#[derive(Parser)]
#[command(
    name = "compact",
    about = "raw capture hours → the Parquet archive (paper trading only)"
)]
struct Cli {
    #[arg(long, global = true, default_value = "/etc/cute-trading/compact.toml")]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the service (compact_rw via DATABASE_URL). One per archive (a lock in _state/).
    Run,
    /// Check one archived day (YYYY-MM-DD); exits non-zero on any problem.
    Verify {
        day: String,
        /// The archive folder (default: archive_dir from the config).
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Compact one raw hour file with no database; print time, rows, bytes and peak memory.
    Bench {
        hour_file: PathBuf,
        #[arg(long, default_value = "/tmp/compact-bench")]
        out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("compact: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    compact::guard::check_env()?;
    let cli = Cli::parse();
    let (cfg, sha) = Config::load(&cli.config)?;
    match cli.cmd {
        Cmd::Run => run(cfg, sha).await,
        Cmd::Verify { day, archive } => {
            let dir = archive.unwrap_or_else(|| PathBuf::from(&cfg.archive_dir));
            let problems = compact::verify::verify_day(&dir, &day)?;
            if problems.is_empty() {
                println!("verify {day}: ok");
                return Ok(());
            }
            for p in &problems {
                println!("{p}");
            }
            anyhow::bail!("verify {day}: {} problem(s)", problems.len())
        }
        Cmd::Bench { hour_file, out } => {
            println!("{}", compact::bench::bench(&cfg, &hour_file, &out)?);
            Ok(())
        }
    }
}

/// Connect and check the role. `Err`: a refusal (fatal). `Ok(Err)`: unreachable, retried next poll.
async fn connect(opts: &PgConnectOptions) -> anyhow::Result<anyhow::Result<Store>> {
    let s = match Store::connect_compact(opts.clone(), 2, STATEMENT_TIMEOUT_MS, None).await {
        Ok(s) => s,
        Err(e) => return Ok(Err(e.into())),
    };
    let bad = match s.role_writes_outside_compact().await {
        Ok(b) => b,
        Err(e) => {
            s.close().await;
            return Ok(Err(e.into()));
        }
    };
    if let Err(refusal) = compact::refuse_other_writes(&bad) {
        s.close().await;
        return Err(refusal);
    }
    Ok(Ok(s))
}

async fn run(cfg: Config, sha: String) -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is not set (the compact_rw URL)"))?;
    let opts = Store::options(&url)?;
    // Held until run returns: a second `compact run` on this archive refuses and exits non-zero.
    let _lock = lock_archive(Path::new(&cfg.archive_dir))?;
    eprintln!(
        "compact: every {POLL_SECS} s, {} → {} (writes compact_hours/compact_status only, paper only)",
        cfg.raw_dir, cfg.archive_dir
    );
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(tokio::sync::Notify::new());
    {
        let (stop, wake) = (stop.clone(), wake.clone());
        tokio::spawn(async move {
            shutdown().await;
            stop.store(true, Ordering::Relaxed);
            wake.notify_one();
        });
    }
    let mut svc = Service::new(cfg, sha);
    let mut every = tokio::time::interval(Duration::from_secs(POLL_SECS));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut store: Option<Store> = None;
    loop {
        tokio::select! {
            _ = wake.notified() => {}
            _ = every.tick() => {}
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if store.is_none() {
            match connect(&opts).await? {
                Ok(s) => store = Some(s),
                Err(e) => {
                    eprintln!("compact: database unreachable, retrying next poll: {e:#}");
                    continue;
                }
            }
        }
        let Some(s) = store.as_ref() else {
            continue;
        };
        // An hour in progress is never interrupted here; a SIGKILL mid-hour is safe (atomic writes).
        // Every poll: the ready hours, then the merge and the budget whatever failed, then status.
        let st = svc.poll(s, compact::now_ms, &stop).await;
        if let Err(e) = s.compact_status_put(&st).await {
            eprintln!("compact: status not written: {e:#}");
        }
    }
    if let Some(s) = store {
        s.close().await;
    }
    Ok(())
}

/// SIGTERM (docker stop) or Ctrl-C.
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
