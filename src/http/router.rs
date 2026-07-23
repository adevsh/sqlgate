//! Method + path router with colon-prefix path parameter support. Routes HTTP
//! requests to handler functions by matching method and path pattern. Path
//! params like `:id` are extracted and passed to handlers via the Request's
//! params map.

use crate::http::request::Request;
use crate::http::response::Response;
use std::collections::HashMap;

/// An HTTP method supported by the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
}

/// A route handler: receives the parsed request and returns a response.
pub type Handler = fn(&Request) -> Response;

/// A simple method + path router with colon-prefix path parameters.
pub struct Router {
    routes: Vec<(Method, String, Handler)>,
}

impl Router {
    /// Create an empty router.
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Register a handler for `method` at the given path `pattern`.
    ///
    /// Patterns use colon-prefix segments for path parameters:
    /// `/requests/:id`. Literal segments must match exactly. Parameter names
    /// must be unique within a single pattern — duplicate names panic
    /// (programmer error).
    ///
    /// # Panics
    ///
    /// Panics if `pattern` contains a duplicate parameter name (e.g.
    /// `/a/:x/b/:x`).
    pub fn add(&mut self, method: Method, pattern: &str, handler: Handler) {
        // Validate: no duplicate param names in the pattern.
        let mut seen = HashMap::new();
        for seg in pattern.split('/').filter(|s| s.starts_with(':')) {
            if seen.contains_key(seg) {
                panic!("duplicate param name '{}' in pattern '{}'", seg, pattern);
            }
            seen.insert(seg, true);
        }
        self.routes.push((method, pattern.to_string(), handler));
    }

    /// Match a method and path against the registered routes.
    ///
    /// Returns `Some((handler, params))` on the first match, or `None` if no
    /// route matches. Routes are tested in insertion order; the first match
    /// wins.
    pub fn route(
        &self,
        method_str: &str,
        path: &str,
    ) -> Option<(Handler, HashMap<String, String>)> {
        let method = match method_str.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            _ => return None,
        };

        for (m, pattern, handler) in &self.routes {
            if *m != method {
                continue;
            }
            if let Some(params) = match_path(pattern, path) {
                return Some((*handler, params));
            }
        }
        None
    }
}

/// Try to match a pattern like `/a/:id/b` against a concrete path like `/a/42/b`.
/// Returns `Some(params)` on match, `None` on mismatch.
fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    // Treat trailing slash as a distinct path: "/health" != "/health/".
    if pattern.ends_with('/') != path.ends_with('/') {
        return None;
    }

    let pat_segs: Vec<&str> = pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if pat_segs.len() != path_segs.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pseg, aseg) in pat_segs.iter().zip(path_segs.iter()) {
        if let Some(name) = pseg.strip_prefix(':') {
            params.insert(name.to_string(), aseg.to_string());
        } else if pseg != aseg {
            return None;
        }
    }
    Some(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_handler(_req: &Request) -> Response {
        Response::ok_text("OK".into())
    }

    fn get_request(_req: &Request) -> Response {
        Response::ok_json(r#"{"id":1}"#.into())
    }

    #[test]
    fn test_exact_match() {
        let mut router = Router::new();
        router.add(Method::GET, "/health", health_handler);
        let result = router.route("GET", "/health");
        assert!(result.is_some());
        let (_, params) = result.unwrap();
        assert!(params.is_empty());
    }

    #[test]
    fn test_path_param() {
        let mut router = Router::new();
        router.add(Method::GET, "/requests/:id", get_request);
        let result = router.route("GET", "/requests/42");
        assert!(result.is_some());
        let (_, params) = result.unwrap();
        assert_eq!(params.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn test_multi_segment_param() {
        let mut router = Router::new();
        router.add(Method::GET, "/db/:name/table/:tbl", get_request);
        let result = router.route("GET", "/db/mydb/table/users");
        assert!(result.is_some());
        let (_, params) = result.unwrap();
        assert_eq!(params.get("name"), Some(&"mydb".to_string()));
        assert_eq!(params.get("tbl"), Some(&"users".to_string()));
    }

    #[test]
    fn test_no_match_404() {
        let mut router = Router::new();
        router.add(Method::GET, "/health", health_handler);
        let result = router.route("GET", "/nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_method_mismatch() {
        let mut router = Router::new();
        router.add(Method::GET, "/health", health_handler);
        let result = router.route("POST", "/health");
        assert!(result.is_none());
    }

    #[test]
    fn test_trailing_slash_mismatch() {
        let mut router = Router::new();
        router.add(Method::GET, "/health", health_handler);
        // "/health/" has an empty segment after splitting, so segment count
        // differs — should not match.
        let result = router.route("GET", "/health/");
        assert!(result.is_none());
    }

    #[test]
    #[should_panic(expected = "duplicate param")]
    fn test_duplicate_param_panics() {
        let mut router = Router::new();
        router.add(Method::GET, "/a/:x/b/:x", get_request);
    }

    #[test]
    fn test_root_path() {
        let mut router = Router::new();
        router.add(Method::GET, "/", health_handler);
        let result = router.route("GET", "/");
        assert!(result.is_some());
    }
}
