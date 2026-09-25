#![allow(clippy::unwrap_used, clippy::expect_used)]
use arb_core::stats::{annualized, days_to_end, parse_rfc3339_ms};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn parses_gamma_timestamps() {
    assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        parse_rfc3339_ms("2026-09-25T12:45:48Z"),
        Some(1_790_340_348_000)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-09-25T12:45:48.680Z"),
        Some(1_790_340_348_680)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-09-25T12:45:48.68Z"),
        Some(1_790_340_348_680)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-09-25T13:45:48+01:00"),
        Some(1_790_340_348_000)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-09-25T10:45:48-02:00"),
        Some(1_790_340_348_000)
    );
    assert_eq!(parse_rfc3339_ms("2026-09-25"), Some(1_790_294_400_000));
    assert_eq!(
        parse_rfc3339_ms("2024-02-29T00:00:00Z"),
        Some(1_709_164_800_000)
    );
}

#[test]
fn rejects_garbage() {
    for s in [
        "",
        "2026",
        "2026-13-01",
        "2026-09-25T25:00:00Z",
        "2026-09-25T12:45Z",
        "yesterday",
        "2026-09-25T12:45:48X",
    ] {
        assert_eq!(parse_rfc3339_ms(s), None, "{s}");
    }
}

#[test]
fn days_are_exact_and_may_be_negative() {
    assert_eq!(days_to_end(86_400_000 * 3, 0), dec!(3));
    assert_eq!(days_to_end(43_200_000, 0), dec!(0.5));
    assert_eq!(days_to_end(0, 86_400_000), dec!(-1));
}

#[test]
fn annualized_floors_days_at_seven() {
    // spec §5.5: 0.5% at 3 days would be 83% without the floor; floored at 7 days it is ~29.7%
    let a = annualized(dec!(0.005), dec!(3)).unwrap();
    assert!((a - dec!(0.2970)).abs() < dec!(0.0005), "{a}");
    let one_year = annualized(dec!(0.1), dec!(365)).unwrap();
    assert!((one_year - dec!(0.1)).abs() < dec!(0.000001), "{one_year}");
    assert_eq!(annualized(Decimal::ZERO, dec!(10)), Some(Decimal::ZERO));
    assert_eq!(annualized(dec!(-1), dec!(10)), None);
}
