//! Clock-offset estimation from CLOB `/time` samples (scanner-clock-offset brief). Pure, no IO:
//! consumes `pm_api::clob::TimeSample`s the scanner already collected.
//!
//! One sample says: server second `S` was current at some instant in `[sent_ms, recv_ms]` (our
//! local wall clock). The offset `o = server_time − local_time` therefore lies in the window
//! `[S*1000 − recv_ms, (S+1)*1000 − sent_ms)` (closed-open; integers only, no floats).
use pm_api::clob::TimeSample;

/// A minute whose `|clock_offset_ms|` exceeds this is skewed enough that its measured host
/// latency is unreliable (spec: `bins/scanner/src/premises.rs` `HostBand::observe`). The one place
/// this threshold is defined.
pub const CLOCK_SKEW_MS: i64 = 250;

/// `(lo, hi)`: the closed-open window of possible offsets (ms) implied by one sample. Saturating
/// throughout: `pm_api::clob`'s `parse_time_body` already bounds `server_s` to a sane unix-second
/// range, but this is a pure fn over any `TimeSample` at all, so it must never panic (debug) or
/// silently wrap (release) — a single bad or adversarial reply must not take the health task down.
pub fn offset_window(s: &TimeSample) -> (i64, i64) {
    let lo = s.server_s.saturating_mul(1000).saturating_sub(s.recv_ms);
    let hi = s
        .server_s
        .saturating_add(1)
        .saturating_mul(1000)
        .saturating_sub(s.sent_ms);
    (lo, hi)
}

/// The offset estimate for a minute's samples: `None` if there were none; otherwise the midpoint
/// of the intersection of every sample's window, or — if a clock jump inside the window makes the
/// intersection empty — the midpoint of the newest sample's own window (by `recv_ms`). Note: if
/// the clock steps partway through the minute, this fallback reports the offset *after* the step
/// for that whole minute (the newest sample wins) — accepted, since the step itself is exactly
/// what the caller (`HostBand::observe`) needs surfaced, not averaged away.
pub fn estimate(samples: &[TimeSample]) -> Option<i64> {
    let (lo, hi) = samples
        .iter()
        .map(offset_window)
        .reduce(|(lo, hi), (l, h)| (lo.max(l), hi.min(h)))?;
    if lo < hi {
        return Some(lo + (hi - lo) / 2);
    }
    // Empty intersection: a clock jump happened inside the sampling window. Trust the newest
    // sample instead of an intersection that can no longer be true of all of them at once.
    let newest = samples.iter().max_by_key(|s| s.recv_ms)?;
    let (l, h) = offset_window(newest);
    Some(l + (h - l) / 2)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Build a sample as if the true offset were `true_offset_ms` and the round trip took
    /// `rtt_ms`, sent at local time `local_sent_ms`.
    fn sample_at(local_sent_ms: i64, rtt_ms: i64, true_offset_ms: i64) -> TimeSample {
        let recv_ms = local_sent_ms + rtt_ms;
        // The server's clock reads (local + true_offset) throughout; its second ticked over at
        // some point in [sent, recv] — pin it at the midpoint of the request for realism.
        let server_now_ms = local_sent_ms + rtt_ms / 2 + true_offset_ms;
        let server_s = server_now_ms.div_euclid(1000);
        TimeSample {
            sent_ms: local_sent_ms,
            recv_ms,
            server_s,
        }
    }

    #[test]
    fn empty_input_is_none() {
        assert_eq!(estimate(&[]), None);
    }

    #[test]
    fn a_single_sample_is_the_midpoint_of_its_window() {
        let s = TimeSample {
            sent_ms: 1_000,
            recv_ms: 1_040,
            server_s: 2,
        };
        let (lo, hi) = offset_window(&s);
        assert_eq!((lo, hi), (2000 - 1040, 3000 - 1000));
        assert_eq!(estimate(&[s]), Some(lo + (hi - lo) / 2));
    }

    #[test]
    fn several_samples_at_different_sub_second_phases_narrow_to_the_true_offset() {
        let true_offset = 2_000;
        let samples: Vec<TimeSample> = (0..20)
            .map(|i| sample_at(i * 137, 20, true_offset))
            .collect();
        let est = estimate(&samples).unwrap();
        assert!(
            (true_offset - 20..=true_offset + 20).contains(&est),
            "est={est} true={true_offset}"
        );

        let true_offset = -37;
        let samples: Vec<TimeSample> = (0..20)
            .map(|i| sample_at(i * 211, 20, true_offset))
            .collect();
        let est = estimate(&samples).unwrap();
        assert!(
            (true_offset - 20..=true_offset + 20).contains(&est),
            "est={est} true={true_offset}"
        );
    }

    #[test]
    fn an_empty_intersection_falls_back_to_the_newest_sample() {
        // Two samples whose windows can't both be true at once (a clock jump mid-window): the
        // older one implies an offset near 0, the newer implies an offset near +5000.
        let old = TimeSample {
            sent_ms: 0,
            recv_ms: 10,
            server_s: 0,
        };
        let new = TimeSample {
            sent_ms: 100,
            recv_ms: 110,
            server_s: 5,
        };
        let (lo, hi) = offset_window(&old);
        assert!(lo < hi);
        let (nlo, nhi) = offset_window(&new);
        assert!(nlo < nhi);
        // sanity: these two windows really don't overlap
        assert!(lo.max(nlo) >= hi.min(nhi));
        let expected = nlo + (nhi - nlo) / 2;
        assert_eq!(estimate(&[old, new]), Some(expected));
        // order shouldn't matter: still the newest by recv_ms
        assert_eq!(estimate(&[new, old]), Some(expected));
    }

    #[test]
    fn offset_window_saturates_instead_of_overflowing() {
        // pm-api's `parse_time_body` already bounds `server_s`, but this is a pure fn: it must
        // never panic (debug) or wrap (release) on any `TimeSample` at all, belt and braces.
        let huge = TimeSample {
            sent_ms: 0,
            recv_ms: 0,
            server_s: i64::MAX,
        };
        let (lo, hi) = offset_window(&huge);
        assert_eq!((lo, hi), (i64::MAX, i64::MAX));

        let tiny = TimeSample {
            sent_ms: 0,
            recv_ms: 0,
            server_s: i64::MIN,
        };
        let (lo, hi) = offset_window(&tiny);
        assert_eq!((lo, hi), (i64::MIN, i64::MIN));

        // extreme sent/recv (not just server_s) must not panic either: both subtractions
        // overflow positive here and saturate up.
        let extreme_bracket = TimeSample {
            sent_ms: i64::MIN,
            recv_ms: i64::MIN,
            server_s: 0,
        };
        let (lo, hi) = offset_window(&extreme_bracket);
        assert_eq!(lo, i64::MAX); // 0 - i64::MIN overflows positive, saturates up
        assert_eq!(hi, i64::MAX); // 1000 - i64::MIN overflows positive, saturates up
    }

    #[test]
    fn a_large_rtt_widens_the_window_but_never_excludes_the_truth() {
        let true_offset = 500;
        let tight = sample_at(0, 10, true_offset);
        let wide = sample_at(50, 4_000, true_offset);
        let (tlo, thi) = offset_window(&tight);
        let (wlo, whi) = offset_window(&wide);
        assert!(whi - wlo > thi - tlo, "a bigger rtt must widen the window");
        // the true offset lies inside both windows
        assert!(tlo <= true_offset && true_offset < thi);
        assert!(wlo <= true_offset && true_offset < whi);
        let est = estimate(&[tight, wide]).unwrap();
        assert!(
            (true_offset - 10..=true_offset + 10).contains(&est),
            "est={est}"
        );
    }
}
