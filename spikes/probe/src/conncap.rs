//! Per-IP websocket connection cap, measured politely: 1 token each, spaced, stop at first refusal.
//! Sockets are opened only through pm-api's `run_connection` (deny.toml: only pm-api may use tungstenite).
use pm_api::ApiError;
use pm_api::ws::{Frame, run_connection};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

type Conn = (JoinHandle<Result<(), ApiError>>, mpsc::Receiver<Frame>);

pub async fn conncap(
    token: &str,
    max: usize,
    spacing_ms: u64,
    hold_secs: u64,
    out: &Path,
) -> anyhow::Result<()> {
    let mut conns: Vec<Conn> = Vec::new();
    let mut lines = vec!["n,ok,detail".to_string()];
    for n in 1..=max {
        let (tx, mut rx) = mpsc::channel::<Frame>(1024);
        let handle = tokio::spawn(run_connection(n, vec![token.to_string()], tx));
        // A connection counts as open once it delivers its first frame (the initial book dump).
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(_)) => {
                lines.push(format!("{n},1,"));
                conns.push((handle, rx));
            }
            Ok(None) => {
                let detail = match handle.await {
                    Ok(Err(e)) => e.to_string(),
                    Ok(Ok(())) => "closed before first frame".to_string(),
                    Err(e) => e.to_string(),
                };
                lines.push(format!("{n},0,{}", detail.replace(',', ";")));
                break;
            }
            Err(_) => {
                handle.abort();
                lines.push(format!("{n},0,no frame within 15 s"));
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(spacing_ms.max(500))).await;
    }
    // Hold, draining every receiver so no connection ends on a full channel (ApiError::Dropped).
    // run_connection pings every 10 s and returns ApiError::Stale after 30 s of silence,
    // so a connection still running after the hold is alive.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(hold_secs);
    while tokio::time::Instant::now() < deadline {
        for (_, rx) in &mut conns {
            while rx.try_recv().is_ok() {}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let alive = conns.iter().filter(|(h, _)| !h.is_finished()).count();
    for (h, _) in &conns {
        h.abort();
    }
    lines.push(format!("# opened={} alive_after_hold={alive}", conns.len()));
    std::fs::write(out, lines.join("\n") + "\n")?;
    println!("opened={} alive_after_hold={alive}", conns.len());
    Ok(())
}
