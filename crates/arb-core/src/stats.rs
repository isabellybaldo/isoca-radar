//! Duration statistics (spec §5.5). Left- and gap-censored episodes are excluded by the caller.
use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;

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

const MS_PER_DAY: i64 = 86_400_000;

/// Unix ms from an RFC 3339 timestamp as Gamma sends `endDate` ("2026-10-01T12:00:00Z", optional
/// fractional seconds, optional ±hh:mm offset), or a bare date (= midnight UTC). None otherwise.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let date = s.get(..10)?;
    let rest = s.get(10..)?;
    let mut it = date.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, m, d) * MS_PER_DAY;
    if rest.is_empty() {
        return Some(ms);
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    let hms = rest.get(..8)?;
    let mut tz = rest.get(8..)?;
    let mut t = hms.split(':');
    let hh: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let ss: i64 = t.next()?.parse().ok()?;
    if t.next().is_some() || hh > 23 || mi > 59 || ss > 60 {
        return None;
    }
    ms += (hh * 3600 + mi * 60 + ss) * 1000;
    if let Some(frac) = tz.strip_prefix('.') {
        let n = frac
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(frac.len());
        if n == 0 {
            return None;
        }
        let first3: String = frac
            .get(..n)?
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        ms += first3.parse::<i64>().ok()?;
        tz = frac.get(n..)?;
    }
    match tz {
        "" | "Z" | "z" => Some(ms),
        _ => {
            let sign = match tz.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let off = tz.get(1..)?;
            let (oh, om) = match off.split_once(':') {
                Some(x) => x,
                None => (off.get(..2)?, off.get(2..)?),
            };
            let oh: i64 = oh.parse().ok()?;
            let om: i64 = om.parse().ok()?;
            if oh > 23 || om > 59 {
                return None;
            }
            Some(ms - sign * (oh * 60 + om) * 60_000)
        }
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days from `t_ms` until `end_ms`: an estimate (events can resolve before or after `endDate`),
/// negative once past it.
pub fn days_to_end(end_ms: i64, t_ms: i64) -> Decimal {
    Decimal::from(end_ms - t_ms) / Decimal::from(MS_PER_DAY)
}

/// Secondary lock-up figure (spec §5.5): (1 + r)^(365 / max(days, 7)) − 1. Days are floored at 7
/// so tiny horizons don't explode. None if 1 + r ≤ 0 or the power overflows.
pub fn annualized(simple_return: Decimal, days: Decimal) -> Option<Decimal> {
    let base = Decimal::ONE + simple_return;
    if base <= Decimal::ZERO {
        return None;
    }
    let exp = Decimal::from(365) / days.max(Decimal::from(7));
    base.checked_powd(exp).map(|x| x - Decimal::ONE)
}
