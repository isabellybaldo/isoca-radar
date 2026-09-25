#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Side;
use arb_core::money::{Price, Shares};
use pm_api::ws::*;

fn p(s: &str) -> Price {
    Price::parse(s).unwrap()
}
fn sh(s: &str) -> Shares {
    Shares::parse(s).unwrap()
}

#[test]
fn initial_dump_array_of_books() {
    let f = r#"[{"event_type":"book","asset_id":"111","market":"0xbd","timestamp":"1790121492396","hash":"h",
                 "bids":[{"price":"0.48","size":"30"}],"asks":[{"price":"0.52","size":"25"}]},
                {"event_type":"book","asset_id":"222","market":"0xbd","timestamp":"1790121492396","hash":"h2",
                 "bids":[],"asks":[]}]"#;
    let ev = parse_frame(f).unwrap();
    assert_eq!(ev.len(), 2);
    assert_eq!(
        ev[0],
        MarketEvent::Snapshot {
            token: "111".into(),
            server_ts_ms: 1_790_121_492_396,
            bids: vec![(p("0.48"), sh("30"))],
            asks: vec![(p("0.52"), sh("25"))],
            hash: Some("h".into()),
        }
    );
}

#[test]
fn price_change_touches_several_assets_and_empty_best_is_none() {
    let f = r#"{"event_type":"price_change","market":"0x5f","timestamp":"1790121493000",
                "price_changes":[
                  {"asset_id":"111","price":"0.5","size":"200","side":"BUY","hash":"x","best_bid":"0.5","best_ask":"0.52"},
                  {"asset_id":"222","price":"0.5","size":"0","side":"SELL","hash":"y","best_bid":"","best_ask":"0.51"}
                ]}"#;
    let ev = parse_frame(f).unwrap();
    assert_eq!(
        ev,
        vec![
            MarketEvent::Delta {
                token: "111".into(),
                server_ts_ms: 1_790_121_493_000,
                side: Side::Bid,
                price: p("0.5"),
                size: sh("200"),
                best_bid: Some(p("0.5")),
                best_ask: Some(p("0.52")),
            },
            MarketEvent::Delta {
                token: "222".into(),
                server_ts_ms: 1_790_121_493_000,
                side: Side::Ask,
                price: p("0.5"),
                size: sh("0"),
                best_bid: None,
                best_ask: Some(p("0.51")),
            },
        ]
    );
}

#[test]
fn pong_and_unknown_events() {
    assert!(parse_frame("PONG").unwrap().is_empty());
    assert!(parse_frame("  ").unwrap().is_empty());
    let ev = parse_frame(r#"{"event_type":"something_new","x":1}"#).unwrap();
    assert_eq!(
        ev,
        vec![MarketEvent::Other {
            event_type: "something_new".into()
        }]
    );
    let ev = parse_frame(r#"{"event_type":"last_trade_price","asset_id":"111","timestamp":1790121493001,"price":"0.5"}"#).unwrap();
    assert_eq!(
        ev,
        vec![MarketEvent::LastTrade {
            token: "111".into(),
            server_ts_ms: 1_790_121_493_001
        }]
    );
}

#[test]
fn malformed_frames_are_errors_not_panics() {
    assert!(parse_frame("{not json").is_err());
    assert!(parse_frame(r#"{"event_type":"book","asset_id":"1","timestamp":"1","bids":[{"price":"2","size":"1"}],"asks":[]}"#).is_err());
    assert!(parse_frame(r#"{"event_type":"price_change","timestamp":"1","price_changes":[{"asset_id":"1","price":"0.5","size":"1","side":"HOLD"}]}"#).is_err());
    assert!(parse_frame(r#"{"event_type":"book","asset_id":"1","bids":[],"asks":[]}"#).is_err()); // no timestamp
}

#[test]
fn resolved_and_new_market_timestamps_never_default_to_zero() {
    // No timestamp at all.
    let ev = parse_frame(r#"{"event_type":"market_resolved","market":"0xabc"}"#).unwrap();
    assert_eq!(
        ev,
        vec![MarketEvent::Resolved {
            market: Some("0xabc".into()),
            server_ts_ms: None,
        }]
    );
    // Invalid (non-numeric) timestamp.
    let ev = parse_frame(r#"{"event_type":"market_resolved","timestamp":"not-a-number"}"#).unwrap();
    assert_eq!(
        ev,
        vec![MarketEvent::Resolved {
            market: None,
            server_ts_ms: None,
        }]
    );
    let ev = parse_frame(r#"{"event_type":"new_market"}"#).unwrap();
    assert_eq!(ev, vec![MarketEvent::NewMarket { server_ts_ms: None }]);
    let ev = parse_frame(r#"{"event_type":"new_market","timestamp":"1790121493001"}"#).unwrap();
    assert_eq!(
        ev,
        vec![MarketEvent::NewMarket {
            server_ts_ms: Some(1_790_121_493_001)
        }]
    );
}

#[test]
fn bad_item_in_array_names_its_asset_id() {
    let f = r#"[{"event_type":"book","asset_id":"111","timestamp":"1","bids":[],"asks":[]},
                {"event_type":"book","asset_id":"bad-one","timestamp":"1",
                 "bids":[{"price":"2","size":"1"}],"asks":[]}]"#;
    let err = parse_frame(f).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("asset_id="), "{msg}");
    assert!(msg.contains("bad-one"), "{msg}");
}

#[test]
fn subscribe_message_shape() {
    let v: serde_json::Value =
        serde_json::from_str(&subscribe_message(&["1".into(), "2".into()])).unwrap();
    assert_eq!(v["type"], "market");
    assert_eq!(v["assets_ids"], serde_json::json!(["1", "2"]));
    assert_eq!(v["initial_dump"], true);
}
