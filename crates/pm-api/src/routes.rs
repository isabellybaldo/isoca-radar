//! Every URL pm-api may reach. Adding one here is the only way to add network access (spec §16).
pub const GAMMA_BASE: &str = "https://gamma-api.polymarket.com";
pub const CLOB_BASE: &str = "https://clob.polymarket.com";
pub const WS_MARKET: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
pub const WS_SPORTS: &str = "wss://sports-api.polymarket.com/ws";

pub const GAMMA_EVENTS_KEYSET: &str = "/events/keyset";
pub const GAMMA_MARKETS: &str = "/markets";
pub const CLOB_BOOK: &str = "/book";
pub const CLOB_BOOKS: &str = "/books";
pub const CLOB_OK: &str = "/ok";
pub const CLOB_TIME: &str = "/time";

/// Unit tests only (`http.rs`'s round-trip timing tests): the scheme of the throwaway plain-http
/// server they start on 127.0.0.1. Never a route and never compiled into the crate; it lives here
/// because this is the one file allowed URL literals (tests/routes.rs).
#[cfg(test)]
pub(crate) const LOCAL_TEST_SCHEME: &str = "http://";

/// (method, full URL). Asserted exactly by tests/routes.rs.
pub const ALLOWED: &[(&str, &str)] = &[
    ("GET", "https://gamma-api.polymarket.com/events/keyset"),
    ("GET", "https://gamma-api.polymarket.com/markets"),
    ("GET", "https://clob.polymarket.com/book"),
    ("POST", "https://clob.polymarket.com/books"),
    ("GET", "https://clob.polymarket.com/ok"),
    ("GET", "https://clob.polymarket.com/time"),
    ("WS", "wss://ws-subscriptions-clob.polymarket.com/ws/market"),
    ("WS", "wss://sports-api.polymarket.com/ws"),
];
