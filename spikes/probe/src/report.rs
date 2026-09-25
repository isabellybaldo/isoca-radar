//! Integer statistics over frames.csv (no floats): rates, bytes/day, lag percentiles, stalls.
use crate::capture::Row;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn percentile(sorted: &[i64], pct: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let idx = (sorted.len() - 1) * pct.min(100) / 100;
    sorted.get(idx).copied()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub shard_tokens: usize,
    pub conns: usize,
    pub frames: usize,
    pub parse_errors: usize,
    pub frames_per_s: i64,
    pub bytes_per_day: i64,
    pub lag_p50: Option<i64>,
    pub lag_p95: Option<i64>,
    pub lag_p99: Option<i64>,
    pub lag_min: Option<i64>,
    /// Frames whose lag exceeds this bucket's p5 lag + 500 ms (p5 absorbs the clock offset).
    pub stalls: usize,
}

pub fn bucket_stats(rows: &[Row]) -> Vec<Stats> {
    let mut by: BTreeMap<usize, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by.entry(r.shard_tokens).or_default().push(r);
    }
    by.into_iter()
        .map(|(shard_tokens, rs)| {
            let conns: BTreeSet<usize> = rs.iter().map(|r| r.conn).collect();
            let t0 = rs.iter().map(|r| r.recv_us).min().unwrap_or(0);
            let t1 = rs.iter().map(|r| r.recv_us).max().unwrap_or(0);
            let secs = ((t1 - t0) / 1_000_000).max(1);
            let bytes: i64 = rs.iter().map(|r| i64::try_from(r.bytes).unwrap_or(0)).sum();
            let frames = rs.len();
            let mut lags: Vec<i64> = rs
                .iter()
                .filter_map(|r| r.server_ms.map(|s| r.recv_us / 1_000 - s))
                .collect();
            lags.sort_unstable();
            let p5 = percentile(&lags, 5);
            let stalls = p5.map_or(0, |p| lags.iter().filter(|&&l| l > p + 500).count());
            Stats {
                shard_tokens,
                conns: conns.len(),
                frames,
                parse_errors: rs.iter().filter(|r| !r.ok).count(),
                frames_per_s: i64::try_from(frames).unwrap_or(0) / secs,
                bytes_per_day: bytes * 86_400 / secs,
                lag_p50: percentile(&lags, 50),
                lag_p95: percentile(&lags, 95),
                lag_p99: percentile(&lags, 99),
                lag_min: lags.first().copied(),
                stalls,
            }
        })
        .collect()
}

fn o(v: Option<i64>) -> String {
    v.map_or("-".to_string(), |x| x.to_string())
}

pub fn render(stats: &[Stats]) -> String {
    let mut s = String::from(
        "| shard tokens | conns | frames | parse errors | frames/s | bytes/day | lag p50 ms | p95 | p99 | min | stalls (>p5+500ms) |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for x in stats {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            x.shard_tokens,
            x.conns,
            x.frames,
            x.parse_errors,
            x.frames_per_s,
            x.bytes_per_day,
            o(x.lag_p50),
            o(x.lag_p95),
            o(x.lag_p99),
            o(x.lag_min),
            x.stalls
        ));
    }
    s
}

pub fn report(dir: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(dir.join("frames.csv"))?;
    let rows: Vec<Row> = text
        .lines()
        .filter(|l| *l != Row::HEADER)
        .filter_map(Row::from_csv)
        .collect();
    let md = render(&bucket_stats(&rows));
    std::fs::write(dir.join("report.md"), &md)?;
    println!("{md}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(recv_ms: i64, conn: usize, shard: usize, server_ms: Option<i64>, ok: bool) -> Row {
        Row {
            recv_us: recv_ms * 1_000,
            conn,
            shard_tokens: shard,
            bytes: 100,
            ok,
            events: 1,
            server_ms,
            kinds: "book".into(),
        }
    }

    #[test]
    fn percentile_by_sorted_index() {
        let v = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50), Some(5));
        assert_eq!(percentile(&v, 100), Some(10));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn stats_per_shard_size_with_negative_lag_kept() {
        // shard 50: 3 frames over 10 s; lags (recv − server) = −5, 20, 900 ms
        let rows = vec![
            row(1_000, 0, 50, Some(1_005), true),
            row(6_000, 0, 50, Some(5_980), true),
            row(11_000, 1, 50, Some(10_100), true),
            row(11_000, 2, 500, None, false),
        ];
        let s = bucket_stats(&rows);
        assert_eq!(s.len(), 2);
        let a = &s[0];
        assert_eq!(
            (a.shard_tokens, a.conns, a.frames, a.parse_errors),
            (50, 2, 3, 0)
        );
        assert_eq!(
            a.lag_min,
            Some(-5),
            "negative lag is the clock-offset signal; never clamp it"
        );
        assert_eq!(a.lag_p50, Some(20));
        assert_eq!(a.stalls, 1, "900 ms is > p5 (−5) + 500");
        assert_eq!(a.frames_per_s, 0, "3 frames / 10 s rounds down");
        assert_eq!(a.bytes_per_day, 300 * 86_400 / 10);
        assert_eq!(
            (s[1].shard_tokens, s[1].parse_errors, s[1].lag_p50),
            (500, 1, None)
        );
        assert!(render(&s).contains("| 50 |"));
    }
}
