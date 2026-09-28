#![allow(clippy::unwrap_used, clippy::expect_used)]
use compact::pq::*;
use compact::rows::*;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "compact-pq-{name}-{}-{}",
        std::process::id(),
        compact::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write<R: Table + Clone>(p: PathBuf, rows: &[R]) -> Staged {
    let mut w = PartWriter::<R>::create(p).unwrap();
    for r in rows {
        w.push(r.clone()).unwrap();
    }
    w.close().unwrap()
}

#[test]
fn every_table_round_trips_with_nulls_and_is_invisible_until_published() {
    let d = tmp("rt");
    let tob = vec![
        TobRow {
            ts_ms: 1,
            recv_ms: 2,
            token_idx: 0,
            bid: Some(400_000),
            bid_sz: Some(1),
            ask: None,
            ask_sz: None,
        },
        TobRow {
            ts_ms: 3,
            recv_ms: 4,
            token_idx: 7,
            bid: None,
            bid_sz: None,
            ask: Some(420_000),
            ask_sz: Some(9),
        },
    ];
    let s = write(d.join("00.tob.parquet"), &tob);
    assert!(
        s.tmp.exists() && !s.fin.exists(),
        "written under a temporary name"
    );
    s.publish().unwrap();
    assert!(!s.tmp.exists() && s.fin.exists());
    assert_eq!(read_all::<TobRow>(&s.fin).unwrap(), tob);
    let depth = vec![DepthRow {
        ts_ms: 5,
        token_idx: 1,
        side: SIDE_ASK,
        level: 9,
        price: 1,
        size: 2,
    }];
    let l2 = vec![L2Row {
        ts_ms: 6,
        recv_ms: 7,
        token_idx: 2,
        side: SIDE_BID,
        price: 3,
        new_size: 0,
        snapshot: true,
        episode_id: "live-1-01".into(),
    }];
    let gaps = vec![
        GapRow {
            start_ms: 1,
            end_ms: 2,
            shard: None,
            reason: reason::RAW_HOUR_LOST.into(),
        },
        GapRow {
            start_ms: 3,
            end_ms: 4,
            shard: Some(12),
            reason: reason::DOWN.into(),
        },
    ];
    let tokens = vec![TokenRow {
        token_idx: 0,
        token_id: "123".into(),
        market_id: "m".into(),
        event_id: "e".into(),
        outcome: "yes".into(),
        neg_risk: true,
    }];
    for (s, n) in [
        (write(d.join("depth.parquet"), &depth), 1),
        (write(d.join("l2.parquet"), &l2), 1),
        (write(d.join("gaps.parquet"), &gaps), 2),
        (write(d.join("tokens.parquet"), &tokens), 1),
    ] {
        assert_eq!(s.rows, n);
        s.publish().unwrap();
        assert_eq!(num_rows(&s.fin).unwrap(), n);
    }
    assert_eq!(
        read_all::<DepthRow>(&d.join("depth.parquet")).unwrap(),
        depth
    );
    assert_eq!(read_all::<L2Row>(&d.join("l2.parquet")).unwrap(), l2);
    assert_eq!(read_all::<GapRow>(&d.join("gaps.parquet")).unwrap(), gaps);
    assert_eq!(
        read_all::<TokenRow>(&d.join("tokens.parquet")).unwrap(),
        tokens
    );
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn row_groups_hold_262144_rows_so_memory_stays_bounded() {
    let d = tmp("rg");
    let rows: Vec<TobRow> = (0..600_000)
        .map(|i| TobRow {
            ts_ms: i,
            recv_ms: i,
            token_idx: u32::try_from(i % 4_000).unwrap(),
            bid: Some(i),
            bid_sz: Some(1),
            ask: None,
            ask_sz: None,
        })
        .collect();
    let s = write(d.join("tob.parquet"), &rows);
    s.publish().unwrap();
    assert_eq!(ROW_GROUP_ROWS, 262_144);
    assert_eq!(
        num_row_groups(&s.fin).unwrap(),
        3,
        "600,000 rows = 262,144 + 262,144 + 75,712"
    );
    assert_eq!(num_rows(&s.fin).unwrap(), 600_000);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn zero_pushed_rows_still_closes_and_publishes_a_valid_empty_file() {
    let d = tmp("zero");
    let rows: Vec<TobRow> = Vec::new();
    let s = write(d.join("empty.parquet"), &rows);
    assert_eq!(s.rows, 0);
    s.publish().unwrap();
    assert_eq!(num_rows(&s.fin).unwrap(), 0);
    assert_eq!(read_all::<TobRow>(&s.fin).unwrap(), Vec::<TobRow>::new());
    std::fs::remove_dir_all(&d).ok();
}
