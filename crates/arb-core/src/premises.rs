//! Premise versions (Phase 5a spec §4). One table names every area a card can depend on, its code
//! version, and the source files that define it. `premises.lock` holds a sha256 of each area's
//! files; `tests/premises_lock.rs` fails when a file changed but its area's version didn't. The
//! fix: bump the version here (every card that uses the area restarts on a new premise key), then
//!     PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock
//! A run records `area → effective version` in `run_premises`; a card counts only runs whose
//! versions match its premise key, so different premises never mix.
//!
//! `bins/scanner/src/engine.rs` is deliberately not locked (it changes weekly for unrelated
//! reasons); the thresholds behind the flags cards exclude enter through `episode_def_section`.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// (area, code version, source files relative to the workspace root), sorted by area. A version
/// is a plain token: no `=`, `;`, `+`, `|`, `:` or spaces.
pub const PREMISES: &[(&str, &str, &[&str])] = &[
    ("censoring", "1", &["crates/arb-core/src/episode.rs"]),
    ("depth", "1", &["crates/arb-core/src/fill.rs"]),
    ("detector_no", "1", &["crates/arb-core/src/detect.rs"]),
    ("detector_yes", "1", &["crates/arb-core/src/detect.rs"]),
    ("episode_def", "1", &["crates/arb-core/src/episode.rs"]),
    ("fees", "1", &["crates/arb-core/src/fee.rs"]),
    // The host's version is its label and latency band (host_version); no code defines it.
    ("host", "1", &[]),
    (
        "sim",
        "3",
        &[
            "crates/sim/src/account.rs",
            "crates/sim/src/config.rs",
            "crates/sim/src/consumed.rs",
            "crates/sim/src/latency.rs",
            "crates/sim/src/policy.rs",
            "crates/sim/src/trader.rs",
            "crates/sim/src/types.rs",
            "crates/sim/src/venue.rs",
        ],
    ),
];

pub fn code_version(area: &str) -> Option<&'static str> {
    PREMISES.iter().find(|p| p.0 == area).map(|p| p.1)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// The first 8 hex digits of sha256(text).
pub fn sha8(text: &str) -> String {
    let d = Sha256::digest(text.as_bytes());
    hex(d.get(..4).unwrap_or_default())
}

/// `{code}+{sha8(config_section)}`: a changed config section behaves like a code change (spec §4).
pub fn effective_version(code: &str, config_section: &str) -> String {
    format!("{code}+{}", sha8(config_section))
}

/// The scanner config behind `episode_def`: the thresholds of the flags OUR METHOD excludes.
pub fn episode_def_section(
    stale_lag_ms: i64,
    suspect_window_ms: i64,
    l1_suspect_window_ms: i64,
    drift_settle_ms: i64,
) -> String {
    format!(
        "stale_lag_ms={stale_lag_ms};suspect_window_ms={suspect_window_ms};\
         l1_suspect_window_ms={l1_suspect_window_ms};drift_settle_ms={drift_settle_ms}"
    )
}

/// The scanner config behind `censoring`: the feed-gap rule (parent spec §5.5).
pub fn censoring_section(gap_ms: i64) -> String {
    format!("gap_ms={gap_ms}")
}

/// The paper trader's config: the frozen policies file, by its sha256.
pub fn sim_section(policies_sha256: &str) -> String {
    format!("policies_sha256={policies_sha256}")
}

/// The band of a measured latency (feed lag p50 + REST RTT p50 / 2, spec §6.3).
pub fn host_band(measured_ms: Option<i64>) -> &'static str {
    match measured_ms {
        None => "unmeasured",
        Some(m) if m < 25 => "lt25",
        Some(m) if m < 100 => "25-100",
        Some(m) if m <= 250 => "100-250",
        Some(_) => "gt250",
    }
}

/// `SCANNER_HOST` labels: 1 to 32 of `a-z`, `0-9` and `-`.
pub fn valid_host_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 32
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The `host` area's version: `{code}:{label}|{band}`.
pub fn host_version(label: &str, measured_ms: Option<i64>) -> String {
    let code = code_version("host").unwrap_or("0");
    format!("{code}:{label}|{}", host_band(measured_ms))
}

/// A card's premise key: `area=version` for each of its areas, sorted by area, joined by `;`.
/// None when a run lacks one of the areas (such a run never counts for the card).
pub fn premise_key(areas: &[&str], versions: &BTreeMap<String, String>) -> Option<String> {
    let mut sorted: Vec<&str> = areas.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::with_capacity(sorted.len());
    for a in sorted {
        parts.push(format!("{a}={}", versions.get(a)?));
    }
    Some(parts.join(";"))
}

/// sha256 over every file as `path NUL text-without-\r NUL`, in the listed order, so a CRLF
/// checkout on Windows and an LF checkout in CI agree.
pub fn files_hash(files: &[(&str, &str)]) -> String {
    let mut h = Sha256::new();
    for (path, text) in files {
        h.update(path.as_bytes());
        h.update([0u8]);
        let stripped: String = text.chars().filter(|c| *c != '\r').collect();
        h.update(stripped.as_bytes());
        h.update([0u8]);
    }
    hex(&h.finalize())
}

/// `(area, version, hash)` lines; `#` comments, blank lines and `\r` are ignored.
pub fn parse_lock(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((
                it.next()?.to_string(),
                it.next()?.to_string(),
                it.next()?.to_string(),
            ))
        })
        .collect()
}

pub fn render_lock(entries: &[(String, String, String)]) -> String {
    let mut out = String::from(
        "# premises.lock: area version sha256(files, \\r stripped). Never edit by hand.\n\
         # Refresh after a version bump: PREMISES_LOCK_WRITE=1 cargo test -p arb-core --test premises_lock\n",
    );
    for (a, v, h) in entries {
        let _ = writeln!(out, "{a} {v} {h}");
    }
    out
}

/// Everything wrong between the current `(area, version, hash)` triples and the lock; empty when
/// they agree. A changed hash under an unchanged version is the one a refresh can't fix.
pub fn lock_problems(
    current: &[(String, String, String)],
    lock: &[(String, String, String)],
) -> Vec<String> {
    let mut out = Vec::new();
    for (area, version, hash) in current {
        match lock.iter().find(|l| &l.0 == area) {
            None => out.push(format!("{area}: not in premises.lock; refresh the lock")),
            Some((_, lv, lh)) if lh != hash && lv == version => out.push(format!(
                "{area}: its files changed but its version is still {version}: bump it in \
                 crates/arb-core/src/premises.rs (cards restart on the new key), then refresh the lock"
            )),
            Some((_, lv, _)) if lv != version => {
                out.push(format!("{area}: version {lv} → {version}; refresh the lock"))
            }
            Some(_) => {}
        }
    }
    for (a, _, _) in lock {
        if !current.iter().any(|c| &c.0 == a) {
            out.push(format!(
                "{a}: in premises.lock but no longer in PREMISES; refresh the lock"
            ));
        }
    }
    out
}

/// The refreshed lock text, or the unbumped changes that make a refresh refuse.
pub fn refreshed_lock(
    current: &[(String, String, String)],
    lock: &[(String, String, String)],
) -> Result<String, Vec<String>> {
    let unbumped: Vec<String> = lock_problems(current, lock)
        .into_iter()
        .filter(|p| p.contains("version is still"))
        .collect();
    if unbumped.is_empty() {
        Ok(render_lock(current))
    } else {
        Err(unbumped)
    }
}
