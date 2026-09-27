//! Edge capture (spec §6): the edge that was measurably on the table versus what each latency
//! scenario's paper account captured. Opportunities are measured live; the only hypothetical is
//! execution (first in line at the best moment).
use crate::body::{BodyCtx, Card, CardBody, DecayPoint, Measured, Parts, Series, make, row};
use crate::cells::{CellData, Ep, HourInput, Num, PerSide, opened_in, overlap};
use crate::stats::{self, CountMap, dec};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CAP_USD: i64 = 100;
pub const HEADLINE: &str = "sp-idle";
pub const THIN_EDGE_N: i64 = 10;
pub const ON_TABLE: &str = "edge on the table";
pub const UPPER_BOUND: &str = "upper bound: assumes first-in-line execution; CLOB queue ignored";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Biggest {
    #[serde(default)]
    pub usd: Num,
    #[serde(default)]
    pub episode_id: String,
}

/// The two Edge figures a window can't get by adding cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdgeMax {
    pub capital: Decimal,
    pub biggest: Option<(Decimal, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeCell {
    /// Σ pnl_net_budget100_usd of counted episodes opened in the hour: NO side (cashable before
    /// settlement) and YES side (locked until resolution).
    #[serde(default)]
    pub table: PerSide<Num>,
    #[serde(default)]
    pub n: PerSide<i64>,
    /// scenario → (paper marks, Σ equity change); a scenario without marks is absent (null).
    #[serde(default)]
    pub captured: BTreeMap<String, (i64, Num)>,
    /// Measured latency (ms) per health minute → count.
    #[serde(default)]
    pub lat: CountMap,
    /// The most capital needed at once in this hour (capped costs of overlapping episodes,
    /// including ones opened earlier). Not summed by `add`: see `EdgeMax`.
    #[serde(default)]
    pub max_capital: Num,
    #[serde(default)]
    pub biggest: Option<Biggest>,
    /// Net-positive and flagged episodes opened in the hour: kept off the table, counted (spec §5).
    #[serde(default)]
    pub flagged: PerSide<i64>,
}

/// Counted on the table: net-positive, not flagged, with a capped P&L. Returns that P&L.
fn counted(e: &Ep) -> Option<Decimal> {
    if e.positive() && !e.flagged() {
        e.budget100
    } else {
        None
    }
}

/// Larger first; ties to the smaller episode id.
fn better(a: &(Decimal, String), b: &(Decimal, String)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1 < b.1)
}

impl EdgeCell {
    fn apply(&mut self, o: &Self, s: i64) {
        if s > 0 {
            self.table.yes.0 += o.table.yes.0;
            self.table.no.0 += o.table.no.0;
        } else {
            self.table.yes.0 -= o.table.yes.0;
            self.table.no.0 -= o.table.no.0;
        }
        self.n.yes += s * o.n.yes;
        self.n.no += s * o.n.no;
        self.flagged.yes += s * o.flagged.yes;
        self.flagged.no += s * o.flagged.no;
        for (k, (n, v)) in &o.captured {
            let e = self
                .captured
                .entry(k.clone())
                .or_insert((0, Num::default()));
            e.0 += s * n;
            if s > 0 {
                e.1.0 += v.0;
            } else {
                e.1.0 -= v.0;
            }
            if e.0 == 0 {
                self.captured.remove(k);
            }
        }
        stats::add_counts(&mut self.lat, &o.lat, s);
    }
}

impl CellData for EdgeCell {
    const FAMILY: &'static str = "edge";
    const AREAS: &'static [&'static str] = &[
        "depth",
        "detector_no",
        "detector_yes",
        "episode_def",
        "fees",
        "host",
        "sim",
    ];
    const EDGE_INPUTS: bool = true;
    type Max = EdgeMax;

    fn build(inp: &HourInput<'_>) -> Self {
        let h = inp.hour_ms;
        let mut c = EdgeCell::default();
        let mut best: Option<(Decimal, String)> = None;
        for e in opened_in(inp.episodes, h) {
            if e.positive() && e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
            }
            let Some(b) = counted(e) else { continue };
            c.table.get_mut(e.side).0 += b;
            *c.n.get_mut(e.side) += 1;
            let cand = (b, e.id.clone());
            if best.as_ref().is_none_or(|x| better(&cand, x)) {
                best = Some(cand);
            }
        }
        let cap = Decimal::from(CAP_USD);
        let mut events: Vec<(i64, u8, Decimal)> = Vec::new();
        for e in inp.episodes {
            if counted(e).is_none() {
                continue;
            }
            if let Some((s, t)) = overlap(e, h) {
                let cost = e.cost.min(cap).max(Decimal::ZERO);
                events.push((s, 1, cost));
                events.push((t, 0, -cost)); // at equal times closes (0) sort before opens (1)
            }
        }
        events.sort_by_key(|a| (a.0, a.1));
        let (mut run, mut max) = (Decimal::ZERO, Decimal::ZERO);
        for (_, _, d) in events {
            run += d;
            if run > max {
                max = run;
            }
        }
        c.max_capital = Num(max);
        c.biggest = best.map(|(usd, episode_id)| Biggest {
            usd: Num(usd),
            episode_id,
        });
        for v in inp.latency_ms {
            *c.lat.entry(*v).or_insert(0) += 1;
        }
        for (scenario, d) in inp.equity_deltas {
            let e = c
                .captured
                .entry(scenario.clone())
                .or_insert((0, Num::default()));
            e.0 += 1;
            e.1.0 += *d;
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> EdgeMax {
        EdgeMax {
            capital: self.max_capital.0,
            biggest: self
                .biggest
                .as_ref()
                .map(|b| (b.usd.0, b.episode_id.clone())),
        }
    }

    fn merge_max(into: &mut EdgeMax, o: &EdgeMax) {
        if o.capital > into.capital {
            into.capital = o.capital;
        }
        // Controller ruling F1: a let-chain instead of a nested `if` (clippy::collapsible_if).
        if let Some(b) = &o.biggest
            && into.biggest.as_ref().is_none_or(|a| better(b, a))
        {
            into.biggest = Some(b.clone());
        }
    }
}

/// The host label inside a premise key's `host=code:label|band`.
pub fn host_from_key(key: &str) -> Option<&str> {
    let v = key.split(';').find_map(|p| p.strip_prefix("host="))?;
    let (_, rest) = v.split_once(':')?;
    rest.split('|').next()
}

pub fn edge(c: &EdgeCell, m: &EdgeMax, ctx: &BodyCtx<'_>) -> CardBody {
    make(edge_parts(c, m, ctx), ctx)
}

/// Edge capture's window-independent parts (reads `ctx.premise_key` and `ctx.scenarios` only).
pub fn edge_parts(c: &EdgeCell, m: &EdgeMax, ctx: &BodyCtx<'_>) -> Parts {
    let on_table = c.table.yes.0 + c.table.no.0;
    let n = c.n.yes + c.n.no;
    let mut thin = Vec::new();
    if n < THIN_EDGE_N {
        thin.push(format!(
            "{n} net-positive episodes < {THIN_EDGE_N}; the capture rates are not reliable"
        ));
    }
    if on_table.is_zero() {
        thin.push("nothing was on the table, so there is no capture rate (—)".to_string());
    }
    let rate = |scenario: &str| -> (Option<String>, Option<String>) {
        match c.captured.get(scenario) {
            None => (None, None),
            Some((_, v)) => (
                Some(dec(v.0)),
                (!on_table.is_zero())
                    .then(|| dec((v.0 * Decimal::from(100) / on_table).round_dp(1))),
            ),
        }
    };
    let head_net = ctx
        .scenarios
        .iter()
        .find(|s| s.name == HEADLINE)
        .map_or(145, |s| s.net_ms);
    let (head_cap, head_rate) = rate(HEADLINE);
    let lat_n = stats::total(&c.lat);
    let lat_med = stats::nearest_rank(c.lat.iter().map(|(v, k)| (*v, *k)), lat_n, 50);
    let columns = vec![
        row(ON_TABLE, "USD", None, None, Some(dec(on_table))),
        row(
            "NO side, cashable before settlement",
            "USD",
            None,
            None,
            Some(dec(c.table.no.0)),
        ),
        row(
            "YES side, locked until resolution",
            "USD",
            None,
            None,
            Some(dec(c.table.yes.0)),
        ),
        row(
            &format!("captured at {HEADLINE} ({head_net} ms)"),
            "USD",
            None,
            None,
            head_cap,
        ),
        row(
            &format!("capture rate, {HEADLINE}"),
            "%",
            None,
            None,
            head_rate,
        ),
        row(
            "capital needed at once",
            "USD",
            None,
            None,
            Some(dec(m.capital)),
        ),
        row(
            "biggest single opportunity",
            "USD",
            None,
            None,
            m.biggest.as_ref().map(|b| dec(b.0)),
        ),
        row(
            "measured latency (estimate)",
            "ms",
            None,
            None,
            lat_med.map(|v| v.to_string()),
        ),
    ];
    let decay = ctx
        .scenarios
        .iter()
        .map(|s| {
            let (captured, rate_pct) = rate(&s.name);
            DecayPoint {
                scenario: s.name.clone(),
                latency_ms: s.latency_ms,
                net_ms: s.net_ms,
                captured,
                rate_pct,
                headline: s.name == HEADLINE,
            }
        })
        .collect();
    let measured = lat_med.map(|latency_ms| Measured {
        latency_ms,
        host: host_from_key(ctx.premise_key)
            .unwrap_or("unknown")
            .to_string(),
    });
    let notes = vec![
        UPPER_BOUND.to_string(),
        "P0-converter takes the NO side only; the YES side is locked until resolution and no account takes it."
            .to_string(),
        "Captured: each scenario's P0-converter paper account (default fee), its equity change over the window \
         from paper_marks. Capture rate = captured ÷ on the table, rounded to 0.1% with round_dp (banker's \
         rounding: half to even)."
            .to_string(),
        format!(
            "On the table: net-positive episodes with a capped P&L; drops {} flagged episodes (engine_suspect or \
             stale_transport).",
            c.flagged.yes + c.flagged.no
        ),
        "Restarts: the paper accounts are restored, so the first mark after a restart books the downtime \
         revaluation (the equity change while the scanner was down) into the new premise key's first hour."
            .to_string(),
        "Measured latency: the window's median of feed lag p50 + REST round trip p50 / 2, per health minute. \
         An estimate."
            .to_string(),
    ];
    Parts {
        card: "EDGE",
        paper: "edge",
        title: "Edge capture",
        params: format!("cap_usd={CAP_USD};headline={HEADLINE};thin_n={THIN_EDGE_N}"),
        n,
        columns,
        series: Series {
            decay,
            measured,
            ..Series::default()
        },
        notes,
        thin,
        episode_link: m.biggest.as_ref().map(|b| b.1.clone()),
    }
}

impl Card for EdgeCell {
    fn parts(agg: &Self, max: &EdgeMax, ctx: &BodyCtx<'_>) -> Vec<Parts> {
        vec![edge_parts(agg, max, ctx)]
    }
}
