//! Universe for the probe: NegRisk groups (YES tokens of live legs) + canary binary pairs.
use arb_core::detect::LegStatus;
use pm_api::gamma::{GammaEvent, leg_status, token_ids};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub event_id: String,
    pub title: String,
    pub yes_tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryPair {
    pub market_id: String,
    pub yes: String,
    pub no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Universe {
    pub groups: Vec<Group>,
    pub canary: Vec<CanaryPair>,
}

pub fn select(events: &[GammaEvent], canary_n: usize) -> Universe {
    let mut groups = Vec::new();
    let mut binaries = Vec::new();
    for e in events {
        if e.is_neg_risk() {
            let aug = e.is_augmented();
            let yes: Vec<String> = e
                .markets()
                .iter()
                .filter(|m| leg_status(m, aug) == LegStatus::Live)
                .filter_map(|m| token_ids(m).ok().flatten().map(|(y, _)| y))
                .collect();
            if yes.len() >= 2 {
                groups.push(Group {
                    event_id: e.id.clone(),
                    title: e.title.clone().unwrap_or_default(),
                    yes_tokens: yes,
                });
            }
        } else {
            for m in e.markets() {
                if leg_status(m, false) == LegStatus::Live
                    && let Ok(Some((y, n))) = token_ids(m)
                {
                    binaries.push(CanaryPair {
                        market_id: m.id.clone(),
                        yes: y,
                        no: n,
                    });
                }
            }
        }
    }
    binaries.sort_by(|a, b| a.market_id.cmp(&b.market_id));
    binaries.dedup_by(|a, b| a.market_id == b.market_id);
    binaries.truncate(canary_n);
    groups.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    Universe {
        groups,
        canary: binaries,
    }
}

impl Universe {
    pub fn token_groups(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = self.groups.iter().map(|g| g.yes_tokens.clone()).collect();
        out.extend(
            self.canary
                .iter()
                .map(|c| vec![c.yes.clone(), c.no.clone()]),
        );
        out
    }

    pub fn load(path: &Path) -> anyhow::Result<Universe> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_api::gamma::parse_page;

    const PAGE: &str = r#"{"events":[
      {"id":"2","title":"B","negRisk":true,"markets":[
        {"id":"21","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"b1y\",\"b1n\"]"},
        {"id":"22","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"b2y\",\"b2n\"]"},
        {"id":"23","active":true,"closed":true,"outcomePrices":"[\"0\",\"1\"]","clobTokenIds":"[\"b3y\",\"b3n\"]"}]},
      {"id":"1","title":"A","negRisk":true,"markets":[
        {"id":"11","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"a1y\",\"a1n\"]"}]},
      {"id":"3","title":"binary","negRisk":false,"markets":[
        {"id":"32","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"c2y\",\"c2n\"]"},
        {"id":"31","active":true,"closed":false,"acceptingOrders":true,"enableOrderBook":true,"clobTokenIds":"[\"c1y\",\"c1n\"]"}]}
    ],"next_cursor":null}"#;

    #[test]
    fn selects_live_negrisk_legs_and_sorted_canaries() {
        let events = parse_page(PAGE).unwrap().events;
        let u = select(&events, 1);
        // event 1 has a single live leg → not a group; event 2 keeps its 2 live legs, not the closed one
        assert_eq!(u.groups.len(), 1);
        assert_eq!(u.groups[0].event_id, "2");
        assert_eq!(u.groups[0].yes_tokens, vec!["b1y", "b2y"]);
        // canaries sorted by market id, truncated to 1
        assert_eq!(
            u.canary,
            vec![CanaryPair {
                market_id: "31".into(),
                yes: "c1y".into(),
                no: "c1n".into()
            }]
        );
        assert_eq!(
            u.token_groups(),
            vec![
                vec!["b1y".to_string(), "b2y".to_string()],
                vec!["c1y".to_string(), "c1n".to_string()]
            ]
        );
    }
}
