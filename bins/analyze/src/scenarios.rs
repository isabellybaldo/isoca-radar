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

/// P1's thresholds from the same frozen policies.toml: SWEET's YES hurdle (min_annualized) and
/// the days edge its bucket closes on (Phase 5d spec §5, §7.5).
pub fn p1(path: &str) -> anyhow::Result<sim::config::P1Params> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let sim: sim::config::SimConfig = toml::from_str(&text).context("parse policies")?;
    sim.validate().context("validate policies")?;
    Ok(sim.p1)
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

    #[test]
    fn the_repo_policies_give_the_sweet_hurdle_and_days_edge() {
        let p = p1(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/policies.toml"
        ))
        .unwrap();
        assert_eq!(p.min_annualized, rust_decimal::Decimal::new(10, 2));
        assert_eq!(
            p.max_days_to_end,
            rust_decimal::Decimal::from(cards::sweet::P1_MAX_DAYS),
            "SWEET's days bucket closes at P1's max_days_to_end"
        );
    }
}
