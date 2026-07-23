//! Query wrapper: wraps a user-submitted SQL query with a subquery and
//! LIMIT to enforce row-limited previews. This runs server-side, so even
//! if the user writes `LIMIT 999999`, the outer LIMIT 5 caps the result.

/// Maximum rows returned by any preview.
pub const PREVIEW_LIMIT: u32 = 5;

/// Wrap a user query as `SELECT * FROM (<query>) sub LIMIT <PREVIEW_LIMIT>`.
///
/// Detects and strips a trailing semicolon before wrapping, since the
/// subquery syntax requires no terminator inside the parentheses.
///
/// Examples:
/// - `SELECT 1` → `SELECT * FROM (SELECT 1) sub LIMIT 5`
/// - `SELECT * FROM users WHERE id = 42;` → `SELECT * FROM (SELECT * FROM users WHERE id = 42) sub LIMIT 5`
pub fn wrap_query(query: &str) -> String {
    let cleaned = query.trim().trim_end_matches(';').trim();
    format!("SELECT * FROM ({cleaned}) sub LIMIT {PREVIEW_LIMIT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_wrap() {
        let result = wrap_query("SELECT 1");
        assert_eq!(result, "SELECT * FROM (SELECT 1) sub LIMIT 5");
    }

    #[test]
    fn test_strips_trailing_semicolon() {
        let result = wrap_query("SELECT 1;");
        assert_eq!(result, "SELECT * FROM (SELECT 1) sub LIMIT 5");
    }

    #[test]
    fn test_preserves_user_limit_in_subquery() {
        // The user's LIMIT is inside the subquery — harmless.
        let result = wrap_query("SELECT * FROM users LIMIT 999999");
        assert_eq!(
            result,
            "SELECT * FROM (SELECT * FROM users LIMIT 999999) sub LIMIT 5"
        );
    }

    #[test]
    fn test_with_cte() {
        let result = wrap_query("WITH cte AS (SELECT 1 AS n) SELECT * FROM cte");
        assert_eq!(
            result,
            "SELECT * FROM (WITH cte AS (SELECT 1 AS n) SELECT * FROM cte) sub LIMIT 5"
        );
    }
}
