#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::{
    CaptureMsg, Ctl, DiskGuard, Line, UniverseRecord, Writer, civil, expired_hours, hour_path,
    over_budget_hours, universe_file_name, universe_path,
};
use scanner::engine::{EngineCtx, Out};
use scanner::paper::push_sim;
use scanner::replay::{replay_dir, replay_dir_paper_with, replay_dir_with};
use scanner::universe::{ConnMap, GroupMeta, LegMeta, Status, Universe};
use sim::config::SimConfig;
use sim::trader::PaperTrader;
use std::path::Path;
use std::path::PathBuf;
use store::Batch;

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
        issue_throttle_ms: 5_000,
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
            tags: None,
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
    let ufile = universe_file_name(u.taken_ms, 1);
    let msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
            file: ufile.clone(),
        })),
        line(
            t0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
                file: ufile.clone(),
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

#[test]
fn replay_dir_with_streams_the_same_outputs_as_replay_dir() {
    let dir = tmp("replay-stream");
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let mut w = Writer::new(dir.clone());
    let t0 = 1_790_121_600_000_i64;
    let ufile = universe_file_name(u.taken_ms, 1);
    let msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
            file: ufile.clone(),
        })),
        line(
            t0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
                file: ufile.clone(),
            }),
        ),
        line(
            t0 + 100,
            Some(book_frame(t0, ["0.40", "0.35", "0.30"])),
            None,
        ),
        line(
            t0 + 1_100,
            Some(book_frame(t0 + 1_000, ["0.30", "0.35", "0.30"])),
            None,
        ),
        line(t0 + 1_500, None, Some(Ctl::Down)),
        line(
            t0 + 2_100,
            Some(book_frame(t0 + 2_000, ["0.30", "0.35", "0.30"])),
            None,
        ),
        line(t0 + 2_200, Some("{broken".into()), None),
        line(t0 + 3_000, None, Some(Ctl::Shutdown)),
    ];
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();

    let collected = replay_dir(&dir, ctx()).unwrap();
    let mut streamed_outs: Vec<Out> = Vec::new();
    let stats = replay_dir_with(&dir, ctx(), |o| streamed_outs.push(o)).unwrap();

    let collected_dbg: Vec<String> = collected.outs.iter().map(|o| format!("{o:?}")).collect();
    let streamed_dbg: Vec<String> = streamed_outs.iter().map(|o| format!("{o:?}")).collect();
    assert_eq!(collected_dbg, streamed_dbg);
    assert_eq!(
        (stats.frames, stats.parse_errors),
        (collected.frames, collected.parse_errors)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn capture_budget_deletes_the_oldest_hours_but_never_the_open_one() {
    let dir = tmp("budget");
    let t = 1_790_121_600_000_i64;
    let paths: Vec<_> = (0..4)
        .map(|h| {
            let p = hour_path(&dir, t + h * 3_600_000);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            p
        })
        .collect();
    std::fs::write(
        dir.join("2026-09-23").join("universe-1.json"),
        vec![0u8; 1000],
    )
    .unwrap(); // an old-style name (no seq/.zst): matches neither an hour file nor a universe record
    // 400 bytes of hour files, budget 250: delete the two oldest
    assert_eq!(
        over_budget_hours(&dir, 250, None),
        vec![paths[0].clone(), paths[1].clone()]
    );
    // the open file is skipped even if it is the oldest
    assert_eq!(
        over_budget_hours(&dir, 250, Some(&paths[0])),
        vec![paths[1].clone(), paths[2].clone()]
    );
    assert!(over_budget_hours(&dir, 1_000, None).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_universe_record_counts_toward_the_budget_but_is_never_returned_by_it() {
    let dir = tmp("budget-universe");
    let t = 1_790_121_600_000_i64;
    let hour = hour_path(&dir, t);
    std::fs::create_dir_all(hour.parent().unwrap()).unwrap();
    std::fs::write(&hour, vec![0u8; 100]).unwrap();
    // 300 bytes of universe records, matching the real naming: not returned for deletion here,
    // but counted, so the (otherwise under-budget) hour file above is now evicted too.
    let up = universe_path(&dir, t, &universe_file_name(t, 1));
    std::fs::create_dir_all(up.parent().unwrap()).unwrap();
    std::fs::write(&up, vec![0u8; 300]).unwrap();
    assert!(
        over_budget_hours(&dir, 1_000, None).is_empty(),
        "under budget without the universe file"
    );
    assert_eq!(
        over_budget_hours(&dir, 350, None),
        vec![hour],
        "the universe bytes push the total over budget, but only the hour file is evicted"
    );
    std::fs::remove_dir_all(&dir).ok();
}

const POLICIES: &str = include_str!("../../../config/policies.toml");
const T0: i64 = 1_790_121_600_000; // a whole minute: the first mark boundary is T0 + 60 000

/// Book snapshots for y0..y2 with one bid and one ask level each, size 100.
fn paper_frame(ts: i64, bids: [&str; 3], asks: [&str; 3]) -> String {
    let items: Vec<String> = (0..3)
        .map(|i| {
            format!(
                r#"{{"event_type":"book","asset_id":"y{i}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{{"price":"{}","size":"100"}}],"asks":[{{"price":"{}","size":"100"}}]}}"#,
                bids[i], asks[i]
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// A NO basket opens at T0 + 1 100, every scenario executes by T0 + 2 000 and converts by
/// T0 + 4 000; m0 resolves 50-50 at T0 + 5 000; the capture ends with a Shutdown line at
/// T0 + 6 000, or (a crash) without one.
fn write_paper_capture(dir: &Path, clean_stop: bool) {
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let ufile = universe_file_name(u.taken_ms, 1);
    let asks = ["0.45", "0.40", "0.35"]; // ΣA = 1.20: never a YES basket
    let open = ["0.40", "0.35", "0.30"]; // NO asks 0.60/0.65/0.70: Σ 1.95 < 2
    let mut msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
            file: ufile.clone(),
        })),
        line(
            T0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
                file: ufile,
            }),
        ),
        line(
            T0 + 100,
            Some(paper_frame(T0, ["0.30", "0.30", "0.30"], asks)),
            None,
        ), // ΣNO 2.10: nothing
        line(T0 + 1_100, Some(paper_frame(T0 + 1_000, open, asks)), None), // the NO basket opens
        line(T0 + 2_000, Some(paper_frame(T0 + 1_900, open, asks)), None), // every scenario's t_x has passed
        line(T0 + 4_000, Some(paper_frame(T0 + 3_900, open, asks)), None), // conversions due (+2 s)
        line(
            T0 + 5_000,
            None,
            Some(Ctl::Resolved {
                market_id: "m0".into(),
                payout_yes: "0.5".into(),
                payout_no: "0.5".into(),
            }),
        ),
    ];
    if clean_stop {
        msgs.push(line(T0 + 6_000, None, Some(Ctl::Shutdown)));
    }
    let mut w = Writer::new(dir.to_path_buf());
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

fn replay_paper(dir: &Path, policies: &SimConfig) -> Vec<Out> {
    let mut outs = Vec::new();
    replay_dir_paper_with(
        dir,
        ctx(),
        Some(PaperTrader::new(policies.clone()).unwrap()),
        |o| outs.push(o),
    )
    .unwrap();
    outs
}

fn paper_rows(outs: &[Out]) -> Batch {
    let mut b = Batch::default();
    for o in outs {
        if let Out::Paper(s) = o {
            push_sim(1, (**s).clone(), &mut b);
        }
    }
    b
}

#[test]
fn replay_paper_trades_deterministically_and_keeps_the_equity_identity() {
    let dir = tmp("paper");
    write_paper_capture(&dir, true);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let (a, b) = (replay_paper(&dir, &policies), replay_paper(&dir, &policies));
    assert_eq!(a, b, "same capture, same outputs, paper rows included");
    let (ra, rb) = (paper_rows(&a), paper_rows(&b));
    assert_eq!(ra, rb, "same paper ledger rows");
    assert!(!ra.paper_ledger.is_empty());

    assert!(
        a.iter()
            .any(|o| matches!(o, Out::Episode(w) if w.row.kind == "no_basket"))
    );
    assert!(a.iter().any(
        |o| matches!(o, Out::Integrity(i) if i.kind == "non_binary_payout" && i.ts_ms == T0 + 5_000)
    ));
    let p0 = "P0-converter/colo-idle";
    assert!(
        ra.paper_execs
            .iter()
            .any(|x| x.account_id == p0 && x.q_eff == dec!(51.2))
    );
    assert!(
        ra.paper_ledger
            .iter()
            .any(|l| l.account_id == p0 && l.kind == "convert")
    );

    // Ctl::Shutdown closes the trader: one final mark per account, and the identity holds on each
    let finals: Vec<_> = ra
        .paper_marks
        .iter()
        .filter(|m| m.t_ms == T0 + 6_000)
        .collect();
    assert_eq!(finals.len(), policies.accounts.len());
    for m in &finals {
        assert_eq!(
            m.equity,
            dec!(1000) + m.realized + m.unrealized,
            "{}",
            m.account_id
        );
    }
    let p0_final = finals.iter().find(|m| m.account_id == p0).unwrap();
    assert_eq!(p0_final.realized, dec!(2.56), "51.2 sets × (2 − 1.95)");

    // the bare replay (no trader) emits exactly the scanner outputs of the paper replay
    let mut bare = Vec::new();
    replay_dir_with(&dir, ctx(), |o| bare.push(o)).unwrap();
    let scanner_only: Vec<Out> = a
        .iter()
        .filter(|o| !matches!(o, Out::Paper(_)))
        .cloned()
        .collect();
    assert_eq!(scanner_only, bare);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_capture_without_a_shutdown_line_closes_the_trader_at_its_last_instant() {
    let dir = tmp("paper-crash");
    write_paper_capture(&dir, false);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let rows = paper_rows(&replay_paper(&dir, &policies));
    // the last captured instant is the Resolved line (T0 + 5 000): every account marks there
    let finals = rows
        .paper_marks
        .iter()
        .filter(|m| m.t_ms == T0 + 5_000)
        .count();
    assert!(
        finals >= policies.accounts.len(),
        "{finals} marks at the crash close"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_resolved_line_round_trips_through_the_capture_format() {
    let l = Line {
        recv_ts_us: 5,
        shard: 0,
        raw: None,
        ctl: Some(Ctl::Resolved {
            market_id: "m0".into(),
            payout_yes: "0.5".into(),
            payout_no: "0.5".into(),
        }),
    };
    let json = serde_json::to_string(&l).unwrap();
    assert!(json.contains(r#""kind":"resolved""#), "{json}");
    assert_eq!(serde_json::from_str::<Line>(&json).unwrap(), l);
}

/// `write_paper_capture`'s NO basket (frames to T0 + 4 000), then a paper reset whose payload
/// `t` (T0 + 4 500) is 100 ms before its own line, then a clean stop at T0 + 6 000.
fn write_reset_capture(dir: &Path) {
    let u = universe();
    let conn_of: ConnMap = [("e1".to_string(), 0)].into_iter().collect();
    let ufile = universe_file_name(u.taken_ms, 1);
    let asks = ["0.45", "0.40", "0.35"];
    let open = ["0.40", "0.35", "0.30"];
    let msgs = vec![
        CaptureMsg::Universe(Box::new(UniverseRecord {
            universe: u.clone(),
            conn_of,
            file: ufile.clone(),
        })),
        line(
            T0,
            None,
            Some(Ctl::Universe {
                taken_ms: u.taken_ms,
                file: ufile,
            }),
        ),
        line(
            T0 + 100,
            Some(paper_frame(T0, ["0.30", "0.30", "0.30"], asks)),
            None,
        ),
        line(T0 + 1_100, Some(paper_frame(T0 + 1_000, open, asks)), None),
        line(T0 + 2_000, Some(paper_frame(T0 + 1_900, open, asks)), None),
        line(T0 + 4_000, Some(paper_frame(T0 + 3_900, open, asks)), None),
        line(T0 + 4_600, None, Some(Ctl::PaperReset { t: T0 + 4_500 })),
        line(T0 + 6_000, None, Some(Ctl::Shutdown)),
    ];
    let mut w = Writer::new(dir.to_path_buf());
    for m in &msgs {
        w.write(m).unwrap();
    }
    w.finish();
}

#[test]
fn replay_resets_at_the_payload_instant_and_keeps_every_seq_rising() {
    let dir = tmp("paper-reset");
    write_reset_capture(&dir);
    let policies: SimConfig = toml::from_str(POLICIES).unwrap();
    let (a, b) = (replay_paper(&dir, &policies), replay_paper(&dir, &policies));
    assert_eq!(a, b, "deterministic across the reset");
    let rows = paper_rows(&a);
    let n = policies.accounts.len();
    let at = |t: i64| rows.paper_marks.iter().filter(|m| m.t_ms == t).count();
    assert_eq!(
        (at(T0 + 4_500), at(T0 + 4_501), at(T0 + 4_600)),
        (n, n, 0),
        "the close at the payload t, the new epoch at t + 1, nothing at the line's own time"
    );
    assert!(
        rows.paper_marks
            .iter()
            .filter(|m| m.t_ms == T0 + 4_501)
            .all(|m| m.equity == dec!(1000))
    );
    let mut by: std::collections::BTreeMap<&str, Vec<i64>> = Default::default();
    for l in &rows.paper_ledger {
        by.entry(l.account_id.as_str()).or_default().push(l.seq);
    }
    for (acct, seqs) in by {
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{acct}: {seqs:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
