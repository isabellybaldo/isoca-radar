#![allow(clippy::unwrap_used, clippy::expect_used)]
use pm_api::gamma::{Resolution, parse_markets, parse_resolutions, payout_vector};
use rust_decimal_macros::dec;

/// Shaped like `GET /markets?id=…&closed=true`: an array of markets, `outcomePrices` as a
/// JSON-encoded string (Gamma's habit), ids as strings or numbers, many fields we ignore.
const MARKETS: &str = r#"[
  {"id": "501001", "conditionId": "0xaa01", "question": "Will A win?", "slug": "will-a-win",
   "active": true, "closed": true, "acceptingOrders": false, "enableOrderBook": true, "negRisk": true,
   "clobTokenIds": "[\"111\", \"112\"]", "outcomes": "[\"Yes\", \"No\"]",
   "outcomePrices": "[\"1\", \"0\"]", "umaResolutionStatus": "resolved",
   "endDate": "2026-09-20T00:00:00Z", "closedTime": "2026-09-21 14:02:11+00"},
  {"id": "501002", "active": true, "closed": true, "outcomePrices": "[\"0\", \"1\"]",
   "umaResolutionStatus": "resolved"},
  {"id": 501003, "active": true, "closed": true, "outcomePrices": "[\"0.5\", \"0.5\"]",
   "umaResolutionStatus": "resolved"},
  {"id": "501004", "active": true, "closed": true, "outcomePrices": "[\"0.62\", \"0.38\"]",
   "umaResolutionStatus": "proposed"},
  {"id": "501005", "active": true, "closed": false, "outcomePrices": "[\"0.45\", \"0.55\"]"},
  {"id": "501006", "closed": true, "outcomePrices": "[\"0.7\", \"0.7\"]", "umaResolutionStatus": "resolved"},
  {"id": "501007", "closed": true, "outcomePrices": "[\"1.2\", \"-0.2\"]", "umaResolutionStatus": "resolved"},
  {"id": "501008", "closed": true, "outcomePrices": null},
  {"id": "501009", "closed": true, "outcomePrices": "[\"1\", \"0\"]"},
  {"id": "501010", "closed": true, "outcomePrices": "[\"0.5\", \"0.5\"]"}
]"#;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn all_ids() -> Vec<String> {
    (1..=10).map(|i| format!("5010{i:02}")).collect()
}

fn r(id: &str, y: rust_decimal::Decimal, n: rust_decimal::Decimal) -> Resolution {
    Resolution {
        market_id: id.into(),
        payout_yes: y,
        payout_no: n,
    }
}

#[test]
fn parses_payout_vectors_exactly() {
    assert_eq!(
        parse_resolutions(MARKETS, &all_ids()).unwrap(),
        vec![
            r("501001", dec!(1), dec!(0)),
            r("501002", dec!(0), dec!(1)),
            r("501003", dec!(0.5), dec!(0.5)), // Q19: a confirmed 50-50 is a payout, flagged later
            r("501009", dec!(1), dec!(0)),     // binary: accepted without a UMA status
        ]
    );
}

#[test]
fn a_closed_market_awaiting_resolution_is_not_a_payout() {
    let ms = parse_markets(MARKETS).unwrap();
    let by_id = |id: &str| ms.iter().find(|m| m.id == id).unwrap();
    assert_eq!(
        payout_vector(by_id("501004")),
        None,
        "closed, still in dispute: trading prices"
    );
    assert_eq!(payout_vector(by_id("501005")), None, "still open");
    assert_eq!(payout_vector(by_id("501006")), None, "sums to 1.4");
    assert_eq!(payout_vector(by_id("501007")), None, "outside [0, 1]");
    assert_eq!(payout_vector(by_id("501008")), None, "no prices");
    assert_eq!(
        payout_vector(by_id("501010")),
        None,
        "fractional without a resolved status"
    );
    assert_eq!(payout_vector(by_id("501003")), Some((dec!(0.5), dec!(0.5))));
}

#[test]
fn only_the_requested_markets_come_back() {
    // if Gamma ever ignores the id filter, unrelated closed markets must not leak through
    assert_eq!(
        parse_resolutions(MARKETS, &ids(&["501002", "999"])).unwrap(),
        vec![r("501002", dec!(0), dec!(1))]
    );
    assert!(parse_resolutions(MARKETS, &[]).unwrap().is_empty());
}

#[test]
fn a_body_that_is_not_a_market_list_is_an_error() {
    assert!(parse_resolutions(r#"{"error": "rate limited"}"#, &all_ids()).is_err());
    assert!(parse_resolutions("[]", &all_ids()).unwrap().is_empty());
}
