//! Preview engine: orchestrates the full preview pipeline — connect, wrap,
//! execute, serialize, persist, render. Called from the submit handler after
//! the request is persisted.

use crate::db;
use crate::targets;
use crate::preview::wrapper;
use crate::templates;
use crate::http::response::Response;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;

/// Run the preview pipeline for a newly submitted request.
///
/// 1. Resolve target connection string from env vars
/// 2. Wrap the query with LIMIT 5
/// 3. Execute against the target (with timeout, preview role)
/// 4. Persist preview result to DB
/// 5. Update request status to 'previewed'
/// 6. Write audit log
/// 7. Return HTMX fragment with results table
pub fn run_preview_pipeline(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &uuid::Uuid,
    query: &str,
    target_kind: &str,
    target_db: &str,
    target_topology: &str,
    email: &str,
) -> Response {
    // 1. Resolve target connection.
    let url = match targets::resolve_target_url(target_db, "PREVIEW") {
        Some(u) => u,
        None => {
            return templates::submit_error(&format!(
                "no preview connection configured for target '{}' (set TARGET_{}_PREVIEW)",
                target_db,
                target_db.to_uppercase()
            ));
        }
    };

    let topology_used = target_topology.to_string();

    // 2. Wrap the query.
    let wrapped = wrapper::wrap_query(query);

    // 3. Execute based on target kind.
    let preview_result = match target_kind {
        "postgres" => {
            crate::targets::postgres_target::run_preview(&url, &wrapped, &topology_used)
        }
        "mysql" => {
            return templates::submit_error("MySQL preview engine not yet implemented");
        }
        _ => return templates::submit_error(&format!("unsupported target kind: {target_kind}")),
    };

    let result = match preview_result {
        Ok(r) => r,
        Err(msg) => return templates::submit_error(&msg),
    };

    // 4. Persist preview.
    let preview_json = serde_json::json!({
        "columns": result.columns,
        "rows": result.rows,
    });

    if let Err(e) = db::previews::insert_preview(
        pool,
        request_id,
        &preview_json,
        result.row_count,
        result.duration_ms as i32,
    ) {
        return templates::submit_error(&format!("failed to persist preview: {e}"));
    }

    // 5. Update request status.
    let _ = db::requests::update_status(pool, request_id, "previewed");

    // 6. Audit log.
    let details = serde_json::json!({
        "row_count": result.row_count,
        "duration_ms": result.duration_ms,
        "topology_used": &topology_used,
        "target_kind": target_kind,
    });
    let _ = db::audit::append_audit_event(
        pool,
        Some(request_id),
        "previewed",
        email,
        Some(&details),
    );

    let elapsed_sec = result.duration_ms;

    // 7. Render fragment.
    templates::preview_result(
        &request_id.to_string(),
        &preview_json,
        result.row_count,
        elapsed_sec,
        &topology_used,
    )
}
