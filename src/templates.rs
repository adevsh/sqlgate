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


// --- Submit form fragments ---

/// The query submission form rendered as an HTMX fragment.
/// Posted back to POST /submit via `hx-post`.
///
/// Fields:
/// - `query` — the SQL SELECT statement (textarea, required)
/// - `target_kind` — "postgres" | "mysql" (hidden if only one is supported)
/// - `target_db` — database name (text input, required)
/// - `target_topology` — "primary" | "replica" (toggle)
pub fn submit_form() -> Response {
    let html = r##"<div class="max-w-2xl mx-auto">
    <h2 class="text-xl font-bold text-rust mb-4">Submit Query for Preview</h2>
    <form id="submit-form" hx-post="/submit" hx-target="#content" hx-swap="innerHTML"
          class="space-y-4">
        <div>
            <label for="query" class="block text-sm font-medium text-ink-muted mb-1">
                SQL Query
            </label>
            <textarea id="query" name="query" rows="8" required
                placeholder="SELECT * FROM ..."
                class="w-full border border-parchment-darker rounded px-3 py-2 bg-parchment font-mono text-sm
                       focus:outline-none focus:border-rust focus:ring-1 focus:ring-rust"
                x-data
                x-init="$el.style.height = 'auto'; $el.style.height = $el.scrollHeight + 'px'"
                @input="$el.style.height = 'auto'; $el.style.height = $el.scrollHeight + 'px'"></textarea>
            <p class="text-xs text-ink-muted mt-1">
                Only <code class="bg-parchment-darker px-1 rounded">SELECT</code> queries (or
                <code class="bg-parchment-darker px-1 rounded">WITH</code> CTEs) are allowed.
                Stacked queries (<code class="bg-parchment-darker px-1 rounded">;</code>) are rejected.
            </p>
        </div>
        <div class="grid grid-cols-3 gap-4">
            <div>
                <label for="target_kind" class="block text-sm font-medium text-ink-muted mb-1">
                    Target
                </label>
                <select id="target_kind" name="target_kind" required
                    class="w-full border border-parchment-darker rounded px-3 py-2 bg-parchment text-sm
                           focus:outline-none focus:border-rust focus:ring-1 focus:ring-rust">
                    <option value="postgres">PostgreSQL</option>
                    <option value="mysql">MySQL</option>
                </select>
            </div>
            <div>
                <label for="target_db" class="block text-sm font-medium text-ink-muted mb-1">
                    Database
                </label>
                <input type="text" id="target_db" name="target_db" required
                    placeholder="mydb"
                    class="w-full border border-parchment-darker rounded px-3 py-2 bg-parchment text-sm
                           focus:outline-none focus:border-rust focus:ring-1 focus:ring-rust">
            </div>
            <div>
                <label class="block text-sm font-medium text-ink-muted mb-1">
                    Topology
                </label>
                <div class="flex items-center gap-3 pt-2">
                    <label class="inline-flex items-center gap-1 cursor-pointer">
                        <input type="radio" name="target_topology" value="primary" checked
                            class="accent-rust">
                        <span class="text-sm text-ink">Primary</span>
                    </label>
                    <label class="inline-flex items-center gap-1 cursor-pointer">
                        <input type="radio" name="target_topology" value="replica"
                            class="accent-rust">
                        <span class="text-sm text-ink">Replica</span>
                    </label>
                </div>
            </div>
        </div>
        <div>
            <button type="submit"
                class="bg-rust text-parchment font-medium px-6 py-2 rounded
                       hover:bg-rust/90 active:bg-rust/80 transition-colors">
                Submit for Preview
            </button>
        </div>
    </form>
    <div id="submit-error" class="hidden mt-4 p-3 bg-red-100 border border-red-300 text-red-800 rounded text-sm"></div>
</div>"##;
    Response::ok_html(html.to_string())
}

/// Success message shown after a query is submitted.
pub fn submit_success(request_id: &str) -> Response {
    let html = format!(
        r##"<div class="max-w-2xl mx-auto text-center mt-20">
    <div class="text-5xl mb-4 text-emerald-600">&#10003;</div>
    <h2 class="text-xl font-bold text-ink mb-2">Query Submitted</h2>
    <p class="text-ink-muted mb-4">
        Your query has been submitted for preview. Request ID:
        <code class="bg-parchment-darker px-2 py-0.5 rounded text-sm font-mono">{}</code>
    </p>
    <p class="text-ink-muted text-sm">
        The preview engine will run it against a read-only role and return results shortly.
    </p>
    <a href="/submit" class="inline-block mt-6 text-rust hover:underline text-sm"
       hx-get="/submit" hx-target="#content" hx-swap="innerHTML">
        Submit another query
    </a>
</div>"##,
        request_id
    );
    Response::ok_html(html)
}

/// Error fragment returned inline via HTMX when submission fails validation.
pub fn submit_error(message: &str) -> Response {
    let html = format!(
        r#"<div id="submit-error" class="mt-4 p-3 bg-red-100 border border-red-300 text-red-800 rounded text-sm" hx-swap-oob="true">
    <p>{}</p>
</div>"#,
        message
    );
    Response::ok_html(html)
}


/// Preview results table rendered as an HTMX fragment.
/// Shows column headers, data rows, row count, and topology badge.
pub fn preview_result(
    request_id: &str,
    preview_json: &serde_json::Value,
    row_count: usize,
    duration_ms: u64,
    topology: &str,
) -> Response {
    let columns = preview_json["columns"].as_array().cloned().unwrap_or_default();
    let rows = preview_json["rows"].as_array().cloned().unwrap_or_default();

    let mut html = String::with_capacity(4096);
    html.push_str(concat!(
        r##"<div class="max-w-4xl mx-auto" id="preview-results">
    <div class="flex items-center justify-between mb-4">
        <h2 class="text-xl font-bold text-rust">Query Preview</h2>
        <div class="flex gap-2 text-xs">
            <span class="bg-parchment-darker px-2 py-1 rounded text-ink-muted">"##
    ));
    html.push_str(&format!("{}ms", duration_ms));
    html.push_str(concat!(r##"</span>
            <span class="bg-parchment-darker px-2 py-1 rounded text-ink-muted">"##));
    html.push_str(&format!("{} row{}", row_count, if row_count == 1 { "" } else { "s" }));
    html.push_str(concat!(r##"</span>
            <span class="bg-parchment-darker px-2 py-1 rounded text-ink-muted">"##));
    html.push_str(topology);
    html.push_str(concat!(r##"</span>
        </div>
    </div>
    <div class="overflow-x-auto border border-parchment-darker rounded">
        <table class="w-full text-sm">
            <thead>
                <tr class="bg-parchment-dark">"##));
    for col in &columns {
        html.push_str(&format!(
            r##"<th class="px-3 py-2 text-left font-medium text-ink-muted border-b border-parchment-darker">{}</th>"##,
            col.as_str().unwrap_or("?")
        ));
    }
    html.push_str(concat!(r##"</tr>
            </thead>
            <tbody>"##));
    for row in &rows {
        html.push_str(concat!(r##"<tr class="border-b border-parchment-darker hover:bg-parchment-dark/50">"##));
        for cell in row.as_array().unwrap_or(&vec![]) {
            let val = match cell {
                serde_json::Value::Null => r##"<span class="text-ink-muted italic">NULL</span>"##.to_string(),
                serde_json::Value::String(s) => html_escape(s),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => serde_json::to_string(cell).unwrap_or_default(),
            };
            html.push_str(&format!(r##"<td class="px-3 py-1.5 text-ink font-mono text-xs">{}</td>"##, val));
        }
        html.push_str(concat!(r##"</tr>"##));
    }
    html.push_str(concat!(r##"</tbody>
        </table>
    </div>
    <div class="mt-4 flex gap-3">
        <button class="bg-emerald-700 text-parchment px-4 py-2 rounded text-sm font-medium hover:bg-emerald-800"
                hx-post="/approve" hx-vals='{"request_id": ""##));
    html.push_str(request_id);
    html.push_str(concat!(r##""}' hx-target="#content">
            Request Approval
        </button>
        <button class="border border-rust text-rust px-4 py-2 rounded text-sm font-medium hover:bg-rust hover:text-parchment"
                hx-post="/reject" hx-vals='{"request_id": ""##));
    html.push_str(request_id);
    html.push_str(concat!(r##""}' hx-target="#content">
            Reject
        </button>
        <a href="/submit" class="inline-block text-ink-muted hover:text-ink text-sm py-2"
           hx-get="/submit" hx-target="#content" hx-swap="innerHTML">
            Submit another query
        </a>
    </div>
</div>"##));

    Response::ok_html(html)
}

/// Minimal HTML escape for cell values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
