//! Postgres target: connects with the preview role, runs a wrapped query
//! with a statement timeout, and returns rows as `serde_json::Value` arrays.
//!
//! No connection pooling — each preview is a one-off connection. The LIMIT 5
//! wrap and 5-second timeout keep it cheap.

use serde_json::Value;
use std::time::Instant;

/// Result of a preview query execution.
#[derive(Debug)]
pub struct PreviewResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    pub duration_ms: u64,
    pub topology_used: String,
}

/// Run a preview query against a Postgres target.
///
/// Opens a fresh connection (no pooling), sets `statement_timeout`,
/// runs the already-wrapped query, and collects all rows.
///
/// # Errors
///
/// Returns `Err(String)` if the connection fails, the query times out,
/// or the preview role lacks permission (defense in depth).
pub fn run_preview(url: &str, wrapped_query: &str, topology: &str) -> Result<PreviewResult, String> {
    let start = Instant::now();

    let mut client = postgres::Client::connect(url, postgres::NoTls)
        .map_err(|e| format!("failed to connect to target: {e}"))?;

    // Set a per-session statement timeout. This catches accidental
    // long-running queries even if the LIMIT 5 wrap is somehow bypassed.
    client
        .batch_execute("SET statement_timeout = '5s'")
        .map_err(|e| format!("failed to set timeout: {e}"))?;

    let rows = client
        .query(wrapped_query, &[])
        .map_err(|e| {
            let msg = e.to_string();
            let db_err = e.as_db_error();
            let code = db_err.map(|d| d.code().code());

            // 57014 = query_canceled (statement_timeout or user cancel)
            if code == Some("57014") || msg.contains("timeout") || msg.contains("cancel") {
                "query timed out after 5 seconds".to_string()
            } else if code == Some("42501") {
                "preview role lacks permission for this query (defense in depth)".to_string()
            } else {
                // Include the server message for better diagnostics.
                let detail = db_err.map(|d| d.message().to_string())
                    .filter(|m| !m.is_empty());
                match detail {
                    Some(m) => format!("query failed: {m}"),
                    None => format!("query failed: {msg}"),
                }
            }
        })?;

    // Extract column names.
    let columns: Vec<String> = if !rows.is_empty() {
        rows[0].columns().iter().map(|c| c.name().to_string()).collect()
    } else {
        Vec::new()
    };

    // Serialize each row as Vec<Value> — `try_get::<_, Value>` only works
    // for json/jsonb columns, so match column type OID explicitly.
    let values: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| {
            let mut vals = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let col = &row.columns()[i];
                let val: Value = match col.type_().name() {
                    "int2" | "int4" | "int8" => row.try_get::<_, i64>(i)
                        .map(Value::from).unwrap_or(Value::Null),
                    "float4" | "float8" => row.try_get::<_, f64>(i)
                        .map(|f| serde_json::json!(f)).unwrap_or(Value::Null),
                    "bool" => row.try_get::<_, bool>(i)
                        .map(Value::from).unwrap_or(Value::Null),
                    "numeric" => row.try_get::<_, String>(i)
                        .map(Value::from).unwrap_or(Value::Null),
                    _ => row.try_get::<_, String>(i)
                        .map(Value::from).unwrap_or(Value::Null),
                };
                vals.push(val);
            }
            vals
        })
        .collect();

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(PreviewResult {
        columns,
        rows: values,
        row_count: rows.len(),
        duration_ms,
        topology_used: topology.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview_url() -> Option<String> {
        match std::env::var("TARGET_sample_pg_PREVIEW") {
            Ok(u) if !u.is_empty() => Some(u),
            _ => None,
        }
    }

    #[test]
    fn test_run_preview_selects_users() {
        let url = match preview_url() {
            Some(u) => u,
            None => {
                eprintln!("skipping: TARGET_sample_pg_PREVIEW not set");
                return;
            }
        };

        let result = run_preview(
            &url,
            "SELECT * FROM (SELECT id, name, email FROM users) sub LIMIT 5",
            "primary",
        )
        .expect("preview should succeed");

        assert_eq!(result.columns, vec!["id", "name", "email"]);
        assert_eq!(result.topology_used, "primary");
    }

    /// Verify sqlgate_preview cannot execute DDL inside a subquery.
    #[test]
    fn test_preview_role_cannot_drop_table() {
        let url = match preview_url() {
            Some(u) => u,
            None => return,
        };

        let err = run_preview(
            &url,
            "SELECT * FROM (DROP TABLE users) sub LIMIT 5",
            "primary",
        )
        .unwrap_err();

        assert!(
            err.contains("syntax") || err.contains("permission") || err.contains("lacks"),
            "expected syntax or permission error, got: {err}"
        );
    }

    /// Verify statement_timeout cuts off slow queries.
    #[test]
    fn test_query_timeout() {
        let url = match preview_url() {
            Some(u) => u,
            None => return,
        };

        let err = run_preview(
            &url,
            "SELECT * FROM (SELECT pg_sleep(10)) sub LIMIT 5",
            "primary",
        )
        .unwrap_err();

        assert!(
            err.contains("timed out") || err.contains("timeout") || err.contains("cancel"),
            "expected timeout error, got: {err}"
        );
    }
}
