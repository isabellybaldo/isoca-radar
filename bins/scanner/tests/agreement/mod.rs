#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
//! Spec 5b §8 "agreement with the scanner": a hand-built capture hour, and the driver that replays
//! it through the engine (what `scanner replay` reconstructs) and tape::feed::FeedBooks side by side.
use arb_core::book::{Book, Top};
use arb_core::money::{Price, Shares};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use scanner::capture::{CaptureMsg, Ctl, Line, UniverseRecord, Writer, universe_file_name};
use scanner::engine::{Engine, EngineCtx};
use scanner::replay::{ReplayState, apply_line};
use scanner::universe::{CanaryPair, ConnMap, GroupMeta, LegMeta, Status, Universe};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tape::feed::FeedBooks;
use tape::universe_view::load_record;

pub const T0: i64 = 1_790_121_600_000;
pub const TOKENS: [&str; 11] = [
    "a1", "a2", "a3", "b1", "b2", "cy", "cn", "d1", "d2", "c8y", "c8n",
];

pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tape-agree-{name}-{}-{}",
        std::process::id(),
        scanner::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn ctx() -> EngineCtx {
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

fn leg(t: &str, status: Status) -> LegMeta {
    LegMeta {
        market_id: format!("m-{t}"),
        yes_token: t.into(),
        no_token: format!("n-{t}"),
        status,
        fee_rate: Decimal::ZERO,
        fee_exponent: 1,
        tick: Some(dec!(0.01)),
        min_size: Some(dec!(5)),
        seconds_delay: Some(0),
        end_date: None,
    }
}

fn group(e: &str, legs: Vec<LegMeta>) -> GroupMeta {
    GroupMeta {
        event_id: e.into(),
        slug: None,
        title: None,
        neg_risk_market_id: Some(format!("nr-{e}")),
        augmented: false,
        legs,
        tags: None,
    }
}

fn canary(m: &str, e: &str, y: &str, n: &str) -> CanaryPair {
    CanaryPair {
        market_id: m.into(),
        event_id: e.into(),
        yes_token: y.into(),
        no_token: n.into(),
    }
}

/// U1: e1 (a1, a2, a3 halted), e2 (b1, b2), canary m9.
fn universe1() -> Universe {
    Universe {
        taken_ms: T0,
        groups: vec![
            group(
                "e1",
                vec![
                    leg("a1", Status::Live),
                    leg("a2", Status::Live),
                    leg("a3", Status::Halted),
                ],
            ),
            group("e2", vec![leg("b1", Status::Live), leg("b2", Status::Live)]),
        ],
        canary: vec![canary("m9", "e9", "cy", "cn")],
    }
}

/// U2: e1 loses its connection and a2 (a3 goes live); a2 moves to e2; b2 halts; a new group e3
/// (d1, d2, and b1 again: a YES token live in two groups, which the universe builder does not
/// dedupe) and a new canary pair m8 appear. b1's carried book goes to its first listing (e2); the
/// visible book is the last live listing's (e3), empty. Groups stay sorted by event id (the engine
/// searches them).
fn universe2() -> Universe {
    Universe {
        taken_ms: T0 + 600_000,
        groups: vec![
            group("e1", vec![leg("a1", Status::Live), leg("a3", Status::Live)]),
            group(
                "e2",
                vec![
                    leg("b1", Status::Live),
                    leg("b2", Status::Halted),
                    leg("a2", Status::Live),
                ],
            ),
            group(
                "e3",
                vec![
                    leg("d1", Status::Live),
                    leg("d2", Status::Live),
                    leg("b1", Status::Live),
                ],
            ),
        ],
        canary: vec![
            canary("m9", "e9", "cy", "cn"),
            canary("m8", "e8", "c8y", "c8n"),
        ],
    }
}

/// U3: U2 with b2 live again (its halted book carried back; the deferred check it had before U2
/// must not come back with it) and b1 halted in e3: b1's last old listing (e3, live, the book the
/// deltas since U2 built) is carried to its first new listing (e2), which is visible again. When the
/// restarted run re-applies U3, b1's last old listing is e3's HALTED leg (empty book): the old
/// books offer every listing whatever its status, so e2's b1 is carried that empty book.
fn universe3() -> Universe {
    let mut u = universe2();
    u.taken_ms = T0 + 602_000;
    u.groups[1].legs[1].status = Status::Live;
    u.groups[2].legs[2].status = Status::Halted;
    u
}

fn line(t: i64, shard: usize, raw: String) -> CaptureMsg {
    CaptureMsg::Line(Line {
        recv_ts_us: t * 1000,
        shard,
        raw: Some(raw),
        ctl: None,
    })
}

fn ctl(t: i64, shard: usize, c: Ctl) -> CaptureMsg {
    CaptureMsg::Line(Line {
        recv_ts_us: t * 1000,
        shard,
        raw: None,
        ctl: Some(c),
    })
}

type Side<'a> = &'a [(&'a str, &'a str)];

fn books(items: &[(&str, i64, Side, Side)]) -> String {
    let lv = |v: Side| {
        v.iter()
            .map(|(p, s)| format!(r#"{{"price":"{p}","size":"{s}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    let one: Vec<String> = items
        .iter()
        .map(|(t, ts, b, a)| format!(r#"{{"event_type":"book","asset_id":"{t}","market":"0x","timestamp":"{ts}","hash":"h","bids":[{}],"asks":[{}]}}"#, lv(b), lv(a)))
        .collect();
    format!("[{}]", one.join(","))
}

/// One price_change entry: (token, BUY|SELL, price, size, best_bid, best_ask).
type Delta<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
);

/// One price_change message of `items`.
fn change(ts: i64, items: &[Delta]) -> String {
    let one: Vec<String> = items
        .iter()
        .map(|(t, side, p, s, bb, ba)| {
            let mut x = format!(r#"{{"asset_id":"{t}","side":"{side}","price":"{p}","size":"{s}""#);
            if let Some(b) = bb {
                x.push_str(&format!(r#","best_bid":"{b}""#));
            }
            if let Some(a) = ba {
                x.push_str(&format!(r#","best_ask":"{a}""#));
            }
            x.push('}');
            x
        })
        .collect();
    format!(
        r#"{{"event_type":"price_change","market":"0x","timestamp":"{ts}","price_changes":[{}]}}"#,
        one.join(",")
    )
}

fn record(u: &Universe, conn: &[(&str, usize)], seq: u64) -> UniverseRecord {
    UniverseRecord {
        universe: u.clone(),
        conn_of: conn
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<ConnMap>(),
        file: universe_file_name(u.taken_ms, seq),
    }
}

/// Write the hour (and its three universe records) into `dir`.
pub fn fixture(dir: &Path) {
    let r1 = record(&universe1(), &[("e1", 0), ("e2", 1), ("canary:m9", 1)], 1);
    let r2 = record(
        &universe2(),
        &[("e2", 1), ("e3", 2), ("canary:m9", 1), ("canary:m8", 2)],
        2,
    );
    let r3 = record(
        &universe3(),
        &[("e2", 1), ("e3", 2), ("canary:m9", 1), ("canary:m8", 2)],
        3,
    );
    let mut m = vec![
        CaptureMsg::Universe(Box::new(r1.clone())),
        ctl(
            T0 + 1_000,
            0,
            Ctl::Universe {
                taken_ms: T0,
                file: r1.file.clone(),
            },
        ),
        line(
            T0 + 2_000,
            0,
            books(&[
                (
                    "a1",
                    T0 + 1_990,
                    &[("0.40", "10"), ("0.39", "5")],
                    &[("0.45", "10")],
                ),
                ("a2", T0 + 1_990, &[("0.20", "3")], &[("0.30", "7")]),
            ]),
        ),
        line(
            T0 + 2_100,
            1,
            books(&[
                ("b1", T0 + 2_090, &[("0.50", "1")], &[("0.52", "2")]),
                ("b2", T0 + 2_090, &[], &[("0.60", "4")]),
                ("cy", T0 + 2_090, &[("0.10", "9")], &[("0.12", "9")]),
                ("cn", T0 + 2_090, &[("0.88", "9")], &[("0.90", "9")]),
            ]),
        ),
        line(
            T0 + 3_000,
            0,
            change(
                T0 + 2_995,
                &[("a1", "BUY", "0.41", "2", Some("0.41"), Some("0.45"))],
            ),
        ),
        line(
            T0 + 3_100,
            0,
            change(
                T0 + 3_095,
                &[("a1", "SELL", "0.44", "3", Some("0.41"), Some("0.46"))],
            ),
        ),
        line(
            T0 + 3_101,
            0,
            change(
                T0 + 3_095,
                &[("a1", "SELL", "0.46", "1", Some("0.41"), Some("0.44"))],
            ),
        ),
        line(
            T0 + 5_000,
            0,
            change(
                T0 + 4_995,
                &[("a2", "BUY", "0.21", "1", Some("0.21"), Some("0.30"))],
            ),
        ),
        line(
            T0 + 5_100,
            1,
            format!(
                r#"{{"event_type":"last_trade_price","asset_id":"zz","timestamp":"{}"}}"#,
                T0 + 9_000
            ),
        ),
        line(
            T0 + 5_200,
            1,
            change(
                T0 + 5_195,
                &[("b1", "SELL", "0.51", "1", Some("0.50"), Some("0.55"))],
            ),
        ),
        ctl(
            T0 + 5_300,
            0,
            Ctl::RestSnapshot {
                token: "b1".into(),
                server_ts_ms: T0 + 100,
                bids: vec![],
                asks: vec![("0.70".into(), "1".into())],
            },
        ),
        ctl(
            T0 + 5_400,
            0,
            Ctl::RestSnapshot {
                token: "cy".into(),
                server_ts_ms: T0 + 5_390,
                bids: vec![("0.11".into(), "1".into())],
                asks: vec![],
            },
        ),
        ctl(T0 + 6_000, 1, Ctl::Down),
        ctl(
            T0 + 7_000,
            0,
            Ctl::Suspect {
                event_id: "e2".into(),
                until_ms: T0 + 67_000,
            },
        ),
        ctl(
            T0 + 8_000,
            0,
            Ctl::Resolved {
                market_id: "m-b2".into(),
                payout_yes: "1".into(),
                payout_no: "0".into(),
            },
        ),
        // a1: a wrong server top left pending, then a ws snapshot supersedes it (not trimmed when
        // the next shard-0 frame settles checks)
        line(
            T0 + 8_100,
            0,
            change(
                T0 + 8_095,
                &[("a1", "SELL", "0.47", "1", Some("0.41"), Some("0.60"))],
            ),
        ),
        line(
            T0 + 8_200,
            0,
            books(&[("a1", T0 + 8_195, &[("0.41", "5")], &[("0.45", "5")])]),
        ),
        line(
            T0 + 9_000,
            1,
            change(
                T0 + 8_995,
                &[("b1", "BUY", "0.49", "2", Some("0.50"), None)],
            ),
        ),
        // b1: an accepted REST snapshot supersedes the pending (0.50) check its bid 0.55 would fail
        ctl(
            T0 + 9_100,
            0,
            Ctl::RestSnapshot {
                token: "b1".into(),
                server_ts_ms: T0 + 9_050,
                bids: vec![("0.55".into(), "2".into())],
                asks: vec![("0.60".into(), "1".into())],
            },
        ),
        line(
            T0 + 9_500,
            0,
            change(
                T0 + 9_495,
                &[("a2", "BUY", "0.19", "1", Some("0.21"), Some("0.30"))],
            ),
        ),
        // b2: a wrong server top (bid 0.50 under our 0.55) still pending when U2 halts b2
        line(
            T0 + 10_500,
            1,
            change(
                T0 + 10_495,
                &[("b2", "BUY", "0.55", "1", Some("0.50"), Some("0.60"))],
            ),
        ),
        CaptureMsg::Universe(Box::new(r2.clone())),
        ctl(
            T0 + 600_000,
            0,
            Ctl::Universe {
                taken_ms: T0 + 600_000,
                file: r2.file.clone(),
            },
        ),
        line(
            T0 + 601_000,
            0,
            change(
                T0 + 600_995,
                &[
                    ("a1", "BUY", "0.42", "1", Some("0.42"), Some("0.47")),
                    ("a3", "SELL", "0.80", "2", Some("0"), Some("0.80")),
                ],
            ),
        ),
        // b1 is now e3's (shard 2) visible leg, starting empty
        line(
            T0 + 600_500,
            2,
            books(&[("b1", T0 + 600_495, &[("0.47", "3")], &[("0.53", "3")])]),
        ),
        line(
            T0 + 601_500,
            2,
            change(
                T0 + 601_495,
                &[("b1", "BUY", "0.48", "1", Some("0.48"), Some("0.53"))],
            ),
        ),
        CaptureMsg::Universe(Box::new(r3.clone())),
        ctl(
            T0 + 602_000,
            0,
            Ctl::Universe {
                taken_ms: T0 + 602_000,
                file: r3.file.clone(),
            },
        ),
        line(
            T0 + 603_000,
            0,
            change(
                T0 + 602_995,
                &[("a1", "BUY", "0.43", "1", Some("0.43"), Some("0.48"))],
            ),
        ),
        line(
            T0 + 604_000,
            2,
            books(&[
                ("d1", T0 + 603_990, &[("0.33", "4")], &[("0.35", "4")]),
                ("d2", T0 + 603_990, &[("0.60", "1")], &[]),
                ("c8y", T0 + 603_990, &[("0.05", "2")], &[("0.07", "2")]),
                ("c8n", T0 + 603_990, &[("0.93", "2")], &[("0.95", "2")]),
            ]),
        ),
        line(
            T0 + 605_000,
            1,
            change(
                T0 + 604_995,
                &[("a2", "BUY", "0.22", "1", Some("0.22"), Some("0.31"))],
            ),
        ), // a2 now in e2, book carried
        line(
            T0 + 606_000,
            2,
            change(
                T0 + 605_995,
                &[("d1", "SELL", "0.34", "1", Some("0.33"), Some("0.36"))],
            ),
        ),
    ];
    // 1,500 random deltas: an LCG picks token, side, price, size and whether the server top agrees.
    let mut x: u64 = 0x5eed;
    let mut next = |n: u64| {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (x >> 33) % n
    };
    let pool = [
        ("a1", 0usize),
        ("a2", 1),
        ("b1", 1),
        ("cy", 1),
        ("cn", 1),
        ("d1", 2),
        ("c8y", 2),
    ];
    let mut t = T0 + 700_000;
    let mut ts = t;
    for _ in 0..1_500 {
        t += i64::try_from(next(1_500)).unwrap();
        if next(4) != 0 {
            ts = t - 5; // else: same server ts as the previous message (a split update)
        }
        let (tok, conn) = pool[usize::try_from(next(7)).unwrap()];
        let side = if next(2) == 0 { "BUY" } else { "SELL" };
        let price = format!("0.{:02}", 1 + next(98));
        let size = next(40).to_string();
        let bb = match next(3) {
            0 => None,
            1 => Some("0".to_string()),
            _ => Some(format!("0.{:02}", 1 + next(98))),
        };
        let ba = match next(3) {
            0 => None,
            1 => Some("1".to_string()),
            _ => Some(format!("0.{:02}", 1 + next(98))),
        };
        m.push(line(
            t,
            conn,
            change(
                ts,
                &[(tok, side, &price, &size, bb.as_deref(), ba.as_deref())],
            ),
        ));
    }
    // d2 (e3, shard 2; bid 0.60, no asks): a wrong server top (bid 0.50) still pending at Shutdown.
    // close_all drops it; the restarted run appends to the same hour file and replay keeps the same
    // engine, so re-applying U3 keeps only checks that survived, and the next shard-2 frame (d1,
    // more than 1 s later) would otherwise settle d2's and trim its 0.60 bid.
    m.push(line(
        t + 1,
        2,
        change(
            t - 4,
            &[("d2", "SELL", "0.70", "1", Some("0.50"), Some("0.70"))],
        ),
    ));
    m.push(ctl(t + 2, 0, Ctl::Shutdown));
    let r = t + 60_000;
    m.push(ctl(
        r,
        0,
        Ctl::Universe {
            taken_ms: T0 + 602_000,
            file: r3.file.clone(),
        },
    ));
    m.push(line(
        r + 2_000,
        2,
        change(r + 1_995, &[("d1", "BUY", "0.02", "1", None, None)]),
    ));
    m.push(line(
        r + 3_000,
        1,
        change(
            r + 2_995,
            &[("b1", "BUY", "0.30", "2", Some("0.30"), Some("1"))],
        ),
    ));
    m.push(ctl(r + 4_000, 0, Ctl::Shutdown));
    let mut w = Writer::new(dir.to_path_buf());
    for msg in &m {
        w.write(msg).unwrap();
    }
    w.finish();
}

/// A book's levels, best first on each side, and its server timestamp.
type Lv = (Vec<(Price, Shares)>, Vec<(Price, Shares)>, i64);

fn lv(b: Option<&Book>) -> Option<Lv> {
    b.map(|b| (b.bids_best_first(), b.asks_best_first(), b.server_ts_ms()))
}

/// Drive the engine and FeedBooks over every hour file of `dir`, comparing `tokens` after every
/// line (None: the tokens each line touched, then every visible token at the end), and the
/// top-of-book change streams. Returns the number of lines compared.
pub fn compare(dir: &Path, tokens: Option<&[&str]>) -> usize {
    let mut engine = Engine::new(ctx());
    let mut st = ReplayState::default();
    let mut feed = FeedBooks::new();
    let mut load = |taken: i64, file: &str| load_record(dir, taken, file);
    let mut engine_tops: BTreeMap<String, Vec<(usize, Top)>> = BTreeMap::new();
    let mut feed_tops: BTreeMap<String, Vec<(usize, Top)>> = BTreeMap::new();
    let mut n = 0usize;
    for h in tape::files::listing(dir).hours {
        tape::files::for_each_line(&h.path, |l| {
            // a control line that can move books without touching them (a universe change, a
            // disconnect, a shutdown, a REST snapshot): check every token visible before or after
            let wide = tokens.is_none()
                && matches!(
                    l.ctl,
                    Some(
                        Ctl::Universe { .. } | Ctl::Down | Ctl::Shutdown | Ctl::RestSnapshot { .. }
                    )
                );
            let was = if wide { feed.visible_tokens() } else { vec![] };
            apply_line(&mut engine, dir, &l, &mut st, &mut |_| {})?;
            let ap = feed.apply(&l, &mut load, &|_| false);
            let check: Vec<String> = match tokens {
                Some(t) => t.iter().map(|s| s.to_string()).collect(),
                None => {
                    let mut c = ap.touched.clone();
                    if wide {
                        c.extend(was);
                        c.extend(feed.visible_tokens());
                        c.sort();
                        c.dedup();
                    }
                    c
                }
            };
            for t in &check {
                assert_eq!(
                    lv(engine.book_of(t)),
                    lv(feed.book(t)),
                    "line {n}: books differ for {t}"
                );
                for (tops, top) in [
                    (&mut engine_tops, engine.book_of(t).map(|b| b.top())),
                    (&mut feed_tops, feed.book(t).map(|b| b.top())),
                ] {
                    let s = tops.entry(t.clone()).or_default();
                    if let Some(top) = top
                        && s.last().map(|(_, x)| *x) != Some(top)
                    {
                        s.push((n, top));
                    }
                }
            }
            n += 1;
            Ok(())
        })
        .unwrap();
    }
    for t in feed.visible_tokens() {
        assert_eq!(
            lv(engine.book_of(&t)),
            lv(feed.book(&t)),
            "end: books differ for {t}"
        );
    }
    assert_eq!(engine_tops, feed_tops, "top-of-book change streams differ");
    n
}
