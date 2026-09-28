#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::book::Side;
use tape::frame::{FrameError, MarketEvent, parse_frame};

#[test]
fn a_book_and_a_price_change_parse_like_the_scanner_always_did() {
    let evs = parse_frame(r#"[{"event_type":"book","asset_id":"1","timestamp":"5","hash":"h","bids":[{"price":"0.4","size":"10"}],"asks":[]}]"#).unwrap();
    assert!(
        matches!(&evs[0], MarketEvent::Snapshot { token, server_ts_ms: 5, bids, .. } if token == "1" && bids.len() == 1)
    );
    let evs = parse_frame(r#"{"event_type":"price_change","timestamp":"6","price_changes":[{"asset_id":"1","side":"SELL","price":"0.5","size":"0","best_bid":"0.4","best_ask":"1"}]}"#).unwrap();
    assert!(matches!(
        &evs[0],
        MarketEvent::Delta {
            side: Side::Ask,
            server_ts_ms: 6,
            ..
        }
    ));
    assert!(parse_frame("PONG").unwrap().is_empty());
}

#[test]
fn a_bad_item_in_an_array_names_its_asset_id() {
    let e = parse_frame(r#"[{"event_type":"book","asset_id":"bad-one","timestamp":"1","bids":[{"price":"2","size":"1"}],"asks":[]}]"#).unwrap_err();
    assert!(matches!(e, FrameError::Field { .. }));
    assert!(e.to_string().contains("asset_id=bad-one"), "{e}");
    assert!(matches!(parse_frame("{").unwrap_err(), FrameError::Json(_)));
}
