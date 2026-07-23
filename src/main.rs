//! sqlgate entry point. Binds a TcpListener, dispatches connections to a
//! thread-per-connection accept loop, routes requests through the http::router,
//! and shuts down gracefully on SIGTERM.
//!
//! Concurrency model: one OS thread per connection. This is the simplest
//! correct model for a small internal tool's expected load. If load testing
//! in Phase 12 shows thread-spawn overhead is a bottleneck, switch to a
//! bounded thread pool.

mod http;
mod static_files;
mod templates;
mod db;
mod execute;
mod targets;
mod preview;
mod auth;

use auth::cf_access;
use http::request;
use http::response::Response;
use http::router::{Method, Router};
use preview::engine;
use preview::validator::validate_query;
use sha2::{Digest, Sha256};
use execute::engine as exec_engine;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;
static DB_POOL: OnceLock<Option<Pool<PostgresConnectionManager<postgres::NoTls>>>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn handle_signal(_signum: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

/// GET /submit — render the query submission form.
fn submit_form_handler(req: &request::Request) -> Response {
    templates::render_page(req, &extract_body(&templates::submit_form()), "Submit Query")
}

/// POST /submit — validate, hash, persist, return success.
///
/// Requires authentication (auth check happens before routing).
fn submit_handler(req: &request::Request) -> Response {
    let user = match &req.authenticated_user {
        Some(u) => u,
        None => return Response::bad_request("authentication required"),
    };

    // Parse form body.
    let form = req.parse_form();
    let query = form.get("query").map(|s| s.as_str()).unwrap_or("");
    let target_kind = form.get("target_kind").map(|s| s.as_str()).unwrap_or("postgres");
    let target_db = form.get("target_db").map(|s| s.as_str()).unwrap_or("");
    let target_topology = form.get("target_topology").map(|s| s.as_str()).unwrap_or("primary");

    // Validate SQL.
    if let Err(msg) = validate_query(query, preview::validator::MAX_QUERY_LEN) {
        return templates::submit_error(&msg);
    }

    // Validate required fields.
    if target_db.is_empty() {
        return templates::submit_error("Database name is required");
    }
    if !["postgres", "mysql"].contains(&target_kind) {
        return templates::submit_error("Invalid target kind");
    }
    if !["primary", "replica"].contains(&target_topology) {
        return templates::submit_error("Invalid target topology");
    }

    // Compute query hash.
    let mut hasher = Sha256::new();
    hasher.update(query.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    // Persist.
    let pool = DB_POOL.get()
        .and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized or database unavailable");
    match db::requests::insert_request(
        pool,
        query,
        &hash,
        target_kind,
        target_db,
        target_topology,
        &user.email,
    ) {
        Ok(request_id) => {
            // Audit log — fire and forget (best-effort).
            let details = serde_json::json!({
                "target_kind": target_kind,
                "target_db": target_db,
                "target_topology": target_topology,
                "query_hash": hash,
            });
            let _ = db::audit::append_audit_event(
                pool,
                Some(&request_id),
                "submitted",
                &user.email,
                Some(&details),
            );

            // Run preview immediately after submission.
            engine::run_preview_pipeline(
                pool,
                &request_id,
                query,
                target_kind,
                target_db,
                target_topology,
                &user.email,
            )
        }
        Err(e) => {
            // If the hash collides (UNIQUE constraint), it's a duplicate
            // submission — treat as validation failure.
            let msg = if let db::DbError::Query(ref pg_err) = e {
                if pg_err.code().map(|c| c.code()) == Some("23505") {
                    "This exact query has already been submitted".to_string()
                } else {
                    format!("Database error: {}", e)
                }
            } else {
                format!("Database error: {}", e)
            };
            templates::submit_error(&msg)
        }
    }
}


/// Extract the HTML body string from a Response for use with render_page.
fn extract_body(resp: &Response) -> String {
    String::from_utf8_lossy(&resp.body).into_owned()
}

// --- Approval workflow handlers ---

/// GET /approvals — list pending_approval requests.
fn approvals_list_handler(req: &request::Request) -> Response {
    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");

    // Lazy-expire stale approvals first.
    let _ = db::approvals::expire_stale_approvals(pool);

    match db::requests::list_requests(pool, Some("pending_approval"), 50, 0) {
        Ok(requests) => templates::render_page(
            req,
            &extract_body(&templates::approvals_list(&requests)),
            "Pending Approvals",
        ),
        Err(e) => templates::submit_error(&format!("Failed to load approvals: {e}")),
    }
}

/// POST /request-approval — transition previewed → pending_approval.
fn request_approval_handler(req: &request::Request) -> Response {
    let user = match &req.authenticated_user {
        Some(u) => u,
        None => return Response::bad_request("authentication required"),
    };

    let form = req.parse_form();
    let request_id_str = form.get("request_id").map(|s| s.as_str()).unwrap_or("");
    let request_id = match uuid::Uuid::parse_str(request_id_str) {
        Ok(id) => id,
        Err(_) => return templates::submit_error("Invalid request ID"),
    };

    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");

    // Fetch the request.
    let request = match db::requests::get_request(pool, &request_id) {
        Ok(Some(r)) => r,
        Ok(None) => return templates::submit_error("Request not found"),
        Err(e) => return templates::submit_error(&format!("Database error: {e}")),
    };

    // Only previewed requests can request approval.
    if request.status != "previewed" {
        return templates::submit_error(&format!(
            "Cannot request approval for request in status '{}'",
            request.status
        ));
    }

    // Update status.
    let _ = db::requests::update_status(pool, &request_id, "pending_approval");

    // Audit log.
    let _ = db::audit::append_audit_event(
        pool,
        Some(&request_id),
        "requested_approval",
        &user.email,
        None,
    );

    templates::approval_requested(&request_id.to_string())
}

/// POST /approve — transition pending_approval → approved.
fn approve_handler(req: &request::Request) -> Response {
    let user = match &req.authenticated_user {
        Some(u) => u,
        None => return Response::bad_request("authentication required"),
    };

    let form = req.parse_form();
    let request_id_str = form.get("request_id").map(|s| s.as_str()).unwrap_or("");
    let request_id = match uuid::Uuid::parse_str(request_id_str) {
        Ok(id) => id,
        Err(_) => return templates::submit_error("Invalid request ID"),
    };

    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");

    // Lazy-expire stale approvals.
    let _ = db::approvals::expire_stale_approvals(pool);

    let request = match db::requests::get_request(pool, &request_id) {
        Ok(Some(r)) => r,
        Ok(None) => return templates::submit_error("Request not found"),
        Err(e) => return templates::submit_error(&format!("Database error: {e}")),
    };

    // Prevent self-approval.
    if request.requester_email == user.email {
        return templates::submit_error("You cannot approve your own request");
    }

    // Only pending_approval requests can be approved.
    if request.status != "pending_approval" {
        return templates::submit_error(&format!(
            "Cannot approve request in status '{}'",
            request.status
        ));
    }

    // Compute TTL.
    let ttl_secs: u64 = std::env::var("APPROVAL_TTL_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900);
    let expires_at = std::time::SystemTime::now()
        .checked_add(std::time::Duration::from_secs(ttl_secs))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    // Insert approval.
    let _ = db::approvals::insert_approval(
        pool,
        &request_id,
        &user.email,
        "approved",
        Some(expires_at),
    );

    // Update status.
    let _ = db::requests::update_status(pool, &request_id, "approved");

    // Audit log.
    let details = serde_json::json!({
        "ttl_seconds": ttl_secs,
        "expires_at": format!("{:?}", expires_at),
    });
    let _ = db::audit::append_audit_event(
        pool,
        Some(&request_id),
        "approved",
        &user.email,
        Some(&details),
    );

    // Return updated approvals list.
    match db::requests::list_requests(pool, Some("pending_approval"), 50, 0) {
        Ok(requests) => templates::approvals_list(&requests),
        Err(e) => templates::submit_error(&format!("Failed to reload: {e}")),
    }
}

/// POST /reject — transition pending_approval → rejected (terminal).
fn reject_handler(req: &request::Request) -> Response {
    let user = match &req.authenticated_user {
        Some(u) => u,
        None => return Response::bad_request("authentication required"),
    };

    let form = req.parse_form();
    let request_id_str = form.get("request_id").map(|s| s.as_str()).unwrap_or("");
    let request_id = match uuid::Uuid::parse_str(request_id_str) {
        Ok(id) => id,
        Err(_) => return templates::submit_error("Invalid request ID"),
    };

    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");

    let request = match db::requests::get_request(pool, &request_id) {
        Ok(Some(r)) => r,
        Ok(None) => return templates::submit_error("Request not found"),
        Err(e) => return templates::submit_error(&format!("Database error: {e}")),
    };

    // Prevent self-rejection (same principle as self-approval).
    if request.requester_email == user.email {
        return templates::submit_error("You cannot reject your own request");
    }

    // Only pending_approval can be rejected.
    if request.status != "pending_approval" {
        return templates::submit_error(&format!(
            "Cannot reject request in status '{}'",
            request.status
        ));
    }

    // Insert rejection (no expiry).
    let _ = db::approvals::insert_approval(pool, &request_id, &user.email, "rejected", None);

    // Update status.
    let _ = db::requests::update_status(pool, &request_id, "rejected");

    // Audit log.
    let _ = db::audit::append_audit_event(
        pool,
        Some(&request_id),
        "rejected",
        &user.email,
        None,
    );

    // Return updated approvals list.
    match db::requests::list_requests(pool, Some("pending_approval"), 50, 0) {
        Ok(requests) => templates::approvals_list(&requests),
        Err(e) => templates::submit_error(&format!("Failed to reload: {e}")),
    }
}
/// POST /execute — execute an approved query against the target.
fn execute_handler(req: &request::Request) -> Response {
    let user = match &req.authenticated_user {
        Some(u) => u,
        None => return Response::bad_request("authentication required"),
    };

    let form = req.parse_form();
    let request_id_str = form.get("request_id").map(|s| s.as_str()).unwrap_or("");
    let request_id = match uuid::Uuid::parse_str(request_id_str) {
        Ok(id) => id,
        Err(_) => return templates::submit_error("Invalid request ID"),
    };

    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");

    // Lazy-expire stale approvals.
    let _ = db::approvals::expire_stale_approvals(pool);

    exec_engine::run_execute_pipeline(pool, &request_id, &user.email)
}


/// Root page — welcome / dashboard.

/// GET /history — list all requests.
fn history_list_handler(req: &request::Request) -> Response {
    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");
    let status_filter = req.query.get("status").map(|s| s.as_str());
    match db::requests::list_requests(pool, status_filter, 100, 0) {
        Ok(requests) => templates::render_page(
            req, &extract_body(&templates::history_list(&requests)), "Query History",
        ),
        Err(e) => templates::submit_error(&format!("Failed: {e}")),
    }
}

/// GET /requests/:id — detail with audit timeline.
fn request_detail_handler(req: &request::Request) -> Response {
    let pool = DB_POOL.get().and_then(|p| p.as_ref())
        .expect("DB_POOL not initialized");
    let id_str = req.params.get("id").map(|s| s.as_str()).unwrap_or("");
    let request_id = match uuid::Uuid::parse_str(id_str) {
        Ok(id) => id,
        Err(_) => return templates::submit_error("Invalid request ID"),
    };
    let request = match db::requests::get_request(pool, &request_id) {
        Ok(Some(r)) => r,
        Ok(None) => return templates::submit_error("Request not found"),
        Err(e) => return templates::submit_error(&format!("DB error: {e}")),
    };
    let events = match db::audit::list_audit_events(pool, &request_id) {
        Ok(evs) => evs,
        Err(e) => return templates::submit_error(&format!("DB error: {e}")),
    };
    templates::render_page(
        req, &extract_body(&templates::request_detail(&request, &events)), "Request Detail",
    )
}

fn root_handler(req: &request::Request) -> Response {
    templates::render_page(
        req,
        r#"<div class="max-w-2xl mx-auto text-center mt-20">
    <h1 class="text-3xl font-bold text-rust mb-4">sqlgate</h1>
    <p class="text-ink-muted mb-8">SQL query preview &amp; approval gateway</p>
    <div class="flex gap-4 justify-center">
        <a href="/submit" class="bg-rust text-parchment px-6 py-2 rounded font-medium hover:bg-rust-dark no-underline">Submit Query</a>
        <a href="/approvals" class="border border-rust text-rust px-6 py-2 rounded font-medium hover:bg-rust hover:text-parchment no-underline">Approve</a>
    </div>
</div>"#,
        "sqlgate",
    )
}

/// Return a 200 OK plain-text response for the health check endpoint.
fn health_handler(_req: &request::Request) -> Response {
    Response::ok_text("ok\n".into())
}

fn main() {
    // Register signal handlers FIRST — before any socket bind, to avoid
    // potential signal-related edge cases on macOS.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handle_signal as *const () as usize;
        if libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut()) != 0 {
            eprintln!("sqlgate: warning: failed to register SIGTERM handler");
        }
        if libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut()) != 0 {
            eprintln!("sqlgate: warning: failed to register SIGINT handler");
        }
    }

    let listen_addr =
        std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    // Initialize database pool (optional — server starts without it).
    let pool = db::connect();
    let has_db = pool.is_some();
    DB_POOL.set(pool).expect("DB_POOL already set");

    // Build the router with registered routes.
    // Build the router with registered routes.
    let mut router = Router::new();
    router.add(Method::GET, "/", root_handler);
    router.add(Method::GET, "/health", health_handler);
    router.add(Method::GET, "/submit", submit_form_handler);
    router.add(Method::GET, "/approvals", approvals_list_handler);
    router.add(Method::GET, "/history", history_list_handler);
    router.add(Method::GET, "/requests/:id", request_detail_handler);
    if has_db {
        router.add(Method::POST, "/submit", submit_handler);
        router.add(Method::POST, "/request-approval", request_approval_handler);
        router.add(Method::POST, "/approve", approve_handler);
        router.add(Method::POST, "/reject", reject_handler);
        router.add(Method::POST, "/execute", execute_handler);
    }
    let router = Arc::new(router);

    let listener = TcpListener::bind(&listen_addr).unwrap_or_else(|e| {
        eprintln!("sqlgate: failed to bind {}: {}", listen_addr, e);
        std::process::exit(1);
    });
    listener
        .set_nonblocking(true)
        .expect("set_nonblocking on listener");


    eprintln!(
        "sqlgate: listening on {} (pid={})",
        listen_addr,
        std::process::id()
    );

    let threads: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

    // Accept loop.
    while RUNNING.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let router = router.clone();
                let threads = threads.clone();
                let handle = std::thread::spawn(move || {
                    handle_connection(stream, &router);
                });
                // Prune finished threads before pushing.
                if let Ok(mut guard) = threads.lock() {
                    guard.retain(|h| !h.is_finished());
                    guard.push(handle);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection ready; brief sleep to avoid busy-wait.
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                eprintln!("sqlgate: accept error: {}", e);
                break;
            }
        }
    }

    eprintln!("sqlgate: shutting down, waiting for {} connections...", {
        threads.lock().map(|g| g.len()).unwrap_or(0)
    });

    // Join all remaining threads.
    let remaining: Vec<JoinHandle<()>> = threads
        .lock()
        .map(|mut g| g.drain(..).collect())
        .unwrap_or_default();
    for handle in remaining {
        let _ = handle.join();
    }

    eprintln!("sqlgate: shutdown complete");
}

/// Handle a single connection: parse, route, respond, close.
///
/// Catches handler panics via `catch_unwind` and returns 500 instead of
/// crashing the thread. Every connection is closed after one response.
/// ponytail: keep-alive skipped — add when load testing shows connection
/// overhead matters.
fn handle_connection(mut stream: TcpStream, router: &Router) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match request::parse(&mut stream) {
            Ok(mut req) => {
                // --- Auth check ---
                // Public paths (/health, /static/*) bypass authentication.
                // Everything else requires CF Access + tunnel secret headers.
                if !cf_access::is_public_path(&req.path) {
                    let secret_header = std::env::var("CF_TUNNEL_SECRET_HEADER")
                        .unwrap_or_else(|_| "X-CF-Tunnel-Secret".into());
                    let secret_value = std::env::var("CF_TUNNEL_SECRET_VALUE")
                        .unwrap_or_else(|_| String::new());
                    match cf_access::authenticate(&req.headers, &secret_header, &secret_value) {
                        Ok(user) => req.authenticated_user = Some(user),
                        Err(response) => {
                            let _ = response.write(&mut stream);
                            return;
                        }
                    }
                }

                let method = req.method.clone();
                let path = req.path.clone();

                // Route /static/* before the general router.
                let response = if let Some(file_path) = path.strip_prefix("/static/") {
                    if method == "GET" || method == "HEAD" {
                        static_files::serve(file_path)
                    } else {
                        Response::bad_request("method not allowed on static files")
                    }
                } else {
                    match router.route(&method, &path) {
                        Some((handler, params)) => {
                            req.params = params;
                            handler(&req)
                        }
                        None => Response::not_found(),
                    }
                };

                if let Err(e) = response.write(&mut stream) {
                    eprintln!("sqlgate: write error: {}", e);
                }
            }
            Err(e) => {
                let response = match e {
                    request::ParseError::ReadTimeout => {
                        Response::bad_request("request read timeout")
                    }
                    request::ParseError::ChunkedNotSupported => {
                        Response::not_implemented("chunked transfer-encoding not supported")
                    }
                    request::ParseError::UnsupportedVersion(_) => {
                        Response::bad_request("unsupported HTTP version")
                    }
                    request::ParseError::ContentLengthMismatch { .. } => {
                        Response::bad_request("Content-Length mismatch")
                    }
                    request::ParseError::TooLarge => {
                        Response::bad_request("request too large")
                    }
                    _ => Response::bad_request("malformed request"),
                };
                let _ = response.write(&mut stream);
            }
        }
    }));

    if result.is_err() {
        // Handler panicked — try to send 500.
        let _ = Response::internal_error("internal server error").write(&mut stream);
    }

    // Close the connection. Dropping TcpStream does this; explicit shutdown
    // ensures the client sees EOF promptly.
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    /// Integration test: start the server on a random port, send a GET /health,
    /// and verify the response.
    #[test]
    fn test_server_health_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let mut router = Router::new();
        router.add(Method::GET, "/health", health_handler);
        let router = Arc::new(router);

        let server_handle = std::thread::spawn(move || {
            while running_clone.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        handle_connection(stream, &router);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        // Send request.
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();

        let mut reader = BufReader::new(&client);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).unwrap();
        assert!(
            status_line.contains("200 OK"),
            "expected 200 OK, got: {}",
            status_line
        );

        // Read headers until blank line, then body.
        let mut body = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        reader.read_line(&mut body).unwrap();
        assert_eq!(body.trim(), "ok");

        running.store(false, Ordering::SeqCst);
        server_handle.join().unwrap();
    }
}
