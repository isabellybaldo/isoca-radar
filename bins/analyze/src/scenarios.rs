//! The latency scenarios of policies.toml (the same frozen file the scanner's paper trader runs):
//! x = proc + net + match for Edge capture's decay chart (spec §6.2).
use anyhow::Context;
use cards::body::ScenarioLat;

pub fn parse(text: &str) -> anyhow::Result<Vec<ScenarioLat>> {
    let sim: sim::config::SimConfig = toml::from_str(text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(sim
        .scenarios
        .iter()
        .map(|s| ScenarioLat {
            name: s.name.clone(),
            net_ms: s.net_ms,
            latency_ms: s.proc_ms + s.net_ms + s.match_ms,
        })
        .collect())
}

pub fn load(path: &str) -> anyhow::Result<Vec<ScenarioLat>> {
    parse(&std::fs::read_to_string(path).with_context(|| format!("read {path}"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repo_policies_give_four_scenarios() {
        let s = load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/policies.toml"
        ))
        .unwrap();
        let got: Vec<(&str, i64, i64)> = s
            .iter()
            .map(|x| (x.name.as_str(), x.net_ms, x.latency_ms))
            .collect();
        assert_eq!(
            got,
            vec![
                ("colo-idle", 1, 52),
                ("sp-idle", 145, 300),
                ("sp-burst", 145, 450),
                ("sp-idle-nodelay", 145, 300)
            ]
        );
    }
}
