//! JSON shapes shared by every endpoint. Money and prices are decimal STRINGS (spec §4): a JSON
//! number would be read as a float by the browser.
use rust_decimal::Decimal;
use serde::{Serialize, Serializer};

/// A decimal that serializes as `"0.41"`. Trailing zeros are dropped (`0.410000` → `"0.41"`),
/// `-0` becomes `"0"`, and a measured zero stays `"0"`. `Option<Dec>::None` is JSON `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec(pub Decimal);

impl Serialize for Dec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.normalize().to_string())
    }
}

pub fn opt(d: Option<Decimal>) -> Option<Dec> {
    d.map(Dec)
}

/// One keyset page; `next_cursor` is None on the last page.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn decimals_are_strings_and_zero_is_not_null() {
        let j = |v: Option<Decimal>| serde_json::to_string(&opt(v)).unwrap_or_default();
        assert_eq!(j(Some(dec!(0.410000))), "\"0.41\"");
        assert_eq!(j(Some(dec!(0))), "\"0\"");
        assert_eq!(j(Some(dec!(-0.000))), "\"0\"");
        assert_eq!(j(Some(dec!(-99.84))), "\"-99.84\"");
        assert_eq!(j(Some(dec!(1000))), "\"1000\"");
        assert_eq!(
            j(Some(dec!(12345678901234567.89))),
            "\"12345678901234567.89\""
        );
        assert_eq!(j(None), "null");
    }
}
