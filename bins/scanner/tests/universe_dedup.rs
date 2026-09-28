#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §4.5: a universe record is written only when its content changed; an unchanged refresh
//! still captures a control line, naming the last record, so replay is unchanged; retention never
//! deletes the record still in force for the oldest remaining hour.
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::*;
use scanner::engine::EngineCtx;
use scanner::metrics::Metrics;
use scanner::replay::replay_dir;
use scanner::universe::{ConnMap, GroupMeta, LegMeta, Status, Universe};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, UNIX_EPOCH};

const T0: i64 = 1_790_121_600_000;
const H: i64 = 3_600_000;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "scanner-dedup-{name}-{}-{}",
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
        issue_throttle_ms: 5_000,
    }
}

fn universe(taken_ms: i64, status: Status) -> Universe {
    let legs = (0..3)
        .map(|i| LegMeta {
            market_id: format!("m{i}"),
            yes_token: format!("y{i}"),
            no_token: format!("n{i}"),
            status: if i == 2 { status } else { Status::Live },
            fee_rate: Decimal::ZERO,
            fee_exponent: 1,
            tick: Some(dec!(0.01)),
            min_size: Some(dec!(5)),
            seconds_delay: Some(0),
            end_date: None,
        })
        .collect();
    Universe {
        taken_ms,
        groups: vec![GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: None,
            augmented: false,
            legs,
            tags: None,
        }],
        canary: vec![],
    }
}

fn conn(c: usize) -> ConnMap {
    [("e1".to_string(), c)].into_iter().collect()
}

/// Capture is running and has written everything up to record `acked_seq`.
fn ok(acked_seq: u64) -> CaptureState {
    CaptureState {
        acked_seq,
        paused: false,
        drops: 0,
    }
}

#[test]
fn an_unchanged_refresh_writes_no_record_but_names_the_last_one() {
    let mut d = UniverseDedup::default();
    let u1 = universe(T0, Status::Live);
    let UniversePlan::Write { hash } = d.plan(&u1, &conn(0), ok(0)) else {
        panic!("first refresh writes")
    };
    d.written(hash, T0, universe_file_name(T0, 1), 1, 0);
    let u2 = universe(T0 + 300_000, Status::Live); // same content, new taken_ms
    assert_eq!(
        d.plan(&u2, &conn(0), ok(1)),
        UniversePlan::Reuse {
            taken_ms: T0,
            file: universe_file_name(T0, 1)
        }
    );
    assert!(
        matches!(
            d.plan(&universe(T0 + 600_000, Status::Halted), &conn(0), ok(1)),
            UniversePlan::Write { .. }
        ),
        "content changed"
    );
    assert!(
        matches!(d.plan(&u2, &conn(1), ok(1)), UniversePlan::Write { .. }),
        "connection map changed"
    );
    assert_ne!(
        universe_hash(&u1, &conn(0)).unwrap(),
        universe_hash(&u1, &conn(1)).unwrap()
    );
    assert_eq!(
        universe_hash(&u1, &conn(0)).unwrap(),
        universe_hash(&u2, &conn(0)).unwrap()
    );
}

#[test]
fn a_record_capture_never_wrote_is_written_again_once_capture_resumes() {
    let mut d = UniverseDedup::default();
    let (u1, u2, u3) = (
        universe(T0, Status::Live),
        universe(T0 + 300_000, Status::Live),
        universe(T0 + 600_000, Status::Live),
    );
    let UniversePlan::Write { hash } = d.plan(&u1, &conn(0), ok(0)) else {
        panic!("first refresh writes")
    };
    d.written(hash, T0, universe_file_name(T0, 7), 7, 0);
    // capture paused on low disk: record 7 was dropped and never acknowledged
    let paused = CaptureState {
        acked_seq: 0,
        paused: true,
        drops: 1,
    };
    assert!(
        matches!(d.plan(&u2, &conn(0), paused), UniversePlan::Write { .. }),
        "never name a record while paused"
    );
    // resumed, but record 7 never reached disk: write a new one
    let resumed = CaptureState {
        acked_seq: 0,
        paused: false,
        drops: 1,
    };
    let UniversePlan::Write { hash } = d.plan(&u2, &conn(0), resumed) else {
        panic!("unacknowledged: write again")
    };
    d.written(
        hash,
        T0 + 300_000,
        universe_file_name(T0 + 300_000, 8),
        8,
        1,
    );
    // record 8 acknowledged: the next unchanged refresh names it
    let acked = CaptureState {
        acked_seq: 8,
        paused: false,
        drops: 1,
    };
    assert_eq!(
        d.plan(&u3, &conn(0), acked),
        UniversePlan::Reuse {
            taken_ms: T0 + 300_000,
            file: universe_file_name(T0 + 300_000, 8)
        }
    );
    // a message dropped since record 8 was written: write again (the drop may have been its line)
    let dropped = CaptureState {
        acked_seq: 8,
        paused: false,
        drops: 2,
    };
    assert!(matches!(
        d.plan(&u3, &conn(0), dropped),
        UniversePlan::Write { .. }
    ));
}

#[test]
fn deliver_acknowledges_only_records_that_reach_disk() {
    let dir = tmp("deliver");
    let m = Metrics::default();
    let mut w = Writer::new(dir.clone());
    let file = universe_file_name(T0, 42);
    let rec = CaptureMsg::Universe(Box::new(UniverseRecord {
        universe: universe(T0, Status::Live),
        conn_of: conn(0),
        file: file.clone(),
    }));
    m.capture_paused.store(true, Ordering::SeqCst);
    assert!(!deliver(&mut w, &rec, &m));
    assert_eq!(
        (
            m.universe_acked_seq.load(Ordering::SeqCst),
            m.capture_drops.load(Ordering::SeqCst)
        ),
        (0, 1)
    );
    assert!(!universe_path(&dir, T0, &file).exists());
    m.capture_paused.store(false, Ordering::SeqCst);
    assert!(deliver(&mut w, &rec, &m));
    assert_eq!(m.universe_acked_seq.load(Ordering::SeqCst), 42);
    assert!(universe_path(&dir, T0, &file).exists());
    assert_eq!(
        CaptureState::of(&m),
        CaptureState {
            acked_seq: 42,
            paused: false,
            drops: 1
        }
    );
    std::fs::remove_dir_all(&dir).ok();
}

fn book_frame(ts: i64, ask: &str) -> String {
    let items: Vec<String> = (0..3)
        .map(|i| format!(r#"{{"event_type":"book","asset_id":"y{i}","market":"0x","timestamp":"{ts}","hash":"h","bids":[],"asks":[{{"price":"{ask}","size":"10"}}]}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

fn write(dir: &Path, reuse: bool) {
    let u1 = universe(T0, Status::Live);
    let f1 = universe_file_name(T0, 1);
    let mut w = Writer::new(dir.to_path_buf());
    let mut msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u1.clone(),
            conn_of: conn(0),
            file: f1.clone(),
        })),
        CaptureMsg::Line(Line {
            recv_ts_us: (T0 + 1_000) * 1000,
            shard: 0,
            raw: None,
            ctl: Some(Ctl::Universe {
                taken_ms: T0,
                file: f1.clone(),
            }),
        }),
        CaptureMsg::Line(Line {
            recv_ts_us: (T0 + 2_000) * 1000,
            shard: 0,
            raw: Some(book_frame(T0 + 1_990, "0.30")),
            ctl: None,
        }),
    ];
    let t2 = T0 + 300_000;
    let second = if reuse {
        Ctl::Universe {
            taken_ms: T0,
            file: f1,
        }
    } else {
        let f2 = universe_file_name(t2, 2);
        msgs.push(CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: universe(t2, Status::Live),
            conn_of: conn(0),
            file: f2.clone(),
        })));
        Ctl::Universe {
            taken_ms: t2,
            file: f2,
        }
    };
    msgs.push(CaptureMsg::Line(Line {
        recv_ts_us: (t2 + 1_000) * 1000,
        shard: 0,
        raw: None,
        ctl: Some(second),
    }));
    msgs.push(CaptureMsg::Line(Line {
        recv_ts_us: (t2 + 2_000) * 1000,
        shard: 0,
        raw: Some(book_frame(t2 + 1_990, "0.31")),
        ctl: None,
    }));
    msgs.push(CaptureMsg::Line(Line {
        recv_ts_us: (t2 + 3_000) * 1000,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::Shutdown),
    }));
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

#[test]
fn replay_of_a_reused_record_equals_replay_of_a_fresh_identical_one() {
    let (a, b) = (tmp("reuse"), tmp("fresh"));
    write(&a, true);
    write(&b, false);
    let (ra, rb) = (
        replay_dir(&a, ctx()).unwrap(),
        replay_dir(&b, ctx()).unwrap(),
    );
    assert_eq!(format!("{:?}", ra.outs), format!("{:?}", rb.outs));
    assert!(!ra.outs.is_empty());
    std::fs::remove_dir_all(&a).ok();
    std::fs::remove_dir_all(&b).ok();
}

fn touch(p: &Path, ms: i64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap()))
        .unwrap();
}

#[test]
fn retention_keeps_the_universe_record_in_force_at_the_oldest_hour() {
    let dir = tmp("keep");
    let t = T0 + 10 * H;
    let rec = |taken: i64, seq: u64, mtime: i64| {
        let p = universe_path(&dir, taken, &universe_file_name(taken, seq));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        touch(&p, mtime);
        p
    };
    let older = rec(t - 3 * H, 1, t - 3 * H);
    let in_force = rec(t - 2 * H, 2, t - 2 * H); // written before the oldest hour, still named by it
    let fresh = rec(t + 60_000, 3, t + 61_000);
    assert!(
        stale_universe_files(&dir).is_empty(),
        "no hour files: nothing to compare against"
    );
    let hour = hour_path(&dir, t);
    std::fs::create_dir_all(hour.parent().unwrap()).unwrap();
    std::fs::write(&hour, b"x").unwrap();
    assert_eq!(stale_universe_files(&dir), vec![older]);
    assert!(in_force.exists() && fresh.exists());
    std::fs::remove_dir_all(&dir).ok();
}
