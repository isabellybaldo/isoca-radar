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

/// The initial-dump array frame for a token: `event_type == "book"` and more than one event in
/// the frame. Its timestamp is snapshot time, not delivery time, so it is excluded from lag/rate
/// stats below.
fn is_dump(r: &Row) -> bool {
    r.kinds == "book" && r.events > 1
}

/// A `PONG` heartbeat reply or a blank frame: no events at all.
fn is_pong_or_empty(r: &Row) -> bool {
    r.events == 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// `None` for the final aggregate row over every row of the run ("all").
    pub shard_tokens: Option<usize>,
    pub conns: usize,
    pub secs: i64,
    /// Every frame in the bucket, dumps and pongs included.
    pub frames: usize,
    pub parse_errors: usize,
    pub dump_frames: usize,
    pub dump_bytes: i64,
    /// Computed over rows that are neither a dump nor a pong/empty frame.
    pub frames_per_s: i64,
    pub events_per_s: i64,
    pub bytes_per_day: i64,
    /// Lag percentiles/min/stalls are computed over rows with a server timestamp that are not
    /// dumps (a dump's timestamp is snapshot time, not delivery time).
    pub lag_p50: Option<i64>,
    pub lag_p95: Option<i64>,
    pub lag_p99: Option<i64>,
    pub lag_min: Option<i64>,
    pub lag_p1: Option<i64>,
    /// Frames whose lag exceeds this bucket's p5 lag + 500 ms (p5 absorbs the clock offset).
    pub stalls: usize,
    pub stalls_permille: i64,
}

fn compute(shard_tokens: Option<usize>, rs: &[&Row]) -> Stats {
    let conns: BTreeSet<usize> = rs.iter().copied().map(|r| r.conn).collect();
    let t0 = rs.iter().copied().map(|r| r.recv_us).min().unwrap_or(0);
    let t1 = rs.iter().copied().map(|r| r.recv_us).max().unwrap_or(0);
    let secs = ((t1 - t0) / 1_000_000).max(1);
    let frames = rs.len();
    let parse_errors = rs.iter().copied().filter(|r| !r.ok).count();

    let dump_frames = rs.iter().copied().filter(|r| is_dump(r)).count();
    let dump_bytes: i64 = rs
        .iter()
        .copied()
        .filter(|r| is_dump(r))
        .map(|r| i64::try_from(r.bytes).unwrap_or(0))
        .sum();

    let normal: Vec<&Row> = rs
        .iter()
        .copied()
        .filter(|r| !is_dump(r) && !is_pong_or_empty(r))
        .collect();
    let events_total: i64 = normal
        .iter()
        .map(|r| i64::try_from(r.events).unwrap_or(0))
        .sum();
    let bytes_total: i64 = normal
        .iter()
        .map(|r| i64::try_from(r.bytes).unwrap_or(0))
        .sum();
    let frames_per_s = i64::try_from(normal.len()).unwrap_or(0) / secs;
    let events_per_s = events_total / secs;
    let bytes_per_day = bytes_total * 86_400 / secs;

    let mut lags: Vec<i64> = rs
        .iter()
        .copied()
        .filter(|r| !is_dump(r))
        .filter_map(|r| r.server_ms.map(|s| r.recv_us / 1_000 - s))
        .collect();
    lags.sort_unstable();
    let p5 = percentile(&lags, 5);
    let stalls = p5.map_or(0, |p| lags.iter().filter(|&&l| l > p + 500).count());
    let stalls_permille = if lags.is_empty() {
        0
    } else {
        i64::try_from(stalls).unwrap_or(0) * 1000 / i64::try_from(lags.len()).unwrap_or(1)
    };

    Stats {
        shard_tokens,
        conns: conns.len(),
        secs,
        frames,
        parse_errors,
        dump_frames,
        dump_bytes,
        frames_per_s,
        events_per_s,
        bytes_per_day,
        lag_p50: percentile(&lags, 50),
        lag_p95: percentile(&lags, 95),
        lag_p99: percentile(&lags, 99),
        lag_min: lags.first().copied(),
        lag_p1: percentile(&lags, 1),
        stalls,
        stalls_permille,
    }
}

/// One `Stats` per `shard_tokens` bucket, plus a final row (`shard_tokens: None`, rendered as
/// "all") computed over every row of the run.
pub fn bucket_stats(rows: &[Row]) -> Vec<Stats> {
    let mut by: BTreeMap<usize, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by.entry(r.shard_tokens).or_default().push(r);
    }
    let mut out: Vec<Stats> = by
        .into_iter()
        .map(|(shard_tokens, rs)| compute(Some(shard_tokens), &rs))
        .collect();
    let all: Vec<&Row> = rows.iter().collect();
    out.push(compute(None, &all));
    out
}

fn o(v: Option<i64>) -> String {
    v.map_or("-".to_string(), |x| x.to_string())
}

pub fn render(stats: &[Stats]) -> String {
    let mut s = String::from(
        "| shard tokens | conns | secs | frames | dump frames | dump bytes | parse errors | frames/s | events/s | bytes/day | lag p50 ms | p95 | p99 | p1 | min | stalls (>p5+500ms) | stalls ‰ |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for x in stats {
        let label = x.shard_tokens.map_or("all".to_string(), |v| v.to_string());
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            label,
            x.conns,
            x.secs,
            x.frames,
            x.dump_frames,
            x.dump_bytes,
            x.parse_errors,
            x.frames_per_s,
            x.events_per_s,
            x.bytes_per_day,
            o(x.lag_p50),
            o(x.lag_p95),
            o(x.lag_p99),
            o(x.lag_p1),
            o(x.lag_min),
            x.stalls,
            x.stalls_permille
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
            kinds: "price_change".into(),
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
        // 2 buckets (50, 500) + the final "all" row
        assert_eq!(s.len(), 3);
        let a = &s[0];
        assert_eq!(
            (a.shard_tokens, a.conns, a.frames, a.parse_errors),
            (Some(50), 2, 3, 0)
        );
        assert_eq!(
            a.lag_min,
            Some(-5),
            "negative lag is the clock-offset signal; never clamp it"
        );
        assert_eq!(a.lag_p50, Some(20));
        assert_eq!(a.stalls, 1, "900 ms is > p5 (−5) + 500");
        assert_eq!(a.stalls_permille, 1000 / 3);
        assert_eq!(a.frames_per_s, 0, "3 frames / 10 s rounds down");
        assert_eq!(a.events_per_s, 0);
        assert_eq!(a.bytes_per_day, 300 * 86_400 / 10);
        assert_eq!(
            (s[1].shard_tokens, s[1].parse_errors, s[1].lag_p50),
            (Some(500), 1, None)
        );
        let all = s.last().unwrap();
        assert_eq!(all.shard_tokens, None);
        assert_eq!((all.conns, all.frames), (3, 4));
        assert!(render(&s).contains("| 50 |"));
        assert!(render(&s).contains("| all |"));
    }

    #[test]
    fn dump_and_pong_frames_are_excluded_from_rates_and_lag() {
        // A dump (3 events, huge lag from snapshot time), a pong (0 events, no lag), and two
        // normal frames over 10 s.
        let dump = Row {
            recv_us: 1_000_000,
            conn: 0,
            shard_tokens: 50,
            bytes: 9_000,
            ok: true,
            events: 3,
            server_ms: Some(1), // would be a huge, meaningless lag if counted
            kinds: "book".into(),
        };
        let pong = Row {
            recv_us: 2_000_000,
            conn: 0,
            shard_tokens: 50,
            bytes: 4,
            ok: true,
            events: 0,
            server_ms: None,
            kinds: String::new(),
        };
        let normal1 = row(3_000, 0, 50, Some(2_950), true);
        let normal2 = row(11_000, 0, 50, Some(10_960), true);
        let rows = vec![dump, pong, normal1, normal2];
        let s = bucket_stats(&rows);
        assert_eq!(s.len(), 2); // shard 50 bucket + "all"

        let bucket = &s[0];
        assert_eq!(bucket.frames, 4, "frames still counts every row");
        assert_eq!(bucket.dump_frames, 1);
        assert_eq!(bucket.dump_bytes, 9_000);
        // rates: only the 2 normal frames, over 10 s
        assert_eq!(bucket.frames_per_s, 0);
        assert_eq!(bucket.events_per_s, 0); // 2 events / 10 s rounds down
        assert_eq!(bucket.bytes_per_day, 200 * 86_400 / 10);
        // lag: dump excluded (huge/meaningless), pong excluded (no server_ms); only the 2 normals
        // (recv − server = 50 ms and 40 ms)
        assert_eq!(bucket.lag_min, Some(40));
        assert_eq!(bucket.lag_p50, Some(40));

        let all = s.last().unwrap();
        assert_eq!(all.frames, 4, "'all' totals every row of the run");
        assert_eq!(all.dump_frames, 1);
        assert_eq!(all.dump_bytes, 9_000);
    }
}
