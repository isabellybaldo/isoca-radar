#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::money::{Price, Shares};
use pm_api::clob::*;

// Shape of POST /books (docs, 2026-09-22): best = LAST element of each side.
const BOOKS: &str = r#"[
  {"market": "0x747d", "asset_id": "111", "timestamp": "1782753357257", "hash": "a1",
   "bids": [{"price": "0.52", "size": "42239.53"}, {"price": "0.54", "size": "5603.74"}],
   "asks": [{"price": "0.56", "size": "252281.27"}, {"price": "0.55", "size": "32356.03"}],
   "min_order_size": "5", "tick_size": "0.01", "neg_risk": true},
  {"asset_id": "222", "timestamp": 1782753357257, "bids": [], "asks": []}
]"#;

fn lv(p: &str, s: &str) -> (Price, Shares) {
    (Price::parse(p).unwrap(), Shares::parse(s).unwrap())
}

#[test]
fn books_parse_to_arb_core_books() {
    let books: Vec<BookWire> = serde_json::from_str(BOOKS).unwrap();
    let b = books[0].to_book().unwrap();
    assert_eq!(b.top().best_bid, Some(lv("0.54", "5603.74")));
    assert_eq!(b.top().best_ask, Some(lv("0.55", "32356.03")));
    assert_eq!(b.server_ts_ms(), 1_782_753_357_257);
    // numeric timestamp and empty sides are fine
    let empty = books[1].to_book().unwrap();
    assert_eq!(empty.top().best_bid, None);
    assert_eq!(books[1].server_ts_ms().unwrap(), 1_782_753_357_257);
}

#[test]
fn bad_level_is_an_error_not_a_panic() {
    let bad: Vec<BookWire> = serde_json::from_str(
        r#"[{"asset_id":"x","timestamp":"1","bids":[{"price":"1.5","size":"1"}],"asks":[]}]"#,
    )
    .unwrap();
    assert!(bad[0].to_book().is_err());
    let no_ts: Vec<BookWire> =
        serde_json::from_str(r#"[{"asset_id":"x","bids":[],"asks":[]}]"#).unwrap();
    assert!(no_ts[0].to_book().is_err());
}

#[test]
fn requests_are_chunked_at_500_tokens() {
    let tokens: Vec<String> = (0..1201).map(|i| i.to_string()).collect();
    let chunks = chunk_tokens(&tokens);
    assert_eq!(
        chunks.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![500, 500, 201]
    );
    assert!(chunk_tokens(&[]).is_empty());
}
