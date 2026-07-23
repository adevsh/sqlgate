//! Cloudflare Access identity trust. Extracts and validates the
//! `Cf-Access-Authenticated-User-Email` header from requests proxied through
//! `cloudflared`. A shared-secret tunnel header prevents direct-connection
//! bypass: even a forged CF Access email header is rejected without the
//! tunnel secret.
//!
//! # Auth boundary
//!
//! - `/health` and `/static/*` — **allowed without auth** (read-only, no state
//!   mutation).
//! - Everything else — **requires authenticated identity**.

use crate::http::response::Response;

/// An identity extracted from Cloudflare Access headers.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub email: String,
}

/// Header name set by Cloudflare Access with the authenticated user's email.
const CF_EMAIL_HEADER: &str = "Cf-Access-Authenticated-User-Email";

/// Extract and validate the Cloudflare Access identity from request headers.
///
/// `secret_header` and `secret_value` are the tunnel shared-secret config
/// (read from env vars at the call site). Passed explicitly to avoid env-var
/// races in tests.
pub fn authenticate(
    headers: &std::collections::HashMap<String, String>,
    secret_header: &str,
    secret_value: &str,
) -> Result<AuthenticatedUser, Response> {
    if secret_value.is_empty() {
        return Err(Response::internal_error(
            "server misconfiguration: tunnel secret value not set",
        ));
    }

    let provided_secret = headers.get(&secret_header.to_lowercase());
    if provided_secret != Some(&secret_value.to_string()) {
        return Err(Response {
            status: 401,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: b"unauthorized: missing or invalid tunnel secret".to_vec(),
        });
    }

    let email = headers.get(&CF_EMAIL_HEADER.to_lowercase()).cloned();
    match email {
        Some(e) if !e.is_empty() => Ok(AuthenticatedUser { email: e }),
        _ => Err(Response {
            status: 401,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: b"unauthorized: missing Cf-Access-Authenticated-User-Email".to_vec(),
        }),
    }
}

/// Check whether a path is exempt from authentication.
pub fn is_public_path(path: &str) -> bool {
    path == "/health" || path.starts_with("/static/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SECRET_HEADER: &str = "X-CF-Tunnel-Secret";
    const SECRET_VALUE: &str = "supersecret";

    fn headers_with(email: &str, secret: &str) -> HashMap<String, String> {
        let mut h = HashMap::new();
        h.insert("cf-access-authenticated-user-email".into(), email.into());
        h.insert(SECRET_HEADER.to_lowercase(), secret.into());
        h
    }

    #[test]
    fn test_valid_auth() {
        let headers = headers_with("user@example.com", SECRET_VALUE);
        let user = authenticate(&headers, SECRET_HEADER, SECRET_VALUE).unwrap();
        assert_eq!(user.email, "user@example.com");
    }

    #[test]
    fn test_missing_tunnel_secret_rejected() {
        let headers = headers_with("user@example.com", "wrong");
        let err = authenticate(&headers, SECRET_HEADER, SECRET_VALUE).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn test_forged_email_without_tunnel_secret_rejected() {
        let headers = headers_with("attacker@evil.com", "wrong");
        let err = authenticate(&headers, SECRET_HEADER, SECRET_VALUE).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn test_missing_email_rejected() {
        let mut headers = HashMap::new();
        headers.insert(SECRET_HEADER.to_lowercase(), SECRET_VALUE.into());
        let err = authenticate(&headers, SECRET_HEADER, SECRET_VALUE).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn test_empty_email_rejected() {
        let headers = headers_with("", SECRET_VALUE);
        let err = authenticate(&headers, SECRET_HEADER, SECRET_VALUE).unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn test_no_secret_configured_fails_closed() {
        let headers = headers_with("user@example.com", "anything");
        let err = authenticate(&headers, SECRET_HEADER, "").unwrap_err();
        assert_eq!(err.status, 500);
    }

    #[test]
    fn test_public_paths() {
        assert!(is_public_path("/health"));
        assert!(is_public_path("/static/tailwind.css"));
        assert!(!is_public_path("/submit"));
        assert!(!is_public_path("/"));
    }
}
