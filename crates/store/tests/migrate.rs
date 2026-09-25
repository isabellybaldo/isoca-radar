#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

#[tokio::test]
async fn migrations_apply_from_empty_and_are_idempotent() {
    let Some((store, db)) = common::fresh().await else {
        return;
    };
    // fresh() already migrated once; a second run must be a no-op.
    store.migrate().await.expect("second migrate");
    let tables: Vec<String> = sqlx_names(&store).await;
    for t in [
        "episode_legs",
        "episodes",
        "events",
        "health",
        "integrity_events",
        "legs",
        "observations",
        "runs",
        "universe_snapshots",
    ] {
        assert!(
            tables.iter().any(|x| x == t),
            "missing table {t}: {tables:?}"
        );
    }
    db.drop(store).await;
}

async fn sqlx_names(store: &store::Store) -> Vec<String> {
    store.table_names().await.expect("table names")
}
