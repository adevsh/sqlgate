//! Audit log persistence. Append-only — no update or delete paths exist
//! anywhere in this module or the broader codebase. Every state transition
//! and sensitive action writes an immutable row here.

use crate::db::DbResult;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

/// An audit log entry. Immutable once written.
#[derive(Debug)]
pub struct AuditEvent {
    pub id: Uuid,
    pub request_id: Option<Uuid>,
    pub event_type: String,
    pub actor_email: String,
    pub details: Option<Value>,
    pub created_at: SystemTime,
}

/// Append an audit event. This is the *only* write path to `audit_log` —
/// there is no `update_audit_event` or `delete_audit_event` function.
pub fn append_audit_event(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: Option<&Uuid>,
    event_type: &str,
    actor_email: &str,
    details: Option<&Value>,
) -> DbResult<Uuid> {
    let mut client = pool.get()?;
    let row = client.query_one(
        "INSERT INTO audit_log (request_id, event_type, actor_email, details)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
        &[&request_id, &event_type, &actor_email, &details],
    )?;
    Ok(row.get("id"))
}

/// List audit events for a specific request, ordered by occurrence.
pub fn list_audit_events(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &Uuid,
) -> DbResult<Vec<AuditEvent>> {
    let mut client = pool.get()?;
    let rows = client.query(
        "SELECT id, request_id, event_type, actor_email, details, created_at
         FROM audit_log WHERE request_id = $1
         ORDER BY created_at ASC",
        &[request_id],
    )?;
    Ok(rows
        .into_iter()
        .map(|r| AuditEvent {
            id: r.get("id"),
            request_id: r.get("request_id"),
            event_type: r.get("event_type"),
            actor_email: r.get("actor_email"),
            details: r.get("details"),
            created_at: r.get("created_at"),
        })
        .collect())
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
    fn test_append_and_list_audit_events() {
        let pool = match test_pool() {
            Some(p) => p,
            None => return,
        };
        let hash = format!("test-audit-{}", Uuid::new_v4());
        let mut client = pool.get().unwrap();
        let req_row = client
            .query_one(
                "INSERT INTO requests (query_text, query_hash, target_kind, target_db, target_topology, requester_email)
                 VALUES ('SELECT 1', $1, 'postgres', 'testdb', 'primary', 'a@b.com')
                 RETURNING id",
                &[&hash],
            )
            .unwrap();
        let req_id: Uuid = req_row.get("id");

        let details = serde_json::json!({"rows": 5, "duration_ms": 42});
        let id = append_audit_event(&pool, Some(&req_id), "previewed", "system", Some(&details)).unwrap();

        let events = list_audit_events(&pool, &req_id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "previewed");
        assert_eq!(events[0].id, id);

        client.execute("DELETE FROM requests WHERE id = $1", &[&req_id]).unwrap();
    }

    /// Verify that no update or delete function exists for audit_log.
    /// Patterns are built at runtime to avoid self-matching from the test source.
    #[test]
    fn test_audit_log_has_no_update_or_delete_path() {
        let source = include_str!("audit.rs");
        let update_audit = format!("{} audit_log", "UPDATE");
        let delete_audit = format!("{} audit_log", "DELETE FROM");
        assert!(
            !source.contains(&update_audit) && !source.contains(&delete_audit),
            "audit.rs must not contain UPDATE or DELETE on audit_log"
        );
    }
}
