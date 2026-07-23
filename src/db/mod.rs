#![allow(dead_code)]

//! Postgres persistence layer. Uses `r2d2_postgres` for connection pooling
//! (well-established, minimal API surface — hand-rolled pool is YAGNI here).

pub mod approvals;
pub mod audit;
pub mod executions;
pub mod requests;

use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::env;

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
pub fn connect() -> Pool<PostgresConnectionManager<postgres::NoTls>> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let manager =
        PostgresConnectionManager::new(url.parse().expect("invalid DATABASE_URL"), postgres::NoTls);
    Pool::builder()
        .max_size(5)
        .build(manager)
        .expect("failed to create database connection pool")
}
