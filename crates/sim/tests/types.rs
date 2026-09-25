#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sim::types::{LegKey, Outcome, vecmap};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Holder {
    #[serde(with = "vecmap")]
    m: BTreeMap<LegKey, Decimal>,
}

#[test]
fn struct_keyed_maps_round_trip_through_json_exactly() {
    let k = LegKey {
        market_id: "m1".into(),
        yes_token: "y1".into(),
        outcome: Outcome::No,
    };
    let h = Holder {
        m: BTreeMap::from([(k, Decimal::from_str_exact("0.1").unwrap())]),
    };
    let json = serde_json::to_string(&h).unwrap();
    assert!(
        json.contains("\"0.1\""),
        "decimals serialize as exact strings: {json}"
    );
    assert_eq!(serde_json::from_str::<Holder>(&json).unwrap(), h);
}
