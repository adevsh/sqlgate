//! Approval persistence. All SQL is hand-written and parameterized.

use crate::db::DbResult;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::time::SystemTime;
use uuid::Uuid;

/// An approval decision row.
#[derive(Debug)]
pub struct Approval {
    pub id: Uuid,
    pub request_id: Uuid,
    pub approver_email: String,
    pub decision: String,
    pub expires_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

/// Insert an approval decision. `expires_at` is `None` for rejections.
pub fn insert_approval(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &Uuid,
    approver_email: &str,
    decision: &str,
    expires_at: Option<SystemTime>,
) -> DbResult<Uuid> {
    let mut client = pool.get()?;
    let row = client.query_one(
        "INSERT INTO approvals (request_id, approver_email, decision, expires_at)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
        &[request_id, &approver_email, &decision, &expires_at],
    )?;
    Ok(row.get("id"))
}

/// Transition any `approved` requests past their `expires_at` to `expired`.
pub fn expire_stale_approvals(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
) -> DbResult<u64> {
    let mut client = pool.get()?;
    let n = client.execute(
        "UPDATE requests SET status = 'expired', updated_at = now()
         WHERE status = 'approved'
           AND id IN (
               SELECT a.request_id FROM approvals a
               WHERE a.decision = 'approved'
                 AND a.expires_at IS NOT NULL
                 AND a.expires_at < now()
           )",
        &[],
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
        Some(db::connect())
    }

    #[test]
    fn test_insert_approval() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-approval-{}", Uuid::new_v4());
        let mut client = pool.get().unwrap();
        let req_row = client
            .query_one(
                "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email, status)
                 VALUES ('SELECT 1', $1, 'postgres', 'testdb', 'primary', 'a@b.com', 'pending_approval')
                 RETURNING id",
                &[&hash],
            )
            .unwrap();
        let req_id: Uuid = req_row.get("id");

        let id = insert_approval(
            &pool, &req_id, "approver@e.com", "approved",
            Some(SystemTime::now() + std::time::Duration::from_secs(900)),
        )
        .unwrap();

        let decision: String = client
            .query_one("SELECT decision FROM approvals WHERE id = $1", &[&id])
            .unwrap()
            .get("decision");
        assert_eq!(decision, "approved");

        client.execute("DELETE FROM requests WHERE id = $1", &[&req_id]).unwrap();
    }

    #[test]
    fn test_expire_stale_approvals() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-expire-{}", Uuid::new_v4());
        let mut client = pool.get().unwrap();
        let req_row = client
            .query_one(
                "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email, status)
                 VALUES ('SELECT 2', $1, 'postgres', 'testdb', 'primary', 'a@b.com', 'approved')
                 RETURNING id",
                &[&hash],
            )
            .unwrap();
        let req_id: Uuid = req_row.get("id");

        client
            .execute(
                "INSERT INTO approvals (request_id, approver_email, decision, expires_at)
                 VALUES ($1, 'a@b.com', 'approved', now() - interval '1 hour')",
                &[&req_id],
            )
            .unwrap();

        let expired = expire_stale_approvals(&pool).unwrap();
        assert!(expired >= 1);

        let status: String = client
            .query_one("SELECT status FROM requests WHERE id = $1", &[&req_id])
            .unwrap()
            .get("status");
        assert_eq!(status, "expired");

        client.execute("DELETE FROM requests WHERE id = $1", &[&req_id]).unwrap();
    }
}
