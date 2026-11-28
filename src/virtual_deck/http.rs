//! Just enough HTTP/1.1 for the virtual deck: one bounded request per
//! connection, fixed responses, `Connection: close`.
use serde_json::Value;
use std::io::{self, Read, Write};

/// Request line plus headers.
pub(super) const MAX_HEAD: usize = 8 * 1024;
const MAX_HEADERS: usize = 64;
/// A login answer is about 200 bytes.
pub(super) const MAX_BODY: usize = 4 * 1024;

/// Longest `Host` value the policy repeats: a DNS name plus a port.
const MAX_HOST: usize = 260;

/// The page's Content-Security-Policy. `connect-src` names `ws://<Host>`
/// as well as 'self', because older Safari does not let 'self' cover ws:.
/// A `Host` that is not a plain host[:port] is left out, so a client can
/// never add directives of its own. `form-action 'none'`: the login never
/// posts a form, so a browser without JavaScript cannot send the password.
pub(super) fn content_security_policy(host: Option<&str>) -> String {
    let connect = match host.filter(|host| is_plain_host(host)) {
        Some(host) => format!("'self' ws://{host}"),
        None => "'self'".to_string(),
    };
    format!(
        "default-src 'self'; img-src 'self' data: blob:; connect-src {connect}; \
         style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; form-action 'none'"
    )
}

/// Letters, digits, '.', '-', ':' and IPv6 brackets only.
fn is_plain_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte))
}

pub(super) struct Request {
    pub method: String,
    /// The path without its query.
    pub path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Every value of the cookie `name`, in order.
    pub fn cookies<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("cookie"))
            .flat_map(|(_, value)| value.split(';'))
            .filter_map(move |pair| {
                let (key, value) = pair.trim().split_once('=')?;
                (key == name).then_some(value)
            })
    }

    /// How the `Origin` header relates to `http://<Host>`.
    pub fn origin(&self) -> Origin {
        let Some(origin) = self.header("origin") else {
            return Origin::Absent;
        };
        let same = self
            .header("host")
            .is_some_and(|host| origin.eq_ignore_ascii_case(&format!("http://{host}")));
        match origin {
            _ if same => Origin::Same,
            "null" => Origin::Null,
            _ => Origin::Foreign,
        }
    }

    /// Whether the request asks for a WebSocket (RFC 6455, version 13).
    pub fn is_websocket_upgrade(&self) -> bool {
        let has_token = |name: &str, token: &str| {
            self.header(name).is_some_and(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case(token))
            })
        };
        has_token("upgrade", "websocket")
            && has_token("connection", "upgrade")
            && self.header("sec-websocket-version") == Some("13")
            && self.header("sec-websocket-key").is_some()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// `http://<Host>`.
    Same,
    /// No `Origin` header: not a browser, or a browser too old to send one.
    Absent,
    /// `Origin: null`: what any page can make its posts send (and what a
    /// no-referrer page's own posts send, which is why the pages' fetch()
    /// calls ask for the same-origin referrer policy).
    Null,
    Foreign,
}

#[derive(Debug)]
pub(super) enum HeadError {
    TooLarge,
    Malformed,
    /// Closed, timed out or failed: no reply.
    Gone,
}

/// Reads one request head; returns it with the bytes read past its end.
pub(super) fn read_head(stream: &mut impl Read) -> Result<(Request, Vec<u8>), HeadError> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let end = loop {
        let read = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(HeadError::Gone),
            Ok(read) => read,
        };
        let searched_from = buffer.len().saturating_sub(3);
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = buffer[searched_from..]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            let end = searched_from + at;
            if end > MAX_HEAD {
                return Err(HeadError::TooLarge);
            }
            break end;
        }
        if buffer.len() > MAX_HEAD {
            return Err(HeadError::TooLarge);
        }
    };
    let leftover = buffer.split_off(end + 4);
    let head = std::str::from_utf8(&buffer[..end]).map_err(|_| HeadError::Malformed)?;
    Ok((parse_head(head)?, leftover))
}

fn parse_head(head: &str) -> Result<Request, HeadError> {
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version), None) = (
        request_line.next(),
        request_line.next(),
        request_line.next(),
        request_line.next(),
    ) else {
        return Err(HeadError::Malformed);
    };
    let is_token = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    };
    if !is_token(method) || !target.starts_with('/') || !matches!(version, "HTTP/1.1" | "HTTP/1.0")
    {
        return Err(HeadError::Malformed);
    }
    let mut headers = Vec::new();
    for line in lines {
        if headers.len() == MAX_HEADERS {
            return Err(HeadError::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(HeadError::Malformed)?;
        if !is_token(name) {
            return Err(HeadError::Malformed);
        }
        headers.push((name.to_string(), value.trim().to_string()));
    }
    let path = target.split(['?', '#']).next().unwrap_or(target);
    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        headers,
    })
}

#[derive(Debug)]
pub(super) enum BodyError {
    LengthRequired,
    TooLarge,
    Gone,
}

/// Reads the `Content-Length` body (no chunked bodies), at most `MAX_BODY` bytes.
pub(super) fn read_body(
    stream: &mut impl Read,
    request: &Request,
    mut leftover: Vec<u8>,
) -> Result<Vec<u8>, BodyError> {
    if request.header("transfer-encoding").is_some() {
        return Err(BodyError::LengthRequired);
    }
    let length = match request.header("content-length") {
        None => 0,
        Some(value) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
            value.parse::<usize>().map_err(|_| BodyError::TooLarge)?
        }
        Some(_) => return Err(BodyError::LengthRequired),
    };
    if length > MAX_BODY {
        return Err(BodyError::TooLarge);
    }
    if leftover.len() >= length {
        leftover.truncate(length);
        return Ok(leftover);
    }
    let mut rest = vec![0u8; length - leftover.len()];
    stream.read_exact(&mut rest).map_err(|_| BodyError::Gone)?;
    leftover.extend_from_slice(&rest);
    Ok(leftover)
}

/// A complete response; every one carries the security headers and closes.
pub(super) struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Reply {
    pub fn html(page: &'static str) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            content_type: "text/html; charset=utf-8",
            body: page.as_bytes().to_vec(),
        }
    }

    pub fn json(status: u16, body: &Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            content_type: "application/json",
            body: body.to_string().into_bytes(),
        }
    }

    /// `{"error": message}`.
    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": message }))
    }

    /// 303 See Other to `/`.
    pub fn home() -> Self {
        Self {
            status: 303,
            headers: vec![("Location", "/".to_string())],
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }

    pub fn with(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Writes the response; `host` is the request's `Host`, when it had one.
    pub fn write_to(&self, stream: &mut impl Write, host: Option<&str>) -> io::Result<()> {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        let policy = content_security_policy(host);
        let fixed = [
            ("Content-Type", self.content_type),
            ("Content-Security-Policy", policy.as_str()),
            ("X-Frame-Options", "DENY"),
            ("Cache-Control", "no-store"),
            ("Referrer-Policy", "no-referrer"),
            ("X-Content-Type-Options", "nosniff"),
            ("Connection", "close"),
        ];
        for (name, value) in fixed {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        stream.write_all(head.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

/// The 101 answer to a WebSocket upgrade.
pub(super) fn write_switching_protocols(stream: &mut impl Write, key: &str) -> io::Result<()> {
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_head_and_keeps_the_bytes_after_it() {
        let raw = b"POST /login/finish?next=1 HTTP/1.1\r\nHost: a\r\nCookie: x=1; sdeck_session=ab\r\n\r\nbody";
        let (request, leftover) = read_head(&mut &raw[..]).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/login/finish");
        assert_eq!(request.header("HOST"), Some("a"));
        assert_eq!(request.cookies("sdeck_session").collect::<Vec<_>>(), ["ab"]);
        assert_eq!(leftover, b"body");
    }

    #[test]
    fn refuses_oversized_and_malformed_heads() {
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert!(matches!(
            read_head(&mut long.as_bytes()),
            Err(HeadError::TooLarge)
        ));
        for raw in [
            "GET / HTTP/2.0\r\n\r\n",
            "GET http://x/ HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nno colon\r\n\r\n",
            "GET  / HTTP/1.1\r\n\r\n",
        ] {
            assert!(
                matches!(read_head(&mut raw.as_bytes()), Err(HeadError::Malformed)),
                "{raw}"
            );
        }
        assert!(matches!(
            read_head(&mut &b"GET / HTTP/1.1\r\n"[..]),
            Err(HeadError::Gone)
        ));
    }

    #[test]
    fn the_policy_names_the_websocket_host_but_never_a_hostile_one() {
        assert_eq!(
            content_security_policy(Some("192.168.1.20:8765")),
            "default-src 'self'; img-src 'self' data: blob:; \
             connect-src 'self' ws://192.168.1.20:8765; style-src 'self' 'unsafe-inline'; \
             script-src 'self' 'unsafe-inline'; form-action 'none'"
        );
        assert!(content_security_policy(Some("[::1]:8765")).contains("ws://[::1]:8765;"));
        for hostile in ["x; script-src *", "x'", "x y", "", "a\"b"] {
            let policy = content_security_policy(Some(hostile));
            assert!(
                policy.contains("connect-src 'self'; "),
                "{hostile}: {policy}"
            );
        }
        assert!(content_security_policy(None).contains("connect-src 'self'; "));
    }

    #[test]
    fn same_origin_means_http_and_the_host() {
        let request = |origin: &str| {
            parse_head(&format!(
                "GET /ws HTTP/1.1\r\nHost: 10.0.0.2:8765\r\nOrigin: {origin}"
            ))
            .unwrap()
        };
        assert_eq!(request("http://10.0.0.2:8765").origin(), Origin::Same);
        assert_eq!(request("HTTP://10.0.0.2:8765").origin(), Origin::Same);
        assert_eq!(request("https://10.0.0.2:8765").origin(), Origin::Foreign);
        assert_eq!(request("http://10.0.0.2").origin(), Origin::Foreign);
        assert_eq!(request("http://evil.example").origin(), Origin::Foreign);
        assert_eq!(request("null").origin(), Origin::Null);
        let absent = parse_head("POST /logout HTTP/1.1\r\nHost: 10.0.0.2:8765").unwrap();
        assert_eq!(absent.origin(), Origin::Absent);
    }
}
