//! One throwaway database per test inside the Postgres named by TEST_DATABASE_URL (an owner/superuser URL,
//! e.g. postgres://ct_owner:pw@127.0.0.1:15432/postgres). Without it, tests print SKIPPED and pass,
//! unless STORE_TESTS_REQUIRED is set (CI), where a missing URL is a failure.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use store::{PgConnectOptions, Store};

static N: AtomicU32 = AtomicU32::new(0);

pub struct TestDb {
    admin: PgConnectOptions,
    pub name: String,
    pub opts: PgConnectOptions,
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

async fn admin_exec(admin: &PgConnectOptions, sql: &str) {
    let s = Store::connect_with(admin.clone(), 1)
        .await
        .expect("connect admin");
    s.raw_exec(sql).await.expect(sql);
    s.close().await;
}

/// Drop any `ct_test_%` database left behind by a crashed run (its embedded nanos timestamp is
/// older than 1 hour). Database names come only from `pg_database`, never user input.
const STALE_TEST_DB_AGE_NS: u128 = 3_600 * 1_000_000_000;

async fn sweep_stale_test_dbs(admin: &PgConnectOptions) {
    let s = Store::connect_with(admin.clone(), 1)
        .await
        .expect("connect admin");
    let names = s
        .database_names_like(r"ct\_test\_%") // `_` is a LIKE wildcard: escape it
        .await
        .expect("list ct_test_% databases");
    let now = nanos();
    for name in names {
        let Some(ts) = name.rsplit('_').next().and_then(|f| f.parse::<u128>().ok()) else {
            continue;
        };
        if now.saturating_sub(ts) > STALE_TEST_DB_AGE_NS {
            s.raw_exec(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                .await
                .expect("drop stale test db");
        }
    }
    s.close().await;
}

pub async fn fresh() -> Option<(Store, TestDb)> {
    let url = match std::env::var("TEST_DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            assert!(
                std::env::var("STORE_TESTS_REQUIRED").is_err(),
                "STORE_TESTS_REQUIRED is set but TEST_DATABASE_URL is not"
            );
            eprintln!("SKIPPED: TEST_DATABASE_URL not set");
            return None;
        }
    };
    let admin = Store::options(&url).expect("TEST_DATABASE_URL");
    sweep_stale_test_dbs(&admin).await;
    for role in ["scanner_rw", "analyze_ro"] {
        admin_exec(
            &admin,
            &format!(
                "DO $$ BEGIN CREATE ROLE {role}; \
                 EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$"
            ),
        )
        .await;
    }
    let name = format!(
        "ct_test_{}_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
        nanos()
    );
    admin_exec(&admin, &format!("CREATE DATABASE {name}")).await;
    let opts = admin.clone().database(&name);
    let store = Store::connect_with(opts.clone(), 5)
        .await
        .expect("connect test db");
    store.migrate().await.expect("migrate");
    Some((store, TestDb { admin, name, opts }))
}

impl TestDb {
    /// A connection to this test database acting as `role` (SET ROLE from the owner connection).
    pub async fn role(&self, role: &str) -> Store {
        Store::connect_with_role(self.opts.clone(), 2, role)
            .await
            .expect("role connect")
    }

    pub async fn drop(self, store: Store) {
        store.close().await;
        admin_exec(
            &self.admin,
            &format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
    }
}

/// A scanner run in `mode` ('live' or 'replay'), started at 1 ms.
pub async fn run(store: &Store, mode: &str) -> i64 {
    store
        .start_run(&store::NewRun {
            service: "scanner".into(),
            started_ms: 1,
            version: "t".into(),
            config_hash: "c".into(),
            mode: mode.into(),
        })
        .await
        .unwrap()
}

/// The dashboard's read-only pool (analyze_ro, read-only transactions).
pub async fn ro(db: &TestDb) -> Store {
    Store::connect_read_only(db.opts.clone(), 2, 5_000, Some("analyze_ro"))
        .await
        .unwrap()
}
