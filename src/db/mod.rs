#![allow(dead_code)]

//! Postgres persistence layer. Uses `r2d2_postgres` for connection pooling
//! (well-established, minimal API surface — hand-rolled pool is YAGNI here).

pub mod approvals;
pub mod audit;
pub mod executions;
pub mod requests;

use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;

/// Unified error type for all database operations.
#[derive(Debug)]
pub enum DbError {
    Pool(r2d2::Error),
    Query(postgres::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Pool(e) => write!(f, "pool error: {}", e),
            DbError::Query(e) => write!(f, "query error: {}", e),
        }
    }
}

impl std::error::Error for DbError {}

impl From<r2d2::Error> for DbError {
    fn from(e: r2d2::Error) -> Self {
        DbError::Pool(e)
    }
}

impl From<postgres::Error> for DbError {
    fn from(e: postgres::Error) -> Self {
        DbError::Query(e)
    }
}

/// Convenience result type for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// Build an `r2d2` connection pool from the `DATABASE_URL` environment variable.
///
/// # Panics
///
/// Panics if `DATABASE_URL` is not set or the pool cannot be created — database
/// connectivity is a hard prerequisite for sqlgate.
/// Build an `r2d2` connection pool from the `DATABASE_URL` environment variable.
///
/// Returns `None` if `DATABASE_URL` is not set — the server can start without
/// a database for read-only operations (static assets, health check, form rendering).
pub fn connect() -> Option<Pool<PostgresConnectionManager<postgres::NoTls>>> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let manager =
        PostgresConnectionManager::new(url.parse().ok()?, postgres::NoTls);
    Pool::builder()
        .max_size(5)
        .build(manager)
        .ok()
}
