//! The latency scenarios of policies.toml (the same frozen file the scanner's paper trader runs),
//! shared by analyze (Edge capture's decay chart) and the api (the ladder, spec 7a §7.1): x =
//! proc + net + match. Reads only `[[scenarios]]`; analyze also validates the whole file with sim.
use crate::body::ScenarioLat;
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    scenarios: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    name: String,
    proc_ms: i64,
    net_ms: i64,
    match_ms: i64,
}

pub fn parse(text: &str) -> Result<Vec<ScenarioLat>, String> {
    let f: File = toml::from_str(text).map_err(|e| format!("parse policies: {e}"))?;
    Ok(f.scenarios
        .into_iter()
        .map(|s| ScenarioLat {
            latency_ms: s.proc_ms + s.net_ms + s.match_ms,
            net_ms: s.net_ms,
            name: s.name,
        })
        .collect())
}
