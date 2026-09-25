#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal_macros::dec;
use sim::config::{PolicyKind, SimConfig};

const DEFAULT: &str = include_str!("../../../config/policies.toml");

fn parse(s: &str) -> SimConfig {
    toml::from_str(s).unwrap()
}

#[test]
fn default_file_parses_validates_and_is_exact() {
    let c = parse(DEFAULT);
    c.validate().unwrap();
    assert_eq!(c.min_order_shares, dec!(5));
    assert_eq!(c.p1.min_annualized, dec!(0.10));
    assert_eq!(c.accounts.len(), 15);
    let ids: Vec<String> = c.accounts.iter().map(|a| a.id()).collect();
    assert!(ids.contains(&"P0-converter/sp-idle".to_string()));
    assert!(ids.contains(&"P0-converter/sp-idle/fee10".to_string()));
    assert!(ids.contains(&"P3-naive/colo-idle".to_string()));
    assert!(
        c.accounts
            .iter()
            .all(|a| a.bankroll == dec!(1000) && a.per_trade == dec!(100))
    );
    assert_eq!(c.scenario("sp-burst").unwrap().burst.len(), 2);
    assert_eq!(PolicyKind::P2Combo.as_str(), "P2-combo");
}

#[test]
fn invalid_configs_are_rejected() {
    let unknown_scenario = DEFAULT.replacen("scenario = \"sp-idle\"", "scenario = \"nope\"", 1);
    assert!(parse(&unknown_scenario).validate().is_err());

    let dup = format!(
        "{DEFAULT}\n[[accounts]]\npolicy = \"P1-settlement\"\nscenario = \"sp-idle\"\nbankroll = \"1000\"\nper_trade = \"100\"\n"
    );
    assert!(parse(&dup).validate().is_err(), "duplicate account id");

    let zero_bankroll = DEFAULT.replacen("bankroll = \"1000\"", "bankroll = \"0\"", 1);
    assert!(parse(&zero_bankroll).validate().is_err());

    let bad_bips = DEFAULT.replacen("fee_bips = \"10\"", "fee_bips = \"10001\"", 1);
    assert!(parse(&bad_bips).validate().is_err());

    let unsorted = DEFAULT.replacen(
        "markout_offsets_ms = [1000, 5000, 30000]",
        "markout_offsets_ms = [5000, 1000]",
        1,
    );
    assert!(parse(&unsorted).validate().is_err());
}

#[test]
fn typos_are_parse_errors() {
    let typo = DEFAULT.replacen("settle_delay_ms", "settle_dealy_ms", 1);
    assert!(toml::from_str::<SimConfig>(&typo).is_err());
}
