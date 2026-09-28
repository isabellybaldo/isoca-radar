#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::money::Price;
use tape::feed::server_top;

#[test]
fn the_empty_side_markers_are_not_levels() {
    let p = |s: &str| Price::parse(s).unwrap();
    assert_eq!(server_top(Some(p("0")), Some(p("1"))), (None, None));
    assert_eq!(
        server_top(Some(p("0.4")), Some(p("0.6"))),
        (Some(p("0.4")), Some(p("0.6")))
    );
    assert_eq!(server_top(None, None), (None, None));
}
