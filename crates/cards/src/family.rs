//! Premise keys per cell family and the state analyze keeps between ticks (spec §4, §7).
use crate::cells::CellData;
use crate::window::Windows;
use crate::{HOUR_MS, hour_of};
use arb_core::premises::premise_key;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpan {
    pub run_id: i64,
    pub started_ms: i64,
    /// None: still running.
    pub end_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPremises {
    pub span: RunSpan,
    pub versions: BTreeMap<String, String>,
}

/// The family's current premise key (the key of the newest run that has every area) and the runs
/// with exactly that key, oldest first. Runs missing an area never count. None: no run qualifies.
pub fn current_key(areas: &[&str], runs: &[RunPremises]) -> Option<(String, Vec<RunSpan>)> {
    let mut newest_first: Vec<&RunPremises> = runs.iter().collect();
    newest_first.sort_by_key(|r| std::cmp::Reverse(r.span.run_id));
    let key = newest_first
        .iter()
        .find_map(|r| premise_key(areas, &r.versions))?;
    let mut matched: Vec<RunSpan> = runs
        .iter()
        .filter(|r| premise_key(areas, &r.versions).as_deref() == Some(key.as_str()))
        .map(|r| r.span.clone())
        .collect();
    matched.sort_by_key(|r| r.run_id);
    Some((key, matched))
}

#[derive(Debug, Clone)]
pub struct Family<C: CellData> {
    pub key: Option<String>,
    pub runs: BTreeMap<i64, RunSpan>,
    pub catching_up: bool,
    pub windows: Windows<C>,
    /// When the long windows' bodies were last rendered (ruling T9 (c)).
    pub long: LongRender,
}

/// The long windows, whose bodies cost the most to render: `30d` (SPANS index 2) and `all`.
pub const LONG_WINDOWS: [&str; 2] = ["30d", "all"];

/// Ruling T9 (c): cells update every tick, but the `30d` and `all` bodies re-render at most once
/// per clock hour, and at most one of the two per tick, so no tick pays for both (Review Focus 5:
/// one tick < 200 ms). Each card keeps its own `computed_ms`, so the page's age stays honest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LongRender {
    /// The hour each long window was last rendered in (None: not since this state began).
    pub rendered: [Option<i64>; 2],
}

impl LongRender {
    /// Which long windows to render at `now_hour`:
    /// - both, when either has not been rendered since this state began (a start, a new key, a
    ///   reset);
    /// - otherwise the one rendered longest ago (`30d` on a tie), if not yet this clock hour;
    /// - and any that is two or more hours behind (only happens with fewer than two ticks an
    ///   hour, so staleness stays bounded).
    pub fn due(&self, now_hour: i64) -> [bool; 2] {
        let [Some(a), Some(b)] = self.rendered else {
            return [true, true];
        };
        let mut out = [false; 2];
        let (first, h) = if b < a { (1, b) } else { (0, a) };
        if h < now_hour {
            out[first] = true;
        }
        for (i, r) in [a, b].into_iter().enumerate() {
            if r <= now_hour - 2 * HOUR_MS {
                out[i] = true;
            }
        }
        out
    }

    pub fn mark(&mut self, rendered: [bool; 2], now_hour: i64) {
        for (slot, done) in self.rendered.iter_mut().zip(rendered) {
            if done {
                *slot = Some(now_hour);
            }
        }
    }
}

impl<C: CellData> Family<C> {
    pub fn new(now_hour: i64) -> Self {
        Self {
            key: None,
            runs: BTreeMap::new(),
            catching_up: false,
            windows: Windows::new(now_hour),
            long: LongRender::default(),
        }
    }

    /// When the key's data starts: its first run's start.
    pub fn since_ms(&self) -> Option<i64> {
        self.runs.values().map(|r| r.started_ms).min()
    }

    pub fn run_ids(&self) -> Vec<i64> {
        self.runs.keys().copied().collect()
    }

    /// Where an unclosed episode of `run_id` stops, for hour overlap: its run's end, else now.
    pub fn run_end(&self, run_id: i64, now_ms: i64) -> i64 {
        self.runs
            .get(&run_id)
            .and_then(|r| r.end_ms)
            .unwrap_or(now_ms)
            .min(now_ms)
    }
}

/// Every UTC hour touched by [from_ms, to_ms], both ends included.
pub fn hours_between(from_ms: i64, to_ms: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut h = hour_of(from_ms);
    let end = hour_of(to_ms.max(from_ms));
    while h <= end {
        out.push(h);
        h += HOUR_MS;
    }
    out
}

/// Sorted hours as runs of consecutive hours, at most `max_hours` long each.
pub fn contiguous(hours: &BTreeSet<i64>, max_hours: i64) -> Vec<Vec<i64>> {
    let cap = usize::try_from(max_hours.max(1)).unwrap_or(1);
    let mut out: Vec<Vec<i64>> = Vec::new();
    for h in hours {
        match out.last_mut() {
            Some(g) if g.len() < cap && g.last().is_some_and(|l| l + HOUR_MS == *h) => g.push(*h),
            _ => out.push(vec![*h]),
        }
    }
    out
}
