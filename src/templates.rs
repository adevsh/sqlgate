//! Template rendering: layout shell, fragment detection, and OOB swap
//! conventions. No external template crate — layout is embedded as a const.
//!
//! # Fragment detection
//!
//! When an HTMX request carries the `HX-Request: true` header, the response
//! returns only the requested content fragment (partial). On a direct browser
//! GET, the full layout shell wraps the content. This mirrors the pattern
//! common in htmx-adminlte-ref.
//!
//! # OOB swap targets
//!
//! Multiple UI elements can update from a single HTMX response using
//! `hx-swap-oob` attributes. The following DOM IDs are reserved:
//!
//! | ID                  | Purpose                             |
//! |---------------------|-------------------------------------|
//! | `#content`          | Main content area (full-page swap)  |
//! | `#status-badge`     | Query status indicator badge        |
//! | `#approval-count`   | Pending-approval count badge        |
//! | `#flash-message`    | Ephemeral success/error messages    |
//!
//! # Alpine.js scope
//!
//! Alpine.js is reserved for client-side-only interactions: textarea
//! enhancements (auto-resize, character count), copy-to-clipboard buttons,
//! collapsible panels, and confirm dialogs. Nothing requiring a server
//! round trip — those use HTMX.

use crate::http::request::Request;
use crate::http::response::Response;

/// The full HTML layout shell with Tailwind + HTMX + Alpine wired in.
/// `{{title}}` and `{{content}}` are replaced at render time.
const LAYOUT: &str = r#"<!DOCTYPE html>
<html lang="en" class="bg-parchment text-ink">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{title}} — sqlgate</title>
    <link rel="stylesheet" href="/static/tailwind.css">
    <script src="/static/htmx.min.js"></script>
    <script defer src="/static/alpine.min.js"></script>
</head>
<body hx-boost="true" class="min-h-screen flex flex-col bg-parchment">
    <nav class="bg-parchment-dark border-b border-parchment-darker px-6 py-3 flex items-center gap-6">
        <a href="/" class="text-rust font-bold text-lg no-underline">sqlgate</a>
        <div class="flex gap-4 text-sm">
            <a href="/submit" class="text-ink-muted hover:text-rust no-underline">Submit</a>
            <a href="/approvals" class="text-ink-muted hover:text-rust no-underline">
                Approvals
                <span id="approval-count" class="hidden ml-1 bg-rust text-parchment text-xs rounded-full px-1.5 py-0.5"></span>
            </a>
            <a href="/history" class="text-ink-muted hover:text-rust no-underline">History</a>
        </div>
        <div class="ml-auto text-xs text-ink-muted" id="status-badge"></div>
    </nav>
    <main id="content" class="flex-1 p-6">
{{content}}
    </main>
    <div id="flash-message" class="fixed top-4 right-4 z-50"></div>
</body>
</html>"#;

/// Render a full HTML page or a fragment, depending on whether the request
/// came from HTMX.
///
/// - If `HX-Request` header is present → returns only `content` as HTML,
///   status 200, no layout wrapping.
/// - Otherwise → wraps `content` in the full layout shell with `title`.
pub fn render_page(req: &Request, content: &str, title: &str) -> Response {
    let is_htmx = req
        .headers
        .get("hx-request")
        .map(|v| v == "true")
        .unwrap_or(false);

    if is_htmx {
        Response::ok_html(content.to_string())
    } else {
        let full = LAYOUT
            .replace("{{title}}", title)
            .replace("{{content}}", content);
        Response::ok_html(full)
    }
}

/// Build an OOB (out-of-band) swap fragment. HTMX will use the `hx-swap-oob`
/// attribute to target a specific DOM element without replacing the element
/// that triggered the request.
///
/// Example: `oob_swap("status-badge", "<span>approved</span>")` produces
/// `<span id="status-badge" hx-swap-oob="true">approved</span>`.
#[allow(dead_code)]
pub fn oob_swap(id: &str, inner_html: &str) -> String {
    format!(r#"<div id="{id}" hx-swap-oob="true">{inner_html}</div>"#)
}

/// Status badge CSS classes keyed by query status.
#[allow(dead_code)]
pub fn status_badge_class(status: &str) -> &'static str {
    match status {
        "submitted" => "bg-amber/20 text-amber px-2 py-0.5 rounded text-xs font-medium",
        "previewed" => "bg-blue-100 text-blue-800 px-2 py-0.5 rounded text-xs font-medium",
        "pending_approval" => "bg-amber/20 text-amber px-2 py-0.5 rounded text-xs font-medium",
        "approved" => "bg-emerald-100 text-emerald-800 px-2 py-0.5 rounded text-xs font-medium",
        "rejected" => "bg-red-100 text-red-800 px-2 py-0.5 rounded text-xs font-medium",
        "executed" => "bg-purple-100 text-purple-800 px-2 py-0.5 rounded text-xs font-medium",
        "expired" => "bg-gray-200 text-gray-600 px-2 py-0.5 rounded text-xs font-medium",
        _ => "bg-gray-100 text-gray-600 px-2 py-0.5 rounded text-xs font-medium",
    }
}

/// Render a status badge `<span>` element.
#[allow(dead_code)]
pub fn status_badge(status: &str) -> String {
    let label = match status {
        "submitted" => "submitted",
        "previewed" => "previewed",
        "pending_approval" => "pending",
        "approved" => "approved",
        "rejected" => "rejected",
        "executed" => "executed",
        "expired" => "expired",
        _ => status,
    };
    format!(
        r#"<span class="{}">{}</span>"#,
        status_badge_class(status),
        label
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_req(hx_request: Option<&str>) -> Request {
        let mut headers = HashMap::new();
        if let Some(v) = hx_request {
            headers.insert("hx-request".into(), v.into());
        }
        Request {
            method: "GET".into(),
            path: "/".into(),
            headers,
            body: Vec::new(),
            query: HashMap::new(),
            authenticated_user: None,
        }
    }

    #[test]
    fn test_full_page_when_no_hx_request() {
        let req = make_req(None);
        let resp = render_page(&req, "<p>hello</p>", "Test");
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("<!DOCTYPE html>"));
        assert!(body.contains("<p>hello</p>"));
        assert!(body.contains("<title>Test — sqlgate</title>"));
    }

    #[test]
    fn test_fragment_when_hx_request() {
        let req = make_req(Some("true"));
        let resp = render_page(&req, "<p>fragment</p>", "Test");
        let body = String::from_utf8(resp.body).unwrap();
        assert!(!body.contains("<!DOCTYPE html>"));
        assert_eq!(body, "<p>fragment</p>");
    }

    #[test]
    fn test_oob_swap_format() {
        let html = oob_swap("status-badge", "<span>ok</span>");
        assert!(html.contains(r#"id="status-badge""#));
        assert!(html.contains(r#"hx-swap-oob="true""#));
        assert!(html.contains("<span>ok</span>"));
    }

    #[test]
    fn test_status_badge_all_variants() {
        for status in &[
            "submitted",
            "previewed",
            "pending_approval",
            "approved",
            "rejected",
            "executed",
            "expired",
            "unknown",
        ] {
            let badge = status_badge(status);
            assert!(!badge.is_empty());
            assert!(badge.contains("class="));
        }
    }
}
