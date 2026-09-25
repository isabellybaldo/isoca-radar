#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::detect::LegStatus;
use arb_core::fee::FeeSchedule;
use arb_core::money::Price;
use pm_api::gamma::*;
use rust_decimal_macros::dec;

const PAGE: &str = r#"{
  "events": [
    {
      "id": "101", "title": "Fed Decision in October?", "negRisk": true, "enableNegRisk": true,
      "negRiskAugmented": false, "negRiskMarketID": "0x09ac",
      "markets": [
        {"id": 5001, "question": "No change", "active": true, "closed": false,
         "acceptingOrders": true, "enableOrderBook": true, "negRiskOther": false,
         "clobTokenIds": "[\"111\", \"222\"]", "outcomePrices": "[\"0.45\", \"0.55\"]",
         "feesEnabled": true, "feeSchedule": {"rate": 0.05, "exponent": 1, "takerOnly": true},
         "orderMinSize": 5, "secondsDelay": 1},
        {"id": "5002", "active": true, "closed": true, "acceptingOrders": false,
         "enableOrderBook": true, "clobTokenIds": "[\"333\", \"444\"]", "outcomePrices": "[\"0\", \"1\"]"},
        {"id": "5003", "active": null, "closed": null, "acceptingOrders": null,
         "enableOrderBook": null, "negRiskOther": null, "clobTokenIds": null, "feeSchedule": null}
      ]
    },
    {"id": 102, "title": "Brazil", "negRisk": true, "negRiskAugmented": true, "markets": null}
  ],
  "next_cursor": "abc"
}"#;

fn page() -> KeysetPage {
    parse_page(PAGE).unwrap()
}

#[test]
fn parses_numbers_strings_and_nulls() {
    let p = page();
    assert_eq!(p.next_cursor.as_deref(), Some("abc"));
    assert_eq!(p.events[0].id, "101");
    assert_eq!(p.events[1].id, "102"); // numeric id
    assert_eq!(p.events[0].markets()[0].id, "5001"); // numeric market id
    assert!(p.events[1].markets().is_empty()); // markets: null
    assert_eq!(p.events[0].neg_risk_market_id.as_deref(), Some("0x09ac"));
}

#[test]
fn token_ids_from_json_encoded_string() {
    let m = &page().events[0].markets()[0].clone();
    assert_eq!(
        token_ids(m).unwrap(),
        Some(("111".to_string(), "222".to_string()))
    );
    assert_eq!(token_ids(&page().events[0].markets()[2]).unwrap(), None); // null
}

#[test]
fn fee_schedule_is_exact() {
    let p = page();
    let m = &p.events[0].markets()[0];
    assert_eq!(
        fee_schedule(m).unwrap(),
        FeeSchedule::new(dec!(0.05), 1).unwrap()
    );
    // null schedule → zero fees
    assert_eq!(
        fee_schedule(&page().events[0].markets()[2]).unwrap(),
        FeeSchedule::ZERO
    );
    let fee = fee_schedule(m).unwrap();
    assert_eq!(
        fee.fee_exact(Price::parse("0.50").unwrap(), dec!(100)),
        dec!(1.25)
    );
}

#[test]
fn fees_disabled_means_zero() {
    let mut m = page().events[0].markets()[0].clone();
    m.fees_enabled = Some(false);
    assert_eq!(fee_schedule(&m).unwrap(), FeeSchedule::ZERO);
}

#[test]
fn leg_status_classification() {
    let p = page();
    let ms = p.events[0].markets();
    assert_eq!(leg_status(&ms[0], false), LegStatus::Live);
    assert_eq!(leg_status(&ms[1], false), LegStatus::ClosedNo); // outcomePrices ["0","1"]
    assert_eq!(leg_status(&ms[2], false), LegStatus::Halted); // all null, not augmented
    assert_eq!(leg_status(&ms[2], true), LegStatus::Placeholder); // same, augmented event
    let mut yes = ms[1].clone();
    yes.outcome_prices = Some(serde_json::json!("[\"1\", \"0\"]"));
    assert_eq!(leg_status(&yes, false), LegStatus::ClosedYes);
    let mut other = ms[0].clone();
    other.neg_risk_other = Some(true);
    assert_eq!(leg_status(&other, true), LegStatus::Other);
}

#[test]
fn neg_risk_flags() {
    let p = page();
    assert!(p.events[0].is_neg_risk() && !p.events[0].is_augmented());
    assert!(p.events[1].is_neg_risk() && p.events[1].is_augmented());
}

#[test]
fn malformed_token_ids_is_an_error_not_a_panic() {
    let mut m = page().events[0].markets()[0].clone();
    m.clob_token_ids = Some(serde_json::json!("[\"only-one\"]"));
    assert!(token_ids(&m).is_err());
    m.clob_token_ids = Some(serde_json::json!("not json"));
    assert!(token_ids(&m).is_err());
}

fn ev(id: &str) -> String {
    format!(r#"{{"id": "{id}", "markets": []}}"#)
}
fn page_json(ids: &[&str], cursor: Option<&str>) -> String {
    let events: Vec<String> = ids.iter().map(|i| ev(i)).collect();
    let cur = cursor.map_or("null".to_string(), |c| format!("\"{c}\""));
    format!(
        r#"{{"events": [{}], "next_cursor": {cur}}}"#,
        events.join(",")
    )
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn pager_follows_after_cursor_until_exhausted() {
    let pages = [
        page_json(&["1", "2"], Some("c1")),
        page_json(&["3"], Some("c2")),
        page_json(&["4"], None),
    ];
    let mut seen_cursors = Vec::new();
    let out = block_on(fetch_all(
        |c| {
            let i = match c.as_deref() {
                None => 0,
                Some("c1") => 1,
                _ => 2,
            };
            seen_cursors.push(c.clone());
            let text = pages[i].clone();
            async move { parse_page(&text) }
        },
        100,
        100,
    ))
    .unwrap();
    let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["1", "2", "3", "4"]);
    assert_eq!(
        seen_cursors,
        vec![None, Some("c1".to_string()), Some("c2".to_string())]
    );
}

#[test]
fn pager_errors_when_the_cursor_is_ignored() {
    // A server that ignores the cursor returns page 1 forever (polybots: 104,800 rows, 100 unique ids).
    let text = page_json(&["1", "2"], Some("c1"));
    let r = block_on(fetch_all(
        |_| {
            let t = text.clone();
            async move { parse_page(&t) }
        },
        100,
        100,
    ));
    assert!(matches!(r, Err(pm_api::ApiError::Stuck(2))));
}

#[test]
fn pager_errors_at_max_pages() {
    let mut n = 0u32;
    let r = block_on(fetch_all(
        |_| {
            n += 1;
            let t = page_json(&[&n.to_string()], Some("c"));
            async move { parse_page(&t) }
        },
        3,
        100,
    ));
    assert!(
        matches!(r, Err(pm_api::ApiError::Stuck(3))),
        "max_pages reached is an error, not a silent stop"
    );
}

#[test]
fn short_final_page_with_empty_cursor_is_ok() {
    // "" (not null) is how a real final page signals done; still fine when the page is short.
    let text = page_json(&["1"], Some(""));
    let out = block_on(fetch_all(
        |_| {
            let t = text.clone();
            async move { parse_page(&t) }
        },
        100,
        100,
    ))
    .unwrap();
    let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["1"]);
}

#[test]
fn full_page_with_no_cursor_is_an_error_not_a_silent_truncation() {
    // page_size 2, and the page brought exactly 2 events with no cursor: that shape is more
    // likely a renamed/dropped cursor field than a real final page (spec F7).
    let text = page_json(&["1", "2"], None);
    let r = block_on(fetch_all(
        |_| {
            let t = text.clone();
            async move { parse_page(&t) }
        },
        100,
        2,
    ));
    assert!(matches!(
        r,
        Err(pm_api::ApiError::Field {
            field: "next_cursor",
            ..
        })
    ));
}
