#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::{
    CaptureMsg, Ctl, DiskGuard, Line, UniverseRecord, Writer, civil, expired_hours, hour_path,
};
use scanner::engine::{EngineCtx, Out};
use scanner::replay::replay_dir;
use scanner::universe::{ConnMap, GroupMeta, LegMeta, Status, Universe};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "scanner-test-{name}-{}-{}",
        std::process::id(),
        scanner::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ctx() -> EngineCtx {
    EngineCtx {
        run_id: 0,
        source: "replay".into(),
        scanner_version: "t".into(),
        config_hash: "c".into(),
        gap_ms: 30_000,
        stale_lag_ms: 500,
        suspect_window_ms: 60_000,
        l1_suspect_window_ms: 3_600_000,
        drift_settle_ms: 2_000,
    }
}

fn universe() -> Universe {
    let legs = (0..3)
        .map(|i| LegMeta {
            market_id: format!("m{i}"),
            yes_token: format!("y{i}"),
            no_token: format!("n{i}"),
            status: Status::Live,
            fee_rate: Decimal::ZERO,
            fee_exponent: 1,
            tick: Some(dec!(0.01)),
            min_size: Some(dec!(5)),
            seconds_delay: Some(0),
            end_date: None,
        })
        .collect();
    Universe {
        taken_ms: 1_790_121_600_000,
        groups: vec![GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: None,
            augmented: false,
            legs,
        }],
        canary: vec![],
    }
}

fn book_frame(ts: i64, asks: [&str; 3]) -> String {
    let items: Vec<String> = asks
        .iter()
        .enumerate()
        .map(|(i, a)| {
            format!(r#"{{"event_type":"book","asset_id":"y{i}","market":"0x","timestamp":"{ts}","hash":"h","bids":[],"asks":[{{"price":"{a}","size":"10"}}]}}"#)
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn line(ts_ms: i64, raw: Option<String>, ctl: Option<Ctl>) -> CaptureMsg {
    CaptureMsg::Line(Line {
        recv_ts_us: ts_ms * 1000,
        shard: 0,
        raw,
        ctl,
    })
}

#[test]
fn civil_dates_match_known_values() {
    assert_eq!(civil(0), (1970, 1, 1, 0));
    assert_eq!(civil(1_790_121_600_000), (2026, 9, 23, 0));
    assert_eq!(civil(1_790_200_799_999), (2026, 9, 23, 21));
    assert_eq!(civil(951_782_400_000), (2000, 2, 29, 0));
    let p = hour_path(std::path::Path::new("/data/capture"), 1_790_200_799_999);
    assert!(p.ends_with("2026-09-23/21.jsonl.zst"), "{}", p.display());
}

#[test]
fn disk_guard_has_hysteresis() {
    let gb = 1_000_000_000_u64;
    let mut g = DiskGuard::new(10, 15);
    assert!(!g.update(20 * gb));
    assert!(g.update(9 * gb));
    assert!(g.update(12 * gb), "stays paused until 15 GB");
    assert!(!g.update(16 * gb));
}

#[test]
fn retention_lists_only_old_hour_files() {
    let dir = tmp("retention");
    let t = 1_790_121_600_000_i64; // 2026-09-23 00:00Z
    for h in [t, t + 3_600_000, t + 50 * 3_600_000] {
        let p = hour_path(&dir, h);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let expired = expired_hours(&dir, t + 50 * 3_600_000, 48);
    assert_eq!(expired.len(), 2, "{expired:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn replay_reproduces_episodes_and_censoring_byte_for_byte() {
    let dir = tmp("replay");
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let mut w = Writer::new(dir.clone());
    let t0 = 1_790_121_600_000_i64;
    let msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
        })),
        line(
            t0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
            }),
        ),
        line(
            t0 + 100,
            Some(book_frame(t0, ["0.40", "0.35", "0.30"])),
            None,
        ), // ΣA 1.05
        line(
            t0 + 1_100,
            Some(book_frame(t0 + 1_000, ["0.30", "0.35", "0.30"])),
            None,
        ), // 0.95: open
        line(t0 + 1_500, None, Some(Ctl::Down)), // right-censored close
        line(
            t0 + 2_100,
            Some(book_frame(t0 + 2_000, ["0.30", "0.35", "0.30"])),
            None,
        ), // left-censored open
        line(t0 + 2_200, Some("{broken".into()), None),
        line(t0 + 3_000, None, Some(Ctl::Shutdown)),
    ];
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();

    let a = replay_dir(&dir, ctx()).unwrap();
    let b = replay_dir(&dir, ctx()).unwrap();
    assert_eq!(a.outs, b.outs, "deterministic");
    assert_eq!((a.frames, a.parse_errors), (4, 1));
    let rows: Vec<_> = a
        .outs
        .iter()
        .filter_map(|o| match o {
            Out::Episode(w) => Some(w.row.clone()),
            _ => None,
        })
        .collect();
    let first_close = rows
        .iter()
        .find(|r| r.episode_id == "replay-0-00000001" && r.closed_server_ms.is_some())
        .unwrap();
    assert!(first_close.censor_right);
    assert_eq!(first_close.closed_server_ms, Some(t0 + 1_500));
    let second = rows
        .iter()
        .rfind(|r| r.episode_id == "replay-0-00000002")
        .unwrap();
    assert!(second.censor_left);
    assert!(second.censor_right, "closed by shutdown");
    assert_eq!(second.closed_server_ms, Some(t0 + 3_000));
    std::fs::remove_dir_all(&dir).ok();
}
