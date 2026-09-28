//! UTC calendar helpers, integers only (Howard Hinnant's civil algorithms).
pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;

pub fn hour_of(ms: i64) -> i64 {
    ms.div_euclid(HOUR_MS) * HOUR_MS
}

pub fn day_of(ms: i64) -> i64 {
    ms.div_euclid(DAY_MS) * DAY_MS
}

/// UTC (year, month, day, hour) of a millisecond timestamp.
pub fn civil(ms: i64) -> (i64, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let hour = u32::try_from(secs.rem_euclid(86_400) / 3600).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, hour)
}

/// Days since 1970-01-01 of a civil date (inverse of `civil`).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` of the UTC day containing `ms`.
pub fn day_name(ms: i64) -> String {
    let (y, m, d, _) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DDTHH` of the UTC hour containing `ms` (log lines, spec 5b §5).
pub fn hour_label(ms: i64) -> String {
    let (y, m, d, h) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}T{h:02}")
}

/// 00:00Z of a strict `YYYY-MM-DD` (a real date), else None.
pub fn parse_day(name: &str) -> Option<i64> {
    let b = name.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = name.get(0..4)?.parse().ok()?;
    let m: i64 = name.get(5..7)?.parse().ok()?;
    let d: i64 = name.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let ms = days_from_civil(y, m, d) * DAY_MS;
    (day_name(ms) == name).then_some(ms)
}
