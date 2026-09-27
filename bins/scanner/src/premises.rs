//! Run premises (Phase 5a spec §4): what this run's episodes are computed under. Every area but
//! `host` is written at run start; `host` (SCANNER_HOST + measured latency band) is written as its
//! own row once the run's first HOST_BAND_MINUTES health minutes exist, so `run_premises` stays
//! append-only. A run that dies earlier has no `host` row and never counts for Edge capture.
use crate::config::Config;
use crate::paper::Policies;
use arb_core::premises::{self, PREMISES};
use store::{HealthRow, RunPremiseRow};

pub const HOST_BAND_MINUTES: u32 = 10;

/// `SCANNER_HOST` (e.g. `vps-kvm2`, `pc-dev`); `unknown` when unset or empty.
pub fn host_label(value: Option<String>) -> anyhow::Result<String> {
    match value.filter(|v| !v.is_empty()) {
        None => Ok("unknown".to_string()),
        Some(v) if premises::valid_host_label(&v) => Ok(v),
        Some(_) => anyhow::bail!("SCANNER_HOST must be 1 to 32 characters of a-z, 0-9 and '-'"),
    }
}

/// The config section an area's effective version hashes (spec §4). `sim` exists only with paper
/// trading on; `host` is not config-versioned (see HostBand).
pub fn section(area: &str, cfg: &Config, policies: Option<&Policies>) -> Option<String> {
    match area {
        "episode_def" => Some(premises::episode_def_section(
            cfg.engine.stale_lag_ms,
            cfg.engine.suspect_window_ms,
            cfg.engine.l1_suspect_window_ms,
            cfg.engine.drift_settle_ms,
        )),
        "censoring" => Some(premises::censoring_section(cfg.engine.gap_ms)),
        "sim" => policies.map(|p| premises::sim_section(&p.sha256)),
        "host" => None,
        _ => Some(String::new()),
    }
}

/// The rows written at run start: every area but `host`.
pub fn run_rows(run_id: i64, cfg: &Config, policies: Option<&Policies>) -> Vec<RunPremiseRow> {
    PREMISES
        .iter()
        .filter_map(|(area, code, _)| {
            section(area, cfg, policies).map(|s| RunPremiseRow {
                run_id,
                area: (*area).to_string(),
                version: premises::effective_version(code, &s),
            })
        })
        .collect()
}

/// Collects the first HOST_BAND_MINUTES health minutes' latency estimates (feed lag p50 +
/// REST RTT p50 / 2, minutes with both only) and yields the `host` row once, at the last of them.
#[derive(Debug)]
pub struct HostBand {
    run_id: i64,
    label: String,
    minutes: u32,
    samples: Vec<i64>,
    done: bool,
    pending: Option<RunPremiseRow>,
}

impl HostBand {
    pub fn new(run_id: i64, label: String) -> Self {
        Self {
            run_id,
            label,
            minutes: 0,
            samples: Vec::new(),
            done: false,
            pending: None,
        }
    }

    pub fn observe(&mut self, row: &HealthRow) -> Option<RunPremiseRow> {
        if self.done {
            return None;
        }
        self.minutes += 1;
        // A skewed clock (|offset| beyond CLOCK_SKEW_MS) makes this minute's feed lag unreliable
        // (a host freeze can make the clock run behind and lag read low or negative), so the
        // minute contributes no latency sample even though it still counts toward
        // HOST_BAND_MINUTES (scanner-clock-offset brief).
        let skewed = row
            .clock_offset_ms
            .is_some_and(|o| i64::from(o).abs() > crate::clock::CLOCK_SKEW_MS);
        if !skewed && let (Some(lag), Some(rtt)) = (row.lag_p50_ms, row.rest_rtt_p50_ms) {
            self.samples.push(i64::from(lag) + i64::from(rtt) / 2);
        }
        if self.minutes < HOST_BAND_MINUTES {
            return None;
        }
        self.done = true;
        self.samples.sort_unstable();
        // Fewer than half the minutes contributed a sample (skew, or plain missing data): the
        // band is unmeasured rather than a median built on a minority of the window.
        let median = if self.samples.len() * 2 < HOST_BAND_MINUTES as usize {
            None
        } else {
            match self.samples.len() {
                0 => None,
                n => self.samples.get((n - 1) / 2).copied(),
            }
        };
        let row = RunPremiseRow {
            run_id: self.run_id,
            area: "host".to_string(),
            version: premises::host_version(&self.label, median),
        };
        self.pending = Some(row.clone());
        Some(row)
    }

    /// The computed host row until an insert of it succeeds (`recorded`): the health task retries
    /// it each minute, so a failed insert doesn't leave the run invisible to Edge capture. The
    /// row is kept as computed, never recomputed; the insert is ON CONFLICT DO NOTHING.
    pub fn pending(&self) -> Option<&RunPremiseRow> {
        self.pending.as_ref()
    }

    /// The pending host row reached Postgres: stop retrying.
    pub fn recorded(&mut self) {
        self.pending = None;
    }
}
