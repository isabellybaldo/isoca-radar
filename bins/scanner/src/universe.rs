//! The scanner's universe (spec §7): NegRisk groups with EVERY outcome (tradeable or not, so
//! completeness stays honest) plus a daily canary sample of binary markets (both tokens).
use arb_core::book::Book;
use arb_core::detect::{Group, GroupLeg, LegStatus};
use arb_core::fee::FeeSchedule;
use pm_api::gamma::{GammaEvent, GammaMarket, fee_schedule, leg_status, token_ids};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use store::{EventRow, LegRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Live,
    Placeholder,
    Other,
    ClosedNo,
    ClosedYes,
    Halted,
}

impl Status {
    pub fn from_core(s: LegStatus) -> Self {
        match s {
            LegStatus::Live => Status::Live,
            LegStatus::Placeholder => Status::Placeholder,
            LegStatus::Other => Status::Other,
            LegStatus::ClosedNo => Status::ClosedNo,
            LegStatus::ClosedYes => Status::ClosedYes,
            LegStatus::Halted => Status::Halted,
        }
    }

    pub fn to_core(self) -> LegStatus {
        match self {
            Status::Live => LegStatus::Live,
            Status::Placeholder => LegStatus::Placeholder,
            Status::Other => LegStatus::Other,
            Status::ClosedNo => LegStatus::ClosedNo,
            Status::ClosedYes => LegStatus::ClosedYes,
            Status::Halted => LegStatus::Halted,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Live => "live",
            Status::Placeholder => "placeholder",
            Status::Other => "other",
            Status::ClosedNo => "closed_no",
            Status::ClosedYes => "closed_yes",
            Status::Halted => "halted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegMeta {
    pub market_id: String,
    pub yes_token: String,
    pub no_token: String,
    pub status: Status,
    pub fee_rate: Decimal,
    pub fee_exponent: u8,
    pub tick: Option<Decimal>,
    pub min_size: Option<Decimal>,
    pub seconds_delay: Option<i32>,
    pub end_date: Option<String>,
}

impl LegMeta {
    /// Built from a schedule `FeeSchedule::new` already accepted, so the fallback is unreachable in practice.
    pub fn fee(&self) -> FeeSchedule {
        FeeSchedule::new(self.fee_rate, self.fee_exponent).unwrap_or(FeeSchedule::ZERO)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMeta {
    pub event_id: String,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub neg_risk_market_id: Option<String>,
    pub augmented: bool,
    pub legs: Vec<LegMeta>,
    /// Lower-cased Gamma tag slugs (spec 5d §3). `#[serde(default)]` keeps old `universe_snapshots` readable.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

impl GroupMeta {
    /// Tokens we subscribe for this group: YES of every live leg (NO is derived by L1).
    pub fn live_yes_tokens(&self) -> Vec<String> {
        self.legs
            .iter()
            .filter(|l| l.status == Status::Live && !l.yes_token.is_empty())
            .map(|l| l.yes_token.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanaryPair {
    pub market_id: String,
    pub event_id: String,
    pub yes_token: String,
    pub no_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Universe {
    pub taken_ms: i64,
    pub groups: Vec<GroupMeta>,
    pub canary: Vec<CanaryPair>,
}

fn dec(v: &Option<Value>) -> Option<Decimal> {
    match v {
        Some(Value::String(s)) => Decimal::from_str_exact(s.trim()).ok(),
        Some(Value::Number(n)) => Decimal::from_str_exact(&n.to_string()).ok(),
        _ => None,
    }
}

fn leg_meta(m: &GammaMarket, augmented: bool) -> LegMeta {
    let mut status = Status::from_core(leg_status(m, augmented));
    let (yes_token, no_token) = match token_ids(m) {
        Ok(Some((y, n))) => (y, n),
        _ => {
            if status == Status::Live {
                status = Status::Halted; // can't subscribe → never treat as a tradeable leg
            }
            (String::new(), String::new())
        }
    };
    let (fee_rate, fee_exponent) = match fee_schedule(m) {
        Ok(f) => (f.rate(), f.exponent()),
        Err(_) => {
            if status == Status::Live {
                status = Status::Halted; // unknown fees → can't compute a net edge
            }
            (Decimal::ZERO, 1)
        }
    };
    LegMeta {
        market_id: m.id.clone(),
        yes_token,
        no_token,
        status,
        fee_rate,
        fee_exponent,
        tick: dec(&m.order_price_min_tick_size),
        min_size: dec(&m.order_min_size),
        seconds_delay: dec(&m.seconds_delay).and_then(|d| d.to_i32()),
        end_date: m.end_date.clone(),
    }
}

fn canary_rank(market_id: &str, day: i64) -> u64 {
    let d = Sha256::digest(format!("{market_id}:{day}").as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b)
}

impl Universe {
    pub fn build(events: &[GammaEvent], taken_ms: i64, canary_n: usize) -> Universe {
        let mut groups = Vec::new();
        let mut candidates = Vec::new();
        for e in events {
            if e.is_neg_risk() {
                let augmented = e.is_augmented();
                let legs: Vec<LegMeta> =
                    e.markets().iter().map(|m| leg_meta(m, augmented)).collect();
                if legs.is_empty() {
                    continue;
                }
                groups.push(GroupMeta {
                    event_id: e.id.clone(),
                    slug: e.slug.clone(),
                    title: e.title.clone(),
                    neg_risk_market_id: e.neg_risk_market_id.clone(),
                    augmented,
                    legs,
                    tags: Some(e.tag_slugs()).filter(|t| !t.is_empty()),
                });
            } else {
                for m in e.markets() {
                    let l = leg_meta(m, false);
                    if l.status == Status::Live && !l.yes_token.is_empty() {
                        candidates.push(CanaryPair {
                            market_id: l.market_id,
                            event_id: e.id.clone(),
                            yes_token: l.yes_token,
                            no_token: l.no_token,
                        });
                    }
                }
            }
        }
        let day = taken_ms.div_euclid(86_400_000);
        candidates.sort_by_key(|c| (canary_rank(&c.market_id, day), c.market_id.clone()));
        candidates.truncate(canary_n);
        groups.sort_by(|a, b| a.event_id.cmp(&b.event_id));
        Universe {
            taken_ms,
            groups,
            canary: candidates,
        }
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(s: &str) -> anyhow::Result<Universe> {
        let mut u: Universe = serde_json::from_str(s)?;
        // group_by_event binary-searches by event_id: never trust the stored order.
        u.groups.sort_by(|a, b| a.event_id.cmp(&b.event_id));
        Ok(u)
    }

    /// NegRisk groups only (canaries are an integrity sample, not persisted as events).
    pub fn store_rows(&self) -> (Vec<EventRow>, Vec<LegRow>) {
        let mut events = Vec::with_capacity(self.groups.len());
        let mut legs = Vec::new();
        for g in &self.groups {
            events.push(EventRow {
                event_id: g.event_id.clone(),
                slug: g.slug.clone(),
                title: g.title.clone(),
                neg_risk_market_id: g.neg_risk_market_id.clone(),
                augmented: g.augmented,
                n_legs: i32::try_from(g.legs.len()).unwrap_or(i32::MAX),
                tags: g.tags.clone(),
            });
            for l in &g.legs {
                legs.push(LegRow {
                    market_id: l.market_id.clone(),
                    event_id: g.event_id.clone(),
                    yes_token: l.yes_token.clone(),
                    no_token: l.no_token.clone(),
                    status: l.status.as_str().to_string(),
                    fee_rate: l.fee_rate,
                    fee_exponent: i16::from(l.fee_exponent),
                    tick: l.tick,
                    min_size: l.min_size,
                    seconds_delay: l.seconds_delay,
                    end_date: l.end_date.clone(),
                });
            }
        }
        (events, legs)
    }

    pub fn subscribed_tokens(&self) -> usize {
        self.groups
            .iter()
            .map(|g| g.live_yes_tokens().len())
            .sum::<usize>()
            + 2 * self.canary.len()
    }

    pub fn group_by_event(&self, event_id: &str) -> Option<&GroupMeta> {
        self.groups
            .binary_search_by(|g| g.event_id.as_str().cmp(event_id))
            .ok()
            .map(|i| &self.groups[i])
    }
}

/// An arb-core `Group` for `meta` with the given YES books (keyed by YES token).
pub fn group_with_books(meta: &GroupMeta, books: &HashMap<String, Book>) -> Group {
    Group {
        legs: meta
            .legs
            .iter()
            .map(|l| GroupLeg {
                status: l.status.to_core(),
                book: books.get(&l.yes_token).cloned().unwrap_or_default(),
                fee: l.fee(),
            })
            .collect(),
        augmented: meta.augmented,
    }
}

/// Deterministic map type used where serialized order matters (capture records).
pub type ConnMap = BTreeMap<String, usize>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use rust_decimal_macros::dec;

    const EVENTS: &str = r#"[
      {"id": "100", "slug": "who-wins", "title": "Who wins?", "negRisk": true, "negRiskAugmented": false,
       "negRiskMarketID": "0xnr",
       "markets": [
         {"id": "1", "active": true, "closed": false, "acceptingOrders": true, "enableOrderBook": true,
          "clobTokenIds": "[\"11\", \"12\"]", "feesEnabled": true, "feeSchedule": {"rate": 0.07, "exponent": 1},
          "orderMinSize": 5, "orderPriceMinTickSize": "0.01", "secondsDelay": 0, "endDate": "2026-11-03T00:00:00Z"},
         {"id": "2", "active": true, "closed": false, "acceptingOrders": true, "enableOrderBook": true,
          "clobTokenIds": "[\"21\", \"22\"]", "feesEnabled": false},
         {"id": "3", "active": true, "closed": true, "clobTokenIds": "[\"31\", \"32\"]", "outcomePrices": "[\"0\", \"1\"]"},
         {"id": "4", "active": true, "closed": false, "acceptingOrders": true, "enableOrderBook": true,
          "clobTokenIds": "not json"}
       ]},
      {"id": "200", "slug": "binary", "negRisk": false,
       "markets": [
         {"id": "5", "active": true, "closed": false, "acceptingOrders": true, "enableOrderBook": true,
          "clobTokenIds": "[\"51\", \"52\"]"},
         {"id": "6", "active": true, "closed": false, "acceptingOrders": true, "enableOrderBook": true,
          "clobTokenIds": "[\"61\", \"62\"]"}
       ]}
    ]"#;

    fn events() -> Vec<GammaEvent> {
        serde_json::from_str(EVENTS).unwrap()
    }

    #[test]
    fn builds_groups_with_every_outcome_and_classifies_legs() {
        let u = Universe::build(&events(), 1_790_000_000_000, 200);
        assert_eq!(u.groups.len(), 1);
        let g = &u.groups[0];
        assert_eq!(g.event_id, "100");
        let st: Vec<Status> = g.legs.iter().map(|l| l.status).collect();
        // leg 4 has unparseable token ids: it can't be subscribed, so it is Halted (keeps the set incomplete)
        assert_eq!(
            st,
            vec![Status::Live, Status::Live, Status::ClosedNo, Status::Halted]
        );
        assert_eq!(
            (g.legs[0].fee_rate, g.legs[0].fee_exponent),
            (dec!(0.07), 1)
        );
        assert_eq!((g.legs[1].fee_rate, g.legs[1].fee_exponent), (dec!(0), 1));
        assert_eq!(g.legs[0].min_size, Some(dec!(5)));
        assert_eq!(g.legs[0].tick, Some(dec!(0.01)));
        assert_eq!(g.legs[0].seconds_delay, Some(0));
        assert_eq!(
            g.live_yes_tokens(),
            vec!["11".to_string(), "21".to_string()]
        );
        assert_eq!(u.subscribed_tokens(), 2 + 2 * u.canary.len());
    }

    #[test]
    fn canary_sample_is_deterministic_per_day_and_capped() {
        let a = Universe::build(&events(), 1_790_000_000_000, 1);
        let b = Universe::build(&events(), 1_790_000_000_000 + 1_000, 1);
        assert_eq!(a.canary.len(), 1);
        assert_eq!(a.canary, b.canary, "same UTC day → same sample");
        let all = Universe::build(&events(), 1_790_000_000_000, 200);
        assert_eq!(all.canary.len(), 2);
    }

    #[test]
    fn json_round_trip_and_store_rows() {
        let u = Universe::build(&events(), 1_790_000_000_000, 200);
        assert_eq!(Universe::from_json(&u.to_json().unwrap()).unwrap(), u);
        let (ev, legs) = u.store_rows();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].n_legs, 4);
        assert_eq!(legs.len(), 4);
        assert_eq!(legs[2].status, "closed_no");
    }

    #[test]
    fn group_with_books_uses_given_books_and_empty_ones_otherwise() {
        let u = Universe::build(&events(), 0, 0);
        let mut books = HashMap::new();
        let mut b = Book::default();
        b.apply_snapshot(
            &[],
            &[(
                arb_core::money::Price::parse("0.4").unwrap(),
                arb_core::money::Shares::parse("10").unwrap(),
            )],
            7,
        );
        books.insert("11".to_string(), b);
        let g = group_with_books(&u.groups[0], &books);
        assert_eq!(g.legs.len(), 4);
        assert_eq!(g.legs[0].book.server_ts_ms(), 7);
        assert!(g.legs[1].book.asks_best_first().is_empty());
        assert_eq!(g.legs[2].status, LegStatus::ClosedNo);
    }

    #[test]
    fn store_rows_carry_the_event_tags_and_old_snapshots_still_load() {
        let mut u = Universe {
            taken_ms: 0,
            groups: Vec::new(),
            canary: Vec::new(),
        };
        u.groups.push(GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: None,
            augmented: false,
            legs: vec![],
            tags: None,
        });
        // a snapshot written before this field existed: drop "tags" from the JSON and reload it
        let mut v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&u).unwrap()).unwrap();
        v["groups"][0].as_object_mut().unwrap().remove("tags");
        let old = Universe::from_json(&v.to_string()).unwrap();
        assert_eq!(old.groups[0].tags, None);
        u.groups[0].tags = Some(vec!["politics".into()]);
        let (events, _) = u.store_rows();
        assert_eq!(events[0].tags, Some(vec!["politics".to_string()]));
    }
}
