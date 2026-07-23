//! SQL query validator — first line of defense before a query reaches the
//! preview engine. Rejects anything that isn't a single SELECT (or CTE/WITH
//! opening). This is best-effort string analysis, not a full SQL parser —
//! defense in depth is provided by the read-only preview database role.
//!
//! # Validation rules (ordered for fail-fast):
//!
//! 1. **Empty** — rejected (400)
//! 2. **Overlength** — rejected above `max_len` bytes (400)
//! 3. **Stacked queries** — any `;` that splits into multiple non-empty
//!    statements is rejected (400). String literals containing `;` may
//!    false-positive; the preview role prevents execution either way.
//! 4. **Non-SELECT** — must start with `SELECT` or `WITH` (case-insensitive),
//!    after stripping leading whitespace and block comments. Rejected (400).
//!
//! Returns `Ok(())` if the query passes all checks, or `Err(String)` with
//! a human-readable error fragment.

/// Maximum allowed query length in bytes (UTF-8).
pub const MAX_QUERY_LEN: usize = 32_768; // 32 KiB

/// Validate a SQL query string.
///
/// Returns `Ok(())` if the query looks like a single safe SELECT statement,
/// or `Err(error_fragment)` describing the first violation found.
pub fn validate_query(query: &str, max_len: usize) -> Result<(), String> {
    let trimmed = strip_comments(query.trim()).trim().to_string();

    // 1. Empty check
    if trimmed.is_empty() {
        return Err("query must not be empty".into());
    }

    // 2. Length check (on the original, not trimmed)
    if query.len() > max_len {
        return Err(format!(
            "query exceeds maximum length of {} bytes",
            max_len
        ));
    }

    // 3. Stacked-query check: split on `;`, reject if more than one
    //    non-empty statement.
    let statements: Vec<&str> = query
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if statements.len() > 1 {
        return Err("stacked queries (multiple `;`-separated statements) are not allowed".into());
    }

    // 4. Non-SELECT check: must start with SELECT or WITH (CTE)
    let upper = trimmed.to_uppercase();
    if !upper.starts_with("SELECT") && !upper.starts_with("WITH") {
        return Err("only SELECT queries (or WITH CTEs) are allowed".into());
    }

    Ok(())
}

/// Strip C-style block comments (`/* ... */`, multiline) and line comments
/// (`--` to end of line) from a SQL query, preserving the line/column
/// structure for subsequent prefix matching. This is intentionally
/// stripped-down; an actual SQL parser would handle nested comments and
/// string literals, but the preview role is the real guardrail.
fn strip_comments(sql: &str) -> String {
    let mut result = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Block comment: /* ... */
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            result.push(' '); // preserve token boundary
            continue;
        }
        // Line comment: -- to end of line
        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            result.push(' ');
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_rejected() {
        assert!(validate_query("", MAX_QUERY_LEN).is_err());
        assert!(validate_query("   ", MAX_QUERY_LEN).is_err());
    }

    #[test]
    fn test_overlength_rejected() {
        let long = "SELECT ".repeat(10_000);
        let err = validate_query(&long, 100).unwrap_err();
        assert!(err.contains("exceeds maximum length"));
    }

    #[test]
    fn test_stacked_queries_rejected() {
        let err = validate_query(
            "SELECT * FROM users; DROP TABLE users;",
            MAX_QUERY_LEN,
        )
        .unwrap_err();
        assert!(err.contains("stacked queries"));
    }

    #[test]
    fn test_insert_rejected() {
        let err = validate_query("INSERT INTO users VALUES (1)", MAX_QUERY_LEN).unwrap_err();
        assert!(err.contains("only SELECT"));
    }

    #[test]
    fn test_delete_rejected() {
        let err = validate_query("DELETE FROM users", MAX_QUERY_LEN).unwrap_err();
        assert!(err.contains("only SELECT"));
    }

    #[test]
    fn test_select_accepted() {
        assert!(validate_query("SELECT 1", MAX_QUERY_LEN).is_ok());
        assert!(validate_query("  select * from users  ", MAX_QUERY_LEN).is_ok());
        assert!(validate_query("SELECT * FROM users WHERE id = 42", MAX_QUERY_LEN).is_ok());
    }

    #[test]
    fn test_cte_accepted() {
        assert!(validate_query("WITH cte AS (SELECT 1) SELECT * FROM cte", MAX_QUERY_LEN).is_ok());
    }

    #[test]
    fn test_block_comment_stripped() {
        // "/* comment */SELECT 1" → " SELECT 1" → starts with SELECT
        assert!(validate_query("/* comment */SELECT 1", MAX_QUERY_LEN).is_ok());
    }

    #[test]
    fn test_line_comment_stripped() {
        assert!(validate_query("-- comment\nSELECT 1", MAX_QUERY_LEN).is_ok());
    }
}
