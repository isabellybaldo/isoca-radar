#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
//! Shared by the compact tests: hand-built universes, capture lines, frames and configs.
use compact::config::Config;
use compact::reduce::{ReduceCfg, Reducer};
use compact::rows::{VecSink, micros};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tape::line::{Ctl, Line};
use tape::universe_view::{GroupView, LegView, RecordView, UniverseView};

pub const T0: i64 = 1_790_121_600_000; // 2026-09-23 00:00Z
pub const H: i64 = 3_600_000;

pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "compact-{name}-{}-{}",
        std::process::id(),
        compact::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A universe record named `file`: groups (event, live YES tokens; every group is NegRisk).
pub fn rec(
    file: &str,
    taken_ms: i64,
    groups: &[(&str, &[&str])],
    conn: &[(&str, usize)],
) -> RecordView {
    RecordView {
        universe: UniverseView {
            taken_ms,
            groups: groups
                .iter()
                .map(|(e, toks)| GroupView {
                    event_id: e.to_string(),
                    neg_risk_market_id: Some(format!("nr-{e}")),
                    legs: toks
                        .iter()
                        .map(|t| LegView {
                            market_id: format!("m-{t}"),
                            yes_token: t.to_string(),
                            no_token: format!("n-{t}"),
                            status: "live".into(),
                        })
                        .collect(),
                })
                .collect(),
            canary: vec![],
        },
        conn_of: conn
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<BTreeMap<_, _>>(),
        file: file.into(),
    }
}

pub fn uni(t: i64, r: &RecordView) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::Universe {
            taken_ms: r.universe.taken_ms,
            file: r.file.clone(),
        }),
    }
}

pub fn frame(t: i64, shard: usize, raw: String) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard,
        raw: Some(raw),
        ctl: None,
    }
}

pub fn down(t: i64, shard: usize) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard,
        raw: None,
        ctl: Some(Ctl::Down),
    }
}

pub fn shutdown(t: i64) -> Line {
    Line {
        recv_ts_us: t * 1000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::Shutdown),
    }
}

pub fn book(tok: &str, ts: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> String {
    let lv = |v: &[(&str, &str)]| {
        v.iter()
            .map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"event_type":"book","asset_id":"{tok}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#,
        lv(bids),
        lv(asks)
    )
}

/// One price_change item. `bb`/`ba`: the server's best bid/ask after it ("0"/"1" = empty side).
pub fn change(
    tok: &str,
    ts: i64,
    side: &str,
    price: &str,
    size: &str,
    bb: &str,
    ba: &str,
) -> String {
    format!(
        r#"{{"event_type":"price_change","market":"0x","timestamp":"{ts}","price_changes":[{{"asset_id":"{tok}","side":"{side}","price":"{price}","size":"{size}","best_bid":"{bb}","best_ask":"{ba}"}}]}}"#
    )
}

pub fn trade(tok: &str, ts: i64) -> String {
    format!(r#"{{"event_type":"last_trade_price","asset_id":"{tok}","timestamp":"{ts}"}}"#)
}

/// Micro-units of a decimal string, for expected rows.
pub fn m(x: &str) -> i64 {
    micros(Decimal::from_str_exact(x).unwrap()).unwrap()
}

/// Reducer settings for the tests: depth every 5 min, 2 levels.
pub fn rcfg() -> ReduceCfg {
    ReduceCfg {
        depth_every_ms: 300_000,
        depth_levels: 2,
    }
}

pub fn cfg(raw: &Path, archive: &Path) -> Config {
    Config {
        raw_dir: raw.to_string_lossy().to_string(),
        archive_dir: archive.to_string_lossy().to_string(),
        depth_every_secs: 300,
        depth_levels: 2,
        l2_before_secs: 300,
        l2_after_secs: 300,
        l2_max_window_secs: 1_800,
        archive_budget_gb: 55,
        lag_alarm_hours: 3,
    }
}

/// Feed lines to a reducer; universe lines resolve against `recs` by file name.
pub fn run(red: &mut Reducer, lines: &[Line], recs: &[RecordView], sink: &mut VecSink) {
    let mut load = |_: i64, file: &str| -> anyhow::Result<RecordView> {
        recs.iter()
            .find(|r| r.file == file)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no record {file}"))
    };
    for l in lines {
        red.line(l, &mut load, sink).unwrap();
    }
}

pub fn write_record(raw: &Path, r: &RecordView) {
    tape::files::write_zst_json(
        &tape::files::universe_path(raw, r.universe.taken_ms, &r.file),
        r,
    )
    .unwrap();
}

pub fn write_hour(raw: &Path, hour_ms: i64, lines: &[Line]) {
    tape::files::write_lines(&tape::files::hour_path(raw, hour_ms), lines).unwrap();
}
