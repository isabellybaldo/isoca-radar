//! Episodes (spec §5.5): a maximal run of net-positive states per group, in server time.
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Censor {
    pub left: bool,
    pub right: bool,
    pub gap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpisodeEvent {
    Open {
        id: u64,
        key: String,
        ts_ms: i64,
        pnl: Decimal,
    },
    Peak {
        id: u64,
        ts_ms: i64,
        pnl: Decimal,
    },
    Close {
        id: u64,
        ts_ms: i64,
        duration_ms: i64,
        peak_pnl: Decimal,
        censor: Censor,
    },
}

#[derive(Debug, Clone)]
struct Open {
    id: u64,
    opened_ms: i64,
    last_ms: i64,
    peak: Decimal,
    censor: Censor,
}

#[derive(Debug)]
pub struct EpisodeTracker {
    gap_ms: i64,
    next_id: u64,
    open: HashMap<String, Open>,
    seen: HashSet<String>,
}

impl EpisodeTracker {
    pub fn new(gap_ms: i64) -> Self {
        Self {
            gap_ms,
            next_id: 1,
            open: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    /// Censor flags of the open episode for `key` (None if none is open). Lets callers read
    /// `left` at Open time without mirroring the tracker's first-sight state.
    pub fn open_censor(&self, key: &str) -> Option<Censor> {
        self.open.get(key).map(|ep| ep.censor)
    }

    /// Feed one evaluated state. `net_pnl` is Some only when the signal is net-positive:
    /// `Some(p)` with a positive `p` counts as positive; anything else (`None`, or `Some(p)`
    /// with `p <= 0`) is not.
    pub fn on_state(
        &mut self,
        key: &str,
        ts_ms: i64,
        net_pnl: Option<Decimal>,
    ) -> Vec<EpisodeEvent> {
        let net_pnl = net_pnl.filter(|p| *p > Decimal::ZERO);
        let first_sight = self.seen.insert(key.to_string());
        let gap_ms = self.gap_ms;
        let mut out = Vec::new();
        match (self.open.contains_key(key), net_pnl) {
            (true, Some(pnl)) => {
                if let Some(ep) = self.open.get_mut(key) {
                    if ts_ms - ep.last_ms > gap_ms {
                        ep.censor.gap = true;
                    }
                    ep.last_ms = ts_ms;
                    if pnl > ep.peak {
                        ep.peak = pnl;
                        out.push(EpisodeEvent::Peak {
                            id: ep.id,
                            ts_ms,
                            pnl,
                        });
                    }
                }
            }
            (true, None) => {
                if let Some(mut ep) = self.open.remove(key) {
                    if ts_ms - ep.last_ms > gap_ms {
                        ep.censor.gap = true;
                    }
                    out.push(close(ep, ts_ms));
                }
            }
            (false, Some(pnl)) => {
                let id = self.next_id;
                self.next_id += 1;
                let censor = Censor {
                    left: first_sight,
                    ..Censor::default()
                };
                self.open.insert(
                    key.to_string(),
                    Open {
                        id,
                        opened_ms: ts_ms,
                        last_ms: ts_ms,
                        peak: pnl,
                        censor,
                    },
                );
                out.push(EpisodeEvent::Open {
                    id,
                    key: key.to_string(),
                    ts_ms,
                    pnl,
                });
            }
            (false, None) => {}
        }
        out
    }

    /// The group's feed dropped: right-censor any open episode, and treat the next state as a first sight.
    pub fn on_disconnect(&mut self, key: &str, ts_ms: i64) -> Vec<EpisodeEvent> {
        self.seen.remove(key);
        match self.open.remove(key) {
            Some(mut ep) => {
                ep.censor.right = true;
                vec![close(ep, ts_ms)]
            }
            None => Vec::new(),
        }
    }

    /// The feed for this group is alive at `ts_ms` but the group's state didn't change:
    /// advance the gap clock of an open episode without re-evaluating. Callers (the S2 scanner)
    /// call this on shard heartbeats / frames so quiet books aren't mistaken for feed gaps.
    pub fn on_alive(&mut self, key: &str, ts_ms: i64) {
        let gap_ms = self.gap_ms;
        if let Some(ep) = self.open.get_mut(key) {
            if ts_ms - ep.last_ms > gap_ms {
                ep.censor.gap = true;
            }
            ep.last_ms = ts_ms;
        }
    }

    /// Scanner shutdown: right-censor and close every open episode at `ts_ms`.
    pub fn close_all(&mut self, ts_ms: i64) -> Vec<EpisodeEvent> {
        let gap_ms = self.gap_ms;
        let mut opens: Vec<Open> = self.open.drain().map(|(_, ep)| ep).collect();
        opens.sort_by_key(|ep| ep.id);
        self.seen.clear();
        opens
            .into_iter()
            .map(|mut ep| {
                if ts_ms - ep.last_ms > gap_ms {
                    ep.censor.gap = true;
                }
                ep.censor.right = true;
                close(ep, ts_ms)
            })
            .collect()
    }
}

fn close(ep: Open, ts_ms: i64) -> EpisodeEvent {
    EpisodeEvent::Close {
        id: ep.id,
        ts_ms,
        duration_ms: ts_ms - ep.opened_ms,
        peak_pnl: ep.peak,
        censor: ep.censor,
    }
}
