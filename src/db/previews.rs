//! Preview persistence: store serialized query results.

use crate::db::DbResult;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use serde_json::Value;
use uuid::Uuid;

/// Insert a preview result for a request.
pub fn insert_preview(
    pool: &Pool<PostgresConnectionManager<postgres::NoTls>>,
    request_id: &Uuid,
    preview_json: &Value,
    row_count: usize,
    duration_ms: i32,
) -> DbResult<Uuid> {
    let mut client = pool.get()?;
    let row = client.query_one(
        "INSERT INTO previews (request_id, preview_json, row_count, duration_ms)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
        &[&request_id, &preview_json, &(row_count as i32), &duration_ms],
    )?;
    Ok(row.get("id"))
}
