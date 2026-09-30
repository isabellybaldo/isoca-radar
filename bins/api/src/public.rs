//! The public snapshot (7b spec §4–§5): the latency ladder and today's counts as one static file
//! that nginx serves at /math-data/public.json, so no public request ever reaches the api. Built
//! from the same load functions as GET /api/paper/ladder and GET /api/today, every
//! PUBLIC_EVERY_S seconds, by one task beside the server. Only typed, whitelisted fields ship: no
//! account, market, token or episode id and no host name.
use crate::dto::Dec;
use crate::episodes::TodayDto;
use crate::ladder::LadderDto;
use cards::edge::LADDER;
use cards::ladder::BreakEven;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Category buckets kept in `ladder.markets`, most executed first (§5.1).
pub const PUBLIC_MAX_CATEGORIES: usize = 20;
/// A bigger file is never written: the run is skipped with a log line (§5.1).
pub const PUBLIC_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
pub struct PublicSnapshot {
    pub v: u32,
    pub written_ms: i64,
    pub ladder: PublicLadder,
    pub today: PublicToday,
}

#[derive(Debug, Serialize)]
pub struct PublicLadder {
    pub epoch: i32,
    pub start_ms: Option<i64>,
    pub as_of_ms: Option<i64>,
    pub break_even: BreakEven,
    pub measured: Option<PublicMeasured>,
    pub markets_status: &'static str,
    pub rungs: Vec<PublicRung>,
    pub markets: Vec<PublicSweetRow>,
}

/// `MeasuredDto` without `host` (it names the VPS).
#[derive(Debug, Serialize)]
pub struct PublicMeasured {
    pub latency_ms: i64,
}

/// `RungDto` without `account_id`: every other field `LatencyLadder` reads.
#[derive(Debug, Serialize)]
pub struct PublicRung {
    pub scenario: String,
    pub latency_ms: Option<i64>,
    pub extra: bool,
    pub n_exec: i64,
    pub n_leg_risk: i64,
    pub n_missed: i64,
    pub gains: Dec,
    pub leg_risk: Dec,
    pub settle: Dec,
    pub unrealized: Dec,
    pub net: Dec,
}

/// One SWEET row as `lib/sweet.ts` (`grid`, `ladderGrid`, `glyphKey`, `cellDetail`, `interval`)
/// and `SweetLadderGrid` read it; every other SWEET field is dropped. Decimals stay strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSweetRow {
    pub side: String,
    pub scenario: String,
    pub fee_bips: i64,
    pub latency_ms: Option<i64>,
    pub dimension: String,
    pub bucket: String,
    pub label: String,
    pub n_exec: i64,
    pub clusters: i64,
    pub mean: Option<String>,
    pub lo: Option<String>,
    pub hi: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicToday {
    pub day_start_ms: i64,
    pub arb: i64,
    pub no: i64,
    pub yes: i64,
    pub edge_budget100: Option<Dec>,
    pub hourly: Vec<PublicHour>,
}

#[derive(Debug, Serialize)]
pub struct PublicHour {
    pub hour: i64,
    pub arb: i64,
}

/// Keep only what `LatencyLadder`'s "which markets" grid draws (`SweetLadderGrid` with side "no";
/// `ladderGrid`'s columns are the LADDER scenarios at fee 0), then the `all` rows and the `max`
/// category buckets with the most executed attempts (summed over the bucket's scenarios; ties by
/// name), in their original order. Other dimensions are dropped.
pub fn trim_markets(rows: Vec<PublicSweetRow>, max: usize) -> Vec<PublicSweetRow> {
    let rows: Vec<PublicSweetRow> = rows
        .into_iter()
        .filter(|r| r.side == "no" && r.fee_bips == 0 && LADDER.contains(&r.scenario.as_str()))
        .collect();
    let mut n_exec: BTreeMap<&str, i64> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.dimension == "category") {
        *n_exec.entry(r.bucket.as_str()).or_default() += r.n_exec;
    }
    let mut ranked: Vec<(&str, i64)> = n_exec.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let keep: BTreeSet<String> = ranked
        .into_iter()
        .take(max)
        .map(|(b, _)| b.to_string())
        .collect();
    rows.into_iter()
        .filter(|r| r.dimension == "all" || (r.dimension == "category" && keep.contains(&r.bucket)))
        .collect()
}

/// The snapshot from the dashboard's own DTOs: the dropped fields never enter it.
pub fn build(ladder: LadderDto, today: TodayDto, written_ms: i64) -> PublicSnapshot {
    let mut bad = 0usize;
    let rows: Vec<PublicSweetRow> = ladder
        .markets
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<PublicSweetRow>(v) {
            Ok(r) => Some(r),
            Err(_) => {
                bad += 1;
                None
            }
        })
        .collect();
    if bad > 0 {
        eprintln!("api: public snapshot: {bad} SWEET rows did not parse; left out");
    }
    PublicSnapshot {
        v: 1,
        written_ms,
        ladder: PublicLadder {
            epoch: ladder.epoch,
            start_ms: ladder.start_ms,
            as_of_ms: ladder.as_of_ms,
            break_even: ladder.break_even,
            measured: ladder.measured.map(|m| PublicMeasured {
                latency_ms: m.latency_ms,
            }),
            markets_status: ladder.markets_status,
            rungs: ladder
                .rungs
                .into_iter()
                .map(|r| PublicRung {
                    scenario: r.scenario,
                    latency_ms: r.latency_ms,
                    extra: r.extra,
                    n_exec: r.n_exec,
                    n_leg_risk: r.n_leg_risk,
                    n_missed: r.n_missed,
                    gains: r.gains,
                    leg_risk: r.leg_risk,
                    settle: r.settle,
                    unrealized: r.unrealized,
                    net: r.net,
                })
                .collect(),
            markets: trim_markets(rows, PUBLIC_MAX_CATEGORIES),
        },
        today: PublicToday {
            day_start_ms: today.day_start_ms,
            arb: today.arb,
            no: today.no,
            yes: today.yes,
            edge_budget100: today.edge_budget100,
            hourly: today
                .hourly
                .into_iter()
                .map(|h| PublicHour {
                    hour: h.hour,
                    arb: h.arb,
                })
                .collect(),
        },
    }
}

/// The file's bytes, or why the run is skipped (over `PUBLIC_MAX_BYTES`).
pub fn to_bytes(snap: &PublicSnapshot) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(snap).map_err(|e| format!("serialize: {e}"))?;
    if bytes.len() > PUBLIC_MAX_BYTES {
        return Err(format!("{} bytes, over 64 KiB", bytes.len()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episodes::HourDto;
    use crate::ladder::{MeasuredDto, RungDto};
    use cards::ladder::Direction;
    use rust_decimal::Decimal;
    use serde_json::{Value, json};

    /// Every key path of a JSON value; array elements share one `[]` path.
    fn paths(v: &Value, at: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if at.is_empty() {
                        k.clone()
                    } else {
                        format!("{at}.{k}")
                    };
                    out.insert(p.clone());
                    paths(x, &p, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    paths(x, &format!("{at}[]"), out);
                }
            }
            _ => {}
        }
    }

    /// A long decimal, so the size test is a worst case.
    fn dec(s: &str) -> Dec {
        Dec(Decimal::from_str_exact(s).unwrap())
    }

    fn rung(scenario: &str, extra: bool) -> RungDto {
        RungDto {
            account_id: format!("P0-converter/{scenario}"),
            scenario: scenario.to_string(),
            latency_ms: Some(300),
            extra,
            n_exec: 123_456,
            n_leg_risk: 12_345,
            n_missed: 1_234,
            gains: dec("-12345.678901234567"),
            leg_risk: dec("-12345.678901234567"),
            settle: dec("12345.678901234567"),
            unrealized: dec("-12345.678901234567"),
            net: dec("-12345.678901234567"),
        }
    }

    /// A SWEET body row with every field `cards::sweet::SweetRow` has, as the ladder passes it.
    fn sweet(side: &str, scenario: &str, dimension: &str, bucket: &str, n_exec: i64) -> Value {
        json!({
            "side": side, "scenario": scenario, "fee_bips": 0, "latency_ms": 300,
            "dimension": dimension, "bucket": bucket, "label": "inconclusive", "few_trades": false,
            "n_exec": n_exec, "n_filled": 1, "n_missed": 1, "clusters": 12_345, "n_attempts": 1,
            "skipped": {"consumed": 3}, "episodes": 1, "flagged": 0, "on_table": "1",
            "on_table_eligible": "1", "captured": "1", "captured_leftover0": "1",
            "mean_leftover0": "1", "mean": "-0.1234567890123", "lo": "-0.1234567890123",
            "hi": "0.1234567890123", "eff_n": "1", "cost": "1", "annualized": null,
            "annualized_lo": null, "annualized_upper": null, "leftover_unpriced": 0, "pooled": 0,
            "episode_id": "live-9-00000001", "market_id": "0xmarket"
        })
    }

    const SCENARIOS: [&str; 8] = [
        "rung-0",
        "rung-25",
        "colo-idle",
        "rung-100",
        "rung-200",
        "sp-idle",
        "sp-burst",
        "sp-idle-nodelay",
    ];

    fn full(markets: Vec<Value>) -> (LadderDto, TodayDto) {
        let ladder = LadderDto {
            epoch: 12,
            start_ms: Some(1_790_121_600_000),
            as_of_ms: Some(1_790_121_600_000),
            rungs: SCENARIOS
                .iter()
                .map(|s| rung(s, s.starts_with("sp-b")))
                .collect(),
            markets,
            measured: Some(MeasuredDto {
                latency_ms: 153,
                host: "vps-kvm2".to_string(),
            }),
            markets_status: "live",
            break_even: BreakEven::At {
                latency_ms: 140,
                direction: Direction::LosesBeyond,
                not_monotone: true,
            },
        };
        let today = TodayDto {
            day_start_ms: 1_790_121_600_000,
            arb: 123_456,
            no: 123_455,
            yes: 1,
            edge_budget100: Some(dec("12345.678901234567")),
            hourly: (0..24).map(|hour| HourDto { hour, arb: 99_999 }).collect(),
        };
        (ladder, today)
    }

    const WHITELIST: [&str; 48] = [
        "v",
        "written_ms",
        "ladder",
        "ladder.epoch",
        "ladder.start_ms",
        "ladder.as_of_ms",
        "ladder.break_even",
        "ladder.break_even.kind",
        "ladder.break_even.latency_ms",
        "ladder.break_even.direction",
        "ladder.break_even.not_monotone",
        "ladder.measured",
        "ladder.measured.latency_ms",
        "ladder.markets_status",
        "ladder.rungs",
        "ladder.rungs[].scenario",
        "ladder.rungs[].latency_ms",
        "ladder.rungs[].extra",
        "ladder.rungs[].n_exec",
        "ladder.rungs[].n_leg_risk",
        "ladder.rungs[].n_missed",
        "ladder.rungs[].gains",
        "ladder.rungs[].leg_risk",
        "ladder.rungs[].settle",
        "ladder.rungs[].unrealized",
        "ladder.rungs[].net",
        "ladder.markets",
        "ladder.markets[].side",
        "ladder.markets[].scenario",
        "ladder.markets[].fee_bips",
        "ladder.markets[].latency_ms",
        "ladder.markets[].dimension",
        "ladder.markets[].bucket",
        "ladder.markets[].label",
        "ladder.markets[].n_exec",
        "ladder.markets[].clusters",
        "ladder.markets[].mean",
        "ladder.markets[].lo",
        "ladder.markets[].hi",
        "today",
        "today.day_start_ms",
        "today.arb",
        "today.no",
        "today.yes",
        "today.edge_budget100",
        "today.hourly",
        "today.hourly[].hour",
        "today.hourly[].arb",
    ];

    #[test]
    fn every_key_path_is_on_the_whitelist_and_nothing_else_ships() {
        let (ladder, today) = full(vec![sweet("no", "rung-0", "category", "crypto", 5)]);
        let raw = String::from_utf8(to_bytes(&build(ladder, today, 7)).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let mut got = BTreeSet::new();
        paths(&v, "", &mut got);
        let want: BTreeSet<String> = WHITELIST.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(got, want, "a new field ships only once this list names it");
        let text = raw.as_str();
        for gone in [
            "account_id",
            "P0-converter",
            "host",
            "vps-kvm2",
            "episode_id",
            "market_id",
            "skipped",
        ] {
            assert!(!text.contains(gone), "{gone} shipped: {text}");
        }
        assert!(
            text.starts_with(r#"{"v":1,"written_ms":7,"ladder":{"epoch":12,"#),
            "the fixed field order: {text}"
        );
    }

    #[test]
    fn no_side_fee_0_six_ladder_scenarios_all_plus_20_categories_and_24_hours_fit_in_64_kib() {
        let mut rows = Vec::new();
        for side in ["no", "yes"] {
            for s in SCENARIOS {
                rows.push(sweet(side, s, "all", "all", 1));
                for c in 0..PUBLIC_MAX_CATEGORIES {
                    rows.push(sweet(side, s, "category", &format!("category-{c:02}"), 1));
                }
            }
        }
        let (ladder, today) = full(rows);
        let snap = build(ladder, today, 1_790_121_600_000);
        assert_eq!(
            snap.ladder.markets.len(),
            6 * 21,
            "what the grid draws: NO side, fee 0, the six LADDER scenarios"
        );
        let bytes = to_bytes(&snap).unwrap();
        println!("worst case: {} bytes", bytes.len());
        assert!(bytes.len() < PUBLIC_MAX_BYTES, "{} bytes", bytes.len());
    }

    #[test]
    fn markets_keep_all_rows_and_the_20_busiest_categories_and_drop_other_dimensions() {
        let mut rows = vec![
            sweet("no", "rung-0", "all", "all", 0),
            sweet("no", "rung-0", "days", "lt1", 999),
        ];
        for c in 0..25 {
            // category-24 is the busiest; category-00..04 are the five left out
            rows.push(sweet(
                "no",
                "rung-0",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
            rows.push(sweet(
                "no",
                "sp-idle",
                "category",
                &format!("category-{c:02}"),
                c,
            ));
        }
        let (ladder, today) = full(rows);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1 + 2 * 20);
        assert_eq!(
            (m[0].dimension.as_str(), m[0].bucket.as_str()),
            ("all", "all")
        );
        let buckets: BTreeSet<&str> = m.iter().skip(1).map(|r| r.bucket.as_str()).collect();
        assert!(!buckets.contains("category-04") && buckets.contains("category-05"));
        assert!(m.iter().all(|r| r.dimension != "days"));
    }

    #[test]
    fn a_file_over_64_kib_is_refused_not_written() {
        let (mut ladder, today) = full(Vec::new());
        ladder.rungs[0].scenario = "x".repeat(PUBLIC_MAX_BYTES);
        let e = to_bytes(&build(ladder, today, 1)).unwrap_err();
        assert!(e.ends_with("bytes, over 64 KiB"), "{e}");
    }

    #[test]
    fn a_sweet_row_that_does_not_parse_is_left_out() {
        let mut bad = sweet("no", "rung-0", "category", "sports", 5);
        bad["n_exec"] = json!("five");
        let mut no_label = sweet("no", "rung-0", "category", "politics", 5);
        no_label.as_object_mut().unwrap().remove("label");
        let (ladder, today) = full(vec![sweet("no", "rung-0", "all", "all", 5), bad, no_label]);
        let m = build(ladder, today, 1).ladder.markets;
        assert_eq!(m.len(), 1, "only the good row: {m:?}");
        assert_eq!(m[0].dimension, "all");
    }
}
