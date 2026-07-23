//! Raw HTTP/1.1 request parsing from a TcpStream. Parses method, path with
//! query string, headers, and body. Rejects chunked transfer-encoding with
//! 501. This is the first stop in the request lifecycle — every incoming
//! connection hits this module before routing.

use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::time::Duration;

/// A parsed HTTP/1.1 request.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub query: HashMap<String, String>,
}

/// Errors that can occur during request parsing.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ParseError {
    ReadTimeout,
    /// Could not split the request line into three whitespace-delimited tokens.
    MalformedRequestLine(String),
    /// HTTP version is not 1.0 or 1.1.
    UnsupportedVersion(String),
    /// Transfer-Encoding: chunked is not supported.
    ChunkedNotSupported,
    /// Declared Content-Length doesn't match actual bytes read.
    ContentLengthMismatch { declared: usize, actual: usize },
    /// Request line > 8KB, headers > 64KB, or body > 1MB.
    TooLarge,
}

/// Parse an HTTP/1.1 request from a TCP stream.
///
/// Sets a 30-second read timeout on the stream before parsing. Reads the
/// request line, headers, and — if Content-Length is present — the body.
/// Rejects chunked transfer-encoding.
///
/// # Errors
///
/// Returns `ParseError` on any malformed input. The stream is left in an
/// undefined state on error; the caller should close the connection.
pub fn parse(stream: &mut TcpStream) -> Result<Request, ParseError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|_| ParseError::ReadTimeout)?;

    let mut reader = BufReader::new(stream);

    // 1. Request line
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|_| ParseError::ReadTimeout)?;
    if n == 0 {
        return Err(ParseError::MalformedRequestLine("empty".into()));
    }
    if line.len() > 8192 {
        return Err(ParseError::TooLarge);
    }
    let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(ParseError::MalformedRequestLine(line.to_string()));
    }
    let method = parts[0].to_uppercase();
    let path_and_query = parts[1];
    let version = parts[2];

    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ParseError::UnsupportedVersion(version.to_string()));
    }

    // 2. Parse query string from path
    let (path, query) = split_query(path_and_query);

    // 3. Headers
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut content_length: Option<usize> = None;
    let mut header_bytes: usize = 0;
    loop {
        let mut hdr_line = String::new();
        let n = reader
            .read_line(&mut hdr_line)
            .map_err(|_| ParseError::ReadTimeout)?;
        if n == 0 {
            break;
        }
        header_bytes += n;
        if header_bytes > 65536 {
            return Err(ParseError::TooLarge);
        }
        let hdr_line = hdr_line.trim_end_matches(|c| c == '\r' || c == '\n');
        if hdr_line.is_empty() {
            break; // end of headers
        }
        if let Some((name, value)) = hdr_line.split_once(':') {
            let name = name.trim().to_lowercase();
            let value = value.trim().to_string();

            match name.as_str() {
                "content-length" => {
                    if content_length.is_some() {
                        return Err(ParseError::MalformedRequestLine(
                            "duplicate Content-Length".into(),
                        ));
                    }
                    content_length = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| ParseError::MalformedRequestLine(
                                hdr_line.to_string(),
                            ))?,
                    );
                }
                "host" => {
                    if headers.contains_key("host") {
                        return Err(ParseError::MalformedRequestLine(
                            "duplicate Host".into(),
                        ));
                    }
                }
                "transfer-encoding" => {
                    if value.to_lowercase().contains("chunked") {
                        return Err(ParseError::ChunkedNotSupported);
                    }
                }
                _ => {}
            }
            headers.insert(name, value);
        }
    }

    // 4. Body
    let body = if let Some(len) = content_length {
        if len > 1_048_576 {
            return Err(ParseError::TooLarge);
        }
        let mut buf = vec![0u8; len];
        // Use the inner reader to read exactly — BufReader may have already
        // consumed bytes past the header boundary, so read from the buffered
        // reader which handles partial consumption correctly.
        reader
            .read_exact(&mut buf)
            .map_err(|_| ParseError::ContentLengthMismatch {
                declared: len,
                actual: 0,
            })?;
        buf
    } else {
        Vec::new()
    };

    Ok(Request {
        method,
        path,
        headers,
        body,
        query,
    })
}

impl Request {
    /// Deserialize the body as JSON. Assumes Content-Type: application/json
    /// has already been checked by the caller if strict enforcement is desired.
    ///
    /// # Errors
    ///
    /// Returns `ParseError::MalformedRequestLine` wrapping the serde error if
    /// deserialization fails.
    pub fn parse_json<T: DeserializeOwned>(&self) -> Result<T, ParseError> {
        serde_json::from_slice(&self.body)
            .map_err(|e| ParseError::MalformedRequestLine(e.to_string()))
    }

    /// Parse the body as `application/x-www-form-urlencoded`.
    ///
    /// Percent-decodes keys and values. Invalid percent-encoding sequences are
    /// silently kept as-is (the `%` is preserved).
    pub fn parse_form(&self) -> HashMap<String, String> {
        let body_str = String::from_utf8_lossy(&self.body);
        parse_query_string(&body_str)
    }
}

/// Split `path?key=val&...` into `(path, query_map)`.
fn split_query(path_and_query: &str) -> (String, HashMap<String, String>) {
    if let Some((path, qs)) = path_and_query.split_once('?') {
        (path.to_string(), parse_query_string(qs))
    } else {
        (path_and_query.to_string(), HashMap::new())
    }
}

/// Parse `key=val&key2=val2` into a HashMap, percent-decoding keys and values.
fn parse_query_string(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if raw.is_empty() {
        return map;
    }
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key = percent_decode(k);
        let val = percent_decode(v);
        map.insert(key, val);
    }
    map
}

/// Minimal percent-decode for URL query strings.
/// Handles `%XX` hex sequences. Invalid sequences are kept as-is.
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes().peekable();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(hi), Some(lo)) = (hi, lo) {
                if let (Some(h), Some(l)) = (hex_val(hi), hex_val(lo)) {
                    result.push((h << 4 | l) as char);
                } else {
                    result.push('%');
                    result.push(hi as char);
                    result.push(lo as char);
                }
            } else {
                result.push('%');
                if let Some(hi) = hi {
                    result.push(hi as char);
                }
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};

    /// Helper: opens a TcpListener, writes raw bytes, returns the stream.
    fn parse_bytes(raw: &[u8]) -> Result<Request, ParseError> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Write raw request in a thread so parse() doesn't deadlock on read.
        let raw_vec = raw.to_vec();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream.write_all(&raw_vec).unwrap();
        });

        let (mut stream, _) = listener.accept().unwrap();
        parse(&mut stream)
    }

    #[test]
    fn test_parse_valid_get() {
        let raw = b"GET /path?key=val HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_bytes(raw).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/path");
        assert_eq!(req.query.get("key"), Some(&"val".to_string()));
        assert_eq!(req.headers.get("host"), Some(&"localhost".to_string()));
        assert!(req.body.is_empty());
    }

    #[test]
    fn test_parse_post_with_body() {
        let body = r#"{"hello":"world"}"#;
        let raw = format!(
            "POST /api HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
            body.len(),
            body
        );
        let req = parse_bytes(raw.as_bytes()).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api");
        assert_eq!(req.body, body.as_bytes());
    }

    #[test]
    fn test_malformed_request_line_no_panic() {
        let raw = b"GARBAGE\r\n\r\n";
        let result = parse_bytes(raw);
        assert!(matches!(result, Err(ParseError::MalformedRequestLine(_))));
    }

    #[test]
    fn test_chunked_rejected() {
        let raw = b"POST /api HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n";
        let result = parse_bytes(raw);
        assert!(matches!(result, Err(ParseError::ChunkedNotSupported)));
    }

    #[test]
    fn test_content_length_mismatch() {
        // Declare 100 bytes but only send 10.
        let raw = b"POST /api HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\nshort";
        let result = parse_bytes(raw);
        assert!(matches!(
            result,
            Err(ParseError::ContentLengthMismatch { .. })
        ));
    }

    #[test]
    fn test_percent_decode_query() {
        let raw =
            b"GET /search?q=hello%20world&page=1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_bytes(raw).unwrap();
        assert_eq!(req.query.get("q"), Some(&"hello world".to_string()));
        assert_eq!(req.query.get("page"), Some(&"1".to_string()));
    }

    #[test]
    fn test_form_body_parsing() {
        let body = b"username=adev&role=admin";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body_vec = body.to_vec();
        let content_length = body_vec.len();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            let raw = format!(
                "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\n",
                content_length
            );
            stream.write_all(raw.as_bytes()).unwrap();
            stream.write_all(&body_vec).unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();
        let req = parse(&mut stream).unwrap();
        let form = req.parse_form();
        assert_eq!(form.get("username"), Some(&"adev".to_string()));
        assert_eq!(form.get("role"), Some(&"admin".to_string()));
    }

    #[test]
    fn test_duplicate_content_length_rejected() {
        let raw = b"POST /api HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhello";
        let result = parse_bytes(raw);
        assert!(matches!(result, Err(ParseError::MalformedRequestLine(_))));
    }

    #[test]
    fn test_http_1_0_accepted() {
        let raw = b"GET / HTTP/1.0\r\n\r\n";
        let req = parse_bytes(raw).unwrap();
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn test_http_2_rejected() {
        let raw = b"GET / HTTP/2.0\r\n\r\n";
        let result = parse_bytes(raw);
        assert!(matches!(result, Err(ParseError::UnsupportedVersion(_))));
    }
}
