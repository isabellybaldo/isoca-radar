//! 7b spec §4–§5: the public snapshot writer against a real database: what never ships, and a
//! failed run keeps the old file.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use api::public::{PUBLIC_FILE, PUBLIC_TMP, run_once};
use serde_json::{Value, json};
use std::path::PathBuf;
use store::*;

/// Epoch 1's start.
const S: i64 = 1_790_121_600_000;
/// Recognisable strings the fixture seeds; none may reach the public file.
const SECRETS: [&str; 7] = [
    "0xmkt-zz-secret",
    "tok-zz-secret",
    "Zzsecret referendum",
    "ev-zz-secret",
    "live-9-zzsecret",
    "P0-converter/",
    "vps-zz-secret",
];

async fn sql(owner: &Store, s: String) {
    owner.raw_exec(&s).await.unwrap();
}

/// A temp dir removed on drop, so a failed assertion leaves nothing behind.
struct TmpDir(PathBuf);

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> TmpDir {
    let d = std::env::temp_dir().join(format!("ct_public_it_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    TmpDir(d)
}

/// A ladder epoch with two rungs, a ledger naming a market, token and episode, a SWEET body whose
/// rows carry extra fields, a host premise, and today's episodes with titles and leg ids.
async fn seed(owner: &Store) {
    let run = common::run(owner, "live").await;
    for (id, scenario) in [
        ("P0-converter/rung-0", "rung-0"),
        ("P0-converter/sp-idle", "sp-idle"),
    ] {
        sql(owner, format!(
            "INSERT INTO paper_accounts (run_id, account_id, policy, scenario, fee_bips, bankroll, per_trade, policies_sha256) \
             VALUES ({run}, '{id}', 'P0-converter', '{scenario}', 0, 1000, 100, 'sha')"
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
             qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run}, '{id}', 1, {}, 'sell', 'live-9-zzsecret', '0xmkt-zz-secret', 'tok-zz-secret', 'no', \
             5, 0.4, 2, 0, -1, 1002, NULL)", S + 10
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_ledger (run_id, account_id, seq, t_ms, kind, episode_id, market_id, yes_token, outcome, \
             qty, price, cash_delta, fees, realized_delta, cash_after, equity) \
             VALUES ({run}, '{id}', 2, {}, 'mark', NULL, NULL, NULL, NULL, 0, NULL, 0, 0, 0, 1002, 999)", S + 60_000
        ))
        .await;
        sql(owner, format!(
            "INSERT INTO paper_marks (run_id, account_id, t_ms, cash, equity, realized, unrealized, fees_paid, locked, \
             max_drawdown, n_positions) VALUES ({run}, '{id}', {}, 1002, 999, -1, 0, 0, 0, 0, 0)", S + 60_000
        ))
        .await;
    }
    sql(owner, format!(
        "INSERT INTO paper_epochs (epoch, start_ms, cause, request_id, run_id, policies_sha256) \
         VALUES (1, {S}, 'policies', NULL, {run}, 'sha')"
    ))
    .await;
    let row = |bucket: &str| {
        json!({"side": "no", "fee_bips": 0, "scenario": "rung-0", "latency_ms": 0, "dimension": "category",
               "bucket": bucket, "label": "thin", "n_exec": 3, "clusters": 1, "mean": "-0.5", "lo": null, "hi": null,
               "episode_id": "live-9-zzsecret", "market_id": "0xmkt-zz-secret", "event_title": "Zzsecret referendum"})
    };
    let body = json!({"status": "live", "series": {"sweet": {"rows": [row("crypto"), row("sports")]}},
                      "biggest": {"episode_id": "live-9-zzsecret", "usd": "1"}});
    sql(
        owner,
        format!(
            "INSERT INTO card_results (card, win, premise_key, status, body, computed_ms) \
         VALUES ('SWEET', '30d', 'k', 'live', '{body}'::jsonb, 1)"
        ),
    )
    .await;
    let mut h = common::health(run, common::now() - 600_000, 0);
    h.rest_rtt_p50_ms = Some(150);
    owner
        .write_batch(&Batch {
            health: vec![h],
            ..Batch::default()
        })
        .await
        .unwrap();
    owner
        .insert_run_premises(&[RunPremiseRow {
            run_id: run,
            area: "host".into(),
            version: "1:vps-zz-secret|100-250".into(),
        }])
        .await
        .unwrap();
    let n = common::now();
    let mut ep = common::ep("live-9-zzsecret", run, n - n.rem_euclid(86_400_000) + 1_000);
    ep.event_id = "ev-zz-secret".into();
    ep.event_title = Some("Zzsecret referendum".into());
    let mut leg = common::leg("live-9-zzsecret", 0, true);
    leg.market_id = "0xmkt-zz-secret".into();
    leg.token = "tok-zz-secret".into();
    owner
        .write_batch(&Batch {
            episodes: vec![EpisodeWrite {
                row: ep,
                legs: vec![leg],
            }],
            ..Batch::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn the_snapshot_names_no_market_token_episode_account_or_host() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    seed(&owner).await;
    let (_app, st) = common::app_with_policies(&db).await;
    let t = dir("content");
    let d = &t.0;
    let n = run_once(&st, d, 42).await.unwrap();
    let text = std::fs::read_to_string(d.join(PUBLIC_FILE)).unwrap();
    assert_eq!(text.len(), n);
    for s in SECRETS {
        assert!(!text.contains(s), "{s} shipped: {text}");
    }
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (v["v"].clone(), v["written_ms"].clone()),
        (json!(1), json!(42))
    );
    assert_eq!(v["ladder"]["epoch"], 1);
    assert_eq!(
        v["ladder"]["rungs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["scenario"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["rung-0", "sp-idle"],
        "the rungs are there, by scenario"
    );
    assert_eq!(v["ladder"]["measured"], json!({"latency_ms": 153}));
    assert_eq!(v["ladder"]["markets"].as_array().unwrap().len(), 2);
    assert_eq!(v["today"]["arb"], 1, "today counts the seeded episode");
    assert!(!d.join(PUBLIC_TMP).exists());
    db.drop(owner).await;
}

#[tokio::test]
async fn a_failed_query_keeps_the_old_file() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let (_app, st) = common::app_with_policies(&db).await;
    let t = dir("query");
    let d = &t.0;
    run_once(&st, d, 1).await.unwrap();
    let before = std::fs::read(d.join(PUBLIC_FILE)).unwrap();
    owner.raw_exec("DROP TABLE episodes CASCADE").await.unwrap();
    let e = run_once(&st, d, 2).await.unwrap_err();
    assert!(
        e.starts_with("today: ") && e.contains("episodes"),
        "the reason names the real cause: {e}"
    );
    assert_eq!(std::fs::read(d.join(PUBLIC_FILE)).unwrap(), before);
    db.drop(owner).await;
}
