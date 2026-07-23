//! Target database abstraction. Each target kind (Postgres, MySQL) has a
//! module that knows how to connect and run queries with the correct role
//! (preview = read-only, execute = read-write).
//!
//! Connection strings come from environment variables:
//! `TARGET_<dbname>_PREVIEW` and `TARGET_<dbname>_EXECUTE`.

pub mod postgres_target;

/// Resolve a target connection URL from environment variables.
///
/// Tries keys in this order (case-sensitive on Unix):
/// 1. `TARGET_{UPPER_DB}_{ROLE}`  (e.g. `TARGET_SAMPLE_PG_PREVIEW`)
/// 2. `TARGET_{original_db}_{ROLE}` (e.g. `TARGET_sample_pg_PREVIEW`)
///
/// Returns `None` if no matching env var is set.
pub fn resolve_target_url(db_name: &str, role: &str) -> Option<String> {
    let upper = db_name.to_uppercase();

    for candidate in &[
        format!("TARGET_{upper}_{role}"),
        format!("TARGET_{db_name}_{role}"),
    ] {
        if let Ok(url) = std::env::var(candidate) {
            let url = url.trim().to_string();
            if !url.is_empty() {
                return Some(url);
            }
        }
    }
    None
}
