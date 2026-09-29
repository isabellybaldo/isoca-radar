#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use store::*;

const DAY: i64 = 86_400_000;
/// A UTC midnight.
const D0: i64 = DAY * 20_720;

async fn live_run(store: &Store) -> i64 {
    store
        .start_run(&NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "v".into(),
            config_hash: "c".into(),
            mode: "live".into(),
        })
        .await
        .unwrap()
}

fn epoch_row(
    epoch: i32,
    start_ms: i64,
    cause: &str,
    request_id: Option<i64>,
    run_id: i64,
) -> PaperEpochRow {
    PaperEpochRow {
        epoch,
        start_ms,
        cause: cause.into(),
        request_id,
        run_id,
        policies_sha256: "sha".into(),
    }
}

async fn reset_pool(db: &common::TestDb) -> Store {
    Store::connect_reset(db.opts.clone(), 5_000, Some("paper_reset_rw"))
        .await
        .unwrap()
}

#[tokio::test]
async fn an_epoch_row_is_written_with_its_state_in_one_batch_and_a_resend_changes_nothing() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = live_run(&owner).await;
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 0,
            last_applied: None
        }
    );
    let b = Batch {
        paper_epochs: vec![epoch_row(1, D0 + 1, "policies", None, run)],
        paper_state: vec![PaperStateRow {
            account_id: "P0-converter/rung-0".into(),
            state: "{}".into(),
            updated_ms: D0 + 2,
            policies_sha256: Some("sha".into()),
        }],
        ..Batch::default()
    };
    let scanner = db.role("scanner_rw").await;
    scanner
        .write_batch(&b)
        .await
        .expect("scanner_rw writes the epoch row and the state together");
    scanner
        .write_batch(&b)
        .await
        .expect("a resend after a lost ack is a no-op");
    assert_eq!(
        owner.paper_epochs().await.unwrap(),
        vec![PaperEpoch {
            epoch: 1,
            start_ms: D0 + 1,
            cause: "policies".into(),
            request_id: None
        }]
    );
    assert_eq!(owner.paper_epoch_starts().await.unwrap(), vec![D0 + 1]);
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 1,
            last_applied: None
        }
    );
    scanner.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn requests_collapse_into_the_epoch_that_names_the_highest_one() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let run = live_run(&owner).await;
    let reset = reset_pool(&db).await;
    // two requests across UTC midnight: both land (spec 7a §5.1 races)
    let ResetInsert::Queued(a) = reset.insert_reset_request(D0 + DAY - 1).await.unwrap() else {
        panic!("queued")
    };
    let ResetInsert::Queued(b) = reset.insert_reset_request(D0 + DAY + 1).await.unwrap() else {
        panic!("queued")
    };
    assert_eq!(owner.reset_requests_after(0).await.unwrap(), vec![a, b]);
    assert_eq!(owner.reset_requests_after(a.id).await.unwrap(), vec![b]);
    assert_eq!(reset.pending_reset_request().await.unwrap(), Some(b));
    assert_eq!(reset.latest_reset_request().await.unwrap(), Some(b));
    // the scanner collapses both into ONE epoch naming the highest id
    owner
        .write_batch(&Batch {
            paper_epochs: vec![epoch_row(1, D0 + DAY + 5, "reset", Some(b.id), run)],
            ..Batch::default()
        })
        .await
        .unwrap();
    assert_eq!(
        reset.pending_reset_request().await.unwrap(),
        None,
        "the lower id collapsed into the epoch is not pending either"
    );
    assert_eq!(
        owner.epoch_status().await.unwrap(),
        EpochStatus {
            current: 1,
            last_applied: Some(b.id)
        }
    );
    reset.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn a_second_request_on_the_same_utc_day_is_the_daily_limit() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = reset_pool(&db).await;
    assert!(matches!(
        reset.insert_reset_request(D0 + 10).await.unwrap(),
        ResetInsert::Queued(r) if r.requested_ms == D0 + 10
    ));
    assert_eq!(
        reset.insert_reset_request(D0 + DAY - 1).await.unwrap(),
        ResetInsert::DailyLimit
    );
    assert!(matches!(
        reset.insert_reset_request(D0 + DAY).await.unwrap(),
        ResetInsert::Queued(_)
    ));
    reset.close().await;
    db.drop(owner).await;
}

#[tokio::test]
async fn the_reset_role_writes_only_reset_requests() {
    let Some((owner, db)) = common::fresh().await else {
        return;
    };
    let reset = reset_pool(&db).await;
    assert_eq!(
        reset.role_writes_outside_reset().await.unwrap(),
        Vec::<String>::new()
    );
    assert!(
        !owner.role_writes_outside_reset().await.unwrap().is_empty(),
        "the owner can write everything: refused"
    );
    owner
        .raw_exec(
            "GRANT INSERT ON episodes TO paper_reset_rw; GRANT UPDATE ON paper_reset_requests TO paper_reset_rw",
        )
        .await
        .unwrap();
    assert_eq!(
        reset.role_writes_outside_reset().await.unwrap(),
        vec!["episodes".to_string(), "paper_reset_requests".to_string()]
    );
    reset.close().await;
    db.drop(owner).await;
}
