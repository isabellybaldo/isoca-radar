#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl StoreError {
    /// True when Postgres rejected the DATA (SQLSTATE class 22 data exception or 23 integrity
    /// constraint violation): retrying the same rows can never succeed. Transport/timeouts → false.
    pub fn is_data_error(&self) -> bool {
        match self {
            StoreError::Db(sqlx::Error::Database(e)) => e
                .code()
                .map(|c| c.starts_with("22") || c.starts_with("23"))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// True when the database is busy or unreachable, so the api answers 503 ("try again"):
    /// a statement timeout (57014), an admin shutdown/crash (57P*), a connection failure (08*),
    /// too many connections (53300), pool exhaustion, or a broken socket.
    pub fn is_unavailable(&self) -> bool {
        match self {
            StoreError::Db(sqlx::Error::Database(e)) => e
                .code()
                .map(|c| {
                    c == "57014" || c.starts_with("57P") || c.starts_with("08") || c == "53300"
                })
                .unwrap_or(false),
            StoreError::Db(
                sqlx::Error::PoolTimedOut
                | sqlx::Error::PoolClosed
                | sqlx::Error::Io(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::WorkerCrashed,
            ) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_data_error_is_not_classified_as_data() {
        assert!(!StoreError::Db(sqlx::Error::PoolTimedOut).is_data_error());
    }

    #[test]
    fn pool_timeouts_are_unavailable_but_missing_rows_are_not() {
        assert!(StoreError::Db(sqlx::Error::PoolTimedOut).is_unavailable());
        assert!(!StoreError::Db(sqlx::Error::RowNotFound).is_unavailable());
    }
}
