//! Shared counters for the health task (spec §10 `health`). Counters are drained per minute; gauges are read.
use crate::clock;
use pm_api::clob::TimeSample;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64};
use store::HealthRow;

#[derive(Debug, Default)]
pub struct Metrics {
    pub msgs: AtomicU64,
    pub bytes: AtomicU64,
    pub drops: AtomicU64,
    pub reconnects: AtomicU64,
    pub capture_drops: AtomicU64,
    /// Rows the writer dropped: over-cap evictions plus quarantine failures (spec §10, F2).
    pub writer_drops: AtomicU64,
    /// Hour/universe files the capture thread deleted (retention, budget, or stale universe
    /// housekeeping — spec §10, F2).
    pub capture_deletes: AtomicU64,
    pub parked: AtomicU64,
    pub tokens: AtomicU64,
    pub shards: AtomicU64,
    pub dropped_groups: AtomicU64,
    pub capture_paused: AtomicBool,
    pub lags: Mutex<Vec<i64>>,
    pub rest_ms: Mutex<Vec<i64>>,
    pub rtt_ms: Mutex<Vec<i64>>,
    pub ok: Mutex<Option<bool>>,
    /// CLOB `/time` samples for this minute's clock-offset estimate (scanner-clock-offset brief).
    pub clock: Mutex<Vec<TimeSample>>,
}

/// Sorted-index percentile: element at (len − 1) · pct / 100 of an ascending slice.
pub fn percentile(sorted: &[i64], pct: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    sorted.get((sorted.len() - 1) * pct.min(100) / 100).copied()
}

fn i32_of(v: u64) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

fn take_sorted(m: &Mutex<Vec<i64>>) -> Vec<i64> {
    let mut v = std::mem::take(&mut *m.lock().unwrap_or_else(|p| p.into_inner()));
    v.sort_unstable();
    v
}

impl Metrics {
    pub fn push_lag(&self, v: i64) {
        self.lags.lock().unwrap_or_else(|p| p.into_inner()).push(v);
    }

    pub fn push_rest(&self, v: i64) {
        self.rest_ms
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(v);
    }

    /// One CLOB request's round trip (pm-api's RttSink).
    pub fn push_rtt(&self, v: i64) {
        self.rtt_ms
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(v);
    }

    pub fn set_ok(&self, ok: bool) {
        *self.ok.lock().unwrap_or_else(|p| p.into_inner()) = Some(ok);
    }

    /// One CLOB `/time` sample for this minute's clock-offset estimate.
    pub fn push_clock(&self, s: TimeSample) {
        self.clock.lock().unwrap_or_else(|p| p.into_inner()).push(s);
    }

    pub fn minute_row(&self, run_id: i64, minute_ms: i64) -> HealthRow {
        let lags = take_sorted(&self.lags);
        let rest = take_sorted(&self.rest_ms);
        let rtt = take_sorted(&self.rtt_ms);
        let clock_samples =
            std::mem::take(&mut *self.clock.lock().unwrap_or_else(|p| p.into_inner()));
        let p = |v: &[i64], q: usize| {
            percentile(v, q).map(|x| {
                i32::try_from(x.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
            })
        };
        let clock_offset_ms = clock::estimate(&clock_samples)
            .map(|x| i32::try_from(x.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0));
        HealthRow {
            run_id,
            minute_ms,
            tokens: i32_of(self.tokens.load(Ordering::Relaxed)),
            shards: i32_of(self.shards.load(Ordering::Relaxed)),
            msgs: i64::try_from(self.msgs.swap(0, Ordering::Relaxed)).unwrap_or(i64::MAX),
            bytes: i64::try_from(self.bytes.swap(0, Ordering::Relaxed)).unwrap_or(i64::MAX),
            drops: i32_of(self.drops.swap(0, Ordering::Relaxed)),
            reconnects: i32_of(self.reconnects.swap(0, Ordering::Relaxed)),
            rest_p50_ms: p(&rest, 50),
            rest_p99_ms: p(&rest, 99),
            lag_p50_ms: p(&lags, 50),
            lag_p99_ms: p(&lags, 99),
            clock_offset_ms,
            ok: *self.ok.lock().unwrap_or_else(|p| p.into_inner()),
            capture_paused: self.capture_paused.load(Ordering::Relaxed),
            parked_shards: i32_of(self.parked.load(Ordering::Relaxed)),
            dropped_groups: i32_of(self.dropped_groups.load(Ordering::Relaxed)),
            rest_rtt_p50_ms: p(&rtt, 50),
            rest_rtt_p99_ms: p(&rtt, 99),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_sorted_index_rule() {
        let v: Vec<i64> = (1..=100).collect();
        assert_eq!(percentile(&v, 50), Some(50));
        assert_eq!(percentile(&v, 99), Some(99));
        assert_eq!(percentile(&[], 50), None);
        assert_eq!(percentile(&[-5], 99), Some(-5));
    }

    #[test]
    fn minute_row_reports_rest_round_trip_percentiles_and_drains_them() {
        let m = Metrics::default();
        for v in [40, 10, 30, 20, 1_000] {
            m.push_rtt(v);
        }
        let r = m.minute_row(1, 60_000);
        assert_eq!((r.rest_rtt_p50_ms, r.rest_rtt_p99_ms), (Some(30), Some(40)));
        let r2 = m.minute_row(1, 120_000);
        assert_eq!((r2.rest_rtt_p50_ms, r2.rest_rtt_p99_ms), (None, None));
    }

    #[test]
    fn minute_row_drains_counters_and_keeps_gauges() {
        let m = Metrics::default();
        m.msgs.fetch_add(10, std::sync::atomic::Ordering::Relaxed);
        m.shards.store(3, std::sync::atomic::Ordering::Relaxed);
        m.push_lag(200);
        m.push_lag(100);
        m.set_ok(true);
        let r = m.minute_row(1, 60_000);
        assert_eq!(
            (r.msgs, r.shards, r.lag_p50_ms, r.ok),
            (10, 3, Some(100), Some(true))
        );
        let r2 = m.minute_row(1, 120_000);
        assert_eq!((r2.msgs, r2.shards, r2.lag_p50_ms), (0, 3, None));
    }

    #[test]
    fn minute_row_fills_clock_offset_from_samples_and_drains_them() {
        let m = Metrics::default();
        m.push_clock(TimeSample {
            sent_ms: 1_000,
            recv_ms: 1_040,
            server_s: 2,
        });
        m.push_clock(TimeSample {
            sent_ms: 1_500,
            recv_ms: 1_530,
            server_s: 2,
        });
        // window 1: [2000-1040, 3000-1000) = [960, 2000); window 2: [2000-1530, 3000-1500) =
        // [470, 1500). Intersection [960, 1500), midpoint 960 + (1500-960)/2 = 1230.
        let r = m.minute_row(1, 60_000);
        assert_eq!(r.clock_offset_ms, Some(1230));
        let r2 = m.minute_row(1, 120_000);
        assert_eq!(r2.clock_offset_ms, None, "drained, no samples this minute");
    }

    #[test]
    fn minute_row_clock_offset_is_none_with_no_samples() {
        let m = Metrics::default();
        let r = m.minute_row(1, 60_000);
        assert_eq!(r.clock_offset_ms, None);
    }
}
