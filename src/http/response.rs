//! HTTP/1.1 response writer. Writes status line, headers, and body directly
//! to a TcpStream. Provides convenience constructors for common content types
//! and HTMX-specific header helpers.

use std::io::Write;
use std::net::TcpStream;

/// An HTTP/1.1 response ready to be written to a TcpStream.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[allow(dead_code)]
impl Response {
    // -- Convenience constructors --

    /// 200 OK with `Content-Type: text/html; charset=utf-8`.
    pub fn ok_html(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![(
                "Content-Type".into(),
                "text/html; charset=utf-8".into(),
            )],
            body: body.into_bytes(),
        }
    }

    /// 200 OK with `Content-Type: application/json`.
    pub fn ok_json(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into_bytes(),
        }
    }

    /// 200 OK with `Content-Type: text/plain; charset=utf-8`.
    pub fn ok_text(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![(
                "Content-Type".into(),
                "text/plain; charset=utf-8".into(),
            )],
            body: body.into_bytes(),
        }
    }

    /// 404 Not Found, empty body.
    pub fn not_found() -> Self {
        Self {
            status: 404,
            headers: vec![],
            body: Vec::new(),
        }
    }

    /// 500 Internal Server Error with a plain-text message.
    pub fn internal_error(msg: &str) -> Self {
        Self {
            status: 500,
            headers: vec![(
                "Content-Type".into(),
                "text/plain; charset=utf-8".into(),
            )],
            body: msg.as_bytes().to_vec(),
        }
    }

    /// 400 Bad Request with a plain-text message.
    pub fn bad_request(msg: &str) -> Self {
        Self {
            status: 400,
            headers: vec![(
                "Content-Type".into(),
                "text/plain; charset=utf-8".into(),
            )],
            body: msg.as_bytes().to_vec(),
        }
    }

    /// 501 Not Implemented with a plain-text message.
    pub fn not_implemented(msg: &str) -> Self {
        Self {
            status: 501,
            headers: vec![(
                "Content-Type".into(),
                "text/plain; charset=utf-8".into(),
            )],
            body: msg.as_bytes().to_vec(),
        }
    }

    /// 303 See Other redirect.
    pub fn redirect(url: &str) -> Self {
        Self {
            status: 303,
            headers: vec![("Location".into(), url.to_string())],
            body: Vec::new(),
        }
    }

    // -- HTMX header helpers --

    /// Set the `HX-Trigger` response header so the client fires an event.
    pub fn with_hx_trigger(mut self, event: &str) -> Self {
        self.headers
            .push(("HX-Trigger".into(), event.to_string()));
        self
    }

    /// Set the `HX-Redirect` response header for client-side navigation.
    pub fn with_hx_redirect(mut self, url: &str) -> Self {
        self.headers
            .push(("HX-Redirect".into(), url.to_string()));
        self
    }

    /// Set the `HX-Reswap` response header to control swap behavior.
    pub fn with_hx_reswap(mut self, strategy: &str) -> Self {
        self.headers
            .push(("HX-Reswap".into(), strategy.to_string()));
        self
    }

    // -- Generic builder --

    /// Add an arbitrary header to the response.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    // -- Write --

    /// Write the full HTTP response (status line, headers, body) to the
    /// TcpStream. Automatically adds a `Content-Length` header from
    /// `body.len()`.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the stream write fails.
    pub fn write(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let status_text = status_text(self.status);
        let mut buf = format!("HTTP/1.1 {} {}\r\n", self.status, status_text);

        // Headers already set by caller
        for (name, value) in &self.headers {
            buf.push_str(&format!("{}: {}\r\n", name, value));
        }

        // Always add Content-Length
        buf.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        buf.push_str("\r\n");

        stream.write_all(buf.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

/// Map numeric status code to the canonical reason phrase.
fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Unknown",
    }
}
