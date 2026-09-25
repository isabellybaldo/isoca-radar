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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_data_error_is_not_classified_as_data() {
        assert!(!StoreError::Db(sqlx::Error::PoolTimedOut).is_data_error());
    }
}
