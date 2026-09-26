use analyze::config::Config;
use analyze::tick::Analyzer;
use cards::body::ScenarioLat;
use std::process::ExitCode;
use std::time::Duration;
use store::{PgConnectOptions, Store};

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("analyze: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    // refusals and bad configuration stop the process; a database hiccup never does (no crash loop)
    analyze::guard::check_env()?;
    let cfg = Config::from_env()?;
    let scenarios = analyze::scenarios::load(&cfg.policies_path)?;
    let opts = Store::options(&cfg.database_url)?;
    eprintln!(
        "analyze: every {} s (writes card tables only, paper only)",
        cfg.every_secs
    );
    let mut every = tokio::time::interval(Duration::from_secs(cfg.every_secs));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let stop = shutdown();
    tokio::pin!(stop);
    let mut running: Option<(Store, Analyzer)> = None;
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = every.tick() => {}
        }
        if running.is_none() {
            match start(&cfg, opts.clone(), &scenarios).await? {
                Ok(started) => running = Some(started),
                Err(e) => {
                    eprintln!("analyze: start failed, retrying next tick: {e:#}");
                    continue;
                }
            }
        }
        let Some((_, a)) = running.as_mut() else {
            continue;
        };
        let now = analyze::now_ms();
        match a.tick(now).await {
            Ok(r) => eprintln!(
                "analyze: tick {} ms, {} dirty hours, {} catch-up hours, {} results",
                r.ms, r.dirty_hours, r.catch_up_hours, r.results
            ),
            Err(e) => {
                eprintln!("analyze: tick failed: {e:#}");
                a.record_error(now, &format!("{e:#}")).await;
                a.reset();
            }
        }
    }
    if let Some((store, _)) = running {
        store.close().await;
    }
    Ok(())
}

/// Connect, check the role and load the analyzer. `Err`: a refusal (the role can write other
/// tables), fatal. `Ok(Err)`: the database could not be reached or read, retried next tick.
async fn start(
    cfg: &Config,
    opts: PgConnectOptions,
    scenarios: &[ScenarioLat],
) -> anyhow::Result<anyhow::Result<(Store, Analyzer)>> {
    let store = match Store::connect_analyze(opts, 2, cfg.statement_timeout_ms, None).await {
        Ok(s) => s,
        Err(e) => return Ok(Err(e.into())),
    };
    let bad = match store.role_writes_outside_cards().await {
        Ok(bad) => bad,
        Err(e) => {
            store.close().await;
            return Ok(Err(e.into()));
        }
    };
    if let Err(refusal) = analyze::refuse_other_writes(&bad) {
        store.close().await;
        return Err(refusal);
    }
    let pause = Duration::from_millis(cfg.catchup_pause_ms);
    let loaded =
        match Analyzer::new(store.clone(), pause, analyze::version(), scenarios.to_vec()).await {
            Ok(a) => a.write_static_meta().await.map(|()| a),
            Err(e) => Err(e),
        };
    match loaded {
        Ok(a) => Ok(Ok((store, a))),
        Err(e) => {
            store.close().await;
            Ok(Err(e))
        }
    }
}

/// SIGTERM (docker stop) or Ctrl-C. A tick in progress finishes first (its writes are idempotent).
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
