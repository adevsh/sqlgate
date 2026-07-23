//! Execution persistence. One row per execution attempt (success or failure).

use crate::db::DbResult;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::time::SystemTime;
use uuid::Uuid;

/// An execution record.
#[derive(Debug)]
pub struct Execution {
    pub id: Uuid,
    pub request_id: Uuid,
    pub executed_query_hash: String,
    pub hash_matched: bool,
    pub rows_affected: Option<i32>,
    pub error_message: Option<String>,
    pub created_at: SystemTime,
}

/// Record an execution attempt.
pub fn insert_execution(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &Uuid,
    executed_query_hash: &str,
    hash_matched: bool,
    rows_affected: Option<i32>,
    error_message: Option<&str>,
) -> DbResult<Uuid> {
    let mut client = pool.get()?;
    let row = client.query_one(
        "INSERT INTO executions (request_id, executed_query_hash, hash_matched, rows_affected, error_message)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
        &[request_id, &executed_query_hash, &hash_matched, &rows_affected, &error_message],
    )?;
    Ok(row.get("id"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::env;

    fn test_pool() -> Option<Pool<PostgresConnectionManager<postgres::NoTls>>> {
        if env::var("DATABASE_URL").is_err() {
            return None;
        }
        Some(db::connect())
    }

    #[test]
    fn test_insert_execution_success() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-exec-{}", Uuid::new_v4());
        let mut client = pool.get().unwrap();
        let req_row = client
            .query_one(
                "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email, status)
                 VALUES ('SELECT 1', $1, 'postgres', 'testdb', 'primary', 'a@b.com', 'executed')
                 RETURNING id",
                &[&hash],
            )
            .unwrap();
        let req_id: Uuid = req_row.get("id");

        let id = insert_execution(&pool, &req_id, &hash, true, Some(42), None).unwrap();

        let row = client
            .query_one("SELECT rows_affected, error_message FROM executions WHERE id = $1", &[&id])
            .unwrap();
        let rows: i32 = row.get("rows_affected");
        let err: Option<String> = row.get("error_message");
        assert_eq!(rows, 42);
        assert!(err.is_none());

        client.execute("DELETE FROM requests WHERE id = $1", &[&req_id]).unwrap();
    }

    #[test]
    fn test_insert_execution_failure() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-exec-fail-{}", Uuid::new_v4());
        let mut client = pool.get().unwrap();
        let req_row = client
            .query_one(
                "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email, status)
                 VALUES ('SELECT 1', $1, 'postgres', 'testdb', 'primary', 'a@b.com', 'executed')
                 RETURNING id",
                &[&hash],
            )
            .unwrap();
        let req_id: Uuid = req_row.get("id");

        let id = insert_execution(&pool, &req_id, &hash, true, None, Some("constraint violation")).unwrap();

        let row = client
            .query_one("SELECT error_message FROM executions WHERE id = $1", &[&id])
            .unwrap();
        let err: Option<String> = row.get("error_message");
        assert_eq!(err.unwrap(), "constraint violation");

        client.execute("DELETE FROM requests WHERE id = $1", &[&req_id]).unwrap();
    }
}
