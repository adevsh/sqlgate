//! Hash verification: re-hash stored query text and compare against
//! the stored hash using a constant-time comparison.

use sha2::{Digest, Sha256};

/// Recompute the SHA-256 hash of `query_text` and compare it against
/// `stored_hash` (hex-encoded) using constant-time comparison.
pub fn verify_query_hash(query_text: &str, stored_hash: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(query_text.as_bytes());
    let actual = format!("{:x}", hasher.finalize());
    constant_time_eq(actual.as_bytes(), stored_hash.as_bytes())
}

/// Constant-time byte comparison: no early exit on mismatch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_match() {
        let query = "SELECT 1";
        let mut hasher = Sha256::new();
        hasher.update(query.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        assert!(verify_query_hash(query, &hash));
    }

    #[test]
    fn test_hash_mismatch() {
        assert!(!verify_query_hash("SELECT 1", "deadbeef"));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
