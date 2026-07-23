//! Execution engine: connects with the execute role, runs the query
//! in an explicit transaction, records the result.

use crate::db;
use crate::http::response::Response;
use crate::templates;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::time::Instant;

/// Result of an execution attempt.
struct ExecResult {
    rows_affected: Option<i32>,
    error_message: Option<String>,
    hash_matched: bool,
}

/// Run the execution pipeline for an approved request.
pub fn run_execute_pipeline(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &uuid::Uuid,
    email: &str,
) -> Response {
    // Fetch the request.
    let request = match db::requests::get_request(pool, request_id) {
        Ok(Some(r)) => r,
        Ok(None) => return templates::submit_error("Request not found"),
        Err(e) => return templates::submit_error(&format!("Database error: {e}")),
    };

    // Only approved requests can be executed.
    if request.status != "approved" {
        return templates::submit_error(&format!(
            "Cannot execute request in status '{}'",
            request.status
        ));
    }

    // 1. Re-hash and verify.
    let hash_matched = crate::execute::hash::verify_query_hash(
        &request.query_text,
        &request.query_hash,
    );

    if !hash_matched {
        // Record the failed verification.
        let _ = db::executions::insert_execution(
            pool, request_id, &request.query_hash,
            false, None,
            Some("query hash mismatch — query text may have been tampered"),
        );
        return templates::submit_error(
            "Query hash verification failed — the stored query text does not match the submitted hash. Execution aborted.",
        );
    }

    // 2. Resolve execute connection.
    let url = match crate::targets::resolve_target_url(&request.target_db, "EXECUTE") {
        Some(u) => u,
        None => {
            return templates::submit_error(&format!(
                "no execute connection configured for target '{}'",
                request.target_db
            ));
        }
    };

    let start = Instant::now();

    // 3. Execute against target.
    let exec_result = match request.target_kind.as_str() {
        "postgres" => execute_postgres(&url, &request.query_text),
        "mysql" => {
            return templates::submit_error("MySQL execution not yet implemented");
        }
        _ => return templates::submit_error(&format!("unsupported target: {}", request.target_kind)),
    };

    // 4. Record execution.
    let _ = db::executions::insert_execution(
        pool, request_id, &request.query_hash,
        exec_result.hash_matched,
        exec_result.rows_affected,
        exec_result.error_message.as_deref(),
    );

    // 5. Update status if successful.
    if exec_result.error_message.is_none() {
        let _ = db::requests::update_status(pool, request_id, "executed");
    }

    // 6. Audit log.
    let details = serde_json::json!({
        "hash_matched": exec_result.hash_matched,
        "rows_affected": exec_result.rows_affected,
        "error": exec_result.error_message,
        "duration_ms": start.elapsed().as_millis() as u64,
    });
    let _ = db::audit::append_audit_event(
        pool, Some(request_id),
        if exec_result.error_message.is_some() { "execution_failed" } else { "executed" },
        email, Some(&details),
    );

    match exec_result.error_message {
        Some(msg) => templates::submit_error(&format!("Execution failed: {msg}")),
        None => templates::submit_error("Query executed successfully"),
    }
}

/// Execute a query against a Postgres target in an explicit transaction.
fn execute_postgres(url: &str, query: &str) -> ExecResult {
    let mut client = match postgres::Client::connect(url, postgres::NoTls) {
        Ok(c) => c,
        Err(e) => {
            return ExecResult {
                rows_affected: None,
                error_message: Some(format!("connection failed: {e}")),
                hash_matched: true,
            };
        }
    };

    // Begin transaction.
    if let Err(e) = client.batch_execute("BEGIN") {
        return ExecResult {
            rows_affected: None,
            error_message: Some(format!("BEGIN failed: {e}")),
            hash_matched: true,
        };
    }

    // Execute query.
    let result = client.execute(query, &[]);

    match result {
        Ok(rows) => {
            if let Err(e) = client.batch_execute("COMMIT") {
                return ExecResult {
                    rows_affected: Some(rows as i32),
                    error_message: Some(format!("COMMIT failed: {e}")),
                    hash_matched: true,
                };
            }
            ExecResult {
                rows_affected: Some(rows as i32),
                error_message: None,
                hash_matched: true,
            }
        }
        Err(e) => {
            let _ = client.batch_execute("ROLLBACK");
            ExecResult {
                rows_affected: None,
                error_message: Some(format!("{e}")),
                hash_matched: true,
            }
        }
    }
}
