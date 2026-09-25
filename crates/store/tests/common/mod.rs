//! One throwaway database per test inside the Postgres named by TEST_DATABASE_URL (an owner/superuser URL,
//! e.g. postgres://ct_owner:pw@127.0.0.1:55432/postgres). Without it, tests print SKIPPED and pass,
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
