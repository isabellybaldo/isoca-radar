use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("api: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> anyhow::Result<()> {
    api::guard::check_env()?;
    let cfg = api::config::Config::from_env()?;
    let opts = store::Store::options(&cfg.database_url)?;
    let store =
        store::Store::connect_read_only(opts, cfg.max_conns, cfg.statement_timeout_ms, None)
            .await?;
    api::ensure_read_only(&store).await?;
    // Phase 7a §5.1: the second pool, the api's one write. Unset: POST /api/paper/reset is a 503.
    let reset = match &cfg.reset_database_url {
        Some(url) => {
            // connect_reset's statement_timeout replaces the role's 5 s default: never above it.
            // A failed connect degrades to None; only an over-granted role stops the api.
            let connected = async {
                let opts = store::Store::options(url)?;
                Ok(
                    store::Store::connect_reset(opts, cfg.statement_timeout_ms.min(5_000), None)
                        .await?,
                )
            }
            .await;
            api::reset_pool(connected).await?
        }
        None => {
            eprintln!(
                "api: RESET_DATABASE_URL is not set: POST /api/paper/reset answers 503 reset_disabled"
            );
            None
        }
    };
    // Phase 7a §7.1: the ladder's latencies. Missing or unreadable: every rung's latency is null.
    let scenarios = match std::fs::read_to_string(&cfg.policies_path) {
        Ok(text) => match cards::scenarios::parse(&text) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("api: {}: {e}; ladder latencies are null", cfg.policies_path);
                None
            }
        },
        Err(e) => {
            eprintln!(
                "api: read {}: {e}; ladder latencies are null",
                cfg.policies_path
            );
            None
        }
    };
    let state = api::AppState::new(store)
        .with_reset(reset)
        .with_scenarios(scenarios);
    // Phase 7b §4: the public snapshot task, only with PUBLIC_DIR.
    let writer = api::public::writer_plan(&cfg).map(|plan| {
        eprintln!(
            "api: public snapshot every {} s into {}",
            plan.every.as_secs(),
            plan.dir.display()
        );
        tokio::spawn(api::public::run_writer(state.clone(), plan))
    });
    let served = api::run(
        state,
        &cfg.bind,
        std::time::Duration::from_secs(15),
        shutdown(),
    )
    .await;
    if let Some(w) = writer {
        w.abort();
    }
    served
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
