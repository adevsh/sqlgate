//! Query request persistence. All SQL is hand-written and parameterized
//! (never string-concatenated). No ORM.

use crate::db::DbResult;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::time::SystemTime;
use uuid::Uuid;

/// A query request row.
#[derive(Debug)]
pub struct Request {
    pub id: Uuid,
    pub query_text: String,
    pub query_hash: String,
    pub target_kind: String,
    pub target_db: String,
    pub target_topology: String,
    pub requester_email: String,
    pub status: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// Insert a new query request. Returns the generated UUID.
pub fn insert_request(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    query_text: &str,
    query_hash: &str,
    target_kind: &str,
    target_db: &str,
    target_topology: &str,
    requester_email: &str,
) -> DbResult<Uuid> {
    let mut client = pool.get()?;
    let row = client.query_one(
        "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
        &[&query_text, &query_hash, &target_kind, &target_db, &target_topology, &requester_email],
    )?;
    Ok(row.get("id"))
}

/// Fetch a single request by its ID.
pub fn get_request(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    id: &Uuid,
) -> DbResult<Option<Request>> {
    let mut client = pool.get()?;
    let row = client.query_opt(
        "SELECT id, query_text, query_hash, target_kind, target_db, target_topology,
                requester_email, status, created_at, updated_at
         FROM requests WHERE id = $1",
        &[id],
    )?;
    Ok(row.map(|r| Request {
        id: r.get("id"),
        query_text: r.get("query_text"),
        query_hash: r.get("query_hash"),
        target_kind: r.get("target_kind"),
        target_db: r.get("target_db"),
        target_topology: r.get("target_topology"),
        requester_email: r.get("requester_email"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }))
}

/// List requests, most recent first. Supports optional status filter.
pub fn list_requests(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    status_filter: Option<&str>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<Request>> {
    let mut client = pool.get()?;
    let rows = if let Some(status) = status_filter {
        client.query(
            "SELECT id, query_text, query_hash, target_kind, target_db, target_topology,
                    requester_email, status, created_at, updated_at
             FROM requests WHERE status = $1
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            &[&status, &limit, &offset],
        )?
    } else {
        client.query(
            "SELECT id, query_text, query_hash, target_kind, target_db, target_topology,
                    requester_email, status, created_at, updated_at
             FROM requests
             ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            &[&limit, &offset],
        )?
    };
    Ok(rows
        .into_iter()
        .map(|r| Request {
            id: r.get("id"),
            query_text: r.get("query_text"),
            query_hash: r.get("query_hash"),
            target_kind: r.get("target_kind"),
            target_db: r.get("target_db"),
            target_topology: r.get("target_topology"),
            requester_email: r.get("requester_email"),
            status: r.get("status"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

/// Update the status of a request.
pub fn update_status(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    id: &Uuid,
    new_status: &str,
) -> DbResult<u64> {
    let mut client = pool.get()?;
    let n = client.execute(
        "UPDATE requests SET status = $1, updated_at = now() WHERE id = $2",
        &[&new_status, id],
    )?;
    Ok(n)
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
        db::connect()
    }

    #[test]
    fn test_insert_and_get_request() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-hash-{}", Uuid::new_v4());
        let id = insert_request(
            &pool, "SELECT 1", &hash, "postgres", "testdb", "primary", "test@e.com",
        )
        .unwrap();
        let req = get_request(&pool, &id).unwrap().unwrap();
        assert_eq!(req.query_text, "SELECT 1");
        assert_eq!(req.status, "submitted");
        let mut client = pool.get().unwrap();
        client.execute("DELETE FROM requests WHERE id = $1", &[&id]).unwrap();
    }

    #[test]
    fn test_list_requests() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-list-{}", Uuid::new_v4());
        let id = insert_request(
            &pool, "SELECT 2", &hash, "postgres", "testdb", "primary", "a@b.com",
        )
        .unwrap();
        let list = list_requests(&pool, None, 10, 0).unwrap();
        assert!(list.iter().any(|r| r.id == id));
        let mut client = pool.get().unwrap();
        client.execute("DELETE FROM requests WHERE id = $1", &[&id]).unwrap();
    }

    #[test]
    fn test_update_status() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-status-{}", Uuid::new_v4());
        let id = insert_request(
            &pool, "SELECT 3", &hash, "postgres", "testdb", "primary", "a@b.com",
        )
        .unwrap();
        update_status(&pool, &id, "previewed").unwrap();
        let req = get_request(&pool, &id).unwrap().unwrap();
        assert_eq!(req.status, "previewed");
        let mut client = pool.get().unwrap();
        client.execute("DELETE FROM requests WHERE id = $1", &[&id]).unwrap();
    }
}
