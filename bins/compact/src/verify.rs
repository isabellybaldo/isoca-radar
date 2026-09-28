//! `compact verify <YYYY-MM-DD>` (spec 5b §6): read one merged day and report every problem
//! (the binary exits non-zero on any). Whole-feed gaps (shard null) are checked against the rows;
//! shard gaps can't be, because rows carry no shard (plan ruling R6). Row order within a token is
//! the file order (never re-sorted): tob/depth/l2 per-token ts checks run in that order.
use crate::archive::{MANIFEST, Manifest, daily_path, day_dir};
use crate::pq::{Table, for_each_batch, num_rows, read_all};
use crate::rows::{DepthRow, GapRow, L2Row, TobRow, TokenRow};
use anyhow::Context;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tape::time::parse_day;

pub const MAX_LISTED: usize = 50;

#[derive(Default)]
struct Problems {
    list: Vec<String>,
    total: usize,
}

impl Problems {
    fn add(&mut self, s: String) {
        self.total += 1;
        if self.list.len() < MAX_LISTED {
            self.list.push(s);
        }
    }

    fn done(mut self) -> Vec<String> {
        if self.total > self.list.len() {
            let more = self.total - self.list.len();
            self.list.push(format!("… and {more} more"));
        }
        self.list
    }
}

type TopKey = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);

pub fn verify_day(archive: &Path, day: &str) -> anyhow::Result<Vec<String>> {
    let day_ms = parse_day(day).with_context(|| format!("{day}: expected YYYY-MM-DD"))?;
    let dir = day_dir(archive, day_ms);
    let mp = dir.join(MANIFEST);
    let m: Manifest = serde_json::from_slice(
        &std::fs::read(&mp).with_context(|| format!("read {}", mp.display()))?,
    )
    .with_context(|| format!("parse {}", mp.display()))?;
    let mut p = Problems::default();

    let sum =
        |f: fn(&crate::hour::RowCounts) -> u64| m.hours.iter().map(|h| f(&h.rows)).sum::<u64>();
    for (name, want, hours) in [
        (
            TokenRow::NAME,
            m.totals.tokens,
            m.hours.last().map_or(0, |h| h.rows.tokens),
        ),
        (TobRow::NAME, m.totals.tob, sum(|r| r.tob)),
        (DepthRow::NAME, m.totals.depth, sum(|r| r.depth)),
        (L2Row::NAME, m.totals.l2, sum(|r| r.l2)),
        (GapRow::NAME, m.totals.gaps, sum(|r| r.gaps)),
    ] {
        let got = num_rows(&daily_path(&dir, name))?;
        if got != want {
            p.add(format!("{name}: {got} rows, the manifest says {want}"));
        }
        if hours != want {
            p.add(format!(
                "{name}: the manifest's total {want} is not the sum of its hours {hours}"
            ));
        }
    }

    let tokens: Vec<TokenRow> = read_all(&daily_path(&dir, TokenRow::NAME))?;
    for (i, t) in tokens.iter().enumerate() {
        if usize::try_from(t.token_idx).ok() != Some(i) {
            p.add(format!("tokens: row {i} has token_idx {}", t.token_idx));
        }
    }
    let n_tokens = tokens.len();
    let known = |idx: u32| usize::try_from(idx).is_ok_and(|i| i < n_tokens);

    let mut global: Vec<(i64, i64)> = read_all::<GapRow>(&daily_path(&dir, GapRow::NAME))?
        .into_iter()
        .filter(|g| g.shard.is_none())
        .map(|g| (g.start_ms, g.end_ms))
        .collect();
    global.sort();
    let in_gap = |t: i64| global.iter().any(|(s, e)| *s <= t && t < *e);

    let mut last: HashMap<u32, (TopKey, i64)> = HashMap::new();
    for_each_batch(&daily_path(&dir, TobRow::NAME), |b| {
        for r in TobRow::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("tob: token_idx {} is not in tokens", r.token_idx));
            }
            let key = (r.bid, r.bid_sz, r.ask, r.ask_sz);
            if let Some((k, ts)) = last.get(&r.token_idx) {
                if *k == key {
                    p.add(format!(
                        "tob: token_idx {} repeats its top at recv_ms {}",
                        r.token_idx, r.recv_ms
                    ));
                }
                if r.ts_ms < *ts {
                    p.add(format!(
                        "tob: token_idx {} ts_ms goes back at recv_ms {}",
                        r.token_idx, r.recv_ms
                    ));
                }
            }
            if in_gap(r.recv_ms) {
                p.add(format!("tob: a row inside a gap at recv_ms {}", r.recv_ms));
            }
            last.insert(r.token_idx, (key, r.ts_ms));
        }
        Ok(())
    })?;

    let mut depth_ts: HashMap<u32, i64> = HashMap::new();
    for_each_batch(&daily_path(&dir, DepthRow::NAME), |b| {
        for r in DepthRow::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("depth: token_idx {} is not in tokens", r.token_idx));
            }
            if depth_ts.get(&r.token_idx).is_some_and(|t| r.ts_ms < *t) {
                p.add(format!(
                    "depth: token_idx {} ts_ms goes back at {}",
                    r.token_idx, r.ts_ms
                ));
            }
            if in_gap(r.ts_ms) {
                p.add(format!("depth: a row inside a gap at ts_ms {}", r.ts_ms));
            }
            depth_ts.insert(r.token_idx, r.ts_ms);
        }
        Ok(())
    })?;

    // Per (episode_id, token_idx): the first row with size > 0 must be preceded by (or be) a
    // snapshot row. A stream that restarts onto an empty book zeroes the levels shown earlier
    // (snapshot = false, new_size = 0) before its next real snapshot, so leading size-0 rows are
    // allowed (reduce.rs: `l2_restart`).
    let mut saw_snapshot: HashSet<(String, u32)> = HashSet::new();
    let mut l2_ts: HashMap<u32, i64> = HashMap::new();
    for_each_batch(&daily_path(&dir, L2Row::NAME), |b| {
        for r in L2Row::rows(b)? {
            if !known(r.token_idx) {
                p.add(format!("l2: token_idx {} is not in tokens", r.token_idx));
            }
            let key = (r.episode_id.clone(), r.token_idx);
            if r.snapshot {
                saw_snapshot.insert(key);
            } else if r.new_size > 0 && !saw_snapshot.contains(&key) {
                p.add(format!(
                    "l2: episode {} token_idx {} does not start with a snapshot",
                    r.episode_id, r.token_idx
                ));
            }
            if l2_ts.get(&r.token_idx).is_some_and(|t| r.ts_ms < *t) {
                p.add(format!(
                    "l2: token_idx {} ts_ms goes back at recv_ms {}",
                    r.token_idx, r.recv_ms
                ));
            }
            l2_ts.insert(r.token_idx, r.ts_ms);
            if in_gap(r.recv_ms) {
                p.add(format!("l2: a row inside a gap at recv_ms {}", r.recv_ms));
            }
        }
        Ok(())
    })?;
    Ok(p.done())
}
