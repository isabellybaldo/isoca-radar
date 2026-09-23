//! Duration statistics (spec §5.5). Left- and gap-censored episodes are excluded by the caller.
use rust_decimal::Decimal;

/// Kaplan–Meier with right-censoring. `obs` = (duration_ms, observed).
pub fn kaplan_meier(obs: &[(i64, bool)]) -> Vec<(i64, Decimal)> {
    let mut sorted = obs.to_vec();
    sorted.sort_by_key(|&(d, _)| d);
    let mut at_risk = sorted.len() as u64;
    let mut surv = Decimal::ONE;
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let t = sorted[i].0;
        let mut events = 0u64;
        let mut leaving = 0u64;
        while i < sorted.len() && sorted[i].0 == t {
            if sorted[i].1 {
                events += 1;
            }
            leaving += 1;
            i += 1;
        }
        if events > 0 && at_risk > 0 {
            surv *= Decimal::from(at_risk - events) / Decimal::from(at_risk);
            out.push((t, surv));
        }
        at_risk -= leaving;
    }
    out
}

/// Gebele's empirical survival over uncensored durations: share with D > tau.
pub fn empirical_survival(uncensored: &[i64], tau: i64) -> Decimal {
    if uncensored.is_empty() {
        return Decimal::ZERO;
    }
    let above = uncensored.iter().filter(|&&d| d > tau).count() as u64;
    Decimal::from(above) / Decimal::from(uncensored.len() as u64)
}
