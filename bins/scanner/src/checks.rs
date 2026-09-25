//! REST checks (spec §7 integrity, §8.2 re-checker), all through ONE rate limiter:
//! re-checks at open + offsets → observations; resnapshots after a top mismatch; a drift sampler
//! (engine compares after settling); an L1 mirror sampler on NegRisk legs (Phase 0 found 6/346
//! mismatches); and the /ok health probe.
use crate::config::RecheckCfg;
use crate::engine::{Det, RecheckReq};
use crate::metrics::Metrics;
use crate::now_ms;
use crate::universe::{GroupMeta, Status, Universe, group_with_books};
use crate::writer::WriterMsg;
use arb_core::book::Book;
use arb_core::detect::{detect_no_basket, detect_yes_basket};
use pm_api::backoff::ConnectBudget;
use pm_api::clob::ClobClient;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use store::ObservationRow;
use tokio::sync::{mpsc, watch};

pub struct Limiter(Mutex<ConnectBudget>);

impl Limiter {
    pub fn new(per_sec: usize) -> Limiter {
        Limiter(Mutex::new(ConnectBudget::new(
            per_sec.max(1),
            per_sec.max(1) * 60,
        )))
    }

    pub async fn wait(&self) {
        loop {
            let ok = self
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .try_take(now_ms());
            if ok {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[derive(Debug)]
pub enum CheckCmd {
    RestSnapshot { token: String, book: Book },
    Drift { token: String, book: Book },
    L1Mismatch { event_id: String, detail: String },
}

pub async fn fetch_books(
    clob: &ClobClient,
    limiter: &Limiter,
    tokens: &[String],
) -> Result<HashMap<String, Book>, String> {
    let mut out = HashMap::new();
    for chunk in tokens.chunks(pm_api::clob::MAX_TOKENS_PER_REQUEST) {
        limiter.wait().await;
        let wires = clob.books(chunk).await.map_err(|e| e.to_string())?;
        for w in wires {
            let b = w.to_book().map_err(|e| e.to_string())?;
            out.insert(w.asset_id.clone(), b);
        }
    }
    Ok(out)
}

pub fn observe(
    req: &RecheckReq,
    offset_ms: i64,
    meta: Option<&GroupMeta>,
    books: Result<HashMap<String, Book>, String>,
) -> ObservationRow {
    let base = ObservationRow {
        episode_id: req.episode_id.clone(),
        offset_ms: i32::try_from(offset_ms).unwrap_or(i32::MAX),
        source: "rest".into(),
        observed_ms: None,
        skipped: true,
        skip_reason: None,
        edge: None,
        q_max: None,
        pnl: None,
        still_profitable: None,
        fill_at_original_size: None,
    };
    let Some(meta) = meta else {
        return ObservationRow {
            skip_reason: Some("event left the universe".into()),
            ..base
        };
    };
    let books = match books {
        Ok(b) => b,
        Err(e) => {
            return ObservationRow {
                skip_reason: Some(e),
                ..base
            };
        }
    };
    let missing = meta
        .live_yes_tokens()
        .iter()
        .filter(|t| !books.contains_key(*t))
        .count();
    if missing > 0 {
        return ObservationRow {
            skip_reason: Some(format!("{missing} books missing")),
            ..base
        };
    }
    let observed_ms = books.values().map(|b| b.server_ts_ms()).max();
    let g = group_with_books(meta, &books);
    let sig = match req.det {
        Det::Yes => detect_yes_basket(&g),
        Det::No => detect_no_basket(&g),
    };
    match sig {
        Some(s) => ObservationRow {
            observed_ms,
            skipped: false,
            edge: Some(s.quote.top_edge),
            q_max: Some(s.quote.q_max),
            pnl: Some(s.quote.pnl),
            still_profitable: Some(s.net_positive()),
            fill_at_original_size: Some(s.quote.q_max >= req.q_max),
            ..base
        },
        // no gross-positive basket at all: the walk takes nothing, a measured zero
        None => ObservationRow {
            observed_ms,
            skipped: false,
            q_max: Some(Decimal::ZERO),
            pnl: Some(Decimal::ZERO),
            still_profitable: Some(false),
            fill_at_original_size: Some(false),
            ..base
        },
    }
}

pub fn mirror_ok(yes: &Book, no: &Book) -> bool {
    yes.no_asks_best_first() == no.asks_best_first()
        && yes.no_bids_best_first() == no.bids_best_first()
}

pub fn next_batch(len: usize, cursor: usize, n: usize) -> (Vec<usize>, usize) {
    if len == 0 {
        return (Vec::new(), 0);
    }
    let take = n.min(len);
    let start = cursor % len;
    let idx: Vec<usize> = (0..take).map(|k| (start + k) % len).collect();
    (idx, (start + take) % len)
}

pub async fn run_rechecks(
    mut rx: mpsc::Receiver<RecheckReq>,
    universe: watch::Receiver<Arc<Universe>>,
    clob: Arc<ClobClient>,
    limiter: Arc<Limiter>,
    offsets: Vec<i64>,
    writer: mpsc::Sender<WriterMsg>,
) {
    while let Some(req) = rx.recv().await {
        let (universe, clob, limiter, writer) = (
            universe.clone(),
            clob.clone(),
            limiter.clone(),
            writer.clone(),
        );
        let mut offs = offsets.clone();
        let received = now_ms();
        tokio::spawn(async move {
            // spec §8.2: also re-check at +Δd (max secondsDelay over the group's legs) when > 0
            let delay_ms = universe
                .borrow()
                .group_by_event(&req.event_id)
                .and_then(|m| m.legs.iter().filter_map(|l| l.seconds_delay).max())
                .map(|s| i64::from(s) * 1000)
                .unwrap_or(0);
            if delay_ms > 0 {
                offs.push(delay_ms);
            }
            offs.sort_unstable();
            offs.dedup();
            for off in offs {
                let wait = (received + off - now_ms()).max(0);
                tokio::time::sleep(Duration::from_millis(u64::try_from(wait).unwrap_or(0))).await;
                let meta = universe.borrow().group_by_event(&req.event_id).cloned();
                let books = match &meta {
                    Some(m) => fetch_books(&clob, &limiter, &m.live_yes_tokens()).await,
                    None => Ok(HashMap::new()),
                };
                let row = observe(&req, off, meta.as_ref(), books);
                if writer.send(WriterMsg::Observation(row)).await.is_err() {
                    return;
                }
            }
        });
    }
}

pub async fn run_resnapshots(
    mut rx: mpsc::Receiver<String>,
    clob: Arc<ClobClient>,
    limiter: Arc<Limiter>,
    cmd: mpsc::Sender<CheckCmd>,
) {
    while let Some(token) = rx.recv().await {
        match fetch_books(&clob, &limiter, std::slice::from_ref(&token)).await {
            Ok(mut books) => {
                if let Some(book) = books.remove(&token)
                    && cmd
                        .send(CheckCmd::RestSnapshot { token, book })
                        .await
                        .is_err()
                {
                    return;
                }
            }
            Err(e) => eprintln!("resnapshot {token}: {e}"),
        }
    }
}

pub async fn run_samplers(
    universe: watch::Receiver<Arc<Universe>>,
    clob: Arc<ClobClient>,
    limiter: Arc<Limiter>,
    cfg: RecheckCfg,
    cmd: mpsc::Sender<CheckCmd>,
    metrics: Arc<Metrics>,
) {
    let mut minute = tokio::time::interval(Duration::from_secs(60));
    let mut ok_tick = tokio::time::interval(Duration::from_secs(5));
    minute.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ok_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (mut drift_cursor, mut l1_cursor) = (0usize, 0usize);
    loop {
        tokio::select! {
            _ = ok_tick.tick() => {
                limiter.wait().await;
                let t = now_ms();
                let ok = clob.ok().await.unwrap_or(false);
                metrics.push_rest(now_ms() - t);
                metrics.set_ok(ok);
            }
            _ = minute.tick() => {
                let u = universe.borrow().clone();
                // drift: rotate through every subscribed token
                let tokens: Vec<String> = u
                    .groups
                    .iter()
                    .flat_map(|g| g.live_yes_tokens())
                    .chain(u.canary.iter().flat_map(|c| [c.yes_token.clone(), c.no_token.clone()]))
                    .collect();
                let (idx, next) = next_batch(tokens.len(), drift_cursor, cfg.drift_tokens_per_min);
                drift_cursor = next;
                let pick: Vec<String> = idx.into_iter().map(|i| tokens[i].clone()).collect();
                if let Ok(books) = fetch_books(&clob, &limiter, &pick).await {
                    for (token, book) in books {
                        if cmd.send(CheckCmd::Drift { token, book }).await.is_err() {
                            return;
                        }
                    }
                }
                // L1: rotate through live NegRisk legs, YES and NO books from REST
                let legs: Vec<(String, String, String)> = u
                    .groups
                    .iter()
                    .flat_map(|g| {
                        g.legs
                            .iter()
                            .filter(|l| l.status == Status::Live && !l.yes_token.is_empty() && !l.no_token.is_empty())
                            .map(|l| (g.event_id.clone(), l.yes_token.clone(), l.no_token.clone()))
                            .collect::<Vec<_>>()
                    })
                    .chain(u.canary.iter().map(|c| (c.event_id.clone(), c.yes_token.clone(), c.no_token.clone())))
                    .collect();
                let (idx, next) = next_batch(legs.len(), l1_cursor, cfg.l1_legs_per_min);
                l1_cursor = next;
                let pick: Vec<&(String, String, String)> = idx.iter().map(|&i| &legs[i]).collect();
                let toks: Vec<String> = pick.iter().flat_map(|(_, y, n)| [y.clone(), n.clone()]).collect();
                if let Ok(books) = fetch_books(&clob, &limiter, &toks).await {
                    for (event_id, y, n) in pick {
                        if let (Some(yb), Some(nb)) = (books.get(y), books.get(n))
                            && !mirror_ok(yb, nb)
                        {
                            let detail = format!("L1 mirror mismatch yes={y} no={n}");
                            if cmd.send(CheckCmd::L1Mismatch { event_id: event_id.clone(), detail }).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::universe::{LegMeta, Status};
    use arb_core::money::{Price, Shares};
    use rust_decimal_macros::dec;

    fn meta() -> GroupMeta {
        GroupMeta {
            event_id: "e1".into(),
            slug: None,
            title: None,
            neg_risk_market_id: None,
            augmented: false,
            legs: (0..2)
                .map(|i| LegMeta {
                    market_id: format!("m{i}"),
                    yes_token: format!("y{i}"),
                    no_token: format!("n{i}"),
                    status: Status::Live,
                    fee_rate: Decimal::ZERO,
                    fee_exponent: 1,
                    tick: None,
                    min_size: None,
                    seconds_delay: None,
                    end_date: None,
                })
                .collect(),
        }
    }

    fn book(ts: i64, asks: &[(&str, &str)], bids: &[(&str, &str)]) -> Book {
        let lv = |v: &[(&str, &str)]| {
            v.iter()
                .map(|(p, s)| (Price::parse(p).unwrap(), Shares::parse(s).unwrap()))
                .collect::<Vec<_>>()
        };
        let mut b = Book::default();
        b.apply_snapshot(&lv(bids), &lv(asks), ts);
        b
    }

    fn req() -> RecheckReq {
        RecheckReq {
            episode_id: "ep".into(),
            event_id: "e1".into(),
            det: Det::Yes,
            opened_ms: 0,
            q_max: dec!(10),
        }
    }

    #[test]
    fn observation_cases() {
        let m = meta();
        let gone = observe(&req(), 1000, None, Ok(HashMap::new()));
        assert!(gone.skipped && gone.skip_reason.is_some());
        let failed = observe(&req(), 1000, Some(&m), Err("timeout".into()));
        assert_eq!(
            (failed.skipped, failed.skip_reason.as_deref()),
            (true, Some("timeout"))
        );
        let partial: HashMap<String, Book> = [("y0".to_string(), book(5, &[("0.4", "10")], &[]))]
            .into_iter()
            .collect();
        assert!(observe(&req(), 1000, Some(&m), Ok(partial)).skipped);

        let still: HashMap<String, Book> = [
            ("y0".to_string(), book(5, &[("0.40", "10")], &[])),
            ("y1".to_string(), book(6, &[("0.50", "20")], &[])),
        ]
        .into_iter()
        .collect();
        let o = observe(&req(), 2000, Some(&m), Ok(still));
        assert!(!o.skipped);
        assert_eq!((o.offset_ms, o.observed_ms), (2000, Some(6)));
        assert_eq!(
            (o.still_profitable, o.pnl, o.q_max),
            (Some(true), Some(dec!(1.0)), Some(dec!(10)))
        );
        assert_eq!(o.fill_at_original_size, Some(true));

        let gone_edge: HashMap<String, Book> = [
            ("y0".to_string(), book(5, &[("0.60", "10")], &[])),
            ("y1".to_string(), book(6, &[("0.50", "20")], &[])),
        ]
        .into_iter()
        .collect();
        let o = observe(&req(), 5000, Some(&m), Ok(gone_edge));
        assert_eq!(
            (o.still_profitable, o.pnl, o.q_max, o.edge),
            (Some(false), Some(dec!(0)), Some(dec!(0)), None)
        );
    }

    #[test]
    fn mirror_check_compares_both_sides_level_by_level() {
        let yes = book(1, &[("0.40", "10")], &[("0.35", "5")]);
        let no_ok = book(1, &[("0.65", "5")], &[("0.60", "10")]);
        let no_bad = book(1, &[("0.66", "5")], &[("0.60", "10")]);
        assert!(mirror_ok(&yes, &no_ok));
        assert!(!mirror_ok(&yes, &no_bad));
    }

    #[test]
    fn round_robin_wraps() {
        assert_eq!(next_batch(5, 3, 3), (vec![3, 4, 0], 1));
        assert_eq!(next_batch(5, 0, 10), (vec![0, 1, 2, 3, 4], 0));
        assert_eq!(next_batch(0, 0, 3), (vec![], 0));
    }
}
