//! Hourly, merge-exact cells (spec §7). One cell per family per premise key per UTC hour. Every
//! field except Edge capture's maxima is a sum, so a window is the sum of its cells, and a
//! window drops an expired or rebuilt cell by subtracting it. Counts are i64: a negative count
//! would mean a bug, never a valid state. Money is `Num` (a JSON string).
use crate::HOUR_MS;
use crate::stats::{self, CountMap, DurMap};
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Debug;
use std::str::FromStr;

/// Flicker-merge sensitivity (parent spec §5.5: runs separated by ≤ {0, 250 ms, 1 s}).
pub const MERGE_GAPS_MS: [i64; 3] = [0, 250, 1_000];
/// G3: long-lived means ≥ 50 minutes.
pub const LONG_MS: i64 = 3_000_000;
/// G3 THEIR METHOD: present in the first and in the last 5 minutes of the hour.
pub const EDGE_MINUTES_MS: i64 = 300_000;

const GEBELE_AREAS: &[&str] = &["detector_no", "detector_yes", "episode_def", "fees"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Yes,
    No,
}

impl Side {
    /// The scanner's `kind_str`: `yes_basket` is the YES side, `no_basket` the NO side;
    /// `yes_basket_incomplete` (not arbitrage) and `implication` belong to no side.
    pub fn from_kind(kind: &str) -> Option<Side> {
        match kind {
            "yes_basket" => Some(Side::Yes),
            "no_basket" => Some(Side::No),
            _ => None,
        }
    }
}

/// A decimal stored as a JSON string, never a JSON number, normalized like the api's own decimal
/// strings (Global Constraints: trailing zeros dropped, `-0` written as `"0"`). Deserializing
/// still accepts any valid decimal string, including one with trailing zeros (e.g. `"0.410"`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Num(pub Decimal);

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&stats::dec(self.0))
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Decimal::from_str(&s)
            .map(Num)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerSide<T> {
    pub yes: T,
    pub no: T,
}

impl<T> PerSide<T> {
    pub fn get(&self, s: Side) -> &T {
        match s {
            Side::Yes => &self.yes,
            Side::No => &self.no,
        }
    }

    pub fn get_mut(&mut self, s: Side) -> &mut T {
        match s {
            Side::Yes => &mut self.yes,
            Side::No => &mut self.no,
        }
    }
}

/// One episode as the cards see it (analyze maps `store::CardEpisode` into this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ep {
    pub id: String,
    pub run_id: i64,
    pub side: Side,
    pub opened_ms: i64,
    pub closed_ms: Option<i64>,
    /// The span's end for hour overlap: `closed_ms`, else its run's end, else now.
    pub end_ms: i64,
    pub duration_ms: Option<i64>,
    pub censor_left: bool,
    pub censor_right: bool,
    pub censor_gap: bool,
    /// Peak net / gross top edge, as a fraction of $1.
    pub edge_net: Decimal,
    pub edge_gross: Decimal,
    pub pnl_net: Decimal,
    pub pnl_gross: Decimal,
    pub cost: Decimal,
    pub budget100: Option<Decimal>,
    pub suspect: bool,
    pub stale: bool,
    pub prev_gap_ms: Option<i64>,
}

impl Ep {
    /// Net of taker fees, Π > 0 (Gebele's episode, and ours).
    pub fn positive(&self) -> bool {
        self.pnl_net > Decimal::ZERO
    }

    /// Excluded under OUR METHOD (and counted as excluded).
    pub fn flagged(&self) -> bool {
        self.suspect || self.stale
    }

    /// Closed with no censoring flag: its duration is known exactly.
    pub fn uncensored(&self) -> bool {
        self.closed_ms.is_some() && !self.censor_left && !self.censor_right && !self.censor_gap
    }
}

/// Everything one hour's cell is built from. `episodes` may hold more than the hour (a whole
/// batch): each builder picks what it needs with `opened_in` and `overlap`.
#[derive(Debug, Clone, Copy)]
pub struct HourInput<'a> {
    pub hour_ms: i64,
    pub episodes: &'a [Ep],
    /// Edge capture: one measured latency (ms) per health minute of the hour.
    pub latency_ms: &'a [i64],
    /// Edge capture: (scenario, equity change) per paper mark inside the hour.
    pub equity_deltas: &'a [(String, Decimal)],
    /// SWEET: the hour's attempts and their episodes (Phase 5d). Default (empty) for other cards.
    pub sweet: crate::sweet::SweetInput<'a>,
}

pub fn opened_in(eps: &[Ep], hour_ms: i64) -> impl Iterator<Item = &Ep> {
    eps.iter()
        .filter(move |e| e.opened_ms >= hour_ms && e.opened_ms < hour_ms + HOUR_MS)
}

/// The part of the episode's span inside the hour, if any.
pub fn overlap(e: &Ep, hour_ms: i64) -> Option<(i64, i64)> {
    let s = e.opened_ms.max(hour_ms);
    let t = e.end_ms.min(hour_ms + HOUR_MS);
    (s < t).then_some((s, t))
}

pub trait CellData: Clone + Default + PartialEq + Debug + Serialize + DeserializeOwned {
    /// `card_cells.card`.
    const FAMILY: &'static str;
    /// The premise areas the family depends on (arb_core::premises names).
    const AREAS: &'static [&'static str];
    /// Needs health latency and paper marks (Edge capture only).
    const EDGE_INPUTS: bool = false;
    /// Needs the SWEET inputs (Phase 5d): SWEET only.
    const SWEET_INPUTS: bool = false;
    /// analyze rebuilds every hour touched by [last tick − LATE_MS, now] (0: no such rule).
    const LATE_MS: i64 = 0;
    /// Non-subtractable per-hour maxima, merged over a window's hours by `merge_max`.
    type Max: Clone + Default + PartialEq + Debug;

    fn build(inp: &HourInput<'_>) -> Self;
    fn add(&mut self, other: &Self);
    fn sub(&mut self, other: &Self);
    fn max_part(&self) -> Self::Max;
    fn merge_max(into: &mut Self::Max, other: &Self::Max);

    /// Appended to the run-derived premise key (card-side parameters, Phase 5d spec §6). Empty
    /// for every 5a family, so their keys never change.
    fn key_suffix() -> String {
        String::new()
    }

    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

fn add_i64(a: &mut PerSide<i64>, b: &PerSide<i64>, sign: i64) {
    a.yes += sign * b.yes;
    a.no += sign * b.no;
}

fn add_cm(a: &mut PerSide<CountMap>, b: &PerSide<CountMap>, sign: i64) {
    stats::add_counts(&mut a.yes, &b.yes, sign);
    stats::add_counts(&mut a.no, &b.no, sign);
}

fn add_dm(a: &mut PerSide<DurMap>, b: &PerSide<DurMap>, sign: i64) {
    stats::add_durs(&mut a.yes, &b.yes, sign);
    stats::add_durs(&mut a.no, &b.no, sign);
}

fn bump(m: &mut CountMap, k: i64) {
    *m.entry(k).or_insert(0) += 1;
}

/// G1: side asymmetry (spec §5.1). Episodes opened in the hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G1Cell {
    /// Π_net > 0 (THEIR METHOD: raw episodes).
    pub their: PerSide<i64>,
    /// Π_gross > 0 (context).
    pub gross: PerSide<i64>,
    /// Π_net > 0 and not flagged (OUR METHOD).
    pub ours: PerSide<i64>,
    /// Π_net > 0 and flagged (excluded from OUR METHOD, counted).
    pub flagged: PerSide<i64>,
    /// OUR METHOD episodes that merge into their predecessor at gap ≤ 0 / 250 / 1000 ms.
    pub merged_away: PerSide<[i64; 3]>,
}

impl G1Cell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_i64(&mut self.their, &o.their, s);
        add_i64(&mut self.gross, &o.gross, s);
        add_i64(&mut self.ours, &o.ours, s);
        add_i64(&mut self.flagged, &o.flagged, s);
        for (a, b) in self.merged_away.yes.iter_mut().zip(o.merged_away.yes) {
            *a += s * b;
        }
        for (a, b) in self.merged_away.no.iter_mut().zip(o.merged_away.no) {
            *a += s * b;
        }
    }
}

impl CellData for G1Cell {
    const FAMILY: &'static str = "g1";
    const AREAS: &'static [&'static str] = GEBELE_AREAS;
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let mut c = G1Cell::default();
        for e in opened_in(inp.episodes, inp.hour_ms) {
            if e.pnl_gross > Decimal::ZERO {
                *c.gross.get_mut(e.side) += 1;
            }
            if !e.positive() {
                continue;
            }
            *c.their.get_mut(e.side) += 1;
            if e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
                continue;
            }
            *c.ours.get_mut(e.side) += 1;
            if let Some(gap) = e.prev_gap_ms {
                for (slot, g) in c.merged_away.get_mut(e.side).iter_mut().zip(MERGE_GAPS_MS) {
                    if gap <= g {
                        *slot += 1;
                    }
                }
            }
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}

/// G2 and G3: durations and survival, the long-lived share (spec §5.2, §5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurCell {
    /// THEIR METHOD: uncensored durations (flagged included), by opening hour.
    pub their: PerSide<CountMap>,
    /// OUR METHOD, Kaplan–Meier input: duration → (events, right-censored), flagged, left- and
    /// gap-censored episodes excluded.
    pub ours: PerSide<DurMap>,
    pub left: PerSide<i64>,
    /// Gap-censored recorded spans (an upper bound on the true duration).
    pub gap: PerSide<CountMap>,
    pub flagged: PerSide<i64>,
    /// Not closed yet: no duration.
    pub open: PerSide<i64>,
    /// G3 THEIR METHOD: episodes observed in this clock hour, and those present in both its first
    /// and last 5 minutes. An episode spanning several hours is observed in each.
    pub hour_observed: i64,
    pub hour_long: i64,
    /// G3 OUR METHOD: overlap of each uncensored episode with each hour it touches.
    pub clipped: CountMap,
}

impl DurCell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_cm(&mut self.their, &o.their, s);
        add_dm(&mut self.ours, &o.ours, s);
        add_i64(&mut self.left, &o.left, s);
        add_cm(&mut self.gap, &o.gap, s);
        add_i64(&mut self.flagged, &o.flagged, s);
        add_i64(&mut self.open, &o.open, s);
        self.hour_observed += s * o.hour_observed;
        self.hour_long += s * o.hour_long;
        stats::add_counts(&mut self.clipped, &o.clipped, s);
    }
}

impl CellData for DurCell {
    const FAMILY: &'static str = "dur";
    const AREAS: &'static [&'static str] = &[
        "censoring",
        "detector_no",
        "detector_yes",
        "episode_def",
        "fees",
    ];
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let h = inp.hour_ms;
        let mut c = DurCell::default();
        for e in opened_in(inp.episodes, h).filter(|e| e.positive()) {
            let Some(d) = e.duration_ms.filter(|_| e.closed_ms.is_some()) else {
                *c.open.get_mut(e.side) += 1;
                continue;
            };
            if e.uncensored() {
                bump(c.their.get_mut(e.side), d);
            }
            if e.censor_left {
                *c.left.get_mut(e.side) += 1;
                continue;
            }
            if e.censor_gap {
                bump(c.gap.get_mut(e.side), d);
                continue;
            }
            if e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
                continue;
            }
            let slot = c.ours.get_mut(e.side).entry(d).or_insert((0, 0));
            if e.censor_right {
                slot.1 += 1;
            } else {
                slot.0 += 1;
            }
        }
        for e in inp.episodes.iter().filter(|e| e.positive()) {
            let Some((s, t)) = overlap(e, h) else {
                continue;
            };
            c.hour_observed += 1;
            let first = e.opened_ms < h + EDGE_MINUTES_MS && e.end_ms > h;
            let last = e.opened_ms < h + HOUR_MS && e.end_ms > h + HOUR_MS - EDGE_MINUTES_MS;
            if first && last {
                c.hour_long += 1;
            }
            if e.uncensored() && !e.flagged() {
                bump(&mut c.clipped, t - s);
            }
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}

/// G4: net edge by side (spec §5.4). Peak edges in 0.01-bp units, episodes opened in the hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct G4Cell {
    /// Raw Π_net > 0 counts (THEIR METHOD, and the counts the paper's figure insets show).
    pub their: PerSide<i64>,
    /// OUR METHOD (not flagged): peak net edge → count.
    pub net: PerSide<CountMap>,
    /// The same episodes' peak gross edge (the overlay).
    pub gross: PerSide<CountMap>,
    /// Π_net > 0 and flagged (excluded from OUR METHOD, counted — spec §5).
    /// Default when absent: cells stored before this field existed still load (zero flagged).
    #[serde(default)]
    pub flagged: PerSide<i64>,
}

impl G4Cell {
    fn apply(&mut self, o: &Self, s: i64) {
        add_i64(&mut self.their, &o.their, s);
        add_cm(&mut self.net, &o.net, s);
        add_cm(&mut self.gross, &o.gross, s);
        add_i64(&mut self.flagged, &o.flagged, s);
    }
}

impl CellData for G4Cell {
    const FAMILY: &'static str = "g4";
    const AREAS: &'static [&'static str] = &[
        "depth",
        "detector_no",
        "detector_yes",
        "episode_def",
        "fees",
    ];
    type Max = ();

    fn build(inp: &HourInput<'_>) -> Self {
        let mut c = G4Cell::default();
        for e in opened_in(inp.episodes, inp.hour_ms).filter(|e| e.positive()) {
            *c.their.get_mut(e.side) += 1;
            if e.flagged() {
                *c.flagged.get_mut(e.side) += 1;
                continue;
            }
            bump(c.net.get_mut(e.side), stats::centi_bps(e.edge_net));
            bump(c.gross.get_mut(e.side), stats::centi_bps(e.edge_gross));
        }
        c
    }

    fn add(&mut self, o: &Self) {
        self.apply(o, 1);
    }

    fn sub(&mut self, o: &Self) {
        self.apply(o, -1);
    }

    fn max_part(&self) -> Self::Max {}

    fn merge_max(_into: &mut Self::Max, _other: &Self::Max) {}
}
