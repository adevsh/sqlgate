//! Static file handler for `/static/*` requests. Serves CSS, JS, and other
//! assets from the `static/` directory with correct Content-Type headers,
//! basic caching, and path-traversal protection.

use crate::http::response::Response;

/// MIME type lookup by file extension.
fn mime_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Serve a static file. `file_path` is the path portion after `/static/`
/// (e.g. `css/style.css`). Rejects `..` for path-traversal protection.
/// Sets `Cache-Control: public, max-age=3600`.
pub fn serve(file_path: &str) -> Response {
    // Reject path traversal attempts.
    if file_path.contains("..") || file_path.starts_with('/') || file_path.contains("\\") {
        return Response::bad_request("invalid path");
    }

    let disk_path = std::path::Path::new("static").join(file_path);

    // Only serve files, not directories.
    if !disk_path.is_file() {
        return Response::not_found();
    }

    match std::fs::read(&disk_path) {
        Ok(body) => Response {
            status: 200,
            headers: vec![
                ("Content-Type".into(), mime_type(file_path).into()),
                ("Cache-Control".into(), "public, max-age=3600".into()),
            ],
            body,
        },
        Err(_) => Response::not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_path_traversal_rejected() {
        let resp = serve("../../etc/passwd");
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn test_path_traversal_double_dot_in_middle() {
        let resp = serve("css/../../etc/passwd");
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn test_nonexistent_file_returns_404() {
        let resp = serve("nonexistent.css");
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn test_serves_existing_file() {
        // Create a temp file in static/ for the test.
        let path = std::path::Path::new("static/test_serve.txt");
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"hello static").unwrap();

        let resp = serve("test_serve.txt");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello static");
        let content_type = resp
            .headers
            .iter()
            .find(|(k, _)| k == "Content-Type")
            .map(|(_, v)| v.as_str());
        assert_eq!(content_type, Some("text/plain; charset=utf-8"));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_cache_control_header_present() {
        let path = std::path::Path::new("static/test_cache.txt");
        std::fs::File::create(path).unwrap();

        let resp = serve("test_cache.txt");
        let cc = resp
            .headers
            .iter()
            .find(|(k, _)| k == "Cache-Control")
            .map(|(_, v)| v.as_str());
        assert_eq!(cc, Some("public, max-age=3600"));

        std::fs::remove_file(path).unwrap();
    }
}
