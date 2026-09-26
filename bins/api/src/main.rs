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
    let state = api::AppState::new(store.clone());
    let _bridge = api::stream::spawn_bridge(
        store.clone(),
        state.events.clone(),
        std::time::Duration::from_secs(15),
    );
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    eprintln!("api: listening on {} (read-only, paper only)", cfg.bind);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    store.close().await;
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
